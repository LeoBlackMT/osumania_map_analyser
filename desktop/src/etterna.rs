// Etterna 桥轮询（M3）：2Hz 读 Save/LeosMmaBridge.txt / LeosMmaGameplay.txt。
//
// - bridge 变化（换歌/改 rate 的 key 门控写入）→ 组装 song 帧推送
//   （identity = ett:{stem}:{difficulty}:{meter}:{contentMd5}；modData.speedRate
//   = rate；meta.devMsd8 = 桥 msd×8 仅开发对照；cover 进白名单并同帧下发 URL）
// - gameplay 变化 → playing 外推过期（lastWrite + total_seconds/rate×1.2 + 30s）
// - Etterna 根：MMA_ETTERNA_ROOT 覆盖 → 设置（在线=只读 tosu 设置文件 / 离线=壳 JSON 的 etternaRoot）

use crate::config;
use crate::frames::MAX_PAYLOAD_BYTES;
use crate::server::{broadcast, Shared};
use std::fs;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, SystemTime};

const POLL_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Clone, Default)]
pub struct EtternaStatus {
    pub alive: bool,
    pub playing: bool,
    pub playing_expire_at: Option<u64>,
}

/// Etterna 根目录：env 覆盖 → offline_settings（=mma-shell.json，可直接编辑）→ tosu 在线只读。
pub fn etterna_root(shared: &Shared) -> Option<PathBuf> {
    if let Ok(over) = std::env::var("MMA_ETTERNA_ROOT") {
        if !over.is_empty() {
            return Some(PathBuf::from(over));
        }
    }
    let offline = shared
        .offline_settings
        .lock()
        .unwrap()
        .get("etternaRoot")
        .cloned();
    if let Some(v) = offline.as_ref().and_then(|v| v.as_str()) {
        if !v.is_empty() {
            return Some(PathBuf::from(v));
        }
    }
    let value = if shared.tosu.is_some() && *shared.tosu_online.lock().unwrap() {
        shared
            .tosu
            .as_ref()
            .map(|info| config::read_tosu_settings(info).get("etternaRoot").cloned())
            .flatten()
    } else {
        None
    };
    if let Some(v) = value.as_ref().and_then(|v| v.as_str()) {
        if !v.is_empty() {
            return Some(PathBuf::from(v));
        }
    }
    config::detect_etterna_root()
}

fn read_kv(text: &str) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    for line in text.lines() {
        if let Some((k, v)) = line.split_once('=') {
            out.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    out
}

fn md5_hex(input: &str) -> String {
    use md5::{Digest, Md5};
    let mut hasher = Md5::new();
    hasher.update(input.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect()
}

fn mtime_of(path: &std::path::Path) -> Option<SystemTime> {
    fs::metadata(path).ok().and_then(|m| m.modified().ok())
}

pub fn spawn_poller(shared: std::sync::Arc<Shared>) {
    thread::spawn(move || {
        let mut bridge_seen = false;
        let mut bridge_sig: Option<(SystemTime, String)> = None;
        let mut gameplay_sig: Option<(SystemTime, String)> = None;
        loop {
            thread::sleep(POLL_INTERVAL);
            let Some(root) = etterna_root(&shared) else { continue };
            let save = root.join("Save");
            let bridge_path = save.join("LeosMmaBridge.txt");
            let gameplay_path = save.join("LeosMmaGameplay.txt");

            // ── gameplay：playing 外推 ──
            if let (Some(mt), Ok(text)) = (
                mtime_of(&gameplay_path),
                fs::read_to_string(&gameplay_path),
            ) {
                let sig = (mt, text.clone());
                if gameplay_sig.as_ref() != Some(&sig) {
                    gameplay_sig = Some(sig);
                    let kv = read_kv(&text);
                    let playing = kv.get("playing").map(|v| v == "1").unwrap_or(false);
                    let rate = kv
                        .get("rate")
                        .and_then(|v| v.parse::<f64>().ok())
                        .unwrap_or(1.0);
                    let total = kv
                        .get("total_seconds")
                        .and_then(|v| v.parse::<f64>().ok())
                        .unwrap_or(0.0);
                    let expire = if playing {
                        let base = mt
                            .duration_since(SystemTime::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis() as u64;
                        Some(base + ((total / rate.max(0.01) * 1.2) as u64) + 30_000)
                    } else {
                        None
                    };
                    let current = shared.etterna.lock().unwrap().clone();
                    *shared.etterna.lock().unwrap() = EtternaStatus {
                        playing,
                        playing_expire_at: expire,
                        alive: bridge_seen || bridge_path.exists(),
                        ..current
                    };
                }
            }

            // ── bridge：换歌/改 rate → song 帧 ──
            if let (Some(mt), Ok(text)) = (
                mtime_of(&bridge_path),
                fs::read_to_string(&bridge_path),
            ) {
                let sig = (mt, text.clone());
                let changed = bridge_sig.as_ref() != Some(&sig);
                bridge_sig = Some(sig);
                bridge_seen = true;
                if changed {
                    crate::server::log_line(&format!(
                        "etterna bridge changed (root={}, {} bytes)",
                        root.display(),
                        text.len()
                    ));
                    if let Some(song) = build_song_frame(&shared, &root, &text) {
                        broadcast(&shared, "song", Some(song));
                        crate::server::log_line("etterna song frame broadcast");
                    } else {
                        crate::server::log_line("etterna bridge parse FAILED (no song frame)");
                    }
                }
            }

            // alive 与 state 同步（timers 周期推送 state，poller 仅更新字段）
            {
                let mut st = shared.etterna.lock().unwrap();
                st.alive = bridge_seen && bridge_path.exists();
            }
        }
    });
}

fn build_song_frame(
    shared: &Shared,
    root: &std::path::Path,
    text: &str,
) -> Option<serde_json::Value> {
    let kv = read_kv(text);
    let step_file = kv.get("step_file")?;
    let song_dir = kv.get("song_dir").cloned().unwrap_or_default();
    // 谱面绝对路径：Etterna 的 song_dir 以 "Songs/.../" 形式返回。
    let chart_path = root.join(song_dir).join(step_file);
    let raw_text = fs::read_to_string(&chart_path).ok()?;
    if raw_text.len() > MAX_PAYLOAD_BYTES {
        shared.shell_errors.lock().unwrap().push(format!(
            "Etterna 谱面过大被跳过：{}（>5MB 字节）",
            step_file
        ));
        return None;
    }
    let stem = std::path::Path::new(step_file)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| step_file.clone());
    let difficulty = kv.get("difficulty").cloned().unwrap_or_default();
    let meter = kv.get("meter").cloned().unwrap_or_default();
    let content_md5 = md5_hex(&raw_text);
    let identity = format!("ett:{}:{}:{}:{}", stem, difficulty, meter, content_md5);
    let rate = kv.get("rate").cloned().unwrap_or_else(|| "1.0".to_string());

    // 封面：GetBackgroundPath 上报 → 白名单（绝对路径）→ 同帧 URL
    let cover = kv.get("cover").filter(|c| !c.is_empty()).and_then(|c| {
        let abs = root.join(c);
        if !abs.is_file() {
            return None;
        }
        let url = format!(
            "/cover/{}",
            percent_encode(&abs.to_string_lossy().to_string())
        );
        shared.cover_whitelist.lock().unwrap().insert(abs.to_string_lossy().to_string());
        Some(serde_json::json!({ "path": c, "url": url }))
    });

    let dev_msd8: Vec<f64> = (1..=8)
        .map(|i| kv.get(&format!("msd_{}", i)).and_then(|v| v.parse().ok()).unwrap_or(0.0))
        .collect();

    Some(serde_json::json!({
        "requestId": null,
        "source": "etterna",
        "identity": identity,
        "modData": { "speedRate": rate, "odFlag": "none", "cvtFlag": "none", "classic": 0 },
        "meta": {
            "title": kv.get("title").cloned().unwrap_or_default(),
            "artist": kv.get("artist").cloned().unwrap_or_default(),
            "version": format!("{} {}", difficulty, meter),
            "keys": 0,
            "devMsd8": dev_msd8,
        },
        "cover": cover,
        "rawText": raw_text,
    }))
}

fn percent_encode(input: &str) -> String {
    let mut out = String::new();
    for b in input.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' | b':' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}