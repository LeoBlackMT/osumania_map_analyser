// 壳-页面桥帧定义（CONTRACT.md 契约版本 5 的直接实现）。

use serde::{Deserialize, Serialize};
use std::time::Duration;

pub const CONTRACT_VERSION: u32 = 5;
pub const MAX_PAYLOAD_BYTES: usize = 5 * 1024 * 1024;
pub const POST_TIMEOUT: Duration = Duration::from_secs(30);
pub const TOSU_PROBE_INTERVAL: Duration = Duration::from_secs(30);
pub const PING_INTERVAL: Duration = Duration::from_secs(15);

// ---- Malody V BepInEx 选曲桥（17653）----
/// 桥 listener 端口（上游插件 `UseProxy=false` 时固定 POST 到这里）。
pub const BRIDGE_PORT: u16 = 17653;
/// `POST /selection` 的 body 上限（与上游参考实现 `service.py:283-299` 的 16384 一致）。
pub const BRIDGE_MAX_BODY_BYTES: usize = 16 * 1024;
/// 桥存活窗口：任意 POST（含 2s 心跳）在此时限内即算桥还在。
pub const BRIDGE_STALE_AFTER: Duration = Duration::from_secs(8);
/// 桥下发的谱面文件上限（沙箱闸）；`song` 帧的 `rawText` 另有 5MB 闸（`MAX_PAYLOAD_BYTES`）。
pub const CHART_MAX_BYTES: u64 = 50 * 1024 * 1024;

// ---- 错误文本常量（编辑器 ShowMessage 直用）----

pub fn source_not_active(active: &str) -> String {
    format!("route unavailable: current active source is {}", active)
}
pub fn payload_too_large() -> &'static str {
    "chart file too large (>5MB)"
}
pub fn timeout_msg() -> &'static str {
    "analysis timeout (30s)"
}
pub fn analysis_failed(errors: &[String]) -> String {
    format!("analysis failed: {}", errors.join("; "))
}

// ---- 帧信封 ----

#[derive(Serialize, Clone)]
pub struct Envelope {
    pub v: u32,
    #[serde(rename = "type")]
    pub frame_type: String,
    pub seq: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Value>,
}

impl Envelope {
    pub fn new(frame_type: &str, seq: u64, payload: Option<serde_json::Value>) -> Self {
        Envelope {
            v: CONTRACT_VERSION,
            frame_type: frame_type.to_string(),
            seq,
            payload,
        }
    }
}

// ---- 帧载荷（契约字段为 camelCase）----

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct HelloFrame {
    pub tosu_online: bool,
    pub contract: u32,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct SourcesFrame {
    pub etterna: EtternaSource,
    pub malody: MalodySource,
    pub malody4: Malody4Source,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct EtternaSource {
    pub alive: bool,
    pub playing: bool,
    pub playing_expire_at: Option<u64>,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct MalodySource {
    pub alive: bool,
}

/// v4：`sources.malody` 的完整形态（Malody V 选曲桥；v5 起为八个字段）。
///
/// 与上面的 `MalodySource` 分开的原因：`malody4::build_state_frame` 用结构体字面量组装
/// `MalodySource`（v3 的 `{alive}` 形态），而 v4 要求 `sources.malody` 整体变成这组字段。
/// 定义在这里与其余帧字段同处一地，由 `server::bridge::malody_source` 组装、
/// `server::state_frame` 覆盖 `sources.malody` 对象——两侧都不必改别人的模块。
#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct MalodyBridgeSource {
    /// 桥存活（8s 内有过 POST）或 Lua 通道存活（60s 内有过 POST/文件请求）。
    pub alive: bool,
    /// 当前可用通道：`bridge` / `lua` / `none`。
    pub transport: String,
    /// 场景：`selection` / `playing` / `result` / `other`；桥不存活时恒为 `none`。
    pub screen: String,
    /// 是否正在游玩（= 桥存活且 `screen == "playing"`）。
    pub playing: bool,
    /// 真实事件序号（只在真实事件时 +1，心跳与重复观察都不动）。
    pub event_seq: u64,
    /// 判定档数值（`0..=4` ↔ A~E）；尚未采集到为 `null`。
    pub judge: Option<u8>,
    /// Pro（严格组）开关（v5 新增）；插件未上报为 `null`（页面据此关闭动态 OD，不冒充常态组）。
    pub pro: Option<bool>,
    /// Turbo 开关（v5 新增）；插件未上报为 `null`。
    pub turbo: Option<bool>,
}

/// Malody 4.3.7 原生源的状态位（`reason` 为闭集字面量，健康/空闲时为空串 → 不出现在帧里）。
#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct Malody4Source {
    pub alive: bool,
    pub playing: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub screen: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub judge: Option<char>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StateFrame {
    pub tosu_online: bool,
    pub errors: Vec<String>,
    pub sources: SourcesFrame,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ModData {
    pub speed_rate: String,
    pub od_flag: String,
    pub cvt_flag: String,
    pub classic: u8,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SongMeta {
    pub title: String,
    pub artist: String,
    pub version: String,
    pub keys: u64,
    pub dev_msd8: Vec<f64>,
    /// Malody 4 源的判定档字母（`A`~`E`）；缺省表示未知（页面回落 C 档）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub judge: Option<char>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CoverRef {
    pub path: String,
    pub url: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SongFrame {
    pub request_id: Option<String>,
    pub source: String,
    pub identity: String,
    pub mod_data: ModData,
    pub meta: SongMeta,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover: Option<CoverRef>,
    pub raw_text: String,
    /// 桥通道的场景（`selection` / `playing` / `result`，v4 新增）。
    /// **Lua 通道与其它源不带此字段**（缺省即"不是桥帧"，页面据此置卡片归属）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub screen: Option<String>,
    /// 判定档数值（`0..=4` ↔ A~E；未知为 `null`，v4 新增）。与判定档一起下发是为了
    /// 避免"换谱同时改判定"时页面拿到旧档（判定采集与桥事件异步）。
    pub judge: Option<u8>,
    /// Pro（严格组）开关（v5 新增）。`null` = **未知**（插件没上报）⇒ 页面关闭动态 OD 并
    /// 在状态行明示，**不得**按常态组冒充（与 `winScale == null` 同构）。
    pub pro: Option<bool>,
    /// Turbo 开关（v5 新增）。`null` = 未知。
    pub turbo: Option<bool>,
    /// 窗口缩放因子（v5 新增语义）：只有"**确认非 Turbo**"（`turbo == false`）且倍率命中名义值
    /// （1.2 / 1.5 / 0.8，±0.005）时才是 `1/名义倍率`；Turbo / 未知 / 自定义倍率一律 `null`
    /// ——**绝不回落 `1.0`**（回落会把 Dash/Rush/Slow 当成 Turbo，正是本规则要防的混淆）。
    pub win_scale: Option<f64>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SettingsFrame {
    pub settings: serde_json::Value,
}

// 页面 → 壳：只关心应答所需字段（契约 camelCase）。
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ResultInbound {
    pub request_id: Option<String>,
    pub status_hint: Option<String>,
    pub active_source: Option<String>,
    pub errors: Option<Vec<String>>,
}

// 页面 → 壳控制帧（契约 v2：窗口操控）。
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ControlInbound {
    pub action: String,
    pub value: Option<bool>,
}

// Malody 编辑器 POST 载荷。
#[derive(Deserialize)]
pub struct MalodyPost {
    pub meta: MalodyPostMeta,
    #[serde(rename = "chartText")]
    pub chart_text: String,
}

#[derive(Deserialize)]
pub struct MalodyPostMeta {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub level: String,
    #[serde(default)]
    pub keys: u64,
}