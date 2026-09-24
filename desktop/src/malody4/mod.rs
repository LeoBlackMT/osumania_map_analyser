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

use crate::etterna::EtternaStatus;
use crate::frames::{
    EtternaSource, Malody4Source, MalodySource, SourcesFrame, StateFrame, MAX_PAYLOAD_BYTES,
};
use crate::server::log::log_at;
use crate::server::{broadcast, Shared};
use self::anchor::{AnchorError, IdentityKey, MemoryProbe, MemorySource, RawSettings};
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

// --------------------------------------------- 判定档 / 变速位：取值来源 --

/// 本 tick 判定档与变速位的**来源**（诊断与日志用；不进任何帧）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsOrigin {
    /// 游戏进程内存：`S`（用户设置单例）优先，`S` 读不到时用 `P`（本局 play-config）。
    Memory,
    /// `config.json` 兜底（内存这条链现在读不到）。
    ConfigJson,
}

impl SettingsOrigin {
    /// 稳定短字面量（进壳日志，不进任何帧）。
    pub fn as_str(&self) -> &'static str {
        match self {
            SettingsOrigin::Memory => "memory",
            SettingsOrigin::ConfigJson => "config.json",
        }
    }
}

/// 本 tick 实际使用的判定档 / 速率 / `user_mods`（纯函数 `effective_settings` 的产物）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EffectiveSettings {
    pub source: SettingsOrigin,
    /// 判定档字母；`None` = 判定未知（song 帧 withhold，既有行为不变）。
    pub judge: Option<char>,
    /// 变速位换算出的速率。
    pub rate: f64,
    /// `user_mods` 位掩码（FAIR 位判定用）：未知位原样带过来，不裁剪。
    pub user_mods: u64,
}

/// 取值优先级（纯函数）：**内存读到就赢**（游戏里改判定 / 改变速位立即生效），
/// 否则回落 `config.json`。
///
/// 内存读数在 `anchor` 里已经过完所有 fail-closed 校验——指针为 0 / 不对齐 / 超出 32 位用户
/// 地址空间 / 对象块短读 / 判定档越界 / `P` 的子集不变量不成立，都会让对应的链读不出来——
/// 到这里要么是一个校验过的值，要么是 `None`。本函数**不再二次猜测**，也绝不把两条链拼起来用。
///
/// 两个来源都给不出值 → `judge = None`（song 帧 withhold 的既有路径）、速率 1.0。
pub fn effective_settings(
    memory: Option<(MemorySource, RawSettings)>,
    file: Option<&GameConfig>,
) -> EffectiveSettings {
    if let Some((_, raw)) = memory {
        // 内存里的判定档 / `user_mods` 与 config.json 是**同一组设置**（该文件正是从这两个偏移
        // 写出去的），因此直接复用 `GameConfig` 的解释路径：判定档 → JudgeLevel → 字母，
        // 位掩码 → ModFlags → 速率。未知位一律不解释（`ModFlags` 只看三个变速位）。
        let view = GameConfig {
            user_mods: u64::from(raw.user_mods),
            user_judge_level: u64::from(raw.judge),
        };
        return EffectiveSettings {
            source: SettingsOrigin::Memory,
            judge: view.judge_letter(),
            rate: view.mod_flags().speed_rate(),
            user_mods: view.user_mods,
        };
    }
    match file {
        Some(config) => EffectiveSettings {
            source: SettingsOrigin::ConfigJson,
            judge: config.judge_letter(),
            rate: config.mod_flags().speed_rate(),
            user_mods: config.user_mods,
        },
        None => EffectiveSettings {
            source: SettingsOrigin::ConfigJson,
            judge: None,
            rate: 1.0,
            user_mods: 0,
        },
    }
}

/// 判定档的诊断文本（`None` = 判定未知）。
fn judge_text(judge: Option<char>) -> String {
    judge
        .map(|letter| letter.to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

/// **info** 诊断行的文案（纯函数）：来源 + 判定字母 + 速率。
/// 用户据此确认"游戏里一改就生效"（不必看十六进制转储），也是"值从哪来"的唯一落点。
fn settings_log(effective: &EffectiveSettings) -> String {
    format!(
        "malody4 settings: source={} judge={} speed_rate={:.2}",
        effective.source.as_str(),
        judge_text(effective.judge),
        effective.rate
    )
}

/// `S` / `P` 持续不一致的一次性告警文案（纯函数；两边的原始值都给出来便于定位）。
fn chain_disagreement_warning(user: RawSettings, play: RawSettings, polls: u32) -> String {
    format!(
        "malody4 settings chains disagree for {polls} consecutive polls: user-settings judge={} user_mods={:#x} vs play-config judge={} user_mods={:#x} — publishing the play-config value",
        user.judge, user.user_mods, play.judge, play.user_mods
    )
}

/// 内存值与 `config.json` 不一致时的一次性告警文案（纯函数；两边都给出来）。
fn file_cross_check_warning(effective: &EffectiveSettings, file: &GameConfig) -> String {
    format!(
        "malody4 settings cross-check: memory judge={} speed_rate={:.2} vs config.json judge={} speed_rate={:.2} — keeping the memory value (the file is only rewritten at game start/exit)",
        judge_text(effective.judge),
        effective.rate,
        judge_text(file.judge_letter()),
        file.mod_flags().speed_rate()
    )
}

/// info 行（`settings_log`）的去重键：来源 + 判定档 + 速率。**同值不记**（绝不逐 tick 刷）。
#[derive(Debug, Clone, Copy, PartialEq)]
struct SettingsLogKey {
    source: SettingsOrigin,
    judge: Option<char>,
    rate: f64,
}

impl SettingsLogKey {
    /// 速率按 `RATE_EPS` 容差比较（来源与判定档精确比较）。
    fn matches(&self, other: &Self) -> bool {
        self.source == other.source
            && self.judge == other.judge
            && (self.rate - other.rate).abs() < RATE_EPS
    }
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
mod tests {
    use super::*;
    use std::io::Write as _;

    /// 契约 §8 的 `reason` 闭集（逐字）。
    const REASON_CLOSED_SET: [&str; 14] = [
        "chart-not-indexed",
        "chart-unresolved",
        "chart-unknown-identity",
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

    // ---- state_push_due（state 帧的立即补发判定） ----

    /// 基线状态：通道健康、已选中谱面、判定已知。
    fn settled_state() -> Malody4Status {
        Malody4Status {
            alive: true,
            screen: "playing".to_string(),
            playing: true,
            reason: String::new(),
            judge: Some('B'),
        }
    }

    /// 真机缺陷复现：`chart-unknown-identity` 既不改 `alive` 也不改 `playing`，只比较后两者时
    /// 页面要等 30s 周期帧才知道"这张谱无法识别"——用户坐在卡片上什么也看不到。判定必须当拍为真。
    /// 同时钉死反方向：**完全不变的 tick 必须为假**（否则就是每 tick 广播）。
    #[test]
    fn state_push_due_fires_on_a_reason_change_and_not_on_an_unchanged_tick() {
        let base = settled_state();
        let same = base.clone();
        // 不变的 tick：不发
        assert!(!state_push_due(&base, &same), "同值不得补发");
        // 只有 reason 变了（身份键在本库里查不到）→ 必须当拍补发
        let unknown = Malody4Status {
            reason: UnavailableReason::ChartUnknownIdentity.as_str().to_string(),
            ..base.clone()
        };
        assert_eq!(unknown.reason, "chart-unknown-identity");
        assert!(state_push_due(&base, &unknown), "reason 变化必须补发");
        // 反向：reason 从有到无（提示要消失）同样是变化
        assert!(state_push_due(&unknown, &base), "reason 清空必须补发");
        // 其余进帧的字段：逐个改一次都必须命中（字段表漏一个就会重演同一个缺陷）
        let variants = [
            Malody4Status { alive: !base.alive, ..base.clone() },
            Malody4Status { playing: !base.playing, ..base.clone() },
            Malody4Status { screen: "result".to_string(), ..base.clone() },
            Malody4Status { reason: "chart-not-indexed".to_string(), ..base.clone() },
            Malody4Status { judge: None, ..base.clone() },
        ];
        for (index, variant) in variants.iter().enumerate() {
            assert!(state_push_due(&base, variant), "字段 {index} 变化必须补发");
            assert!(!state_push_due(variant, variant), "字段 {index} 不变不得补发");
        }
    }

    #[test]
    fn every_unavailable_reason_is_a_member_of_the_contract_closed_set() {
        let causes = [
            UnavailableReason::NoSelection,
            UnavailableReason::ChartNotIndexed,
            UnavailableReason::ChartUnresolved,
            UnavailableReason::ChartUnknownIdentity,
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

    // ---- 判定档 / 变速位：内存优先，config.json 兜底 ----

    fn raw(judge: u8, mods: u32) -> RawSettings {
        RawSettings {
            judge,
            user_mods: mods,
        }
    }

    /// 内存读到就赢：同一 tick 内游戏里改的值必须立刻压过文件里的旧值（本特性的全部意义）。
    #[test]
    fn effective_settings_prefers_the_memory_value_over_config_json() {
        // 文件：RUSH + 判定 B（游戏开局时写下的旧值）；内存：SLOW + 判定 E（刚在游戏里改的）
        let file = GameConfig {
            user_mods: 0x20,
            user_judge_level: 1,
        };
        let memory = raw(4, 0x100);
        // 正常情形：发布链是 `P`（本局 play-config）
        let effective = effective_settings(Some((MemorySource::PlayConfig, memory)), Some(&file));
        assert_eq!(effective.source, SettingsOrigin::Memory);
        assert_eq!(effective.source.as_str(), "memory");
        assert_eq!(effective.judge, Some('E'));
        assert_eq!(effective.rate, 0.8);
        assert_eq!(effective.user_mods, 0x100);
        // `P` 读不到、由 `S` 顶上时同样是内存来源（发布链不同不改来源语义）
        let from_user = effective_settings(Some((MemorySource::UserSettings, memory)), Some(&file));
        assert_eq!(from_user, effective);
    }

    /// 内存"现在读不到"（指针 0 / 不变量不成立 / 短读 → `None`）→ 用文件值，绝不发半份。
    #[test]
    fn effective_settings_falls_back_to_config_json_when_memory_is_not_ready() {
        let file = GameConfig {
            user_mods: 0x10,
            user_judge_level: 2,
        };
        let effective = effective_settings(None, Some(&file));
        assert_eq!(effective.source, SettingsOrigin::ConfigJson);
        assert_eq!(effective.source.as_str(), "config.json");
        assert_eq!(effective.judge, Some('C'));
        assert_eq!(effective.rate, 1.2);
        assert_eq!(effective.user_mods, 0x10);
    }

    /// 两个来源都给不出值 → 判定未知（走既有的 song 帧 withhold 路径），速率 1.0。
    #[test]
    fn effective_settings_without_any_source_is_unknown_judge_and_unity_rate() {
        let effective = effective_settings(None, None);
        assert_eq!(effective.judge, None);
        assert_eq!(effective.rate, 1.0);
        assert_eq!(effective.user_mods, 0);
        assert_eq!(effective.source, SettingsOrigin::ConfigJson);
    }

    /// 端到端：内存原始值 → `JudgeLevel` / `ModFlags` → 有效值 → song 帧签名
    /// `(identity, rate, judge)`。判定或速率一变就必须重发 song 帧（页面据此重算 OD）。
    #[test]
    fn a_memory_change_moves_the_judge_letter_the_rate_and_the_song_signature() {
        // 文件里是 RUSH + B：内存值必须盖过它
        let file = GameConfig {
            user_mods: 0x20,
            user_judge_level: 1,
        };
        let mut state = SongDispatchState::default();
        let before = effective_settings(Some((MemorySource::UserSettings, raw(1, 0x20))), Some(&file));
        assert_eq!((before.judge, before.rate), (Some('B'), 1.5));
        assert_eq!(
            songs(&dispatch_plan(
                &emit("A", before.rate, "selection", "anchor-changed"),
                &mut state,
                Some(5),
                before.judge
            )),
            1
        );

        // 只改判定 B → D（判定档 UI 处理器写的就是内存里的这两个偏移）
        let judge_only = effective_settings(Some((MemorySource::UserSettings, raw(3, 0x20))), Some(&file));
        assert_eq!((judge_only.judge, judge_only.rate), (Some('D'), 1.5));
        assert_eq!(
            songs(&dispatch_plan(
                &emit("A", judge_only.rate, "selection", "heartbeat"),
                &mut state,
                Some(5),
                judge_only.judge
            )),
            1,
            "判定变化必须重发 song 帧（签名里就有 judge）"
        );

        // 只改变速位 RUSH → SLOW
        let rate_only = effective_settings(Some((MemorySource::UserSettings, raw(3, 0x100))), Some(&file));
        assert_eq!((rate_only.judge, rate_only.rate), (Some('D'), 0.8));
        assert_eq!(
            songs(&dispatch_plan(
                &emit("A", rate_only.rate, "selection", "heartbeat"),
                &mut state,
                Some(5),
                rate_only.judge
            )),
            1,
            "速率变化同样在签名里"
        );
        assert_eq!(
            state.last_signature,
            Some(("mdy4:ab".to_string(), 0.8, Some('D')))
        );

        // 同值再来一次：不重发（签名路径不需要任何改动）
        assert_eq!(
            songs(&dispatch_plan(
                &emit("A", rate_only.rate, "selection", "heartbeat"),
                &mut state,
                Some(5),
                rate_only.judge
            )),
            0
        );
    }

    /// 内存来源的 FAIR 位（0x400）同样触发那条一次性 warning；未知位不干扰。
    #[test]
    fn the_fair_bit_is_honoured_from_the_memory_value_too() {
        let memory = effective_settings(Some((MemorySource::UserSettings, raw(2, 0x400 | 0x8))), None);
        assert!(fair_judge_mod(memory.user_mods), "内存值里的 FAIR 位必须能触发提示");
        assert_eq!(memory.rate, 1.0, "FAIR 位与未知位都不是变速位");
        // 只置 FAIR 位时文件路径的口径不变
        let file = effective_settings(
            None,
            Some(&GameConfig {
                user_mods: 0x400,
                user_judge_level: 0,
            }),
        );
        assert!(fair_judge_mod(file.user_mods));
    }

    /// info 行的逐字文案：来源 + 判定字母 + 速率（用户靠它确认"改了立刻生效"）。
    #[test]
    fn settings_log_names_the_source_the_judge_and_the_rate() {
        assert_eq!(
            settings_log(&effective_settings(
                Some((MemorySource::UserSettings, raw(3, 0x100))),
                None
            )),
            "malody4 settings: source=memory judge=D speed_rate=0.80"
        );
        assert_eq!(
            settings_log(&effective_settings(
                None,
                Some(&GameConfig {
                    user_mods: 0x20,
                    user_judge_level: 0
                })
            )),
            "malody4 settings: source=config.json judge=A speed_rate=1.50"
        );
        assert_eq!(
            settings_log(&effective_settings(None, None)),
            "malody4 settings: source=config.json judge=unknown speed_rate=1.00"
        );
    }

    /// **只有变化才记**：同值（含 `RATE_EPS` 容差内的同值）一律不记 —— 绝不逐 tick 刷。
    #[test]
    fn the_settings_log_is_deduplicated_on_source_judge_and_rate() {
        let (_, mut runtime) = runtime_for_tests();
        let first = effective_settings(Some((MemorySource::UserSettings, raw(1, 0x20))), None);
        assert!(runtime.log_settings_change(&first), "首次取值要记一条");
        assert_eq!(
            runtime.last_settings,
            Some(SettingsLogKey {
                source: SettingsOrigin::Memory,
                judge: Some('B'),
                rate: 1.5
            })
        );
        assert!(
            !runtime.log_settings_change(&first),
            "同值不记（200ms 一拍，逐 tick 记会淹掉日志）"
        );
        // 浮点容差内的"同值"同样不记
        let noisy = EffectiveSettings {
            rate: first.rate + 1e-9,
            ..first
        };
        assert!(!runtime.log_settings_change(&noisy));

        // 判定变了 → 记
        let changed = effective_settings(Some((MemorySource::UserSettings, raw(2, 0x20))), None);
        assert!(runtime.log_settings_change(&changed));
        // 来源变了（同样的数字，但从内存变成文件）→ 也记：来源本身就是诊断信息
        let from_file = effective_settings(
            None,
            Some(&GameConfig {
                user_mods: 0x20,
                user_judge_level: 2,
            }),
        );
        assert!(runtime.log_settings_change(&from_file));
        assert_eq!(
            runtime.last_settings.map(|key| key.source),
            Some(SettingsOrigin::ConfigJson)
        );
    }

    /// 两条链持续不一致（> 两拍）才记一次 warn；一致时计数清零；发布值不受影响。
    #[test]
    fn the_chain_disagreement_warning_needs_several_consecutive_polls_and_fires_once() {
        let (_, mut runtime) = runtime_for_tests();
        let user = raw(1, 0x20);
        let play = raw(4, 0x100);
        let disagree = MemoryProbe {
            user_settings: Some(user),
            play_config: Some(play),
        };
        let agree = MemoryProbe {
            user_settings: Some(user),
            play_config: Some(user),
        };
        assert!(CHAIN_DISAGREE_POLLS >= 3, "阈值必须严于两拍");
        for step in 1..CHAIN_DISAGREE_POLLS {
            runtime.note_chain_disagreement(&disagree);
            assert_eq!(runtime.chain_disagree, step);
            assert!(!runtime.chain_disagree_logged, "未到阈值不得告警");
        }
        runtime.note_chain_disagreement(&disagree);
        assert!(runtime.chain_disagree_logged, "连续到阈值 → 记一次");
        // 一致一次 → 计数清零（只判"持续"）
        runtime.note_chain_disagreement(&agree);
        assert_eq!(runtime.chain_disagree, 0);
        // 只有一条链读不到时也清零，且不告警
        runtime.note_chain_disagreement(&MemoryProbe {
            user_settings: Some(user),
            play_config: None,
        });
        assert_eq!(runtime.chain_disagree, 0);
        // 告警文案：两边都给出来，并说明发布的是哪一边
        let line = chain_disagreement_warning(user, play, CHAIN_DISAGREE_POLLS);
        assert!(line.starts_with("malody4 settings chains disagree"), "{line}");
        assert!(line.contains("consecutive polls"), "{line}");
        assert!(line.contains("judge=1") && line.contains("judge=4"), "{line}");
        assert!(line.contains("0x20") && line.contains("0x100"), "{line}");
        assert!(line.contains("publishing the play-config value"), "{line}");
    }

    /// 与 `config.json` 的交叉校验是**软信号**：只在附着窗口内比一次，分歧照用内存值。
    #[test]
    fn the_file_cross_check_is_soft_and_runs_once_inside_the_attach_window() {
        let (_, mut runtime) = runtime_for_tests();
        let file = GameConfig {
            user_mods: 0x20,
            user_judge_level: 1,
        };
        let now = Instant::now();
        let memory = effective_settings(Some((MemorySource::UserSettings, raw(4, 0x100))), Some(&file));

        // 未附着 → 不比、不立 flag
        assert!(!runtime.note_file_cross_check(&memory, Some(&file), now));
        assert!(!runtime.settings_cross_checked);

        // 窗口内：分歧 → 记一条，且不再比
        runtime.attached_at = Some(now);
        assert!(runtime.note_file_cross_check(&memory, Some(&file), now));
        assert!(runtime.settings_cross_checked);
        assert!(
            !runtime.note_file_cross_check(&memory, Some(&file), now),
            "每个附着窗口最多一条"
        );

        // 文案里两个数字都给出来，且明确"用内存值、不是拒绝它"
        let line = file_cross_check_warning(&memory, &file);
        assert!(line.contains("memory judge=E speed_rate=0.80"), "{line}");
        assert!(line.contains("config.json judge=B speed_rate=1.50"), "{line}");
        assert!(line.contains("keeping the memory value"), "{line}");

        // 一致时静默（同样的比较口径：判定 + 速率）
        let (_, mut agree_runtime) = runtime_for_tests();
        agree_runtime.attached_at = Some(now);
        let agree = effective_settings(Some((MemorySource::UserSettings, raw(1, 0x20))), Some(&file));
        assert_eq!(agree.judge, file.judge_letter());
        assert_eq!(agree.rate, file.mod_flags().speed_rate());
        assert!(!agree_runtime.note_file_cross_check(&agree, Some(&file), now));
        assert!(agree_runtime.settings_cross_checked, "比过一次就不再比");

        // 窗口外（附着 30s 之后）：直接判过，不记
        let (_, mut late) = runtime_for_tests();
        late.attached_at = Some(now);
        assert!(!late.note_file_cross_check(
            &memory,
            Some(&file),
            now + SETTINGS_CROSS_CHECK + Duration::from_millis(1)
        ));
        assert!(late.settings_cross_checked);

        // 内存没读到（发布的**就是**文件值）→ 没有可交叉校验的对象，flag 不立
        let (_, mut fallback) = runtime_for_tests();
        fallback.attached_at = Some(now);
        let from_file = effective_settings(None, Some(&file));
        assert!(!fallback.note_file_cross_check(&from_file, Some(&file), now));
        assert!(!fallback.settings_cross_checked);
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

    // ---- miss 的重建预算 ----

    /// 现场日志里那个查不到的身份键（真机 25 个身份键中 19 个不在盘上）。
    const MISSING_MD5: &str = "5a6d5fe073b0b2a8fe5bac4167cff5c8";

    fn runtime_for_tests() -> (Arc<IndexShared>, Runtime) {
        let idx = Arc::new(IndexShared::new());
        let (root_tx, _root_rx) = mpsc::channel::<PathBuf>();
        (idx.clone(), Runtime::new(idx, root_tx))
    }

    /// 持续 miss 的身份键最多只置 `MISS_REBUILD_ATTEMPTS` 次重建位，之后**永远**不再请求
    /// （旧行为：只受 5s 节流、永不放弃 → 整库每 5s 重哈希一次）。
    #[test]
    fn a_persistent_miss_requests_rebuilds_at_most_the_attempt_budget() {
        let (idx, mut runtime) = runtime_for_tests();
        let start = Instant::now();
        let mut requested = 0u32;
        // 30 分钟 @200ms 的连续 miss（远超节流窗口，预算若没封顶会一直置位）
        for step in 0..9000u32 {
            // 模拟构建线程把请求位消费掉
            idx.rebuild_requested.store(false, Ordering::Relaxed);
            runtime.request_miss_rebuild(MISSING_MD5, start + POLL_INTERVAL * step);
            if idx.rebuild_requested.load(Ordering::Relaxed) {
                requested += 1;
            }
        }
        assert_eq!(
            requested, MISS_REBUILD_ATTEMPTS,
            "同一个 md5 的重建请求次数必须封顶在 MISS_REBUILD_ATTEMPTS"
        );
        assert_eq!(MISS_REBUILD_ATTEMPTS, 2, "预算取小值：2 次重建 ≈ 10s 内试完就沉默");
        assert!(runtime.miss_tracker.is_unresolvable(MISSING_MD5));
        assert_eq!(
            runtime.miss_tracker.attempts(MISSING_MD5),
            MISS_REBUILD_ATTEMPTS
        );
    }

    /// 预算在两次请求之间必须**隔着节流窗口**：一次 200ms 的连续 miss 不能把预算烧光
    /// （否则重建还没跑完，身份键就被误判成"本会话不可解析"）。
    #[test]
    fn the_budget_is_spent_at_the_throttled_cadence_not_per_tick() {
        let (idx, mut runtime) = runtime_for_tests();
        let start = Instant::now();
        idx.rebuild_requested.store(false, Ordering::Relaxed);
        runtime.request_miss_rebuild(MISSING_MD5, start);
        assert!(idx.rebuild_requested.load(Ordering::Relaxed));
        // 节流窗口内的 24 个 tick：一次都不算尝试，也不置位
        for step in 1..25u32 {
            idx.rebuild_requested.store(false, Ordering::Relaxed);
            runtime.request_miss_rebuild(MISSING_MD5, start + POLL_INTERVAL * step);
            assert!(
                !idx.rebuild_requested.load(Ordering::Relaxed),
                "节流窗口内不得重复请求"
            );
        }
        assert_eq!(runtime.miss_tracker.attempts(MISSING_MD5), 1);
        // 窗口一过：第 2 次（也是最后一次）尝试
        runtime.request_miss_rebuild(MISSING_MD5, start + INDEX_REBUILD_THROTTLE);
        assert!(idx.rebuild_requested.load(Ordering::Relaxed));
        assert_eq!(runtime.miss_tracker.attempts(MISSING_MD5), MISS_REBUILD_ATTEMPTS);
    }

    /// 预算与放弃记录**按 md5 各自独立**；命中（lookup 成功）不碰它们，也不会解开
    /// 已判定不可解析的身份键——只有"库真的变了"才放开。
    #[test]
    fn the_budget_is_per_identity_and_a_hit_never_unblocks_it() {
        let (idx, mut runtime) = runtime_for_tests();
        let other = "0123456789abcdef0123456789abcdef";
        let start = Instant::now();
        // 先用光 MISSING_MD5 的预算
        let mut now = start;
        for _ in 0..MISS_REBUILD_ATTEMPTS + 1 {
            idx.rebuild_requested.store(false, Ordering::Relaxed);
            runtime.request_miss_rebuild(MISSING_MD5, now);
            now += INDEX_REBUILD_THROTTLE;
        }
        assert!(runtime.miss_tracker.is_unresolvable(MISSING_MD5));

        // 另一个 md5 有独立的预算：不会被前者的放弃状态牵连
        idx.rebuild_requested.store(false, Ordering::Relaxed);
        runtime.request_miss_rebuild(other, now);
        assert!(
            idx.rebuild_requested.load(Ordering::Relaxed),
            "另一个身份键仍应拿到自己的第 1 次重建"
        );
        assert_eq!(runtime.miss_tracker.attempts(other), 1);

        // 命中一张在库里的谱面：不碰预算、不解开任何放弃状态
        runtime.log_hit(other, Path::new("D:/Games/Malody-4.3.7/beatmap/x/0/x.mc"));
        assert!(runtime.miss_tracker.is_unresolvable(MISSING_MD5));
        assert_eq!(runtime.miss_tracker.attempts(MISSING_MD5), MISS_REBUILD_ATTEMPTS);
        assert_eq!(runtime.miss_tracker.attempts(other), 1);

        // 库真的变了（周期重扫检出变化 / 根目录换了）→ 放开：预算与放弃记录一起清空
        idx.content_revision.fetch_add(1, Ordering::Relaxed);
        runtime.sync_content_revision();
        assert!(!runtime.miss_tracker.is_unresolvable(MISSING_MD5));
        assert_eq!(runtime.miss_tracker.attempts(MISSING_MD5), 0);
        idx.rebuild_requested.store(false, Ordering::Relaxed);
        runtime.request_miss_rebuild(MISSING_MD5, now + INDEX_REBUILD_THROTTLE);
        assert!(
            idx.rebuild_requested.load(Ordering::Relaxed),
            "库变了之后，此前不可解析的身份键要重新拿到尝试机会"
        );
        // 同修订号再同步一次不得重复清空（否则预算等于没有）
        assert_eq!(runtime.miss_tracker.attempts(MISSING_MD5), 1);
        runtime.sync_content_revision();
        assert_eq!(
            runtime.miss_tracker.attempts(MISSING_MD5),
            1,
            "修订号没变 ⇒ 不清空预算"
        );

        // miss 路径自己绝不改修订号：只有构建线程在"树真的变了"的重建上 +1
        assert_eq!(
            idx.content_revision.load(Ordering::Relaxed),
            1,
            "整个用例里只手工 bump 过一次（模拟周期重扫检出变化）"
        );
    }

    #[test]
    fn miss_rebuild_tracker_spends_a_bounded_budget_per_identity() {
        let mut tracker = MissRebuildTracker::default();
        assert!(!tracker.is_unresolvable(MISSING_MD5));
        for _ in 0..MISS_REBUILD_ATTEMPTS {
            assert!(tracker.spend(MISSING_MD5), "预算内的尝试应当被批准");
        }
        assert!(!tracker.spend(MISSING_MD5), "预算用完后一律拒绝");
        assert!(tracker.is_unresolvable(MISSING_MD5));
        assert_eq!(tracker.attempts(MISSING_MD5), MISS_REBUILD_ATTEMPTS);
        // 别的 md5 不受影响
        assert!(tracker.spend("00ff"));
        tracker.reset();
        assert!(!tracker.is_unresolvable(MISSING_MD5));
        assert_eq!(tracker.attempts(MISSING_MD5), 0);
        assert!(tracker.spend(MISSING_MD5));
    }

    // ---- 线上 reason ----

    /// **刚 miss**（重建预算还没用完）的身份键：当拍就给出**临时**原因 `chart-unresolved`
    /// （重试仍在进行），而不是等 ~10s 的重试窗口走完才第一次给提示。
    #[test]
    fn a_fresh_miss_reports_the_provisional_chart_unresolved() {
        let (idx, mut runtime) = runtime_for_tests();
        let start = Instant::now();
        idx.rebuild_requested.store(false, Ordering::Relaxed);
        runtime.request_miss_rebuild(MISSING_MD5, start);
        let provisional = runtime.miss_tracker.unavailable_reason(MISSING_MD5);
        assert_eq!(provisional, UnavailableReason::ChartUnresolved);
        assert_eq!(provisional.as_str(), "chart-unresolved");
        assert!(
            REASON_CLOSED_SET.contains(&provisional.as_str().as_str()),
            "临时原因也必须在契约 §8 的闭集里"
        );
        // 第一拍（预算还没花）与结论态是两个不同的字面量：页面据此分"正在解析"与"最终结论"
        assert_ne!(
            provisional.as_str(),
            UnavailableReason::ChartUnknownIdentity.as_str()
        );
        assert_ne!(
            provisional.as_str(),
            UnavailableReason::ChartNotIndexed.as_str()
        );
        // blocker 当拍就设 ⇒ state 帧本 tick 立刻带上它（不必等状态机的 hidden 心跳）
        assert_eq!(
            wire_reason(Some(&provisional), &Action::None, ""),
            "chart-unresolved"
        );
    }

    /// **两档提示的完整转变**：第一拍 miss ⇒ 临时原因；重试窗口内 ⇒ 仍是临时原因；预算耗尽 ⇒
    /// `chart-unknown-identity`；命中（库里其实有这张谱）⇒ 线上原因清空。
    ///
    /// 这也是"~200ms 有话说、~10s 给结论"的机器可验证形态：临时态在**第一次** miss 就成立。
    #[test]
    fn the_miss_notice_is_two_tier_provisional_then_final_and_clears_on_a_hit() {
        let (idx, mut runtime) = runtime_for_tests();
        let start = Instant::now();

        // 第一拍：miss 刚发生（重建请求已置位）→ 临时原因
        idx.rebuild_requested.store(false, Ordering::Relaxed);
        runtime.request_miss_rebuild(MISSING_MD5, start);
        assert!(idx.rebuild_requested.load(Ordering::Relaxed), "第一拍就要请求重建");
        assert_eq!(
            runtime.miss_tracker.unavailable_reason(MISSING_MD5),
            UnavailableReason::ChartUnresolved,
            "第一拍就要给临时原因（用户等不起 ~10s）"
        );

        // 重试窗口内的后续 tick（全局节流挡住、连重建请求都不再置位）→ 临时原因稳定不变
        for step in 1..=(INDEX_REBUILD_THROTTLE.as_millis() / POLL_INTERVAL.as_millis()) as u32 {
            let tick = start + POLL_INTERVAL * step;
            idx.rebuild_requested.store(false, Ordering::Relaxed);
            runtime.request_miss_rebuild(MISSING_MD5, tick);
            assert_eq!(
                runtime.miss_tracker.unavailable_reason(MISSING_MD5),
                UnavailableReason::ChartUnresolved,
                "重试窗口内原因不得跳变（也不得提前下结论）"
            );
        }

        // 走完预算（每次尝试之间隔开全局节流窗口）→ 结论已定
        let mut now = start;
        for _ in 0..MISS_REBUILD_ATTEMPTS {
            idx.rebuild_requested.store(false, Ordering::Relaxed);
            runtime.request_miss_rebuild(MISSING_MD5, now);
            now += INDEX_REBUILD_THROTTLE;
        }
        idx.rebuild_requested.store(false, Ordering::Relaxed);
        runtime.request_miss_rebuild(MISSING_MD5, now);
        assert_eq!(
            runtime.miss_tracker.unavailable_reason(MISSING_MD5),
            UnavailableReason::ChartUnknownIdentity,
            "预算耗尽 ⇒ 最终原因"
        );

        // 命中：库里其实有这张谱 → 线上原因清空（state 帧由 `state_push_due` 当拍补发）
        let hit = emit("C:/lib/now-in-library.mc", 1.0, "selection", "anchor-changed");
        assert!(matches!(hit, Action::Emit(_)), "命中走 Emit 路径（卡片换成这张谱）");
        let final_reason = UnavailableReason::ChartUnknownIdentity.as_str();
        assert_eq!(
            wire_reason(None, &hit, final_reason.as_str()),
            "",
            "命中必须清空 reason"
        );
        assert!(
            state_push_due(
                &Malody4Status {
                    reason: final_reason,
                    ..settled_state()
                },
                &Malody4Status {
                    reason: String::new(),
                    ..settled_state()
                }
            ),
            "清空也要当拍推给页面（提示必须收掉）"
        );
    }

    /// 预算耗尽的身份键：对外原因从临时态换成 `chart-unknown-identity`，且**本 tick 就设 blocker**
    /// （state 帧立刻可见，不必等 2s 的 hidden 心跳），此后每 tick 稳定同一个原因。
    #[test]
    fn an_exhausted_identity_reports_chart_unknown_identity() {
        let (idx, mut runtime) = runtime_for_tests();
        let mut now = Instant::now();
        // 用光预算（每次尝试之间隔开全局节流窗口）
        for _ in 0..MISS_REBUILD_ATTEMPTS {
            idx.rebuild_requested.store(false, Ordering::Relaxed);
            runtime.request_miss_rebuild(MISSING_MD5, now);
            now += INDEX_REBUILD_THROTTLE;
        }
        assert_eq!(
            runtime.miss_tracker.unavailable_reason(MISSING_MD5),
            UnavailableReason::ChartUnresolved,
            "预算用光之前一直是临时态（重试仍在进行）"
        );

        // 预算耗尽的那一次 miss：判定"本会话不可解析"
        idx.rebuild_requested.store(false, Ordering::Relaxed);
        runtime.request_miss_rebuild(MISSING_MD5, now);
        let reason = runtime.miss_tracker.unavailable_reason(MISSING_MD5);
        assert_eq!(reason, UnavailableReason::ChartUnknownIdentity);
        assert_eq!(reason.as_str(), "chart-unknown-identity");
        assert_ne!(reason.as_str(), UnavailableReason::ChartNotIndexed.as_str());
        assert_ne!(reason.as_str(), UnavailableReason::ChartUnresolved.as_str());
        assert!(
            REASON_CLOSED_SET.contains(&reason.as_str().as_str()),
            "新原因必须在契约 §8 的闭集里"
        );
        // blocker 优先于状态机给出的原因：state 帧本 tick 就是这个字面量
        assert_eq!(
            wire_reason(Some(&reason), &Action::None, "chart-unresolved"),
            "chart-unknown-identity"
        );

        // 之后的每一 tick 都在 `is_unresolvable` 处短路：不再请求重建 ⇒ 那行 info 也不会再记
        for step in 1..=20u32 {
            runtime.request_miss_rebuild(MISSING_MD5, now + INDEX_REBUILD_THROTTLE * step);
            assert!(
                !idx.rebuild_requested.load(Ordering::Relaxed),
                "判定之后绝不重启重建"
            );
            assert_eq!(
                runtime.miss_tracker.unavailable_reason(MISSING_MD5),
                UnavailableReason::ChartUnknownIdentity,
                "原因稳定，不是只报一次"
            );
        }
    }

    /// "本会话不可解析"那一行 **info** 日志：点名 md5、说明查不出来的事实与**用户可见的后果**
    /// （卡片保留上一张谱面），且是单行。去重由预算保证（判定那一次之后走 `is_unresolvable`
    /// 短路），不需要另加节流。
    #[test]
    fn the_unresolvable_notice_names_the_md5_and_the_visible_consequence() {
        let line = unresolvable_chart_log(MISSING_MD5, 7, 6);
        assert!(line.starts_with("malody4 "), "日志前缀统一：{line}");
        assert!(line.contains(MISSING_MD5), "必须点名 md5：{line}");
        assert!(
            line.contains("cannot be identified from the local library"),
            "必须说清查不出来的事实：{line}"
        );
        assert!(
            line.contains("the previous chart stays on screen"),
            "必须说清用户可见的后果：{line}"
        );
        assert!(line.contains("generation=7"), "带索引代数便于定位：{line}");
        assert!(line.contains("indexed=6"), "带条目数便于区分空库：{line}");
        assert!(!line.contains('\n'), "一条日志就是一行：{line}");
    }

    #[test]
    fn wire_reason_covers_every_closed_set_branch() {
        let healthy = emit("A", 1.0, "selection", "heartbeat");
        let no_selection = Action::Hidden(UnavailableReason::NoSelection);
        let not_indexed = Action::Hidden(UnavailableReason::ChartNotIndexed);

        // 未收录：blocker 为空时由状态机给出（否则该成因在线上不可见）。
        // 注意：轮询器现在对 miss **当拍就设 blocker**（临时态 `chart-unresolved` / 结论态
        // `chart-unknown-identity`），所以线上不会再走到这一支——这里钉的是纯函数的优先级口径
        // （`chart-not-indexed` 仍是闭集里的合法取值，仍是状态机的内部成因）。
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
