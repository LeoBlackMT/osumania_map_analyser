// mma-shell 桌面壳入口：
// 在线（tosu.env 命中且存活）→ 导航到 tosu 插件页（设置/静态零适配）；
// 离线 → 24061 本地页。窗口属性（透明/置顶/无边框）由 tauri.conf.json 配置。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use mma_shell::{config, server};
use tauri::Manager;

fn startup_url(tosu: &Option<config::TosuInfo>) -> String {
    let Some(info) = tosu else {
        return "http://127.0.0.1:24061/".to_string();
    };
    if !config::tosu_online(info) {
        return "http://127.0.0.1:24061/".to_string();
    }
    let encoded = config::PLUGIN_FOLDER.replace(' ', "%20");
    format!("{}/{}/", info.base_url(), encoded)
}

fn main() {
    let plugin_dir = config::plugin_dir();
    let tosu = config::probe_tosu_env();
    let url_text = startup_url(&tosu);
    println!("plugin dir: {}", plugin_dir.display());
    println!("startup url: {}", url_text);

    tauri::Builder::default()
        .setup(move |app| {
            let shared = server::start(plugin_dir, tosu);
            if let Some(window) = app.get_webview_window("main") {
                server::set_main_window(&shared, window.clone());
                let url = url_text.parse().expect("invalid startup url");
                let _ = window.navigate(url);
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}