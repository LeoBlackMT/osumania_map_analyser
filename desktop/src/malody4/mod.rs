// Malody 4.3.7（旧原生 32 位客户端）数据源：纯逻辑层 + 壳侧 poller。
//
// - anchor：版本表（RVA / PE 时间戳 / 文件大小）、身份键解析、PE 版本校验
// - config：config.json 只读解析（只取 user_mods / user_judge_level）
// - gamelog：日志场景行解析（switch to N）+ 日志目录枚举（`latest_log`）
// - library：<root>/beatmap/** 索引（流式 md5 + 有界前缀 meta 提取）——后台构建线程独占写入
// - model：Screen / ModFlags / JudgeLevel / Selection / UnavailableReason
// - selection：selection 状态机（防抖 + 心跳 + 不可用原因）
//
// 本模块的阈值全部集中在这里，任何子模块都不得各写一份数字。
//
// poller 的 tick 顺序（对应 Step 4 的伪码，逐条见 `.omo/evidence/malody4-source/task-4-loop.txt`）：
//   ① attach（惰性，2s 重试）→ ② 根目录解析链 → ③ 60s 重建请求 → ④ 日志尾随取 screen
//   → ⑤ 锚点读取（硬失败立即 detach；软失败容忍 `SOFT_READ_TOLERANCE` 次）→ ⑥ 索引 lookup
//   → ⑦ config（rate/judge）→ ⑧ selection.fold → ⑨ dispatch_plan 发帧
//   → ⑩ 状态更新（跳变时即时推 state 帧）。
//
// 主循环内**不做文件系统遍历**：日志目录枚举在 `gamelog::latest_log`、索引遍历在
// `library::rebuild`（后台构建线程），本文件只调用它们。

pub mod anchor;
pub mod config;
pub mod gamelog;
pub mod library;
pub mod model;
pub mod selection;

use crate::etterna::EtternaStatus;
use crate::frames::{
    EtternaSource, Malody4Source, MalodySource, SourcesFrame, StateFrame, MAX_PAYLOAD_BYTES,
};
use crate::server::log::log_at;
use crate::server::{broadcast, Shared};
use self::anchor::{AnchorError, IdentityKey};
use self::config::GameConfig;
use self::gamelog::SceneTracker;
use self::library::{ChartLibrary, ChartMeta, LibraryEntry, LibraryStats};
use self::model::{Screen, Selection, UnavailableReason};
use self::selection::{Action, Availability, SelectionState};
use std::collections::HashMap;
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
/// 索引周期重建间隔。
pub const INDEX_RESCAN: Duration = Duration::from_secs(60);
/// 同一失败原因的日志节流窗口。
pub const ACTION_LOG_THROTTLE: Duration = Duration::from_secs(30);
/// 未附着时的重新 attach 间隔（惰性 attach）。
pub const ATTACH_RETRY: Duration = Duration::from_secs(2);
/// 索引 miss 触发的重建请求节流（全局）。
pub const INDEX_REBUILD_THROTTLE: Duration = Duration::from_secs(5);
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
/// poller 只读且不持锁做别的事。60s 周期重建与 miss 触发的重建都**只置位**，
/// 200ms 主循环绝不遍历文件系统。
pub struct IndexShared {
    pub lib: Mutex<ChartLibrary>,
    pub rebuild_requested: AtomicBool,
    pub generation: AtomicU64,
}

impl IndexShared {
    pub fn new() -> Self {
        IndexShared {
            lib: Mutex::new(empty_library()),
            rebuild_requested: AtomicBool::new(false),
            generation: AtomicU64::new(0),
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

/// 索引构建线程：`idx.lib` 的唯一写者；根目录由 poller 经 channel 送达。
fn index_build_loop(idx: Arc<IndexShared>, root_rx: mpsc::Receiver<PathBuf>) {
    let mut root: Option<PathBuf> = None;
    let mut built_root: Option<PathBuf> = None;
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
        if !requested && !stale {
            continue;
        }
        let started = Instant::now();
        let rebuilt = library::rebuild(&dir);
        let stats = rebuilt.stats.clone();
        *idx.lib.lock().unwrap() = rebuilt;
        let generation = idx.generation.fetch_add(1, Ordering::Relaxed) + 1;
        // stats 全字段：用于区分"库里没有这张谱"与"被过滤掉了"。
        // info 级（默认 logLevel=info 可见）：这是判断索引是否建成、收了多少张的唯一现场证据。
        log_at(
            "info",
            &format!(
                "malody4 index built: root={} indexed={} skipped_non_key={} skipped_junk={} skipped_ext={} skipped_unparsed={} duplicate_md5={} elapsed_ms={} generation={}",
                dir.display(),
                stats.indexed,
                stats.skipped_non_key,
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
    last_rescan: Option<Instant>,
    last_rebuild_request: Option<Instant>,
    last_miss: Option<(String, u64)>,
    last_hit: Option<(String, String)>,
    last_selection_log: Option<(String, String, f64, &'static str)>,
    /// 上一 tick 写进 `shared.malody4.reason` 的值（心跳之间的 tick 保持不变，避免闪）。
    reason: String,
    /// FAIR 判定模组的一次性提示是否已发（进程内只发一次）。
    fair_logged: bool,
    song_seq: u64,
}

impl Runtime {
    fn new(idx: Arc<IndexShared>, root_tx: mpsc::Sender<PathBuf>) -> Self {
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
            last_rescan: None,
            last_rebuild_request: None,
            last_miss: None,
            last_hit: None,
            last_selection_log: None,
            reason: String::new(),
            fair_logged: false,
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

        // ③ 60s 周期重建：只置请求位（主循环绝不遍历文件系统）。
        if self
            .last_rescan
            .map(|at| now.duration_since(at) >= INDEX_RESCAN)
            .unwrap_or(true)
        {
            request_rebuild(&self.idx);
            self.last_rescan = Some(now);
        }

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

        // ⑥ 索引 lookup：未就绪 → NoLibrary；miss → 置重建请求（节流）+ 记一次带 md5 的日志。
        let mut entry: Option<LibraryEntry> = None;
        if blocker.is_none() {
            if let Some(identity) = key.as_ref() {
                if !index_ready(&self.idx) {
                    blocker = Some(UnavailableReason::NoLibrary);
                } else {
                    entry = lookup(&self.idx, &identity.md5);
                    match entry.as_ref() {
                        Some(found) => self.log_hit(&identity.md5, &found.path),
                        None => self.request_miss_rebuild(&identity.md5, now),
                    }
                }
            }
        }

        // ⑦ config.json：变速位 → rate；判定档 → judge（按字节内容缓存，(mtime, 长度) 仅作提示）。
        let game_config = match root.as_ref() {
            Some(root) => self.game_config.read(&root.join("config.json")),
            None => None,
        };
        let rate = game_config
            .map(|config| config.mod_flags().speed_rate())
            .unwrap_or(1.0);
        let judge = game_config.and_then(|config| config.judge_letter());
        // FAIR 判定模组（`user_mods` bit 0x400）：首次读到 `user_mods` 时检测一次，命中即
        // 记一条 warning（进程内只记一次）。`user_mods` 从不进任何帧，这是该局限唯一的提示通道。
        if let Some(config) = game_config {
            if fair_judge_mod(config.user_mods) && !self.fair_logged {
                self.fair_logged = true;
                log_at("warn", FAIR_JUDGE_WARNING);
            }
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

        // 判定不可确定（config.json 缺失/未解析）→ 本 tick 的 song 帧被保守 withhold，
        // 只记一条节流日志；`malody4_selection` 的 2s 心跳不受影响。
        if judge.is_none() && matches!(action, Action::Emit(_)) && self.reason_log.due("judge-unknown", now)
        {
            log_at(
                "warn",
                "malody4: judge level unknown (config.json missing or unparsable) — song frame withheld this tick",
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
        let alive = root.is_some() && read_ok;
        let reason = wire_reason(blocker.as_ref(), &action, &self.reason);
        let raised = {
            let mut status = shared.malody4.lock().unwrap();
            let raised = status.alive != alive || status.playing != playing;
            status.alive = alive;
            status.playing = playing;
            status.screen = screen.as_str().to_string();
            status.reason = reason.clone();
            status.judge = judge;
            raised
        };
        self.reason = reason;
        if raised {
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

    /// miss：置重建请求（全局 `INDEX_REBUILD_THROTTLE` 节流）+ 每个 `(md5, generation)`
    /// 记一次 **info** 日志（同一 md5 去重；重建完成、代数变化后再记一次，便于定位）。
    /// 连当前 `stats.indexed` 一起记：读者据此区分"库里没有这张谱"与"库是空的 / 还没建起来"。
    fn request_miss_rebuild(&mut self, md5: &str, now: Instant) {
        let generation = self.idx.generation.load(Ordering::Relaxed);
        if self.last_miss.as_ref() != Some(&(md5.to_string(), generation)) {
            log_at(
                "info",
                &format!(
                    "malody4 index miss md5={} (generation={}, indexed={}) — rebuild requested",
                    md5,
                    generation,
                    indexed_count(&self.idx)
                ),
            );
            self.last_miss = Some((md5.to_string(), generation));
        }
        if self
            .last_rebuild_request
            .map(|at| now.duration_since(at) >= INDEX_REBUILD_THROTTLE)
            .unwrap_or(true)
        {
            request_rebuild(&self.idx);
            self.last_rebuild_request = Some(now);
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
/// `NoSelection`（游戏在跑但没选中谱面）是正常态 → 空串。心跳之间的 `Action::None`
/// 沿用上一 tick 的值，避免 state 帧每 200ms 闪一次 reason 的有无。
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
mod tests {
    use super::*;
    use std::io::Write as _;

    /// 契约 §8 的 `reason` 闭集（逐字）。
    const REASON_CLOSED_SET: [&str; 12] = [
        "chart-not-indexed",
        "process-not-found",
        "multiple-instances",
        "access-denied",
        "bad-read",
        "target-mismatch:pe_timestamp_mismatch",
        "target-mismatch:file_size_mismatch",
        "target-mismatch:pe_header_out_of_range",
        "target-mismatch:unknown",
        "root-not-configured",
        "no-library",
        "platform-unsupported",
    ];

    fn emit(path: &str, rate: f64, screen: &'static str, event: &str) -> Action {
        Action::Emit(Selection {
            path: path.to_string(),
            speed_rate: rate,
            screen,
            sequence: 1,
            event: event.to_string(),
            version: "Hard".to_string(),
            chart_hash: "ab".to_string(),
            source: super::model::SOURCE_ID,
        })
    }

    fn songs(plans: &[Dispatch]) -> usize {
        plans
            .iter()
            .filter(|plan| **plan == Dispatch::SongFrame)
            .count()
    }

    fn selections(plans: &[Dispatch]) -> usize {
        plans
            .iter()
            .filter(|plan| **plan == Dispatch::SelectionFrame)
            .count()
    }

    fn tmp_dir(tag: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "mma-malody4-mod-{}-{}-{}",
            tag,
            std::process::id(),
            stamp
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    // ---- build_state_frame ----

    #[test]
    fn build_state_frame_keeps_both_existing_sources_and_adds_malody4() {
        let etterna = EtternaStatus {
            alive: true,
            playing: false,
            playing_expire_at: Some(1234),
        };
        let malody4 = Malody4Status {
            alive: true,
            playing: true,
            screen: "playing".to_string(),
            reason: String::new(),
            judge: None,
        };
        let value = build_state_frame(true, &[], &etterna, true, &malody4);

        let top = value.as_object().expect("state 帧必须是对象");
        assert_eq!(top.len(), 3, "顶层键只有 tosuOnline / errors / sources");
        for key in ["tosuOnline", "errors", "sources"] {
            assert!(top.contains_key(key), "缺顶层键 {key}");
        }
        assert_eq!(value["tosuOnline"], serde_json::json!(true));
        assert_eq!(value["errors"], serde_json::json!([]));

        // 既有两个源：结构体化不得丢字段
        assert_eq!(value["sources"]["etterna"]["alive"], serde_json::json!(true));
        assert_eq!(value["sources"]["etterna"]["playing"], serde_json::json!(false));
        assert_eq!(
            value["sources"]["etterna"]["playingExpireAt"],
            serde_json::json!(1234)
        );
        assert_eq!(value["sources"]["malody"]["alive"], serde_json::json!(true));

        // 新源
        assert_eq!(value["sources"]["malody4"]["alive"], serde_json::json!(true));
        assert_eq!(value["sources"]["malody4"]["playing"], serde_json::json!(true));
        assert_eq!(
            value["sources"]["malody4"]["screen"],
            serde_json::json!("playing")
        );
        assert!(
            value["sources"]["malody4"].get("reason").is_none(),
            "空 reason 不得出现在帧里"
        );
        assert!(
            value["sources"]["malody4"].get("judge").is_none(),
            "None 判定不得出现在帧里"
        );
    }

    #[test]
    fn build_state_frame_serialises_reason_and_judge_when_present() {
        let malody4 = Malody4Status {
            alive: false,
            playing: false,
            screen: "other".to_string(),
            reason: "process-not-found".to_string(),
            judge: Some('B'),
        };
        let value = build_state_frame(
            false,
            &["boom".to_string()],
            &EtternaStatus::default(),
            false,
            &malody4,
        );
        assert_eq!(value["errors"], serde_json::json!(["boom"]));
        assert_eq!(
            value["sources"]["malody4"]["reason"],
            serde_json::json!("process-not-found")
        );
        assert_eq!(value["sources"]["malody4"]["judge"], serde_json::json!("B"));
        // etterna 的 playingExpireAt 缺省仍是 null（与结构体化前的手写 JSON 语义一致）
        assert!(value["sources"]["etterna"]["playingExpireAt"].is_null());
        assert!(REASON_CLOSED_SET.contains(
            &value["sources"]["malody4"]["reason"]
                .as_str()
                .expect("reason 是字符串")
        ));
    }

    #[test]
    fn malody4_source_omits_the_screen_key_when_unknown() {
        let malody4 = Malody4Status::default();
        let value = build_state_frame(false, &[], &EtternaStatus::default(), false, &malody4);
        assert!(value["sources"]["malody4"].get("screen").is_none());
        assert_eq!(value["sources"]["malody4"]["alive"], serde_json::json!(false));
        assert_eq!(value["sources"]["malody4"]["playing"], serde_json::json!(false));
    }

    #[test]
    fn every_unavailable_reason_is_a_member_of_the_contract_closed_set() {
        let causes = [
            UnavailableReason::NoSelection,
            UnavailableReason::ChartNotIndexed,
            UnavailableReason::ProcessNotFound,
            UnavailableReason::MultipleInstances,
            UnavailableReason::AccessDenied,
            UnavailableReason::BadRead,
            UnavailableReason::TargetMismatch("pe_timestamp_mismatch"),
            UnavailableReason::TargetMismatch("file_size_mismatch"),
            UnavailableReason::TargetMismatch("pe_header_out_of_range"),
            UnavailableReason::TargetMismatch("something-else"),
            UnavailableReason::RootNotConfigured,
            UnavailableReason::NoLibrary,
            UnavailableReason::PlatformUnsupported,
        ];
        for cause in causes {
            let text = cause.as_str();
            if text.is_empty() {
                continue; // NoSelection = 正常态：清空 reason
            }
            assert!(
                REASON_CLOSED_SET.contains(&text.as_str()),
                "{text} 不在契约闭集里"
            );
        }
    }

    // ---- dispatch_plan ----

    #[test]
    fn dispatch_plan_signature_change_sends_selection_and_song() {
        let mut state = SongDispatchState::default();
        let plans = dispatch_plan(
            &emit("A", 1.0, "selection", "anchor-changed"),
            &mut state,
            None,
            Some('B'),
        );
        assert_eq!(plans, vec![Dispatch::SelectionFrame, Dispatch::SongFrame]);
        assert_eq!(
            state.last_signature,
            Some(("mdy4:ab".to_string(), 1.0, Some('B')))
        );
        assert_eq!(state.last_conn_id, None, "无客户端时不记连接 id");
    }

    #[test]
    fn dispatch_plan_hidden_sends_only_the_selection_frame() {
        let mut state = SongDispatchState::default();
        let plans = dispatch_plan(
            &Action::Hidden(UnavailableReason::ChartNotIndexed),
            &mut state,
            Some(3),
            Some('B'),
        );
        assert_eq!(plans, vec![Dispatch::SelectionFrame]);
        assert_eq!(songs(&plans), 0);
        assert_eq!(selections(&plans), 1);
    }

    #[test]
    fn dispatch_plan_none_action_sends_no_frames_at_all() {
        let mut state = SongDispatchState::default();
        let plans = dispatch_plan(&Action::None, &mut state, Some(3), Some('B'));
        assert_eq!(plans, vec![Dispatch::None]);
        assert_eq!(songs(&plans), 0);
        assert_eq!(selections(&plans), 0);
    }

    #[test]
    fn dispatch_plan_heartbeat_resend_never_sends_a_song_frame() {
        let mut state = SongDispatchState::default();
        assert_eq!(
            songs(&dispatch_plan(
                &emit("A", 1.0, "selection", "anchor-changed"),
                &mut state,
                Some(7),
                Some('B')
            )),
            1
        );
        // 同签名 + 同连接 + event=heartbeat：只发 selection（2s 心跳不得作废在途分析）
        let heartbeat = dispatch_plan(
            &emit("A", 1.0, "selection", "heartbeat"),
            &mut state,
            Some(7),
            Some('B'),
        );
        assert_eq!(heartbeat, vec![Dispatch::SelectionFrame]);
        // 连续心跳永远不重发 song 帧
        let mut total = 0;
        for _ in 0..50 {
            total += songs(&dispatch_plan(
                &emit("A", 1.0, "selection", "heartbeat"),
                &mut state,
                Some(7),
                Some('B'),
            ));
        }
        assert_eq!(total, 0);
    }

    #[test]
    fn dispatch_plan_same_signature_and_same_conn_id_sends_one_song_frame_only() {
        let mut state = SongDispatchState::default();
        let mut total = 0;
        for _ in 0..20 {
            total += songs(&dispatch_plan(
                &emit("A", 1.5, "selection", "anchor-changed"),
                &mut state,
                Some(11),
                Some('C'),
            ));
        }
        assert_eq!(total, 1, "同一签名 + 同一连接 id 在任意长时间内只发 1 条 song 帧");
    }

    #[test]
    fn dispatch_plan_larger_conn_id_resends_the_song_frame_once() {
        let mut state = SongDispatchState::default();
        // 壳先起、页面后连（首个连接 id）
        assert_eq!(
            songs(&dispatch_plan(
                &emit("A", 1.0, "selection", "heartbeat"),
                &mut state,
                Some(4),
                Some('B')
            )),
            1
        );
        assert_eq!(
            songs(&dispatch_plan(
                &emit("A", 1.0, "selection", "heartbeat"),
                &mut state,
                Some(4),
                Some('B')
            )),
            0
        );
        // 页面刷新 → 更大的连接 id：即便本 tick 是心跳也要补发一次
        assert_eq!(
            songs(&dispatch_plan(
                &emit("A", 1.0, "selection", "heartbeat"),
                &mut state,
                Some(9),
                Some('B')
            )),
            1
        );
        assert_eq!(
            songs(&dispatch_plan(
                &emit("A", 1.0, "selection", "heartbeat"),
                &mut state,
                Some(9),
                Some('B')
            )),
            0
        );
        // 连接被移除后 max_id 变小（ws.rs 的 retain）：绝不能被当成"新连接"
        assert_eq!(
            songs(&dispatch_plan(
                &emit("A", 1.0, "selection", "heartbeat"),
                &mut state,
                Some(4),
                Some('B')
            )),
            0
        );
        assert_eq!(state.last_conn_id, Some(9));
    }

    #[test]
    fn dispatch_plan_screen_transition_does_not_resend_the_song_frame() {
        let mut state = SongDispatchState::default();
        assert_eq!(
            songs(&dispatch_plan(
                &emit("A", 1.0, "selection", "anchor-changed"),
                &mut state,
                Some(5),
                Some('B')
            )),
            1
        );
        let playing = dispatch_plan(
            &emit("A", 1.0, "playing", "scene-changed"),
            &mut state,
            Some(5),
            Some('B'),
        );
        assert_eq!(playing, vec![Dispatch::SelectionFrame]);
        let result = dispatch_plan(
            &emit("A", 1.0, "result", "scene-changed"),
            &mut state,
            Some(5),
            Some('B'),
        );
        assert_eq!(result, vec![Dispatch::SelectionFrame]);
    }

    #[test]
    fn dispatch_plan_judge_change_resends_the_song_frame() {
        let mut state = SongDispatchState::default();
        assert_eq!(
            songs(&dispatch_plan(
                &emit("A", 1.0, "selection", "anchor-changed"),
                &mut state,
                Some(5),
                Some('B')
            )),
            1
        );
        // 同 (identity, rate)、判定 B→C：必须刷新（否则页面的 OD 永远停在旧档）
        assert_eq!(
            songs(&dispatch_plan(
                &emit("A", 1.0, "selection", "heartbeat"),
                &mut state,
                Some(5),
                Some('C')
            )),
            1
        );
        assert_eq!(
            state.last_signature,
            Some(("mdy4:ab".to_string(), 1.0, Some('C')))
        );
        // 判定不变 → 不再重发
        assert_eq!(
            songs(&dispatch_plan(
                &emit("A", 1.0, "selection", "heartbeat"),
                &mut state,
                Some(5),
                Some('C')
            )),
            0
        );
    }

    #[test]
    fn dispatch_plan_unknown_judge_withholds_every_song_frame() {
        let mut state = SongDispatchState::default();
        let plans = dispatch_plan(
            &emit("A", 1.0, "selection", "anchor-changed"),
            &mut state,
            Some(5),
            None,
        );
        assert_eq!(plans, vec![Dispatch::SelectionFrame]);
        assert_eq!(songs(&plans), 0);
        assert_eq!(state.last_signature, None, "withhold 时不记签名");
        // 读到完整 config 后（判定出现）→ 下一 tick 立刻补发 song 帧
        assert_eq!(
            songs(&dispatch_plan(
                &emit("A", 1.0, "selection", "heartbeat"),
                &mut state,
                Some(5),
                Some('C')
            )),
            1
        );
    }

    // ---- FAIR 判定模组位 ----

    #[test]
    fn fair_judge_mod_matches_only_bit_0x400() {
        assert!(!fair_judge_mod(0));
        assert!(fair_judge_mod(MOD_JUDGE_KEY_FAIR));
        assert!(fair_judge_mod(0x410), "与变速位共存时仍命中");
        assert!(fair_judge_mod(u64::MAX));
        assert!(!fair_judge_mod(0x10), "DASH 位不是 FAIR 位");
        assert!(!fair_judge_mod(0x3FF), "FAIR 位以下的位不得误命中");
        assert!(FAIR_JUDGE_WARNING.contains("FAIR judge mod detected"));
    }

    // ---- 锚点读取：硬失败 / 软失败 ----

    /// 一次"软失败"读取：指针槽位读到了、内容当前不是身份键。
    fn soft_read() -> Result<anchor::IdentityRead, AnchorError> {
        Ok(anchor::IdentityRead::Unparsable)
    }

    #[test]
    fn nine_consecutive_soft_failures_are_tolerated_and_the_tenth_detaches() {
        let mut tolerance = SoftReadTolerance::new();
        for step in 1..SOFT_READ_TOLERANCE {
            assert_eq!(
                read_action(&mut tolerance, soft_read()),
                ReadAction::Tolerated,
                "第 {step} 次连续软失败仍在容忍窗口内（不 detach）"
            );
            assert_eq!(tolerance.consecutive(), step);
        }
        assert_eq!(
            read_action(&mut tolerance, soft_read()),
            ReadAction::Detach(AnchorError::BadRead),
            "第 {SOFT_READ_TOLERANCE} 次连续软失败才 detach"
        );
        // N = 10 ≈ 2s @ 200ms tick（阈值来自常量，不是散落的字面量）
        assert_eq!(SOFT_READ_TOLERANCE, 10);
        assert_eq!(TICK * SOFT_READ_TOLERANCE, Duration::from_secs(2));
    }

    #[test]
    fn hard_anchor_read_failure_detaches_immediately() {
        // 硬失败：第一次就 detach，且不进软失败计数（ProcessNotFound/AccessDenied/BadRead 同档）
        for err in [
            AnchorError::BadRead,
            AnchorError::AccessDenied,
            AnchorError::NotFound,
        ] {
            let mut fresh = SoftReadTolerance::new();
            assert_eq!(
                read_action(&mut fresh, Err(err.clone())),
                ReadAction::Detach(err.clone())
            );
            assert_eq!(fresh.consecutive(), 0, "硬失败不计容忍次数");
            // 连续硬失败同样立即 detach（不叠加到 10 次）
            assert_eq!(
                read_action(&mut fresh, Err(err.clone())),
                ReadAction::Detach(err)
            );
        }
        // 已攒了 9 次软失败时再来一次硬失败：仍然立即 detach（不被容忍窗口推迟）
        let mut tolerance = SoftReadTolerance::new();
        for _ in 0..SOFT_READ_TOLERANCE - 1 {
            assert_eq!(read_action(&mut tolerance, soft_read()), ReadAction::Tolerated);
        }
        assert_eq!(
            read_action(&mut tolerance, Err(AnchorError::NotFound)),
            ReadAction::Detach(AnchorError::NotFound)
        );
    }

    #[test]
    fn a_successful_anchor_read_resets_the_soft_failure_counter() {
        let mut tolerance = SoftReadTolerance::new();
        for _ in 0..SOFT_READ_TOLERANCE - 1 {
            assert_eq!(read_action(&mut tolerance, soft_read()), ReadAction::Tolerated);
        }
        assert_eq!(tolerance.consecutive(), 9);

        // 读到身份键 → 清零：再来的软失败从 1 重新数，绝不会"接着 9 次"立刻 detach
        let key = IdentityKey {
            md5: "ab".repeat(16),
            slot: 7,
        };
        assert_eq!(
            read_action(&mut tolerance, Ok(anchor::IdentityRead::Key(key.clone()))),
            ReadAction::Healthy(Some(key))
        );
        assert_eq!(tolerance.consecutive(), 0);
        for _ in 0..SOFT_READ_TOLERANCE - 1 {
            assert_eq!(read_action(&mut tolerance, soft_read()), ReadAction::Tolerated);
        }

        // 指针为 0（游戏在跑但没选中谱面）同样是"读成功"：清零、绝不 detach
        assert_eq!(
            read_action(&mut tolerance, Ok(anchor::IdentityRead::Empty)),
            ReadAction::Healthy(None)
        );
        assert_eq!(tolerance.consecutive(), 0);
    }

    // ---- config.json 的内容感知缓存 ----

    #[test]
    fn config_cache_treats_identical_bytes_as_unchanged() {
        let dir = tmp_dir("cfg-same");
        let path = dir.join("config.json");
        let text = r#"{"user_mods":32,"user_judge_level":1}"#;
        fs::write(&path, text).unwrap();

        let mut cache = ConfigCache::default();
        assert_eq!(
            cache.read(&path).cloned(),
            Some(GameConfig {
                user_mods: 32,
                user_judge_level: 1
            })
        );
        assert_eq!(cache.bytes.as_deref(), Some(text.as_bytes()));

        // 把缓存值改成"重新解析绝不会得到"的值：下次读若判为未变化，就绝不会替换它
        cache.value = Some(GameConfig {
            user_mods: 0,
            user_judge_level: 0,
        });
        assert_eq!(
            cache.read(&path).cloned(),
            Some(GameConfig {
                user_mods: 0,
                user_judge_level: 0
            }),
            "同路径 + 同字节 ⇒ 视为未变化（不重新解析、不替换缓存值）"
        );

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn config_cache_detects_changed_bytes_with_an_unchanged_mtime_and_len() {
        let dir = tmp_dir("cfg-bytes");
        let path = dir.join("config.json");
        let before = r#"{"user_mods":32,"user_judge_level":1}"#;
        let after = r#"{"user_mods":16,"user_judge_level":3}"#;
        assert_eq!(before.len(), after.len(), "本用例要求两次内容等长");

        fs::write(&path, before).unwrap();
        let sig = |path: &Path| {
            fs::metadata(path)
                .ok()
                .and_then(|meta| meta.modified().ok().map(|mtime| (mtime, meta.len())))
        };
        let sig_before = sig(&path).unwrap();

        let mut cache = ConfigCache::default();
        assert_eq!(
            cache.read(&path).cloned(),
            Some(GameConfig {
                user_mods: 32,
                user_judge_level: 1
            })
        );

        // 整份重写（等长），再把 mtime 恢复成原值 → `(mtime, len)` 完全没变，
        // 模拟"两次重写落在同一文件系统时间戳粒度内"（只比签名的旧实现永远看不见这次改动）
        fs::write(&path, after).unwrap();
        let file = fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.set_modified(sig_before.0).unwrap();
        drop(file);
        assert_eq!(sig(&path).unwrap(), sig_before, "本用例要求 (mtime, len) 逐字段相同");

        assert_eq!(
            cache.read(&path).cloned(),
            Some(GameConfig {
                user_mods: 16,
                user_judge_level: 3
            }),
            "字节变了就必须重新解析（判据是内容，(mtime, len) 只是廉价提示）"
        );
        assert_eq!(cache.bytes.as_deref(), Some(after.as_bytes()));

        fs::remove_dir_all(&dir).unwrap();
    }

    // ---- IndexShared / 日志尾随 ----

    // ---- 线上 reason ----

    #[test]
    fn wire_reason_covers_every_closed_set_branch() {
        let healthy = emit("A", 1.0, "selection", "heartbeat");
        let no_selection = Action::Hidden(UnavailableReason::NoSelection);
        let not_indexed = Action::Hidden(UnavailableReason::ChartNotIndexed);

        // 未收录：blocker 为空时由状态机给出（否则该成因在线上不可见）
        assert_eq!(wire_reason(None, &not_indexed, ""), "chart-not-indexed");
        // 未选中 = 正常态 → 清空
        assert_eq!(wire_reason(None, &no_selection, "chart-not-indexed"), "");
        // 健康 → 清空
        assert_eq!(wire_reason(None, &healthy, "process-not-found"), "");
        // 附着/根目录层面的成因优先，且不依赖本 tick 是否发了帧
        assert_eq!(
            wire_reason(Some(&UnavailableReason::ProcessNotFound), &Action::None, ""),
            "process-not-found"
        );
        assert_eq!(
            wire_reason(
                Some(&UnavailableReason::TargetMismatch("pe_timestamp_mismatch")),
                &no_selection,
                ""
            ),
            "target-mismatch:pe_timestamp_mismatch"
        );
        // 心跳之间的 tick：沿用上一状态（不闪）
        assert_eq!(
            wire_reason(None, &Action::None, "chart-not-indexed"),
            "chart-not-indexed"
        );
        // 所有取值都在契约闭集内（空串 = 正常态，也允许）
        for text in [
            wire_reason(None, &not_indexed, ""),
            wire_reason(None, &no_selection, ""),
            wire_reason(None, &healthy, ""),
            wire_reason(Some(&UnavailableReason::NoLibrary), &Action::None, ""),
            wire_reason(Some(&UnavailableReason::RootNotConfigured), &Action::None, ""),
        ] {
            assert!(
                text.is_empty() || REASON_CLOSED_SET.contains(&text.as_str()),
                "reason 越出闭集：{text}"
            );
        }
    }

    #[test]
    fn index_shared_starts_empty_and_accepts_rebuild_requests() {        let idx = IndexShared::new();
        assert!(!index_ready(&idx));
        assert_eq!(lookup(&idx, "00"), None);
        assert_eq!(meta_of(&idx, Path::new("nope.mc")), None);
        request_rebuild(&idx);
        assert!(idx.rebuild_requested.load(Ordering::Relaxed));
    }

    #[test]
    fn log_tail_reads_new_bytes_and_keeps_half_lines() {
        let dir = tmp_dir("tail");
        let log_dir = dir.join("log");
        fs::create_dir_all(&log_dir).unwrap();
        let path = log_dir.join("log-20260921T171305+0800.txt");
        fs::write(&path, "1.000 [MS] LOG: switch to 1\n").unwrap();

        let mut tail = LogTail::new();
        let mut tracker = SceneTracker::new();
        tail.follow(&log_dir, &mut tracker);
        assert_eq!(tracker.screen(), Screen::Selection);

        // 半行（无换行）不得被解析
        let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        write!(file, "2.000 [MS] LOG: switch to 3").unwrap();
        drop(file);
        tail.follow(&log_dir, &mut tracker);
        assert_eq!(tracker.screen(), Screen::Selection, "半行不生效");

        // 补上换行 → 半行与新增字节拼接后生效
        let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        writeln!(file).unwrap();
        drop(file);
        tail.follow(&log_dir, &mut tracker);
        assert_eq!(tracker.screen(), Screen::Playing);

        // 新文件（游戏新开一局）→ 从 0 读起
        let newer = log_dir.join("log-20260922T090000+0800.txt");
        fs::write(&newer, "9.000 [MS] LOG: switch to 4\n").unwrap();
        tail.follow(&log_dir, &mut tracker);
        assert_eq!(tracker.screen(), Screen::Result);

        // 日志目录整个消失 → 保留最后已知场景（playing 由 fresh(10s) 自然失鲜）
        fs::remove_dir_all(&log_dir).unwrap();
        tail.follow(&log_dir, &mut tracker);
        assert_eq!(tracker.screen(), Screen::Result);

        fs::remove_dir_all(&dir).unwrap();
    }
}
