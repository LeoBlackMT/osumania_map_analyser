// 壳-页面桥帧定义（CONTRACT.md 契约版本 1 的直接实现）。

use serde::{Deserialize, Serialize};
use std::time::Duration;

pub const CONTRACT_VERSION: u32 = 2;
pub const MAX_PAYLOAD_BYTES: usize = 5 * 1024 * 1024;
pub const POST_TIMEOUT: Duration = Duration::from_secs(30);
pub const TOSU_PROBE_INTERVAL: Duration = Duration::from_secs(30);
pub const PING_INTERVAL: Duration = Duration::from_secs(15);

// ---- 错误文本常量（编辑器 ShowMessage 直用）----

pub fn source_not_active(active: &str) -> String {
    format!("路由不可用：当前活跃源为 {}", active)
}
pub fn payload_too_large() -> &'static str {
    "谱面文件过大（>5MB 字节）"
}
pub fn timeout_msg() -> &'static str {
    "分析超时（30s）"
}
pub fn analysis_failed(errors: &[String]) -> String {
    format!("分析失败：{}", errors.join("；"))
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