// app::shortcuts - 全局快捷键注册与调度

use std::path::PathBuf;
use tauri::Manager;
use tauri_plugin_global_shortcut::{
    Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState,
};
use crate::config;
use crate::server;
use crate::settings_window;
use super::window_state::{apply_flag_change, persist_window_state};

/// 解析快捷键字符串（例如 "Ctrl+Shift+T"）。
pub fn parse_shortcut(s: &str) -> Option<Shortcut> {
    let mut mods = Modifiers::empty();
    let mut key: Option<Code> = None;
    for part in s.split('+') {
        match part.trim() {
            "Ctrl" | "CTRL" | "ctrl" => mods |= Modifiers::CONTROL,
            "Shift" | "SHIFT" | "shift" => mods |= Modifiers::SHIFT,
            "Alt" | "ALT" | "alt" => mods |= Modifiers::ALT,
            "Super" | "Win" | "Meta" | "CMD" => mods |= Modifiers::SUPER,
            other => {
                key = match other {
                    "T" => Some(Code::KeyT),
                    "C" => Some(Code::KeyC),
                    "Q" => Some(Code::KeyQ),
                    "A" => Some(Code::KeyA),
                    "B" => Some(Code::KeyB),
                    "D" => Some(Code::KeyD),
                    "E" => Some(Code::KeyE),
                    "F" => Some(Code::KeyF),
                    "G" => Some(Code::KeyG),
                    "H" => Some(Code::KeyH),
                    "I" => Some(Code::KeyI),
                    "J" => Some(Code::KeyJ),
                    "K" => Some(Code::KeyK),
                    "L" => Some(Code::KeyL),
                    "M" => Some(Code::KeyM),
                    "N" => Some(Code::KeyN),
                    "O" => Some(Code::KeyO),
                    "P" => Some(Code::KeyP),
                    "R" => Some(Code::KeyR),
                    "S" => Some(Code::KeyS),
                    "U" => Some(Code::KeyU),
                    "V" => Some(Code::KeyV),
                    "W" => Some(Code::KeyW),
                    "X" => Some(Code::KeyX),
                    "Y" => Some(Code::KeyY),
                    "Z" => Some(Code::KeyZ),
                    _ => None,
                };
            }
        }
    }
    key.map(|k| Shortcut::new(if mods.is_empty() { None } else { Some(mods) }, k))
}

/// 注册全局快捷键（置顶、穿透、设置、关闭）。
pub fn register_global_shortcuts(app: &tauri::AppHandle, plugin_dir: PathBuf) {
    let cfg = config::read_shell_config();
    let hot = |k: &str, def: &str| {
        cfg.get("hotkeys")
            .and_then(|h| h.get(k))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| def.to_string())
    };

    let h_top = hot("topmost", "Ctrl+Shift+T");
    let h_click = hot("clickThrough", "Ctrl+Shift+C");
    let h_settings = hot("settings", "Ctrl+Shift+S");
    let h_close = hot("close", "Ctrl+Q");

    if let Some(sc) = parse_shortcut(&h_top) {
        let _ = app.global_shortcut().on_shortcut(sc, move |app, _sc, event| {
            if event.state() == ShortcutState::Pressed {
                if settings_window::is_open() {
                    server::log::log_at("debug", "shortcut ignored: settings window is open (topmost)");
                    return;
                }
                server::log::log_at("debug", "shortcut pressed: topmost");
                let next = !config::read_window_state().topmost;
                apply_flag_change(app, Some(next), None);
            }
        });
    }

    if let Some(sc) = parse_shortcut(&h_click) {
        let _ = app.global_shortcut().on_shortcut(sc, move |app, _sc, event| {
            if event.state() == ShortcutState::Pressed {
                if settings_window::is_open() {
                    server::log::log_at("debug", "shortcut ignored: settings window is open (clickThrough)");
                    return;
                }
                server::log::log_at("debug", "shortcut pressed: clickThrough");
                let next = !config::read_window_state().click_through;
                apply_flag_change(app, None, Some(next));
            }
        });
    }

    if let Some(sc) = parse_shortcut(&h_settings) {
        let p_dir = plugin_dir.clone();
        let _ = app.global_shortcut().on_shortcut(sc, move |app, _sc, event| {
            if event.state() == ShortcutState::Pressed {
                server::log::log_at("debug", "shortcut pressed: settings");
                settings_window::open_or_focus(app, &p_dir);
            }
        });
    }

    if let Some(sc) = parse_shortcut(&h_close) {
        let _ = app.global_shortcut().on_shortcut(sc, move |app, _sc, event| {
            if event.state() == ShortcutState::Pressed {
                server::log::log_at("debug", "shortcut pressed: close");
                let app2 = app.clone();
                let _ = app.run_on_main_thread(move || {
                    persist_window_state();
                    if let Some(window) = app2.get_webview_window("main") {
                        let _ = window.close();
                    }
                });
            }
        });
    }
}
