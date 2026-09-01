// /ws 帧循环：hello/state/song/settings 出站 + diag/control/result 入站匹配 pending。
// 逻辑从原 server.rs 拆分。

use crate::frames::{ControlInbound, ResultInbound};
use crate::server::{hello_frame, log::log_at, next_seq, Shared};
use std::net::TcpStream;
use std::sync::{mpsc, Arc};
use std::time::Duration;

/// 契约 v2 control 帧处理（窗口操控）。
pub fn handle_control(shared: &Shared, ctrl: &ControlInbound) {
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
            let window2 = window.clone();
            let _ = window2.start_dragging();
        }
        _ => {}
    }
}

/// 从 result 帧（Envelope 包裹或裸帧）提取 ResultInbound（契约 §2 payload 载体）。
pub fn parse_result_payload(text: &str) -> Option<ResultInbound> {
    if let Ok(envelope) = serde_json::from_str::<serde_json::Value>(text) {
        if let Some(payload) = envelope.get("payload") {
            if let Ok(inbound) = serde_json::from_value::<ResultInbound>(payload.clone()) {
                return Some(inbound);
            }
        }
        if let Ok(inbound) = serde_json::from_value::<ResultInbound>(envelope) {
            return Some(inbound);
        }
    }
    None
}

pub fn handle_ws(shared: Arc<Shared>, stream: TcpStream) {
    // 出站排水依赖周期性唤醒：read timeout 让阻塞的 read 周期返回。
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
        },
    ) else {
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
                continue;
            }
            Ok(tungstenite::Message::Text(text)) => {
                // diag 帧（页面诊断回传）
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
                    if value.get("type").and_then(|v| v.as_str()) == Some("diag") {
                        if let Some(msg) = value
                            .get("payload")
                            .and_then(|p| p.get("message"))
                            .and_then(|m| m.as_str())
                        {
                            log_at("debug", &format!("page diag: {}", msg));
                        }
                        continue;
                    }
                }
                // control 帧（契约 v2）：页面 → 壳窗口控制。
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
                    if value.get("type").and_then(|v| v.as_str()) == Some("control") {
                        if let Some(payload) = value.get("payload") {
                            if let Ok(ctrl) = serde_json::from_value::<ControlInbound>(payload.clone()) {
                                handle_control(&shared, &ctrl);
                            }
                        }
                        continue;
                    }
                }
                // result 帧按 Envelope 包裹（payload 内层）；兼容裸帧两种情况。
                if let Some(inbound) = parse_result_payload(&text) {
                    log_at("debug", &format!(
                        "page result: req={} hint={} active={} errors={}",
                        inbound.request_id.as_deref().unwrap_or(""),
                        inbound.status_hint.as_deref().unwrap_or(""),
                        inbound.active_source.as_deref().unwrap_or(""),
                        inbound
                            .errors
                            .as_ref()
                            .map(|e| e.join(";"))
                            .unwrap_or_default()
                            .chars()
                            .take(160)
                            .collect::<String>(),
                    ));
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
