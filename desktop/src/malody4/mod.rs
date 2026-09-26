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
pub mod config;
pub mod gamelog;
pub mod library;
pub mod model;
pub mod selection;
pub mod settings;
pub use self::settings::{
    chain_disagreement_warning, effective_settings, file_cross_check_warning, settings_log,
    EffectiveSettings, SettingsLogKey, SettingsOrigin,
};

use crate::etterna::EtternaStatus;
use crate::frames::{
    EtternaSource, Malody4Source, MalodySource, SourcesFrame, StateFrame, MAX_PAYLOAD_BYTES,
};
use crate::server::log::log_at;
use crate::server::{broadcast, Shared};
use self::anchor::{AnchorError, IdentityKey, MemoryProbe};
// `RawSettings` / `MemorySource` moved into `settings.rs`'s import list but stay part of
// this module's public surface: the local test module glob-imports them from here.
pub use self::anchor::{MemorySource, RawSettings};
use self::config::GameConfig;
use self::gamelog::SceneTracker;
use self::library::{ChartLibrary, ChartMeta, LibraryEntry, LibraryFingerprint, LibraryStats};
use self::model::{Screen, Selection, UnavailableReason};
use self::selection::{Action, Availability, SelectionState};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

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
    fn soft_failure(&mut self) -> bool {
        self.consecutive = self.consecutive.saturating_add(1);
        self.consecutive >= SOFT_READ_TOLERANCE
    }

    /// 一次成功读取或一次 detach（重新开始一段通道）→ 清零。
    fn reset(&mut self) {
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
fn state_push_due(prev: &Malody4Status, next: &Malody4Status) -> bool {
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
fn song_frame_due(
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
fn index_ready(idx: &IndexShared) -> bool {
    idx.generation.load(Ordering::Relaxed) > 0
}

fn empty_library() -> ChartLibrary {
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
fn indexed_count(idx: &IndexShared) -> usize {
    idx.lib.lock().map(|lib| lib.stats.indexed).unwrap_or(0)
}

/// 当前索引的树指纹（克隆，不持锁做别的事）：周期重扫拿它当"变了没有"的基准。
fn index_fingerprint(idx: &IndexShared) -> LibraryFingerprint {
    idx.lib
        .lock()
        .map(|lib| lib.fingerprint.clone())
        .unwrap_or_default()
}

/// 索引构建线程：`idx.lib` 的唯一写者；根目录由 poller 经 channel 送达。
///
/// 两条重建理由：
/// - **请求**（`rebuild_requested`，只由 miss 置位）：强制执行。miss 是"指纹可能看不见变化"
///   的唯一实证（盲区见 `library::LibraryFingerprint`），故不吃指纹这一关；次数由 poller 的
///   `MISS_REBUILD_ATTEMPTS` 预算封顶。
/// - **周期**（每 `INDEX_RESCAN`）：只走一遍**元数据**指纹（`library::rescan_change`），与上次
///   构建一致就完全不动（不哈希、不计入 `generation`、不发 `index built`），只记一条 debug。
fn index_build_loop(idx: Arc<IndexShared>, root_rx: mpsc::Receiver<PathBuf>) {
    let mut root: Option<PathBuf> = None;
    let mut built_root: Option<PathBuf> = None;
    let mut last_rescan: Option<Instant> = None;
    loop {
        match root_rx.recv_timeout(POLL_INTERVAL) {
            Ok(new_root) => root = Some(new_root),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            // poller 线程结束（进程收尾）→ 本线程收工
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
        while let Ok(new_root) = root_rx.try_recv() {
            root = Some(new_root);
        }
        let requested = idx.rebuild_requested.swap(false, Ordering::Relaxed);
        let Some(dir) = root.clone() else { continue };
        // 首次拿到根目录、或根目录换了（旧索引整体作废）→ 立即建一次
        let stale = built_root.as_deref() != Some(dir.as_path());
        let rescan_due = last_rescan
            .map(|at| at.elapsed() >= INDEX_RESCAN)
            .unwrap_or(true);
        // 到点且没有别的重建理由 → 只读元数据地判定"树变了没有"（绝不在这里哈希）
        let scanned = rescan_due && !stale && !requested;
        let changed = scanned && library::rescan_change(&dir, &index_fingerprint(&idx)).is_some();
        if rescan_due {
            last_rescan = Some(Instant::now());
        }
        if !requested && !stale && !changed {
            if scanned {
                // 每个重扫周期一条（60s），不是每 tick 一条：库没变就不该有重建噪声
                log_at(
                    "debug",
                    &format!(
                        "malody4 index rescan: root={} unchanged (metadata fingerprint identical) — rebuild skipped",
                        dir.display()
                    ),
                );
            }
            continue;
        }
        let started = Instant::now();
        let rebuilt = library::rebuild(&dir);
        let stats = rebuilt.stats.clone();
        *idx.lib.lock().unwrap() = rebuilt;
        let generation = idx.generation.fetch_add(1, Ordering::Relaxed) + 1;
        // 树确实变了（周期重扫检出 / 根目录换了）→ 放开此前判定"不可解析"的身份键：
        // 它们可能正是这次新增的文件。miss 触发的重建不在此列（见 `content_revision`）。
        if changed || stale {
            idx.content_revision.fetch_add(1, Ordering::Relaxed);
        }
        // stats 全字段：用于区分"库里没有这张谱"与"被过滤掉了"。
        // info 级（默认 logLevel=info 可见）：这是判断索引是否建成、收了多少张的唯一现场证据。
        log_at(
            "info",
            &format!(
                "malody4 index built: root={} indexed={} skipped_non_key={} skipped_osu_not_mania={} skipped_osu_no_keys={} skipped_junk={} skipped_ext={} skipped_unparsed={} duplicate_md5={} elapsed_ms={} generation={}",
                dir.display(),
                stats.indexed,
                stats.skipped_non_key,
                stats.skipped_osu_not_mania,
                stats.skipped_osu_no_keys,
                stats.skipped_junk,
                stats.skipped_ext,
                stats.skipped_unparsed,
                stats.duplicate_md5,
                started.elapsed().as_millis(),
                generation
            ),
        );
        built_root = Some(dir);
    }
}

// ------------------------------------------------------------------- poller --

/// 已 attach 的目标：`Target` 持进程句柄（基址 / 映像路径 / 磁盘文件大小的来源），
/// `Attachment` 持读取句柄。任一次读取失败 → 整体丢弃（两者 `Drop` 各自关闭句柄）。
struct Attached {
    target: anchor::Target,
    attachment: anchor::Attachment,
}

impl Attached {
    fn process_exe(&self) -> &Path {
        &self.target.exe_path
    }
}

/// 日志尾随状态：`(文件路径, 已读字节偏移, 半行残留)`。
struct LogTail {
    path: Option<PathBuf>,
    offset: u64,
    partial: String,
}

impl LogTail {
    fn new() -> Self {
        LogTail {
            path: None,
            offset: 0,
            partial: String::new(),
        }
    }

    /// 尾随最新日志文件：新文件 → 从 0 读起；旧文件被删且无后继 → 保留最后已知 `screen`
    /// （`playing` 由 `fresh(PLAYING_FRESH)` 自然失鲜）。
    fn follow(&mut self, dir: &Path, tracker: &mut SceneTracker) {
        let Some(latest) = gamelog::latest_log(dir) else {
            return;
        };
        if self.path.as_deref() != Some(latest.as_path()) {
            self.path = Some(latest.clone());
            self.offset = 0;
            self.partial.clear();
        }
        self.read_new(&latest, tracker);
    }

    /// 按字节读新增内容（**绝不整文件 read_to_string**），半行留到下一 tick 再拼。
    fn read_new(&mut self, path: &Path, tracker: &mut SceneTracker) {
        let Ok(len) = fs::metadata(path).map(|meta| meta.len()) else {
            return;
        };
        if len < self.offset {
            // 文件被截断 / 同名替换：从头重读
            self.offset = 0;
            self.partial.clear();
        }
        if len == self.offset {
            return;
        }
        let Ok(mut file) = fs::File::open(path) else {
            return;
        };
        if file.seek(SeekFrom::Start(self.offset)).is_err() {
            return;
        }
        let mut buf = Vec::new();
        if file.read_to_end(&mut buf).is_err() {
            return;
        }
        self.offset += buf.len() as u64;
        let mut combined = std::mem::take(&mut self.partial);
        combined.push_str(&String::from_utf8_lossy(&buf));
        let mut lines: Vec<&str> = combined.split('\n').collect();
        let tail = lines.pop().unwrap_or_default().to_string();
        for line in lines {
            tracker.apply_line(line.trim_end_matches('\r'));
        }
        self.partial = tail;
    }
}

/// `config.json` 的解析缓存：**按字节内容**判定是否变化，`(mtime, 长度)` 只作廉价提示。
///
/// 游戏在开局与退出时把 `config.json` 整份重写（`CREATE_ALWAYS`）。只比 `(mtime, 长度)`
/// 会漏掉"两次重写落进同一文件系统时间戳粒度（且长度相同）"的情形——那会让整个新会话的
/// 判定档 / 变速位停留在旧值上（OD 直接错）。文件只有 ~3.2 KB，每 tick 读一次的代价远低于
/// 一次错判，因此这里**每 tick 都读字节**、以字节比较为准；字节与 `(mtime, 长度)` 都没变
/// 时连解析都省掉。解析结果只在**真的不同**时才替换缓存值，下游 `judge` / `rate` 因而
/// 在内容不变的 tick 上恒定。
#[derive(Default)]
struct ConfigCache {
    sig: Option<(SystemTime, u64)>,
    /// 上次**接受**的原始字节（内容判据的基准；`None` = 文件不存在或读不出来）。
    bytes: Option<Vec<u8>>,
    value: Option<GameConfig>,
    loaded: bool,
}

impl ConfigCache {
    fn read(&mut self, path: &Path) -> Option<&GameConfig> {
        let sig = fs::metadata(path)
            .ok()
            .and_then(|meta| meta.modified().ok().map(|mtime| (mtime, meta.len())));
        let bytes = fs::read(path).ok();
        // 快速路径：签名与字节都没变 → 不重新解析（`bytes` 为 `None` 时同样成立）
        if self.loaded && self.sig == sig && self.bytes.as_deref() == bytes.as_deref() {
            return self.value.as_ref();
        }
        self.sig = sig;
        self.loaded = true;
        // 与 `config::read` 同语义：非 UTF-8 视同读不出来
        let parsed = bytes
            .as_deref()
            .and_then(|raw| std::str::from_utf8(raw).ok())
            .and_then(config::parse);
        self.bytes = bytes;
        if self.value != parsed {
            self.value = parsed;
        }
        self.value.as_ref()
    }
}

/// 按"原因"分别节流的日志簿（同一原因在 `ACTION_LOG_THROTTLE` 内只记一条）。
#[derive(Default)]
struct ReasonLog {
    last: HashMap<String, Instant>,
}

impl ReasonLog {
    fn due(&mut self, reason: &str, now: Instant) -> bool {
        match self.last.get(reason) {
            Some(at) if now.duration_since(*at) < ACTION_LOG_THROTTLE => false,
            _ => {
                self.last.insert(reason.to_string(), now);
                true
            }
        }
    }
}

/// 一个身份键的重建尝试簿（纯逻辑、无 IO；住在 `Runtime` 里）。
///
/// 旧行为：miss 只受全局 `INDEX_REBUILD_THROTTLE`（5s）节流、**永不放弃**——真机 25 个身份键里
/// 只有 6 个在盘上，剩下 19 个会让整库（~22 MB）每 5s 被重哈希一次（实测一个会话 22 代索引）。
/// 现在每个 md5 各有 `MISS_REBUILD_ATTEMPTS` 次重建预算；用完即判"本会话不可解析"：不再请求、
/// 也不再记日志（判定那一次记一条 info，且只记一次）。
#[derive(Debug, Default)]
struct MissRebuildTracker {
    /// md5 → 已经用掉的重建次数。
    used: HashMap<String, u32>,
    /// 已判定本会话不可解析的 md5。
    unresolvable: HashSet<String>,
}

impl MissRebuildTracker {
    /// 该 md5 是否已判定不可解析（判定后调用方直接跳过：不请求、不记日志）。
    fn is_unresolvable(&self, md5: &str) -> bool {
        self.unresolvable.contains(md5)
    }

    /// 该 md5 的对外不可用原因（**两档**）：
    /// - 预算已耗尽（结论已定）→ `ChartUnknownIdentity`；
    /// - 否则 → `ChartUnresolved`：**第一拍就给**的临时原因（重建请求已发出 / 重试仍在窗口内）。
    ///
    /// 为什么第一拍就要给：miss 在 200ms 内就能测出来，而结论要等完 `INDEX_REBUILD_THROTTLE`（5s）×
    /// `MISS_REBUILD_ATTEMPTS`（2）≈ 10s 的重试窗口——那段窗口里页面**必须已经有话说**，否则用户
    /// 看到的就是"高亮了一张谱、卡片纹丝不动、一句提示都没有"（真机复测：提示要 9~10s 才出现）。
    /// 临时原因只是把"结果未定"这件事说到线上，**不动重试预算**（重试是"游戏里刚导入的谱能被
    /// 重新扫到"的唯一途径，不缩短也不取消）。两者的语义分界见 `model::UnavailableReason`。
    fn unavailable_reason(&self, md5: &str) -> UnavailableReason {
        if self.is_unresolvable(md5) {
            UnavailableReason::ChartUnknownIdentity
        } else {
            UnavailableReason::ChartUnresolved
        }
    }

    /// 已用掉的重建次数（诊断与单测）。
    #[cfg(test)]
    fn attempts(&self, md5: &str) -> u32 {
        self.used.get(md5).copied().unwrap_or(0)
    }

    /// 记一次重建尝试；预算用完返回 `false`，并就此把该 md5 记为不可解析。
    ///
    /// 调用方只在**真的要置重建位**时调用（全局节流窗口内不算一次尝试）：预算计的是"重建了
    /// 几次"，不是"miss 了几个 tick"——否则一次 200ms 的连续 miss 会在重建还没跑完时就把预算烧光。
    fn spend(&mut self, md5: &str) -> bool {
        let used = self.used.entry(md5.to_string()).or_insert(0);
        if *used >= MISS_REBUILD_ATTEMPTS {
            self.unresolvable.insert(md5.to_string());
            return false;
        }
        *used += 1;
        true
    }

    /// 库**真的**变了（周期重扫检出变化 / 根目录换了）→ 清空预算与放弃记录。
    ///
    /// 为什么放开：这些结论是从"上一份索引快照"推出来的，而快照的依据（树的内容）已经变了；
    /// 此前不可解析的 md5 可能正是这次新增的文件。miss 自己触发的重建**不**走这里——那种重建
    /// 消耗的是预算，不是"库变了"的证据（否则预算永远清不空，等于没封顶）。
    fn reset(&mut self) {
        self.used.clear();
        self.unresolvable.clear();
    }
}

/// 判定"本会话不可解析"那一刻的 **info** 日志文案（纯函数，便于单测逐字钉住）。
///
/// 这是用户要的"明确指示"：只靠 `reason` 字面量读者看不出卡片为什么停在上一张谱面上，
/// 所以这一行把**后果**说出来（卡片不会被门控或隐藏——那是明确排除的范围）。
/// 调用点只在预算耗尽的那一次（`MissRebuildTracker::spend` 返回 `false` 的那个 tick），
/// 同一个 md5 之后走 `is_unresolvable` 短路，**一整个会话只有这一行**（库真的变了会重置预算，
/// 那时允许再记一次）。
fn unresolvable_chart_log(md5: &str, generation: u64, indexed: usize) -> String {
    format!(
        "malody4 chart cannot be identified from the local library: md5={md5} (generation={generation}, indexed={indexed}) — {MISS_REBUILD_ATTEMPTS} rebuild attempts exhausted, no further rebuilds for this identity; the previous chart stays on screen"
    )
}

/// poller 的运行时状态（私有；`IndexShared` 是它与构建线程之间的共享）。
struct Runtime {
    idx: Arc<IndexShared>,
    root_tx: mpsc::Sender<PathBuf>,
    sent_root: Option<PathBuf>,
    attached: Option<Attached>,
    attach_error: Option<UnavailableReason>,
    last_attach_try: Option<Instant>,
    tail: LogTail,
    tracker: SceneTracker,
    game_config: ConfigCache,
    /// 连续软锚点读取失败的计数（硬失败不计数、直接 detach）。
    soft_reads: SoftReadTolerance,
    selection: SelectionState,
    dispatch: SongDispatchState,
    reason_log: ReasonLog,
    last_rebuild_request: Option<Instant>,
    /// 每个身份键的 miss 重试预算（用完即本会话不再为它重建）。
    miss_tracker: MissRebuildTracker,
    /// 上次同步过的 `IndexShared::content_revision`（变大 = 库真的变了 → 放开不可解析记录）。
    seen_content_revision: u64,
    last_miss: Option<(String, u64)>,
    last_hit: Option<(String, String)>,
    last_selection_log: Option<(String, String, f64, &'static str)>,
    /// 上一 tick 写进 `shared.malody4.reason` 的值（心跳之间的 tick 保持不变，避免闪）。
    reason: String,
    /// FAIR 判定模组的一次性提示是否已发（进程内只发一次）。
    fair_logged: bool,
    /// 附着成功时刻 = "内存 vs config.json"交叉校验窗口的起点（未附着为 `None`）。
    attached_at: Option<Instant>,
    /// 交叉校验是否已经做过（每个附着窗口只做一次，无论结论如何）。
    settings_cross_checked: bool,
    /// `S` / `P` 连续不一致的拍数（一致或读不到就清零）。
    chain_disagree: u32,
    /// 两条链持续不一致的告警是否已记（**进程内只记一次**）。
    chain_disagree_logged: bool,
    /// 上一次记过的有效值（来源 / 判定 / 速率）：只有变化时才再记 info。
    last_settings: Option<SettingsLogKey>,
    song_seq: u64,
}

impl Runtime {
    fn new(idx: Arc<IndexShared>, root_tx: mpsc::Sender<PathBuf>) -> Self {
        let seen_content_revision = idx.content_revision.load(Ordering::Relaxed);
        Runtime {
            idx,
            root_tx,
            sent_root: None,
            attached: None,
            attach_error: None,
            last_attach_try: None,
            tail: LogTail::new(),
            tracker: SceneTracker::new(),
            game_config: ConfigCache::default(),
            soft_reads: SoftReadTolerance::new(),
            selection: SelectionState::new(),
            dispatch: SongDispatchState::default(),
            reason_log: ReasonLog::default(),
            last_rebuild_request: None,
            miss_tracker: MissRebuildTracker::default(),
            seen_content_revision,
            last_miss: None,
            last_hit: None,
            last_selection_log: None,
            reason: String::new(),
            fair_logged: false,
            attached_at: None,
            settings_cross_checked: false,
            chain_disagree: 0,
            chain_disagree_logged: false,
            last_settings: None,
            song_seq: 0,
        }
    }

    /// 一个 tick。
    fn tick(&mut self, shared: &Shared) {
        let now = Instant::now();

        // ① 惰性 attach：未附着时每 ATTACH_RETRY 试一次；各失败原因分别节流记 warn。
        self.ensure_attached(now);

        // ② 根目录解析链（process_exe → env → 壳配置 → tosu 设置 → 启发候选）。
        let process_exe = self
            .attached
            .as_ref()
            .map(|attached| attached.process_exe().to_path_buf());
        let root = crate::server::malody4_root(shared, process_exe.as_deref());
        if let Some(root) = root.as_ref() {
            // 根目录变化时才通知构建线程（构建线程只做构建，不做解析）
            if self.sent_root.as_deref() != Some(root.as_path()) {
                let _ = self.root_tx.send(root.clone());
                self.sent_root = Some(root.clone());
            }
        }

        // ③ 索引侧同步：周期重扫（60s）已下沉到构建线程——指纹扫描要在那里跟 `idx.lib` 比对，
        //    且主循环绝不遍历文件系统。这里只把"库真的变了"的修订号同步过来，放开此前判定
        //    不可解析的身份键（它们可能正是这次新增的文件）。
        self.sync_content_revision();

        // ④ 场景：尾随最新日志（目录枚举与字节读取分别在 gamelog / LogTail 里）。
        let screen = match root.as_ref() {
            Some(root) => {
                self.tail.follow(&root.join("log"), &mut self.tracker);
                self.tracker.screen()
            }
            None => Screen::Other,
        };
        let playing = screen == Screen::Playing && self.tracker.fresh(PLAYING_FRESH);

        // ⑤ 锚点：只有根目录已知且已 attach 时才读内存。
        //    硬失败（进程 / 模块 / 句柄已不在）立即 detach；软失败（指针槽位读到了、内容当前
        //    不是身份键，如过场瞬间）只计数：连续 SOFT_READ_TOLERANCE 次才 detach，且在容忍
        //    窗口内本 tick 按"没选中谱面"处理（`key = None` → `Hidden(NoSelection)`），页面
        //    保留上一张谱面而不是看到一条错误。
        let mut key: Option<IdentityKey> = None;
        let mut read_ok = false;
        let mut blocker: Option<UnavailableReason> = None;
        if root.is_none() {
            // 整条解析链走完仍为 None —— 唯一的"没配"语义
            blocker = Some(UnavailableReason::RootNotConfigured);
        } else if let Some(attached) = self.attached.as_ref() {
            match read_action(
                &mut self.soft_reads,
                anchor::read_identity_classified(&attached.attachment),
            ) {
                ReadAction::Healthy(identity) => {
                    key = identity;
                    read_ok = true;
                }
                // 指针槽位读到了 ⇒ 进程与模块仍在：通道算健康（state 帧的 alive 不为假），
                // 只是这一拍没有可用的身份键 → `key = None` 交给状态机给出 Hidden(NoSelection)
                ReadAction::Tolerated => read_ok = true,
                // 游戏退出 / 句柄失效 / PID 复用：丢弃 Target（Drop 关句柄），回到 2s 重连
                ReadAction::Detach(err) => blocker = Some(self.detach(err, now)),
            }
        } else {
            blocker = Some(
                self.attach_error
                    .clone()
                    .unwrap_or(UnavailableReason::ProcessNotFound),
            );
        }

        // ⑥ 索引 lookup：未就绪 → NoLibrary；miss → 置重建请求（受预算 + 节流约束）+ 记一次带
        //    md5 的日志。miss 的对外原因是**两档**（都由这里当拍设 blocker ⇒ state 帧立即带着它走，
        //    见 `state_push_due`）：预算还没用完 = **结果未定** → `chart-unresolved`（重试仍在进行）；
        //    预算已耗尽 = **结论已定** → `chart-unknown-identity`。
        let mut entry: Option<LibraryEntry> = None;
        if blocker.is_none() {
            if let Some(identity) = key.as_ref() {
                if !index_ready(&self.idx) {
                    blocker = Some(UnavailableReason::NoLibrary);
                } else {
                    entry = lookup(&self.idx, &identity.md5);
                    match entry.as_ref() {
                        Some(found) => self.log_hit(&identity.md5, &found.path),
                        None => {
                            self.request_miss_rebuild(&identity.md5, now);
                            blocker = Some(self.miss_tracker.unavailable_reason(&identity.md5));
                        }
                    }
                }
            }
        }

        // ⑦ 判定档 / 变速位：**进程内存优先**（游戏里一改立刻生效），config.json 兜底。
        //    游戏只在开局与退出时重写 config.json（实测 mtime 全程不动），它永远反映不了本局
        //    中途的改动 —— 所以内存这条链是主路径，文件只在内存读不到时顶上（含"判定未知"）。
        let file_config = match root.as_ref() {
            Some(root) => self.game_config.read(&root.join("config.json")).cloned(),
            None => None,
        };
        let probe = self.read_memory_settings(now);
        self.note_chain_disagreement(&probe);
        let effective = effective_settings(probe.published(), file_config.as_ref());
        self.note_file_cross_check(&effective, file_config.as_ref(), now);
        self.log_settings_change(&effective);
        let rate = effective.rate;
        let judge = effective.judge;
        // FAIR 判定模组（`user_mods` bit 0x400）：内存值与 config.json 值都能触发；首次读到置了
        // 该位的 `user_mods` 时记一条 warning（进程内只记一次）。`user_mods` 从不进任何帧，
        // 这是该局限唯一的提示通道。
        if fair_judge_mod(effective.user_mods) && !self.fair_logged {
            self.fair_logged = true;
            log_at("warn", FAIR_JUDGE_WARNING);
        }

        // ⑧ selection 状态机（难度名取自索引的 ChartMeta，不重读谱面）。
        let version = entry
            .as_ref()
            .and_then(|found| meta_of(&self.idx, &found.path))
            .map(|meta| meta.version)
            .unwrap_or_default();
        let availability = match blocker.clone() {
            Some(reason) => Availability::Unavailable(reason),
            None => Availability::Ready,
        };
        let action = self.selection.fold(
            key,
            entry.as_ref(),
            screen,
            &version,
            rate,
            availability,
            now,
        );

        // 判定不可确定（内存与 config.json 都没给出判定）→ 本 tick 的 song 帧被保守 withhold，
        // 只记一条节流日志；`malody4_selection` 的 2s 心跳不受影响。
        if judge.is_none() && matches!(action, Action::Emit(_)) && self.reason_log.due("judge-unknown", now)
        {
            log_at(
                "warn",
                "malody4: judge level unknown (not readable from game memory and config.json missing or unparsable) — song frame withheld this tick",
            );
        }

        // ⑨ 分派：selection 帧必发；song 帧只在签名变化 / 出现更大连接 id 时发。
        let conn = max_conn_id(shared);
        for plan in dispatch_plan(&action, &mut self.dispatch, conn, judge) {
            match plan {
                Dispatch::SelectionFrame => self.send_selection(shared, &action),
                Dispatch::SongFrame => self.send_song(shared, entry.as_ref(), rate, judge),
                Dispatch::None => {}
            }
        }

        // ⑩ 状态更新：**先在独立作用域里改完并 drop guard**，再广播 state 帧。
        //    `state_frame` 会再 lock 同一字段；std Mutex 不可重入，跨调用持锁会自死锁（
        //    这会把 L1 抢占延迟从 ~300ms 变成壳的 30s 周期）。
        //    补发条件见 `state_push_due`：`alive` / `playing` 之外还包括 `reason`（未知身份这类
        //    只在 reason 上体现的跳变必须当拍推给页面）、`screen`、`judge`。
        let alive = root.is_some() && read_ok;
        let reason = wire_reason(blocker.as_ref(), &action, &self.reason);
        let next = Malody4Status {
            alive,
            screen: screen.as_str().to_string(),
            playing,
            reason: reason.clone(),
            judge,
        };
        let due = {
            let mut status = shared.malody4.lock().unwrap();
            let due = state_push_due(&status, &next);
            *status = next;
            // guard 随本作用域结束 drop —— 下面的 `broadcast` 在锁外（它要再 lock 同一字段）
            due
        };
        self.reason = reason;
        if due {
            broadcast(shared, "state", Some(crate::server::state_frame(shared)));
        }
    }

    /// 未附着时每 `ATTACH_RETRY` 重试一次 `find_target()` + `open()`。
    fn ensure_attached(&mut self, now: Instant) {
        if self.attached.is_some() {
            return;
        }
        if let Some(at) = self.last_attach_try {
            if now.duration_since(at) < ATTACH_RETRY {
                return;
            }
        }
        self.last_attach_try = Some(now);
        let error = match anchor::find_target() {
            Ok(target) => match anchor::open(&target) {
                Ok(attachment) => {
                    log_at(
                        "debug",
                        &format!(
                            "malody4 attached: pid={} base=0x{:08X} exe={}",
                            target.pid,
                            target.base_u32,
                            target.exe_path.display()
                        ),
                    );
                    self.attached = Some(Attached { target, attachment });
                    self.attach_error = None;
                    // 新的附着窗口：交叉校验重新开一次（窗口内的分歧才值得提示）
                    self.attached_at = Some(now);
                    self.settings_cross_checked = false;
                    self.chain_disagree = 0;
                    return;
                }
                Err(err) => unavailable_from_anchor(err),
            },
            Err(err) => unavailable_from_anchor(err),
        };
        self.attach_error = Some(error.clone());
        if self.reason_log.due(&error.as_str(), now) {
            log_at(
                "warn",
                &format!(
                    "malody4 attach failed: {} (retrying every {}s)",
                    error.as_str(),
                    ATTACH_RETRY.as_secs()
                ),
            );
        }
    }

    /// 丢弃已附着目标（两个 `Drop` 各自关句柄），回到"每 2s 重新 attach"状态。
    /// 软失败计数一并清零：重连后从零开始数，不会因上一段通道的残值立刻再次 detach。
    fn detach(&mut self, err: AnchorError, now: Instant) -> UnavailableReason {
        let reason = unavailable_from_anchor(err);
        self.attached = None;
        self.attach_error = Some(reason.clone());
        self.last_attach_try = Some(now);
        self.soft_reads.reset();
        self.attached_at = None;
        self.chain_disagree = 0;
        if self.reason_log.due(&reason.as_str(), now) {
            log_at(
                "warn",
                &format!(
                    "malody4 detached: {} (retrying every {}s)",
                    reason.as_str(),
                    ATTACH_RETRY.as_secs()
                ),
            );
        }
        reason
    }

    /// 读一次内存设置（`P` 发布 + `S` 兜底与旁证）：未附着 → 空探针；硬失败 → 空探针 + 一条节流 debug。
    ///
    /// 空探针 = "现在读不到"，与"读到 0"同档语义：调用方回落 `config.json`，**不 latch**。
    /// 这里**不 detach**：通道健康由锚点身份读取那条路径判定（硬失败即时 detach），
    /// 两条路径各 detach 一次只会把同一件事记两遍。
    fn read_memory_settings(&mut self, now: Instant) -> MemoryProbe {
        let Some(attached) = self.attached.as_ref() else {
            return MemoryProbe::default();
        };
        match anchor::read_settings(&attached.attachment) {
            Ok(probe) => probe,
            Err(err) => {
                if self.reason_log.due("settings-read", now) {
                    log_at(
                        "debug",
                        &format!("malody4 settings read failed: {err:?} — using config.json this tick"),
                    );
                }
                MemoryProbe::default()
            }
        }
    }

    /// `P` / `S` 是否**持续**不一致：判定档设置器把两个对象写在一起，正常同值；连续
    /// `CHAIN_DISAGREE_POLLS` 拍仍不一致 ⇒ 其中一条链的偏移理解有误 → 记**一次** warn
    /// （进程内只此一次）。发布值不受影响：仍按 `P`（`S` 只是兜底与旁证；mods 面板只写 `P`，
    /// 那里的分歧本来就可能持续存在）。
    fn note_chain_disagreement(&mut self, probe: &MemoryProbe) {
        match probe.disagreement() {
            Some((user, play)) => {
                self.chain_disagree = self.chain_disagree.saturating_add(1);
                if self.chain_disagree >= CHAIN_DISAGREE_POLLS && !self.chain_disagree_logged {
                    self.chain_disagree_logged = true;
                    log_at(
                        "warn",
                        &chain_disagreement_warning(user, play, self.chain_disagree),
                    );
                }
            }
            // 一致，或有一条 / 两条读不到 → 清零（只判"持续"）
            None => self.chain_disagree = 0,
        }
    }

    /// 交叉校验（**软信号，绝不是门**）：`config.json` 的这两个键正是从内存里这两个偏移写出去的，
    /// 所以**刚附着**时两者应当一致。本局中途改判定造成的分歧正是本次改动的理由 —— 分歧
    /// **不拒绝、不覆盖**内存值，只在附着后的 `SETTINGS_CROSS_CHECK` 窗口内记一条 warn
    /// （每个附着窗口最多一条）。返回是否记了这条告警（便于单测）。
    fn note_file_cross_check(
        &mut self,
        effective: &EffectiveSettings,
        file: Option<&GameConfig>,
        now: Instant,
    ) -> bool {
        if self.settings_cross_checked {
            return false;
        }
        let Some(attached_at) = self.attached_at else {
            return false;
        };
        if now.duration_since(attached_at) > SETTINGS_CROSS_CHECK {
            // 窗口过了就不再比；flag 立起来，省掉之后每 tick 的时间比较
            self.settings_cross_checked = true;
            return false;
        }
        // 发布的**就是** config.json 的值（内存没读到）→ 没有可交叉校验的对象
        let (Some(file), SettingsOrigin::Memory) = (file, effective.source) else {
            return false;
        };
        self.settings_cross_checked = true;
        if file.judge_letter() != effective.judge
            || (file.mod_flags().speed_rate() - effective.rate).abs() >= RATE_EPS
        {
            log_at("warn", &file_cross_check_warning(effective, file));
            return true;
        }
        false
    }

    /// 有效值**变化**时记一条 info（来源 / 判定字母 / 速率）；同值一律不记（绝不逐 tick 刷）。
    /// 返回是否记了（便于单测）。
    fn log_settings_change(&mut self, effective: &EffectiveSettings) -> bool {
        let key = SettingsLogKey {
            source: effective.source,
            judge: effective.judge,
            rate: effective.rate,
        };
        if self.last_settings.as_ref().map(|last| last.matches(&key)) == Some(true) {
            return false;
        }
        log_at("info", &settings_log(effective));
        self.last_settings = Some(key);
        true
    }

    /// 命中：`md5 → path` 每次变化只记一条 **info** 日志（诊断工具在"不跟随"时的唯一来源；
    /// 默认 `logLevel=info`，故这一行在真机现场必然可见）。
    fn log_hit(&mut self, md5: &str, path: &Path) {
        let hit = (md5.to_string(), path.display().to_string());
        if self.last_hit.as_ref() != Some(&hit) {
            log_at(
                "info",
                &format!("malody4 anchor hit md5={} -> {}", hit.0, hit.1),
            );
            self.last_hit = Some(hit);
        }
    }

    /// 库真的变了（周期重扫检出变化 / 根目录换了）→ 放开所有"本会话不可解析"的记录。
    ///
    /// 修订号由构建线程在**非 miss 触发**的重建上 +1；poller 每 tick 同步一次。
    fn sync_content_revision(&mut self) {
        let revision = self.idx.content_revision.load(Ordering::Relaxed);
        if revision != self.seen_content_revision {
            self.seen_content_revision = revision;
            self.miss_tracker.reset();
        }
    }

    /// miss：在预算内为这个 md5 置一次重建请求位（仍受全局 `INDEX_REBUILD_THROTTLE` 节流）。
    ///
    /// 每个 `(md5, generation)` 记一次 **info** 命中现场（同一 md5 去重；重建完成、代数变化后
    /// 再记一次，便于定位）。连当前 `stats.indexed` 一起记：读者据此区分"库里没有这张谱"与
    /// "库是空的 / 还没建起来"。
    ///
    /// 预算用完时记**唯一一次** info 说明"本会话不会再为它重建"与**用户可见的后果**（卡片保留
    /// 上一张谱面），此后对同一 md5 完全沉默（真机 19/25 个身份键永远查不到，旧行为每 5s
    /// 重哈希一次整库）。这一次也是该身份键的对外原因从临时态 `chart-unresolved` 换成结论态
    /// `chart-unknown-identity` 的时刻（见 `MissRebuildTracker::unavailable_reason`）。
    fn request_miss_rebuild(&mut self, md5: &str, now: Instant) {
        if self.miss_tracker.is_unresolvable(md5) {
            return;
        }
        // 全局节流：窗口内什么都不做——上一次请求还在被构建线程服务（或刚服务完），
        // 它整库重建一次就同时回答了这段时间里的所有 miss。
        if self
            .last_rebuild_request
            .map(|at| now.duration_since(at) < INDEX_REBUILD_THROTTLE)
            .unwrap_or(false)
        {
            return;
        }
        let generation = self.idx.generation.load(Ordering::Relaxed);
        if !self.miss_tracker.spend(md5) {
            log_at(
                "info",
                &unresolvable_chart_log(md5, generation, indexed_count(&self.idx)),
            );
            return;
        }
        request_rebuild(&self.idx);
        self.last_rebuild_request = Some(now);
        if self.last_miss.as_ref() != Some(&(md5.to_string(), generation)) {
            log_at(
                "info",
                &format!(
                    "malody4 index miss md5={md5} (generation={generation}, indexed={}) — rebuild requested",
                    indexed_count(&self.idx)
                ),
            );
            self.last_miss = Some((md5.to_string(), generation));
        }
    }

    /// 发一条 `malody4_selection`（`Emit` 用状态机给出的记录；`Hidden` 用全空的 hidden 记录）。
    fn send_selection(&mut self, shared: &Shared, action: &Action) {
        let record = match action {
            Action::Emit(selection) => selection.clone(),
            Action::Hidden(_) => Selection::hidden(self.selection.sequence(), "hidden"),
            Action::None => return,
        };
        // U6/F7 验"hidden 记录五字段全空"的唯一证据来源；按记录内容去重，不逐 tick 刷。
        let log_key = (
            record.event.clone(),
            record.path.clone(),
            record.speed_rate,
            record.screen,
        );
        if self.last_selection_log.as_ref() != Some(&log_key) {
            log_at(
                "debug",
                &format!(
                    "malody4 selection: path=\"{}\" speed_rate={:.5} screen={} event={} sequence={}",
                    record.path, record.speed_rate, record.screen, record.event, record.sequence
                ),
            );
            self.last_selection_log = Some(log_key);
        }
        let payload = serde_json::to_value(&record).unwrap_or(serde_json::Value::Null);
        broadcast(shared, "malody4_selection", Some(payload));
    }

    /// 发一条 song 帧（`requestId = n{seq}`）。
    fn send_song(
        &mut self,
        shared: &Shared,
        entry: Option<&LibraryEntry>,
        rate: f64,
        judge: Option<char>,
    ) {
        let (Some(entry), Some(judge)) = (entry, judge) else {
            return;
        };
        let Some(meta) = meta_of(&self.idx, &entry.path) else {
            if self.reason_log.due("meta-missing", Instant::now()) {
                log_at(
                    "warn",
                    &format!("malody4 index meta missing for {}", entry.path.display()),
                );
            }
            return;
        };
        let seq = self.song_seq + 1;
        let Some(song) = build_song_frame(shared, entry, &meta, rate, judge, seq) else {
            return;
        };
        self.song_seq = seq;
        log_at(
            "debug",
            &format!(
                "malody4 song frame: identity=mdy4:{} requestId=n{} path={} rate={:.5} judge={}",
                entry.md5,
                seq,
                entry.path.display(),
                rate,
                judge
            ),
        );
        broadcast(shared, "song", Some(song));
    }
}

/// 启动 Malody 4 源 poller：单线程 200ms 轮转睡眠 + 一个索引构建线程。
pub fn spawn_poller(shared: Arc<Shared>) {
    thread::spawn(move || {
        let idx = Arc::new(IndexShared::new());
        let idx_build = idx.clone();
        // 根目录由 poller 解析后经 channel 送达（构建线程不做解析，只做构建）
        let (root_tx, root_rx) = mpsc::channel::<PathBuf>();
        thread::spawn(move || index_build_loop(idx_build, root_rx));
        let mut runtime = Runtime::new(idx, root_tx);
        loop {
            thread::sleep(POLL_INTERVAL);
            runtime.tick(&shared);
        }
    });
}

/// song 帧（与 MalodyV 通道同形 + `meta.judge`）。
fn build_song_frame(
    shared: &Shared,
    entry: &LibraryEntry,
    meta: &ChartMeta,
    rate: f64,
    judge: char,
    seq: u64,
) -> Option<serde_json::Value> {
    let raw_text = match fs::read_to_string(&entry.path) {
        Ok(text) => text,
        Err(err) => {
            log_at(
                "warn",
                &format!(
                    "malody4 chart read failed: {} ({err})",
                    entry.path.display()
                ),
            );
            return None;
        }
    };
    // 超限护栏（MalodyV 通道缺此护栏）：丢弃 rawText 并推 shell_errors。
    if raw_text.len() > MAX_PAYLOAD_BYTES {
        shared.shell_errors.lock().unwrap().push(format!(
            "Malody 4 chart too large, skipped: {} (>5MB)",
            entry.path.display()
        ));
        return None;
    }
    Some(serde_json::json!({
        "requestId": format!("n{}", seq),
        "source": "malody4",
        "identity": format!("mdy4:{}", entry.md5),
        "modData": { "speedRate": format!("{:.5}", rate) },
        "meta": {
            "title": meta.title,
            "artist": meta.artist,
            "version": meta.version,
            "keys": meta.keys,
            "devMsd8": [],
            "judge": judge,
        },
        "cover": null,
        "rawText": raw_text,
    }))
}

/// 当前 WS 客户端里最大的连接 id（加锁时间极短，不持锁做任何其他事）。
fn max_conn_id(shared: &Shared) -> Option<u64> {
    shared.sinks.lock().unwrap().iter().map(|(id, _)| *id).max()
}

/// 线上 `reason` 的选取（纯函数，闭集见 `UnavailableReason::as_str`）。
///
/// 优先级：附着/根目录/索引层面的成因（`blocker`）→ 状态机给出的隐藏成因（如
/// `chart-not-indexed`，它在 `selection` 里判定，只有这里能让它在线上可见）→ 健康时清空。
/// `chart-unknown-identity` 与它的临时态 `chart-unresolved` 都由本文件判定
/// （见 `MissRebuildTracker::unavailable_reason`），以 `blocker` 的形态走第一条分支。
/// `NoSelection`（游戏在跑但没选中谱面）是正常态 → 空串。
/// 心跳之间的 `Action::None` 沿用上一 tick 的值，避免 state 帧每 200ms 闪一次 reason 的有无。
fn wire_reason(blocker: Option<&UnavailableReason>, action: &Action, previous: &str) -> String {
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
fn unavailable_from_anchor(err: AnchorError) -> UnavailableReason {
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
