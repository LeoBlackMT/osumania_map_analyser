// server::bridge::settings - 判定档、Pro/Turbo 开关与 winScale 计算与交叉校验

use std::fs;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};
use super::state::{flag_text, win_scale_text};

/// 名义倍率（Dash / Rush / Slow，与 4.3.7 的 `ModFlags::speed_rate` 同序）。
/// `winScale` 的取值就是命中项的名义倍率的倒数（**用名义值，不用 `speed_rate`**）。
pub const NOMINAL_RATES: [f64; 3] = [1.2, 1.5, 0.8];

/// 名义倍率与桥 `speed_rate` 的一致性容差（±0.005）。
pub const RATE_TOLERANCE: f64 = 0.005;

/// 写盘竞态的一次重试：总窗口（≤1s）与轮询间隔。**只服务"插件没给 `judge_level`"的兜底读**
/// （插件给了值就不需要重试窗口——交叉校验用单次廉价读）。
pub const SNAPSHOT_RETRY_WINDOW: Duration = Duration::from_secs(1);
pub const SNAPSHOT_RETRY_POLL: Duration = Duration::from_millis(100);

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
