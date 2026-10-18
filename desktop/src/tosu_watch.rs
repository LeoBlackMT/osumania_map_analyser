// tosu v2 WebSocket 观察器：只缓存 `resultsScreen.hits`。
//
// 为什么需要它：lazer 的原生内存读取没有结算 hits（`result_hits` 只在 stable.rs 产生），
// 而反馈签名要求客观结算数据。tosu 在跑时它已经在读 lazer 的结算屏——这里复用它，
// 只取 hits 一个字段，其余字段仍以壳自己的原生快照为准。
//
// 信任边界：tosu 是用户本机的进程、只经 127.0.0.1；这条兜底只在 **lazer + 原生没有
// hits** 时启用（stable 永远走原生）。页面无法影响这里的数据。

use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use serde_json::{Map, Value};
use tungstenite::connect;

use crate::osu::model::Hits;

static LATEST_HITS: OnceLock<Mutex<Option<Value>>> = OnceLock::new();

fn cell() -> &'static Mutex<Option<Value>> {
    LATEST_HITS.get_or_init(|| Mutex::new(None))
}

/// 启动后台观察线程（幂等；断线 2s 后重连）。
pub fn start(host: &str, port: u16) {
    static STARTED: OnceLock<()> = OnceLock::new();
    if STARTED.set(()).is_err() {
        return;
    }
    let url = format!("ws://{host}:{port}/websocket/v2");
    thread::spawn(move || loop {
        if let Ok((mut socket, _)) = connect(&url) {
            loop {
                let message = match socket.read() {
                    Ok(message) => message,
                    Err(_) => break,
                };
                if !message.is_text() {
                    continue;
                }
                let Ok(value) = serde_json::from_str::<Value>(message.to_text().unwrap_or("")) else {
                    continue;
                };
                let state_name = value
                    .get("state")
                    .and_then(|state| state.get("name"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let mut slot = cell().lock().unwrap();
                if state_name != "resultScreen" {
                    *slot = None;
                    continue;
                }
                if let Some(hits) = value
                    .get("resultsScreen")
                    .and_then(|screen| screen.get("hits"))
                    .filter(|hits| hits.is_object())
                {
                    *slot = Some(hits.clone());
                }
            }
        }
        thread::sleep(Duration::from_secs(2));
    });
}

fn cached_hits() -> Option<Value> {
    cell().lock().unwrap().clone()
}

fn count(hits: &Map<String, Value>, key: &str) -> u32 {
    hits.get(key).and_then(Value::as_u64).unwrap_or(0).min(u32::MAX as u64) as u32
}

/// tosu 的结算 hits → 壳的 `Hits`（六键；tosu v2 也带 sliderBreaks 等，这里不用）。
pub fn native_hits() -> Option<Hits> {
    let value = cached_hits()?;
    let hits = value.as_object()?;
    let parsed = Hits {
        n300: count(hits, "300"),
        n100: count(hits, "100"),
        n50: count(hits, "50"),
        geki: count(hits, "geki"),
        katu: count(hits, "katu"),
        miss: count(hits, "0"),
    };
    let total = parsed.n300 as u64 + parsed.n100 as u64 + parsed.n50 as u64
        + parsed.geki as u64 + parsed.katu as u64 + parsed.miss as u64;
    (total > 0).then_some(parsed)
}

/// 把 tosu 的 hits 合并进原生包（仅 lazer + 原生无 hits + 结算屏）。
pub fn merge_into_packet(packet: &mut Value, snapshot: &crate::osu::model::Snapshot) {
    if snapshot.client != Some(crate::osu::model::Client::Lazer)
        || snapshot.result_hits.is_some()
        || snapshot.state_name.as_deref() != Some("resultScreen")
    {
        return;
    }
    let Some(hits) = cached_hits() else { return };
    let Some(root) = packet.as_object_mut() else { return };
    let screen = root
        .entry("resultsScreen".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if let Some(screen) = screen.as_object_mut() {
        screen.insert("hits".to_string(), hits);
    }
}
