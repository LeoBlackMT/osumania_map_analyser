// 壳服务核心：Shared 状态、帧封装、启动与定时器。
// 拆分：mod.rs（状态/帧/启动/timers）+ http.rs（24061 静态/settings/cover）
//       + ws.rs（/ws 帧循环）+ post.rs（24060 POST/resolve/control）+ log.rs（日志）。

pub mod http;
pub mod log;
pub mod post;
pub mod ws;

use crate::config::{self, TosuInfo};
use crate::frames::*;
use std::collections::HashMap;
use std::net::TcpListener;
use std::path::PathBuf;
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
    /// Malody 最近 POST 时间（60s 存活窗口）。
    pub last_malody_post: Mutex<Option<Instant>>,
    pub tosu_online: Mutex<bool>,
    /// 壳侧推送错误面（state.errors，页面 status 行展示）。
    pub shell_errors: Mutex<Vec<String>>,
    /// Etterna 桥状态（poller 更新）。
    pub etterna: Mutex<crate::etterna::EtternaStatus>,
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
        last_malody_post: Mutex::new(None),
        tosu_online: Mutex::new(false),
        shell_errors: Mutex::new(Vec::new()),
        etterna: Mutex::new(crate::etterna::EtternaStatus::default()),
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

fn state_frame(shared: &Shared) -> serde_json::Value {
    let tosu_online = *shared.tosu_online.lock().unwrap();
    let errors = shared.shell_errors.lock().unwrap().clone();
    let etterna = shared.etterna.lock().unwrap().clone();
    let malody_alive = match *shared.last_malody_post.lock().unwrap() {
        Some(at) if at.elapsed() < Duration::from_secs(60) => true,
        _ => false,
    };
    serde_json::json!({
        "tosuOnline": tosu_online,
        "errors": errors,
        "sources": {
            "etterna": {
                "alive": etterna.alive,
                "playing": etterna.playing,
                "playingExpireAt": etterna.playing_expire_at,
            },
            "malody": { "alive": malody_alive },
        },
    })
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

pub fn md5_hex(input: &str) -> String {
    use md5::{Digest, Md5};
    let mut hasher = Md5::new();
    hasher.update(input.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect()
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
    let post_listener = TcpListener::bind("127.0.0.1:24060").unwrap();
    http::spawn_http_ws(shared.clone(), listener);
    post::spawn_post(shared.clone(), post_listener);
    spawn_timers(shared.clone());
    crate::etterna::spawn_poller(shared.clone());
    crate::malody::spawn_malody_poller(shared.clone());
    shared
}
