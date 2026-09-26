// 路径与 tosu 探测（契约 §6/§8）。

use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

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
    match exe_dir() {
        Some(dir) => read_json_file(&dir.join(file), display),
        None => serde_json::Value::Null,
    }
}

/// 读一份 JSON 文件：不存在/读不到 → `Null`；解析失败 → 记 stderr 警告并 `Null`。
fn read_json_file(path: &Path, display: &str) -> serde_json::Value {
    match fs::read_to_string(path) {
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
        "{\n  \"gameClient\": \"Auto\",\n  \"etternaRoot\": \"\",\n  \"malodyRoot\": \"\",\n  \"malody4Root\": \"\",\n  \"hotkeys\": {\n    \"topmost\": \"Ctrl+Shift+T\",\n    \"clickThrough\": \"Ctrl+Shift+C\",\n    \"close\": \"Ctrl+Q\",\n    \"settings\": \"Ctrl+Shift+S\"\n  },\n  \"logLevel\": \"info\"\n}\n",
    );
}

fn write_exe_json(dir: PathBuf, file: &str, value: &serde_json::Value) -> bool {
    let path = dir.join(file);
    let tmp = path.with_extension("json.tmp");
    let ok = fs::write(&tmp, serde_json::to_string_pretty(value).unwrap_or_default()).is_ok()
        && fs::rename(&tmp, &path).is_ok();
    ok
}

/// 全量插件设置解析（优先级链，**单一在线门控**）：
///   1. tosu 在线（`shared.tosu.is_some()` **且** `tosu_online`）→ tosu 设置文件（只读）
///   2. exe 旁 mma-settings.json 存在 → 读它
///   3. 都没有 → 用插件 settings.json 的默认值生成 mma-settings.json 骨架
///      （用户手动编辑后重启生效），返回该默认。
/// 在线时绝不落盘 mma-settings.json（tosu 权威）。
///
/// 门控与 `server/http.rs` 的 `POST /settings` 403 条件是**同一个表达式**：任何一侧
/// 单独放宽都会造出"能写却读不回"的窗口（详见 Step 3）。
pub fn resolve_plugin_settings(shared: &crate::server::Shared) -> serde_json::Value {
    // 1. tosu 在线：tosu 设置文件权威（离线**一律不看** tosu 文件——那是记忆里的旧值，
    //    本地 mma-settings.json 才是离线权威）。
    if shared.tosu.is_some() && *shared.tosu_online.lock().unwrap() {
        if let Some(info) = shared.tosu.as_ref() {
            let from_tosu = read_tosu_settings(info);
            if from_tosu.is_object() && !from_tosu.as_object().map(|m| m.is_empty()).unwrap_or(true) {
                return from_tosu;
            }
        }
    }
    // 2. mma-settings.json。
    let local = read_plugin_settings();
    if local.is_object() {
        return local;
    }
    // 3. 生成默认（从插件 settings.json 的 value 字段）并落盘 mma-settings.json
    //    （用户手动编辑后重启生效；见 should_seed_local_settings）。
    let defaults = generate_default_plugin_settings();
    if defaults.is_object() {
        let _ = write_plugin_settings(&defaults);
    }
    defaults
}

/// 是否需要生成 mma-settings.json 本地骨架（纯判定，单测四象限）。
///
/// 只在**离线且本地无文件**时生成：在线时 tosu 是权威（落一份本地骨架会立刻被
/// 权威链跳过，还会在切回离线时把记忆里的旧值当成本地权威）；本地已有文件时
/// 绝不覆盖（用户的离线编辑就是权威）。
/// **不实现**从 tosu values 播种（决策 D4a：离线骨架取插件 settings.json 的默认值）。
pub fn should_seed_local_settings(online: bool, has_local: bool) -> bool {
    !online && !has_local
}

/// 启动时确保 mma-settings.json 存在：**仅**离线且无本地设置文件时生成默认骨架
/// （用户手动编辑后重启生效；与 resolve_plugin_settings 第 3 级同源）。
pub fn ensure_plugin_settings(tosu: &Option<TosuInfo>, online: bool) {
    // 在线 → 不生成任何东西（tosu 权威）。`tosu` 参数保留在签名里（调用点已持有它，
    // 且不变量是"权威来源只由 online 决定"，故判定不看 tosu 文件是否存在）。
    let _ = tosu;
    let has_local = read_plugin_settings().is_object();
    if !should_seed_local_settings(online, has_local) {
        return;
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

/// exe 旁 `mma-settings.json` 的完整路径（文件可能不存在）。
pub fn plugin_settings_path() -> Option<PathBuf> {
    Some(exe_dir()?.join(PLUGIN_SETTINGS_FILE))
}

/// exe 旁 `mma-shell-config.json` 的完整路径（文件可能不存在）。
pub fn shell_config_path() -> Option<PathBuf> {
    Some(exe_dir()?.join(SHELL_CONFIG_FILE))
}

/// 文件 mtime（不存在/读不到 → `None`）：定时器用 `stat → read → stat` 识别
/// "读取期间文件又被写入"的 tick（两次 mtime 不同则跳过本轮推送，避免推旧内容）。
pub fn file_mtime(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).ok()?.modified().ok()
}

/// 全量写壳配置（`mma-shell-config.json`，tmp + rename）；返回是否落盘成功。
pub fn write_shell_config(value: &serde_json::Value) -> bool {
    match exe_dir() {
        Some(dir) => write_exe_json(dir, SHELL_CONFIG_FILE, value),
        None => false,
    }
}

/// 读-改-写串行化锁：tmp + rename 只保证**单次写**原子，不保证两次读改写不交错
/// （交错会造出"旧内容回滚"广播）。
static WRITE_LOCK: Mutex<()> = Mutex::new(());

/// 对象递归合并：
///   * 两侧都是对象 → 逐键递归（base 的未知键全部保留）；
///   * patch 值为 `null` → **忽略**（base 值存活；base 无此键则不新增）；
///   * 其余（任一侧非对象）→ 以 patch 覆盖。
pub fn merge_json(base: &serde_json::Value, patch: &serde_json::Value) -> serde_json::Value {
    match (base, patch) {
        (serde_json::Value::Object(b), serde_json::Value::Object(p)) => {
            let mut out = b.clone();
            for (k, v) in p {
                if v.is_null() {
                    continue;
                }
                let merged = match out.get(k) {
                    Some(existing) => merge_json(existing, v),
                    None => v.clone(),
                };
                out.insert(k.clone(), merged);
            }
            serde_json::Value::Object(out)
        }
        _ => patch.clone(),
    }
}

/// 一份 exe 旁 JSON 的"请求键优先"读-改-写：base 非对象（缺失/损坏）→ `None`
/// （**不写盘**、不生成骨架，交调用方回 400/500）；写失败 → `None`；
/// 成功 → 返回合并后的全量值。
fn merge_exe_json(
    dir: &Path,
    file: &str,
    display: &str,
    patch: &serde_json::Value,
) -> Option<serde_json::Value> {
    let _guard = WRITE_LOCK.lock().unwrap();
    let base = read_json_file(&dir.join(file), display);
    if !base.is_object() {
        return None;
    }
    let merged = merge_json(&base, patch);
    if !write_exe_json(dir.to_path_buf(), file, &merged) {
        return None;
    }
    Some(merged)
}

/// `mma-settings.json` 的读-改-写（离线 `POST /settings`：请求键优先）。
pub fn merge_plugin_settings(patch: &serde_json::Value) -> Option<serde_json::Value> {
    let dir = exe_dir()?;
    merge_exe_json(dir.as_path(), PLUGIN_SETTINGS_FILE, "mma-settings.json", patch)
}

/// `mma-shell-config.json` 的读-改-写（`POST /shell-config`：请求键优先）。
pub fn patch_shell_config(patch: &serde_json::Value) -> Option<serde_json::Value> {
    let dir = exe_dir()?;
    merge_exe_json(dir.as_path(), SHELL_CONFIG_FILE, "mma-shell-config.json", patch)
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
static MALODY4_DETECT_CACHE: Mutex<Option<(Instant, Option<PathBuf>)>> = Mutex::new(None);

/// 清空三个探测缓存（壳配置改了根目录后立即失效，否则 ≤30s 内仍用旧结果）。
pub fn clear_detect_caches() {
    if let Ok(mut c) = ETTERNA_DETECT_CACHE.lock() {
        *c = None;
    }
    if let Ok(mut c) = MALODY_DETECT_CACHE.lock() {
        *c = None;
    }
    if let Ok(mut c) = MALODY4_DETECT_CACHE.lock() {
        *c = None;
    }
}

/// 探测结果缓存。`once` 是泛型闭包（不是函数指针）：Malody 4 的启发探测需要捕获
/// `process_exe` 传给 `detect_malody4_root_once`，函数指针签名无法编译。
fn detect_cached<F: Fn() -> Option<PathBuf>>(
    cache: &Mutex<Option<(Instant, Option<PathBuf>)>>,
    once: F,
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

/// Malody 4.3.7 的启发候选（绝对路径**只允许出现在这里**，且是解析链的最后一级，
/// 绝不是权威来源）。候选目录可能撞上 MalodyV / Maupdate，故每级都做 PE 版本校验。
const MALODY4_CANDIDATES: [&str; 5] = [
    "D:/Games/Malody-4.3.7",
    "C:/Games/Malody-4.3.7",
    "D:/Malody-4.3.7",
    "D:/Games/Malody",
    "C:/Malody-4.3.7",
];

/// Malody 4.3.7 根目录的**启发式尾部**（30s TTL 缓存 + 盘符预检）。
///
/// 只遍历 [`MALODY4_CANDIDATES`]：`process_exe` 级已由 `server::malody4_root` 作为解析链
/// 第 1 级"非空即采纳"处理（在这里再写一次就是不可达的死分支），`MMA_MALODY4_ROOT` /
/// 壳配置 `malody4Root` / tosu 设置三级同样由 `server::malody4_root` 处理。
/// `process_exe` 参数**仅用于日志说明**，本函数不据此判定。
pub fn detect_malody4_root(process_exe: Option<&Path>) -> Option<PathBuf> {
    detect_cached(&MALODY4_DETECT_CACHE, || detect_malody4_root_once(process_exe))
}

fn detect_malody4_root_once(process_exe: Option<&Path>) -> Option<PathBuf> {
    let hit = scan_malody4_candidates(&MALODY4_CANDIDATES);
    crate::server::log::log_at(
        "debug",
        &format!(
            "malody4 heuristic scan: process_exe={} -> {}",
            process_exe
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "none".to_string()),
            hit.as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "no candidate validated".to_string())
        ),
    );
    hit
}

/// 候选的**三重检查**：`beatmap/` 目录存在 **且** 同目录 `malody.exe` 存在
/// **且** PE 版本校验通过（`anchor::validate_pe_file`）。
fn scan_malody4_candidates(candidates: &[&str]) -> Option<PathBuf> {
    for candidate in candidates {
        let dir = PathBuf::from(candidate);
        if !drive_root_ready(&dir) {
            continue; // 盘符不存在/未就绪：快速跳过（读卡器上的元数据查询可能阻塞）
        }
        let exe = dir.join("malody.exe");
        if !dir.join("beatmap").is_dir() || !exe.is_file() {
            continue;
        }
        if crate::malody4::anchor::validate_pe_file(&exe, crate::malody4::anchor::ClientSpec::current())
            .is_err()
        {
            continue;
        }
        return Some(dir);
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

// ---- 设置窗口几何（mma-shell-settings-window.json，exe 旁）----
// **独立文件**：与主窗 WindowState / mma-shell-state.json 完全分离——主窗的 4 条
// 写通道与 5s persister 只碰 WindowState，两窗几何不会互相踩。

const SETTINGS_WINDOW_STATE_FILE: &str = "mma-shell-settings-window.json";

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
#[serde(default)]
pub struct SettingsWindowState {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl Default for SettingsWindowState {
    fn default() -> Self {
        // x == i32::MIN 是"无记忆位置"哨兵（与 WindowState 同约定）→ 建窗时居中。
        Self { x: i32::MIN, y: 0, w: 1040, h: 720 }
    }
}

pub fn read_settings_window_state() -> SettingsWindowState {
    let Some(dir) = exe_dir() else {
        return SettingsWindowState::default();
    };
    read_settings_window_state_in(&dir)
}

/// `read_settings_window_state` 的可注入路径版（缺失/损坏 → 默认；单测打在这条缝上）。
fn read_settings_window_state_in(dir: &Path) -> SettingsWindowState {
    fs::read_to_string(dir.join(SETTINGS_WINDOW_STATE_FILE))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn write_settings_window_state(state: &SettingsWindowState) {
    let Some(dir) = exe_dir() else {
        return;
    };
    write_settings_window_state_in(&dir, state);
}

fn write_settings_window_state_in(dir: &Path, state: &SettingsWindowState) {
    let path = dir.join(SETTINGS_WINDOW_STATE_FILE);
    let tmp = path.with_extension("state.tmp");
    if fs::write(&tmp, serde_json::to_string(state).unwrap_or_default()).is_ok() {
        let _ = fs::rename(&tmp, &path);
    }
}

#[cfg(test)]
#[path = "../tests-local/config.rs"]
mod tests;
