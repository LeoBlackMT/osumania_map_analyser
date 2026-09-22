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
    ChartNotIndexed,
    /// 这个身份键**已查过、且重建尝试已耗尽仍解析不出来**（本次会话不会再为它重建）。
    ///
    /// 与 `ChartNotIndexed` 的分界是"试完没有"，不是"查没查过"：
    /// - `ChartNotIndexed` = 结果未定（还在重试窗口内，或索引尚未就绪）；
    /// - `ChartUnknownIdentity` = 结论已定（实测现场 25 个身份键里 19 个在盘上根本没有对应文件）。
    /// 两者对页面的**行为完全相同**（hidden 帧、卡片保留上一张谱面）；这一区分只让"这张谱不在你的
    /// 库里"与"索引还没建好"在诊断面上可分辨。
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
mod tests {
    use super::*;

    #[test]
    fn screen_strings() {
        assert_eq!(Screen::Selection.as_str(), "selection");
        assert_eq!(Screen::Playing.as_str(), "playing");
        assert_eq!(Screen::Result.as_str(), "result");
        assert_eq!(Screen::Other.as_str(), "other");
    }

    #[test]
    fn selection_serialises_with_exact_keys_and_values() {
        let sel = Selection {
            path: "Maps/x/0/y.mc".to_string(),
            speed_rate: 1.2,
            screen: Screen::Selection.as_str(),
            sequence: 3,
            event: "anchor-changed".to_string(),
            version: "Hard".to_string(),
            chart_hash: "ab".to_string(),
            source: SOURCE_ID,
        };
        let json = serde_json::to_string(&sel).unwrap();
        assert_eq!(
            json,
            r#"{"path":"Maps/x/0/y.mc","speed_rate":1.2,"screen":"selection","sequence":3,"event":"anchor-changed","version":"Hard","chart_hash":"ab","source":"malody4-native"}"#
        );
        // 逐键断言：将来若有人改成 rename_all，这里会先炸
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["path"], "Maps/x/0/y.mc");
        assert_eq!(value["speed_rate"], 1.2);
        assert_eq!(value["screen"], "selection");
        assert_eq!(value["sequence"], 3);
        assert_eq!(value["event"], "anchor-changed");
        assert_eq!(value["version"], "Hard");
        assert_eq!(value["chart_hash"], "ab");
        assert_eq!(value["source"], "malody4-native");
        assert_eq!(value.as_object().unwrap().len(), 8);
        assert!(!json.contains("speedRate"));
    }

    #[test]
    fn hidden_record_has_concrete_values_and_rising_sequence() {
        let first = Selection::hidden(1, "hidden");
        assert_eq!(first.path, "");
        assert_eq!(first.version, "");
        assert_eq!(first.chart_hash, "");
        assert_eq!(first.screen, "other");
        assert_eq!(first.speed_rate, 1.0);
        assert_eq!(first.event, "hidden");
        assert_eq!(first.sequence, 1);
        assert_eq!(first.source, "malody4-native");
        assert_eq!(first.source(), "malody4-native");
        assert_eq!(
            serde_json::to_string(&first).unwrap(),
            r#"{"path":"","speed_rate":1.0,"screen":"other","sequence":1,"event":"hidden","version":"","chart_hash":"","source":"malody4-native"}"#
        );
        let second = Selection::hidden(2, "hidden");
        assert!(second.sequence > first.sequence);
    }

    #[test]
    fn mod_flags_single_hits() {
        let dash = ModFlags { dash: true, rush: false, slow: false };
        let rush = ModFlags { dash: false, rush: true, slow: false };
        let slow = ModFlags { dash: false, rush: false, slow: true };
        assert_eq!(dash.speed_rate(), 1.2);
        assert_eq!(rush.speed_rate(), 1.5);
        assert_eq!(slow.speed_rate(), 0.8);
        assert_eq!(ModFlags { dash: false, rush: false, slow: false }.speed_rate(), 1.0);
    }

    #[test]
    fn mod_flags_multi_hits_use_fixed_order() {
        assert_eq!(ModFlags { dash: true, rush: true, slow: true }.speed_rate(), 1.2);
        assert_eq!(ModFlags { dash: true, rush: false, slow: true }.speed_rate(), 1.2);
        assert_eq!(ModFlags { dash: false, rush: true, slow: true }.speed_rate(), 1.5);
    }

    #[test]
    fn judge_level_letters_and_out_of_range() {
        for (level, letter) in [(0u8, 'A'), (1, 'B'), (2, 'C'), (3, 'D'), (4, 'E')] {
            assert_eq!(JudgeLevel(level).letter(), Some(letter));
        }
        assert_eq!(JudgeLevel(5).letter(), None);
        assert_eq!(JudgeLevel(9).letter(), None);
        assert_eq!(JudgeLevel(255).letter(), None);
    }

    #[test]
    fn unavailable_reason_literals() {
        assert_eq!(UnavailableReason::NoSelection.as_str(), "");
        assert_eq!(UnavailableReason::ChartNotIndexed.as_str(), "chart-not-indexed");
        assert_eq!(
            UnavailableReason::ChartUnknownIdentity.as_str(),
            "chart-unknown-identity"
        );
        assert_eq!(UnavailableReason::ProcessNotFound.as_str(), "process-not-found");
        assert_eq!(UnavailableReason::MultipleInstances.as_str(), "multiple-instances");
        assert_eq!(UnavailableReason::AccessDenied.as_str(), "access-denied");
        assert_eq!(UnavailableReason::BadRead.as_str(), "bad-read");
        assert_eq!(UnavailableReason::RootNotConfigured.as_str(), "root-not-configured");
        assert_eq!(UnavailableReason::NoLibrary.as_str(), "no-library");
        assert_eq!(UnavailableReason::PlatformUnsupported.as_str(), "platform-unsupported");
    }

    /// 新原因的字面量逐字钉死，且与"暂时没查到"的 `chart-not-indexed` 是两个不同的串；
    /// `NoSelection` 仍返回空串（正常态靠空串清 `reason`，这一条不得被新原因带偏）。
    #[test]
    fn chart_unknown_identity_literal_is_exact_and_distinct() {
        assert_eq!(
            UnavailableReason::ChartUnknownIdentity.as_str(),
            "chart-unknown-identity"
        );
        assert_ne!(
            UnavailableReason::ChartUnknownIdentity.as_str(),
            UnavailableReason::ChartNotIndexed.as_str()
        );
        assert_eq!(UnavailableReason::NoSelection.as_str(), "");
    }

    #[test]
    fn target_mismatch_literals_and_unknown_fallback() {
        assert_eq!(
            UnavailableReason::TargetMismatch("pe_timestamp_mismatch").as_str(),
            "target-mismatch:pe_timestamp_mismatch"
        );
        assert_eq!(
            UnavailableReason::TargetMismatch("file_size_mismatch").as_str(),
            "target-mismatch:file_size_mismatch"
        );
        assert_eq!(
            UnavailableReason::TargetMismatch("pe_header_out_of_range").as_str(),
            "target-mismatch:pe_header_out_of_range"
        );
        assert_eq!(
            UnavailableReason::TargetMismatch("something-else").as_str(),
            "target-mismatch:unknown"
        );
    }

    #[test]
    fn identity_key_is_reexported() {
        let key = IdentityKey { md5: "ab".to_string(), slot: 4 };
        assert_eq!(key.slot, 4);
    }

    #[test]
    fn hex16_is_lowercase_hex() {
        assert_eq!(hex16(&[0x00, 0x0f, 0xa5, 0xff]), "000fa5ff");
        assert_eq!(hex16(&[]), "");
        assert_eq!(hex16(&[0xde, 0xad, 0xbe, 0xef]), "deadbeef");
    }
}
