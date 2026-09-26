// 17653 Malody V BepInEx 选曲桥：POST /selection 门禁 → 载荷归一化 → 路径沙箱 →
// 内容去重/心跳/会话重置（唯一去重点）→ song/state 帧。
//
// 上游协议事实（权威）：插件固定 POST `http://127.0.0.1:17653/selection`，
// `UseProxy=false`、禁止重定向、超时 1.5s、**任何 2xx 都算成功**（Plugin.cs:701）。
// 载荷 11 字段（path / speed_rate / screen / sequence / event / version / chart_hash /
// source / judge_level / pro_judge / turbo），`screen ∈ {selection, playing, result, other}`。
// 心跳每 2s **逐字节重发**同一份快照——但 `sequence` 并非"仅真实变化时递增"（JUDGE/Mod/滚速
// 面板与重复重绘都会推高它）⇒ 事件判据必须是**内容六元组**的变化，`sequence` 只用于识别心跳
// 与重启。
//
// 因此本模块的分工钉死：
//   * `sequence`：只判"心跳（同 seq 同内容）"与"游戏重启（seq 回退）"；
//   * 内容六元组 `(path, rate_text, screen, judge_text, pro_text, turbo_text)`：判"真实事件 /
//     重复观察"。`pro` 与 `turbo` **必须在元组里**：选曲界面切换 Turbo 时倍率不变，少了它们
//     这类帧会被判成"重复观察"而不广播 song 帧，页面 `winScale` 停在旧值 ⇒ 卡片静默显示旧星数。
// 这条区分是"心跳不得续期页面窗口"能否成立的关键。
//
// 判定/Pro/Turbo 来源（契约 v5，2026-09-24 起）：**插件载荷是权威**（实时通道）。
// `{malodyRoot}/config.json` 只在插件未上报 `judge_level` 时为 fallback（该文件只在"启动游戏 /
// 开始游玩"写盘，选曲期读到的是上一局的值——滞后但至少有值），并在两者都有值时作**交叉校验**
// （不一致 ⇒ 用插件值 + 一条 info，按"结论变化"去重，绝不逐帧刷屏）。只取
// `user_judge_level` / `user_mods` 两键，只读不写、不改 mtime。
// 旧插件（8 字段载荷）继续可用：三个新字段缺省 ⇒ 一律发布 `null`，绝不猜。
// `winScale` 的唯一实现点是 `win_scale_for(turbo, speed_rate)`：只有**确认非 Turbo**
// （`turbo == false`）且倍率命中名义值（1.2 / 1.5 / 0.8，±0.005）时才给 `1/名义值`，其余
// （Turbo / 未知 / 自定义倍率）一律 `null`——`turbo == null` 与"确认非 Turbo"是两件事，
// 混为一谈会让 Dash/Rush/Slow 被当成 Turbo。

use crate::config;
use crate::frames::{
    Envelope, MalodyBridgeSource, BRIDGE_MAX_BODY_BYTES, BRIDGE_PORT, BRIDGE_STALE_AFTER,
    CHART_MAX_BYTES, MAX_PAYLOAD_BYTES,
};
use crate::server::log::log_at;
use crate::server::{broadcast, http, json_error, malody_root, md5_hex, next_seq, Shared};
use serde::Deserialize;
use std::fs;
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------- 桥状态 --

/// 内容六元组 `(path, rate_text, screen, judge_text, pro_text, turbo_text)`。
pub type ContentKey = (String, String, String, String, String, String);

/// 日志闸（纯状态）：同一"结论"只放行一次——`malody4/mod.rs` 的 `ReasonLog::due` 同形。
/// 设置类日志的语义是"值变化时才打"：心跳每 2s 一次，逐帧打会把日志淹掉。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LogGate {
    last: Option<String>,
}

impl LogGate {
    /// 与上次打过的那条不同 ⇒ `true`（本条该打）；相同 ⇒ `false`。
    pub fn due(&mut self, line: &str) -> bool {
        if self.last.as_deref() == Some(line) {
            return false;
        }
        self.last = Some(line.to_string());
        true
    }
}

/// 设置类日志的类别（**每类一个闸门**：同类同值不再打）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogKind {
    /// 解析后的设置行（`source/judge/pro/turbo/speed_rate/winScale` 任一变化）。
    Settings,
    /// 交叉校验不一致。
    CrossCheck,
    /// judge 两处都取不到。
    Unavailable,
    /// `winScale = null` 的原因。
    WinScale,
}

/// 四条设置类日志的闸门组（顺序即出日志的顺序）。
#[derive(Debug, Clone, Default)]
pub struct SettingsLogGates {
    pub settings: LogGate,
    pub cross_check: LogGate,
    pub unavailable: LogGate,
    pub win_scale: LogGate,
}

impl SettingsLogGates {
    fn gate_mut(&mut self, kind: LogKind) -> &mut LogGate {
        match kind {
            LogKind::Settings => &mut self.settings,
            LogKind::CrossCheck => &mut self.cross_check,
            LogKind::Unavailable => &mut self.unavailable,
            LogKind::WinScale => &mut self.win_scale,
        }
    }
}

/// Malody 选曲桥状态（`Shared::malody_bridge` 的内容）。
///
/// 去重基线（`last_event` / `last_seq` / `event_seq`）与"最近一次真实事件"的展示字段
/// 同处一地：**这是全壳唯一的去重点**——HTTP 处理线程与 WS 重连补发都从这里读，
/// 不得各自维护副本（否则补发会把基线搅乱）。
#[derive(Debug, Clone)]
pub struct MalodyBridgeState {
    /// 最近一次收到任意 POST（含心跳）的时间——8s 存活窗口的唯一输入。
    /// 三种"不发帧"分支同样刷新它：心跳 = 桥还活着。
    pub last_seen: Option<Instant>,
    /// 最近一次真实事件的场景（`selection|playing|result|other`）。
    pub screen: String,
    /// 最近一次真实事件的谱面绝对路径（`screen=other` 时为空串）。
    pub chart_path: String,
    /// 最近一次真实事件的倍率文本（`{:.5}`；补发 song 帧时还原 `modData.speedRate`）。
    pub rate_text: String,
    /// 内容六元组 `(path, rate_text, screen, judge_text, pro_text, turbo_text)`。
    pub last_event: Option<ContentKey>,
    /// 桥最近上报的 `sequence`（仅用于识别心跳与游戏重启）。
    pub last_seq: Option<i64>,
    /// 真实事件自增计数器（`state.sources.malody.eventSeq`）。
    pub event_seq: u64,
    /// 当前**发布给页面**的判定档（`0..=4` ↔ A~E；两处都取不到为 `None`）。
    /// 插件值优先，`config.json` 仅在插件未上报时作 fallback。进内容六元组第 4 位：
    /// 判定变化 = 真实事件 = 重发 song 帧（与 v4 的"当局冻结"语义无关，不再冻结）。
    pub judge: Option<u8>,
    /// 当前发布的 Pro（严格组）开关；插件未上报为 `None`（页面据此关闭动态 OD）。
    /// **进内容六元组第 5 位**：只改 Pro 也是真实事件。
    pub pro: Option<bool>,
    /// 当前发布的 Turbo 开关；插件未上报为 `None`。**进内容六元组第 6 位**：
    /// 选曲界面切换 Turbo（倍率不变）必须是真实事件，否则页面 `winScale` 停在旧值。
    pub turbo: Option<bool>,
    /// 窗口缩放因子：`Some(1/名义倍率)`（确认非 Turbo 且倍率命中名义值）或 `None`（未知）。
    /// **不是 `1.0` 兜底**——`None` 会原样进 song 帧的 `winScale: null`。
    pub win_scale: Option<f64>,
    /// 四条设置类日志的闸门（每类按"值变化时才打"）。
    pub log_gates: SettingsLogGates,
}

impl Default for MalodyBridgeState {
    fn default() -> Self {
        MalodyBridgeState {
            last_seen: None,
            screen: String::new(),
            chart_path: String::new(),
            rate_text: String::new(),
            last_event: None,
            last_seq: None,
            event_seq: 0,
            judge: None,
            pro: None,
            turbo: None,
            win_scale: None,
            log_gates: SettingsLogGates::default(),
        }
    }
}

/// 桥存活判定（纯函数）：最近一次 POST（含心跳）在 `BRIDGE_STALE_AFTER` 内。
///
/// 上游约定"8s 未收到心跳应清除旧结果"。**绝不把"进入游玩/结算"当作断线**——
/// 那两态的 `screen` 是稳定的，靠这条门控而不是靠猜。
pub fn bridge_alive(last_seen: Option<Instant>) -> bool {
    matches!(last_seen, Some(at) if at.elapsed() < BRIDGE_STALE_AFTER)
}

/// 去重判定结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dedupe {
    /// 心跳：同 `sequence` 同内容（上游逐字节重发快照）——只刷新 `last_seen`。
    Heartbeat,
    /// 重复观察：同内容但 `sequence` 不同（JUDGE/Mod/滚速面板开关、重复重绘）。
    Duplicate,
    /// 真实事件：内容六元组变化。
    Event,
}

/// 去重判定的产物：分支 + 需要写壳日志的那一条。
///
/// `log` 为 `None` 表示不打日志（**除重启外不再对序号异常打日志**：早期写法
/// "序号不在 recent 集合就 warn"会在每次真实选曲时误报，反而淹没真问题）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DedupeOutcome {
    pub kind: Dedupe,
    pub log: Option<(&'static str, String)>,
}

/// 判定档的文本形态（内容六元组第 4 位）：`none` = 尚未采集到。
pub fn judge_text(judge: Option<u8>) -> String {
    judge
        .map(|value| value.to_string())
        .unwrap_or_else(|| "none".to_string())
}

/// 三态布尔的文本形态（内容六元组第 5/6 位与日志共用）：`true` / `false` / `none`（未知）。
///
/// ⚠️ `none`（未知）**不是** `false`：把两者都当"确认非 Turbo"会让 Dash/Rush/Slow 被算成 Turbo。
pub fn flag_text(value: Option<bool>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "none".to_string())
}

/// 日志里的 `winScale` 文本：`null`（未知）或数值。
pub fn win_scale_text(win_scale: Option<f64>) -> String {
    match win_scale {
        Some(value) => format!("{value:?}"),
        None => "null".to_string(),
    }
}

/// 游戏重启的 info 日志文本（纯函数，单测断言其确切文本与级别）。
pub fn restart_log(previous_seq: i64, next_seq: i64) -> String {
    format!(
        "malody bridge: sequence went backwards ({} -> {}) — game restarted, dedupe baseline cleared",
        previous_seq, next_seq
    )
}

/// 内容去重 + 心跳识别 + 会话重置（**唯一去重点**；纯逻辑，单测直接驱动）。
///
/// 四条分支：
///   * `sequence == last_seq` **且**内容相同 ⇒ `Heartbeat`（不发帧、不算事件、基线不动）；
///   * `sequence < last_seq` ⇒ 游戏重启：记一条 info、清空 `last_event`，然后**按正常规则
///     处理本帧**（清空后内容必然 ≠ `None` ⇒ 本帧必为 `Event`，满足"重启后同一张谱面仍能
///     重新跟上"）；
///   * 内容相同但 `sequence` 不同 ⇒ `Duplicate`（不发帧、`event_seq` 不动，但 `last_seq`
///     前移，保证后续序号比较的基线持续前移）；
///   * 内容不同 ⇒ `Event`。
///
/// ⚠️ **不得只按 `sequence == last_seq` 判心跳**：若重启前后首帧序号相同而内容不同
/// （例如两局都从同一序号起步），只按序号会把它误吞且永不恢复；加上内容判据后该帧
/// 必然走 `Event`。
pub fn classify(
    state: &mut MalodyBridgeState,
    content: ContentKey,
    sequence: i64,
    now: Instant,
) -> DedupeOutcome {
    // 任意 POST（含心跳与重复观察）都是"桥还活着"的证据。
    state.last_seen = Some(now);

    let mut log = None;
    match state.last_seq {
        Some(previous) if sequence < previous => {
            log = Some(("info", restart_log(previous, sequence)));
            state.last_event = None;
        }
        _ => {}
    }

    let same_content = state.last_event.as_ref() == Some(&content);
    if same_content {
        if state.last_seq == Some(sequence) {
            return DedupeOutcome {
                kind: Dedupe::Heartbeat,
                log,
            };
        }
        state.last_seq = Some(sequence);
        return DedupeOutcome {
            kind: Dedupe::Duplicate,
            log,
        };
    }

    state.last_event = Some(content);
    state.last_seq = Some(sequence);
    state.event_seq += 1;
    DedupeOutcome {
        kind: Dedupe::Event,
        log,
    }
}

/// `state.sources.malody` 的组装（纯函数）。
///
/// **门控是必须的**：游戏在 `playing` 中途被关掉时桥不再发心跳，若不按 `last_seen`
/// 门控，壳会永久上报 `playing: true`，页面 L1 会把路由钉死在 Malody（明确不可接受的
/// 失效类）。`screen` / `playing` 因此只在桥存活时有效。
pub fn malody_source(state: &MalodyBridgeState, lua_alive: bool) -> MalodyBridgeSource {
    let alive = bridge_alive(state.last_seen);
    MalodyBridgeSource {
        alive: alive || lua_alive,
        transport: if alive {
            "bridge"
        } else if lua_alive {
            "lua"
        } else {
            "none"
        }
        .to_string(),
        screen: if alive {
            state.screen.clone()
        } else {
            "none".to_string()
        },
        playing: alive && state.screen == "playing",
        event_seq: state.event_seq,
        judge: state.judge,
        pro: state.pro,
        turbo: state.turbo,
    }
}

// ---------------------------------------------------------------- 载荷 --

/// `judge_level` 的容错解析（§7.7）：**任何形态都不拒收**——缺失 / `null` / 非整数 /
/// 越界（`>4`）一律 `None`（= 未采集到），绝不猜、绝不夹断。
fn opt_judge_level<'de, D>(deserializer: D) -> Result<Option<u8>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(raw
        .and_then(|value| value.as_u64())
        .and_then(|value| u8::try_from(value).ok())
        .filter(|level| *level <= 4))
}

/// `pro_judge` / `turbo` 的容错解析：非布尔（`1` / `"true"` / 对象）一律 `None`。
fn opt_bool<'de, D>(deserializer: D) -> Result<Option<bool>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(raw.and_then(|value| value.as_bool()))
}

/// `POST /selection` 的载荷（全部 `#[serde(default)]` 容错：缺字段不拒收，由归一化兜底）。
/// **向后兼容**：旧插件只发前 8 个字段，因此新增的 `judge_level` / `pro_judge` / `turbo`
/// 缺省即 `None`（= 未知），绝不因缺字段报 400。
#[derive(Deserialize, Debug, Clone, Default)]
pub struct BridgeSelection {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub speed_rate: f64,
    #[serde(default)]
    pub screen: String,
    #[serde(default)]
    pub sequence: i64,
    #[serde(default)]
    pub event: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub chart_hash: String,
    #[serde(default)]
    pub source: String,
    /// 判定档（`0..=4` ↔ A~E；v5 新增，插件权威）。
    #[serde(default, deserialize_with = "opt_judge_level")]
    pub judge_level: Option<u8>,
    /// Pro（严格组）开关（v5 新增，插件权威）。
    #[serde(default, deserialize_with = "opt_bool")]
    pub pro_judge: Option<bool>,
    /// Turbo 开关（v5 新增，插件权威）。**不得用 UI 开关判**：插件报的是已提交记录。
    #[serde(default, deserialize_with = "opt_bool")]
    pub turbo: Option<bool>,
}

/// 归一化后的请求（门禁之后的唯一分支依据）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Normalized {
    pub screen: String,
    pub path: String,
    pub rate_text: String,
}

/// `speed_rate` 有效性：非有限值或越界（`< 0.05` 或 `> 10`）即无效。
pub fn rate_is_valid(rate: f64) -> bool {
    rate.is_finite() && (0.05..=10.0).contains(&rate)
}

/// `screen` 归一化：闭集外的一切（含空串、未知值）回落 `other`。
pub fn normalize_screen(raw: &str) -> &'static str {
    match raw.trim().to_ascii_lowercase().as_str() {
        "selection" => "selection",
        "playing" => "playing",
        "result" => "result",
        _ => "other",
    }
}

/// 载荷归一化（纯函数）：`speed_rate` 无效 ⇒ **整帧按 `other` 处理**（无谱面、倍率 1.0），
/// `screen` 未知值 ⇒ `other`。`other` 的 `path` 恒为空串、`rate_text` 恒为 `1.00000`
/// ——与上游"`screen=other` 时空 path、rate=1.0"的语义一致。
pub fn normalize(payload: &BridgeSelection) -> Normalized {
    let rate_ok = rate_is_valid(payload.speed_rate);
    let screen = if rate_ok {
        normalize_screen(&payload.screen)
    } else {
        "other"
    };
    if screen == "other" {
        Normalized {
            screen: "other".to_string(),
            path: String::new(),
            rate_text: "1.00000".to_string(),
        }
    } else {
        Normalized {
            screen: screen.to_string(),
            path: config::normalize_path(&payload.path),
            rate_text: format!("{:.5}", payload.speed_rate),
        }
    }
}

/// 内容六元组（`last_event` 的唯一形态）。
///
/// 第 5/6 位（`pro_text` / `turbo_text`）不可省：选曲界面切换 Turbo 时倍率不变，若它们不在
/// 元组里，这类帧会被判成"重复观察"而不广播 song 帧 ⇒ 页面 `winScale` 停在旧值。
pub fn content_key(
    normalized: &Normalized,
    judge: Option<u8>,
    pro: Option<bool>,
    turbo: Option<bool>,
) -> ContentKey {
    (
        normalized.path.clone(),
        normalized.rate_text.clone(),
        normalized.screen.clone(),
        judge_text(judge),
        flag_text(pro),
        flag_text(turbo),
    )
}

// ----------------------------------- 判定/设置（插件权威 + config.json 兜底）--

/// 名义倍率（Dash / Rush / Slow，与 4.3.7 的 `ModFlags::speed_rate` 同序）。
/// `winScale` 的取值就是命中项的名义倍率的倒数（**用名义值，不用 `speed_rate`**）。
const NOMINAL_RATES: [f64; 3] = [1.2, 1.5, 0.8];

/// 名义倍率与桥 `speed_rate` 的一致性容差（±0.005）。
const RATE_TOLERANCE: f64 = 0.005;

/// 写盘竞态的一次重试：总窗口（≤1s）与轮询间隔。**只服务"插件没给 `judge_level`"的兜底读**
/// （插件给了值就不需要重试窗口——交叉校验用单次廉价读）。
const SNAPSHOT_RETRY_WINDOW: Duration = Duration::from_secs(1);
const SNAPSHOT_RETRY_POLL: Duration = Duration::from_millis(100);

/// `config.json` 里本源需要的两个键（**只有这两个**：该文件含会话 token 与用户名，
/// 其余键一律不进结构体、不进日志）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SettingsSnapshot {
    /// `user_judge_level`：整数 `0..=4`（↔ A~E）；缺失 / 非整数 / 越界 = 未取到。
    pub judge: Option<u8>,
    /// `user_mods`：位掩码；缺失 / 非整数 / 超出 u32 = 未取到。
    /// **不再参与任何判据**（`winScale` 的输入已改为插件上报的 `turbo`）——保留解析只为
    /// 该文件的解析契约与既有单测不变（`config.json` 的键集仍被钉死在两个）。
    pub mod_mask: Option<u32>,
}

/// 解析 `config.json` 文本（**只读两个键**）。返回 `None` 只表示"不是 JSON"（调用方据此
/// 判定读失败）；键缺失 / 非整数 / 越界 ⇒ 对应字段为 `None`（**fail-closed，绝不猜、绝不夹断**）。
pub fn parse_settings(text: &str) -> Option<SettingsSnapshot> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let judge = value
        .get("user_judge_level")
        .and_then(|v| v.as_u64())
        .and_then(|raw| u8::try_from(raw).ok())
        .filter(|level| *level <= 4);
    let mod_mask = value
        .get("user_mods")
        .and_then(|v| v.as_u64())
        .and_then(|raw| u32::try_from(raw).ok());
    Some(SettingsSnapshot { judge, mod_mask })
}

/// 读取并解析 `config.json`（**只读**：不写、不改 mtime）。IO / 非 JSON ⇒ `None`。
pub fn read_settings(path: &Path) -> Option<SettingsSnapshot> {
    parse_settings(&fs::read_to_string(path).ok()?)
}

/// 带**一次 ≤1s 重试**的读取：**仅在插件未上报 `judge_level` 时**作为兜底调用，覆盖
/// "开始游玩"的写盘与首次读竞争。首次读到可用判定即返回；否则每 100ms 重读一次
/// （内容/mtime 变化都能覆盖），窗口用尽后返回最后一次读到的结果（可能是"读到了但判定键
/// 不可用"），调用方据此记**一条**日志（日志闸保证不逐帧刷屏）。
pub fn read_settings_with_retry(path: &Path) -> Option<SettingsSnapshot> {
    let deadline = Instant::now() + SNAPSHOT_RETRY_WINDOW;
    loop {
        let last = read_settings(path);
        if last.map(|snapshot| snapshot.judge.is_some()).unwrap_or(false) {
            return last;
        }
        if Instant::now() >= deadline {
            return last;
        }
        thread::sleep(SNAPSHOT_RETRY_POLL);
    }
}

/// 判定档字母（`0..=4` ↔ A~E；越界 = 无字母，与 4.3.7 的 `JudgeLevel::letter` 同表）。
pub fn judge_letter(judge: u8) -> Option<char> {
    match judge {
        0 => Some('A'),
        1 => Some('B'),
        2 => Some('C'),
        3 => Some('D'),
        4 => Some('E'),
        _ => None,
    }
}

/// 窗口缩放因子（**唯一实现点**）：输入是**插件上报的 `turbo`** 与**桥上报的 `speed_rate`**
/// （`config.json` 的 `user_mods` 已彻底退出判据）。规则内联写死：
///
/// | `turbo` | `speed_rate` | 返回 |
/// |---|---|---|
/// | `Some(false)`（确认非 Turbo） | 1.2 / 1.5 / 0.8（±`RATE_TOLERANCE`） | `Some(1/名义倍率)` |
/// | `Some(false)` | 其它 | `None` |
/// | `Some(true)` | 任意 | `None` |
/// | `None`（未知） | 任意 | `None` |
///
/// ⚠️ `None` 是"**判不出来**"，**不是 `1.0`**：把 `turbo == None`（未知）与"确认非 Turbo"
/// 混为一谈，会让 Dash/Rush/Slow（倍率恰好命中名义值）的 `analysisRate` 从 1.0 变成 1.2——
/// 正是本规则要防的"把 Dash/Rush/Slow 当 Turbo"。`speed_rate` 只用于判别，绝不参与取值。
pub fn win_scale_for(turbo: Option<bool>, speed_rate: f64) -> Option<f64> {
    if turbo != Some(false) {
        return None;
    }
    NOMINAL_RATES
        .iter()
        .find(|nominal| (**nominal - speed_rate).abs() <= RATE_TOLERANCE)
        .map(|nominal| 1.0 / *nominal)
}

/// 判定档来源（进 `settings_log` 的 `source=`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JudgeSource {
    /// 插件实时上报（权威）。
    Plugin,
    /// 插件未上报 ⇒ 兜底用 `config.json` 的 `user_judge_level`（选曲期滞后但至少有值）。
    ConfigJson,
    /// 两处都没有 ⇒ 未知（页面回落默认 OD，绝不猜）。
    Unknown,
}

impl JudgeSource {
    pub fn as_str(self) -> &'static str {
        match self {
            JudgeSource::Plugin => "plugin",
            JudgeSource::ConfigJson => "configjson",
            JudgeSource::Unknown => "none",
        }
    }
}

/// 交叉校验不一致的 info 行（**一条**，由 `LogKind::CrossCheck` 的闸门去重）。
pub fn judge_cross_check_log(plugin: u8, file: u8) -> String {
    format!(
        "malodyv settings: judge cross-check mismatch — config.json user_judge_level={} ({}) vs plugin judge_level={} ({}); using the plugin value (authoritative)",
        file,
        judge_letter(file).unwrap_or('?'),
        plugin,
        judge_letter(plugin).unwrap_or('?')
    )
}

/// judge 两处都取不到时的**唯一一条**告警（`LogKind::Unavailable` 的闸门去重）。
/// `readable` = 文件读到了但 `user_judge_level` 缺失 / 非整数 / 越界（`false` = 文件读不出或
/// 不是 JSON）；`path` 为 `None` = `malodyRoot` 未配置。
pub fn judge_unavailable_log(path: Option<&Path>, readable: bool) -> String {
    match path {
        None => "malodyv settings: judge unavailable — the plugin reported none and malodyRoot is not configured; the page keeps its default OD".to_string(),
        Some(path) if readable => format!(
            "malodyv settings: judge unavailable — the plugin reported none and user_judge_level is missing or not an integer in 0..=4 in {}; the page keeps its default OD",
            path.display()
        ),
        Some(path) => format!(
            "malodyv settings: judge unavailable — the plugin reported none and {} is unreadable or not JSON; the page keeps its default OD",
            path.display()
        ),
    }
}

/// 判定档解析（**纯函数**）：**插件值优先**；插件缺失时用 `config.json` 的值（兜底）；
/// 两者都有且不一致 ⇒ 用插件值 + 一条交叉校验日志（是否真打由调用方的日志闸决定）。
pub fn resolve_judge(
    plugin: Option<u8>,
    file: Option<u8>,
) -> (Option<u8>, JudgeSource, Option<String>) {
    match (plugin, file) {
        (Some(plugin), Some(file)) if plugin != file => (
            Some(plugin),
            JudgeSource::Plugin,
            Some(judge_cross_check_log(plugin, file)),
        ),
        (Some(plugin), _) => (Some(plugin), JudgeSource::Plugin, None),
        (None, Some(file)) => (Some(file), JudgeSource::ConfigJson, None),
        (None, None) => (None, JudgeSource::Unknown, None),
    }
}

/// 解析后的设置行（info；**值变化时才打**，由 `LogKind::Settings` 的闸门去重）：
/// `malodyv settings: source=plugin judge=C pro=true turbo=false speed_rate=1.20 win_scale=0.8333333333333334`
pub fn settings_log(
    source: JudgeSource,
    judge: Option<u8>,
    pro: Option<bool>,
    turbo: Option<bool>,
    speed_rate: f64,
    win_scale: Option<f64>,
) -> String {
    format!(
        "malodyv settings: source={} judge={} pro={} turbo={} speed_rate={:.2} win_scale={}",
        source.as_str(),
        judge
            .map(|level| judge_letter(level).unwrap_or('?').to_string())
            .unwrap_or_else(|| "none".to_string()),
        flag_text(pro),
        flag_text(turbo),
        speed_rate,
        win_scale_text(win_scale)
    )
}

/// `winScale` 判不出来（`null`）时的 info（**不静默**：倍率语义必须留痕；由
/// `LogKind::WinScale` 的闸门去重）。`null` 会原样进 song 帧，页面据此关闭动态 OD。
pub fn win_scale_fallback_log(turbo: Option<bool>, speed_rate: f64) -> String {
    let reason = match turbo {
        Some(true) => "turbo is on — Turbo keeps the default windows".to_string(),
        None => "turbo is unknown (the plugin did not report it)".to_string(),
        Some(false) => format!(
            "speed_rate {:.2} is not within ±{:.3} of a nominal Dash/Rush/Slow rate (1.2 / 1.5 / 0.8)",
            speed_rate, RATE_TOLERANCE
        ),
    };
    format!(
        "malodyv settings: win_scale=null — {}; the song frame reports winScale=null and the page keeps dynamic OD off",
        reason
    )
}

// ---------------------------------------------------------------- 沙箱 --

/// 路径沙箱的拒绝原因（**每类一个可区分的错误**：单测逐条断言）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxError {
    /// `malodyRoot` 未配置或无法解析。
    RootUnavailable,
    /// 目标无法解析（不存在 / 权限 / 坏路径）。
    Unresolvable,
    /// 解析后落在 `{malodyRoot}/chart/` 之外。
    Escape,
    /// 后缀不是 `.mc` / `.osu`。
    BadSuffix,
    /// 不是普通文件（目录等）。
    NotAFile,
    /// 0 字节。
    Empty,
    /// 超过 50 MB。
    TooLarge,
}

impl SandboxError {
    /// 403 响应体与壳日志共用的文案（`path outside` 类记录是验收判据）。
    pub fn message(self) -> &'static str {
        match self {
            SandboxError::RootUnavailable => "malodyRoot unavailable",
            SandboxError::Unresolvable => "chart path cannot be resolved",
            SandboxError::Escape => "path outside malodyRoot/chart",
            SandboxError::BadSuffix => "chart suffix must be .mc or .osu",
            SandboxError::NotAFile => "chart path is not a file",
            SandboxError::Empty => "chart file is empty",
            SandboxError::TooLarge => "chart file exceeds 50MB",
        }
    }
}

/// 路径沙箱（`screen ∈ {selection, playing, result}` 时必过；`other` 跳过）。
///
/// 顺序固定：`normalize_path` → `canonicalize`（失败即拒）→ 必须
/// `starts_with(canonicalize(malody_root)/chart/)` → 后缀 ∈ `{.mc, .osu}`（大小写不敏感）
/// → `is_file()` → `1 B <= len <= 50 MB`。
///
/// **不得放宽前缀校验**：`starts_with` 是"本地 HTTP 端点不得成为任意文件读取原语"的
/// 唯一保证（同 `post.rs::handle_resolve` 的判据，只是这里额外收紧到 `chart/`）。
pub fn sandbox_chart_path(root: &Path, raw_path: &str) -> Result<PathBuf, SandboxError> {
    let root_canon = root.canonicalize().map_err(|_| SandboxError::RootUnavailable)?;
    let chart_root = root_canon.join("chart");
    let normalized = config::normalize_path(raw_path);
    if normalized.is_empty() {
        return Err(SandboxError::Unresolvable);
    }
    let candidate = PathBuf::from(&normalized)
        .canonicalize()
        .map_err(|_| SandboxError::Unresolvable)?;
    if !candidate.starts_with(&chart_root) {
        return Err(SandboxError::Escape);
    }
    let suffix_ok = candidate
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| matches!(e.to_ascii_lowercase().as_str(), "mc" | "osu"))
        .unwrap_or(false);
    if !suffix_ok {
        return Err(SandboxError::BadSuffix);
    }
    let meta = fs::metadata(&candidate).map_err(|_| SandboxError::Unresolvable)?;
    if !meta.is_file() {
        return Err(SandboxError::NotAFile);
    }
    let len = meta.len();
    if len == 0 {
        return Err(SandboxError::Empty);
    }
    if len > CHART_MAX_BYTES {
        return Err(SandboxError::TooLarge);
    }
    Ok(candidate)
}

// ------------------------------------------------------------ 谱面字段 --

/// `song` 帧的元信息（`.mc` / `.osu` 的差异全部收敛在这里）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChartFields {
    pub title: String,
    pub artist: String,
    pub level: String,
    pub keys: u64,
}

/// `.osu` 的 `[Difficulty]` 段 `CircleSize` 扫描（壳侧简单扫描；页面侧 `analysis.js`
/// 在本通道不可用）。只在 `[Difficulty]` 段内认键，遇到下一个 `[Section]` 即停。
pub fn osu_circle_size(text: &str) -> u64 {
    let mut in_difficulty = false;
    for line in text.lines() {
        let line = line.trim().trim_start_matches('\u{feff}');
        if line.starts_with('[') && line.ends_with(']') {
            in_difficulty = line.eq_ignore_ascii_case("[Difficulty]");
            continue;
        }
        if !in_difficulty {
            continue;
        }
        let Some(rest) = line.strip_prefix("CircleSize") else {
            continue;
        };
        let Some(rest) = rest.trim_start().strip_prefix(':') else {
            continue;
        };
        return rest
            .trim()
            .parse::<f64>()
            .ok()
            .map(|v| v.round() as u64)
            .unwrap_or(0);
    }
    0
}

/// 谱面字段派生（**字段来源钉死**，见 CONTRACT.md §11 的来源表）：
///
/// | 字段 | 首选 | 回落 |
/// |---|---|---|
/// | `level` | payload 的 `version`（游戏里的 `DisplayVersion`） | `.mc` 的 `/meta/version` → 文件名 stem（**不从 `.osu` 里猜**：osu 无 level 概念） |
/// | `keys` | `.mc` 的 `/meta/mode_ext/column` | `.osu` 的 `[Difficulty]/CircleSize` → `0`（`0` 再由调用方回落 `4`，与 `post.rs` 同法） |
/// | `title` / `artist` | `.mc` 的 `/meta/song/{title,artist}` | 文件名 stem / `""` |
///
/// `chart_hash` 不使用（仅诊断）；identity 用壳自算的 `contentMd5`。
pub fn derive_fields(text: &str, chart_path: &Path, payload_version: &str) -> ChartFields {
    let stem = chart_path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let is_mc = chart_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("mc"))
        .unwrap_or(false);
    let mc = if is_mc {
        serde_json::from_str::<serde_json::Value>(text).ok()
    } else {
        None
    };
    let mc_str = |pointer: &str| -> Option<String> {
        mc.as_ref()
            .and_then(|v| v.pointer(pointer))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty())
    };

    let level = if !payload_version.is_empty() {
        payload_version.to_string()
    } else {
        mc_str("/meta/version").unwrap_or(stem.clone())
    };
    let keys = mc
        .as_ref()
        .and_then(|v| v.pointer("/meta/mode_ext/column"))
        .and_then(|v| v.as_u64())
        .filter(|k| *k > 0)
        .unwrap_or_else(|| {
            if is_mc {
                0
            } else {
                osu_circle_size(text)
            }
        });
    ChartFields {
        title: mc_str("/meta/song/title").unwrap_or(stem),
        artist: mc_str("/meta/song/artist").unwrap_or_default(),
        level,
        keys,
    }
}

/// identity：`mdy:{title}:{level}:{keys}:{contentMd5}u`。
///
/// `u` 后缀 = "上游桥通道"来源标记，与 Lua 通道的 `mdy:…:{contentMd5}` 区分。代价是同一
/// 谱面在两条通道间切换时会多算一次（LRU 里两条快照）——刻意的来源可追溯性取舍。
pub fn identity_of(fields: &ChartFields, content_md5: &str) -> String {
    format!(
        "mdy:{}:{}:{}:{}u",
        if fields.title.is_empty() {
            "untitled"
        } else {
            &fields.title
        },
        if fields.level.is_empty() {
            "Unknown"
        } else {
            &fields.level
        },
        if fields.keys > 0 { fields.keys } else { 4 },
        content_md5
    )
}

/// `song` 帧构造（真实事件与重连补发**共用此唯一构造点**）。
///
/// `judge` / `pro` / `turbo` / `winScale` 是 v5 的桥通道字段：**未知一律是 `null`**（不是省略、
/// 不是 `1.0`）——页面按 `null` 关闭动态 OD 并在状态行说明原因。
pub fn song_frame(
    request_id: &str,
    fields: &ChartFields,
    identity: &str,
    rate_text: &str,
    raw_text: String,
    screen: &str,
    judge: Option<u8>,
    pro: Option<bool>,
    turbo: Option<bool>,
    win_scale: Option<f64>,
) -> serde_json::Value {
    let keys = if fields.keys > 0 { fields.keys } else { 4 };
    serde_json::json!({
        "requestId": request_id,
        "source": "malody",
        "identity": identity,
        "modData": { "speedRate": rate_text },
        "meta": {
            "title": fields.title,
            "artist": fields.artist,
            "version": if fields.level.is_empty() { "Unknown" } else { &fields.level },
            "keys": keys,
            "devMsd8": [],
        },
        "cover": null,
        "rawText": raw_text,
        "screen": screen,
        "judge": judge,
        "pro": pro,
        "turbo": turbo,
        "winScale": win_scale,
    })
}

// ------------------------------------------------------------- HTTP 层 --

fn header_value<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    head.lines().skip(1).find_map(|line| {
        let (key, value) = line.split_once(':')?;
        if key.trim().eq_ignore_ascii_case(name) {
            Some(value.trim())
        } else {
            None
        }
    })
}

/// `Content-Type` 的 type 部分（`;` 之前）必须是 `application/json`；缺失 ⇒ 非 JSON。
fn is_json_content_type(head: &str) -> bool {
    header_value(head, "content-type")
        .map(|v| {
            v.split(';')
                .next()
                .unwrap_or("")
                .trim()
                .eq_ignore_ascii_case("application/json")
        })
        .unwrap_or(false)
}

fn respond(stream: &mut TcpStream, code: u16, body: &str) {
    http::respond_json(stream, code, body);
}

pub fn spawn_bridge(shared: Arc<Shared>, listener: TcpListener) {
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let shared = shared.clone();
            thread::spawn(move || handle_bridge(shared, stream));
        }
    });
}

/// 门禁顺序**固定**（与上游参考实现 `malody_insight/service.py:283-299` 的 HTTP 语义对齐）：
/// Host → path → Origin → method → Content-Type → body 长度 → JSON 解析。
/// 成功一律 **202**（插件只看 2xx）。
fn handle_bridge(shared: Arc<Shared>, mut stream: TcpStream) {
    let Some((head, body)) = http::read_request(&mut stream) else {
        return;
    };
    if !http::is_local_host(&head, BRIDGE_PORT) {
        respond(&mut stream, 403, &json_error("forbidden host"));
        return;
    }
    let request_line = head.lines().next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("GET");
    let url = parts.next().unwrap_or("/");
    let path = url.split('?').next().unwrap_or("");
    if path != "/selection" {
        respond(&mut stream, 404, &json_error("not found"));
        return;
    }
    // 浏览器页面不可能合法地调用本端点：**任何 Origin 头出现即拒**（非空/空都一样）。
    if header_value(&head, "origin").is_some() {
        respond(&mut stream, 403, &json_error("forbidden origin"));
        return;
    }
    if !method.eq_ignore_ascii_case("POST") {
        respond(&mut stream, 405, &json_error("method not allowed"));
        return;
    }
    if !is_json_content_type(&head) {
        respond(
            &mut stream,
            415,
            &json_error("unsupported media type: application/json required"),
        );
        return;
    }
    if body.is_empty() || body.len() > BRIDGE_MAX_BODY_BYTES {
        respond(
            &mut stream,
            413,
            &json_error("payload too large (0 < len <= 16384)"),
        );
        return;
    }
    let Ok(payload) = serde_json::from_str::<BridgeSelection>(&body) else {
        respond(&mut stream, 400, &json_error("invalid selection json"));
        return;
    };
    let (code, text) = handle_selection(&shared, &payload);
    respond(&mut stream, code, &text);
}

/// 一帧载荷的设置解析产物（纯数据；日志行是否真打由状态里的日志闸按"结论变化"决定）。
struct ResolvedSettings {
    judge: Option<u8>,
    pro: Option<bool>,
    turbo: Option<bool>,
    win_scale: Option<f64>,
    /// 候选日志（级别 + 类别 + 文本）。
    logs: Vec<(&'static str, LogKind, String)>,
}

/// 判定/Pro/Turbo/窗口解析（**插件权威** + `config.json` 兜底与交叉校验）。
///
/// 读盘策略（`config.json` 的唯一读盘点）：
///   * 插件上报了 `judge_level` ⇒ 只做**单次廉价读**做交叉校验（不需要重试窗口）；
///   * 插件没上报 ⇒ 用 `read_settings_with_retry`（≤1s，覆盖"开始游玩"的写盘竞态），
///     把文件值当兜底发布。
///
/// 读盘在锁外（不长时间持 `malody_bridge`）；日志在锁外打；去重由 `LogKind` 各自的闸门保证
/// "值变化时才打"，绝不逐帧刷屏。
fn resolve_settings(shared: &Shared, payload: &BridgeSelection, rate: f64) -> ResolvedSettings {
    let pro = payload.pro_judge;
    let turbo = payload.turbo;
    let win_scale = win_scale_for(turbo, rate);
    let config_path = malody_root(shared).map(|root| root.join("config.json"));
    let fallback = match (payload.judge_level, config_path.as_deref()) {
        (Some(_), Some(path)) => read_settings(path),
        (None, Some(path)) => read_settings_with_retry(path),
        (_, None) => None,
    };
    let file_judge = fallback.and_then(|snapshot| snapshot.judge);
    let (judge, source, cross_check) = resolve_judge(payload.judge_level, file_judge);

    let mut logs: Vec<(&'static str, LogKind, String)> = vec![(
        "info",
        LogKind::Settings,
        settings_log(source, judge, pro, turbo, rate, win_scale),
    )];
    if let Some(line) = cross_check {
        logs.push(("info", LogKind::CrossCheck, line));
    }
    // 插件没值、兜底也没值 ⇒ 一条 warn（页面回落默认 OD，必须留痕）。
    if payload.judge_level.is_none() && judge.is_none() {
        logs.push((
            "warn",
            LogKind::Unavailable,
            judge_unavailable_log(config_path.as_deref(), fallback.is_some()),
        ));
    }
    // `winScale` 判不出来 ⇒ 一条 info 说明原因（`null` 会原样进 song 帧）。
    if win_scale.is_none() {
        logs.push((
            "info",
            LogKind::WinScale,
            win_scale_fallback_log(turbo, rate),
        ));
    }
    ResolvedSettings {
        judge,
        pro,
        turbo,
        win_scale,
        logs,
    }
}

/// 真实事件处理（归一化 → 去重 → 沙箱 → 读谱 → 发帧）。返回 HTTP 状态码与响应体。
fn handle_selection(shared: &Arc<Shared>, payload: &BridgeSelection) -> (u16, String) {
    if !rate_is_valid(payload.speed_rate) {
        log_at(
            "error",
            &format!(
                "malody bridge: invalid speed_rate {:?} — treating the frame as screen=other",
                payload.speed_rate
            ),
        );
    }
    let normalized = normalize(payload);
    let rate = normalized.rate_text.parse::<f64>().unwrap_or(1.0);

    // 判定/Pro/Turbo/窗口解析**必须先于去重**：它们都进内容六元组（第 4/5/6 位），若在去重之后
    // 再解析，基线里的文本会与实际发布值分叉，逐字节重发的心跳会被误判成真实事件。
    let ResolvedSettings {
        judge,
        pro,
        turbo,
        win_scale,
        logs,
    } = resolve_settings(shared, payload, rate);

    // 去重（唯一去重点）：先判分支，再决定是否走真实事件处理。日志闸、发布值写回与去重共用
    // 同一个短锁；真正的 `log_at` 在锁外（文件 IO 不持锁）。
    let mut pending: Vec<(&'static str, String)> = Vec::new();
    let (kind, restart, event_seq) = {
        let mut state = shared.malody_bridge.lock().unwrap();
        for (level, category, line) in logs {
            if state.log_gates.gate_mut(category).due(&line) {
                pending.push((level, line));
            }
        }
        state.judge = judge;
        state.pro = pro;
        state.turbo = turbo;
        state.win_scale = win_scale;
        let content = content_key(&normalized, judge, pro, turbo);
        let outcome = classify(&mut state, content, payload.sequence, Instant::now());
        (outcome.kind, outcome.log, state.event_seq)
    };
    for (level, message) in pending {
        log_at(level, &message);
    }
    if let Some((level, message)) = restart {
        log_at(level, &message);
    }
    if kind != Dedupe::Event {
        return (202, "{}".to_string());
    }

    if normalized.screen == "other" {
        // 没有谱面可分析：只更新桥状态 + 推 state 帧（页面的清空由 state 帧的
        // eventSeq 边沿驱动）。**不发 song 帧**。
        {
            let mut state = shared.malody_bridge.lock().unwrap();
            state.screen = "other".to_string();
            state.chart_path.clear();
            state.rate_text = normalized.rate_text.clone();
        }
        broadcast(shared, "state", Some(crate::server::state_frame(shared)));
        return (202, "{}".to_string());
    }

    let Some(root) = malody_root(shared) else {
        log_at(
            "error",
            "malody bridge: REJECTED — malodyRoot unavailable (set MMA_MALODY_ROOT or malodyRoot in the shell config)",
        );
        return (
            403,
            json_error(SandboxError::RootUnavailable.message()),
        );
    };
    let chart = match sandbox_chart_path(&root, &payload.path) {
        Ok(path) => path,
        Err(err) => {
            log_at(
                "error",
                &format!(
                    "malody bridge: REJECTED {} — {}",
                    payload.path,
                    err.message()
                ),
            );
            return (403, json_error(err.message()));
        }
    };

    // 两道闸显式分开：沙箱允许 50 MB，`song` 帧的 `rawText` 只允许 5 MB。
    // 读之前先 stat——5~50 MB 的谱面必须"显式拒绝并留痕"，绝不读进来又被帧上限静默丢掉。
    let len = fs::metadata(&chart).map(|m| m.len()).unwrap_or(0);
    if len > MAX_PAYLOAD_BYTES as u64 {
        log_at(
            "error",
            &format!(
                "malody bridge: chart {} is {} bytes, exceeds the rawText cap ({} bytes) — NOT broadcast",
                chart.display(),
                len,
                MAX_PAYLOAD_BYTES
            ),
        );
        return (
            504,
            json_error("chart exceeds the rawText cap (>5MB) — not broadcast"),
        );
    }
    let Ok(text) = fs::read_to_string(&chart) else {
        log_at(
            "error",
            &format!("malody bridge: chart read FAILED {}", chart.display()),
        );
        return (500, json_error("chart read failed"));
    };

    let content_md5 = md5_hex(&text);
    let fields = derive_fields(&text, &chart, &payload.version);
    let identity = identity_of(&fields, &content_md5);
    let request_id = format!("b{}", event_seq);
    {
        let mut state = shared.malody_bridge.lock().unwrap();
        state.screen = normalized.screen.clone();
        state.chart_path = chart.to_string_lossy().to_string();
        state.rate_text = normalized.rate_text.clone();
    }
    broadcast(
        shared,
        "song",
        Some(song_frame(
            &request_id,
            &fields,
            &identity,
            &normalized.rate_text,
            text,
            &normalized.screen,
            judge,
            pro,
            turbo,
            win_scale,
        )),
    );
    // 立即推 state 帧（不等 30s 定时帧）：桥事件的场景变化是页面及时跟上的唯一可靠路径。
    broadcast(shared, "state", Some(crate::server::state_frame(shared)));
    log_at(
        "info",
        &format!(
            "malody bridge: event #{} screen={} rate={} chart={} identity={}",
            event_seq,
            normalized.screen,
            normalized.rate_text,
            chart.display(),
            identity
        ),
    );
    (202, "{}".to_string())
}

// --------------------------------------------------------- 重连补发 --

/// 新 WS 连接建立后的补发（**只发给这个连接**）：先一帧 `state`，再（满足触发条件时）
/// 当前谱面的 `song` 帧。
///
/// 触发条件钉死：`bridge_alive` **且** 已存 `screen ∈ {selection, playing, result}` **且**
/// `chart_path` 非空。⚠️ 不能只判 `last_event.is_some()`——`screen=other` 的真实事件会把
/// `last_event` 覆盖成空 `path` 的六元组，那样会补发出一个空 `path` 的 song 帧，与
/// "`screen=other` 不发 song 帧"直接冲突。
///
/// 补发**不是新事件**：不修改 `last_event` / `last_seq` / `event_seq`，也不参与去重状态机。
/// 内容按当前状态重算（重新读谱、重算 md5 与 identity）；`requestId = b{event_seq}r{k}`，
/// 与真实事件的 `b{seq}` 区分便于日志排查（补发只在注册时发生一次 ⇒ `k` 恒为 1）。
///
/// 为什么要连 state 帧一起补：注册 sink 后壳原本只发 `hello`，而 `state` 帧只由 30s
/// 定时器或真实桥事件触发（`spawn_timers` = 15s ping + 15s sleep ⇒ **周期 30s**）。
/// 页面刷新/壳重启发生在游玩中时，`song` 帧回来了、`state.malodyPlaying` 也置了真，但
/// `state.malodyAlive` 会一直是 `undefined` ⇒ 页面 L1 门控 `malodyPlaying && malodyAlive`
/// 不成立，路由与源圆点在最长 30s 内仍指向 osu/Etterna。
pub fn reconnect_messages(shared: &Shared) -> Vec<String> {
    let (alive, screen, chart_path, rate_text, event_seq, judge, pro, turbo, win_scale) = {
        let state = shared.malody_bridge.lock().unwrap();
        (
            bridge_alive(state.last_seen),
            state.screen.clone(),
            state.chart_path.clone(),
            state.rate_text.clone(),
            state.event_seq,
            state.judge,
            state.pro,
            state.turbo,
            state.win_scale,
        )
    };

    let mut out = Vec::new();
    let state_envelope = Envelope::new(
        "state",
        next_seq(shared),
        Some(crate::server::state_frame(shared)),
    );
    out.push(serde_json::to_string(&state_envelope).unwrap_or_default());

    let screen_ok = matches!(screen.as_str(), "selection" | "playing" | "result");
    if !alive || !screen_ok || chart_path.is_empty() {
        return out;
    }
    let chart = PathBuf::from(&chart_path);
    let Ok(text) = fs::read_to_string(&chart) else {
        log_at(
            "debug",
            &format!(
                "malody bridge: replay skipped, chart unreadable {}",
                chart.display()
            ),
        );
        return out;
    };
    if text.len() > MAX_PAYLOAD_BYTES {
        log_at(
            "error",
            &format!(
                "malody bridge: replay skipped, chart {} exceeds the rawText cap ({} bytes)",
                chart.display(),
                text.len()
            ),
        );
        return out;
    }
    let content_md5 = md5_hex(&text);
    // payload 的 `version` 不落库 ⇒ 补发按 `.mc` 的 `/meta/version` → 文件名 stem 回落
    // （与字段来源表一致：绝不从 `.osu` 里猜 level）。
    let fields = derive_fields(&text, &chart, "");
    let identity = identity_of(&fields, &content_md5);
    let request_id = format!("b{}r1", event_seq);
    let frame = song_frame(
        &request_id,
        &fields,
        &identity,
        &rate_text,
        text,
        &screen,
        judge,
        pro,
        turbo,
        win_scale,
    );
    out.push(
        serde_json::to_string(&Envelope::new("song", next_seq(shared), Some(frame)))
            .unwrap_or_default(),
    );
    log_at(
        "info",
        &format!(
            "malody bridge: replayed {} to a new connection (screen={} rate={})",
            request_id, screen, rate_text
        ),
    );
    out
}

// ---------------------------------------------------------------- 单测 --

#[cfg(test)]
#[path = "../../tests-local/server_bridge.rs"]
mod tests;
