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
#[path = "../../tests-local/malody4_config.rs"]
mod tests;
