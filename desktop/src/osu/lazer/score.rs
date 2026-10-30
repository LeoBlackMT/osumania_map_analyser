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

use crate::osu::offsets::ScoreSection;
use super::source::{read_f64, read_i32, read_ptr_field, Source};

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
}

/// 一次增量（或全量）推送。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HitEventBatch {
    pub reset: bool,
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
        self.cv.notify_all();
    }

    /// 取走增量；若有待发的 reset，则返回全量（`reset:true`）。
    pub fn drain(&self) -> HitEventBatch {
        let mut inner = self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if inner.reset_pending {
            inner.reset_pending = false;
            inner.pending.clear();
            HitEventBatch {
                reset: true,
                events: inner.events.clone(),
            }
        } else {
            HitEventBatch {
                reset: false,
                events: std::mem::take(&mut inner.pending),
            }
        }
    }

    /// 当前全量快照（页面刷新/重连用）。
    pub fn snapshot(&self) -> HitEventBatch {
        let inner = self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        HitEventBatch {
            reset: true,
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
    pub frames: Vec<RawReplayFrame>,
}

#[derive(Default)]
struct ReplayInner {
    frames: Vec<RawReplayFrame>,
    pending: Vec<RawReplayFrame>,
    reset_pending: bool,
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
        self.cv.notify_all();
    }

    pub fn drain(&self) -> ReplayFrameBatch {
        let mut inner = self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if inner.reset_pending {
            inner.reset_pending = false;
            inner.pending.clear();
            ReplayFrameBatch {
                reset: true,
                frames: inner.frames.clone(),
            }
        } else {
            ReplayFrameBatch {
                reset: false,
                frames: std::mem::take(&mut inner.pending),
            }
        }
    }

    pub fn snapshot(&self) -> ReplayFrameBatch {
        let inner = self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        ReplayFrameBatch {
            reset: true,
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
    #[allow(dead_code)]
    fn reset(&mut self) {
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
    Some(RawHitEvent { col, result, offset })
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
        return Err("replay-score-unreadable");
    };
    let Some(replay) = read_ptr_field(source, score, section.score_replay) else {
        return Err("replay-object-unreadable");
    };
    let Some(list) = read_ptr_field(source, replay, section.replay_frames) else {
        return Err("replay-frames-unreadable");
    };
    let Some(items) = read_ptr_field(source, list, LIST_ITEMS) else {
        return Err("replay-items-unreadable");
    };
    let Some(size) = read_i32(source, list + LIST_SIZE as u64) else {
        return Err("replay-size-unreadable");
    };
    let Some(capacity) = read_i32(source, items + ARRAY_LENGTH as u64) else {
        return Err("replay-capacity-unreadable");
    };
    if size < 0 || capacity < size || size > MAX_EVENTS {
        return Err("replay-size-invalid");
    }
    let size = size as usize;
    let reset = state.score != score || state.replay != replay || state.list != list || size < state.len;
    let start = if reset { 0 } else { state.len };
    let mut frames = Vec::with_capacity(size.saturating_sub(start));
    for index in start..size {
        let Some(frame) = read_ptr_field(source, items, (ARRAY_DATA as i64) + (index as i64) * 8) else {
            continue;
        };
        let Some(time) = read_f64(source, frame + section.replay_time as u64) else {
            continue;
        };
        if !time.is_finite() {
            continue;
        }
        let mut keys = 0u32;
        if let Some(actions) = read_ptr_field(source, frame, section.replay_actions) {
            if let Some(action_items) = read_ptr_field(source, actions, section.actions_items) {
                if let Some(action_size) = read_i32(source, actions + section.actions_size as u64) {
                    for i in 0..action_size.clamp(0, 32) {
                        if let Some(action) = read_i32(source, action_items + ARRAY_DATA + (i as u64) * 4) {
                            if (0..=19).contains(&action) {
                                keys |= 1 << (action as u32);
                            }
                        }
                    }
                }
            }
        }
        frames.push(RawReplayFrame { time, keys });
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
}
