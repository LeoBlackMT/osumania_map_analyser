// app::builder - 应用程序生命周期流水线构建器

use std::sync::atomic::Ordering;
use tauri::Manager;
use crate::config;
use crate::server;
use crate::settings_window;
use super::shortcuts::register_global_shortcuts;
use super::url::startup_url;
use super::window_events::{handle_window_event, APP_INITIALIZED};
use super::window_state::{spawn_window_state_watchdog, WINDOW_STATE};

/// 检查窗口矩形是否与任意可用显示器有重叠（防止断开显示器或分辨率改变后窗口落在虚空中）。
pub fn is_window_visible_on_any_monitor(
    window: &tauri::WebviewWindow,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
) -> bool {
    let Ok(monitors) = window.available_monitors() else {
        return true;
    };
    if monitors.is_empty() {
        return true;
    }
    let win_r = x.saturating_add(w as i32);
    let win_b = y.saturating_add(h as i32);

    for m in monitors {
        let m_pos = m.position();
        let m_size = m.size();
        let m_r = m_pos.x.saturating_add(m_size.width as i32);
        let m_b = m_pos.y.saturating_add(m_size.height as i32);

        let overlap_l = x.max(m_pos.x);
        let overlap_t = y.max(m_pos.y);
        let overlap_r = win_r.min(m_r);
        let overlap_b = win_b.min(m_b);

        // 要求至少与任一显示器有 50x50 像素的可视/可拖动交集
        if overlap_r >= overlap_l + 50 && overlap_b >= overlap_t + 50 {
            return true;
        }
    }
    false
}

pub struct AppBuilder;

impl AppBuilder {
    pub fn new() -> Self {
        Self
    }

    pub fn build_and_run(self) {
        let plugin_dir = config::plugin_dir();
        let tosu = config::probe_tosu_env();
        let online = tosu.as_ref().map(config::tosu_online).unwrap_or(false);
        let shell_config = config::read_shell_config();
        let url_text = startup_url(&tosu, &shell_config);
        let open_settings_on_start = std::env::args().any(|arg| arg == "--settings");

        println!("plugin dir: {}", plugin_dir.display());
        println!("startup url: {}", url_text);

        tauri::Builder::default()
            .plugin(tauri_plugin_global_shortcut::Builder::new().build())
            .setup(move |app| {
                let plugin_version = std::fs::read_to_string(plugin_dir.join("metadata.txt"))
                    .ok()
                    .and_then(|text| {
                        text.lines().find_map(|line| {
                            line.trim().strip_prefix("Version:").map(|v| v.trim().to_string())
                        })
                    })
                    .unwrap_or_else(|| "?".to_string());

                server::log::log_line(&format!(
                    "mma-shell start (crate v{}, plugin v{}, ts={})",
                    env!("CARGO_PKG_VERSION"),
                    plugin_version,
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0)
                ));

                config::ensure_shell_config();
                config::ensure_plugin_settings(&tosu, online);

                let shared = server::start(plugin_dir.clone(), tosu);
                server::set_app_handle(&shared, app.handle().clone());

                if let Some(window) = app.get_webview_window("main") {
                    server::set_main_window(&shared, window.clone());
                    let mut saved = config::read_window_state();
                    saved.sanitize();

                    let is_pos_valid = config::is_valid_position(saved.x, saved.y) && saved.x != i32::MIN;
                    let is_visible = is_pos_valid && is_window_visible_on_any_monitor(&window, saved.x, saved.y, saved.w, saved.h);

                    if is_visible {
                        let _ = window.set_position(tauri::PhysicalPosition::new(saved.x, saved.y));
                    } else {
                        let _ = window.center();
                        if let Ok(pos) = window.outer_position() {
                            if config::is_valid_position(pos.x, pos.y) {
                                saved.x = pos.x;
                                saved.y = pos.y;
                            }
                        }
                    }
                    let _ = window.set_size(tauri::PhysicalSize::new(saved.w, saved.h));
                    *WINDOW_STATE.lock().unwrap() = saved.clone();
                    let _ = window.set_always_on_top(saved.topmost);
                    if saved.click_through {
                        let _ = window.set_ignore_cursor_events(true);
                    }
                    if let Ok(url) = url_text.parse() {
                        let _ = window.navigate(url);
                    }
                    // 开发用：`--features devtools` 构建 + `MMA_DEVTOOLS=1` 启动时打开
                    // WebView 控制台（壳窗口默认没有 F12/右键菜单）。普通 release 包
                    // 不含 devtools 特性，这段代码不会编译进去。
                    #[cfg(feature = "devtools")]
                    if std::env::var("MMA_DEVTOOLS").map(|value| value == "1").unwrap_or(false) {
                        window.open_devtools();
                    }
                }

                if open_settings_on_start {
                    settings_window::open_or_focus(app.handle(), &plugin_dir);
                }

                register_global_shortcuts(app.handle(), plugin_dir);
                spawn_window_state_watchdog(app.handle());

                // setup 完成：激活事件监听，彻底消除建窗时的事件竞争
                APP_INITIALIZED.store(true, Ordering::Release);

                Ok(())
            })
            .on_window_event(|window, event| {
                handle_window_event(window, event);
            })
            .run(tauri::generate_context!())
            .expect("error while running tauri application");
    }
}
