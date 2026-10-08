// `osu` 模块门面 + 轮询线程（Wave C2：stable 全字段 + 门状态机）。
//
// 本模块是 stable 读取路径的**装配点**：进程发现（`win.rs`）→ 锚点定址（`patterns.rs`
// + `scan.rs`）→ 每 tick 逐链读取（`stable.rs`）→ 结构与不变量门（`invariants.rs`）→
// 载荷（`packet.rs`）/ 对拍（`compare.rs`）。
//
// 四层门（计划 §3.4）：
// - **L0 锚点解析**：附着时/进程变化时/失败刷新时；**同一映像的重复定址先走
//   `anchor_cache` 的验证路径**（Step 9c：跳过 4 GB 重扫，但每枚都重新验证）；失败 ⇒
//   `signature-miss:<key>` + 退避 2/4/8/16/30 s
// - **L1 结构校验**：每次读（候选自证 + 整块读满 + 指针合理性 I-06）
// - **L2 不变量**：`invariants::evaluate`（每帧；时序性的只在迁移帧）
// - **L3 影子比对**：`shadow.rs`（仅对拍装置使用）
//
// 健康状态机在 `invariants::Gate`（纯逻辑，可单测）；本文件只负责**驱动**它并决定
// 推给消费者的载荷：
// - `healthy`/`degraded` ⇒ 全量帧（`ReaderState.packet`）
// - 硬失败未满 3 次 ⇒ **字段级冻结**帧（只有 `client`+`state`，`beatmap` 被省略）
// - **身份保持**（Step 9g，`IDENTITY_HOLD_GRACE` 内的 I-06 瞬态失败）⇒ `packet::held_packet_from`
//   的保持帧：最后一张好图的图表块 + 本帧 `state`，文件路由照常 200（`holding` 位）
// - 连续 3 次硬失败（或身份失败持续超过保持窗口）⇒ `unhealthy`，**不出帧**
//   （`packet = None` + reason）
//
// **客户端分派**（Step 10B / DEC-19）：附着时按位数定 `client`——32 位走 stable（`read_frame`
// + `patterns::ANCHORS`），64 位走 lazer（`lazer::attach` 的表梯子 + L1 vtable 证明 + 每 tick
// 的表驱动解引用链）。两条路径共用同一条下游：`.osu` 解析（`attach_beatmap_file`）、
// 门（`gate.on_frame`）、载荷（`packet.rs`）。
//
// 线程模型：单线程循环 + 一个 dev-only 的 tosu WS 客户端线程（`compare.rs`）。
// 所有句柄都在 `win::Target` 的 `Drop` 里关闭——`detach` 就是 `target = None`。
//
// **附着重试节奏**（Step 9d）也在本文件，且是纯函数 [`sweep_delay`]（表驱动单测）：曾附着过的
// 目标刚丢 ⇒ 前 30 s 每 1 s 一拍（游戏一回来就附着，"游戏关掉 → 再打开"因此是秒级恢复），
// 窗口过后回既有退避梯 2/4/8/16/30 s；冷启动（从没附着过）与候选集抖动沿用既有间隔。

pub mod anchor_cache;
pub mod beatmap_file;
pub mod compare;
pub mod discovery;
pub mod invariants;
pub mod keys;
pub mod lazer;
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
/// **快重连窗口**（Step 9d）：**曾附着过的目标刚丢**的头这 30 s 里，附着失败只等
/// [`LOSS_FAST_RETRY`]（1 s）——用户此刻最可能的动作就是把游戏再打开。
pub const LOSS_FAST_WINDOW_MS: u64 = 30_000;
/// 快重连窗口内的重试间隔（真机目标：游戏进程可见 → 附着 ≤2 s）。
pub const LOSS_FAST_RETRY: Duration = Duration::from_millis(1_000);

/// 一次**附着失败**之后、下一次尝试之前的等待时间（**纯函数**；表驱动单测
/// `osu::tests_sweep::attach_retry_delay_table` 逐行钉住）。
///
/// 背景（真机实测，`%TEMP%\mma-shell-step9\shell-err3.txt`）：游戏关掉 ⇒ 兜底 PID 扫描空表 ⇒
/// `retry_soon=false` ⇒ 旧的单一 `SWEEP_RETRY = 30 s` 让第一次重试晚了 30 s
/// （`read failed, detaching: read-error` → `pid-sweep stable=0 … decision=Retry retry_soon=false`
/// → **30 s 空等** → `attach pid=37616`）。判据改为"**曾附着过的目标丢了**就先进快重连窗口"，
/// 其余一律沿用既有间隔：
///
/// | 条件 | 间隔 | 理由 |
/// |---|---|---|
/// | 曾附着过 **且** 距丢失 < [`LOSS_FAST_WINDOW_MS`] | [`LOSS_FAST_RETRY`]（1 s） | 快重连窗口；兜底扫描本机实测 30–50 ms，1 s 一拍的负担可忽略 |
/// | `retry_soon`（候选集在抖、很快有定论） | [`ATTACH_RETRY`]（2 s） | 既有行为：下一次扫描就能附着，退避只会拖慢 |
/// | 从未附着过 **且** 走了兜底扫描 **且** 第 0 次失败 | [`SWEEP_RETRY`]（30 s） | 既有行为：游戏没开时不要每 2 s 全量重扫 65k 个 PID |
/// | 其余 | 2/4/8/16/30 s（`invariants::backoff`） | 既有退避梯 |
///
/// 入参（除时钟量外都是 `win::select_target()` 的旁路读数）：
/// - `lost_from_attached`：本次"没目标"是不是**曾附着过的目标丢了**（读过帧之后 `read-error` /
///   目标消失 ⇒ 调用方 detach）。`false` = 本进程从没附着成功过（壳刚起来 / 冷启动）。
/// - `elapsed_since_loss_ms`：距那次丢失的毫秒数（`false` 时传 0）。
/// - `consecutive_failures`：退避梯档位（0 起）。**窗口里的 1 s 重试不计入**（调用方只在窗口
///   过后才递增；否则窗口一过档位就钉在梯子顶端，梯子形同虚设），冷启动则每次失败都递增。
/// - `retry_soon` / `used_sweep`：`win::retry_soon()` / `win::last_discovery_used_sweep()`。
pub fn sweep_delay(
    lost_from_attached: bool,
    elapsed_since_loss_ms: u64,
    consecutive_failures: u32,
    retry_soon: bool,
    used_sweep: bool,
) -> Duration {
    // ① 快重连窗口：曾附着过的目标刚丢 ⇒ 1 s 一拍（**优先于**发现层的抖动标志：这段窗口里
    //    唯一重要的事就是"游戏回来时立刻抓住它"）。
    if lost_from_attached && elapsed_since_loss_ms < LOSS_FAST_WINDOW_MS {
        return LOSS_FAST_RETRY;
    }
    // ② 候选集还在抖、很快会有定论 ⇒ 既有短间隔。
    if retry_soon {
        return ATTACH_RETRY;
    }
    // ③ 冷启动的第一拍：既有策略（此时没有任何"用户刚关掉游戏又打开"的证据，
    //    而全量兜底扫描是秒级开销）。没走兜底扫描的失败本来就用 ATTACH_RETRY。
    if !lost_from_attached && used_sweep && consecutive_failures == 0 {
        return SWEEP_RETRY;
    }
    // ④ 其余：既有退避梯 2/4/8/16/30 s。冷启动的后续失败从 2 s 起步（档位 -1）；
    //    丢失过目标时，窗口过后的第一次失败（档位 0）正是梯子第一档 2 s。
    let step = if lost_from_attached {
        consecutive_failures
    } else {
        consecutive_failures.saturating_sub(1)
    };
    invariants::backoff(step as usize)
}

/// L0/L1 相位（Step 9e）：`sources.osu.phase` 的字面量来源，也是页面"正在扫描"提示的**唯一**输入。
///
/// 闭集（诊断字段；页面据 `phase` + `notice` 决定是否提示，见 CONTRACT.md §8）：
/// - `waiting-for-game`：本会话从没附着过，且失败原因就是 `process-not-found`（最可能"游戏没开"）
/// - `attaching`：目标丢了正在重附着（游戏关掉/重启）——其它失败原因也算这一相
/// - `scanning`：已附着、锚点未定址（L0 正在跑：缓存验证或全量扫描，实测 13–23 s）
/// - `healthy`：L1+ 正在发布载荷 ⇒ **提示必须消失**（这是"帧一流动就撤提示"的实现面）
/// - `unavailable`：其余（锚点已定址但没载荷 / 门在停帧）——说明由既有 `reason` 承担，不提示
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    WaitingForGame,
    Attaching,
    Scanning,
    Healthy,
    Unavailable,
}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Phase::WaitingForGame => "waiting-for-game",
            Phase::Attaching => "attaching",
            Phase::Scanning => "scanning",
            Phase::Healthy => "healthy",
            Phase::Unavailable => "unavailable",
        }
    }

    /// 页面提示句（英文，面向用户；空串 = 不提示）。
    ///
    /// 只给"用户在等"的三相：`healthy` 时卡片自己在走（提示必须消失），`unavailable` 的
    /// 说明由既有 `reason` 承担（页面不据它提示，免得与"零假数据"语义混在一起）。
    /// 字面量同时是**页面侧测试**的期望值来源（`js/app/sources/osuScanHint.js` 只渲染壳下发的这一句）。
    pub fn notice(self) -> &'static str {
        match self {
            Phase::Scanning => "Scanning osu! memory… (about 10–20 s)",
            Phase::Attaching => "Re-attaching to osu!…",
            Phase::WaitingForGame => "Waiting for osu! to start…",
            Phase::Healthy | Phase::Unavailable => "",
        }
    }
}

/// 相位派生的**唯一**实现（纯函数；表驱动单测 `osu::tests_phase::phase_table`）。
///
/// | published | attached | anchors_resolved | ever_attached | no_game | 相位 |
/// |---|---|---|---|---|---|
/// | true | * | * | * | * | `healthy` |
/// | false | true | false | * | * | `scanning` |
/// | false | true | true | * | * | `unavailable` |
/// | false | false | * | true | * | `attaching` |
/// | false | false | * | false | true | `waiting-for-game` |
/// | false | false | * | false | false | `attaching` |
///
/// 入参（全是 `run` 的旁路读数）：`published` = 门决定**出帧**（`FrameAction::Publish` /
/// `FreezeStateOnly`）；`attached` = 当前有目标句柄；`anchors_resolved` = L0 定址已完成；
/// `ever_attached` = 本会话附着过（含"丢了" ⇒ 快重连窗口里）；`no_game` = 本次失败原因是
/// `process-not-found`（只有"从没附着过"时它才能推出"游戏没开"）。
pub fn phase_for(
    published: bool,
    attached: bool,
    anchors_resolved: bool,
    ever_attached: bool,
    no_game: bool,
) -> Phase {
    if published {
        return Phase::Healthy;
    }
    if attached {
        return if anchors_resolved {
            Phase::Unavailable
        } else {
            Phase::Scanning
        };
    }
    if ever_attached || !no_game {
        Phase::Attaching
    } else {
        Phase::WaitingForGame
    }
}

/// L0 全量锚点扫描的**实测**进度（`sources.osu.progress` 的来源；只如实报告量得出的数）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScanProgress {
    /// 过滤器档位（`FILTER_READY` 的索引：0 = 参考过滤器，1 = 宽过滤器）。
    pub filter: usize,
    /// 本档枚举到的区域数（分母；扫描的"面"）。
    pub regions: usize,
    /// 本档已读字节数。
    pub bytes: u64,
    /// 本次定址已耗时（毫秒）。
    pub elapsed_ms: u64,
}

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
    /// `client` + `state`（见 `packet::frozen_packet_from_snapshot`）；
    /// **保持帧**（Step 9g）= 最后一张好图的图表块 + 本帧 `state`（见 `holding`）。
    pub packet: Option<Value>,
    /// 本帧是否为**身份保持帧**（Step 9g）：载荷里的 `beatmap`/`files`/`directPath`/`folders`
    /// 来自 `held_packet`（上一份验证过的），只有 `state` 是本帧的。
    ///
    /// 唯一判据来源 = 门的动作 + 真的持有 `held_packet`（没有最后一张好图 ⇒ `false`，
    /// 于是文件路由按既有语义 404）。`osu_compat::live_map` 读它决定 200/404。
    pub holding: bool,
    /// **最后一张好图**的载荷（Step 9g）：最近一次 `Publish` 的整包。
    ///
    /// 只在身份保持期被供奉（`packet::held_packet_from` + `packet::route_payload`），
    /// 且**只**给与"正在发布的图"身份逐字一致的那一份（纯函数判据）。
    /// 生命周期：`Publish` 时刷新；`unhealthy`/新附着时清空（旧 PID 的值绝不漏出去）。
    pub held_packet: Option<Value>,
    /// 最近一次**处理帧**的时刻（`server::now_ms()`）——保持的"陈腐度"判据。
    ///
    /// `osu_compat` 用它判 `held_fresh`：保持是逐帧刷新的事实，读者一停帧（冻结窗口到期的
    /// 强制重解析、`unhealthy` 之前的空档）这份"最后一张好图"就变成无界陈旧值 ⇒
    /// [`invariants::IDENTITY_HOLD_GRACE`] 之外不再供奉（逐字回到 404）。
    pub frame_at_ms: u64,
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
    /// 相位字面量（`Phase::as_str`；`""` = 读取线程还没给出结论）。Step 9e。
    pub phase: String,
    /// 相位对应的页面提示句（英文；`""` = 不提示）。Step 9e。
    pub phase_notice: String,
    /// L0 全量扫描的实测进度（只在 `scanning` 相位有值）。Step 9e。
    pub scan: Option<ScanProgress>,
    /// lazer 读取路径的诊断（表键/出处/gameBase/链地址/降级清单；Step 10B）。
    /// stable 目标上恒为 `None`。
    pub lazer: Option<lazer::Diagnostics>,
}

impl Default for ReaderState {
    fn default() -> Self {
        ReaderState {
            snapshot: None,
            packet: None,
            holding: false,
            held_packet: None,
            frame_at_ms: 0,
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
            phase: String::new(),
            phase_notice: String::new(),
            scan: None,
            lazer: None,
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
    let mut stable_table: Option<offsets::StableTable> = None;
    // Step 10B：lazer 目标（64 位）的 L0 产物（表 + gameBase + 目标环境读数）。
    // 与 `anchors` 互斥：同一时刻只有一个客户端被附着。
    let mut lazer_attach: Option<lazer::Attach> = None;
    // C1：区域列表按附着缓存（换进程/重附着即重建；见 `scan::RegionCache` 的生命周期说明）。
    let mut regions: scan::RegionCache = scan::RegionCache::new();
    // Step 9c：锚点定址结果的进程内缓存（同一 (md5, bitness, module_base) 的重复定址只做验证）。
    let mut anchor_cache: anchor_cache::AnchorCache = anchor_cache::AnchorCache::new();
    let mut last_scan_fail: Option<std::time::Instant> = None;
    let mut scan_fail_attempt: usize = 0;
    // Step 9d：**目标丢失**的时刻（`None` = 从没附着过、或已经重新附着）与退避梯档位。
    // 两者是 `sweep_delay` 的时钟/计数输入：丢失 ⇒ 开 30 s 快重连窗口（1 s 一拍）；
    // 重新附着 ⇒ 双双归零（"恢复"）。
    let mut lost_at: Option<std::time::Instant> = None;
    let mut attach_failures: u32 = 0;
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
        if gate.should_re_resolve(now_ms) && (anchors.is_some() || lazer_attach.is_some()) {
            eprintln!(
                "[osu] gate: freeze window expired ({}) — forcing anchor re-resolution (reason={:?})",
                gate.re_resolve_due_in_ms(now_ms).unwrap_or(0),
                gate.reason()
            );
            anchors = None;
            stable_table = None;
            lazer_attach = None;
            osu_cache = None;
            last_scan_fail = None;
            // Step 9e：冻结窗口到期 ⇒ 又回到 L0（缓存验证或全量扫描都在这条路上）。
            publish_phase(&reader, Phase::Scanning, None, "reason=freeze-window-expired");
        }
        // ① 附着（进程重启/被 detach 后自动重新发现）。
        if attached.is_none() {
            match win::select_target() {
                Ok(target) => {
                    // **客户端分派**（DEC-19）：位数决定走哪条读取路径；分不出位数 ⇒ 按 stable
                    // 的既有语义走（`win::select_target` 只会给出这两种 machine 之一）。
                    let client = target.client().unwrap_or(Client::Stable);
                    // **清掉上一次发现的"进程选择"原因**（Step 9b 缺陷 ①）：附着成功即
                    // "multiple-instances / process-not-found / client-ambiguous" 不再成立，
                    // 否则门会带着它冻结到 `RECOVERY_CLEAN_FRAMES` 帧之后（冻结帧不含
                    // `beatmap` ⇒ 24062 的两条文件路由 404、WS 帧没有 checksum）。
                    gate.on_attach();
                    publish_idle(&reader, &gate, Some(target.pid), Some(&target), client);
                    eprintln!(
                        "[osu] attached pid={} client={} bitness=0x{:04X} module_base=0x{:08X} path={}",
                        target.pid,
                        client.as_str(),
                        target.bitness,
                        target.module_base,
                        target.image_path.display()
                    );
                    // Step 9e：附着成功 ⇒ 进 L0（提示"正在扫描内存…"；进度由 `resolve_anchors`
                    // 的实测回调填）。写在 `attached = Some(target)` 之前：`win::Target` 不实现
                    // `Clone`，`pid` 只能借在移动之前。
                    publish_phase(&reader, Phase::Scanning, None, &format!("pid={}", target.pid));
                    crate::server::log::log_at(
                        "info",
                        &format!(
                            "[osu] attached to {:?} (pid={}, image={})",
                            target.client(),
                            target.pid,
                            target.image_path.display()
                        ),
                    );
                    attached = Some(target);
                    anchors = None;
                    stable_table = None;
                    lazer_attach = None;
                    regions = scan::RegionCache::new();
                    previous_live = None;
                    // Step 9d **恢复**：重新附着 ⇒ 快重连窗口结束、梯子归零。有丢失时打一行
                    // 明确的"从丢失到附着"耗时（这是"游戏关掉再打开"的 E2E 判据行）。
                    if let Some(lost_at) = lost_at.take() {
                        eprintln!(
                            "[osu] recovered: attached {}ms after the previous target was lost (fast window {}ms @ {}ms retries)",
                            lost_at.elapsed().as_millis(),
                            LOSS_FAST_WINDOW_MS,
                            LOSS_FAST_RETRY.as_millis()
                        );
                    }
                    attach_failures = 0;
                }
                Err(reason) => {
                    let detail = reason.as_str();
                    let outcome = gate.on_anchor_failure(reason.clone(), now_ms);
                    if reason != Reason::ProcessNotFound {
                        eprintln!("[osu] no target: {detail}");
                    }
                    publish_outcome(&reader, &gate, &outcome, None, None, None);
                    log_transition(&outcome);
                    // Step 9e：没目标 ⇒ `attaching`（曾附着过）或 `waiting-for-game`
                    // （本会话从没附着过且原因就是"进程不在"）。
                    publish_phase(
                        &reader,
                        phase_for(false, false, false, lost_at.is_some(), reason == Reason::ProcessNotFound),
                        None,
                        &format!("reason={detail}"),
                    );
                    // Step 9d：等待时长由纯函数 `sweep_delay` 定（表驱动单测逐行钉住）：
                    // **曾附着过的目标刚丢** ⇒ 前 30 s 每 1 s 一拍（游戏回来即附着），
                    // 窗口过后回既有退避梯 2/4/8/16/30 s；冷启动/抖动沿用既有间隔。
                    let (lost_from_attached, elapsed_ms) = match lost_at {
                        Some(at) => (true, at.elapsed().as_millis() as u64),
                        None => (false, 0),
                    };
                    let retry_soon = win::retry_soon();
                    let used_sweep = win::last_discovery_used_sweep();
                    let delay = sweep_delay(
                        lost_from_attached,
                        elapsed_ms,
                        attach_failures,
                        retry_soon,
                        used_sweep,
                    );
                    eprintln!(
                        "[osu] attach retry in {}ms (lost_from_attached={} since_loss={}ms failures={} retry_soon={} swept={})",
                        delay.as_millis(),
                        lost_from_attached,
                        elapsed_ms,
                        attach_failures,
                        retry_soon,
                        used_sweep
                    );
                    // 梯子档位只在**窗口过后**递增（窗口里的 1 s 重试不计入；冷启动每次失败都算）。
                    if !lost_from_attached || elapsed_ms >= LOSS_FAST_WINDOW_MS {
                        attach_failures += 1;
                    }
                    thread::sleep(delay);
                    continue;
                }
            }
        }

        // ② 锚点定址（每进程一次；失败按退避重试）。
        if anchors.is_none() && lazer_attach.is_none() {
            let retry_due = last_scan_fail
                .map(|at| at.elapsed() >= SCAN_RETRY.max(invariants::backoff(scan_fail_attempt)))
                .unwrap_or(true);
            if retry_due {
                let target = attached.as_ref().expect("attached");
                let client = target.client().unwrap_or(Client::Stable);
                // ②.L **lazer 的 L0**（Step 10B）：目标环境读数（`sq.version` /
                // `osu!.runtimeconfig.json` / `storage.ini`）→ 扫标记 → **相位 0 的 GameBase
                // 解析**（`anchor → site → … → gameBase`，多个候选取第一个）→ **表的梯子**
                // （Step 10d：L1 结构证明 = 链解通 + 表侧字段探针）。整条链在 `lazer::attach` 里。
                // 成功 ⇒ `continue`（下一拍从 ③ 开始读帧）；失败 ⇒ 与 stable 定址失败
                // **同一套处置**（detach + 退避 + 相位回 `attaching`）。
                if client == Client::Lazer {
                    match lazer::attach(target, &mut regions, lazer::env_table_path()) {
                        Ok(attach) => {
                            eprintln!(
                                "[osu] lazer L0 resolved: table={} origin={} path={} gameBase=0x{:016X} anchors={} scan_ms={}",
                                attach.table.key(),
                                attach.origin.as_str(),
                                attach.table_path.display(),
                                attach.game_base,
                                attach.marker_hits,
                                attach.scan_ms
                            );
                            publish_lazer_state(
                                &reader,
                                Some(lazer::Diagnostics::from_attach(&attach)),
                                None,
                            );
                            lazer_attach = Some(attach);
                            last_scan_fail = None;
                            scan_fail_attempt = 0;
                            gate.on_anchor_resolved(now_ms);
                            thread::sleep(TICK);
                            continue;
                        }
                        Err(reason) => {
                            let detail = reason.as_str();
                            eprintln!(
                                "[osu] lazer L0 failed (attempt {}): {detail}",
                                scan_fail_attempt + 1
                            );
                            crate::server::log::log_at(
                                "warn",
                                &format!(
                                    "[osu] lazer L0 failed (attempt {}): {detail}",
                                    scan_fail_attempt + 1
                                ),
                            );
                            last_scan_fail = Some(std::time::Instant::now());
                            let outcome = gate.on_anchor_failure(reason.clone(), now_ms);
                            log_transition(&outcome);
                            publish_outcome(&reader, &gate, &outcome, None, None, Some(client));
                            // 缺表/表不可用 ⇒ 把"因此没有来源"的字段清单**一并**上报
                            // （绝不静默：页面看到的 reason 是 `lazer-offsets-missing:<ver>`，
                            // `degradedFields` 说清哪些字段没有来源）。
                            let extra = matches!(reason, Reason::LazerOffsetsMissing(_))
                                .then(lazer::missing_table_degraded_fields);
                            publish_lazer_state(
                                &reader,
                                Some(lazer::Diagnostics {
                                    gaps: extra.clone().unwrap_or_default(),
                                    ..Default::default()
                                }),
                                extra,
                            );
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
                            attached = None;
                            regions = scan::RegionCache::new();
                            lost_at = Some(std::time::Instant::now());
                            attach_failures = 0;
                            publish_phase(&reader, Phase::Attaching, None, &format!("reason={detail}"));
                            thread::sleep(invariants::backoff(scan_fail_attempt));
                            continue;
                        }
                    }
                }
                // ②.a **缓存路径**（Step 9c）：同一个 `(exe md5, bitness, module_base)` 的重复
                //     定址只重新验证地址（签名复核 + 结构自证，判据与扫描时逐字相同），不重扫
                //     4 GB。验证不过 ⇒ 丢掉条目、落到 ②.b 全量扫描（日志已写明哪一枚哪一项失败）。
                if client == Client::Stable && stable_table.is_none() {
                    let exe_dir = target.image_path.parent();
                    let loaded = offsets::find_stable_table(exe_dir)
                        .unwrap_or_else(|_| offsets::default_stable_table());
                    eprintln!(
                        "[osu] stable table loaded: client={} ver={} verified_build={}",
                        loaded.client, loaded.version, loaded.verified_build
                    );
                    stable_table = Some(loaded);
                }
                if let Some((entry, validated_ms)) =
                    cached_anchors(target, &mut regions, &mut anchor_cache, stable_table.as_ref())
                {
                    eprintln!(
                        "[osu] anchors from cache validated in {}ms: {} unresolved={:?}",
                        validated_ms,
                        entry.describe(),
                        entry.unresolved
                    );
                    anchors = Some(entry.table);
                    last_scan_fail = None;
                    scan_fail_attempt = 0;
                    gate.on_anchor_resolved(now_ms);
                }
                // ②.b 全量扫描（首次附着 / 缓存未命中 / 缓存被证伪）。
                if anchors.is_none() {
                    // Step 9e：实测进度只从这条路径上报（缓存验证是毫秒级，不报进度）。
                    let mut on_progress =
                        |progress: ScanProgress| publish_scan_progress(&reader, progress);
                    match resolve_anchors(target, &mut regions, stable_table.as_ref(), &mut on_progress) {
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
                            // 成功即写入缓存（键 = 本次附着的映像身份）：同一进程实例的重附着
                            // （读失败重连 / 冻结窗口重解析）因此不必再扫一遍。
                            if let Some(key) = anchor_cache_key(target) {
                                anchor_cache.store(
                                    key,
                                    anchor_cache::AnchorEntry::from_table(table.clone()),
                                );
                            }
                            anchors = Some(table);
                            last_scan_fail = None;
                            scan_fail_attempt = 0;
                            gate.on_anchor_resolved(now_ms);
                            // dev-only 自测（默认关闭）：立刻把刚写进去的条目按**缓存路径**再
                            // 验证一遍并打印实测耗时——真机上"同一进程内的重定址"要等读失败
                            // 或冻结窗口（≥30 s）才发生，实证拿不到验证耗时（见 D-notes §10）。
                            if anchor_cache_selftest() {
                                let started = std::time::Instant::now();
                                match cached_anchors(target, &mut regions, &mut anchor_cache, stable_table.as_ref()) {
                                    Some((entry, validated_ms)) => eprintln!(
                                        "[osu] selftest: anchors from cache validated in {}ms ({}us): {} unresolved={:?}",
                                        validated_ms,
                                        started.elapsed().as_micros(),
                                        entry.describe(),
                                        entry.unresolved
                                    ),
                                    None => eprintln!(
                                        "[osu] selftest: cache validation returned no table (reason above)"
                                    ),
                                }
                            }
                        }
                        Err(reason) => {
                            let detail = reason.as_str();
                            eprintln!(
                                "[osu] anchor scan failed (attempt {}): {detail}",
                                scan_fail_attempt + 1
                            );
                            crate::server::log::log_at(
                                "warn",
                                &format!(
                                    "[osu] anchor scan failed (attempt {}): {detail}",
                                    scan_fail_attempt + 1
                                ),
                            );
                            last_scan_fail = Some(std::time::Instant::now());
                            let outcome = gate.on_anchor_failure(reason.clone(), now_ms);
                            log_transition(&outcome);
                            publish_outcome(&reader, &gate, &outcome, None, None, Some(client));
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
                            // Step 9d：任何"从 attached 掉到 detached"都算一次丢失（换目标走的就是
                            // 这条路）⇒ 若接下来也附着不上，快重连窗口让下一次失败不再空等 30 s。
                            lost_at = Some(std::time::Instant::now());
                            attach_failures = 0;
                            // Step 9e：定址失败 ⇒ target 被丢掉，接下来就是重附着。
                            publish_phase(&reader, Phase::Attaching, None, &format!("reason={detail}"));
                            thread::sleep(invariants::backoff(scan_fail_attempt));
                            continue;
                        }
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
            let client = target.client().unwrap_or(Client::Stable);
            // 两条读取路径共用同一条下游：`.osu` 解析 → hits 门 → 门状态机 → 载荷。
            let read = match lazer_attach.as_mut() {
                Some(attach) => lazer::read_tick(attach, target).map(|frame| {
                    publish_lazer_state(
                        &reader,
                        Some(lazer::Diagnostics {
                            gaps: frame.gaps.clone(),
                            chain: Some(frame.chain),
                            ..lazer::Diagnostics::from_attach(attach)
                        }),
                        None,
                    );
                    FrameRead {
                        snapshot: frame.snapshot,
                        probe: None,
                        result: None,
                    }
                }),
                None => {
                    let table = anchors.as_ref().expect("anchors");
                    read_frame(target, table, stable_table.as_ref(), previous_live)
                }
            };
            match read {
                Ok(mut frame) => {
                    // `.osu` 解析（C6）：按 checksum 缓存；只有换图时才真的读盘。
                    attach_beatmap_file(&mut frame.snapshot, &mut osu_cache);
                    // hits 的时间门需要 firstObject ⇒ 只能在解析之后应用。
                    apply_hits_with_topo(&mut frame.snapshot, stable_table.as_ref().map(|t| &t.topology));
                    previous_live = frame.snapshot.play_time;
                    let outcome = gate.on_frame(&frame.snapshot, now_ms);
                    log_transition(&outcome);
                    publish_outcome(
                        &reader,
                        &gate,
                        &outcome,
                        Some(&frame.snapshot),
                        anchors.as_ref().map(AnchorAddrs::from_table),
                        Some(client),
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
                    // Step 9e：L1+ 出帧 ⇒ `healthy`（提示随之消失）；被门停帧 ⇒ `unavailable`
                    // （说明由既有 `reason` 承担，页面不提示）。
                    publish_phase(
                        &reader,
                        phase_for(
                            !matches!(outcome.action, FrameAction::Stop),
                            true,
                            true,
                            true,
                            false,
                        ),
                        None,
                        &format!("pid={}", target.pid),
                    );
                }
                Err(reason) => {
                    let detail = reason.as_str();
                    eprintln!("[osu] read failed, detaching: {detail}");
                    crate::server::log::log_at(
                        "warn",
                        &format!("[osu] read failed, detaching: {detail}"),
                    );
                    let outcome = gate.on_anchor_failure(reason.clone(), now_ms);
                    log_transition(&outcome);
                    publish_outcome(&reader, &gate, &outcome, None, None, Some(client));
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
                    lazer_attach = None;
                    osu_cache = None;
                    previous_live = None;
                    regions = scan::RegionCache::new();
                    // Step 9d：这是真机里"游戏关掉"走的那条路（对死进程读内存 ⇒ 这里 detach）。
                    // 记一次**目标丢失** ⇒ 开快重连窗口（失败时 1 s 一拍，见 `sweep_delay`）。
                    lost_at = Some(std::time::Instant::now());
                    attach_failures = 0;
                    // Step 9e：目标丢了 ⇒ `attaching`（提示"正在重新附着"）。
                    publish_phase(&reader, Phase::Attaching, None, &format!("reason={detail}"));
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
    publish_outcome(&reader, &gate, &outcome, None, None, None);
    loop {
        thread::sleep(ATTACH_RETRY);
    }
}

/// 相位 + 提示 + 扫描进度的**唯一写入点**（Step 9e）；相位**变化**时打一行稳定前缀日志
/// `[osu] phase=…`（算子用它核对"扫描 → 健康"的整段耗时）。
///
/// `detail` 只进日志（如 `pid=…` / `reason=…`），**不进帧**。
#[cfg(windows)]
fn publish_phase(reader: &Reader, phase: Phase, scan: Option<ScanProgress>, detail: &str) {
    let transition = {
        let mut state = reader.inner.lock().unwrap();
        let changed = state.phase != phase.as_str();
        let transition = if changed {
            let mut line = format!("[osu] phase={}", phase.as_str());
            if !detail.is_empty() {
                line.push(' ');
                line.push_str(detail);
            }
            // 跳进 `healthy` 时带上刚结束的那次扫描实测（"13–23 s"这句话的证据行）。
            if phase == Phase::Healthy {
                if let Some(scan) = state.scan {
                    line.push_str(&format!(
                        " scan_ms={} filter={} regions={} bytes={}",
                        scan.elapsed_ms, scan.filter, scan.regions, scan.bytes
                    ));
                }
            }
            Some(line)
        } else {
            None
        };
        state.phase = phase.as_str().to_string();
        state.phase_notice = phase.notice().to_string();
        // 非 `scanning` 相位不保留进度：提示消失后旧数字没有读者，留着只会误导。
        state.scan = scan;
        transition
    };
    if let Some(line) = transition {
        eprintln!("{line}");
        crate::server::log::log_at("info", &format!("[osu] {line}"));
    }
}

/// 扫描中的进度更新（**不改相位、不打日志**：只有相位跳变才打，见 [`publish_phase`]）。
#[cfg(windows)]
fn publish_scan_progress(reader: &Reader, progress: ScanProgress) {
    let mut state = reader.inner.lock().unwrap();
    if state.phase == Phase::Scanning.as_str() {
        state.scan = Some(progress);
    }
}

/// 新附着时的状态发布（**必须清掉上一进程的载荷**：SC6"游戏退出 → 不推旧值"）。
#[cfg(windows)]
fn publish_idle(
    reader: &Reader,
    gate: &Gate,
    pid: Option<u32>,
    target: Option<&win::Target>,
    client: Client,
) {
    let mut state = reader.inner.lock().unwrap();
    state.pid = pid.or(state.pid);
    if let Some(target) = target {
        state.image_path = Some(target.image_path.to_string_lossy().to_string());
    }
    state.client = Some(client.as_str().to_string());
    state.health = gate.state().as_str().to_string();
    state.degraded_fields = gate.degraded_fields();
    state.strikes = gate.strikes();
    // 冻结标志也按门的最新判断写：否则上一次附着的冻结会挂在新附着上
    // （`osu_compat::live_map` 同时看 `frozen` 与 `packet`）。
    state.frozen = gate.frozen();
    // 还没读出任何一帧 ⇒ 不发载荷（旧 PID 的值绝不能漏出去）。
    state.snapshot = None;
    state.packet = None;
    // Step 9g：最后一张好图属于**上一个进程**⇒ 连同保持位一并作废
    // （文件路由因此不会把旧进程的谱面文件供奉给新附着）。
    state.holding = false;
    state.held_packet = None;
    state.reason = None;
    // Step 10B：lazer 诊断也属于上一个附着（表键/gameBase 绝不留到新目标上）。
    state.lazer = None;
}

/// lazer 诊断（表/链/降级清单）的**唯一写入点**（Step 10B）。
///
/// `extra_degraded` 用于"表缺失"这类**整条路径**的降级：壳同时下发 reason 与该字段清单
/// （`degradedFields`）——页面因此知道"哪些字段没有来源"，而不是猜"为什么没有数据"。
#[cfg(windows)]
fn publish_lazer_state(
    reader: &Reader,
    diagnostics: Option<lazer::Diagnostics>,
    extra_degraded: Option<Vec<String>>,
) {
    let mut state = reader.inner.lock().unwrap();
    if let Some(diagnostics) = diagnostics {
        state.lazer = Some(diagnostics);
    }
    if let Some(fields) = extra_degraded {
        for field in fields {
            if !state.degraded_fields.contains(&field) {
                state.degraded_fields.push(field);
            }
        }
    }
}

/// 把门的输出写进 `ReaderState`（载荷/原因/健康/降级字段）。
///
/// `client`（Step 10B）：`Some` = 本帧的客户端（附着时按位数定，整段会话不变）；
/// `None` = **不改**（"没有目标"的失败帧不该把上一次的 `client` 抹成 stable）。
fn publish_outcome(
    reader: &Reader,
    gate: &Gate,
    outcome: &invariants::FrameOutcome,
    snapshot: Option<&Snapshot>,
    anchors: Option<AnchorAddrs>,
    client: Option<Client>,
) {
    let mut state = reader.inner.lock().unwrap();
    state.health = gate.state().as_str().to_string();
    state.strikes = gate.strikes();
    state.frozen = gate.frozen();
    // Step 9g：保持的"陈腐度"时钟——每处理一帧就刷一次（读者停帧即停止刷新 ⇒
    // `held_fresh` 在 `IDENTITY_HOLD_GRACE` 之后转假，文件路由回到 404）。
    state.frame_at_ms = crate::server::now_ms();
    state.degraded_fields = if outcome.degraded_fields.is_empty() {
        gate.degraded_fields()
    } else {
        outcome.degraded_fields.clone()
    };
    if let Some(client) = client {
        state.client = Some(client.as_str().to_string());
    }
    if let Some(anchors) = anchors {
        state.anchors = Some(anchors);
    }
    match outcome.action {
        FrameAction::Stop => {
            // `unhealthy`：**不出帧**（这是"页面无假数据"的实现面）。
            state.snapshot = snapshot.map(|snapshot| frozen_copy(snapshot));
            state.packet = None;
            // Step 9g：停帧 ⇒ 最后一张好图作废（没有"最后一张好图" ⇒ 文件路由 404，
            // 与既有语义一致；也挡住"停帧后又被某帧保持回旧图"的可能）。
            state.holding = false;
            state.held_packet = None;
            state.reason = outcome.reason.as_ref().map(Reason::as_str);
        }
        FrameAction::FreezeStateOnly => {
            if let Some(snapshot) = snapshot {
                state.snapshot = Some(frozen_copy(snapshot));
                state.packet = Some(snapshot.to_frozen_packet());
            }
            state.holding = false;
            state.reason = outcome.reason.as_ref().map(Reason::as_str);
        }
        // **身份保持**（Step 9g）：供奉最后一张好图 + 本帧 `state`。
        // 门只是**请求**（它不知道读者手里有没有货）：没有 `held_packet` ⇒ 逐字退回
        // [`FrameAction::FreezeStateOnly`] 的既有行为（冻结帧 + 文件路由 404）。
        FrameAction::HoldLastGood => {
            let held = state.held_packet.clone();
            if let (Some(snapshot), Some(held)) = (snapshot, held) {
                state.snapshot = Some(frozen_copy(snapshot));
                state.packet = Some(crate::osu::packet::held_packet_from(&held, snapshot));
                state.holding = true;
            } else if let Some(snapshot) = snapshot {
                state.snapshot = Some(frozen_copy(snapshot));
                state.packet = Some(snapshot.to_frozen_packet());
                state.holding = false;
            }
            state.reason = outcome.reason.as_ref().map(Reason::as_str);
        }
        FrameAction::Publish => {
            if let Some(snapshot) = snapshot {
                state.snapshot = Some(snapshot.clone());
                let packet = snapshot.to_packet();
                state.held_packet = Some(packet.clone());
                state.packet = Some(packet);
            }
            state.holding = false;
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
    // Step 10B：lazer 侧的图表数据同样必须清掉（冻结帧只发 `client`+`state`）。
    copy.lazer_mods = None;
    copy.lazer_chain = None;
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
///
/// `progress`（Step 9e）：每换一档过滤器、每扫完一枚锚点回一次**实测**读数
/// （`regions`/`bytes`/`elapsed_ms`），由调用方写进 `sources.osu.progress`——只报告，
/// 不影响扫描决策（同一个 `started` 时钟，与该路径既有的 `[osu] scan filter#…` 日志同源）。
#[cfg(windows)]
fn resolve_anchors(
    target: &win::Target,
    regions: &mut scan::RegionCache,
    stable_table: Option<&offsets::StableTable>,
    progress: &mut dyn FnMut(ScanProgress),
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
        // Step 9e：这一档的"面"（区域数）已知 ⇒ 先报一次，页面/算子立刻有分母可看。
        progress(ScanProgress {
            filter: index,
            regions: stats.regions,
            bytes: 0,
            elapsed_ms: started.elapsed().as_millis() as u64,
        });
        for key in &mut order {
            if table.get(key).is_some() {
                continue;
            }
            let Some((pattern_str, offset)) = patterns::anchor_pattern_and_offset(key, stable_table) else {
                continue;
            };
            let pattern = scan::Pattern::parse(pattern_str)
                .map_err(|_| Reason::SignatureMiss(key))?;
            // 同一签名可能出现多次（P1 实测 `configurationAddr` 有 2 处，其中一处只是同形状
            // 代码），所以取列表再**逐个自证**：只有解引用后落在结构上说得通的那一个才算。
            let candidates = scan::find_in_regions(
                target.handle(),
                &regions,
                &pattern,
                offset,
                ANCHOR_HIT_LIMIT,
                &mut stats,
            );
            let topo = stable_table.map(|t| &t.topology);
            let chosen = candidates
                .iter()
                .copied()
                .find(|addr| anchor_proves_out_with_topo(target, *key, *addr, &regions, topo));
            eprintln!(
                "[osu] scan {} -> {} hit(s) {} chosen={:?}",
                key,
                candidates.len(),
                candidates
                    .iter()
                    .map(|a| format!("0x{a:08X}"))
                    .collect::<Vec<_>>()
                    .join(" "),
                chosen.map(|a| format!("0x{a:08X}"))
            );
            match chosen {
                Some(addr) => table.set(key, addr),
                None => {
                    if !degraded.contains(key) {
                        degraded.push(key);
                    }
                }
            }
            // Step 9e：一枚锚点扫完 ⇒ 进度前进一格（`bytes_read` 是真实读出的字节）。
            progress(ScanProgress {
                filter: index,
                regions: stats.regions,
                bytes: stats.bytes_read,
                elapsed_ms: started.elapsed().as_millis() as u64,
            });
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

/// dev-only 自测开关（**默认关闭**，产品路径零差异）：`MMA_OSU_ANCHOR_CACHE_SELFTEST=1` 时，
/// 每次全量扫描成功后立刻按**缓存路径**验证一遍并打印实测耗时。
///
/// 存在的理由（如实记录）：缓存命中的**真实**触发条件是"同一进程实例内的重定址"
/// （读失败重连 / 冻结窗口到期强制重解析），而本轮真机上这两条都没发生（游戏状态良好、
/// 冻结反复被清白帧解除）⇒ 不触碰游戏就量不到"验证耗时"。本开关就是那台测量仪：
/// 它跑的是**产品路径的同一个函数**（`cached_anchors`），只把结果打到日志、不改任何状态。
#[cfg(windows)]
fn anchor_cache_selftest() -> bool {
    matches!(std::env::var("MMA_OSU_ANCHOR_CACHE_SELFTEST"), Ok(value) if value.trim() == "1")
}

/// ②.a 的装配点（**缓存路径**，Step 9c）：算出键 → 命中即只**验证**，不然返回 `None`
/// （调用方落到全量扫描）。
///
/// 返回值里的时间就是**验证耗时**（含区域列表的一次 `VirtualQueryEx` walk，若缓存里还没有
/// 那一档过滤器；同附着内第二次调用只有毫秒级）。
///
/// 三条安全口径都在这里落地：
/// ① 键 = `(exe md5, bitness, module_base)`：键变了 ⇒ [`anchor_cache::AnchorCache::observe`]
///    当场丢掉旧条目（`KeyChange::Dropped`），并记一行日志说明"哪一次加载的地址被丢了"；
/// ② 验证 = **签名复核**（`signature_still_matches`）+ **结构自证**（`anchor_proves_out`），
///    两项都过才算这一枚成立 —— 后者是扫描时"从同形状命中里挑真候选"的同一份判据；
/// ③ 任一不成立 ⇒ `Cache::resolve` 返回 `Stale`、**不会**给出表；这里再清掉已被证伪的条目
///    （留着只会每次白验证一遍），由调用方全量重扫。
#[cfg(windows)]
fn cached_anchors(
    target: &win::Target,
    regions: &mut scan::RegionCache,
    cache: &mut anchor_cache::AnchorCache,
    stable_table: Option<&offsets::StableTable>,
) -> Option<(anchor_cache::AnchorEntry, u128)> {
    use std::time::Instant;
    let key = anchor_cache_key(target)?;
    match cache.observe(&key) {
        anchor_cache::KeyChange::Same => {}
        anchor_cache::KeyChange::First => return None,
        anchor_cache::KeyChange::Dropped(previous) => {
            eprintln!(
                "[osu] anchor cache dropped: image identity changed (md5 {} → {}, bitness 0x{:04X} → 0x{:04X}, module_base 0x{:08X} → 0x{:08X})",
                previous.exe_md5,
                key.exe_md5,
                previous.bitness,
                key.bitness,
                previous.module_base,
                key.module_base
            );
            return None;
        }
    }
    let started = Instant::now();
    let region_list = target.regions_cached(regions, FILTER_READY[0], REGION_LIMIT);
    let topo = stable_table.map(|t| &t.topology);
    let outcome = cache.resolve(&key, |anchor_key, addr| {
        // ① 签名复核：把 match 地址（`addr - offset`；位移只在扫描时施加一次）处的
        //    `pattern.len()` 字节读回来，用台账里同一份掩码签名再匹配一次。
        if !signature_still_matches(target, anchor_key, addr, stable_table) {
            eprintln!(
                "[osu] anchor cache invalid: {anchor_key} signature no longer matches at 0x{addr:08X}"
            );
            return false;
        }
        // ② 结构自证（与扫描时同一份判据）。
        if !anchor_proves_out_with_topo(target, anchor_key, addr, &region_list, topo) {
            eprintln!(
                "[osu] anchor cache invalid: {anchor_key} structure check failed at 0x{addr:08X}"
            );
            return false;
        }
        true
    });
    match outcome {
        anchor_cache::Cached::Validated(entry) => Some((entry, started.elapsed().as_millis())),
        anchor_cache::Cached::Stale(key) => {
            eprintln!(
                "[osu] anchor cache stale at {key} — dropping entry, falling back to a full scan"
            );
            cache.clear();
            None
        }
        anchor_cache::Cached::NoEntry => None,
    }
}

/// 缓存键（Step 9c）：`(exe md5, bitness, module_base)`。
///
/// md5 读的是**磁盘上的映像**（`target.image_path`，`.NET` 静态对象随加载基址/进程实例变化，
/// 所以键必须钉住"哪一次加载"）。读不到文件（换图/更新器正在替换 exe）⇒ `None` ⇒ 本次
/// 不走缓存（**不做**"读不到就当作没变"的近似：那会把另一个构建的地址当成自己的）。
#[cfg(windows)]
fn anchor_cache_key(target: &win::Target) -> Option<anchor_cache::CacheKey> {
    let bytes = std::fs::read(&target.image_path).ok()?;
    Some(anchor_cache::CacheKey::new(
        crate::server::md5_hex_bytes(&bytes),
        target.bitness,
        target.module_base,
    ))
}

/// 缓存地址的**签名复核**：读 `match = addr - offset` 处的 `pattern.len()` 字节，用同一份
/// 掩码签名再匹配一次。读失败/签名不再匹配/台账里没有这个键 ⇒ `false`（该锚点不成立）。
#[cfg(windows)]
fn signature_still_matches(
    target: &win::Target,
    key: &str,
    addr: u32,
    stable_table: Option<&offsets::StableTable>,
) -> bool {
    let Some((pattern_str, offset)) = patterns::anchor_pattern_and_offset(key, stable_table) else {
        return false;
    };
    let Ok(pattern) = scan::Pattern::parse(pattern_str) else {
        return false;
    };
    let mut buf = vec![0u8; pattern.len()];
    // 位移在扫描时已施加一次（`find_in_regions` 的 `hit + offset`），这里逆回去；
    // 用 i64 再截断回 u32（`statusPtr` 的 -0x4 在 u32 上会下溢）。
    let match_addr = (addr as i64 - offset as i64) as u32;
    if win::read_exact_at(target.handle(), match_addr, &mut buf).is_err() {
        return false;
    }
    pattern.matches_at(&buf, 0)
}

/// 逐候选自证（L1 结构校验）：只有"解引用后说得通"的候选才被采纳。
pub fn anchor_proves_out(
    target: &win::Target,
    key: &str,
    addr: u32,
    regions: &[scan::Region],
) -> bool {
    anchor_proves_out_with_topo(target, key, addr, regions, None)
}

pub fn anchor_proves_out_with_topo(
    target: &win::Target,
    key: &str,
    addr: u32,
    regions: &[scan::Region],
    topo: Option<&offsets::StableTopology>,
) -> bool {
    match key {
        "statusPtr" => win::read_pointer(target, addr)
            .map(|value| value <= STATE_INDEX_MAX)
            .unwrap_or(false),
        "baseAddr" => {
            let offset = topo.map(|t| t.beatmap_from_base).unwrap_or(0xC);
            win::read_pointer(target, addr.wrapping_sub(offset))
                .map(|object| object != 0 && region_contains(regions, object))
                .unwrap_or(false)
        }
        "playTimeAddr" => {
            let offset = topo.map(|t| t.play_time_from_anchor).unwrap_or(0x5);
            match win::read_u32(target, addr.wrapping_add(offset)) {
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
            }
        }
        "rulesetsAddr" => {
            let anchor_off = topo.map(|t| t.ruleset_from_anchor).unwrap_or(0xB);
            let list_off = topo.map(|t| t.ruleset_list_offset).unwrap_or(0x4);
            match win::read_u32(target, addr.wrapping_sub(anchor_off)) {
                Ok(slot) => {
                    if let Some(why) = plausible_object(regions, slot) {
                        eprintln!(
                            "[osu] diag {key} candidate=0x{addr:08X} reject=slot slot=0x{slot:08X} why={why}"
                        );
                        return false;
                    }
                    match win::read_u32(target, slot.wrapping_add(list_off)) {
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
            }
        }
        "menuModsPtr" => win::read_u32(target, addr)
            .map(|pointer| pointer != 0 && region_contains(regions, pointer))
            .unwrap_or(false),
        "getAudioLengthPtr" => {
            let offset = topo.map(|t| t.mp3_length_from_anchor).unwrap_or(0x7);
            match win::read_u32(target, addr.wrapping_add(offset)) {
                Ok(slot) => match plausible_object(regions, slot) {
                    None => match win::read_u32(target, slot) {
                        Ok(0) => true,
                        Ok(obj) => match plausible_object(regions, obj) {
                            None => true,
                            Some(why) => {
                                eprintln!(
                                    "[osu] diag {key} candidate=0x{addr:08X} reject=object obj=0x{obj:08X} why={why}"
                                );
                                false
                            }
                        },
                        Err(_) => false,
                    },
                    Some(why) => {
                        eprintln!(
                            "[osu] diag {key} candidate=0x{addr:08X} reject=slot slot=0x{slot:08X} why={why}"
                        );
                        false
                    }
                },
                Err(_) => false,
            }
        }
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
    stable_table: Option<&offsets::StableTable>,
    previous_live: Option<i32>,
) -> Result<FrameRead, Reason> {
    let mut snapshot = Snapshot {
        client: Some(Client::Stable),
        pid: target.pid,
        ..Default::default()
    };
    let mut degraded: Vec<String> = Vec::new();
    let topo = stable_table.map(|t| &t.topology);

    if let Some(status_ptr) = table.status_ptr {
        let raw = win::read_pointer(target, status_ptr)?;
        let index = raw as i32;
        snapshot.state_number = Some(index);
        let name = stable_table
            .and_then(|t| t.state_name(index))
            .unwrap_or_else(|| model::state_name_for(index));
        snapshot.state_name = Some(name.to_string());
        // I-01：观测集外 → 发布 "" 并上报降级（**不**判 unhealthy）——那是 L2 的事，
        // 这里只保证"名字表之外的整数不会变成编造的名字"。
    }

    if let Some(play_time_addr) = table.play_time_addr {
        match stable::read_play_time_with_topo(target, play_time_addr, topo) {
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
        let beatmap_offset = topo.map(|t| t.beatmap_from_base).unwrap_or(stable::BEATMAP_FROM_BASE);
        let beatmap_addr = base_addr.wrapping_sub(beatmap_offset);
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
            let md5_off = topo.map(|t| t.beatmap_md5).unwrap_or(0x6C);
            let fn_off = topo.map(|t| t.beatmap_filename).unwrap_or(0x90);
            let fold_off = topo.map(|t| t.beatmap_folder).unwrap_or(0x78);
            let ver_off = topo.map(|t| t.beatmap_version).unwrap_or(0xAC);
            let art_off = topo.map(|t| t.beatmap_artist).unwrap_or(0x18);
            let tit_off = topo.map(|t| t.beatmap_title).unwrap_or(0x24);
            let map_off = topo.map(|t| t.beatmap_mapper).unwrap_or(0x7C);
            let map_id_off = topo.map(|t| t.beatmap_id).unwrap_or(0xC8);
            let set_id_off = topo.map(|t| t.beatmap_set_id).unwrap_or(0xCC);

            snapshot.checksum = read_string(md5_off, "beatmap.md5");
            snapshot.filename = read_string(fn_off, "files.beatmap");
            snapshot.folder = read_string(fold_off, "folders.beatmap");
            snapshot.version = read_string(ver_off, "beatmap.version");
            snapshot.artist = read_string(art_off, "beatmap.artist");
            snapshot.title = read_string(tit_off, "beatmap.title");
            snapshot.mapper = read_string(map_off, "beatmap.mapper");
            snapshot.map_id = win::read_i32(target, object.wrapping_add(map_id_off)).ok();
            snapshot.set_id = win::read_i32(target, object.wrapping_add(set_id_off)).ok();
            snapshot.map_id_bits = win::read_u32(target, object.wrapping_add(map_id_off)).ok();
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
        match stable::resolve_ruleset_with_topo(target, rulesets_addr, topo) {
            Ok((ruleset, chain_probe)) => {
                snapshot.ruleset_base = Some(ruleset);
                probe = Some(chain_probe);
                if let Some(base_addr) = table.base_addr {
                    let in_game = stable::read_in_game_with_topo(target, ruleset, base_addr, topo);
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
                let result = stable::read_result_with_topo(target, ruleset, topo);
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
        match stable::read_mp3_length_with_topo(target, audio_length_ptr, topo) {
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
    apply_hits_with_topo(snapshot, None);
}

pub fn apply_hits_with_topo(snapshot: &mut Snapshot, topo: Option<&offsets::StableTopology>) {
    snapshot.play_hits = if invariants::play_hits_publishable(snapshot) {
        stable::hits_from_candidates_topo(&snapshot.hits_candidates, topo)
    } else {
        None
    };
    snapshot.result_hits = if invariants::result_hits_publishable(snapshot) {
        stable::hits_from_candidates_topo(&snapshot.result_hits_candidates, topo)
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
    let previous_bg = snapshot.background.clone();
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
            if snapshot.client == Some(Client::Lazer) {
                snapshot.background = previous_bg.or_else(|| {
                    lazer::find_background_file(
                        &snapshot.lazer_files,
                        file.background.as_deref(),
                    )
                });
            } else {
                snapshot.background = file.background.clone();
            }
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
    _stable_table: Option<&offsets::StableTable>,
    _progress: &mut dyn FnMut(ScanProgress),
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
        // Step 9g：本帧是不是保持帧（载荷 = 最后一张好图 + 本帧 state）。
        "holding": state.holding,
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
        // Step 10B：lazer 诊断（表键/出处/gameBase/marker 命中/链地址/降级清单）；
        // stable 目标上恒为 `null`。
        "lazer": state.lazer.as_ref().map(lazer::Diagnostics::to_json),
    })
}

#[cfg(test)]
#[path = "../../tests-local/osu_beatmap_corpus.rs"]
mod tests_beatmap_corpus;
// Step 9g：身份保持的**装配**用例（门请求 → 读者供奉；见文件头）。
#[cfg(all(test, windows))]
#[path = "../../tests-local/osu_hold.rs"]
mod tests_hold;
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
#[cfg(test)]
#[path = "../../tests-local/osu_anchor_cache.rs"]
mod tests_anchor_cache;
#[cfg(test)]
#[path = "../../tests-local/osu_sweep.rs"]
mod tests_sweep;
#[cfg(test)]
#[path = "../../tests-local/osu_phase.rs"]
mod tests_phase;
