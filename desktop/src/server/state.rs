// server::state - 领域子状态与解耦后的服务状态中心

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Mutex};
use std::time::Instant;
use crate::config::TosuInfo;
use crate::frames::Envelope;
use crate::server::bridge::MalodyBridgeState;
use crate::server::osu_source::ReaderLiveness;

/// 1. 广播与长连接中心：专门管理 WebSocket 出站连接池与 POST 回调
pub struct BroadcastHub {
    pub seq: AtomicU64,
    pub sinks: Mutex<Vec<(u64, mpsc::Sender<String>)>>,
    pub pending: Mutex<HashMap<String, mpsc::Sender<String>>>,
}

impl Default for BroadcastHub {
    fn default() -> Self {
        Self {
            seq: AtomicU64::new(0),
            sinks: Mutex::new(Vec::new()),
            pending: Mutex::new(HashMap::new()),
        }
    }
}

impl BroadcastHub {
    pub fn next_seq(&self) -> u64 {
        self.seq.fetch_add(1, Ordering::Relaxed) + 1
    }

    pub fn broadcast(&self, frame_type: &str, payload: Option<serde_json::Value>) {
        let env = Envelope {
            seq: self.next_seq(),
            frame_type: frame_type.to_string(),
            payload,
        };
        let text = serde_json::to_string(&env).unwrap_or_default();
        let mut sinks = self.sinks.lock().unwrap();
        sinks.retain(|(_, tx)| tx.send(text.clone()).is_ok());
    }
}

/// 2. 设置与环境中心：管理插件配置、本地壳配置与 tosu.env 状态
pub struct SettingsHub {
    pub plugin_dir: PathBuf,
    pub tosu: Option<TosuInfo>,
    pub tosu_online: Mutex<bool>,
    pub cover_whitelist: Mutex<HashSet<String>>,
    pub offline_settings: Mutex<serde_json::Value>,
    pub plugin_settings: Mutex<serde_json::Value>,
    pub tosu_settings_cache: Mutex<serde_json::Value>,
}

impl SettingsHub {
    pub fn new(plugin_dir: PathBuf, tosu: Option<TosuInfo>, offline: serde_json::Value, plugin: serde_json::Value) -> Self {
        Self {
            plugin_dir,
            tosu,
            tosu_online: Mutex::new(false),
            cover_whitelist: Mutex::new(HashSet::new()),
            offline_settings: Mutex::new(offline),
            plugin_settings: Mutex::new(plugin),
            tosu_settings_cache: Mutex::new(serde_json::Value::Null),
        }
    }
}

/// 3. 游戏观察源注册中心：聚合各源的运行期状态快照
pub struct SourcesRegistry {
    pub etterna: Mutex<crate::etterna::EtternaStatus>,
    pub malody4: Mutex<crate::malody4::Malody4Status>,
    pub malody4_root_cache: Mutex<Option<PathBuf>>,
    pub malody_bridge: Mutex<MalodyBridgeState>,
    pub bridge_listen_ok: Mutex<bool>,
    pub last_malody_post: Mutex<Option<Instant>>,
    pub osu_reader_liveness: Mutex<ReaderLiveness>,
    pub shell_errors: Mutex<Vec<String>>,
}

impl Default for SourcesRegistry {
    fn default() -> Self {
        Self {
            etterna: Mutex::new(crate::etterna::EtternaStatus::default()),
            malody4: Mutex::new(crate::malody4::Malody4Status::default()),
            malody4_root_cache: Mutex::new(None),
            malody_bridge: Mutex::new(MalodyBridgeState::default()),
            bridge_listen_ok: Mutex::new(false),
            last_malody_post: Mutex::new(None),
            osu_reader_liveness: Mutex::new(ReaderLiveness::default()),
            shell_errors: Mutex::new(Vec::new()),
        }
    }
}

/// 4. 窗口桥接中心：解耦 GUI 句柄与后端网络协议栈
#[derive(Default)]
pub struct WindowBridge {
    pub app: Mutex<Option<tauri::AppHandle>>,
    pub window: Mutex<Option<tauri::WebviewWindow>>,
}
