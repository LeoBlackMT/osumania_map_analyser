// 壳服务核心（契约 §0–§9 的直接实现）：
// 24061 单 listener = 手写迷你 HTTP（静态 /settings /cover）+ WS(/ws)；
// 24060 = Malody POST → requestId → song 帧 → result 应答（两来源映射）。

use crate::config::{self, TosuInfo};
use crate::frames::*;
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
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
    /// Malody 最近 POST 时间（60s 存活窗口）。
    pub last_malody_post: Mutex<Option<Instant>>,
    pub tosu_online: Mutex<bool>,
    /// 壳侧推送错误面（state.errors，页面 status 行展示）。
    pub shell_errors: Mutex<Vec<String>>,
    /// Etterna 桥状态（poller 更新）。
    pub etterna: Mutex<crate::etterna::EtternaStatus>,
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
        offline_settings: Mutex::new(serde_json::json!({})),
        last_malody_post: Mutex::new(None),
        tosu_online: Mutex::new(false),
        shell_errors: Mutex::new(Vec::new()),
        etterna: Mutex::new(crate::etterna::EtternaStatus::default()),
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

// ---- 24061 单 listener：HTTP + WS ----

fn spawn_http_ws(shared: Arc<Shared>, listener: TcpListener) {
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let shared = shared.clone();
            thread::spawn(move || {
                let mut stream = stream;
                // 先 peek 探测 WS 升级：peek 不消费数据，HTTP 路径随后完整读。
                let is_ws = probe_is_ws(&stream);
                if is_ws {
                    handle_ws(shared, stream);
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
/// 单次读取无预读歧义（peek/克隆都会吞数据）；无 body 请求在 head 完整后立即返回。
fn read_request(stream: &mut TcpStream) -> Option<(String, String)> {
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
    // body 超限即切断（>5MB 在调用处判 504），防止无限增长。
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

// 原 read_body 已并入 read_request。

fn write_response(stream: &mut TcpStream, code: u16, ctype: &str, body: &[u8]) {
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

fn respond_json(stream: &mut TcpStream, code: u16, body: &str) {
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
        // 契约 §6：在线 == tosu 权威，壳绝不写；离线才接受变更。
        if shared.tosu.is_some() && *shared.tosu_online.lock().unwrap() {
            respond_json(&mut stream, 403, r#"{"error":"tosu online: settings are read-only"}"#);
            return;
        }
        let body = body.to_string();
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&body) {
            *shared.offline_settings.lock().unwrap() = value;
            respond_json(&mut stream, 200, "{}");
        } else {
            respond_json(&mut stream, 400, r#"{"error":"invalid settings json"}"#);
        }
        return;
    }

    if path == "/settings" {
        let settings = if shared.tosu.is_some() {
            shared
                .tosu
                .as_ref()
                .map(config::read_tosu_settings)
                .unwrap_or(serde_json::Value::Null)
        } else {
            shared.offline_settings.lock().unwrap().clone()
        };
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
                "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
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

fn handle_ws(shared: Arc<Shared>, stream: TcpStream) {
    // 出站排水依赖周期性唤醒：read timeout 让阻塞的 read 周期返回。
    // （否则 drain 被 read 阻塞饿死，song/state 帧卡在 channel。）
    let _ = stream.set_read_timeout(Some(Duration::from_millis(100)));
    let Ok(mut ws) = tungstenite::accept_hdr(
        stream,
        |req: &tungstenite::handshake::server::Request,
         resp: tungstenite::handshake::server::Response| {
        // 仅接受 loopback Origin（契约 §0）。
        let origin_ok = req
            .headers()
            .get("Origin")
            .map(|v| {
                let s = v.to_str().unwrap_or("");
                s.contains("127.0.0.1") || s.contains("localhost")
            })
            .unwrap_or(true);
        if origin_ok {
            Ok(resp)
        } else {
            Err(tungstenite::http::Response::builder()
                .status(403)
                .body(Some("forbidden".to_string()))
                .unwrap())
        }
    }) else {
        return;
    };
    let (tx, rx) = mpsc::channel::<String>();
    let conn_id = next_seq(&shared);
    shared.sinks.lock().unwrap().push((conn_id, tx.clone()));
    let _ = ws.send(tungstenite::Message::Text(
        serde_json::to_string(&hello_frame(&shared)).unwrap_or_default(),
    ));
    // 单线程事件循环：出站排水（try_recv）+ 入站匹配 pending。
    loop {
        while let Ok(text) = rx.try_recv() {
            if ws.send(tungstenite::Message::Text(text)).is_err() {
                shared
                    .sinks
                    .lock()
                    .unwrap()
                    .retain(|(id, _s)| *id != conn_id);
                return;
            }
        }
        match ws.read() {
            Err(tungstenite::Error::Io(ref e))
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                continue; // 排水循环唤醒点
            }
            Ok(tungstenite::Message::Text(text)) => {
                // result 帧按 Envelope 包裹（payload 内层）；兼容裸帧两种情况。
                if let Some(inbound) = parse_result_payload(&text) {
                    if let Some(rid) = inbound.request_id {
                        if let Some(tx) = shared.pending.lock().unwrap().remove(&rid) {
                            let _ = tx.send(text);
                        }
                    }
                }
            }
            Ok(tungstenite::Message::Close(_)) | Err(_) => break,
            _ => {}
        }
    }
    shared
        .sinks
        .lock()
        .unwrap()
        .retain(|(id, _s)| *id != conn_id);
}

// ---- 24060 POST（契约 §3/§4）----

fn spawn_post(shared: Arc<Shared>, listener: TcpListener) {
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let shared = shared.clone();
            thread::spawn(move || handle_post(shared, stream));
        }
    });
}

fn handle_post(shared: Arc<Shared>, mut stream: TcpStream) {
    let Some((_head, body)) = read_request(&mut stream) else { return };
    let Ok(post) = serde_json::from_str::<MalodyPost>(&body) else {
        respond_json(&mut stream, 400, r#"{"error":"bad payload: missing meta or chartText"}"#);
        return;
    };

    if post.chart_text.len() > MAX_PAYLOAD_BYTES {
        respond_json(&mut stream, 504, &json_error(payload_too_large()));
        return;
    }
    if shared.sinks.lock().unwrap().is_empty() {
        // 无页面连接（壳已开但页面未连 /ws）→ 立即超时，不挂 30s。
        respond_json(&mut stream, 504, &json_error(timeout_msg()));
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
        "modData": { "speedRate": "1.0", "odFlag": "none", "cvtFlag": "none", "classic": 0 },
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
            if let Some(inbound) = parse_result_payload(&result_text) {
                let hint = inbound.status_hint.unwrap_or_default();
                let errors = inbound.errors.unwrap_or_default();
                match hint.as_str() {
                    "analysis-failed" => {
                        respond_json(&mut stream, 500, &json_error(&analysis_failed(&errors)))
                    }
                    "routing-reject" => {
                        let active = inbound.active_source.unwrap_or_default();
                        respond_json(&mut stream, 504, &json_error(&source_not_active(&active)))
                    }
                    _ => write_response(&mut stream, 200, "application/json", b"{}"),
                }
            } else {
                respond_json(&mut stream, 500, &json_error(timeout_msg()));
            }
        }
        Err(_) => respond_json(&mut stream, 504, &json_error(timeout_msg())),
    }
}

/// 从 result 帧（Envelope 包裹或裸帧）提取 ResultInbound（契约 §2 payload 载体）。
fn parse_result_payload(text: &str) -> Option<ResultInbound> {
    let value = serde_json::from_str::<serde_json::Value>(text).ok()?;
    let payload = value
        .get("payload")
        .cloned()
        .unwrap_or(value);
    serde_json::from_value::<ResultInbound>(payload).ok()
}

fn json_error(text: &str) -> String {
    serde_json::json!({ "error": text }).to_string()
}

// ---- 周期器（tosu 重探测 30s + ping 15s）----

pub fn spawn_timers(shared: Arc<Shared>) {
    thread::spawn(move || {
        loop {
            thread::sleep(PING_INTERVAL);
            let online = shared.tosu.as_ref().map(config::tosu_online).unwrap_or(false);
            *shared.tosu_online.lock().unwrap() = online;
            broadcast(&shared, "state", Some(state_frame(&shared)));
            broadcast(&shared, "ping", None);
            thread::sleep(TOSU_PROBE_INTERVAL - PING_INTERVAL);
        }
    });
}

// ---- 入口（无窗口版；窗口 wrapper 后续复用 start() 返回的 Shared）----

pub fn start(plugin_dir: PathBuf, tosu: Option<TosuInfo>) -> Arc<Shared> {
    let shared = new_shared(plugin_dir, tosu);
    {
        let online = shared.tosu.as_ref().map(config::tosu_online).unwrap_or(false);
        *shared.tosu_online.lock().unwrap() = online;
    }
    let listener = TcpListener::bind("127.0.0.1:24061").unwrap();
    let post_listener = TcpListener::bind("127.0.0.1:24060").unwrap();
    spawn_http_ws(shared.clone(), listener);
    spawn_post(shared.clone(), post_listener);
    spawn_timers(shared.clone());
    crate::etterna::spawn_poller(shared.clone());
    shared
}

fn percent_decode(input: &str) -> String {
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

fn mime_for(path: &str) -> String {
    match path
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript",
        "css" => "text/css",
        "json" => "application/json",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "wasm" => "application/wasm",
        "txt" => "text/plain; charset=utf-8",
        "woff" | "woff2" => "font/woff2",
        "otf" => "font/otf",
        "ico" => "image/x-icon",
        _ => "application/octet-stream",
    }
    .to_string()
}