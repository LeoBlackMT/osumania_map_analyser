// 路径与 tosu 探测（契约 §6/§8）。

use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const PLUGIN_FOLDER: &str = "ManiaMapAnalyser by Leo_Black";

/// 插件目录解析：MMA_PLUGIN_DIR 覆盖 → exe 相对路径探测。
pub fn plugin_dir() -> PathBuf {
    if let Ok(over) = env::var("MMA_PLUGIN_DIR") {
        if !over.is_empty() {
            return PathBuf::from(over);
        }
    }
    // 生产：exe 与插件目录同层；开发：target/debug 上两级。
    if let Ok(exe) = env::current_exe() {
        let exe_dir = exe.parent().unwrap_or(Path::new("."));
        for up in 0..=3 {
            let mut dir = exe_dir.to_path_buf();
            for _ in 0..up {
                dir = dir.join("..");
            }
            let candidate = dir.join(PLUGIN_FOLDER);
            if candidate.join("index.html").exists() {
                return candidate;
            }
        }
        let direct = exe_dir.join(PLUGIN_FOLDER);
        if direct.join("index.html").exists() {
            return direct;
        }
    }
    PathBuf::from(PLUGIN_FOLDER)
}

/// tosu 运行信息（tosu.env 解析结果）。
pub struct TosuInfo {
    pub root: PathBuf,
    pub ip: String,
    pub port: u16,
}

impl TosuInfo {
    pub fn base_url(&self) -> String {
        format!("http://{}:{}", self.ip, self.port)
    }
}

/// 从 exe 位置逐级向上（含当前层）最多 3 层找 tosu.env。
pub fn probe_tosu_env() -> Option<TosuInfo> {
    if env::var("MMA_SKIP_TOSU_PROBE").is_ok() {
        return None;
    }
    let start = env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    let mut dir = start.clone();
    for _ in 0..=3 {
        let env_file = dir.join("tosu.env");
        if env_file.exists() {
            if let Some(info) = parse_tosu_env(&env_file) {
                return Some(info);
            }
        }
        if !dir.pop() {
            break;
        }
    }
    None
}

fn parse_tosu_env(path: &Path) -> Option<TosuInfo> {
    let content = fs::read_to_string(path).ok()?;
    let mut port = 24050u16;
    let mut ip = String::from("127.0.0.1");
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            let k = k.trim().to_uppercase();
            let v = v.trim();
            match k.as_str() {
                "SERVER_PORT" => {
                    if let Ok(p) = v.parse::<u16>() {
                        port = p;
                    }
                }
                "SERVER_IP" if !v.is_empty() => {
                    ip = v.to_string();
                }
                _ => {}
            }
        }
    }
    let root = path.parent()?.to_path_buf();
    Some(TosuInfo { root, ip, port })
}

/// 健康探测：GET {ip}:{port}/ 返回 200 即在线（仅 TCP 连通性轻量判定）。
pub fn tosu_online(info: &TosuInfo) -> bool {
    let addr = format!("{}:{}", info.ip, info.port);
    let Ok(parsed) = addr.parse::<std::net::SocketAddr>() else {
        return false;
    };
    match TcpStream::connect_timeout(&parsed, Duration::from_secs(2)) {
        Ok(stream) => {
            drop(stream);
            true
        }
        Err(_) => false,
    }
}

/// tosu 设置文件（只读）：{tosuRoot}/settings/{插件目录名}.json
pub fn tosu_settings_path(info: &TosuInfo) -> PathBuf {
    info.root
        .join("settings")
        .join(format!("{}.json", PLUGIN_FOLDER))
}

pub fn read_tosu_settings(info: &TosuInfo) -> serde_json::Value {
    let path = tosu_settings_path(info);
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::Value::Null)
}

// ---- 独立配置（mma-shell.json，exe 旁）：无 tosu 用户也能配置游戏路径 ----

/// 读取 exe 所在目录下的 mma-shell.json（不存在/损坏 → 空对象）。
pub fn read_shell_config() -> serde_json::Value {
    let Some(dir) = exe_dir() else {
        return serde_json::Value::Null;
    };
    let path = dir.join("mma-shell.json");
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::Value::Null)
}

/// /settings POST 时落盘（仅保存已知键，丢弃其余）。
pub fn write_shell_config(value: &serde_json::Value) -> bool {
    let Some(dir) = exe_dir() else {
        return false;
    };
    let mut out = serde_json::Map::new();
    for key in ["gameClient", "etternaRoot", "malodyRoot"] {
        if let Some(v) = value.get(key) {
            out.insert(key.to_string(), v.clone());
        }
    }
    let path = dir.join("mma-shell.json");
    let tmp = path.with_extension("json.tmp");
    let ok = fs::write(&tmp, serde_json::to_string_pretty(&out).unwrap_or_default()).is_ok()
        && fs::rename(&tmp, &path).is_ok();
    ok
}

fn exe_dir() -> Option<PathBuf> {
    env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
}

// ---- 窗口状态记忆（mma-shell-state.json，exe 旁）----

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
pub struct WindowState {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    pub topmost: bool,
    pub click_through: bool,
}

impl Default for WindowState {
    fn default() -> Self {
        Self { x: i32::MIN, y: 0, w: 520, h: 680, topmost: true, click_through: false }
    }
}

pub fn read_window_state() -> WindowState {
    let Some(dir) = exe_dir() else {
        return WindowState::default();
    };
    let path = dir.join("mma-shell-state.json");
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn write_window_state(state: &WindowState) {
    let Some(dir) = exe_dir() else {
        return;
    };
    let path = dir.join("mma-shell-state.json");
    let tmp = path.with_extension("state.tmp");
    if fs::write(&tmp, serde_json::to_string(state).unwrap_or_default()).is_ok() {
        let _ = fs::rename(&tmp, &path);
    }
}