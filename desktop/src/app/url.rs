// app::url - 启动主窗口引导 URL 计算

use crate::config;
use crate::server;

/// 启动 URL（契约 v6，修 DEC-21 的缺陷②）。
pub fn startup_url(tosu: &Option<config::TosuInfo>, shell_config: &serde_json::Value) -> String {
    if !server::osu_source::forced_tosu(shell_config) {
        return "http://127.0.0.1:24061/".to_string();
    }
    let Some(info) = tosu else {
        return "http://127.0.0.1:24061/".to_string();
    };
    if !config::tosu_online(info) {
        return "http://127.0.0.1:24061/".to_string();
    }
    let folder = config::plugin_dir()
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| config::PLUGIN_FOLDER.to_string());
    let encoded = folder.replace(' ', "%20");
    format!("{}/{}/", info.base_url(), encoded)
}
