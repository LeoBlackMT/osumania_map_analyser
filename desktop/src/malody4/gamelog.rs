// Malody 4.3.7 游戏日志：场景行解析与日志文件选择器。
//
// 场景行形如 `<秒> [MS] LOG: switch to N`，另有 `switchback` / `switch back`（回退）
// 与 `switchbefore to N`（即将切换的预声明）等变体。
//
// 目录枚举集中在 `latest_log(dir)` 里（主循环只调用它，自己不遍历文件系统）。

use crate::malody4::model::Screen;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// 场景行标记。
const MARKER: &str = "[MS]";
/// 场景切换标记（含尾随空格，避免匹配到 `switchbefore` 之类的变体）。
const SWITCH_TO: &str = "switch to ";

/// 日志场景跟踪器：逐行喂入最新日志文件的新增内容。
#[derive(Debug, Clone)]
pub struct SceneTracker {
    screen: Screen,
    known: bool,
    last_change: Option<Instant>,
}

impl SceneTracker {
    pub fn new() -> Self {
        SceneTracker {
            screen: Screen::Other,
            known: false,
            last_change: None,
        }
    }

    /// 喂入一行日志；不构成场景切换的行什么都不改。
    pub fn apply_line(&mut self, line: &str) {
        if !line.contains(MARKER) {
            return;
        }
        // 回退与预声明都不是场景切换：switchback / switch back 是回退到上一场景，
        // switchbefore to N 是"即将切换"，二者都不更新 screen，也不更新 last_change。
        if line.contains("switchback") || line.contains("switch back") || line.contains("switchbefore")
        {
            return;
        }
        let rest = match line.split_once(SWITCH_TO) {
            Some((_, rest)) => rest,
            None => return,
        };
        let digits: String = rest
            .chars()
            .skip_while(|c| !c.is_ascii_digit())
            .take_while(char::is_ascii_digit)
            .collect();
        let value: u64 = match digits.parse() {
            Ok(value) => value,
            // 空（`switch to ` 后无数字）或超出 u64 的数字串：无法识别
            Err(_) => return,
        };
        let mapped = match value {
            1 | 2 => Screen::Selection,
            3 => Screen::Playing,
            4 => Screen::Result,
            // 其余（含 7 / 8）都是"其他场景"
            _ => Screen::Other,
        };
        self.known = true;
        if mapped != self.screen {
            self.screen = mapped;
            self.last_change = Some(Instant::now());
        }
    }

    pub fn screen(&self) -> Screen {
        self.screen
    }

    /// 是否见过至少一条可识别的场景行。
    pub fn screen_known(&self) -> bool {
        self.known
    }

    pub fn last_change(&self) -> Option<Instant> {
        self.last_change
    }

    /// 是否处于"新鲜的对局中"：场景为 Playing 且最后一次场景切换在窗口内。
    pub fn fresh(&self, window: Duration) -> bool {
        if self.screen != Screen::Playing {
            return false;
        }
        match self.last_change {
            Some(at) => at.elapsed() < window,
            None => false,
        }
    }
}

impl Default for SceneTracker {
    fn default() -> Self {
        SceneTracker::new()
    }
}

/// 从日志目录条目里选出最新的一份（文件名编码了时间戳，故字典序即时间序）。
///
/// 只认 `log-` 前缀 + `.txt` 后缀；返回文件名（不是路径）。
pub fn pick_latest_log(dir_entries: &[(String, SystemTime)]) -> Option<String> {
    dir_entries
        .iter()
        .map(|(name, _)| name)
        .filter(|name| name.starts_with("log-") && name.ends_with(".txt"))
        .max()
        .cloned()
}

/// 枚举日志目录并选出最新的一份（`fs::read_dir` + `pick_latest_log` 选择器）。
/// 目录不存在 / 不可读 → `None`（调用方保留最后已知场景）。
pub fn latest_log(dir: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    let names: Vec<(String, SystemTime)> = entries
        .flatten()
        .map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            let modified = entry
                .metadata()
                .and_then(|meta| meta.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            (name, modified)
        })
        .collect();
    pick_latest_log(&names).map(|name| dir.join(name))
}

#[cfg(test)]
#[path = "../../tests-local/malody4_gamelog.rs"]
mod tests;
