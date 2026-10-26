// config::window - 窗口位置、尺寸、穿透与置顶持久化状态

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use super::common::exe_dir;

pub const WINDOW_STATE_FILE: &str = "mma-shell-state.json";
pub const LEGACY_SETTINGS_WINDOW_STATE_FILE: &str = "mma-shell-settings-window.json";

/// Windows 最小化时通常被移至 (-32000, -32000)。
/// 多显示器正常虚拟坐标通常在 -20000 到 30000 之间。
pub const MIN_VALID_COORDINATE: i32 = -20000;

/// 主窗口最小允许尺寸（低于此值视为最小化 0x0 或无效尺寸）
pub const MAIN_WINDOW_MIN_W: u32 = 200;
pub const MAIN_WINDOW_MIN_H: u32 = 150;

/// 设置窗口最小允许尺寸
pub const SETTINGS_WINDOW_MIN_W: u32 = 400;
pub const SETTINGS_WINDOW_MIN_H: u32 = 300;

/// 进程级文件写互斥锁，避免多线程/多窗口并发读改写同一文件发生 sharing violation
static STATE_FILE_LOCK: Mutex<()> = Mutex::new(());
static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn unique_tmp_path(path: &Path) -> PathBuf {
    let cnt = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    path.with_extension(format!("state.{}.{}.tmp", pid, cnt))
}

pub fn is_valid_position(x: i32, y: i32) -> bool {
    x > MIN_VALID_COORDINATE && y > MIN_VALID_COORDINATE
}

pub fn is_valid_main_window_size(w: u32, h: u32) -> bool {
    w >= MAIN_WINDOW_MIN_W && h >= MAIN_WINDOW_MIN_H
}

pub fn is_valid_settings_window_size(w: u32, h: u32) -> bool {
    w >= SETTINGS_WINDOW_MIN_W && h >= SETTINGS_WINDOW_MIN_H
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
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
    // 反馈用的匿名 install id（32 hex）。与窗口状态同文件，减少根目录散落文件。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install_id: Option<String>,
}

impl WindowState {
    /// 纠正非法数据（如最小化残留的 -32000 坐标或 0x0 尺寸），防止窗口在屏幕外或不可见。
    pub fn sanitize(&mut self) {
        if !is_valid_position(self.x, self.y) && self.x != i32::MIN {
            self.x = i32::MIN;
            self.y = 0;
        }
        if !is_valid_main_window_size(self.w, self.h) {
            let def = Self::default();
            self.w = def.w;
            self.h = def.h;
        }
        if let Some(ref mut settings) = self.settings {
            settings.sanitize();
        }
    }
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
            install_id: None,
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
    state.sanitize();
    state
}

pub fn write_window_state(state: &WindowState) {
    let Some(dir) = exe_dir() else {
        return;
    };
    write_window_state_in(&dir, state);
}

/// 写窗口状态。返回是否**真正落盘**（tmp + rename 都成功）——install_id 的持久化
/// 依赖这个结果：写失败却报告一个"已保存"的 id，会让每次请求换一个 id、服务端去重失效。
pub fn write_window_state_in(dir: &Path, state: &WindowState) -> bool {
    let _guard = STATE_FILE_LOCK.lock().unwrap();
    let path = dir.join(WINDOW_STATE_FILE);
    let mut to_write = state.clone();
    to_write.sanitize();
    if to_write.settings.is_none() || to_write.install_id.is_none() {
        let existing = read_window_state_in(dir);
        if to_write.settings.is_none() && existing.settings.is_some() {
            to_write.settings = existing.settings;
        }
        if to_write.install_id.is_none() {
            to_write.install_id = existing.install_id;
        }
    }
    let tmp = unique_tmp_path(&path);
    let mut written = false;
    if let Ok(serialized) = serde_json::to_string_pretty(&to_write) {
        if fs::write(&tmp, serialized).is_ok() {
            written = fs::rename(&tmp, &path).is_ok();
            if !written {
                let _ = fs::remove_file(&tmp);
            }
        }
    }
    written
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(default)]
pub struct SettingsWindowState {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl SettingsWindowState {
    pub fn sanitize(&mut self) {
        if !is_valid_position(self.x, self.y) && self.x != i32::MIN {
            self.x = i32::MIN;
            self.y = 0;
        }
        if !is_valid_settings_window_size(self.w, self.h) {
            let def = Self::default();
            self.w = def.w;
            self.h = def.h;
        }
    }
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
    let _guard = STATE_FILE_LOCK.lock().unwrap();
    let mut main_state = read_window_state_in(dir);
    let mut st = *state;
    st.sanitize();
    main_state.settings = Some(st);
    let path = dir.join(WINDOW_STATE_FILE);
    let tmp = unique_tmp_path(&path);
    if let Ok(serialized) = serde_json::to_string_pretty(&main_state) {
        if fs::write(&tmp, serialized).is_ok() {
            if fs::rename(&tmp, &path).is_ok() {
                let legacy = dir.join(LEGACY_SETTINGS_WINDOW_STATE_FILE);
                if legacy.exists() {
                    let _ = fs::remove_file(legacy);
                }
            } else {
                let _ = fs::remove_file(&tmp);
            }
        }
    }
}
