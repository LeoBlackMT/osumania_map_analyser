// Malody 4.3.7 的 config.json：只读解析变速位与判定档。
//
// 只取 `user_mods` / `user_judge_level` 两个字段：该文件里的其余内容一律不读进结构体、
// 不进日志。config.json 在开局与退出时被整份重写。

use crate::malody4::model::{JudgeLevel, ModFlags};
use std::path::Path;

/// `user_mods` 的位掩码（DASH / RUSH / SLOW 互斥）。
const MOD_DASH: u64 = 0x10;
const MOD_RUSH: u64 = 0x20;
const MOD_SLOW: u64 = 0x100;

/// config.json 里本源需要的两个字段。
#[derive(Debug, Clone, PartialEq)]
pub struct GameConfig {
    pub user_mods: u64,
    pub user_judge_level: u64,
}

impl GameConfig {
    /// 变速档位。
    pub fn mod_flags(&self) -> ModFlags {
        ModFlags {
            dash: self.user_mods & MOD_DASH != 0,
            rush: self.user_mods & MOD_RUSH != 0,
            slow: self.user_mods & MOD_SLOW != 0,
        }
    }

    /// 判定档字母（越界值为 `None`）。
    pub fn judge_letter(&self) -> Option<char> {
        u8::try_from(self.user_judge_level)
            .ok()
            .and_then(|level| JudgeLevel(level).letter())
    }
}

/// 解析 config.json 文本；缺失字段各自回落为 0，非 JSON 文本返回 `None`。
pub fn parse(text: &str) -> Option<GameConfig> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    Some(GameConfig {
        user_mods: value.get("user_mods").and_then(|v| v.as_u64()).unwrap_or(0),
        user_judge_level: value
            .get("user_judge_level")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
    })
}

/// 读取并解析 config.json；IO 失败返回 `None`。
pub fn read(path: &Path) -> Option<GameConfig> {
    let text = std::fs::read_to_string(path).ok()?;
    parse(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_reads_mods_and_judge_level() {
        let cfg = parse(r#"{"user_mods":32,"user_judge_level":1}"#).unwrap();
        assert_eq!(
            cfg,
            GameConfig {
                user_mods: 32,
                user_judge_level: 1
            }
        );
        assert_eq!(
            cfg.mod_flags(),
            ModFlags {
                dash: false,
                rush: true,
                slow: false
            }
        );
        assert_eq!(cfg.judge_letter(), Some('B'));
    }

    #[test]
    fn parse_missing_fields_fall_back_to_zero() {
        let only_mods = parse(r#"{"user_mods":16}"#).unwrap();
        assert_eq!(only_mods.user_judge_level, 0);
        assert_eq!(only_mods.judge_letter(), Some('A'));
        let empty_object = parse("{}").unwrap();
        assert_eq!(empty_object, GameConfig { user_mods: 0, user_judge_level: 0 });
        assert_eq!(empty_object.mod_flags().speed_rate(), 1.0);
        assert_eq!(empty_object.judge_letter(), Some('A'));
        // 非整数类型同样回落
        let wrong_types = parse(r#"{"user_mods":"32","user_judge_level":null}"#).unwrap();
        assert_eq!(wrong_types, GameConfig { user_mods: 0, user_judge_level: 0 });
    }

    #[test]
    fn parse_corrupt_json_is_none() {
        assert!(parse("{ not json").is_none());
        assert!(parse("}").is_none());
        assert!(parse("\u{feff}{\"user_mods\":1}").is_none());
    }

    #[test]
    fn parse_empty_string_is_none() {
        assert!(parse("").is_none());
    }

    #[test]
    fn parse_non_object_json_falls_back_to_zero() {
        // 数组 / 字符串 / 数字 / 布尔 / null 都是合法 JSON，只是"没有这两个字段"：
        // 一律回落 0（不是 None）——`parse` 只对"不是 JSON"返回 None。
        for text in ["[]", "[1,2]", "\"user_mods\"", "123", "true", "null"] {
            let cfg = parse(text).unwrap_or_else(|| panic!("{text} 应解析为默认配置"));
            assert_eq!(
                cfg,
                GameConfig {
                    user_mods: 0,
                    user_judge_level: 0
                },
                "{text}"
            );
            assert_eq!(cfg.mod_flags().speed_rate(), 1.0, "{text}");
            assert_eq!(cfg.judge_letter(), Some('A'), "{text}");
        }
    }

    /// 结构体只有 `user_mods` / `user_judge_level` 两个字段（无 `Serialize`，故用 `Debug` 反查）：
    /// 真实 `config.json` 里的 `user_id` / `token` / `sound_hash` 一个都进不了结构体。
    #[test]
    fn parsed_config_carries_no_field_besides_the_two() {
        let text = r#"{"user_id":"1234567","token":"secret-token","sound_hash":"deadbeef",
            "user_mods":16,"user_judge_level":3,"volume":80,"theme":"dark"}"#;
        let cfg = parse(text).unwrap();
        assert_eq!(
            cfg,
            GameConfig {
                user_mods: 16,
                user_judge_level: 3
            }
        );
        let debug = format!("{cfg:?}");
        assert_eq!(debug, "GameConfig { user_mods: 16, user_judge_level: 3 }");
        for absent in ["1234567", "secret-token", "deadbeef", "volume", "theme"] {
            assert!(!debug.contains(absent), "无关字段进了结构体：{absent}");
        }
    }

    #[test]
    fn mod_flags_unknown_bits_are_ignored() {
        let rate = |raw: u64| {
            GameConfig {
                user_mods: raw,
                user_judge_level: 0,
            }
            .mod_flags()
            .speed_rate()
        };
        // 0x8 / 0x400（FAIR 判定位）/ 0x8000 都不是变速位
        assert_eq!(rate(0x8), 1.0);
        assert_eq!(rate(0x400), 1.0);
        assert_eq!(rate(0x8000), 1.0);
        assert_eq!(rate(0x408), 1.0);
        // 未知位与变速位共存时不干扰取值
        assert_eq!(rate(0x408 | 0x20), 1.5);
        assert_eq!(rate(0x8 | 0x100), 0.8);
        assert_eq!(rate(u64::MAX), 1.2, "全位置位时按固定序取 DASH");
    }

    #[test]
    fn mod_flags_bitmask() {
        let flags = |raw: u64| GameConfig { user_mods: raw, user_judge_level: 0 }.mod_flags();
        assert_eq!(flags(0x10), ModFlags { dash: true, rush: false, slow: false });
        assert_eq!(flags(0x20), ModFlags { dash: false, rush: true, slow: false });
        assert_eq!(flags(0x100), ModFlags { dash: false, rush: false, slow: true });
        assert_eq!(flags(0), ModFlags { dash: false, rush: false, slow: false });
        assert_eq!(flags(0x110).speed_rate(), 1.2);
        assert_eq!(flags(0x130).speed_rate(), 1.2);
        assert_eq!(flags(0x120).speed_rate(), 1.5);
    }

    #[test]
    fn judge_letter_out_of_range_is_none() {
        let letter = |raw: u64| GameConfig { user_mods: 0, user_judge_level: raw }.judge_letter();
        assert_eq!(letter(0), Some('A'));
        assert_eq!(letter(4), Some('E'));
        assert_eq!(letter(5), None);
        assert_eq!(letter(255), None);
        assert_eq!(letter(256), None);
        assert_eq!(letter(u64::MAX), None);
    }

    #[test]
    fn read_missing_file_is_none() {
        let path = std::env::temp_dir().join(format!(
            "mma-malody4-config-missing-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        assert!(read(&path).is_none());
    }

    #[test]
    fn read_round_trips_through_a_synthetic_file() {
        let path = std::env::temp_dir().join(format!(
            "mma-malody4-config-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::write(&path, "{\"user_mods\":256,\"user_judge_level\":4}").unwrap();
        let cfg = read(&path).unwrap();
        assert_eq!(cfg.user_mods, 256);
        assert_eq!(cfg.mod_flags().speed_rate(), 0.8);
        assert_eq!(cfg.judge_letter(), Some('E'));
        let _ = std::fs::remove_file(&path);
    }
}
