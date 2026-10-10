// app::builder - 应用程序生命周期流水线构建器

use tauri::Manager;
use crate::config;
use crate::server;
use crate::settings_window;
use super::shortcuts::register_global_shortcuts;
use super::url::startup_url;
use super::window_events::handle_window_event;
use super::window_state::{spawn_window_state_watchdog, WINDOW_STATE};

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
                    let saved = config::read_window_state();
                    *WINDOW_STATE.lock().unwrap() = saved;
                    if saved.x != i32::MIN {
                        let _ = window.set_position(tauri::PhysicalPosition::new(saved.x, saved.y));
                    }
                    let _ = window.set_size(tauri::PhysicalSize::new(saved.w, saved.h));
                    let _ = window.set_always_on_top(saved.topmost);
                    if saved.click_through {
                        let _ = window.set_ignore_cursor_events(true);
                    }
                    if let Ok(url) = url_text.parse() {
                        let _ = window.navigate(url);
                    }
                }

                if open_settings_on_start {
                    settings_window::open_or_focus(app.handle(), &plugin_dir);
                }

                register_global_shortcuts(app.handle(), plugin_dir);
                spawn_window_state_watchdog(app.handle());

                Ok(())
            })
            .on_window_event(|window, event| {
                handle_window_event(window, event);
            })
            .run(tauri::generate_context!())
            .expect("error while running tauri application");
    }
}
