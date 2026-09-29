// `osu` 模块门面 + 轮询线程（Wave C2：stable 全字段 + 门状态机）。
//
// 本模块是 stable 读取路径的**装配点**：进程发现（`win.rs`）→ 锚点定址（`patterns.rs`
// + `scan.rs`）→ 每 tick 逐链读取（`stable.rs`）→ 结构与不变量门（`invariants.rs`）→
// 载荷（`packet.rs`）/ 对拍（`compare.rs`）。
//
// 四层门（计划 §3.4）：
// - **L0 锚点解析**：附着时/进程变化时/失败刷新时；失败 ⇒ `signature-miss:<key>` +
//   退避 2/4/8/16/30 s
// - **L1 结构校验**：每次读（候选自证 + 整块读满 + 指针合理性 I-06）
// - **L2 不变量**：`invariants::evaluate`（每帧；时序性的只在迁移帧）
// - **L3 影子比对**：`shadow.rs`（仅对拍装置使用）
//
// 健康状态机在 `invariants::Gate`（纯逻辑，可单测）；本文件只负责**驱动**它并决定
// 推给消费者的载荷：
// - `healthy`/`degraded` ⇒ 全量帧（`ReaderState.packet`）
// - 硬失败未满 3 次 ⇒ **字段级冻结**帧（只有 `client`+`state`，`beatmap` 被省略）
// - 连续 3 次硬失败 ⇒ `unhealthy`，**不出帧**（`packet = None` + reason）
//
// 线程模型：单线程循环 + 一个 dev-only 的 tosu WS 客户端线程（`compare.rs`）。
// 所有句柄都在 `win::Target` 的 `Drop` 里关闭——`detach` 就是 `target = None`。

pub mod beatmap_file;
pub mod compare;
pub mod invariants;
pub mod keys;
pub mod model;
pub mod offsets;
pub mod packet;
pub mod patterns;
pub mod scan;
pub mod shadow;
pub mod stable;
pub mod win;

use crate::osu::invariants::{FrameAction, Gate, HealthState};
use crate::osu::model::{Client, Reason, Snapshot};
use serde_json::Value;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// 附着失败后的重试间隔（§3.4：附着重试 2 s）。
pub const ATTACH_RETRY: Duration = Duration::from_secs(2);
/// 兜底路径（PID 扫描）失败后的重试间隔：一次全量扫描是秒级开销，不能 2 秒来一次。
pub const SWEEP_RETRY: Duration = Duration::from_secs(30);
/// 正常 tick。
pub const TICK: Duration = Duration::from_millis(250);
/// 锚点扫描失败后的重试间隔（扫描要 ~8s，不能用 250ms 去撞）。
pub const SCAN_RETRY: Duration = Duration::from_secs(5);
/// 区域过滤：先用参考过滤器（RW/RWX），关键锚点没命中再退化到宽过滤器。
pub const FILTER_READY: &[u32] = &[win::FILTER_RW, win::FILTER_READABLE];
/// 单进程区域数上限（防某个进程挖出一个病态长的 VAD 列表把扫描拖死）。
const REGION_LIMIT: usize = 65_536;
/// 单锚点的命中列表上限：同一签名可能出现多次（P1 的 `configurationAddr` 有 2 处），
/// 上限给 16 是为了"同形状代码"不再把真正的那一处挤掉，同时不让扫描无界累积。
const ANCHOR_HIT_LIMIT: usize = 16;

/// 读取线程对外可见的状态。
#[derive(Clone, Debug)]
pub struct ReaderState {
    /// 最新快照（诊断/对拍用；**载荷**看 `packet`）。
    pub snapshot: Option<Snapshot>,
    /// 本帧**对外可用**的载荷：`None` = 不出帧（`unhealthy`）；冻结帧只有
    /// `client` + `state`（见 `packet::frozen_packet_from_snapshot`）。
    pub packet: Option<Value>,
    /// 不可用/降级原因（`Some` = 有事情发生；字面量见 `model::Reason::as_str`）。
    pub reason: Option<String>,
    /// 健康状态（`idle`/`healthy`/`degraded`/`unhealthy`）。
    pub health: String,
    /// 本帧是否处于**字段级冻结**（只发 `state.name`）。
    pub frozen: bool,
    /// 本帧的降级字段（页面 `sources.osu.degradedFields` 的来源）。
    pub degraded_fields: Vec<String>,
    /// 连续硬失败次数（0..=INVARIANT_STRIKES）。
    pub strikes: u32,
    pub pid: Option<u32>,
    pub image_path: Option<String>,
    pub client: Option<String>,
    /// 锚点定址结果（诊断/证据用）。
    pub anchors: Option<AnchorAddrs>,
    pub scan_ms: Option<u128>,
}

impl Default for ReaderState {
    fn default() -> Self {
        ReaderState {
            snapshot: None,
            packet: None,
            reason: None,
            health: HealthState::Idle.as_str().to_string(),
            frozen: false,
            degraded_fields: Vec::new(),
            strikes: 0,
            pid: None,
            image_path: None,
            client: None,
            anchors: None,
            scan_ms: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AnchorAddrs {
    pub status_ptr: u32,
    pub base_addr: u32,
    pub play_time_addr: u32,
    pub ruleset_addr: u32,
    pub menu_mods_ptr: u32,
    /// best-effort（未命中 = 0）。
    pub audio_length_ptr: u32,
    /// B3 的 best-effort 项（未命中 = 0）。
    pub settings_class_addr: u32,
}

impl AnchorAddrs {
    pub fn from_table(table: &patterns::AnchorTable) -> AnchorAddrs {
        AnchorAddrs {
            status_ptr: table.status_ptr.unwrap_or(0),
            base_addr: table.base_addr.unwrap_or(0),
            play_time_addr: table.play_time_addr.unwrap_or(0),
            ruleset_addr: table.rulesets_addr.unwrap_or(0),
            menu_mods_ptr: table.menu_mods_ptr.unwrap_or(0),
            audio_length_ptr: table.audio_length_ptr.unwrap_or(0),
            settings_class_addr: table.settings_class_addr.unwrap_or(0),
        }
    }
}

/// 读取线程的句柄（`spawn_osu_reader` 返回；可 clone 给消费者）。
#[derive(Clone)]
pub struct Reader {
    inner: Arc<Mutex<ReaderState>>,
}

impl Reader {
    pub fn latest(&self) -> ReaderState {
        self.inner.lock().unwrap().clone()
    }

    /// 最新快照（`None` = 未附着/未定址/被 detach/处于 `unhealthy`）。
    pub fn snapshot(&self) -> Option<Snapshot> {
        self.inner.lock().unwrap().snapshot.clone()
    }

    /// 最新原因字符串（`None` = 正常发布）。
    pub fn reason(&self) -> Option<String> {
        self.inner.lock().unwrap().reason.clone()
    }

    /// 本帧可发布的载荷（`None` = 不出帧）。这是 `osu_compat` 的 LIVE 模式的唯一入口。
    pub fn packet(&self) -> Option<Value> {
        self.inner.lock().unwrap().packet.clone()
    }

    pub fn state(&self) -> ReaderState {
        self.latest()
    }
}

/// 启动读取线程（`server::start` 调用）。返回句柄；线程自身永不 panic 退出。
pub fn spawn_osu_reader() -> Reader {
    let reader = Reader {
        inner: Arc::new(Mutex::new(ReaderState::default())),
    };
    let handle = reader.clone();
    thread::spawn(move || run(handle));
    reader
}

/// 进程级读取句柄：`server::start` 注入一次（`osu_compat` 的 LIVE 模式用它）。
pub static INSTANCE: std::sync::OnceLock<Reader> = std::sync::OnceLock::new();

/// 便捷读取入口（未启动时返回 `None`）。
pub fn instance() -> Option<Reader> {
    INSTANCE.get().cloned()
}

/// 主循环：发现 → 附着 → 定址 → 每 tick 读 + 过门 + 发布。
#[cfg(windows)]
fn run(reader: Reader) {
    let mut attached: Option<win::Target> = None;
    let mut anchors: Option<patterns::AnchorTable> = None;
    // C1：区域列表按附着缓存（换进程/重附着即重建；见 `scan::RegionCache` 的生命周期说明）。
    let mut regions: scan::RegionCache = scan::RegionCache::new();
    let mut last_scan_fail: Option<std::time::Instant> = None;
    let mut scan_fail_attempt: usize = 0;
    let mut gate = Gate::new();
    let mut previous_live: Option<i32> = None;

    // dev-only 对拍装置：tosu 载荷进 Mutex，本线程每 tick 落一条 JSONL。
    let compare_on = compare::enabled();
    let feed = if compare_on {
        let feed = Arc::new(compare::TosuFeed::default());
        compare::spawn_tosu_client(feed.clone());
        Some(feed)
    } else {
        None
    };
    let mut writer = compare_on.then(|| compare::SampleWriter::new(compare::samples_path()));
    // 对拍节奏（默认 100 ms；`MMA_OSU_COMPARE_INTERVAL_MS` 覆盖）。关闭对拍时退回 `TICK`。
    let compare_interval = if compare_on {
        compare::interval()
    } else {
        TICK
    };
    if compare_on {
        eprintln!(
            "[osu] compare: interval={}ms out={}",
            compare_interval.as_millis(),
            compare::samples_path().display()
        );
    }
    // `.osu` 解析缓存：key = `beatmap.checksum`（同一张图只解析一次；换图/换 md5 才重读）。
    let mut osu_cache: Option<OsuFileCache> = None;

    loop {
        let now_ms = crate::server::now_ms();
        // ⓪ 冻结的有界窗口到期 ⇒ **强制 L0 重解析**（唯一的"退出冻结"路径之一；
        //    另一条是外部原因导致的重附着）。绝不能靠"值看起来又合理了"退出。
        if gate.should_re_resolve(now_ms) && anchors.is_some() {
            eprintln!(
                "[osu] gate: freeze window expired ({}) — forcing anchor re-resolution (reason={:?})",
                gate.re_resolve_due_in_ms(now_ms).unwrap_or(0),
                gate.reason()
            );
            anchors = None;
            osu_cache = None;
            last_scan_fail = None;
        }
        // ① 附着（进程重启/被 detach 后自动重新发现）。
        if attached.is_none() {
            match win::select_target() {
                Ok(target) => {
                    publish_idle(&reader, &gate, Some(target.pid), Some(&target));
                    eprintln!(
                        "[osu] attached pid={} bitness=0x{:04X} module_base=0x{:08X} path={}",
                        target.pid,
                        target.bitness,
                        target.module_base,
                        target.image_path.display()
                    );
                    attached = Some(target);
                    anchors = None;
                    regions = scan::RegionCache::new();
                    previous_live = None;
                }
                Err(reason) => {
                    let detail = reason.as_str();
                    let outcome = gate.on_anchor_failure(reason.clone(), now_ms);
                    if reason != Reason::ProcessNotFound {
                        eprintln!("[osu] no target: {detail}");
                    }
                    publish_outcome(&reader, &gate, &outcome, None, None);
                    log_transition(&outcome);
                    // 兜底扫描（秒级）失败后拉长重试，避免每 2 秒全量重扫。
                    thread::sleep(if win::last_discovery_used_sweep() {
                        SWEEP_RETRY
                    } else {
                        ATTACH_RETRY
                    });
                    continue;
                }
            }
        }

        // ② 锚点定址（每进程一次；失败按退避重试）。
        if anchors.is_none() {
            let retry_due = last_scan_fail
                .map(|at| at.elapsed() >= SCAN_RETRY.max(invariants::backoff(scan_fail_attempt)))
                .unwrap_or(true);
            if retry_due {
                let target = attached.as_ref().expect("attached");
                match resolve_anchors(target, &mut regions) {
                    Ok((table, elapsed_ms)) => {
                        eprintln!(
                            "[osu] anchors resolved in {}ms: statusPtr=0x{:08X} baseAddr=0x{:08X} playTimeAddr=0x{:08X} rulesetsAddr=0x{:08X} menuModsPtr=0x{:08X} getAudioLengthPtr=0x{:08X} settingsClassAddr=0x{:08X}",
                            elapsed_ms,
                            table.status_ptr.unwrap_or(0),
                            table.base_addr.unwrap_or(0),
                            table.play_time_addr.unwrap_or(0),
                            table.rulesets_addr.unwrap_or(0),
                            table.menu_mods_ptr.unwrap_or(0),
                            table.audio_length_ptr.unwrap_or(0),
                            table.settings_class_addr.unwrap_or(0)
                        );
                        anchors = Some(table);
                        last_scan_fail = None;
                        scan_fail_attempt = 0;
                        gate.on_anchor_resolved(now_ms);
                    }
                    Err(reason) => {
                        let detail = reason.as_str();
                        eprintln!(
                            "[osu] anchor scan failed (attempt {}): {detail}",
                            scan_fail_attempt + 1
                        );
                        last_scan_fail = Some(std::time::Instant::now());
                        let outcome = gate.on_anchor_failure(reason.clone(), now_ms);
                        log_transition(&outcome);
                        publish_outcome(&reader, &gate, &outcome, None, None);
                        write_record(
                            writer.as_mut(),
                            feed.as_ref(),
                            &CompareInput {
                                snapshot: &Snapshot::default(),
                                outcome: &outcome,
                                probe: None,
                                result: None,
                            },
                        );
                        scan_fail_attempt += 1;
                        // 扫描失败不立刻 detach（可能是过滤器不匹配而不是目标变了）；
                        // 但连续失败时把 target 丢掉重来（进程可能已经被替换）。
                        attached = None;
                        regions = scan::RegionCache::new();
                        thread::sleep(invariants::backoff(scan_fail_attempt));
                        continue;
                    }
                }
            } else {
                thread::sleep(TICK);
                continue;
            }
        }

        // ③ 每 tick 读一次 → 过门 → 发布/冻结/停帧。
        {
            let target = attached.as_ref().expect("attached");
            let table = anchors.as_ref().expect("anchors");
            match read_frame(target, table, previous_live) {
                Ok(mut frame) => {
                    // `.osu` 解析（C6）：按 checksum 缓存；只有换图时才真的读盘。
                    attach_beatmap_file(&mut frame.snapshot, &mut osu_cache);
                    // hits 的时间门需要 firstObject ⇒ 只能在解析之后应用。
                    apply_hits(&mut frame.snapshot);
                    previous_live = frame.snapshot.play_time;
                    let outcome = gate.on_frame(&frame.snapshot, now_ms);
                    log_transition(&outcome);
                    publish_outcome(
                        &reader,
                        &gate,
                        &outcome,
                        Some(&frame.snapshot),
                        Some(AnchorAddrs::from_table(table)),
                    );
                    write_record(
                        writer.as_mut(),
                        feed.as_ref(),
                        &CompareInput {
                            snapshot: &frame.snapshot,
                            outcome: &outcome,
                            probe: frame.probe.as_ref(),
                            result: frame.result.as_ref(),
                        },
                    );
                }
                Err(reason) => {
                    let detail = reason.as_str();
                    eprintln!("[osu] read failed, detaching: {detail}");
                    let outcome = gate.on_anchor_failure(reason.clone(), now_ms);
                    log_transition(&outcome);
                    publish_outcome(&reader, &gate, &outcome, None, None);
                    write_record(
                        writer.as_mut(),
                        feed.as_ref(),
                        &CompareInput {
                            snapshot: &Snapshot::default(),
                            outcome: &outcome,
                            probe: None,
                            result: None,
                        },
                    );
                    attached = None;
                    anchors = None;
                    osu_cache = None;
                    previous_live = None;
                    regions = scan::RegionCache::new();
                }
            }
        }
        thread::sleep(compare_interval);
    }
}

#[cfg(not(windows))]
fn run(reader: Reader) {
    let mut gate = Gate::new();
    let outcome = gate.on_anchor_failure(Reason::PlatformUnsupported, 0);
    publish_outcome(&reader, &gate, &outcome, None, None);
    loop {
        thread::sleep(ATTACH_RETRY);
    }
}

/// 新附着时的状态发布（**必须清掉上一进程的载荷**：SC6"游戏退出 → 不推旧值"）。
#[cfg(windows)]
fn publish_idle(
    reader: &Reader,
    gate: &Gate,
    pid: Option<u32>,
    target: Option<&win::Target>,
) {
    let mut state = reader.inner.lock().unwrap();
    state.pid = pid.or(state.pid);
    if let Some(target) = target {
        state.image_path = Some(target.image_path.to_string_lossy().to_string());
    }
    state.client = Some(Client::Stable.as_str().to_string());
    state.health = gate.state().as_str().to_string();
    state.degraded_fields = gate.degraded_fields();
    state.strikes = gate.strikes();
    // 还没读出任何一帧 ⇒ 不发载荷（旧 PID 的值绝不能漏出去）。
    state.snapshot = None;
    state.packet = None;
    state.reason = None;
}

/// 把门的输出写进 `ReaderState`（载荷/原因/健康/降级字段）。
fn publish_outcome(
    reader: &Reader,
    gate: &Gate,
    outcome: &invariants::FrameOutcome,
    snapshot: Option<&Snapshot>,
    anchors: Option<AnchorAddrs>,
) {
    let mut state = reader.inner.lock().unwrap();
    state.health = gate.state().as_str().to_string();
    state.strikes = gate.strikes();
    state.frozen = gate.frozen();
    state.degraded_fields = if outcome.degraded_fields.is_empty() {
        gate.degraded_fields()
    } else {
        outcome.degraded_fields.clone()
    };
    state.client = Some(Client::Stable.as_str().to_string());
    if let Some(anchors) = anchors {
        state.anchors = Some(anchors);
    }
    match outcome.action {
        FrameAction::Stop => {
            // `unhealthy`：**不出帧**（这是"页面无假数据"的实现面）。
            state.snapshot = snapshot.map(|snapshot| frozen_copy(snapshot));
            state.packet = None;
            state.reason = outcome.reason.as_ref().map(Reason::as_str);
        }
        FrameAction::FreezeStateOnly => {
            if let Some(snapshot) = snapshot {
                state.snapshot = Some(frozen_copy(snapshot));
                state.packet = Some(snapshot.to_frozen_packet());
            }
            state.reason = outcome.reason.as_ref().map(Reason::as_str);
        }
        FrameAction::Publish => {
            if let Some(snapshot) = snapshot {
                state.snapshot = Some(snapshot.clone());
                state.packet = Some(snapshot.to_packet());
            }
            state.reason = outcome.reason.as_ref().map(Reason::as_str);
        }
    }
}

/// 冻结帧的快照副本：把图表侧字段清空，保证**消费方拿不到**串局的数值
/// （载荷本来就已经只用 `state`，这里再兜一层，防止后续消费者误用快照）。
fn frozen_copy(snapshot: &Snapshot) -> Snapshot {
    let mut copy = snapshot.clone();
    copy.beatmap_object = None;
    copy.ruleset_base = None;
    copy.gameplay_base = None;
    copy.score_base = None;
    copy.result_base = None;
    copy.play_mods_mask = None;
    copy.result_mods_mask = None;
    copy.menu_mods_mask = None;
    copy.play_hits = None;
    copy.result_hits = None;
    copy.play_time = None;
    copy.paused = None;
    copy.checksum = None;
    copy.map_id = None;
    copy.set_id = None;
    copy.filename = None;
    copy.folder = None;
    copy.version = None;
    copy.artist = None;
    copy.title = None;
    copy.mapper = None;
    copy.beatmap_file = None;
    copy
}

fn log_transition(outcome: &invariants::FrameOutcome) {
    if let Some(reason) = outcome.transition.as_ref() {
        eprintln!(
            "[osu] gate: state={} action={:?} transition reason={}",
            outcome.state.map(|s| s.as_str()).unwrap_or("?"),
            outcome.action,
            reason.as_str()
        );
    }
}

#[cfg(windows)]
struct CompareInput<'a> {
    snapshot: &'a Snapshot,
    outcome: &'a invariants::FrameOutcome,
    probe: Option<&'a stable::ChainProbe>,
    result: Option<&'a stable::ResultRead>,
}

#[cfg(windows)]
fn write_record(
    writer: Option<&mut compare::SampleWriter>,
    feed: Option<&Arc<compare::TosuFeed>>,
    input: &CompareInput<'_>,
) {
    let Some(writer) = writer else {
        return;
    };
    let tosu = feed.and_then(|f| f.latest());
    let record = compare::record(input.snapshot, input.outcome, input.probe, input.result, tosu.as_ref());
    writer.write(&record);
}

/// 锚点定址：先参考过滤器（RW/RWX），必需锚点缺任一就用宽过滤器重扫。
///
/// 每次重扫都要遍历 ~1200–2200 个区域并读 0.6–1.3 GB（P1 实测 ~8s / ~21s），
/// 所以只在 `missing()` 非空时才退化——best-effort 锚点（`getAudioLengthPtr` /
/// `settingsClassAddr`）缺席不值得把扫描时间翻三倍。
///
/// 区域列表走 `regions`（**按附着缓存**）：同一附着内重复定址不再重走 `VirtualQueryEx`；
/// 换进程/重附着由调用方把缓存整体丢掉。
#[cfg(windows)]
fn resolve_anchors(
    target: &win::Target,
    regions: &mut scan::RegionCache,
) -> Result<(patterns::AnchorTable, u128), Reason> {
    use std::time::Instant;
    let started = Instant::now();
    let mut table = patterns::AnchorTable::default();
    // 扫描顺序 = `ANCHORS` 的顺序（自证成本从低到高），但**先算便宜的键**：
    // `statusPtr` 的每候选自证只花 1 次解引用 + 1 次读，可以用来把"同形状代码"的假阳性
    // 直接筛掉；其余自证要读字符串/指针，留到后面。
    let mut order: Vec<&'static str> = vec![
        "statusPtr",
        "baseAddr",
        "playTimeAddr",
        "rulesetsAddr",
        "menuModsPtr",
        // best-effort（C2/B3）：缺席只降级 `beatmap.time.mp3Length` / `folders.songs`。
        "getAudioLengthPtr",
        "settingsClassAddr",
    ];
    let mut degraded: Vec<&'static str> = Vec::new();

    for (index, mask) in FILTER_READY.iter().enumerate() {
        let regions = target.regions_cached(regions, *mask, REGION_LIMIT);
        let mut stats = scan::ScanStats {
            regions: regions.len(),
            ..Default::default()
        };
        for key in &mut order {
            if table.get(key).is_some() {
                continue;
            }
            let Some(anchor) = patterns::ANCHORS.iter().find(|a| a.key == *key) else {
                continue;
            };
            let pattern = scan::Pattern::parse(anchor.pattern)
                .map_err(|_| Reason::SignatureMiss(anchor.key))?;
            // 同一签名可能出现多次（P1 实测 `configurationAddr` 有 2 处，其中一处只是同形状
            // 代码），所以取列表再**逐个自证**：只有解引用后落在结构上说得通的那一个才算。
            let candidates = scan::find_in_regions(
                target.handle(),
                &regions,
                &pattern,
                anchor.offset,
                ANCHOR_HIT_LIMIT,
                &mut stats,
            );
            let chosen = candidates
                .iter()
                .copied()
                .find(|addr| anchor_proves_out(target, *key, *addr, &regions));
            eprintln!(
                "[osu] scan {} -> {} hit(s) {} chosen={:?}",
                anchor.key,
                candidates.len(),
                candidates
                    .iter()
                    .map(|a| format!("0x{a:08X}"))
                    .collect::<Vec<_>>()
                    .join(" "),
                chosen.map(|a| format!("0x{a:08X}"))
            );
            match chosen {
                Some(addr) => table.set(anchor.key, addr),
                None => {
                    if !degraded.contains(key) {
                        degraded.push(key);
                    }
                }
            }
        }
        eprintln!(
            "[osu] scan filter#{} regions={} chunks_ok={} chunks_failed={} bytes={} elapsed={}ms unresolved={:?}",
            index,
            stats.regions,
            stats.chunks_ok,
            stats.chunks_failed,
            stats.bytes_read,
            started.elapsed().as_millis(),
            degraded
        );
        if table.missing().is_none() {
            break;
        }
    }
    match table.missing() {
        Some(key) => Err(Reason::SignatureMiss(key)),
        None => Ok((table, started.elapsed().as_millis())),
    }
}

/// 逐候选自证（L1 结构校验）：只有"解引用后说得通"的候选才被采纳。
///
/// 为什么不是"取首个命中"：同一签名可能出现多次（同形状代码），而 P1 的探针取首个并全部
/// 命中，是在**另一份进程实例**（ASLR 不同、且当时映像路径不同）上发生的。B2 在真机上把
/// **原始命中地址与各候选位移**逐个打印核对（`evidence/B2-stable-minimal/diag-probe-offsets.txt`），
/// 结论：`statusPtr` 必须 `-0x4`、`menuModsPtr` 必须 `+0x9`——两处的原始命中地址读出来都
/// 不是指针，位移之后才是指针。因此判据写成**可自证的结构**（不依赖 tosu，离线同样成立）：
/// - `statusPtr`：`read_pointer(addr)` 落在小整数状态域（`0..=15`，覆盖观测到的 0/2/5/7）
/// - `baseAddr`：`read_pointer(addr - 0xC)` 是非空指针，且落在可读区域里
/// - `playTimeAddr`（C2）：`[addr+0x5]` 是落在可读区里的对齐指针，且 `[[addr+0x5]]` 落在
///   毫秒域（`±27 h`）——两条同时成立才采纳
/// - `rulesetsAddr`（C2）：`[addr-0xB]` 与 `[[addr-0xB]+0x4]` 都是落区内的对齐非空指针，
///   且规则集 `+0x0`（MethodTable）同样成立（比前两枚更严，因为它一次串起两条链）
/// - `menuModsPtr`：`read_pointer(addr)` 的**第一跳**落在可读区域里（掩码本体是什么值
///   由 L2 不变量管，不在这里判——真机实测该掩码此时是 `0x20000000`（ScoreV2 位），
///   只有"已知位"的白名单会误判成假阳性）
/// - `getAudioLengthPtr` / `settingsClassAddr`：best-effort，判据见各自分支
///
/// 校验失败 ⇒ 该键进 `unresolved`，由调用方给出 `signature-miss:<key>`（必需键）或降级
/// （best-effort 键）。
pub fn anchor_proves_out(
    target: &win::Target,
    key: &str,
    addr: u32,
    regions: &[scan::Region],
) -> bool {
    match key {
        "statusPtr" => win::read_pointer(target, addr)
            .map(|value| value <= STATE_INDEX_MAX)
            .unwrap_or(false),
        "baseAddr" => win::read_pointer(target, addr.wrapping_sub(0xC))
            .map(|object| object != 0 && region_contains(regions, object))
            .unwrap_or(false),
        "playTimeAddr" => match win::read_u32(target, addr.wrapping_add(0x5)) {
            Ok(slot) => {
                if let Some(why) = plausible_object(regions, slot) {
                    eprintln!(
                        "[osu] diag {key} candidate=0x{addr:08X} reject=slot slot=0x{slot:08X} why={why}"
                    );
                    return false;
                }
                match win::read_u32(target, slot) {
                    Ok(value) => {
                        (value as i32).unsigned_abs()
                            <= invariants::PLAY_TIME_MAX_MS as u32
                    }
                    Err(_) => false,
                }
            }
            Err(_) => false,
        },
        "rulesetsAddr" => match win::read_u32(target, addr.wrapping_sub(0xB)) {
            Ok(slot) => {
                if let Some(why) = plausible_object(regions, slot) {
                    eprintln!(
                        "[osu] diag {key} candidate=0x{addr:08X} reject=slot slot=0x{slot:08X} why={why}"
                    );
                    return false;
                }
                match win::read_u32(target, slot.wrapping_add(0x4)) {
                    Ok(ruleset) => {
                        if let Some(why) = plausible_object(regions, ruleset) {
                            eprintln!(
                                "[osu] diag {key} candidate=0x{addr:08X} reject=ruleset ruleset=0x{ruleset:08X} why={why}"
                            );
                            return false;
                        }
                        match win::read_u32(target, ruleset) {
                            Ok(vtable) => plausible_object(regions, vtable).is_none(),
                            Err(_) => false,
                        }
                    }
                    Err(_) => false,
                }
            }
            Err(_) => false,
        },
        "menuModsPtr" => win::read_u32(target, addr)
            .map(|pointer| pointer != 0 && region_contains(regions, pointer))
            .unwrap_or(false),
        "getAudioLengthPtr" => match win::read_u32(target, addr.wrapping_add(0x7)) {
            // best-effort：只要求"槽是落区内的对齐非空指针"（槽指向的对象的**值**在读取侧
            // 用 `mp3_length_is_sane` 判域）。首轮真机实测把判据加深到"对象也必须是可读区
            // 内的对齐指针"时会拒绝该候选（`[[A+7]]` 读出来不像对象），于是 mp3Length 恒降级
            // ⇒ 放宽到本判据（该字段不在 §3.3 的承诺面里，属对齐项）。
            Ok(slot) => match plausible_object(regions, slot) {
                None => true,
                Some(why) => {
                    eprintln!(
                        "[osu] diag {key} candidate=0x{addr:08X} reject=slot slot=0x{slot:08X} why={why}"
                    );
                    false
                }
            },
            Err(_) => false,
        },
        "settingsClassAddr" => match win::read_pointer(target, addr) {
            Ok(object) => {
                if let Some(why) = plausible_object(regions, object) {
                    eprintln!(
                        "[osu] diag {key} candidate=0x{addr:08X} reject=object object=0x{object:08X} why={why}"
                    );
                    return false;
                }
                match win::read_u32(target, object) {
                    Ok(vtable) => {
                        if let Some(why) = plausible_object(regions, vtable) {
                            eprintln!(
                                "[osu] diag {key} candidate=0x{addr:08X} reject=vtable object=0x{object:08X} vtable=0x{vtable:08X} why={why}"
                            );
                            return false;
                        }
                        true
                    }
                    Err(reason) => {
                        eprintln!(
                            "[osu] diag {key} candidate=0x{addr:08X} reject=vtable-read object=0x{object:08X} reason={}",
                            reason.as_str()
                        );
                        false
                    }
                }
            }
            Err(reason) => {
                eprintln!(
                    "[osu] diag {key} candidate=0x{addr:08X} reject=object-read reason={}",
                    reason.as_str()
                );
                false
            }
        },
        _ => false,
    }
}

/// 指针合理性（32 位用户态最小判据）：4 字节对齐、非空、≥ 64 KiB、落在已枚举的可读区。
/// 返回 `Some(原因)` = 不合理。
pub fn plausible_object(regions: &[scan::Region], pointer: u32) -> Option<&'static str> {
    if pointer == 0 {
        return Some("null");
    }
    if !patterns::is_aligned(pointer) {
        return Some("unaligned");
    }
    if pointer < 0x1_0000 {
        return Some("too-low");
    }
    if !region_contains(regions, pointer) {
        return Some("not-in-region");
    }
    None
}

/// 状态整数上界：tosu 的 `GameState` 枚举取到 7（`resultScreen`），留一倍余量给未观测值。
/// 注意这里只是"候选自证"的粗筛，**不是** §3.4 I-01 的名字表（观测集外仍发布 `""`）。
const STATE_INDEX_MAX: u32 = invariants::STATE_INDEX_MAX as u32;

fn region_contains(regions: &[scan::Region], addr: u32) -> bool {
    let addr = addr as usize;
    regions
        .iter()
        .any(|r| addr >= r.base && addr < r.base + r.size)
}

/// 一帧的原始读取结果（快照 + 诊断）。
#[cfg(windows)]
struct FrameRead {
    snapshot: Snapshot,
    probe: Option<stable::ChainProbe>,
    result: Option<stable::ResultRead>,
}

/// 单次采样：状态 → 播放时间 → 谱面链 → 菜单 mods → 规则集/局内/结算三条链。
///
/// **失败策略**（写死，见本文件头部）：`statusPtr` 与 Beatmap 对象指针读不出来 ⇒ `Err`
/// （detach 重来）；其余每一跳失败都只降级对应字段（`degraded_fields`）——结构性的错值
/// 由 L1/L2 的硬不变量兜住，不该让单字段抖动引发重附着。
#[cfg(windows)]
fn read_frame(
    target: &win::Target,
    table: &patterns::AnchorTable,
    previous_live: Option<i32>,
) -> Result<FrameRead, Reason> {
    let mut snapshot = Snapshot {
        client: Some(Client::Stable),
        pid: target.pid,
        ..Default::default()
    };
    let mut degraded: Vec<String> = Vec::new();

    if let Some(status_ptr) = table.status_ptr {
        let raw = win::read_pointer(target, status_ptr)?;
        let index = raw as i32;
        snapshot.state_number = Some(index);
        let name = model::state_name_for(index);
        snapshot.state_name = Some(name.to_string());
        // I-01：观测集外 → 发布 "" 并上报降级（**不**判 unhealthy）——那是 L2 的事，
        // 这里只保证"名字表之外的整数不会变成编造的名字"。
    }

    if let Some(play_time_addr) = table.play_time_addr {
        match stable::read_play_time(target, play_time_addr) {
            Ok(Some(live)) => {
                snapshot.play_time = Some(live);
                snapshot.paused = Some(stable::paused_from_previous(previous_live, Some(live)));
            }
            Ok(None) => {
                // `exit` 态：播放时钟的槽还没装载 ⇒ 合法缺失。
                degraded.push("beatmap.time.live".to_string());
            }
            Err(reason) => {
                eprintln!("[osu] playTime read failed: {}", reason.as_str());
                degraded.push("beatmap.time.live".to_string());
            }
        }
    }

    if let Some(base_addr) = table.base_addr {
        let beatmap_addr = base_addr.wrapping_sub(stable::BEATMAP_FROM_BASE);
        let object = win::read_pointer(target, beatmap_addr)?;
        snapshot.beatmap_object = Some(object);
        if object != 0 {
            // 每个字段独立降级：读不出来的那个进 degraded_fields，其余照发。
            let mut read_string = |offset: u32, field: &'static str| -> Option<String> {
                let slot = object.wrapping_add(offset);
                let ptr = match win::read_u32(target, slot) {
                    Ok(p) => p,
                    Err(_) => {
                        degraded.push(field.to_string());
                        return None;
                    }
                };
                match win::read_csharp_string(target, ptr) {
                    Ok(text) => Some(text),
                    Err(_) => {
                        degraded.push(field.to_string());
                        None
                    }
                }
            };
            snapshot.checksum = read_string(0x6C, "beatmap.md5");
            snapshot.filename = read_string(0x90, "files.beatmap");
            snapshot.folder = read_string(0x78, "folders.beatmap");
            snapshot.version = read_string(0xAC, "beatmap.version");
            snapshot.artist = read_string(0x18, "beatmap.artist");
            snapshot.title = read_string(0x24, "beatmap.title");
            snapshot.mapper = read_string(0x7C, "beatmap.mapper");
            snapshot.map_id = win::read_i32(target, object.wrapping_add(0xC8)).ok();
            snapshot.set_id = win::read_i32(target, object.wrapping_add(0xCC)).ok();
            snapshot.map_id_bits = win::read_u32(target, object.wrapping_add(0xC8)).ok();
        }
    }

    if let Some(menu_mods_ptr) = table.menu_mods_ptr {
        match win::read_pointer(target, menu_mods_ptr) {
            Ok(mask) => snapshot.menu_mods_mask = Some(mask),
            Err(_) => degraded.push("menu.mods".to_string()),
        }
    }

    // 规则集链（C2 的枢纽）：解不出来 ⇒ 局内/结算两态的 mods 与 hits 全部缺席。
    let mut probe = None;
    let mut result_read = None;
    if let Some(rulesets_addr) = table.rulesets_addr {
        match stable::resolve_ruleset(target, rulesets_addr) {
            Ok((ruleset, chain_probe)) => {
                snapshot.ruleset_base = Some(ruleset);
                probe = Some(chain_probe);
                if let Some(base_addr) = table.base_addr {
                    let in_game = stable::read_in_game(target, ruleset, base_addr);
                    snapshot.gameplay_base = in_game.gameplay_base;
                    snapshot.score_base = in_game.score_base;
                    snapshot.play_mods_mask = in_game.play_mods_mask;
                    snapshot.hits_candidates = in_game.hits_candidates.clone();
                    snapshot.hits_candidates_complete = in_game.hits_candidates_complete;
                    snapshot.retries = in_game.retries;
                    snapshot.plays = in_game.plays;
                    if in_game.gameplay_base.is_none() {
                        degraded.push("play.mods".to_string());
                    }
                }
                let result = stable::read_result(target, ruleset);
                snapshot.result_base = result.result_base;
                snapshot.result_mods_mask = result.result_mods_mask;
                snapshot.result_hits_candidates = result.hits_candidates.clone();
                snapshot.result_hits_candidates_complete = result.hits_candidates_complete;
                if result.result_base.is_none() {
                    degraded.push("resultsScreen.mods".to_string());
                }
                result_read = Some(result);
            }
            Err(reason) => {
                eprintln!("[osu] ruleset chain failed: {}", reason.as_str());
                degraded.push("play.mods".to_string());
                degraded.push("resultsScreen.mods".to_string());
            }
        }
    }

    if let Some(audio_length_ptr) = table.audio_length_ptr {
        match stable::read_mp3_length(target, audio_length_ptr) {
            Ok(length) => snapshot.mp3_length = Some(length),
            Err(_) => degraded.push("beatmap.time.mp3Length".to_string()),
        }
    }

    snapshot.game_folder = stable::game_folder(&target.image_path);

    // `folders.songs` 链（B3 新增；best-effort，失败只降级不判 unhealthy）：
    // `[[[settingsClassAddr+0x8]+0xB8]+0x4]` → C# 字符串 = osu! cfg 的 `BeatmapDirectory`
    // **值**（本机观测 `"Songs"`，名字不是路径）。
    if let Some(settings_class_addr) = table.settings_class_addr {
        snapshot.songs_cfg_value = read_songs_cfg_value(target, settings_class_addr);
        if snapshot.songs_cfg_value.is_none() {
            degraded.push("folders.songs".to_string());
        }
        // 绝对目录 = `join(dirname(exe), 值)` 存在则用它，否则用值本身（镜像 tosu 的规则）。
        snapshot.songs_folder = snapshot
            .songs_cfg_value
            .as_deref()
            .map(|value| resolve_songs_folder(&target.image_path, value));
    }
    // songs 链未走通时的**回退链**（B3 实测：cfg 链的语义尚未关闭，见 `patterns.rs` 的
    // 未关闭项）。回退顺序固定，且**只在路径真的存在时**才采纳——这样"换图时路径前缀
    // 不变"（`folder\filename` 才是身份的关键部分，前缀只是读盘用）：
    // ① `dirname(exe)\Songs`（stable 默认安装形态，本机命中）② `dirname(exe)\..\Songs`
    // （游戏与数据分离）③ `dirname(exe)\Data\Songs`（旧安装形态）。
    // 三处都不存在 ⇒ 保持 `None` ⇒ 对拍记 `skipped:osu-file-not-parsed`（**不猜**）。
    if snapshot.songs_folder.is_none() {
        snapshot.songs_folder = fallback_songs_folder(&target.image_path);
    }

    snapshot.degraded_fields = degraded;
    Ok(FrameRead {
        snapshot,
        probe,
        result: result_read,
    })
}

/// hits 的**状态门 + 链有效性门 + 时间门**应用（需要 `.osu` 的 `firstObject` ⇒ 只能在
/// 解析之后调）。判据本身是纯函数（`invariants::play_hits_publishable` /
/// `result_hits_publishable`，单测逐条钉住），这里只做"过门才构造对象"：
///
/// - **状态门**：`play.hits` 只在 `play`/`resultScreen` 态发（其他态内存里是上一局的残留；
///   tosu 在菜单/选歌态发的是全零）；`resultsScreen.hits` 只在结算态发。
/// - **链有效性门**：局内链（`score_base`）可信 **且** 8 个候选槽整块读满；
///   结算链（`result_base`）同理。选歌态那条槽是**已释放/残留**的指针
///   （真机实测 `score_base = 0x01000101` ⇒ 8 个槽读出 `38144, 180, 51712, …` 这种垃圾），
///   既不该发布、也不该被当成"当前局的计数"。
/// - **时间门**：`live >= firstObject - 100`（`stable::hits_gate_passes`）——进图加载期
///   计数域还是上一局的残留/全零，直接发会变成"看起来像真的"假计数。
///
/// 任何一道门不过 ⇒ 该键**整个不出现在载荷里**（绝不发半截值，也绝不沿用上一帧）。
pub fn apply_hits(snapshot: &mut Snapshot) {
    snapshot.play_hits = if invariants::play_hits_publishable(snapshot) {
        stable::hits_from_candidates(&snapshot.hits_candidates)
    } else {
        None
    };
    snapshot.result_hits = if invariants::result_hits_publishable(snapshot) {
        stable::hits_from_candidates(&snapshot.result_hits_candidates)
    } else {
        None
    };
}

/// songs 目录的回退解析（只读盘存在性判断；见 `read_frame` 的注释）。
pub fn fallback_songs_folder(image_path: &std::path::Path) -> Option<String> {
    let dir = image_path.parent()?;
    for candidate in [
        dir.join("Songs"),
        dir.join("..").join("Songs"),
        dir.join("Data").join("Songs"),
    ] {
        if candidate.is_dir() {
            return Some(candidate.to_string_lossy().to_string());
        }
    }
    None
}

/// `[[[settingsClassAddr+0x8]+0xB8]+0x4]` → C# 字符串（32 位约定）。
///
/// 三段链全部走 `wrapping_add`（**不**用 `+`：地址溢出在 debug 构建会 panic，而这里
/// 只是"读不到就降级"的路径）。返回 `None` = 任意一跳失败。
///
/// ⚠️ **本步仍未在真机上把这条链走通**（见 `patterns.rs::SETTINGS_CLASS_ADDR` 的未关闭项）：
/// 锚点本身能命中并通过候选自证，但 `[对象+0x8]` 那一跳读出来的是指向映像内的值，
/// `+0xB8` 一跳随即 `read-error`。该字段因此恒为 `degraded`（`folders.songs` 走回退链，
/// 见 `fallback_songs_folder`），**不影响其余字段**。
#[cfg(windows)]
fn read_songs_cfg_value(target: &win::Target, settings_class_addr: u32) -> Option<String> {
    let first = win::read_pointer(target, settings_class_addr.wrapping_add(0x8)).ok()?;
    if first == 0 {
        return None;
    }
    let second = win::read_u32(target, first.wrapping_add(0xB8)).ok()?;
    if second == 0 {
        return None;
    }
    let slot = win::read_u32(target, second.wrapping_add(0x4)).ok()?;
    win::read_csharp_string(target, slot).ok()
}

/// 镜像参照实现的 `folders.songs` 规则（**只读盘，不写**）：
/// `join(dirname(osu!.exe), cfg 值)` 存在 ⇒ 用它；否则用 cfg 值本身（P1-notes F2；
/// 本机实测 `D:\Games\osu!` + `Songs` == tosu 的 `folders.songs`）。
pub fn resolve_songs_folder(image_path: &std::path::Path, value: &str) -> String {
    let candidate = match image_path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir.join(value),
        _ => std::path::PathBuf::from(value),
    };
    if candidate.is_dir() {
        candidate.to_string_lossy().to_string()
    } else {
        value.to_string()
    }
}

/// `.osu` 解析（C6）：按 `beatmap.checksum` 缓存，命中即复用（**成功与失败都缓存**——
/// 否则一张坏图会让本线程以对拍节奏反复读盘）。
///
/// 路径 = `songs_folder \ folder \ filename`（stable 的三段拼接，照 §3.3 的"读盘"约定）。
/// 任何一步失败都只在证据里留 `beatmap_file_error`，**不影响发布**（C6 是对拍项，不是门）。
/// 顺带算一次磁盘 MD5（I-09 的软不变量）与两个文件名（`files.background`/`files.audio`）。
fn attach_beatmap_file(snapshot: &mut Snapshot, cache: &mut Option<OsuFileCache>) {
    snapshot.beatmap_file = None;
    snapshot.beatmap_file_error = None;
    snapshot.beatmap_file_md5 = None;
    snapshot.background = None;
    snapshot.audio = None;
    snapshot.beatmap_file_mismatches.clear();
    let (Some(songs), Some(folder), Some(filename), Some(checksum)) = (
        snapshot.songs_folder.as_deref(),
        snapshot.folder.as_deref(),
        snapshot.filename.as_deref(),
        snapshot.checksum.as_deref(),
    ) else {
        snapshot.beatmap_file_error = Some("songs-or-path-unavailable".to_string());
        return;
    };
    let path = std::path::Path::new(songs).join(folder).join(filename);
    snapshot.beatmap_file_path = Some(path.to_string_lossy().to_string());

    if cache.as_ref().map(|c| c.checksum.as_str()) != Some(checksum) {
        let loaded = match std::fs::read(&path) {
            Ok(bytes) => {
                let md5 = crate::server::md5_hex_bytes(&bytes);
                match String::from_utf8(bytes) {
                    Ok(text) => Ok((beatmap_file::parse(&text), md5)),
                    Err(e) => Err(format!("not-utf8: {e}")),
                }
            }
            Err(e) => Err(format!("read: {e}")),
        };
        *cache = Some(OsuFileCache {
            checksum: checksum.to_string(),
            path: path.to_string_lossy().to_string(),
            loaded,
        });
    }
    let entry = cache.as_ref().expect("cache filled above");
    match entry.loaded.as_ref() {
        Ok((file, md5)) => {
            snapshot.beatmap_file_md5 = Some(md5.clone());
            snapshot.background = file.background.clone();
            snapshot.audio = file.audio.clone();
            snapshot.beatmap_file_mismatches = mismatches_for(file, snapshot);
            snapshot.beatmap_file = Some(file.clone());
        }
        Err(error) => snapshot.beatmap_file_error = Some(error.clone()),
    }
}

/// 同 checksum ⇒ 同一张图：缓存 `.osu` 的解析结果（或失败原因）与磁盘 MD5。
struct OsuFileCache {
    checksum: String,
    /// 诊断用（换图时日志里能看到读的是哪个文件）。
    #[allow(dead_code)]
    path: String,
    loaded: Result<(beatmap_file::BeatmapFile, String), String>,
}

/// 头字段交叉校验（不一致的字段名；空 = 全等或无法比较）。
fn mismatches_for(file: &beatmap_file::BeatmapFile, snapshot: &Snapshot) -> Vec<String> {
    file.header_mismatches(
        snapshot.title.as_deref(),
        snapshot.artist.as_deref(),
        snapshot.mapper.as_deref(),
        snapshot.version.as_deref(),
        snapshot.map_id,
        snapshot.set_id,
    )
    .into_iter()
    .map(|field| field.to_string())
    .collect()
}

#[cfg(not(windows))]
fn read_songs_cfg_value(_target: &win::Target, _addr: u32) -> Option<String> {
    None
}

// ---- 非 Windows 桩（同 `malody4` 约定：同名同签名，行为为空）----

#[cfg(not(windows))]
fn resolve_anchors(
    _target: &win::Target,
    _regions: &mut scan::RegionCache,
) -> Result<(patterns::AnchorTable, u128), Reason> {
    Err(Reason::PlatformUnsupported)
}

/// 便于外部（后续步骤的服务端装配）拿到"当前是否发布了快照"的布尔。
pub fn healthy(state: &ReaderState) -> bool {
    state.packet.is_some() && state.health != HealthState::Unhealthy.as_str()
}

/// 诊断行：一行文本说明当前状态（日志/证据用）。
pub fn describe(state: &ReaderState) -> String {
    match (&state.snapshot, &state.reason) {
        (Some(snapshot), _) => {
            let keys = keys::derive(snapshot);
            format!(
                "pid={:?} health={} frozen={} state={:?} live={:?} md5={:?} identity={}",
                state.pid,
                state.health,
                state.frozen,
                snapshot.state_name,
                snapshot.play_time,
                snapshot.checksum,
                keys.identity
            )
        }
        (_, Some(reason)) => format!("unavailable: {reason}"),
        _ => format!("unavailable: <no snapshot> (health={})", state.health),
    }
}

/// 供后续步骤使用的便捷入口：快照 + 原因。
pub fn latest_pair(reader: &Reader) -> (Option<Snapshot>, Option<String>) {
    let state = reader.latest();
    (state.snapshot, state.reason)
}

/// 证据/日志用的 JSON 摘要（不含任何游戏内存地址之外的隐私内容）。
pub fn state_json(reader: &Reader) -> Value {
    let state = reader.latest();
    let keys = state
        .snapshot
        .as_ref()
        .map(keys::derive)
        .unwrap_or_default();
    serde_json::json!({
        "pid": state.pid,
        "client": state.client,
        "health": state.health,
        "frozen": state.frozen,
        "strikes": state.strikes,
        "reason": state.reason,
        "packet_present": state.packet.is_some(),
        "state_name": state.snapshot.as_ref().and_then(|s| s.state_name.clone()),
        "checksum": state.snapshot.as_ref().and_then(|s| s.checksum.clone()),
        "identity": keys.identity,
        "mod_signature": keys.mod_signature,
        // B3 新增：songs 链（原始 cfg 值 + 解析出的绝对目录）与 C6 的两个值。
        "songs_cfg_value": state.snapshot.as_ref().and_then(|s| s.songs_cfg_value.clone()),
        "songs_folder": state.snapshot.as_ref().and_then(|s| s.songs_folder.clone()),
        "first_object": state.snapshot.as_ref().and_then(|s| s.first_object()),
        "last_object": state.snapshot.as_ref().and_then(|s| s.last_object()),
        "degraded_fields": state.snapshot.as_ref().map(|s| s.degraded_fields.clone()).unwrap_or_default(),
    })
}

#[cfg(test)]
#[path = "../../tests-local/osu_beatmap_corpus.rs"]
mod tests_beatmap_corpus;
// ⚠️ `tests_beatmap_file` / `tests_shadow` 的声明**只在各自的宿主模块**里
// （`osu/beatmap_file.rs` / `osu/shadow.rs`）。这里曾各重复声明一次 ⇒ 那些测试
// 在 `cargo test` 里跑两遍，C1 清理掉，测试集因此**变短**。
#[cfg(test)]
#[path = "../../tests-local/osu_keys.rs"]
mod tests_keys;
#[cfg(test)]
#[path = "../../tests-local/osu_model.rs"]
mod tests_model;
#[cfg(test)]
#[path = "../../tests-local/osu_scan.rs"]
mod tests_scan;
