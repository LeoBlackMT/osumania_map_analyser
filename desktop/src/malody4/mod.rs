// Malody 4.3.7（旧原生 32 位客户端）数据源：纯逻辑层 + 壳侧 poller。
//
// - anchor：版本表（RVA / PE 时间戳 / 文件大小）、身份键解析、PE 版本校验、
//   设置单例（判定档 / 变速位）的内存读取
// - config：config.json 只读解析（只取 user_mods / user_judge_level）
// - gamelog：日志场景行解析（switch to N）+ 日志目录枚举（`latest_log`）
// - library：<root>/beatmap/** 索引（流式 md5 + 有界前缀 meta 提取）+ 树指纹（只读元数据的
//   变更探测）——后台构建线程独占写入
// - model：Screen / ModFlags / JudgeLevel / Selection / UnavailableReason
// - selection：selection 状态机（防抖 + 心跳 + 不可用原因）
// - runtime：后台 poller 轮询、索引构建线程、进程附着与 selection/song 帧分发
//
// 本模块的阈值全部集中在这里，任何子模块都不得各写一份数字。
//
// poller 的 tick 顺序（对应 Step 4 的伪码，逐条见 `.omo/evidence/malody4-source/task-4-loop.txt`）：
//   ① attach（惰性，2s 重试）→ ② 根目录解析链 → ③ 库变更同步（`content_revision`，放开
//   "本会话不可解析"的身份键）→ ④ 日志尾随取 screen
//   → ⑤ 锚点读取（硬失败立即 detach；软失败容忍 `SOFT_READ_TOLERANCE` 次）→ ⑥ 索引 lookup
//   → ⑦ 判定档 / 变速位（**内存优先**，config.json 兜底）→ ⑧ selection.fold
//   → ⑨ dispatch_plan 发帧 → ⑩ 状态更新（跳变时即时推 state 帧）。
//
// 主循环内**不做文件系统遍历**：日志目录枚举在 `gamelog::latest_log`、索引遍历与周期重扫的
// 指纹扫描都在后台构建线程（`library::rebuild` / `library::rescan_change`），本文件只调用它们。

pub mod anchor;
pub mod spec;
pub mod config;
pub mod gamelog;
pub mod library;
pub mod model;
pub mod runtime;
pub mod selection;
pub mod settings;

pub use self::runtime::*;
pub use self::settings::{
    chain_disagreement_warning, effective_settings, file_cross_check_warning, settings_log,
    EffectiveSettings, SettingsLogKey, SettingsOrigin,
};

use crate::etterna::EtternaStatus;
use crate::frames::{
    EtternaSource, Malody4Source, MalodySource, SourcesFrame, StateFrame,
};
use self::anchor::{AnchorError, IdentityKey};
// `RawSettings` / `MemorySource` moved into `settings.rs`'s import list but stay part of
// this module's public surface: the local test module glob-imports them from here.
pub use self::anchor::{MemorySource, RawSettings};
use self::library::{ChartLibrary, ChartMeta, LibraryEntry, LibraryFingerprint, LibraryStats};
use self::model::{Selection, UnavailableReason};
use self::selection::Action;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// 主循环采样间隔（每 tick 一帧：锚点 + 日志 + config）。
pub const POLL_INTERVAL: Duration = Duration::from_millis(200);
/// 选中内容变化后的防抖窗口。
pub const DEBOUNCE: Duration = Duration::from_millis(300);
/// 心跳重发窗口（页面侧据此判定本源是否已离场）。
pub const HEARTBEAT: Duration = Duration::from_secs(2);
/// `POLL_INTERVAL` 的语义别名（供 selection 的"每 tick"判断引用，不另写字面量）。
pub const TICK: Duration = POLL_INTERVAL;
/// 速率比较的容差（`|Δ| < RATE_EPS` 视为同一速率）。
pub const RATE_EPS: f64 = 1e-5;
/// 索引周期重扫间隔（判定走 `library::rescan_change` 的元数据指纹，只有树真的变了才重建）。
pub const INDEX_RESCAN: Duration = Duration::from_secs(60);
/// 同一失败原因的日志节流窗口。
pub const ACTION_LOG_THROTTLE: Duration = Duration::from_secs(30);
/// 未附着时的重新 attach 间隔（惰性 attach）。
pub const ATTACH_RETRY: Duration = Duration::from_secs(2);
/// 索引 miss 触发的重建请求节流（全局）。
pub const INDEX_REBUILD_THROTTLE: Duration = Duration::from_secs(5);
/// 同一个 md5 允许的重建尝试次数（用完即判"本会话不可解析"，不再请求重建、不再记日志）。
///
/// 为什么封顶：实测现场 25 个身份键里只有 6 个在盘上，剩下 19 个永远查不到；旧行为只受
/// `INDEX_REBUILD_THROTTLE` 节流、**永不放弃**，于是整库（~22 MB）每 5s 被重哈希一次
/// （一个会话刷出 22 代索引，日志里全是 `index miss ... rebuild requested` + `index built`）。
/// 为什么是 2 而不是 1：第 2 次尝试覆盖"文件在第 1 次重建之后、下一次周期重扫之前落盘"
/// （最长 60s）的窗口；库**真的**变了时预算会整体重置（见 `MissRebuildTracker::reset`），
/// 因此 2 不会被浪费在"树根本没变"的重哈希上耗光。
pub const MISS_REBUILD_ATTEMPTS: u32 = 2;
/// `playing` 的新鲜窗口：场景为 Playing 且最后一次场景切换在此窗口内。
pub const PLAYING_FRESH: Duration = Duration::from_secs(10);
/// `user_mods` 的 FAIR 判定模组位（桌面版 `JudgeKeyFair`）。
///
/// FAIR 用的是**更宽**的一组判定窗口（C 档 45/85/120 vs 默认 36/76/110），而本源的 OD 表
/// 只按"判定档 × 速率"建模 → 开了 FAIR 的玩家会被按更严的默认组换算，OD 偏高约 2.9。
/// 本轮不建模 FAIR（用户的等效表材料亦未覆盖）。`user_mods` 只存在于壳读到的 `config.json`，
/// **从不进入任何帧**，因此这条提示只能由壳日志发出——页面没有任何数据通路可以发它。
pub const MOD_JUDGE_KEY_FAIR: u64 = 0x400;
/// FAIR 判定模组的 warning 文案（加壳日志是"绝不静默"的落点）。
pub const FAIR_JUDGE_WARNING: &str =
    "malody4: FAIR judge mod detected - the OD table does not model it (OD may be overestimated)";
/// 连续**软**锚点读取失败的容忍次数（10 × `POLL_INTERVAL` ≈ 2s）。
///
/// 硬失败（进程 / 模块 / 句柄已不在）立即 detach；软失败（锚点指针槽位读到了、但内容当前
/// 不是身份键，如游戏过场瞬间）只计数，连续达到此数才判定通道真的坏了。
pub const SOFT_READ_TOLERANCE: u32 = 10;

/// `user_mods` 是否置了 FAIR 判定模组位（纯函数，便于单测）。
pub fn fair_judge_mod(user_mods: u64) -> bool {
    user_mods & MOD_JUDGE_KEY_FAIR != 0
}

/// 附着成功后用于"内存 vs `config.json`"交叉校验的窗口。
///
/// `config.json` 的这两个键正是从内存里那两个偏移写出去的，因此**刚附着**时两者应当一致；
/// 本局中途改判定造成的分歧正是本次改动的理由。这个窗口只决定"要不要记一条诊断告警"，
/// **绝不是门**：窗口内的分歧也不拒绝、不覆盖内存值（见 `note_file_cross_check`）。
pub const SETTINGS_CROSS_CHECK: Duration = Duration::from_secs(30);
/// `S` / `P` 两条链连续不一致多少拍才记那条一次性告警（"> 两拍"）。
///
/// 取 3 而不是 1：判定档设置器把两个对象写在相隔 4 条指令处，而壳读它们要走两次独立的
/// `ReadProcessMemory`，单拍的不一致可能只是"两次系统调用之间游戏真的改了一次设置"。
pub const CHAIN_DISAGREE_POLLS: u32 = 3;

// ---------------------------------------------------- 锚点读取：硬失败 / 软失败 --

/// 一次锚点读取对本 tick 的处置（纯函数 `read_action` 的产物）。
#[derive(Debug, Clone, PartialEq)]
pub enum ReadAction {
    /// 读到了身份键（`Some`）或确认当前没选中谱面（`None`）→ 通道健康，软失败计数清零。
    Healthy(Option<IdentityKey>),
    /// 容忍窗口内的**软**失败（锚点指针槽位读到了、但内容当前不是身份键）→ 本 tick 按
    /// "没选中谱面"处理（`Hidden(NoSelection)`），通道保留。
    Tolerated,
    /// 该 detach：**硬**失败（进程 / 模块 / 句柄已不在）立即，**软**失败要连续
    /// `SOFT_READ_TOLERANCE` 次之后。
    Detach(AnchorError),
}

/// 连续软失败计数器（纯逻辑、无 IO；实例住在 `Runtime` 的 `soft_reads` 字段里）。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SoftReadTolerance {
    consecutive: u32,
}

impl SoftReadTolerance {
    pub fn new() -> Self {
        SoftReadTolerance::default()
    }

    /// 当前连续软失败次数（诊断与单测用）。
    pub fn consecutive(&self) -> u32 {
        self.consecutive
    }

    /// 记一次软失败；返回是否已达 `SOFT_READ_TOLERANCE`（该 detach）。
    pub fn soft_failure(&mut self) -> bool {
        self.consecutive = self.consecutive.saturating_add(1);
        self.consecutive >= SOFT_READ_TOLERANCE
    }

    /// 一次成功读取或一次 detach（重新开始一段通道）→ 清零。
    pub fn reset(&mut self) {
        self.consecutive = 0;
    }
}

/// 读结果 → 本 tick 的处置（纯函数，硬 / 软的分界见 `anchor::IdentityRead`）。
///
/// 硬失败不计容忍、立即 detach；软失败只计数，连续达到 `SOFT_READ_TOLERANCE` 才 detach。
pub fn read_action(
    tolerance: &mut SoftReadTolerance,
    result: Result<anchor::IdentityRead, AnchorError>,
) -> ReadAction {
    match result {
        Ok(anchor::IdentityRead::Key(key)) => {
            tolerance.reset();
            ReadAction::Healthy(Some(key))
        }
        // 指针为 0 = 游戏在跑但没选中谱面：正常态，同样清零
        Ok(anchor::IdentityRead::Empty) => {
            tolerance.reset();
            ReadAction::Healthy(None)
        }
        // 软失败：指针槽位已读成功 ⇒ 进程与模块仍在，只计数
        Ok(anchor::IdentityRead::Unparsable) => {
            if tolerance.soft_failure() {
                ReadAction::Detach(AnchorError::BadRead)
            } else {
                ReadAction::Tolerated
            }
        }
        // 硬失败：进程 / 模块 / 句柄已不在（或平台不支持）→ 立即 detach
        Err(err) => ReadAction::Detach(err),
    }
}

/// Malody 4 源状态（`shared.malody4` 的内容，与 `etterna::EtternaStatus` 同角色）。
///
/// `reason` 只允许装 `UnavailableReason::as_str()` 的闭集字面量；健康/空闲（未选中谱面）
/// 时清空为空串，绝不残留上一次的失败原因。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Malody4Status {
    pub alive: bool,
    pub screen: String,
    pub playing: bool,
    pub reason: String,
    pub judge: Option<char>,
}

// ------------------------------------------------------------------ state 帧 --

/// state 帧组装（纯函数；`server::state_frame` 是它的薄封装）。
///
/// 用 `frames::SourcesFrame` 结构体构造：既有的 `etterna`（含 `playingExpireAt`）与
/// `malody.alive` 语义保持不变，新增的 `malody4` 不可能被漏掉。
pub fn build_state_frame(
    tosu_online: bool,
    errors: &[String],
    etterna: &EtternaStatus,
    malody_alive: bool,
    malody4: &Malody4Status,
) -> serde_json::Value {
    let frame = StateFrame {
        tosu_online,
        errors: errors.to_vec(),
        sources: SourcesFrame {
            etterna: EtternaSource {
                alive: etterna.alive,
                playing: etterna.playing,
                playing_expire_at: etterna.playing_expire_at,
            },
            malody: MalodySource {
                alive: malody_alive,
            },
            malody4: Malody4Source {
                alive: malody4.alive,
                playing: malody4.playing,
                screen: malody4.screen.clone(),
                reason: malody4.reason.clone(),
                judge: malody4.judge,
            },
        },
    };
    serde_json::to_value(frame).unwrap_or(serde_json::Value::Null)
}

/// state 帧的**立即补发**判定（纯函数）：页面消费的有效值里任一变化 → `true`。
///
/// `alive` / `playing` 之外必须逐字段比：兜底的周期帧是 30s 一次（`server::spawn_timers` 里
/// `TOSU_PROBE_INTERVAL` 那一拍），页面等不起。`reason` 尤其
/// ——`chart-unknown-identity`（本地库里查不到这张谱）既不改 `alive` 也不改 `playing`，只比后两者时
/// 页面要等下一次周期帧才知道，用户坐在卡片上什么提示都看不到（真机复测就是这个形态）。
/// `screen` / `judge` 同样进帧，页面按它们切卡片内容，一并纳入。
///
/// 逐字段比较 ⇒ 不变的 tick 返回 `false`：**绝不每 tick 广播**（`broadcast` 要 clone 整帧并推给
/// 每个 WS sink）。字段表必须与 `build_state_frame` 实际输出的字段保持一致。
pub fn state_push_due(prev: &Malody4Status, next: &Malody4Status) -> bool {
    prev.alive != next.alive
        || prev.playing != next.playing
        || prev.screen != next.screen
        || prev.reason != next.reason
        || prev.judge != next.judge
}

// -------------------------------------------------------------- 帧分派（纯） --

/// 本 tick 要发的帧（`None` = 明确什么都不发）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dispatch {
    SelectionFrame,
    SongFrame,
    /// 什么都不发（`Action::None`）。
    None,
}

/// song 帧的去重状态：上次**实际发出**的签名与已服务过的最大 WS 连接 id。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SongDispatchState {
    /// 签名 = `(identity, speed_rate, judge)`。刻意**不含 `screen`**：场景不参与页面缓存键，
    /// 把场景算进签名会让 `selection→playing→result` 的每次跳变都重发 song 帧、作废在途分析。
    pub last_signature: Option<(String, f64, Option<char>)>,
    pub last_conn_id: Option<u64>,
}

/// 分派计划（纯函数，便于单测"心跳不重发 song 帧"）。
///
/// 规则（与 Step 4 逐条对应）：
/// - `Action::Emit` / `Action::Hidden` 各发恰好一条 `malody4_selection`；`Action::None` 不发。
/// - song 帧**只有两个触发条件（取或）**：(a) 签名 `(identity, rate, judge)` 变化、
///   (b) 出现更大的 WS 连接 id（严格递增比较；`ws.rs` 的 `retain` 可能移除大 id，
///   写成 `!=` 会把"连接被移除"误判成"新连接"）。
/// - 心跳**本身**不是重发理由（同签名 + 同连接 ⇒ 只发 selection）；但"页面后连/刷新"
///   带来的更大连接 id 是真信号，此时即便本 tick 的 event 是 heartbeat 也必须补发一次
///   song 帧（否则"壳先起、页面后连"永远拿不到当前谱面）。
/// - `judge` 为 `None`（config.json 缺失/不可解析，本 tick 判定不可确定）⇒ **不发 song 帧**：
///   发 `judge: null` 会让页面静默回落到 C 档 8.08，把"读到半截配置"变成一个错误的 OD。
/// - **没有周期兜底重发**（`SONG_RESEND = 30s` 方案已明确否决：页面 song 路径无去重，
///   任何同谱重发都会让用户看到周期性闪烁/重算）。
pub fn dispatch_plan(
    action: &Action,
    st: &mut SongDispatchState,
    conn: Option<u64>,
    judge: Option<char>,
) -> Vec<Dispatch> {
    let selection = match action {
        Action::None => return vec![Dispatch::None],
        Action::Hidden(_) => None,
        Action::Emit(selection) => Some(selection),
    };
    let mut plans = vec![Dispatch::SelectionFrame];
    if let Some(selection) = selection {
        if judge.is_some() && song_frame_due(st, selection, conn, judge) {
            plans.push(Dispatch::SongFrame);
        }
    }
    plans
}

/// song 帧的两个（且仅有）触发条件；命中时记录签名/连接 id。
pub fn song_frame_due(
    st: &mut SongDispatchState,
    selection: &Selection,
    conn: Option<u64>,
    judge: Option<char>,
) -> bool {
    let signature = (
        format!("mdy4:{}", selection.chart_hash),
        selection.speed_rate,
        judge,
    );
    let signature_changed = st.last_signature.as_ref() != Some(&signature);
    let larger_conn = match (conn, st.last_conn_id) {
        (Some(max_id), Some(last)) => max_id > last,
        (Some(_), None) => true,
        (None, _) => false,
    };
    if !signature_changed && !larger_conn {
        return false;
    }
    st.last_signature = Some(signature);
    if let Some(max_id) = conn {
        st.last_conn_id = Some(max_id);
    }
    true
}

// ------------------------------------------------------------ 索引共享句柄 --

/// poller 与索引构建线程之间的私有共享。
///
/// **不进 `Shared`**（`Shared` 只持有 `Malody4Status`）：`idx.lib` 的唯一写者是构建线程，
/// poller 只读且不持锁做别的事。重建请求只置位、周期重扫与指纹扫描都在构建线程里做，
/// 200ms 主循环绝不遍历文件系统。
pub struct IndexShared {
    pub lib: Mutex<ChartLibrary>,
    pub rebuild_requested: AtomicBool,
    pub generation: AtomicU64,
    /// "库真的变了"的修订号：只在**周期重扫判定为变化**或**根目录换了**（旧索引整体作废）
    /// 时 +1。miss 触发的重建**不**计数——它消耗的是重试预算，不是"库变了"的证据。
    /// poller 据此放开此前判定"本会话不可解析"的身份键（见 `MissRebuildTracker::reset`）。
    pub content_revision: AtomicU64,
}

impl IndexShared {
    pub fn new() -> Self {
        IndexShared {
            lib: Mutex::new(empty_library()),
            rebuild_requested: AtomicBool::new(false),
            generation: AtomicU64::new(0),
            content_revision: AtomicU64::new(0),
        }
    }
}

/// 构建完成过一次之前，索引一律按"未就绪"处理（`reason = no-library`）。
pub fn index_ready(idx: &IndexShared) -> bool {
    idx.generation.load(Ordering::Relaxed) > 0
}

pub fn empty_library() -> ChartLibrary {
    ChartLibrary {
        root: PathBuf::new(),
        by_md5: HashMap::new(),
        meta: HashMap::new(),
        built_at: Instant::now(),
        fingerprint: LibraryFingerprint::default(),
        stats: LibraryStats::default(),
    }
}

/// 置"重建请求"位（调用方负责节流；本函数不碰文件系统）。
pub fn request_rebuild(idx: &IndexShared) {
    idx.rebuild_requested.store(true, Ordering::Relaxed);
}

/// 查索引：克隆一份 entry，调用方不持锁。
pub fn lookup(idx: &IndexShared, md5: &str) -> Option<LibraryEntry> {
    idx.lib.lock().ok()?.lookup(md5).cloned()
}

/// 取一行 `meta`（克隆，不持锁）：song 帧的 title/artist/version/keys 与 selection 的
/// `version` 都从这里取，**不再为每帧重读谱面文件**。
pub fn meta_of(idx: &IndexShared, path: &Path) -> Option<ChartMeta> {
    idx.lib.lock().ok()?.meta.get(path).cloned()
}

/// 当前索引的条目数（`stats.indexed`）：miss 日志用它区分"库里没有这张谱"与"库还是空的"。
pub fn indexed_count(idx: &IndexShared) -> usize {
    idx.lib.lock().map(|lib| lib.stats.indexed).unwrap_or(0)
}

/// 当前索引的树指纹（克隆，不持锁做别的事）：周期重扫拿它当"变了没有"的基准。
pub fn index_fingerprint(idx: &IndexShared) -> LibraryFingerprint {
    idx.lib
        .lock()
        .map(|lib| lib.fingerprint.clone())
        .unwrap_or_default()
}

// -------------------------------------------------------------- 状态原因与映射 --

/// 线上 `reason` 的选取（纯函数，闭集见 `UnavailableReason::as_str`）。
pub fn wire_reason(blocker: Option<&UnavailableReason>, action: &Action, previous: &str) -> String {
    if let Some(blocker) = blocker {
        return blocker.as_str();
    }
    match action {
        Action::Hidden(hidden) => hidden.as_str(),
        Action::Emit(_) => String::new(),
        Action::None => previous.to_string(),
    }
}

/// `AnchorError` → 线上不可用原因（`reason` 闭集）。
pub fn unavailable_from_anchor(err: AnchorError) -> UnavailableReason {
    match err {
        AnchorError::NotFound => UnavailableReason::ProcessNotFound,
        AnchorError::MultipleInstances => UnavailableReason::MultipleInstances,
        AnchorError::AccessDenied => UnavailableReason::AccessDenied,
        AnchorError::BadRead => UnavailableReason::BadRead,
        AnchorError::TargetMismatch(detail) => UnavailableReason::TargetMismatch(detail),
        AnchorError::PlatformUnsupported => UnavailableReason::PlatformUnsupported,
    }
}

#[cfg(test)]
#[path = "../../tests-local/malody4_mod.rs"]
mod tests;
