// 24061 单 listener：HTTP（静态 /settings /cover）+ WS 分发。
// 逻辑从原 server.rs 拆分；帧循环在 ws.rs。

use crate::config;
use std::sync::Arc;
use crate::frames::MAX_PAYLOAD_BYTES;
use crate::server::{mime_for, percent_decode, Shared, ws};
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

pub fn spawn_http_ws(shared: Arc<Shared>, listener: TcpListener) {
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let shared = shared.clone();
            thread::spawn(move || {
                let mut stream = stream;
                // 先 peek 探测 WS 升级：peek 不消费数据，HTTP 路径随后完整读。
                let is_ws = probe_is_ws(&stream);
                if is_ws {
                    ws::handle_ws(shared, stream);
                } else if let Some((head, body)) = read_request(&mut stream) {
                    handle_http(shared, stream, &head, &body);
                }
            });
        }
    });
}

/// peek 前 1KB 判断是否 WS 升级请求（peek 不消费；accept_hdr 需要原文在流中）。
fn probe_is_ws(stream: &TcpStream) -> bool {
    let mut probe = [0u8; 1024];
    for _ in 0..250 {
        match stream.peek(&mut probe) {
            Ok(0) => std::thread::sleep(Duration::from_millis(20)),
            Ok(n) => {
                return String::from_utf8_lossy(&probe[..n])
                    .to_ascii_lowercase()
                    .contains("upgrade: websocket");
            }
            Err(_) => return false,
        }
    }
    false
}

/// 读完整请求（head + body）：先读至 \r\n\r\n，再按 Content-Length 精确读 body。
pub fn read_request(stream: &mut TcpStream) -> Option<(String, String)> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 8192];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        if buf.len() > 64 * 1024 {
            return None;
        }
        let n = stream.read(&mut tmp).ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&tmp[..n]);
    }
    let head_end = buf.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let content_length = parse_content_length(&head);
    let mut body = String::from_utf8_lossy(&buf[head_end..]).to_string();
    while body.len() < content_length && body.len() <= MAX_PAYLOAD_BYTES {
        let n = stream.read(&mut tmp).ok()?;
        if n == 0 {
            break;
        }
        body.push_str(&String::from_utf8_lossy(&tmp[..n]));
    }
    Some((head, body))
}

fn parse_content_length(head: &str) -> usize {
    head.lines()
        .find(|l| l.to_ascii_lowercase().starts_with("content-length:"))
        .and_then(|l| l.split(':').nth(1))
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(0)
}

pub fn write_response(stream: &mut TcpStream, code: u16, ctype: &str, body: &[u8]) {
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        code,
        status_text(code),
        ctype,
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
}

pub fn respond_json(stream: &mut TcpStream, code: u16, body: &str) {
    write_response(stream, code, "application/json", body.as_bytes());
}

fn status_text(code: u16) -> &'static str {
    match code {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        500 => "Internal Server Error",
        504 => "Gateway Timeout",
        _ => "Unknown",
    }
}

fn handle_http(shared: Arc<Shared>, mut stream: TcpStream, head: &str, body: &str) {
    let mut lines = head.lines();
    let request_line = lines.next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("GET").to_string();
    let url = parts.next().unwrap_or("/");
    let path = url.split('?').next().unwrap_or("").to_string();

    if method == "POST" && path == "/settings" {
        // 在线（tosu 存活）→ 只读拒绝；离线 → 全量写 mma-settings.json。
        if shared.tosu.is_some() && *shared.tosu_online.lock().unwrap() {
            respond_json(&mut stream, 403, r#"{"error":"tosu online: settings are read-only"}"#);
            return;
        }
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(body) {
            if value.is_object() {
                // 页面设置变更 → 落盘 mma-settings.json（插件设置全量；壳配置键
                // etternaRoot 等不属于此文件——用户在 mma-shell-config.json 手改）。
                let _ = config::write_plugin_settings(&value);
                respond_json(&mut stream, 200, "{}");
            } else {
                respond_json(&mut stream, 400, r#"{"error":"settings must be an object"}"#);
            }
        } else {
            respond_json(&mut stream, 400, r#"{"error":"invalid settings json"}"#);
        }
        return;
    }

    if path == "/settings" {
        // 优先级：tosu 在线设置文件 > tosu 设置文件（离线）> mma-settings.json >
        // settings.json 生成默认。见 config::resolve_plugin_settings。
        let settings = config::resolve_plugin_settings(&shared);
        let body = serde_json::to_string(&settings).unwrap_or_default();
        write_response(&mut stream, 200, "application/json", body.as_bytes());
        return;
    }

    if path == "/cover" || path.starts_with("/cover/") {
        let rel = percent_decode(path.trim_start_matches("/cover/"));
        let allowed = shared.cover_whitelist.lock().unwrap().contains(&rel);
        if !allowed {
            respond_json(&mut stream, 404, r#"{"error":"cover not whitelisted"}"#);
            return;
        }
        match fs::read(&rel) {
            Ok(bytes) => {
                let ctype = mime_for(&rel);
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n",
                    ctype,
                    bytes.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&bytes);
            }
            Err(_) => respond_json(&mut stream, 404, r#"{"error":"cover not found"}"#),
        }
        return;
    }

    // 静态：插件目录（防穿越）
    if method != "GET" && method != "HEAD" {
        respond_json(&mut stream, 405, r#"{"error":"method not allowed"}"#);
        return;
    }
    let rel = path.trim_start_matches('/');
    let rel = if rel.is_empty() { "index.html" } else { rel };
    let base = shared.plugin_dir.canonicalize().unwrap_or_else(|_| shared.plugin_dir.clone());
    let candidate = shared.plugin_dir.join(rel);
    let candidate = candidate.canonicalize().unwrap_or(candidate);
    if !candidate.starts_with(&base) || !candidate.is_file() {
        respond_json(&mut stream, 404, r#"{"error":"not found"}"#);
        return;
    }
    match fs::read(&candidate) {
        Ok(bytes) => {
            let ctype = mime_for(rel);
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
                ctype,
                bytes.len()
            );
            let _ = stream.write_all(head.as_bytes());
            if method != "HEAD" {
                let _ = stream.write_all(&bytes);
            }
        }
        Err(_) => respond_json(&mut stream, 404, r#"{"error":"not found"}"#),
    }
}
