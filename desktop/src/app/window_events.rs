// app::window_events - Tauri 窗口事件集中分发与几何同步

use tauri::Manager;
use crate::config;
use crate::settings_window;
use super::window_state::{merge_disk_flags_and_persist, WINDOW_STATE};

/// 处理 Tauri 窗口生命周期与几何变动事件。
pub fn handle_window_event(window: &tauri::Window, event: &tauri::WindowEvent) {
    match window.label() {
        "main" => match event {
            tauri::WindowEvent::Moved(position) => {
                WINDOW_STATE.lock().unwrap().x = position.x;
                WINDOW_STATE.lock().unwrap().y = position.y;
                merge_disk_flags_and_persist();
            }
            tauri::WindowEvent::Resized(size) => {
                WINDOW_STATE.lock().unwrap().w = size.width;
                WINDOW_STATE.lock().unwrap().h = size.height;
                merge_disk_flags_and_persist();
            }
            tauri::WindowEvent::CloseRequested { .. } | tauri::WindowEvent::Destroyed => {
                merge_disk_flags_and_persist();
                settings_window::close(window.app_handle());
                std::process::exit(0);
            }
            _ => {}
        },
        settings_window::LABEL => match event {
            tauri::WindowEvent::Moved(position) => {
                let mut state = config::read_settings_window_state();
                state.x = position.x;
                state.y = position.y;
                config::write_settings_window_state(&state);
                if let Ok(mut st) = WINDOW_STATE.lock() {
                    st.settings = Some(state);
                }
            }
            tauri::WindowEvent::Resized(size) => {
                let mut state = config::read_settings_window_state();
                state.w = size.width;
                state.h = size.height;
                config::write_settings_window_state(&state);
                if let Ok(mut st) = WINDOW_STATE.lock() {
                    st.settings = Some(state);
                }
            }
            tauri::WindowEvent::CloseRequested { .. } | tauri::WindowEvent::Destroyed => {
                settings_window::on_window_closed(window.app_handle());
            }
            _ => {}
        },
        _ => {}
    }
}
