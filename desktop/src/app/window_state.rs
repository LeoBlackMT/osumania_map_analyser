// app::window_state - 窗口几何状态内存缓存与双向磁盘持久化

use std::sync::Mutex;
use std::thread;
use std::time::Duration;
use tauri::Manager;
use crate::config;

// 窗口状态内存缓存（位置/尺寸由窗口事件维护；标志位由快捷键切换）。
pub static WINDOW_STATE: Mutex<config::WindowState> = Mutex::new(config::WindowState {
    x: i32::MIN,
    y: 0,
    w: 520,
    h: 680,
    topmost: true,
    click_through: false,
    settings: None,
});

/// 把当前内存状态写盘（仅文件 IO，安全；任何线程可调）。
pub fn persist_window_state() {
    config::write_window_state(&WINDOW_STATE.lock().unwrap());
}

/// 位置/尺寸事件后持久化：置顶/穿透以磁盘为权威（control toggle 可能刚改过）。
pub fn merge_disk_flags_and_persist() {
    let disk = config::read_window_state();
    {
        let mut st = WINDOW_STATE.lock().unwrap();
        st.topmost = disk.topmost;
        st.click_through = disk.click_through;
    }
    persist_window_state();
}

/// 切换置顶/穿透：入参为目标值；应用窗口 API 并写盘。
pub fn apply_flag_change(app: &tauri::AppHandle, topmost: Option<bool>, click_through: Option<bool>) {
    {
        let mut st = WINDOW_STATE.lock().unwrap();
        let disk = config::read_window_state();
        st.topmost = topmost.unwrap_or(disk.topmost);
        st.click_through = click_through.unwrap_or(disk.click_through);
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

/// 窗口状态兜底看门狗：主线程每 5s 查询一次位置/尺寸并写盘。
pub fn spawn_window_state_watchdog(app: &tauri::AppHandle) {
    let handle = app.clone();
    thread::spawn(move || loop {
        thread::sleep(Duration::from_secs(5));
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
            let disk = config::read_window_state();
            {
                let mut st = WINDOW_STATE.lock().unwrap();
                st.topmost = disk.topmost;
                st.click_through = disk.click_through;
            }
            persist_window_state();
        });
    });
}
