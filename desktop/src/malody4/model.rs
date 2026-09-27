// Malody 4.3.7 的线上记录模型：场景、mod 速率、判定档、selection 记录与不可用原因。

use std::sync::Once;

pub use crate::malody4::anchor::IdentityKey;

/// selection 记录的 `source` 值（与参考桥逐字对齐）。
pub const SOURCE_ID: &str = "malody4-native";

/// 游戏场景（日志 `switch to N` 的语义映射）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Selection,
    Playing,
    Result,
    Other,
}

impl Screen {
    pub fn as_str(&self) -> &'static str {
        match self {
            Screen::Selection => "selection",
            Screen::Playing => "playing",
            Screen::Result => "result",
            Screen::Other => "other",
        }
    }
}

/// `config.json` 的 `user_mods` 位（互斥的变速档）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModFlags {
    pub dash: bool,
    pub rush: bool,
    pub slow: bool,
}

/// 多个变速位同时命中时的异常只记一次（进程内）。
static MULTI_SPEED_MOD_WARNED: Once = Once::new();

impl ModFlags {
    /// 速率：DASH 1.2 / RUSH 1.5 / SLOW 0.8，都未命中 1.0。
    /// 多位同时命中属异常，按固定序 DASH > RUSH > SLOW 取值并记一次日志。
    pub fn speed_rate(&self) -> f64 {
        let hits = u8::from(self.dash) + u8::from(self.rush) + u8::from(self.slow);
        if hits > 1 {
            MULTI_SPEED_MOD_WARNED.call_once(|| {
                eprintln!("[malody4] multiple speed mod flags set, taking DASH > RUSH > SLOW");
            });
        }
        if self.dash {
            1.2
        } else if self.rush {
            1.5
        } else if self.slow {
            0.8
        } else {
            1.0
        }
    }
}

/// 判定档：0..=4 → 'A'..='E'，越界值为"判定未知"（不猜档位）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JudgeLevel(pub u8);

impl JudgeLevel {
    pub fn letter(&self) -> Option<char> {
        match self.0 {
            0 => Some('A'),
            1 => Some('B'),
            2 => Some('C'),
            3 => Some('D'),
            4 => Some('E'),
            _ => None,
        }
    }
}

/// selection 记录（线上 wire 记录）。
///
/// 字段名逐个显式 `rename`：**不用 `rename_all`**——`rename_all = "camelCase"` 会把
/// `speed_rate` 变成 `speedRate`，而该字段名绝不能被改。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Selection {
    #[serde(rename = "path")]
    pub path: String,
    #[serde(rename = "speed_rate")]
    pub speed_rate: f64,
    /// `Screen::as_str()` 的字符串。
    #[serde(rename = "screen")]
    pub screen: &'static str,
    #[serde(rename = "sequence")]
    pub sequence: i64,
    #[serde(rename = "event")]
    pub event: String,
    #[serde(rename = "version")]
    pub version: String,
    #[serde(rename = "chart_hash")]
    pub chart_hash: String,
    #[serde(rename = "source")]
    pub source: &'static str,
}

impl Selection {
    /// 只读 getter（字段值恒为 `SOURCE_ID`）。
    pub fn source(&self) -> &'static str {
        self.source
    }

    /// 不可用记录：除 `sequence` / `event` 外全部为空。
    pub fn hidden(sequence: i64, event: &str) -> Selection {
        Selection {
            path: String::new(),
            speed_rate: 1.0,
            screen: Screen::Other.as_str(),
            sequence,
            event: event.to_string(),
            version: String::new(),
            chart_hash: String::new(),
            source: SOURCE_ID,
        }
    }
}

/// 本源不可用的原因（`NoSelection` 是正常态：没有选中谱面）。
#[derive(Debug, Clone, PartialEq)]
pub enum UnavailableReason {
    NoSelection,
    /// 索引里**暂时**没有这张谱：还没有（或还没试完）重建尝试，库可能仍在建、文件可能刚落地。
    ///
    /// 轮询器不再自己发这个字面量：miss 窗口改由下面的 `ChartUnresolved` 覆盖（页面对临时态要有
    /// 话说）。它仍是 selection 状态机"身份键在、条目不在"的成因，也仍是契约闭集里的合法取值。
    ChartNotIndexed,
    /// 这个身份键**已经查过、当前查不到，但重试还没试完**（重建请求已发出 / 仍在窗口内）。
    ///
    /// 临时态，存在的理由是**时间**：miss 在 200ms 内就能测出来，而"库里根本没有这张谱"的结论要等
    /// `INDEX_REBUILD_THROTTLE`（5s）× `MISS_REBUILD_ATTEMPTS`（2）≈ 10s 的重试窗口走完才知道
    /// （真机复测：卡片上的提示要 9~10s 才出现）。页面对它显示"正在解析"这类临时提示，用户第一
    /// 时间就有话说；结论出来后才换成 `ChartUnknownIdentity`。重试本身不受影响（不缩短、不取消）。
    ChartUnresolved,
    /// 这个身份键**已查过、且重建尝试已耗尽仍解析不出来**（本次会话不会再为它重建）。
    ///
    /// 与 `ChartNotIndexed` / `ChartUnresolved` 的分界是"试完没有"，不是"查没查过"：
    /// - `ChartNotIndexed` / `ChartUnresolved` = 结果未定（还在重试窗口内，或索引尚未就绪）；
    /// - `ChartUnknownIdentity` = 结论已定（实测现场 25 个身份键里 19 个在盘上根本没有对应文件）。
    /// 三者对页面的**行为完全相同**（hidden 帧、卡片保留上一张谱面）；这一区分只让"这张谱不在你的
    /// 库里"（结论）与"还在给你找"（临时）在提示文案上可分辨。
    ChartUnknownIdentity,
    ProcessNotFound,
    MultipleInstances,
    AccessDenied,
    BadRead,
    TargetMismatch(&'static str),
    RootNotConfigured,
    NoLibrary,
    PlatformUnsupported,
}

static UNKNOWN_TARGET_MISMATCH_WARNED: Once = Once::new();

impl UnavailableReason {
    /// 稳定的原因字面量（壳 state 帧 `reason` 的闭集）。`NoSelection` 返回空串，
    /// 由调用方用它清空 `reason` 字段。
    pub fn as_str(&self) -> String {
        match self {
            UnavailableReason::NoSelection => String::new(),
            UnavailableReason::ChartNotIndexed => "chart-not-indexed".to_string(),
            UnavailableReason::ChartUnresolved => "chart-unresolved".to_string(),
            UnavailableReason::ChartUnknownIdentity => "chart-unknown-identity".to_string(),
            UnavailableReason::ProcessNotFound => "process-not-found".to_string(),
            UnavailableReason::MultipleInstances => "multiple-instances".to_string(),
            UnavailableReason::AccessDenied => "access-denied".to_string(),
            UnavailableReason::BadRead => "bad-read".to_string(),
            UnavailableReason::TargetMismatch("pe_timestamp_mismatch") => {
                "target-mismatch:pe_timestamp_mismatch".to_string()
            }
            UnavailableReason::TargetMismatch("file_size_mismatch") => {
                "target-mismatch:file_size_mismatch".to_string()
            }
            UnavailableReason::TargetMismatch("pe_header_out_of_range") => {
                "target-mismatch:pe_header_out_of_range".to_string()
            }
            UnavailableReason::TargetMismatch(_) => {
                UNKNOWN_TARGET_MISMATCH_WARNED.call_once(|| {
                    eprintln!("[malody4] unknown target-mismatch detail, reporting target-mismatch:unknown");
                });
                "target-mismatch:unknown".to_string()
            }
            UnavailableReason::RootNotConfigured => "root-not-configured".to_string(),
            UnavailableReason::NoLibrary => "no-library".to_string(),
            UnavailableReason::PlatformUnsupported => "platform-unsupported".to_string(),
        }
    }
}

/// 摘要 → 小写十六进制字符串。调用方传 `digest.as_slice()`。
pub fn hex16(digest: &[u8]) -> String {
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

#[cfg(test)]
#[path = "../../tests-local/malody4_model.rs"]
mod tests;
