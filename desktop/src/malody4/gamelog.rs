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
mod tests {
    use super::*;

    fn tracker_after(lines: &[&str]) -> SceneTracker {
        let mut tracker = SceneTracker::new();
        for line in lines {
            tracker.apply_line(line);
        }
        tracker
    }

    #[test]
    fn before_any_scene_line_screen_is_unknown_and_other() {
        let tracker = SceneTracker::new();
        assert_eq!(tracker.screen(), Screen::Other);
        assert!(!tracker.screen_known());
        assert_eq!(tracker.last_change(), None);
        assert!(!tracker.fresh(Duration::from_secs(10)));
    }

    #[test]
    fn scene_lines_map_to_screens() {
        let selection = tracker_after(&["1.000 [MS] LOG: switch to 1"]);
        assert_eq!(selection.screen(), Screen::Selection);
        assert!(selection.screen_known());

        let selection_two = tracker_after(&["2.000 [MS] LOG: switch to 2"]);
        assert_eq!(selection_two.screen(), Screen::Selection);

        let playing = tracker_after(&["3.000 [MS] LOG: switch to 3"]);
        assert_eq!(playing.screen(), Screen::Playing);
        assert!(playing.fresh(Duration::from_secs(10)));
        assert!(playing.last_change().is_some());

        let result = tracker_after(&["4.000 [MS] LOG: switch to 4"]);
        assert_eq!(result.screen(), Screen::Result);
        assert!(!result.fresh(Duration::from_secs(10)));

        let other_seven = tracker_after(&["5.000 [MS] LOG: switch to 7"]);
        assert_eq!(other_seven.screen(), Screen::Other);
        assert!(other_seven.screen_known());
        assert_eq!(other_seven.last_change(), None);

        let other_eight = tracker_after(&["6.000 [MS] LOG: switch to 8"]);
        assert_eq!(other_eight.screen(), Screen::Other);

        let unknown = tracker_after(&["7.000 [MS] LOG: switch to 42"]);
        assert_eq!(unknown.screen(), Screen::Other);
        assert!(unknown.screen_known());
    }

    #[test]
    fn scene_transition_sequence_updates_last_change_only_on_change() {
        let mut tracker = SceneTracker::new();
        tracker.apply_line("10.000 [MS] LOG: switch to 2");
        let after_selection = tracker.last_change().expect("scene change sets last_change");
        tracker.apply_line("10.500 [MS] LOG: switch to 2");
        assert_eq!(
            tracker.last_change(),
            Some(after_selection),
            "同一场景重复出现不得刷新 last_change"
        );
        std::thread::sleep(Duration::from_millis(2));
        tracker.apply_line("11.000 [MS] LOG: switch to 3");
        assert_ne!(tracker.last_change(), Some(after_selection));
        assert_eq!(tracker.screen(), Screen::Playing);
    }

    #[test]
    fn switchback_and_switch_back_are_ignored() {
        let mut tracker = SceneTracker::new();
        tracker.apply_line("20.000 [MS] LOG: switch to 3");
        let playing = tracker.last_change();
        tracker.apply_line("21.000 [MS] LOG: switchback");
        tracker.apply_line("22.000 [MS] LOG: switch back");
        tracker.apply_line("23.000 [MS] LOG: switchback to 2");
        tracker.apply_line("24.000 [MS] LOG: switch back to 2");
        assert_eq!(tracker.screen(), Screen::Playing);
        assert_eq!(tracker.last_change(), playing);
    }

    #[test]
    fn switchbefore_is_a_pre_announcement_and_changes_nothing() {
        let mut tracker = SceneTracker::new();
        tracker.apply_line("30.000 [MS] LOG: switch to 4");
        let before_change = tracker.last_change();
        tracker.apply_line("31.000 [MS] LOG: switchbefore to 1");
        assert_eq!(tracker.screen(), Screen::Result);
        assert_eq!(tracker.last_change(), before_change);
    }

    #[test]
    fn switchbefore_before_any_scene_does_not_mark_screen_known() {
        let tracker = tracker_after(&["40.000 [MS] LOG: switchbefore to 1"]);
        assert_eq!(tracker.screen(), Screen::Other);
        assert!(!tracker.screen_known());
        assert_eq!(tracker.last_change(), None);
    }

    #[test]
    fn lines_without_the_ms_marker_are_ignored() {
        let tracker = tracker_after(&[
            "50.000 LOG: switch to 3",
            "51.000 [ms] LOG: switch to 3",
            "[MS] LOG: switch to",
            "[MS] LOG: switch to ",
        ]);
        assert_eq!(tracker.screen(), Screen::Other);
        assert!(!tracker.screen_known());
    }

    #[test]
    fn pick_latest_log_filters_and_picks_lexicographic_max() {
        let entries = vec![
            ("log-20260921T171305+0800.txt".to_string(), SystemTime::UNIX_EPOCH),
            ("log-20260920T101010+0800.txt".to_string(), SystemTime::UNIX_EPOCH),
            ("log-20260922T090000+0800.txt".to_string(), SystemTime::UNIX_EPOCH),
            ("other.txt".to_string(), SystemTime::UNIX_EPOCH),
            ("log-20260923T000000+0800.dmp".to_string(), SystemTime::UNIX_EPOCH),
            ("log-notatxt".to_string(), SystemTime::UNIX_EPOCH),
        ];
        assert_eq!(
            pick_latest_log(&entries),
            Some("log-20260922T090000+0800.txt".to_string())
        );
    }

    #[test]
    fn pick_latest_log_returns_none_without_candidates() {
        assert_eq!(pick_latest_log(&[]), None);
        let only_junk = vec![
            ("log-1.txt.bak".to_string(), SystemTime::UNIX_EPOCH),
            ("xlog-1.txt".to_string(), SystemTime::UNIX_EPOCH),
        ];
        assert_eq!(pick_latest_log(&only_junk), None);
    }

    fn tmp_dir(tag: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "mma-malody4-log-{}-{}-{}",
            tag,
            std::process::id(),
            stamp
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn latest_log_picks_lexicographically_greatest_log_txt_and_ignores_other_files() {
        let dir = tmp_dir("pick");
        for name in [
            "log-20260921T171305+0800.txt",
            "log-20260920T101010+0800.txt",
            "log-20260922T090000+0800.txt",
            "other.txt",
            "log-20260923T000000+0800.dmp",
            "log-notatxt",
        ] {
            std::fs::write(dir.join(name), "0.000 [MS] LOG: switch to 1\n").unwrap();
        }

        let picked = latest_log(&dir).expect("必须选出 log-*.txt");
        assert_eq!(picked, dir.join("log-20260922T090000+0800.txt"));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn latest_log_on_missing_or_empty_dir_is_none() {
        let missing = std::env::temp_dir().join(format!(
            "mma-malody4-log-missing-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        assert_eq!(latest_log(&missing), None);

        let empty = tmp_dir("empty");
        assert_eq!(latest_log(&empty), None);
        std::fs::remove_dir_all(&empty).unwrap();
    }
}
