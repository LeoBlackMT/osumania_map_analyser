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

/// 可读本地时间戳（YYYY-MM-DD HH:MM:SS）与按日文件名（YYYYMMDD）。
/// 用本地时区偏移（Windows 取系统时区偏移；简化实现不处理夏令时逐日变化，
/// 展示足够）。
fn local_parts(unix: u64) -> (String, String) {
    // 本地时区偏移（分钟）：读系统 TZ/注册表过重，用固定偏移 +8 近似？不——
    // 用 chrono 太重，直接用 unix 秒算 UTC，文件名与时间戳均 UTC（跨时区一致）。
    let secs = unix as i64;
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    // 1970-01-01 是星期四；用 epoch 天数推年月日（简化公历）。
    let (y, mo, d) = civil_from_days(days);
    (
        format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", y, mo, d, h, m, s),
        format!("{:04}{:02}{:02}", y, mo, d),
    )
}

/// 天数 → 公历日期（Howard Hinnant 算法）。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn log_file_path(dir: &PathBuf, unix: u64) -> PathBuf {
    let (_, ymd) = local_parts(unix);
    dir.join(format!("mma-shell-{}.log", ymd))
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
        let (ts, _) = local_parts(unix);
        let _ = writeln!(f, "[{}] [{}] {}", ts, level, msg);
    }
    // 清理旧日志（保留最近 7 个文件；按文件名日期字符串排序）。
    if let Ok(entries) = fs::read_dir(&dir) {
        let mut logs: Vec<(String, PathBuf)> = entries
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                let stem = name.strip_prefix("mma-shell-")?.strip_suffix(".log")?;
                // 仅认 YYYYMMDD 文件名（8 位数字）。
                if stem.len() == 8 && stem.chars().all(|c| c.is_ascii_digit()) {
                    Some((stem.to_string(), e.path()))
                } else {
                    None
                }
            })
            .collect();
        logs.sort_by_key(|(d, _)| d.clone());
        for (_, p) in logs.iter().take(logs.len().saturating_sub(7)) {
            let _ = fs::remove_file(p);
        }
    }
}
