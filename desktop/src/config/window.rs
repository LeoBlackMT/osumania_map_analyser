// config::window - 窗口位置、尺寸、穿透与置顶持久化状态

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use super::common::exe_dir;

pub const WINDOW_STATE_FILE: &str = "mma-shell-state.json";
pub const LEGACY_SETTINGS_WINDOW_STATE_FILE: &str = "mma-shell-settings-window.json";

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(default)]
pub struct WindowState {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    pub topmost: bool,
    pub click_through: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settings: Option<SettingsWindowState>,
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            x: i32::MIN,
            y: 0,
            w: 520,
            h: 680,
            topmost: true,
            click_through: false,
            settings: None,
        }
    }
}

pub fn read_window_state() -> WindowState {
    let Some(dir) = exe_dir() else {
        return WindowState::default();
    };
    read_window_state_in(&dir)
}

pub fn read_window_state_in(dir: &Path) -> WindowState {
    let path = dir.join(WINDOW_STATE_FILE);
    let mut state: WindowState = fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    if state.settings.is_none() {
        let legacy = dir.join(LEGACY_SETTINGS_WINDOW_STATE_FILE);
        if let Ok(s) = fs::read_to_string(legacy) {
            if let Ok(leg) = serde_json::from_str::<SettingsWindowState>(&s) {
                state.settings = Some(leg);
            }
        }
    }
    state
}

pub fn write_window_state(state: &WindowState) {
    let Some(dir) = exe_dir() else {
        return;
    };
    write_window_state_in(&dir, state);
}

pub fn write_window_state_in(dir: &Path, state: &WindowState) {
    let path = dir.join(WINDOW_STATE_FILE);
    let mut to_write = *state;
    if to_write.settings.is_none() {
        let existing = read_window_state_in(dir);
        if existing.settings.is_some() {
            to_write.settings = existing.settings;
        }
    }
    let tmp = path.with_extension("state.tmp");
    if let Ok(serialized) = serde_json::to_string_pretty(&to_write) {
        if fs::write(&tmp, serialized).is_ok() {
            let _ = fs::rename(&tmp, &path);
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(default)]
pub struct SettingsWindowState {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl Default for SettingsWindowState {
    fn default() -> Self {
        Self { x: i32::MIN, y: 0, w: 1040, h: 720 }
    }
}

pub fn read_settings_window_state() -> SettingsWindowState {
    let Some(dir) = exe_dir() else {
        return SettingsWindowState::default();
    };
    read_settings_window_state_in(&dir)
}

pub fn read_settings_window_state_in(dir: &Path) -> SettingsWindowState {
    let state = read_window_state_in(dir);
    state.settings.unwrap_or_default()
}

pub fn write_settings_window_state(state: &SettingsWindowState) {
    let Some(dir) = exe_dir() else {
        return;
    };
    write_settings_window_state_in(&dir, state);
}

pub fn write_settings_window_state_in(dir: &Path, state: &SettingsWindowState) {
    let mut main_state = read_window_state_in(dir);
    main_state.settings = Some(*state);
    let path = dir.join(WINDOW_STATE_FILE);
    let tmp = path.with_extension("state.tmp");
    if let Ok(serialized) = serde_json::to_string_pretty(&main_state) {
        if fs::write(&tmp, serialized).is_ok() {
            let _ = fs::rename(&tmp, &path);
            let legacy = dir.join(LEGACY_SETTINGS_WINDOW_STATE_FILE);
            if legacy.exists() {
                let _ = fs::remove_file(legacy);
            }
        }
    }
}
