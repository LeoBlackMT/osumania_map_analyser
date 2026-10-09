// server::bridge::state - Malody V 桥运行期状态、去重基线与事件分类

use std::time::Instant;
use crate::frames::{MalodyBridgeSource, BRIDGE_STALE_AFTER};

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
    pub fn gate_mut(&mut self, kind: LogKind) -> &mut LogGate {
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
