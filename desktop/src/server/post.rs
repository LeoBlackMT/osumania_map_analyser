// 24060 POST：Malody 编辑器分析入口 + resolve 通道（按 title/artist 扫 chart）。
// 逻辑从原 server.rs 拆分。

use crate::config;
use crate::frames::{MalodyPost, MAX_PAYLOAD_BYTES};
use crate::server::{
    broadcast, json_error, log::log_at, md5_hex, malody_root, next_seq, percent_decode, Shared,
    http,
};
use std::fs;
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Instant;

pub fn spawn_post(shared: Arc<Shared>, listener: TcpListener) {
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let shared = shared.clone();
            thread::spawn(move || handle_post(shared, stream));
        }
    });
}

const POST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

fn payload_too_large() -> String {
    "payload too large (>5MB)".to_string()
}
fn timeout_msg() -> String {
    "analysis timeout (30s)".to_string()
}
fn analysis_failed(errors: &[String]) -> String {
    format!("analysis failed: {}", errors.join("; "))
}
fn source_not_active(active: &str) -> String {
    format!("source not active: {}", active)
}

/// 按 title/artist/level/keys 在 {malodyRoot}/chart/ 下递归匹配 .mc（来自编辑器 ChartInfo）。
/// 匹配优先级（防止多难度同标题取错谱）：
///   1. 精确 title+artist（+level/keys 若提供）
///   2. 精确 title + 文件名含 level/keys（难度标签常只在文件名）
///   3. 精确 title（无 level 时取第一个）
///   4. title 含子串 / 文件名含 title（fallback）
fn resolve_malody_chart(shared: &Shared, title: &str, artist: &str, level: &str, keys: u64) -> Option<String> {
    let root = malody_root(shared)?;
    let title_norm = title.trim().to_lowercase();
    let artist_norm = artist.trim().to_lowercase();
    let level_norm = level.trim().to_lowercase();
    let mut walk = vec![root.join("chart")];
    let mut exact: Option<String> = None;
    let mut exact_level: Option<String> = None;
    let mut file_level: Option<String> = None;
    let mut sub: Option<String> = None;
    while let Some(dir) = walk.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk.push(path);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("mc") {
                continue;
            }
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
                continue;
            };
            let t = v
                .pointer("/meta/song/title")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_lowercase();
            let a = v
                .pointer("/meta/song/artist")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_lowercase();
            let fname = path.file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
            let is_exact_title = !title_norm.is_empty() && t == title_norm;
            let is_exact = is_exact_title && (artist_norm.is_empty() || a == artist_norm);
            // 精确 title+artist（+level/keys 若提供）
            if is_exact {
                if exact.is_none() {
                    exact = Some(text.clone());
                }
                // level 精化：ChartInfo level 或文件名难度匹配
                let file_has_level = !level_norm.is_empty()
                    && (fname.contains(&level_norm) || path.to_string_lossy().to_lowercase().contains(&level_norm));
                let keys_ok = keys == 0
                    || v.pointer("/meta/mode_ext/column")
                        .and_then(|x| x.as_u64())
                        .map(|c| c == keys)
                        .unwrap_or(true);
                if file_has_level && keys_ok && exact_level.is_none() {
                    exact_level = Some(text.clone());
                }
            }
            // 文件名含 title（ChartInfo title 可能为空/不准时的兜底）
            if exact.is_none()
                && !title_norm.is_empty()
                && fname.contains(&title_norm)
            {
                if file_level.is_none() {
                    file_level = Some(text.clone());
                }
            }
            // title 含子串（最后 fallback）
            if exact.is_none()
                && !title_norm.is_empty()
                && !t.is_empty()
                && (t.contains(&title_norm) || title_norm.contains(&t))
            {
                if sub.is_none() {
                    sub = Some(text);
                }
            }
        }
    }
    exact_level.or(exact).or(file_level).or(sub)
}

fn handle_post(shared: Arc<Shared>, mut stream: TcpStream) {
    let Some((_head, body)) = http::read_request(&mut stream) else { return };
    log_at("debug", &format!("POST len={}", body.len()));
    // 编辑器 resolve 通道（自动读谱）：按 ChartInfo title/artist 扫 chart 目录。
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&body) {
        if value.get("action").and_then(|v| v.as_str()) == Some("resolve") {
            let title = value.get("title").and_then(|v| v.as_str()).unwrap_or("");
            let artist = value.get("artist").and_then(|v| v.as_str()).unwrap_or("");
            let level = value.get("level").and_then(|v| v.as_str()).unwrap_or("");
            let keys = value.get("keys").and_then(|v| v.as_u64()).unwrap_or(0);
            let chart_path = value.get("path").and_then(|v| v.as_str()).unwrap_or("");
            let root = malody_root(&shared);
            // resolve 请求提到 info：用户排障 Malody 编辑器失败时无需开 debug 就能看到。
            crate::server::log::log_line(&format!(
                "resolve title={} artist={} level={} keys={} path={} malodyRoot={:?}",
                title,
                artist,
                level,
                keys,
                chart_path,
                root
            ));
            // 编辑器给了绝对路径：直接读文件（绕开标题扫描，最可靠）。
            if !chart_path.is_empty() {
                let p = PathBuf::from(config::normalize_path(chart_path));
                if p.is_file() {
                    if let Ok(text) = fs::read_to_string(&p) {
                        log_at("debug", "resolve by path HIT");
                        http::write_response(&mut stream, 200, "application/json", text.as_bytes());
                        return;
                    }
                }
            }
            match resolve_malody_chart(&shared, title, artist, level, keys) {
                Some(text) => {
                    log_at("debug", "resolve HIT (chart text returned)");
                    http::write_response(&mut stream, 200, "application/json", text.as_bytes());
                }
                None => {
                    crate::server::log::log_line("resolve MISS (chart not found)");
                    http::respond_json(&mut stream, 404, r#"{"error":"chart not found in malody chart dir"}"#);
                }
            }
            return;
        }
    }
    let Ok(post) = serde_json::from_str::<MalodyPost>(&body) else {
        http::respond_json(&mut stream, 400, r#"{"error":"bad payload: missing meta or chartText"}"#);
        return;
    };

    if post.chart_text.len() > MAX_PAYLOAD_BYTES {
        http::respond_json(&mut stream, 504, &json_error(&payload_too_large()));
        return;
    }
    if shared.sinks.lock().unwrap().is_empty() {
        http::respond_json(&mut stream, 504, &json_error(&timeout_msg()));
        return;
    }

    *shared.last_malody_post.lock().unwrap() = Some(Instant::now());
    let request_id = format!("m{}", next_seq(&shared));
    let content_md5 = md5_hex(&post.chart_text);
    let keys = if post.meta.keys > 0 { post.meta.keys } else { 4 };
    let version = if post.meta.level.is_empty() {
        "Unknown".to_string()
    } else {
        post.meta.level.clone()
    };
    let identity = format!(
        "mdy:{}:{}:{}:{}",
        if post.meta.title.is_empty() {
            "untitled"
        } else {
            &post.meta.title
        },
        version,
        keys,
        content_md5
    );
    let song = serde_json::json!({
        "requestId": request_id,
        "source": "malody",
        "identity": identity,
        "modData": { "speedRate": "1.0" },
        "meta": {
            "title": post.meta.title,
            "artist": post.meta.artist,
            "version": version,
            "keys": keys,
            "devMsd8": [],
        },
        "cover": null,
        "rawText": post.chart_text,
    });
    broadcast(&shared, "song", Some(song));

    let (tx, rx) = mpsc::channel::<String>();
    shared.pending.lock().unwrap().insert(request_id, tx);

    match rx.recv_timeout(POST_TIMEOUT) {
        Ok(result_text) => {
            if let Some(inbound) = crate::server::ws::parse_result_payload(&result_text) {
                let hint = inbound.status_hint.unwrap_or_default();
                let errors = inbound.errors.unwrap_or_default();
                match hint.as_str() {
                    "analysis-failed" => {
                        http::respond_json(&mut stream, 500, &json_error(&analysis_failed(&errors)))
                    }
                    "routing-reject" => {
                        let active = inbound.active_source.unwrap_or_default();
                        http::respond_json(&mut stream, 504, &json_error(&source_not_active(&active)))
                    }
                    _ => http::write_response(&mut stream, 200, "application/json", b"{}"),
                }
            } else {
                http::respond_json(&mut stream, 500, &json_error(&timeout_msg()));
            }
        }
        Err(_) => http::respond_json(&mut stream, 504, &json_error(&timeout_msg())),
    }
}

// 保留 percent_decode 引用（http 模块使用；此处仅为模块一致性）。
#[allow(unused_imports)]
use percent_decode as _pd;
