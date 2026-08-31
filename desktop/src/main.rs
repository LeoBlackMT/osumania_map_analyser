// mma-shell 桌面壳入口：
// 在线（tosu.env 命中且存活）→ 导航到 tosu 插件页（设置/静态零适配）；
// 离线 → 24061 本地页。窗口属性（透明/置顶/无边框）由 tauri.conf.json 配置。
//
// 死锁规避（经验教训）：全局快捷键回调与 on_window_event 都运行在 Tauri 主
// 线程——在回调里同步调用窗口 API（outer_position/set_always_on_top 等）会
// 与事件循环互相等待（自死锁→整个应用无响应）。因此：
//   * 窗口 API 调用一律包进 run_on_main_thread（异步派发）；
//   * 位置/尺寸从不主动查询，由 Moved/Resized 事件维护缓存，关闭时仅写盘。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use mma_shell::{config, server};
use tauri::Manager;
use tauri_plugin_global_shortcut::{
    Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState,
};
use std::sync::Mutex;
use std::thread;

// 窗口状态内存缓存（位置/尺寸由窗口事件维护；标志位由快捷键切换）。
static WINDOW_STATE: Mutex<config::WindowState> = Mutex::new(config::WindowState {
    x: i32::MIN,
    y: 0,
    w: 520,
    h: 680,
    topmost: true,
    click_through: false,
});

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

/// 把当前内存状态写盘（仅文件 IO，安全；任何线程可调）。
fn persist_window_state() {
    config::write_window_state(&WINDOW_STATE.lock().unwrap());
}

/// 切换置顶/穿透：仅在内存改标志并异步应用窗口 API。
fn apply_flag_change(app: &tauri::AppHandle, topmost: Option<bool>, click_through: Option<bool>) {
    {
        let mut st = WINDOW_STATE.lock().unwrap();
        if let Some(v) = topmost {
            st.topmost = v;
        }
        if let Some(v) = click_through {
            st.click_through = v;
        }
    }
    persist_window_state();
    let app2 = app.clone();
    let _ = app.run_on_main_thread(move || {
        let Some(window) = app2.get_webview_window("main") else {
            return;
        };
        if let Some(v) = topmost {
            let _ = window.set_always_on_top(v);
        }
        if let Some(v) = click_through {
            let _ = window.set_ignore_cursor_events(v);
        }
    });
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
            // 启动时生成 mma-shell.json 骨架（无 tosu 用户可发现并编辑）。
            config::ensure_shell_config();

            let shared = server::start(plugin_dir, tosu);
            if let Some(window) = app.get_webview_window("main") {
                server::set_main_window(&shared, window.clone());
                // 恢复记忆的窗口状态（位置/尺寸/置顶/穿透）——setup 内同步调用安全。
                // 注意：存取均为 physical 坐标（outer_position/outer_size），
                // 恢复必须用 Physical* 构造，否则与 logical 混用会在高 DPI
                // 下逐次漂移放大。
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
                let url = url_text.parse().expect("invalid startup url");
                let _ = window.navigate(url);
            }

            // 全局快捷键（页面焦点/点击穿透无关）。回调只改内存+异步应用窗口 API，
            // 绝不在回调内同步调用窗口方法（自死锁见文件头注）。
            let _ = app.global_shortcut().on_shortcut(
                Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyT),
                move |app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        let next = !WINDOW_STATE.lock().unwrap().topmost;
                        apply_flag_change(app, Some(next), None);
                    }
                },
            );
            let _ = app.global_shortcut().on_shortcut(
                Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyC),
                move |app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        let next = !WINDOW_STATE.lock().unwrap().click_through;
                        apply_flag_change(app, None, Some(next));
                    }
                },
            );
            let _ = app.global_shortcut().on_shortcut(
                Shortcut::new(Some(Modifiers::CONTROL), Code::KeyQ),
                move |app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        let app2 = app.clone();
                        let _ = app.run_on_main_thread(move || {
                            persist_window_state();
                            if let Some(window) = app2.get_webview_window("main") {
                                let _ = window.close();
                            }
                        });
                    }
                },
            );
            // 窗口状态兜底：主线程每 5s 查询一次位置/尺寸并写盘（部分透明窗口
            // Moved/Resized 事件可能不触发；查询必须经 run_on_main_thread）。
            {
                let handle = app.handle().clone();
                thread::spawn(move || loop {
                    thread::sleep(std::time::Duration::from_secs(5));
                    let handle2 = handle.clone();
                    let _ = handle.run_on_main_thread(move || {
                        let Some(window) = handle2.get_webview_window("main") else {
                            return;
                        };
                        if let Ok(position) = window.outer_position() {
                            WINDOW_STATE.lock().unwrap().x = position.x;
                            WINDOW_STATE.lock().unwrap().y = position.y;
                        }
                        if let Ok(size) = window.outer_size() {
                            WINDOW_STATE.lock().unwrap().w = size.width;
                            WINDOW_STATE.lock().unwrap().h = size.height;
                        }
                        persist_window_state();
                    });
                });
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            // 事件回调在主线程：只更新内存缓存（数字），不调用任何窗口 API。
            match event {
                tauri::WindowEvent::Moved(position) => {
                    WINDOW_STATE.lock().unwrap().x = position.x;
                    WINDOW_STATE.lock().unwrap().y = position.y;
                    persist_window_state();
                }
                tauri::WindowEvent::Resized(size) => {
                    WINDOW_STATE.lock().unwrap().w = size.width;
                    WINDOW_STATE.lock().unwrap().h = size.height;
                    persist_window_state();
                }
                tauri::WindowEvent::CloseRequested { .. } => {
                    persist_window_state();
                }
                _ => {}
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}