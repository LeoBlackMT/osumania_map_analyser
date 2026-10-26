// app::window_events - Tauri 窗口事件集中分发与几何同步

use std::sync::atomic::{AtomicBool, Ordering};
use tauri::Manager;
use crate::config;
use crate::settings_window;
use super::window_state::{flush_window_state, mark_geometry_dirty, WINDOW_STATE};

/// 启动初始化守卫：在 setup 完成窗口初次几何恢复前，丢弃原生窗口创建时
/// 操作系统派发的初始默认位置/尺寸事件，彻底根除启动事件竞争（Race Condition）。
pub static APP_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// 处理 Tauri 窗口生命周期与几何变动事件。
pub fn handle_window_event(window: &tauri::Window, event: &tauri::WindowEvent) {
    match window.label() {
        "main" => match event {
            tauri::WindowEvent::Moved(position) => {
                if !APP_INITIALIZED.load(Ordering::Acquire) {
                    return;
                }
                // 守卫 1：最小化时绝对不采样坐标（Win+D 会发送 -32000, -32000）
                if window.is_minimized().unwrap_or(false) {
                    return;
                }
                // 守卫 2：过滤非法坐标（防守性编程）
                if !config::is_valid_position(position.x, position.y) {
                    return;
                }
                WINDOW_STATE.lock().unwrap().x = position.x;
                WINDOW_STATE.lock().unwrap().y = position.y;
                mark_geometry_dirty();
            }
            tauri::WindowEvent::Resized(size) => {
                if !APP_INITIALIZED.load(Ordering::Acquire) {
                    return;
                }
                // 守卫 1：最小化时绝对不采样尺寸（Win+D 会发送 0, 0）
                if window.is_minimized().unwrap_or(false) {
                    return;
                }
                // 守卫 2：过滤非法尺寸（主窗最小尺寸守卫）
                if !config::is_valid_main_window_size(size.width, size.height) {
                    return;
                }
                WINDOW_STATE.lock().unwrap().w = size.width;
                WINDOW_STATE.lock().unwrap().h = size.height;
                mark_geometry_dirty();
            }
            tauri::WindowEvent::CloseRequested { .. } | tauri::WindowEvent::Destroyed => {
                // 如果退出时处于最小化状态，严禁将最小化状态写盘！
                if !window.is_minimized().unwrap_or(false) {
                    flush_window_state();
                }
                settings_window::close(window.app_handle());
                std::process::exit(0);
            }
            _ => {}
        },
        settings_window::LABEL => match event {
            tauri::WindowEvent::Moved(position) => {
                if window.is_minimized().unwrap_or(false) {
                    return;
                }
                if !config::is_valid_position(position.x, position.y) {
                    return;
                }
                if let Ok(mut st) = WINDOW_STATE.lock() {
                    let mut settings = st.settings.unwrap_or_else(config::read_settings_window_state);
                    settings.x = position.x;
                    settings.y = position.y;
                    st.settings = Some(settings);
                }
                mark_geometry_dirty();
            }
            tauri::WindowEvent::Resized(size) => {
                if window.is_minimized().unwrap_or(false) {
                    return;
                }
                if !config::is_valid_settings_window_size(size.width, size.height) {
                    return;
                }
                if let Ok(mut st) = WINDOW_STATE.lock() {
                    let mut settings = st.settings.unwrap_or_else(config::read_settings_window_state);
                    settings.w = size.width;
                    settings.h = size.height;
                    st.settings = Some(settings);
                }
                mark_geometry_dirty();
            }
            tauri::WindowEvent::CloseRequested { .. } | tauri::WindowEvent::Destroyed => {
                if !window.is_minimized().unwrap_or(false) {
                    flush_window_state();
                }
                settings_window::on_window_closed(window.app_handle());
            }
            _ => {}
        },
        _ => {}
    }
}
