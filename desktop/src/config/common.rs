// config::common - 基础路径工具与时间戳读取

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// 路径归一化：用户可能写 `D:\Games\Etterna`（json 里反斜杠需转义，但容错
/// 处理 `\` 与 `/` 混用），统一转 `/` 并去掉尾部斜杠。
pub fn normalize_path(input: &str) -> String {
    input.trim().replace('\\', "/").trim_end_matches('/').to_string()
}

/// exe 所在目录。
pub fn exe_dir() -> Option<PathBuf> {
    env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
}

/// 文件 mtime（不存在/读不到 → `None`）：定时器用 `stat → read → stat` 识别
/// "读取期间文件又被写入"的 tick（两次 mtime 不同则跳过本轮推送，避免推旧内容）。
pub fn file_mtime(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).ok()?.modified().ok()
}
