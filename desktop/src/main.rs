// mma-shell 桌面壳入口：
// 在线（tosu.env 命中且存活）→ 导航到 tosu 插件页（设置/静态零适配）；
// 离线 → 24061 本地页。窗口属性（透明/置顶/无边框）由 tauri.conf.json 配置。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use mma_shell::{config, server};
use tauri::Manager;
use tauri_plugin_global_shortcut::{
    Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState,
};
use std::sync::Mutex;

// 窗口标志内存态（tauri 无 ignore_cursor_events getter）：(topmost, click_through)。
static WINDOW_FLAGS: Mutex<(bool, bool)> = Mutex::new((true, false));

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

/// 保存窗口状态（位置/尺寸/置顶/穿透）——关闭与状态切换时调用。
fn save_window_state(app: &tauri::AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let Ok(position) = window.outer_position() else {
        return;
    };
    let Ok(size) = window.outer_size() else {
        return;
    };
    let state = config::WindowState {
        x: position.x,
        y: position.y,
        w: size.width,
        h: size.height,
        topmost: WINDOW_FLAGS.lock().unwrap().0,
        click_through: WINDOW_FLAGS.lock().unwrap().1,
    };
    config::write_window_state(&state);
}

fn toggle_topmost(app: &tauri::AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let next = !WINDOW_FLAGS.lock().unwrap().0;
    *WINDOW_FLAGS.lock().unwrap() = (next, WINDOW_FLAGS.lock().unwrap().1);
    let _ = window.set_always_on_top(next);
    save_window_state(app);
}

fn toggle_click_through(app: &tauri::AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let next = !WINDOW_FLAGS.lock().unwrap().1;
    *WINDOW_FLAGS.lock().unwrap() = (WINDOW_FLAGS.lock().unwrap().0, next);
    // 穿透开启时窗口无鼠标焦点，靠全局快捷键切回。
    let _ = window.set_ignore_cursor_events(next);
    save_window_state(app);
}

fn main() {
    let plugin_dir = config::plugin_dir();
    let tosu = config::probe_tosu_env();
    let url_text = startup_url(&tosu);
    println!("plugin dir: {}", plugin_dir.display());
    println!("startup url: {}", url_text);

    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(move |app| {
            let shared = server::start(plugin_dir, tosu);
            if let Some(window) = app.get_webview_window("main") {
                server::set_main_window(&shared, window.clone());
                // 恢复记忆的窗口状态（位置/尺寸/置顶/穿透）。
                let saved = config::read_window_state();
                if saved.x != i32::MIN {
                    let _ = window.set_position(tauri::LogicalPosition::new(saved.x, saved.y));
                }
                let _ = window.set_size(tauri::LogicalSize::new(saved.w, saved.h));
                let _ = window.set_always_on_top(saved.topmost);
                *WINDOW_FLAGS.lock().unwrap() = (saved.topmost, saved.click_through);
                if saved.click_through {
                    let _ = window.set_ignore_cursor_events(true);
                }
                let url = url_text.parse().expect("invalid startup url");
                let _ = window.navigate(url);
            }

            // 全局快捷键（页面焦点/点击穿透无关）：Ctrl+Shift+T 置顶、Ctrl+Shift+C 穿透、Ctrl+Q 关闭。
            let _ = app.global_shortcut().on_shortcut(
                Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyT),
                move |app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        toggle_topmost(app);
                    }
                },
            );
            let _ = app.global_shortcut().on_shortcut(
                Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyC),
                move |app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        toggle_click_through(app);
                    }
                },
            );
            let _ = app.global_shortcut().on_shortcut(
                Shortcut::new(Some(Modifiers::CONTROL), Code::KeyQ),
                move |app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        if let Some(window) = app.get_webview_window("main") {
                            save_window_state(app);
                            let _ = window.close();
                        }
                    }
                },
            );
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                save_window_state(window.app_handle());
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}