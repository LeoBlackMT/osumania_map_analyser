// app::url - 启动主窗口引导 URL 计算

use crate::config;
use crate::server;

/// 壳窗口标记（与页面 `feedbackSpec.SHELL_MARKER` 一致）。
///
/// tosu 在线时页面由 tosu 提供（端口 24050），页面无法只靠端口判断"我在壳里"；
/// 用 hash 标记：不会被发送到服务端，也不影响 `location.search`/相对路径。
const SHELL_MARKER: &str = "mma-shell";

fn shell_url(base: String) -> String {
    format!("{base}#{SHELL_MARKER}")
}

/// 启动 URL（契约 v6，修 DEC-21 的缺陷②）。
pub fn startup_url(tosu: &Option<config::TosuInfo>, shell_config: &serde_json::Value) -> String {
    if !server::osu_source::forced_tosu(shell_config) {
        return shell_url("http://127.0.0.1:24061/".to_string());
    }
    let Some(info) = tosu else {
        return shell_url("http://127.0.0.1:24061/".to_string());
    };
    if !config::tosu_online(info) {
        return shell_url("http://127.0.0.1:24061/".to_string());
    }
    let folder = config::plugin_dir()
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| config::PLUGIN_FOLDER.to_string());
    let encoded = folder.replace(' ', "%20");
    shell_url(format!("{}/{}/", info.base_url(), encoded))
}
