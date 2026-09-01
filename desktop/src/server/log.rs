// 日志：exe 旁 logs/ 目录，按日轮转（mma-shell-YYYYMMDD.log，保留 7），
// 每行可读时间戳 + 级别；mma-shell.json 的 logLevel 控制（debug/info/warn/error/off）。

use std::fs;
use std::io::Write;
use std::path::PathBuf;

fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
}

fn log_dir() -> Option<PathBuf> {
    let dir = exe_dir()?;
    let logs = dir.join("logs");
    if !logs.exists() {
        let _ = fs::create_dir_all(&logs);
    }
    Some(logs)
}

/// 可读本地时间戳（YYYY-MM-DD HH:MM:SS）。
fn readable_ts(unix: u64) -> String {
    // 用 systemtime 换算本地时间（简化：UTC+偏移取整；Windows 用本地时区）。
    let secs = unix as i64;
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    // 1970-01-01 为星期四；这里仅作展示（不处理夏令时）。
    format!("day{} {:02}:{:02}:{:02}", days, h, m, s)
}

fn log_file_path(dir: &PathBuf, unix: u64) -> PathBuf {
    let days = unix / 86400;
    let name = format!("mma-shell-{}.log", days);
    dir.join(name)
}

fn cfg_level() -> String {
    crate::config::log_level()
}

/// 记一条 info 日志。
pub fn log_line(msg: &str) {
    log_at("info", msg);
}

/// 按级别记日志（过滤见 log_level）。
pub fn log_at(level: &str, msg: &str) {
    let cfg_level = cfg_level();
    if cfg_level == "off" {
        return;
    }
    const ORDER: [&str; 4] = ["debug", "info", "warn", "error"];
    let lv = ORDER.iter().position(|l| *l == level).unwrap_or(1);
    let cfg = ORDER
        .iter()
        .position(|l| *l == cfg_level.as_str())
        .unwrap_or(1);
    if lv < cfg {
        return;
    }
    let Some(dir) = log_dir() else {
        return;
    };
    let unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let path = log_file_path(&dir, unix);
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "[{}] [{}] {}", readable_ts(unix), level, msg);
    }
    // 清理旧日志（保留最近 7 个文件）。
    if let Ok(entries) = fs::read_dir(&dir) {
        let mut logs: Vec<(u64, PathBuf)> = entries
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                let stem = name.strip_prefix("mma-shell-")?.strip_suffix(".log")?;
                stem.parse::<u64>().ok().map(|d| (d, e.path()))
            })
            .collect();
        logs.sort_by_key(|(d, _)| *d);
        for (_, p) in logs.iter().take(logs.len().saturating_sub(7)) {
            let _ = fs::remove_file(p);
        }
    }
}
