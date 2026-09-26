// Split out of `mod.rs` for size only: same items, same behaviour, re-exported
// from the parent so `crate::malody4::*` and every existing caller keep working.

use super::RATE_EPS;
use super::anchor::{MemorySource, RawSettings};
use super::config::GameConfig;


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
pub fn settings_log(effective: &EffectiveSettings) -> String {
    format!(
        "malody4 settings: source={} judge={} speed_rate={:.2}",
        effective.source.as_str(),
        judge_text(effective.judge),
        effective.rate
    )
}

/// `S` / `P` 持续不一致的一次性告警文案（纯函数；两边的原始值都给出来便于定位）。
pub fn chain_disagreement_warning(user: RawSettings, play: RawSettings, polls: u32) -> String {
    format!(
        "malody4 settings chains disagree for {polls} consecutive polls: user-settings judge={} user_mods={:#x} vs play-config judge={} user_mods={:#x} — publishing the play-config value",
        user.judge, user.user_mods, play.judge, play.user_mods
    )
}

/// 内存值与 `config.json` 不一致时的一次性告警文案（纯函数；两边都给出来）。
pub fn file_cross_check_warning(effective: &EffectiveSettings, file: &GameConfig) -> String {
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
pub struct SettingsLogKey {
    pub source: SettingsOrigin,
    pub judge: Option<char>,
    pub rate: f64,
}

impl SettingsLogKey {
    /// 速率按 `RATE_EPS` 容差比较（来源与判定档精确比较）。
    pub fn matches(&self, other: &Self) -> bool {
        self.source == other.source
            && self.judge == other.judge
            && (self.rate - other.rate).abs() < RATE_EPS
    }
}
