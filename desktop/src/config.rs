// 路径与 tosu 探测（契约 §6/§8）。

use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const PLUGIN_FOLDER: &str = "ManiaMapAnalyser by Leo_Black";

/// 插件目录解析：MMA_PLUGIN_DIR 覆盖 → exe 所在目录（exe 直接放插件目录内，
/// 如用户自定义目录名 "ManiaMapAnalyser-PR"）→ 按 PLUGIN_FOLDER 向上探测。
pub fn plugin_dir() -> PathBuf {
    if let Ok(over) = env::var("MMA_PLUGIN_DIR") {
        if !over.is_empty() {
            return PathBuf::from(over);
        }
    }
    if let Ok(exe) = env::current_exe() {
        let exe_dir = exe.parent().unwrap_or(Path::new("."));
        // 1. exe 所在目录本身就是插件目录（index.html 在旁）：用户把壳 exe
        //    直接放进（可能自定义名字的）插件目录时，服务该目录而非硬编码名。
        if exe_dir.join("index.html").exists() {
            return exe_dir.to_path_buf();
        }
        // 2. 兼容：exe 与插件目录同层（"ManiaMapAnalyser by Leo_Black" 默认名）。
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

/// tosu 设置文件（只读）：{tosuRoot}/settings/{插件目录名}.values.json
/// （tosu 的设置文件名 = 插件目录名 + ".values.json"；目录名取实际解析出的
/// plugin_dir 的目录名——用户可能用自定义目录名如 "ManiaMapAnalyser-PR1"）。
pub fn tosu_settings_path(info: &TosuInfo) -> PathBuf {
    let folder = plugin_dir()
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| PLUGIN_FOLDER.to_string());
    info.root
        .join("settings")
        .join(format!("{}.values.json", folder))
}

pub fn read_tosu_settings(info: &TosuInfo) -> serde_json::Value {
    let path = tosu_settings_path(info);
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::Value::Null)
}

// ---- 独立配置：壳配置（mma-shell-config.json，exe 旁）----
// 壳配置 = 仅壳使用：gameClient/etternaRoot/malodyRoot（源路径）+ hotkeys + logLevel。
// 插件全量设置另存 mma-settings.json（见 read_plugin_settings / write_plugin_settings）。

const SHELL_CONFIG_FILE: &str = "mma-shell-config.json";
const PLUGIN_SETTINGS_FILE: &str = "mma-settings.json";

/// 读取 exe 所在目录下的 mma-shell-config.json（不存在/损坏 → 空对象；损坏时记录警告到日志）。
pub fn read_shell_config() -> serde_json::Value {
    read_exe_json(SHELL_CONFIG_FILE, "mma-shell-config.json")
}

/// 读取 exe 旁 mma-settings.json（全量插件设置；无 tosu 用户手动编辑）。
pub fn read_plugin_settings() -> serde_json::Value {
    read_exe_json(PLUGIN_SETTINGS_FILE, "mma-settings.json")
}

fn read_exe_json(file: &str, display: &str) -> serde_json::Value {
    let Some(dir) = exe_dir() else {
        return serde_json::Value::Null;
    };
    let path = dir.join(file);
    match fs::read_to_string(&path) {
        Ok(s) => match serde_json::from_str(&s) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("{} parse failed (using defaults): {}", display, e);
                serde_json::Value::Null
            }
        },
        Err(_) => serde_json::Value::Null,
    }
}

/// 日志级别（mma-shell-config.json 的 logLevel；默认 info）。
pub fn log_level() -> String {
    read_shell_config()
        .get("logLevel")
        .and_then(|v| v.as_str())
        .map(|s| s.to_lowercase())
        .filter(|s| matches!(s.as_str(), "debug" | "info" | "warn" | "error" | "off"))
        .unwrap_or_else(|| "info".to_string())
}

/// 路径归一化：用户可能写 `D:\Games\Etterna`（json 里反斜杠需转义，但容错
/// 处理 `\` 与 `/` 混用），统一转 `/` 并去掉尾部斜杠。
pub fn normalize_path(input: &str) -> String {
    input.trim().replace('\\', "/").trim_end_matches('/').to_string()
}

/// 取配置里的路径字段（归一化）；缺失/空 → None。
pub fn config_path(value: &serde_json::Value, key: &str) -> Option<PathBuf> {
    let raw = value.get(key).and_then(|v| v.as_str())?;
    let norm = normalize_path(raw);
    if norm.is_empty() {
        None
    } else {
        Some(PathBuf::from(norm))
    }
}

/// 启动时确保 mma-shell-config.json 存在（无 tosu 用户可发现并直接编辑）。
/// 骨架含 hotkeys 与 logLevel（用户验收项 1：骨架必须完整可编辑）。
pub fn ensure_shell_config() {
    let Some(dir) = exe_dir() else {
        return;
    };
    let path = dir.join(SHELL_CONFIG_FILE);
    if path.exists() {
        return;
    }
    let _ = fs::write(
        &path,
        "{\n  \"gameClient\": \"Auto\",\n  \"etternaRoot\": \"\",\n  \"malodyRoot\": \"\",\n  \"hotkeys\": {\n    \"topmost\": \"Ctrl+Shift+T\",\n    \"clickThrough\": \"Ctrl+Shift+C\",\n    \"close\": \"Ctrl+Q\"\n  },\n  \"logLevel\": \"info\"\n}\n",
    );
}

fn write_exe_json(dir: PathBuf, file: &str, value: &serde_json::Value) -> bool {
    let path = dir.join(file);
    let tmp = path.with_extension("json.tmp");
    let ok = fs::write(&tmp, serde_json::to_string_pretty(value).unwrap_or_default()).is_ok()
        && fs::rename(&tmp, &path).is_ok();
    ok
}

/// 全量插件设置解析（优先级链）：
///   1. tosu 在线（壳探测到 tosu 存活）→ tosu 设置文件（只读）
///   2. tosu 设置文件存在（即使 tosu 未运行）→ 读文件
///   3. exe 旁 mma-settings.json 存在 → 读它
///   4. 都没有 → 用插件 settings.json 的默认值生成 mma-settings.json 骨架
///      （用户手动编辑后重启生效），返回该默认。
/// 在线时绝不落盘 mma-settings.json（tosu 权威）。
pub fn resolve_plugin_settings(shared: &crate::server::Shared) -> serde_json::Value {
    // 1. tosu 在线：tosu 设置文件权威（若可读）。
    if let Some(info) = shared.tosu.as_ref() {
        let from_tosu = read_tosu_settings(info);
        if from_tosu.is_object() && !from_tosu.as_object().map(|m| m.is_empty()).unwrap_or(true) {
            return from_tosu;
        }
    }
    // 2. tosu 设置文件存在（离线也读）。
    if let Some(info) = shared.tosu.as_ref() {
        if tosu_settings_path(info).exists() {
            let from_tosu = read_tosu_settings(info);
            if from_tosu.is_object() {
                return from_tosu;
            }
        }
    }
    // 3. mma-settings.json。
    let local = read_plugin_settings();
    if local.is_object() {
        return local;
    }
    // 4. 生成默认（从插件 settings.json 的 value 字段）并落盘 mma-settings.json
    //    （用户手动编辑后重启生效；壳只在「无 tosu 设置文件」时才生成）。
    let defaults = generate_default_plugin_settings();
    if defaults.is_object() {
        let _ = write_plugin_settings(&defaults);
    }
    defaults
}

/// 启动时确保 mma-settings.json 存在：无 tosu 设置文件时生成默认骨架
/// （用户手动编辑后重启生效；与 resolve_plugin_settings 第 4 级同源）。
pub fn ensure_plugin_settings(tosu: &Option<TosuInfo>) {
    // tosu 设置文件存在（在线或离线）→ 不生成（tosu 权威）。
    if let Some(info) = tosu {
        if tosu_settings_path(info).exists() {
            return;
        }
    }
    if read_plugin_settings().is_object() {
        return; // 已有本地设置
    }
    let defaults = generate_default_plugin_settings();
    if defaults.is_object() {
        let _ = write_plugin_settings(&defaults);
    }
}

/// 从插件 settings.json 生成默认设置骨架（所有条目 value 字段 → 顶层键）。
/// 返回 {uniqueID: value} 形式；无 settings.json 时返回空对象。
pub fn generate_default_plugin_settings() -> serde_json::Value {
    let mut out = serde_json::Map::new();
    let path = plugin_dir().join("settings.json");
    if let Ok(text) = fs::read_to_string(&path) {
        if let Ok(entries) = serde_json::from_str::<Vec<serde_json::Value>>(&text) {
            for entry in entries {
                if let (Some(id), Some(val)) = (
                    entry.get("uniqueID").and_then(|v| v.as_str()),
                    entry.get("value").cloned(),
                ) {
                    out.insert(id.to_string(), val);
                }
            }
        }
    }
    serde_json::Value::Object(out)
}

/// 离线 /settings POST 落盘：写入 mma-settings.json（全量插件设置）。
pub fn write_plugin_settings(value: &serde_json::Value) -> bool {
    let Some(dir) = exe_dir() else {
        return false;
    };
    write_exe_json(dir, PLUGIN_SETTINGS_FILE, value)
}

fn exe_dir() -> Option<PathBuf> {
    env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
}

// ---- Steam 库发现（注册表 + libraryfolders.vdf）+ 常见路径启发探测 ----

/// 盘符预检：候选路径探测前先确认盘符存在且就绪。用户可能没有 D: 盘；
/// "存在但未就绪"的驱动器（读卡器/光驱）上的元数据查询还可能阻塞数秒——
/// 先探根目录快速跳过。非盘符前缀（UNC/相对路径）视为可用，交由后续判定。
fn drive_root_ready(p: &Path) -> bool {
    let Some(first) = p.iter().next() else {
        return false;
    };
    let first = first.to_string_lossy();
    let bytes = first.as_bytes();
    if bytes.len() == 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        fs::metadata(format!("{}/", &first[..2])).is_ok()
    } else {
        true
    }
}

/// 探测结果 30s TTL 缓存：未配置根目录时轮询器每个周期都会走到 detect_*，
/// 每次都打注册表 + vdf + 逐候选盘符探测既浪费也会放大未就绪驱动器的阻塞。
const DETECT_CACHE_TTL: Duration = Duration::from_secs(30);

static ETTERNA_DETECT_CACHE: Mutex<Option<(Instant, Option<PathBuf>)>> = Mutex::new(None);
static MALODY_DETECT_CACHE: Mutex<Option<(Instant, Option<PathBuf>)>> = Mutex::new(None);

fn detect_cached(
    cache: &Mutex<Option<(Instant, Option<PathBuf>)>>,
    once: fn() -> Option<PathBuf>,
) -> Option<PathBuf> {
    if let Ok(guard) = cache.lock() {
        if let Some((at, hit)) = guard.as_ref() {
            if at.elapsed() < DETECT_CACHE_TTL {
                return hit.clone();
            }
        }
    }
    let hit = once();
    if let Ok(mut guard) = cache.lock() {
        *guard = Some((Instant::now(), hit.clone()));
    }
    hit
}

/// 读 Windows 注册表拿 Steam 安装路径（SteamPath/InstallPath），并解析
/// libraryfolders.vdf 收集全部 Steam 库根（含 Steam 自身库与其他库）。
/// 非 Windows 平台：仅靠硬编码候选（壳目前 Windows-only，Linux 构建时跳过注册表）。
fn steam_library_roots() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    fn push_unique(roots: &mut Vec<PathBuf>, p: &str) {
        let p = p.trim();
        if !p.is_empty() && !roots.iter().any(|r| r.to_string_lossy().eq_ignore_ascii_case(p)) {
            roots.push(PathBuf::from(p));
        }
    }
    // 1. 注册表：HKCU SteamPath、HKLM InstallPath（32/64 位视图）。
    #[cfg(windows)]
    {
        let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
            .open_subkey("Software\\Valve\\Steam")
            .ok();
        if let Some(key) = hkcu {
            if let Ok(p) = key.get_value::<String, _>("SteamPath") {
                push_unique(&mut roots, &p);
            }
        }
        for sub in ["SOFTWARE\\WOW6432Node\\Valve\\Steam", "SOFTWARE\\Valve\\Steam"] {
            let hklm = winreg::RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE)
                .open_subkey(sub)
                .ok();
            if let Some(key) = hklm {
                if let Ok(p) = key.get_value::<String, _>("InstallPath") {
                    push_unique(&mut roots, &p);
                }
            }
        }
    }
    // 2. 每个 Steam 根的 libraryfolders.vdf 里的全部库路径。
    let initial = roots.clone();
    for root in initial {
        let vdf = root.join("steamapps").join("libraryfolders.vdf");
        if let Ok(text) = fs::read_to_string(&vdf) {
            // vdf 形态：`"path"		"C:\\Program Files (x86)\\Steam"`——按行找 path 键。
            for line in text.lines() {
                let line = line.trim();
                if let Some(idx) = line.find("\"path\"") {
                    let rest = &line[idx + "\"path\"".len()..];
                    if let Some(vq) = rest.find('"') {
                        let after = &rest[vq + 1..];
                        if let Some(end) = after.find('"') {
                            let raw = &after[..end];
                            // vdf 用 \\ 表示字面反斜杠（KeyValues 转义）。
                            let decoded = raw.replace("\\\\", "\\").replace("\\/", "/");
                            push_unique(&mut roots, &decoded);
                        }
                    }
                }
            }
        }
    }
    roots
}

/// Etterna：Steam 库（appid 607810 的 common/Etterna）→ 常见路径 → env。
pub fn detect_etterna_root() -> Option<PathBuf> {
    detect_cached(&ETTERNA_DETECT_CACHE, detect_etterna_root_once)
}

fn detect_etterna_root_once() -> Option<PathBuf> {
    if let Ok(over) = env::var("MMA_ETTERNA_ROOT") {
        let p = PathBuf::from(normalize_path(&over));
        if drive_root_ready(&p) && p.join("Save").is_dir() {
            return Some(p);
        }
    }
    for lib in steam_library_roots() {
        let dir = lib.join("steamapps").join("common").join("Etterna");
        if drive_root_ready(&dir) && dir.join("Save").is_dir() {
            return Some(dir);
        }
    }
    let candidates = [
        "D:/Games/Etterna",
        "C:/Games/Etterna",
        "D:/Etterna",
        "C:/Etterna",
    ];
    for c in candidates {
        let dir = PathBuf::from(c);
        if drive_root_ready(&dir) && dir.join("Save").is_dir() {
            return Some(dir);
        }
    }
    None
}

/// MalodyV：Steam 库（common/MalodyV）→ 常见路径 → env。
pub fn detect_malody_root() -> Option<PathBuf> {
    detect_cached(&MALODY_DETECT_CACHE, detect_malody_root_once)
}

fn detect_malody_root_once() -> Option<PathBuf> {
    if let Ok(over) = env::var("MMA_MALODY_ROOT") {
        let p = PathBuf::from(normalize_path(&over));
        if drive_root_ready(&p) && p.join("chart").is_dir() && p.join("skin").is_dir() {
            return Some(p);
        }
    }
    for lib in steam_library_roots() {
        let dir = lib.join("steamapps").join("common").join("MalodyV");
        if drive_root_ready(&dir) && dir.join("chart").is_dir() && dir.join("skin").is_dir() {
            return Some(dir);
        }
    }
    let candidates = [
        "D:/Steam/steamapps/common/MalodyV",
        "D:/SteamLibrary/steamapps/common/MalodyV",
        "C:/Program Files (x86)/Steam/steamapps/common/MalodyV",
        "C:/SteamLibrary/steamapps/common/MalodyV",
    ];
    for c in candidates {
        let dir = PathBuf::from(c);
        if drive_root_ready(&dir) && dir.join("chart").is_dir() && dir.join("skin").is_dir() {
            return Some(dir);
        }
    }
    None
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