// score.rs —— 游玩/回放时的逐判定事件（`ScoreProcessor.hitEvents`）
//
// 数据链路（2026.1005.0.0 静态反射 + 真机探针）：
//   Player + <table.score.player_score_processor>  → ScoreProcessor
//   ScoreProcessor + <table.score.hit_events>       → List<HitEvent>
//   List._items + 0x10 + stride*i                   → HitEvent 元素
//     + time_offset (f64) / result (i32) / hit_object (ptr)
//   HitObject + column (i32)                        → mania 轨道号
//
// `HitEvent` 是 **64 字节顺序值类型**（live 实测：TimeOffset@0x10、Result@0x18、
// HitObject@0x00、LastHitObject@0x08；`Marshal.OffsetOf` 是 unmanaged 布局，不能当 managed
// 字段偏移用）；`HitObject + 0x48` 是 `ManiaHitObject.Column`。这些值由 `live` 复核，
// 见 `temp/laneacc-evidence/CONCLUSION.md`；表里的 `player_score_processor` /
// `hit_events` 是生成器 `live` 复核的字段偏移。
//
// 失败一律**不推进游标**：半截读/坏列/坏结果会让下一拍重试，绝不猜列号。

use std::sync::{Condvar, Mutex};
use std::time::Duration;

use serde::Serialize;

use crate::osu::model::Hits;
use crate::osu::offsets::{OffsetTable, ScoreSection};
use super::source::{field_addr, read_u64, is_store_hash, read_f64, read_i32, read_ptr_field, read_string, Source, StringLayout};

/// `List<T>` 的内部布局（.NET x64）。
const LIST_ITEMS: i64 = 0x08;
const LIST_SIZE: i64 = 0x10;
/// .NET 数组布局：长度 `+0x08`、元素 `+0x10`。
const ARRAY_LENGTH: i64 = 0x08;
const ARRAY_DATA: u64 = 0x10;
/// 事件上限（防御坏指针；马拉松 10 万+ 也远小于此）。
const MAX_EVENTS: i32 = 1_000_000;
/// 连续失败多少拍后清空日志并请求页面重置。
const MAX_FAILURES: u32 = 50;

/// 一条原始判定事件；`result` 是 lazer `HitResult` 的原始枚举值。
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct RawHitEvent {
    pub col: i32,
    pub result: i32,
    pub offset: f64,
    /// stable 判定来源的 note 下标（诊断/对齐用；lazer 事件为 None）。
    pub note: Option<i32>,
}

/// 一次增量（或全量）推送。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HitEventBatch {
    pub reset: bool,
    pub session: u64,
    pub offset: usize,
    pub events: Vec<RawHitEvent>,
}

#[derive(Default)]
struct Inner {
    /// 本局全量事件（快照用）。
    events: Vec<RawHitEvent>,
    /// 尚未被发送线程取走的增量。
    pending: Vec<RawHitEvent>,
    /// 下一次 `drain` 必须发 `reset:true` + 全量。
    reset_pending: bool,
    /// 会话代号：每次 `reset` / `clear` 单调自增，供页面识别跨局迟到增量。
    session: u64,
    /// 已通过 `drain` 发出的事件条数，用于给增量打精确 `offset` 实现幂等去重。
    sent_len: usize,
}

/// 壳内的事件日志：读取线程写入，发送线程按 100ms 合并取走。
pub struct HitEventLog {
    inner: Mutex<Inner>,
    cv: Condvar,
}

impl Default for HitEventLog {
    fn default() -> Self {
        Self::new()
    }
}

impl HitEventLog {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner::default()),
            cv: Condvar::new(),
        }
    }

    pub fn push(&self, reset: bool, events: &[RawHitEvent]) {
        let mut inner = self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if reset {
            inner.events.clear();
            inner.pending.clear();
            inner.reset_pending = true;
            inner.session = inner.session.wrapping_add(1);
            inner.sent_len = 0;
        }
        inner.events.extend_from_slice(events);
        inner.pending.extend_from_slice(events);
        self.cv.notify_all();
    }

    /// 清空本局并让下一次 `drain` 发一个空的全量快照。
    pub fn clear(&self) {
        let mut inner = self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        inner.events.clear();
        inner.pending.clear();
        inner.reset_pending = true;
        inner.session = inner.session.wrapping_add(1);
        inner.sent_len = 0;
        self.cv.notify_all();
    }

    /// 取走增量；若有待发的 reset，则返回全量（`reset:true`）。
    pub fn drain(&self) -> HitEventBatch {
        let mut inner = self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        Self::take_pending(&mut inner)
    }

    /// Snapshot and pending broadcast share one log position, even if a poll arrives during delivery.
    pub fn drain_and_snapshot(&self) -> (HitEventBatch, HitEventBatch) {
        let mut inner = self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let snapshot = HitEventBatch {
            reset: true, session: inner.session, offset: 0, events: inner.events.clone(),
        };
        (Self::take_pending(&mut inner), snapshot)
    }

    fn take_pending(inner: &mut Inner) -> HitEventBatch {
        if inner.reset_pending {
            inner.reset_pending = false;
            inner.pending.clear();
            inner.sent_len = inner.events.len();
            HitEventBatch {
                reset: true,
                session: inner.session,
                offset: 0,
                events: inner.events.clone(),
            }
        } else {
            let events = std::mem::take(&mut inner.pending);
            let offset = inner.sent_len;
            inner.sent_len = inner.sent_len.saturating_add(events.len());
            HitEventBatch {
                reset: false,
                session: inner.session,
                offset,
                events,
            }
        }
    }

    /// 当前全量快照（页面刷新/重连用）。
    pub fn snapshot(&self) -> HitEventBatch {
        let inner = self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        HitEventBatch {
            reset: true,
            session: inner.session,
            offset: 0,
            events: inner.events.clone(),
        }
    }

    pub fn len(&self) -> usize {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .events
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 等新数据（最多 `timeout`）；返回是否有待发内容。
    pub fn wait_for_change(&self, timeout: Duration) -> bool {
        let mut inner = self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if inner.pending.is_empty() && !inner.reset_pending {
            let (guard, _) = self
                .cv
                .wait_timeout(inner, timeout)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            inner = guard;
        }
        !inner.pending.is_empty() || inner.reset_pending
    }

    /// 返回当前已收集全量判定事件所对应的 `Hits`。
    pub fn current_hits(&self) -> Hits {
        let inner = self.inner.lock().unwrap();
        hits_from_events(&inner.events)
    }
}

/// 从逐判定事件列表累加得到当前 `Hits`。
/// lazer HitResult 枚举值映射：
/// 1 = miss
/// 2 = meh (50)
/// 3 = ok (100)
/// 4 = good (katu / 200)
/// 5 = great (300)
/// 6 = perfect (geki / 320)
pub fn hits_from_events(events: &[RawHitEvent]) -> Hits {
    let mut hits = Hits::default();
    for event in events {
        match event.result {
            6 => hits.geki += 1,
            5 => hits.n300 += 1,
            4 => hits.katu += 1,
            3 => hits.n100 += 1,
            2 => hits.n50 += 1,
            1 => hits.miss += 1,
            _ => {}
        }
    }
    hits
}



/// 一条原始 replay 帧：`time` 是 ms，`keys` 是 ManiaAction 位掩码（Key1=bit0）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct RawReplayFrame {
    pub time: f64,
    pub keys: u32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReplayFrameBatch {
    pub reset: bool,
    pub session: u64,
    pub offset: usize,
    pub frames: Vec<RawReplayFrame>,
}

#[derive(Default)]
struct ReplayInner {
    frames: Vec<RawReplayFrame>,
    pending: Vec<RawReplayFrame>,
    reset_pending: bool,
    session: u64,
    sent_len: usize,
}

/// 原始 ReplayFrame 日志；与 `HitEventLog` 同样的 100ms 合并/全量快照语义。
pub struct ReplayFrameLog {
    inner: Mutex<ReplayInner>,
    cv: Condvar,
}

impl Default for ReplayFrameLog {
    fn default() -> Self {
        Self::new()
    }
}

impl ReplayFrameLog {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(ReplayInner::default()),
            cv: Condvar::new(),
        }
    }

    pub fn push(&self, reset: bool, frames: &[RawReplayFrame]) {
        let mut inner = self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if reset {
            inner.frames.clear();
            inner.pending.clear();
            inner.reset_pending = true;
            inner.session = inner.session.wrapping_add(1);
            inner.sent_len = 0;
        }
        inner.frames.extend_from_slice(frames);
        inner.pending.extend_from_slice(frames);
        self.cv.notify_all();
    }

    pub fn clear(&self) {
        let mut inner = self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        inner.frames.clear();
        inner.pending.clear();
        inner.reset_pending = true;
        inner.session = inner.session.wrapping_add(1);
        inner.sent_len = 0;
        self.cv.notify_all();
    }

    pub fn drain(&self) -> ReplayFrameBatch {
        let mut inner = self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        Self::take_pending(&mut inner)
    }

    /// Snapshot and pending broadcast share one log position, even if a poll arrives during delivery.
    pub fn drain_and_snapshot(&self) -> (ReplayFrameBatch, ReplayFrameBatch) {
        let mut inner = self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let snapshot = ReplayFrameBatch {
            reset: true, session: inner.session, offset: 0, frames: inner.frames.clone(),
        };
        (Self::take_pending(&mut inner), snapshot)
    }

    fn take_pending(inner: &mut ReplayInner) -> ReplayFrameBatch {
        if inner.reset_pending {
            inner.reset_pending = false;
            inner.pending.clear();
            inner.sent_len = inner.frames.len();
            ReplayFrameBatch {
                reset: true,
                session: inner.session,
                offset: 0,
                frames: inner.frames.clone(),
            }
        } else {
            let frames = std::mem::take(&mut inner.pending);
            let offset = inner.sent_len;
            inner.sent_len = inner.sent_len.saturating_add(frames.len());
            ReplayFrameBatch {
                reset: false,
                session: inner.session,
                offset,
                frames,
            }
        }
    }

    pub fn snapshot(&self) -> ReplayFrameBatch {
        let inner = self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        ReplayFrameBatch {
            reset: true,
            session: inner.session,
            offset: 0,
            frames: inner.frames.clone(),
        }
    }

    pub fn wait_for_change(&self, timeout: Duration) -> bool {
        let mut inner = self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if inner.pending.is_empty() && !inner.reset_pending {
            let (guard, _) = self
                .cv
                .wait_timeout(inner, timeout)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            inner = guard;
        }
        !inner.pending.is_empty() || inner.reset_pending
    }
}

/// ReplayFrame 读取游标。
#[derive(Clone, Copy, Debug, Default)]
pub struct ReplayPollState {
    score: u64,
    replay: u64,
    list: u64,
    len: usize,
    failures: u32,
}

impl ReplayPollState {
    pub fn reset(&mut self) {
        self.score = 0;
        self.replay = 0;
        self.list = 0;
        self.len = 0;
        self.failures = 0;
    }
}

/// 单次 `poll` 的会话游标；由 runner 持有，跨拍复用。
#[derive(Clone, Debug, Default)]
pub struct PollState {
    player: u64,
    score_processor: u64,
    list: u64,
    len: usize,
    failures: u32,
}

impl PollState {
    pub fn reset(&mut self) {
        self.player = 0;
        self.score_processor = 0;
        self.list = 0;
        self.len = 0;
        self.failures = 0;
    }
}

fn read_event(
    source: &dyn Source,
    section: &ScoreSection,
    items: u64,
    index: usize,
) -> Option<RawHitEvent> {
    let base = items
        .checked_add(ARRAY_DATA)?
        .checked_add((section.stride as u64).checked_mul(index as u64)?)?;
    let offset = read_f64(source, base.checked_add(section.time_offset as u64)?)?;
    let result = read_i32(source, base.checked_add(section.result as u64)?)?;
    let hit_object = read_ptr_field(source, base, section.hit_object)?;
    let col = read_i32(source, hit_object.checked_add(section.column as u64)?)?;
    if !offset.is_finite() || !(0..=255).contains(&result) || !(0..=17).contains(&col) {
        return None;
    }
    Some(RawHitEvent { col, result, offset, note: None })
}

fn fail(state: &mut PollState, log: &HitEventLog, reason: &'static str) -> Result<usize, &'static str> {
    state.failures = state.failures.saturating_add(1);
    if state.failures >= MAX_FAILURES {
        log.clear();
        state.reset();
    }
    Err(reason)
}

/// 读一拍事件。
///
/// - 会话变化 / `_size` 回退 ⇒ `reset:true` + 全量；
/// - 否则只读 `state.len..n` 的增量；
/// - 末尾元素解析失败时可能是写竞争，重试至多 3 拍；中间或重试耗尽的坏事件跳过，避免游标死锁。
pub fn poll(
    source: &dyn Source,
    section: &ScoreSection,
    player: u64,
    log: &HitEventLog,
    state: &mut PollState,
) -> Result<usize, &'static str> {
    if player == 0 {
        return fail(state, log, "player-unreadable");
    }
    let Some(score_processor) = read_ptr_field(source, player, section.player_score_processor) else {
        return fail(state, log, "score-processor-unreadable");
    };
    let Some(list) = read_ptr_field(source, score_processor, section.hit_events) else {
        return fail(state, log, "hit-events-unreadable");
    };
    let Some(items) = read_ptr_field(source, list, LIST_ITEMS) else {
        return fail(state, log, "hit-events-items-unreadable");
    };
    let Some(size) = read_i32(source, list + LIST_SIZE as u64) else {
        return fail(state, log, "hit-events-size-unreadable");
    };
    let Some(capacity) = read_i32(source, items + ARRAY_LENGTH as u64) else {
        return fail(state, log, "hit-events-capacity-unreadable");
    };
    if size < 0 || capacity < size || size > MAX_EVENTS {
        return fail(state, log, "hit-events-size-invalid");
    }
    let size = size as usize;
    let reset = state.player != player
        || state.score_processor != score_processor
        || state.list != list
        || size < state.len;
    let start = if reset { 0 } else { state.len };
    let mut events = Vec::with_capacity(size.saturating_sub(start));
    let mut trailing_failed = false;
    for index in start..size {
        match read_event(source, section, items, index) {
            Some(event) => events.push(event),
            None => {
                if index == size - 1 && state.failures < 3 {
                    trailing_failed = true;
                    break;
                }
            }
        }
    }
    if trailing_failed {
        return fail(state, log, "hit-event-invalid");
    }

    if reset {
        log.push(true, &events);
    } else if !events.is_empty() {
        log.push(false, &events);
    }
    state.player = player;
    state.score_processor = score_processor;
    state.list = list;
    state.len = size;
    state.failures = 0;
    Ok(events.len())
}


/// 读当前成绩的回放哈希（`Player.Score → ScoreInfo.<Hash>`，64-hex store key）。
///
/// 旧表缺 `score_info` / `score_hash`，或该帧读不到 ⇒ `None`（调用方按"无回放暂停数据"降级）。
pub fn read_replay_hash(
    source: &dyn Source,
    table: &OffsetTable,
    player: u64,
    layout: StringLayout,
) -> Option<String> {
    if player == 0 {
        return None;
    }
    let section = table.score()?;
    let score_info_offset = section.score_info_offset()?;
    let hash_offset = section.score_hash_offset()?;
    let score = read_ptr_field(source, player, section.player_score)?;
    let score_info = read_ptr_field(source, score, score_info_offset)?;
    let pointer = read_ptr_field(source, score_info, hash_offset)?;
    let text = read_string(source, pointer, layout)?;
    is_store_hash(&text).then(|| text.to_lowercase())
}

fn fail_replay(
    state: &mut ReplayPollState,
    log: &ReplayFrameLog,
    reason: &'static str,
) -> Result<usize, &'static str> {
    state.failures = state.failures.saturating_add(1);
    if state.failures >= MAX_FAILURES {
        log.clear();
        state.reset();
    }
    Err(reason)
}

fn read_replay_frame(
    source: &dyn Source,
    section: &ScoreSection,
    items: u64,
    index: usize,
) -> Option<RawReplayFrame> {
    let frame = read_ptr_field(source, items, (ARRAY_DATA as i64) + (index as i64) * 8)?;
    let time = read_f64(source, field_addr(frame, section.replay_time)?)?;
    if !time.is_finite() {
        return None;
    }
    let mut keys = 0u32;
    // Null or empty Actions is a genuine key-up frame. An unreadable list is not.
    let actions = read_u64(source, field_addr(frame, section.replay_actions)?)?;
    if actions != 0 {
        let size = read_i32(source, field_addr(actions, section.actions_size)?)?;
        if !(0..=32).contains(&size) { return None; }
        if size > 0 {
            let data = read_ptr_field(source, actions, section.actions_items)?;
            for i in 0..size {
                let action = read_i32(source, data.checked_add(ARRAY_DATA + (i as u64) * 4)?)?;
                if (0..=19).contains(&action) { keys |= 1 << action; }
            }
        }
    }
    Some(RawReplayFrame { time, keys })
}

/// 读一拍原始 ReplayFrame（`Player.Score.Replay.Frames`）。
/// 旧表没有 replay 段时直接返回 0；有则按 `_size` 增量读取。
pub fn poll_replay(
    source: &dyn Source,
    section: &ScoreSection,
    player: u64,
    log: &ReplayFrameLog,
    state: &mut ReplayPollState,
) -> Result<usize, &'static str> {
    if !section.replay_usable() || player == 0 {
        return Ok(0);
    }
    let Some(score) = read_ptr_field(source, player, section.player_score) else {
        return fail_replay(state, log, "replay-score-unreadable");
    };
    let Some(replay) = read_ptr_field(source, score, section.score_replay) else {
        return fail_replay(state, log, "replay-object-unreadable");
    };
    let Some(list) = read_ptr_field(source, replay, section.replay_frames) else {
        return fail_replay(state, log, "replay-frames-unreadable");
    };
    let Some(items) = read_ptr_field(source, list, LIST_ITEMS) else {
        return fail_replay(state, log, "replay-items-unreadable");
    };
    let Some(size) = read_i32(source, list + LIST_SIZE as u64) else {
        return fail_replay(state, log, "replay-size-unreadable");
    };
    let Some(capacity) = read_i32(source, items + ARRAY_LENGTH as u64) else {
        return fail_replay(state, log, "replay-capacity-unreadable");
    };
    if size < 0 || capacity < size || size > MAX_EVENTS {
        return fail_replay(state, log, "replay-size-invalid");
    }
    let size = size as usize;
    let reset = state.score != score || state.replay != replay || state.list != list || size < state.len;
    let start = if reset { 0 } else { state.len };
    let mut frames = Vec::with_capacity(size.saturating_sub(start));
    let mut trailing_failed = false;
    for index in start..size {
        match read_replay_frame(source, section, items, index) {
            Some(frame) => frames.push(frame),
            None => {
                if index == size - 1 && state.failures < 3 {
                    trailing_failed = true;
                    break;
                }
            }
        }
    }
    if trailing_failed {
        return fail_replay(state, log, "replay-frame-invalid");
    }
    if reset {
        log.push(true, &frames);
    } else if !frames.is_empty() {
        log.push(false, &frames);
    }
    state.score = score;
    state.replay = replay;
    state.list = list;
    state.len = size;
    state.failures = 0;
    Ok(frames.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Mem {
        bytes: Vec<u8>,
    }

    impl Mem {
        fn new() -> Self {
            Self { bytes: vec![0; 0x20000] }
        }

        fn write_u64(&mut self, addr: u64, value: u64) {
            let at = addr as usize;
            self.bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
        }

        fn write_i32(&mut self, addr: u64, value: i32) {
            let at = addr as usize;
            self.bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }

        fn write_f64(&mut self, addr: u64, value: f64) {
            let at = addr as usize;
            self.bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
        }
    }

    impl Source for Mem {
        fn read(&self, addr: u64, len: usize) -> Option<Vec<u8>> {
            let start = addr as usize;
            self.bytes.get(start..start + len).map(|slice| slice.to_vec())
        }
    }

    const PLAYER: u64 = 0x1000;
    const SP: u64 = 0x2000;
    const LIST: u64 = 0x3000;
    const ITEMS: u64 = 0x4000;
    const EVENTS: u64 = 0x8000;

    fn section() -> ScoreSection {
        ScoreSection {
            player_score_processor: 1096,
            hit_events: 656,
            hit_object: 0,
            time_offset: 16,
            result: 24,
            column: 72,
            stride: 64,
            player_score: 0,
            score_replay: 0,
            replay_frames: 0,
            replay_time: 0,
            replay_actions: 0,
            actions_items: 0,
            actions_size: 0,
            score_result_counts: None,
            score_info: None,
            score_hash: None,
            witness: String::new(),
        }
    }

    fn wire(mem: &mut Mem, count: i32) {
        mem.write_u64(PLAYER + 1096, SP);
        mem.write_u64(SP + 656, LIST);
        mem.write_u64(LIST + 8, ITEMS);
        mem.write_i32(LIST + 16, count);
        mem.write_i32(ITEMS + 8, count.max(1));
    }

    fn event(mem: &mut Mem, index: usize, result: i32, offset: f64, col: i32) {
        let base = ITEMS + 0x10 + 64 * index as u64;
        mem.write_f64(base + 0x10, offset);
        mem.write_i32(base + 0x18, result);
        let hit_object = EVENTS + 0x100 * index as u64;
        mem.write_u64(base, hit_object);
        mem.write_i32(hit_object + 0x48, col);
    }

    #[test]
    fn first_poll_is_reset_and_then_delta() {
        let mut mem = Mem::new();
        wire(&mut mem, 2);
        event(&mut mem, 0, 6, -1.5, 0);
        event(&mut mem, 1, 5, 2.0, 3);
        let log = HitEventLog::new();
        let mut state = PollState::default();
        assert_eq!(poll(&mem, &section(), PLAYER, &log, &mut state), Ok(2));
        let batch = log.drain();
        assert!(batch.reset);
        assert_eq!(batch.events.len(), 2);
        assert_eq!(batch.events[1].col, 3);

        wire(&mut mem, 3);
        event(&mut mem, 2, 4, 4.0, 1);
        assert_eq!(poll(&mem, &section(), PLAYER, &log, &mut state), Ok(1));
        let batch = log.drain();
        assert!(!batch.reset);
        assert_eq!(batch.events.len(), 1);
        assert_eq!(batch.events[0].result, 4);
    }

    #[test]
    fn size_regression_requests_full_snapshot() {
        let mut mem = Mem::new();
        wire(&mut mem, 2);
        event(&mut mem, 0, 6, 0.0, 0);
        event(&mut mem, 1, 6, 0.0, 1);
        let log = HitEventLog::new();
        let mut state = PollState::default();
        assert_eq!(poll(&mem, &section(), PLAYER, &log, &mut state), Ok(2));
        let _ = log.drain();

        wire(&mut mem, 1);
        assert_eq!(poll(&mem, &section(), PLAYER, &log, &mut state), Ok(1));
        let batch = log.drain();
        assert!(batch.reset);
        assert_eq!(batch.events.len(), 1);
    }

    #[test]
    fn invalid_column_does_not_advance_cursor() {
        let mut mem = Mem::new();
        wire(&mut mem, 1);
        event(&mut mem, 0, 6, 0.0, 18); // 越界列
        let log = HitEventLog::new();
        let mut state = PollState::default();
        assert_eq!(poll(&mem, &section(), PLAYER, &log, &mut state), Err("hit-event-invalid"));
        assert_eq!(state.len, 0);

        // 修正后下一拍能正常读到，不会跳过这条。
        event(&mut mem, 0, 6, 0.0, 2);
        assert_eq!(poll(&mem, &section(), PLAYER, &log, &mut state), Ok(1));
        assert_eq!(log.drain().events[0].col, 2);
    }

    #[test]
    fn permanently_invalid_trailing_event_is_skipped_after_retries() {
        let mut mem = Mem::new();
        wire(&mut mem, 1);
        event(&mut mem, 0, 6, 0.0, 18); // 越界列
        let log = HitEventLog::new();
        let mut state = PollState::default();

        // 前 3 次认为可能是写竞争，返回 Err 并不推进游标
        for _ in 0..3 {
            assert_eq!(poll(&mem, &section(), PLAYER, &log, &mut state), Err("hit-event-invalid"));
            assert_eq!(state.len, 0);
        }

        // 第 4 次重试耗尽，跳过该坏事件并推进游标，避免死锁
        assert_eq!(poll(&mem, &section(), PLAYER, &log, &mut state), Ok(0));
        assert_eq!(state.len, 1);

        // 后续新事件正常读取
        wire(&mut mem, 2);
        event(&mut mem, 1, 5, 1.0, 3);
        assert_eq!(poll(&mem, &section(), PLAYER, &log, &mut state), Ok(1));
        assert_eq!(state.len, 2);
        assert_eq!(log.drain().events[0].col, 3);
    }

    #[test]
    fn intermediate_invalid_event_is_skipped_immediately() {
        let mut mem = Mem::new();
        wire(&mut mem, 2);
        event(&mut mem, 0, 6, 0.0, 18); // 中间坏事件
        event(&mut mem, 1, 5, 1.0, 2);  // 后面已有新事件
        let log = HitEventLog::new();
        let mut state = PollState::default();

        // 因为已不是末尾元素，立刻跳过坏事件并读取后续有效事件
        assert_eq!(poll(&mem, &section(), PLAYER, &log, &mut state), Ok(1));
        assert_eq!(state.len, 2);
        let batch = log.drain();
        assert_eq!(batch.events.len(), 1);
        assert_eq!(batch.events[0].col, 2);
    }

    #[test]
    fn snapshot_survives_drain() {
        let mut mem = Mem::new();
        wire(&mut mem, 1);
        event(&mut mem, 0, 6, 0.0, 0);
        let log = HitEventLog::new();
        let mut state = PollState::default();
        let _ = poll(&mem, &section(), PLAYER, &log, &mut state);
        let _ = log.drain();
        let snapshot = log.snapshot();
        assert!(snapshot.reset);
        assert_eq!(snapshot.events.len(), 1);
    }

    #[test]
    fn hits_from_events_counts_correctly() {
        let events = vec![
            RawHitEvent { col: 0, result: 6, offset: 0.0, note: None },
            RawHitEvent { col: 1, result: 6, offset: 1.0, note: None },
            RawHitEvent { col: 2, result: 5, offset: 2.0, note: None },
            RawHitEvent { col: 3, result: 4, offset: 3.0, note: None },
            RawHitEvent { col: 0, result: 3, offset: 4.0, note: None },
            RawHitEvent { col: 1, result: 2, offset: 5.0, note: None },
            RawHitEvent { col: 2, result: 1, offset: 6.0, note: None },
            RawHitEvent { col: 3, result: 99, offset: 7.0, note: None },
        ];
        let hits = hits_from_events(&events);
        assert_eq!(hits.geki, 2);
        assert_eq!(hits.n300, 1);
        assert_eq!(hits.katu, 1);
        assert_eq!(hits.n100, 1);
        assert_eq!(hits.n50, 1);
        assert_eq!(hits.miss, 1);

        let log = HitEventLog::new();
        log.push(false, &events);
        let log_hits = log.current_hits();
        assert_eq!(log_hits, hits);
    }

    #[test]
    fn log_tracks_session_and_offset_across_resets_and_deltas() {
        let log = HitEventLog::new();
        let e1 = RawHitEvent { col: 0, result: 6, offset: 0.0, note: None };
        let e2 = RawHitEvent { col: 1, result: 5, offset: 1.0, note: None };
        let e3 = RawHitEvent { col: 2, result: 6, offset: 2.0, note: None };

        log.push(true, &[e1]);
        let b1 = log.drain();
        assert!(b1.reset);
        assert_eq!(b1.session, 1);
        assert_eq!(b1.offset, 0);
        assert_eq!(b1.events.len(), 1);

        log.push(false, &[e2, e3]);
        let snap = log.snapshot();
        assert!(snap.reset);
        assert_eq!(snap.session, 1);
        assert_eq!(snap.offset, 0);
        assert_eq!(snap.events.len(), 3);

        let b2 = log.drain();
        assert!(!b2.reset);
        assert_eq!(b2.session, 1);
        assert_eq!(b2.offset, 1);
        assert_eq!(b2.events.len(), 2);

        // Retry / clear bumps session and resets offset to 0.
        log.clear();
        let b3 = log.drain();
        assert!(b3.reset);
        assert_eq!(b3.session, 2);
        assert_eq!(b3.offset, 0);
        assert!(b3.events.is_empty());

        log.push(false, &[e1]);
        let b4 = log.drain();
        assert!(!b4.reset);
        assert_eq!(b4.session, 2);
        assert_eq!(b4.offset, 0);
        assert_eq!(b4.events.len(), 1);
    }

    #[test]
    fn replay_log_tracks_session_and_offset_across_retry() {
        let log = ReplayFrameLog::new();
        let f1 = RawReplayFrame { time: -200.0, keys: 0 };
        let f2 = RawReplayFrame { time: 100.0, keys: 1 };
        let f3 = RawReplayFrame { time: 200.0, keys: 2 };

        log.push(true, &[f1, f2]);
        let b1 = log.drain();
        assert!(b1.reset);
        assert_eq!(b1.session, 1);
        assert_eq!(b1.offset, 0);
        assert_eq!(b1.frames.len(), 2);

        log.push(false, &[f3]);
        let b2 = log.drain();
        assert!(!b2.reset);
        assert_eq!(b2.session, 1);
        assert_eq!(b2.offset, 2);
        assert_eq!(b2.frames.len(), 1);

        log.push(true, &[]);
        let b3 = log.drain();
        assert!(b3.reset);
        assert_eq!(b3.session, 2);
        assert_eq!(b3.offset, 0);
        assert!(b3.frames.is_empty());
    }

    fn replay_section() -> ScoreSection {
        ScoreSection {
            player_score: 0x480,
            score_replay: 0x10,
            replay_frames: 0x08,
            replay_time: 0x10,
            replay_actions: 0x18,
            actions_items: 0x08,
            actions_size: 0x10,
            ..section()
        }
    }

    fn wire_replay(mem: &mut Mem, count: i32) {
        const SCORE: u64 = 0x5000;
        const REPLAY: u64 = 0x5800;
        const RLIST: u64 = 0x6000;
        const RITEMS: u64 = 0x7000;
        mem.write_u64(PLAYER + 0x480, SCORE);
        mem.write_u64(SCORE + 0x10, REPLAY);
        mem.write_u64(REPLAY + 0x08, RLIST);
        mem.write_u64(RLIST + 8, RITEMS);
        mem.write_i32(RLIST + 16, count);
        mem.write_i32(RITEMS + 8, count.max(1));
    }

    fn write_replay_frame(mem: &mut Mem, index: usize, time: f64, actions: &[i32], null_actions: bool) {
        const RITEMS: u64 = 0x7000;
        const FRAMES: u64 = 0x9000;
        const ACTIONS: u64 = 0xA000;
        const ACTION_DATA: u64 = 0xB000;
        let frame = FRAMES + 0x40 * index as u64;
        mem.write_u64(RITEMS + 0x10 + 8 * index as u64, frame);
        mem.write_f64(frame + 0x10, time);
        if null_actions {
            mem.write_u64(frame + 0x18, 0);
            return;
        }
        let act_obj = ACTIONS + 0x40 * index as u64;
        let act_items = ACTION_DATA + 0x80 * index as u64;
        mem.write_u64(frame + 0x18, act_obj);
        mem.write_u64(act_obj + 0x08, act_items);
        mem.write_i32(act_obj + 0x10, actions.len() as i32);
        for (i, &action) in actions.iter().enumerate() {
            mem.write_i32(act_items + 0x10 + 4 * i as u64, action);
        }
    }

    #[test]
    fn poll_replay_reads_frames_and_accepts_null_actions() {
        let mut mem = Mem::new();
        let sec = replay_section();
        wire_replay(&mut mem, 2);
        write_replay_frame(&mut mem, 0, -100.0, &[], true);
        write_replay_frame(&mut mem, 1, 150.0, &[0, 3], false);
        let log = ReplayFrameLog::new();
        let mut state = ReplayPollState::default();

        assert_eq!(poll_replay(&mem, &sec, PLAYER, &log, &mut state), Ok(2));
        let b1 = log.drain();
        assert!(b1.reset);
        assert_eq!(b1.frames, vec![
            RawReplayFrame { time: -100.0, keys: 0 },
            RawReplayFrame { time: 150.0, keys: 0b1001 },
        ]);

        wire_replay(&mut mem, 3);
        write_replay_frame(&mut mem, 2, 200.0, &[1], false);
        assert_eq!(poll_replay(&mem, &sec, PLAYER, &log, &mut state), Ok(1));
        let b2 = log.drain();
        assert!(!b2.reset);
        assert_eq!(b2.offset, 2);
        assert_eq!(b2.frames, vec![RawReplayFrame { time: 200.0, keys: 0b0010 }]);
    }

    #[test]
    fn poll_replay_retries_trailing_invalid_frame_then_skips() {
        let mut mem = Mem::new();
        let sec = replay_section();
        wire_replay(&mut mem, 1);
        // NaN timestamp simulates a partially initialized trailing frame.
        write_replay_frame(&mut mem, 0, f64::NAN, &[], true);
        let log = ReplayFrameLog::new();
        let mut state = ReplayPollState::default();

        for _ in 0..3 {
            assert_eq!(poll_replay(&mem, &sec, PLAYER, &log, &mut state), Err("replay-frame-invalid"));
            assert_eq!(state.len, 0);
        }
        assert_eq!(poll_replay(&mem, &sec, PLAYER, &log, &mut state), Ok(0));
        assert_eq!(state.len, 1);
    }
}
