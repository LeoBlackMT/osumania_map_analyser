// 壳服务核心：Shared 状态、帧封装、启动与定时器。
// 拆分：mod.rs（状态/帧/启动/timers）+ http.rs（24061 静态/settings/cover）
//       + ws.rs（/ws 帧循环）+ post.rs（24060 POST/resolve/control）+ log.rs（日志）。

pub mod bridge;
pub mod http;
pub mod log;
pub mod post;
pub mod ws;

use crate::config::{self, TosuInfo};
use crate::frames::*;
use std::collections::HashMap;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub struct Shared {
    pub plugin_dir: PathBuf,
    pub tosu: Option<TosuInfo>,
    pub seq: AtomicU64,
    /// WS 客户端出站通道（(连接 id, 发送端)）。
    pub sinks: Mutex<Vec<(u64, mpsc::Sender<String>)>>,
    /// pending POST（request_id → 应答通道）。
    pub pending: Mutex<HashMap<String, mpsc::Sender<String>>>,
    /// 封面白名单（精确文件路径）。
    pub cover_whitelist: Mutex<std::collections::HashSet<String>>,
    /// 离线设置存储（在线时全部走 tosu 只读）。
    pub offline_settings: Mutex<serde_json::Value>,
    /// 离线全量插件设置（mma-settings.json）缓存（timers 检测变化推送）。
    pub plugin_settings: Mutex<serde_json::Value>,
    /// tosu 设置文件缓存（timers 检测变化推送 settings 帧）。
    pub tosu_settings_cache: Mutex<serde_json::Value>,
    /// Malody 最近 POST 时间（60s 存活窗口）。
    pub last_malody_post: Mutex<Option<Instant>>,
    /// Malody V 选曲桥状态（17653 listener 更新）——去重、心跳与会话重置的唯一去重点。
    pub malody_bridge: Mutex<bridge::MalodyBridgeState>,
    /// 17653 是否绑定成功（被占用时为 false，其余功能照常）。
    pub bridge_listen_ok: Mutex<bool>,
    pub tosu_online: Mutex<bool>,
    /// 壳侧推送错误面（state.errors，页面 status 行展示）。
    pub shell_errors: Mutex<Vec<String>>,
    /// Etterna 桥状态（poller 更新）。
    pub etterna: Mutex<crate::etterna::EtternaStatus>,
    /// Malody 4.3.7 原生源状态（poller 更新；形状与 `EtternaStatus` 同角色）。
    pub malody4: Mutex<crate::malody4::Malody4Status>,
    /// 主窗口控制句柄（契约 v2 control 帧；无窗口模式为 None）。
    pub window: Mutex<Option<tauri::WebviewWindow>>,
}

/// 注入主窗口句柄（main.rs setup 调用）。
pub fn set_main_window(shared: &Shared, window: tauri::WebviewWindow) {
    *shared.window.lock().unwrap() = Some(window);
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub fn new_shared(plugin_dir: PathBuf, tosu: Option<TosuInfo>) -> Arc<Shared> {
    Arc::new(Shared {
        plugin_dir,
        tosu,
        seq: AtomicU64::new(0),
        sinks: Mutex::new(Vec::new()),
        pending: Mutex::new(HashMap::new()),
        cover_whitelist: Mutex::new(std::collections::HashSet::new()),
        offline_settings: Mutex::new(config::read_shell_config()),
        plugin_settings: Mutex::new(config::read_plugin_settings()),
        tosu_settings_cache: Mutex::new(serde_json::Value::Null),
        last_malody_post: Mutex::new(None),
        malody_bridge: Mutex::new(bridge::MalodyBridgeState::default()),
        bridge_listen_ok: Mutex::new(false),
        tosu_online: Mutex::new(false),
        shell_errors: Mutex::new(Vec::new()),
        etterna: Mutex::new(crate::etterna::EtternaStatus::default()),
        malody4: Mutex::new(crate::malody4::Malody4Status::default()),
        window: Mutex::new(None),
    })
}

pub fn next_seq(shared: &Shared) -> u64 {
    shared.seq.fetch_add(1, Ordering::Relaxed) + 1
}

pub fn broadcast(shared: &Shared, frame_type: &str, payload: Option<serde_json::Value>) {
    let env = Envelope::new(frame_type, next_seq(shared), payload);
    let text = serde_json::to_string(&env).unwrap_or_default();
    let sinks = shared.sinks.lock().unwrap();
    for (_id, sink) in sinks.iter() {
        let _ = sink.send(text.clone());
    }
}

/// state 帧的薄封装：**先全部克隆再组装**，组装本身在 `malody4::build_state_frame`
/// 里用 `frames::SourcesFrame` 结构体完成（字段不漏；既有两个源的语义不变）。
///
/// v4：`sources.malody` 由 `bridge::malody_source` 组装后**整体覆盖**同一对象——v5 起八个字段
/// （`alive/transport/screen/playing/eventSeq/judge/pro/turbo`）与"只由 24060 POST 写入的 60s 窗口"
/// 合成一条口径：桥 8s 存活 **或** Lua 通道 60s 存活。`malody4/**` 属另一份计划，本步不动
/// 它的签名，故在 JSON 层覆盖这一个对象（其余三个源仍由它组装）。
///
/// `pub(crate)`：poller 在 `alive`/`playing` 跳变时要即时推送一次 state。
/// **调用方不得持有 `shared.malody4` / `shared.malody_bridge` 的 `MutexGuard` 跨越本函数**
/// ——本函数会再 lock 这两个字段，std `Mutex` 不可重入，跨调用持锁会自死锁。
pub(crate) fn state_frame(shared: &Shared) -> serde_json::Value {
    let tosu_online = *shared.tosu_online.lock().unwrap();
    let errors = shared.shell_errors.lock().unwrap().clone();
    let etterna = shared.etterna.lock().unwrap().clone();
    let lua_alive = match *shared.last_malody_post.lock().unwrap() {
        Some(at) if at.elapsed() < Duration::from_secs(60) => true,
        _ => false,
    };
    let malody4 = shared.malody4.lock().unwrap().clone();
    let malody = {
        let state = shared.malody_bridge.lock().unwrap();
        bridge::malody_source(&state, lua_alive)
    };
    let mut value =
        crate::malody4::build_state_frame(tosu_online, &errors, &etterna, lua_alive, &malody4);
    if let Some(sources) = value.get_mut("sources").and_then(|s| s.as_object_mut()) {
        sources.insert(
            "malody".to_string(),
            serde_json::to_value(malody).unwrap_or(serde_json::Value::Null),
        );
    }
    value
}

pub fn hello_frame(shared: &Shared) -> Envelope {
    let tosu_online = *shared.tosu_online.lock().unwrap();
    Envelope::new(
        "hello",
        next_seq(shared),
        Some(serde_json::to_value(HelloFrame {
            tosu_online,
            contract: CONTRACT_VERSION,
        })
        .unwrap_or_default()),
    )
}

/// 按字节哈希（十六进制拼接的唯一实现，复用 `malody4::model::hex16`）。
pub fn md5_hex_bytes(bytes: &[u8]) -> String {
    use md5::{Digest, Md5};
    crate::malody4::model::hex16(Md5::digest(bytes).as_slice())
}

pub fn md5_hex(input: &str) -> String {
    md5_hex_bytes(input.as_bytes())
}

pub fn json_error(text: &str) -> String {
    serde_json::json!({ "error": text }).to_string()
}

pub fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&input[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

pub fn mime_for(path: &str) -> String {
    let lower = path.to_lowercase();
    if lower.ends_with(".html") {
        "text/html".to_string()
    } else if lower.ends_with(".css") {
        "text/css".to_string()
    } else if lower.ends_with(".js") || lower.ends_with(".mjs") {
        "application/javascript".to_string()
    } else if lower.ends_with(".json") {
        "application/json".to_string()
    } else if lower.ends_with(".svg") {
        "image/svg+xml".to_string()
    } else if lower.ends_with(".png") {
        "image/png".to_string()
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg".to_string()
    } else if lower.ends_with(".gif") {
        "image/gif".to_string()
    } else if lower.ends_with(".webp") {
        "image/webp".to_string()
    } else if lower.ends_with(".woff2") {
        "font/woff2".to_string()
    } else if lower.ends_with(".woff") {
        "font/woff".to_string()
    } else if lower.ends_with(".wasm") {
        "application/wasm".to_string()
    } else if lower.ends_with(".ico") {
        "image/x-icon".to_string()
    } else if lower.ends_with(".mp3") {
        "audio/mpeg".to_string()
    } else if lower.ends_with(".ogg") {
        "audio/ogg".to_string()
    } else {
        "application/octet-stream".to_string()
    }
}

pub fn malody_root(shared: &Shared) -> Option<PathBuf> {
    if let Ok(over) = std::env::var("MMA_MALODY_ROOT") {
        if !over.is_empty() {
            return Some(PathBuf::from(config::normalize_path(&over)));
        }
    }
    let offline = shared
        .offline_settings
        .lock()
        .unwrap()
        .get("malodyRoot")
        .cloned();
    if let Some(v) = offline.as_ref().and_then(|v| v.as_str()) {
        if !v.is_empty() {
            return config::config_path(&serde_json::json!({"malodyRoot": v}), "malodyRoot");
        }
    }
    let value = if shared.tosu.is_some() && *shared.tosu_online.lock().unwrap() {
        shared
            .tosu
            .as_ref()
            .map(|info| config::read_tosu_settings(info).get("malodyRoot").cloned())
            .flatten()
    } else {
        None
    };
    if let Some(v) = value.as_ref().and_then(|v| v.as_str()) {
        if !v.is_empty() {
            return config::config_path(&serde_json::json!({"malodyRoot": v}), "malodyRoot");
        }
    }
    config::detect_malody_root()
}

/// Malody 4.3.7 根目录解析链（**唯一权威顺序**）：
///
/// 1. `process_exe` 所在目录（poller 从 `anchor::find_target()` 取得；只要求同目录下存在
///    `malody.exe`，**不做版本校验**——这样"版本不符"才会由 `anchor::open()` 如实产生
///    `target-mismatch:…`，而不是被伪装成"找不到进程"）；
/// 2. `MMA_MALODY4_ROOT`；
/// 3. 壳配置 `malody4Root`（`mma-shell-config.json`）；
/// 4. tosu 设置同键；
/// 5. 启发候选列表（`config::detect_malody4_root`——**唯一做版本校验的一级**，因为它可能
///    撞上 MalodyV / Maupdate 目录）。
///
/// 前四级一律"非空即采纳"，不做任何存在性/版本校验：用户把 `malody4Root` 指到错目录时，
/// 失败会如实落到下游的 `process-not-found` / `chart-not-indexed`，而不是被伪装成"你没配"。
/// `root-not-configured` **只表示整条链走完仍为 `None`**。
pub fn malody4_root(shared: &Shared, process_exe: Option<&Path>) -> Option<PathBuf> {
    if let Some(dir) = process_exe.and_then(|exe| exe.parent()) {
        if dir.join("malody.exe").exists() {
            return Some(dir.to_path_buf());
        }
    }
    if let Ok(over) = std::env::var("MMA_MALODY4_ROOT") {
        if !over.is_empty() {
            return Some(PathBuf::from(config::normalize_path(&over)));
        }
    }
    let offline = shared
        .offline_settings
        .lock()
        .unwrap()
        .get("malody4Root")
        .cloned();
    if let Some(v) = offline.as_ref().and_then(|v| v.as_str()) {
        if !v.is_empty() {
            return config::config_path(&serde_json::json!({"malody4Root": v}), "malody4Root");
        }
    }
    let value = if shared.tosu.is_some() && *shared.tosu_online.lock().unwrap() {
        shared
            .tosu
            .as_ref()
            .map(|info| config::read_tosu_settings(info).get("malody4Root").cloned())
            .flatten()
    } else {
        None
    };
    if let Some(v) = value.as_ref().and_then(|v| v.as_str()) {
        if !v.is_empty() {
            return config::config_path(&serde_json::json!({"malody4Root": v}), "malody4Root");
        }
    }
    config::detect_malody4_root(process_exe)
}

const PING_INTERVAL: Duration = Duration::from_secs(15);
const TOSU_PROBE_INTERVAL: Duration = Duration::from_secs(30);

pub fn spawn_timers(shared: Arc<Shared>) {
    thread::spawn(move || {
        loop {
            thread::sleep(PING_INTERVAL);
            let online = shared.tosu.as_ref().map(config::tosu_online).unwrap_or(false);
            *shared.tosu_online.lock().unwrap() = online;
            // 壳配置（mma-shell-config.json）变化检测：用户直接编辑文件 → 重载并推送 settings 帧。
            let file_cfg = config::read_shell_config();
            let changed = {
                let mut mem = shared.offline_settings.lock().unwrap();
                if *mem != file_cfg {
                    *mem = file_cfg.clone();
                    true
                } else {
                    false
                }
            };
            if changed {
                broadcast(&shared, "settings", Some(file_cfg));
            }
            // mma-settings.json（全量插件设置）变化检测：用户手改 → 推送 settings 帧。
            let plugin_cfg = config::read_plugin_settings();
            if plugin_cfg.is_object() && plugin_cfg != *shared.plugin_settings.lock().unwrap() {
                *shared.plugin_settings.lock().unwrap() = plugin_cfg.clone();
                broadcast(&shared, "settings", Some(plugin_cfg));
            }
            // tosu 设置文件（<插件目录名>.values.json）变化检测：用户在线模式
            // 在 tosu dashboard 改设置 → 文件更新 → 推送 settings 帧（壳窗口
            // 页面即时生效，无需重启）。离线（无 tosu）时跳过。
            if let Some(info) = shared.tosu.as_ref() {
                let tosu_cfg = config::read_tosu_settings(info);
                if tosu_cfg.is_object()
                    && tosu_cfg != *shared.tosu_settings_cache.lock().unwrap()
                {
                    *shared.tosu_settings_cache.lock().unwrap() = tosu_cfg.clone();
                    broadcast(&shared, "settings", Some(tosu_cfg));
                }
            }
            broadcast(&shared, "state", Some(state_frame(&shared)));
            broadcast(&shared, "ping", None);
            thread::sleep(TOSU_PROBE_INTERVAL - PING_INTERVAL);
        }
    });
}

// ---- 入口 ----

pub fn start(plugin_dir: PathBuf, tosu: Option<TosuInfo>) -> Arc<Shared> {
    let shared = new_shared(plugin_dir, tosu);
    {
        let online = shared.tosu.as_ref().map(config::tosu_online).unwrap_or(false);
        *shared.tosu_online.lock().unwrap() = online;
    }
    let listener = match TcpListener::bind("127.0.0.1:24061") {
        Ok(l) => l,
        Err(e) => {
            eprintln!("mma-shell: cannot bind 24061 ({e}) — another instance already running?");
            std::process::exit(2);
        }
    };
    // 24060 / 17653 端口占用**不 panic、不 exit**：记一条 error 日志 + 进 `shell_errors`
    // （页面 status 行可见），其余一切照常——单端口被占不该让整个壳起不来。
    let post_listener = match TcpListener::bind("127.0.0.1:24060") {
        Ok(l) => Some(l),
        Err(e) => {
            log::log_at(
                "error",
                &format!("mma-shell: cannot bind 24060 ({e}) — the Malody editor channel is unavailable"),
            );
            shared
                .shell_errors
                .lock()
                .unwrap()
                .push("Malody 编辑器通道端口 24060 被占用，编辑器分析不可用".to_string());
            None
        }
    };
    let bridge_listener = match TcpListener::bind(("127.0.0.1", BRIDGE_PORT)) {
        Ok(l) => {
            *shared.bridge_listen_ok.lock().unwrap() = true;
            Some(l)
        }
        Err(e) => {
            log::log_at(
                "error",
                &format!(
                    "mma-shell: cannot bind {} ({e}) — the Malody selection bridge is unavailable",
                    BRIDGE_PORT
                ),
            );
            shared
                .shell_errors
                .lock()
                .unwrap()
                .push("Malody 选曲桥端口 17653 被占用，游戏内选曲跟随不可用".to_string());
            None
        }
    };
    http::spawn_http_ws(shared.clone(), listener);
    if let Some(post_listener) = post_listener {
        post::spawn_post(shared.clone(), post_listener);
    }
    if let Some(bridge_listener) = bridge_listener {
        bridge::spawn_bridge(shared.clone(), bridge_listener);
    }
    spawn_timers(shared.clone());
    crate::etterna::spawn_poller(shared.clone());
    crate::malodyv::spawn_malody_poller(shared.clone());
    crate::malody4::spawn_poller(shared.clone());
    shared
}

#[cfg(test)]
#[path = "../../tests-local/server_mod.rs"]
mod tests;
