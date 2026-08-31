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
            *shared.offline_settings.lock().unwrap() = value.clone();
            // 落盘 mma-shell.json（无 tosu 用户也可直接编辑该文件）。
            config::write_shell_config(&value);
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
                // control 帧（契约 v2）：页面 → 壳窗口控制。
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
                    if value.get("type").and_then(|v| v.as_str()) == Some("control") {
                        if let Some(payload) = value.get("payload") {
                            if let Ok(ctrl) = serde_json::from_value::<crate::frames::ControlInbound>(
                                payload.clone(),
                            ) {
                                handle_control(&shared, &ctrl);
                            }
                        }
                        continue;
                    }
                }
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

/// 诊断日志（exe 旁 mma-shell.log，追加）。
pub fn log_line(msg: &str) {
    use std::io::Write;
    let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
    else {
        return;
    };
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(dir.join("mma-shell.log")) {
        let _ = writeln!(f, "{}", msg);
    }
}

fn handle_post(shared: Arc<Shared>, mut stream: TcpStream) {
    let Some((_head, body)) = read_request(&mut stream) else { return };
    log_line(&format!("POST len={}", body.len()));
    // 编辑器 resolve 通道（自动读谱）：按 ChartInfo title/artist 扫 chart 目录，
    // 命中即把 .mc 原文作为响应返回（纯文本，插件直接 POST 分析）。
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&body) {
        if value.get("action").and_then(|v| v.as_str()) == Some("resolve") {
            let title = value.get("title").and_then(|v| v.as_str()).unwrap_or("");
            let artist = value.get("artist").and_then(|v| v.as_str()).unwrap_or("");
            let root = malody_root(&shared);
            log_line(&format!(
                "resolve title={} artist={} malodyRoot={:?}",
                title,
                artist,
                root
            ));
            match resolve_malody_chart(&shared, title, artist) {
                Some(text) => {
                    log_line("resolve HIT (chart text returned)");
                    write_response(&mut stream, 200, "application/json", text.as_bytes())
                }
                None => {
                    log_line("resolve MISS");
                    respond_json(&mut stream, 404, r#"{"error":"chart not found in malody chart dir"}"#);
                }
            }
            return;
        }
    }
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
            if let Some(inbound) = parse_result_payload(&result_text) {
                // mma_state 写入：仅 activeSource=malody 且 errors 为空（契约 §9）。
                write_mma_state(&shared, &result_text);
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

/// 契约 v2 control 帧处理（窗口操控）。
fn handle_control(shared: &Shared, ctrl: &crate::frames::ControlInbound) {
    let Some(window) = shared.window.lock().unwrap().clone() else { return };
    match ctrl.action.as_str() {
        "close" => {
            let _ = window.close();
        }
        "alwaysOnTop" => {
            let _ = window.set_always_on_top(ctrl.value.unwrap_or(true));
        }
        "clickThrough" => {
            let _ = window.set_ignore_cursor_events(ctrl.value.unwrap_or(true));
        }
        "dragStart" => {
            // 顶部把手拖动（HTML drag region 在透明窗口部分环境不工作，改走该确定性路径）。
            let window2 = window.clone();
            let _ = window2.start_dragging();
        }
        _ => {
            // 未知动作静默忽略
        }
    }
}

/// 按 title/artist 在 {malodyRoot}/chart/ 下递归匹配 .mc（来自编辑器 ChartInfo）。
fn resolve_malody_chart(shared: &Shared, title: &str, artist: &str) -> Option<String> {
    let root = malody_root(shared)?;
    let title_norm = title.trim().to_lowercase();
    let artist_norm = artist.trim().to_lowercase();
    let mut walk = vec![root.join("chart")];
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
            let title_hit = title_norm.is_empty()
                || (!t.is_empty() && (t.contains(&title_norm) || title_norm.contains(&t)));
            let artist_hit = artist_norm.is_empty() || a.contains(&artist_norm);
            if title_hit && artist_hit {
                return Some(text);
            }
        }
    }
    None
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

// ---- skin 状态文件（契约 §9：maody 源 + 哨兵定位 + 原子写）----

/// Malody 根：MMA_MALODY_ROOT 覆盖 → mma-shell.json/离线设置（malodyRoot）→ tosu 在线只读。
pub fn malody_root(shared: &Shared) -> Option<PathBuf> {
    if let Ok(over) = std::env::var("MMA_MALODY_ROOT") {
        if !over.is_empty() {
            return Some(PathBuf::from(over));
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
            return Some(PathBuf::from(v));
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
            return Some(PathBuf::from(v));
        }
    }
    config::detect_malody_root()
}

/// 扫描 {malodyRoot}/skin/ 下含哨兵 mma.txt 的皮肤目录（命中多个全部写入，幂等）。
fn skin_targets(shared: &Shared) -> Vec<PathBuf> {
    let Some(root) = malody_root(shared) else { return Vec::new() };
    let Ok(entries) = fs::read_dir(root.join("skin")) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let dir = entry.path();
        if dir.is_dir() && dir.join("mma.txt").exists() {
            out.push(dir.join("mma_state.txt"));
        }
    }
    out
}

/// result 帧 → mma_state.txt（KV 文本；tmp+rename 原子写）。
fn write_mma_state(shared: &Shared, result_text: &str) {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(result_text) else {
        return;
    };
    let payload = value
        .get("payload")
        .cloned()
        .unwrap_or(value);
    if payload.get("activeSource").and_then(|v| v.as_str()) != Some("malody") {
        return;
    }
    let errors = payload.get("errors").and_then(|v| v.as_array());
    if errors.map(|arr| !arr.is_empty()).unwrap_or(true) {
        return;
    }
    let kv = |key: &str| -> String {
        payload
            .get(key)
            .map(|v| match v.as_str() {
                Some(s) => s.to_string(),
                None => v.to_string(),
            })
            .unwrap_or_default()
    };
    let content = format!(
        "star={}\npattern={}\nmsd={}\ngraph={}\nclient=malody\nupdatedAt={}\n",
        kv("star"),
        kv("pattern"),
        kv("msd"),
        kv("graph"),
        kv("updatedAt"),
    );
    for target in skin_targets(shared) {
        let tmp = target.with_extension("state.tmp");
        if fs::write(&tmp, &content).is_ok() {
            let _ = fs::rename(&tmp, &target);
        }
    }
}

// ---- 周期器（tosu 重探测 30s + ping 15s）----

pub fn spawn_timers(shared: Arc<Shared>) {
    thread::spawn(move || {
        loop {
            thread::sleep(PING_INTERVAL);
            let online = shared.tosu.as_ref().map(config::tosu_online).unwrap_or(false);
            *shared.tosu_online.lock().unwrap() = online;
            // mma-shell.json 变化检测：用户直接编辑文件 → 重载并推送 settings 帧。
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
    let listener = match TcpListener::bind("127.0.0.1:24061") {
        Ok(l) => l,
        Err(e) => {
            eprintln!("mma-shell: cannot bind 24061 ({e}) — another instance already running?");
            std::process::exit(2);
        }
    };
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