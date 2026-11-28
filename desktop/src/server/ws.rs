// /ws 帧循环：hello/state/song/settings 出站 + diag/control/result 入站匹配 pending。
// 逻辑从原 server.rs 拆分。

use crate::frames::{ControlInbound, ResultInbound};
use crate::server::{broadcast_to, hello_frame, log::log_at, next_seq_in, Shared};
use std::collections::HashMap;
use std::net::TcpStream;
use std::sync::{
    atomic::AtomicU64,
    mpsc, Arc, Mutex,
};
use std::time::Duration;
use tauri::Manager;

/// 契约 v2 control 帧处理（窗口操控）。
pub fn handle_control(shared: &Shared, ctrl: &ControlInbound) {
    let Some(window) = shared.window.lock().unwrap().clone() else { return };
    match ctrl.action.as_str() {
        "close" => {
            let _ = window.close();
        }
        "alwaysOnTop" => {
            crate::app::window_state::apply_flag_change(window.app_handle(), Some(ctrl.value.unwrap_or(true)), None);
        }
        "clickThrough" => {
            crate::app::window_state::apply_flag_change(window.app_handle(), None, Some(ctrl.value.unwrap_or(true)));
        }
        "dragStart" => {
            let window2 = window.clone();
            let _ = window2.start_dragging();
        }
        "resizeStart" => {
            if let Some(direction) = ctrl.direction {
                let _ = window.as_ref().window_ref().start_resize_dragging(direction);
            }
        }
        // 窗口内快捷键兜底（Wayland 无全局快捷键；页面 keydown → 此处 toggle）。
        // 状态以 mma-shell-state.json 为准（main.rs 全局快捷键同源写盘），
        // 读取最新值后取反；窗口 API 一律 run_on_main_thread（自死锁见 main.rs 注）。
        "toggleTopmost" => toggle_window_flag(&window, true),
        "toggleClickThrough" => toggle_window_flag(&window, false),
        _ => {}
    }
}

fn toggle_window_flag(window: &tauri::WebviewWindow, topmost: bool) {
    let app = window.app_handle().clone();
    let window2 = window.clone();
    // 状态读写都在主线程闭包内进行：read → 取反 → set → 写盘。
    // （若在闭包外读，连续两次 toggle 会读到同一旧值 → 竞态翻转失效。）
    let _ = app.run_on_main_thread(move || {
        let saved = crate::config::read_window_state();
        crate::app::window_state::apply_flag_change(
            window2.app_handle(),
            topmost.then_some(!saved.topmost),
            (!topmost).then_some(!saved.click_through),
        );
    });
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

/// Origin 仅允许 loopback（契约 §0）：解析 origin 的 host 段**精确比较**
/// `127.0.0.1` / `localhost` / `[::1]`——子串匹配（contains）会放过
/// `localhost.evil.com` 这类域，形同虚设。
pub(crate) fn is_loopback_origin(origin: &str) -> bool {
    let rest = match origin.split_once("://") {
        Some((_, r)) => r,
        None => return false,
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let authority = match authority.rsplit_once('@') {
        Some((_, h)) => h,
        None => authority,
    };
    // IPv6 字面量：[::1]:port
    if let Some(stripped) = authority.strip_prefix('[') {
        if let Some(end) = stripped.find(']') {
            return &stripped[..end] == "::1";
        }
    }
    let host = match authority.rsplit_once(':') {
        Some((h, _port)) => h,
        None => authority,
    };
    host.eq_ignore_ascii_case("127.0.0.1") || host.eq_ignore_ascii_case("localhost")
}

pub(super) struct LaneAccBatch<'a, T> {
    pub kind: &'a str,
    pub key: &'a str,
    pub session: u64,
    pub offset: usize,
    pub reset: bool,
    pub items: &'a [T],
    pub chunk_size: usize,
}

/// Both snapshots and broadcasts enter each connection's FIFO under the delivery lock.
/// Only enqueue here: a slow socket must never block the poller or another client.
pub(super) fn deliver_laneacc<T: serde::Serialize>(
    seq: &AtomicU64,
    sinks: &Mutex<Vec<(u64, mpsc::Sender<String>)>>,
    batch: LaneAccBatch<'_, T>,
    target: Option<&mpsc::Sender<String>>,
) {
    let send = |index: usize, chunk: &[T]| {
        let payload = serde_json::json!({
            "session": batch.session,
            "offset": batch.offset + index * batch.chunk_size,
            "reset": batch.reset && index == 0,
            (batch.key): chunk,
        });
        if let Some(tx) = target {
            let env = crate::frames::Envelope::new(batch.kind, next_seq_in(seq), Some(payload));
            let _ = tx.send(serde_json::to_string(&env).unwrap());
        } else {
            broadcast_to(seq, sinks, batch.kind, Some(payload));
        }
    };
    if batch.reset && batch.items.is_empty() {
        send(0, batch.items);
    } else {
        for (index, chunk) in batch.items.chunks(batch.chunk_size).enumerate() {
            send(index, chunk);
        }
    }
}

struct WsLoopEnv<'a> {
    seq: &'a AtomicU64,
    sinks: &'a Mutex<Vec<(u64, mpsc::Sender<String>)>>,
    pending: &'a Mutex<HashMap<String, mpsc::Sender<String>>>,
}

trait WsHooks {
    fn on_laneacc_sync(&self, tx: &mpsc::Sender<String>);
    fn on_replay_pauses_sync(&self) -> Vec<String>;
    fn on_control(&self, ctrl: &ControlInbound);
}

struct SharedWsHooks<'a>(&'a Shared);

impl WsHooks for SharedWsHooks<'_> {
    fn on_laneacc_sync(&self, tx: &mpsc::Sender<String>) {
        crate::server::hit_events::sync(self.0, tx);
        crate::server::replay_frames::sync(self.0, tx);
    }

    fn on_replay_pauses_sync(&self) -> Vec<String> {
        crate::server::replay_pauses::snapshot_messages(self.0)
    }

    fn on_control(&self, ctrl: &ControlInbound) {
        handle_control(self.0, ctrl);
    }
}

fn accept_loopback_ws(stream: TcpStream) -> Option<tungstenite::WebSocket<TcpStream>> {
    // 出站排水依赖周期性唤醒：read timeout 让阻塞的 read 周期返回。
    let _ = stream.set_read_timeout(Some(Duration::from_millis(100)));
    tungstenite::accept_hdr(
        stream,
        |req: &tungstenite::handshake::server::Request,
         resp: tungstenite::handshake::server::Response| {
            // 仅接受 loopback Origin（契约 §0）。
            let origin_ok = req
                .headers()
                .get("Origin")
                .map(|v| is_loopback_origin(v.to_str().unwrap_or("")))
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
    )
    .ok()
}

fn serve_ws_connection(
    env: &WsLoopEnv<'_>,
    stream: TcpStream,
    initial_frames: Vec<String>,
    hooks: &dyn WsHooks,
) {
    let Some(mut ws) = accept_loopback_ws(stream) else {
        return;
    };
    let (tx, rx) = mpsc::channel::<String>();
    let conn_id = next_seq_in(env.seq);
    env.sinks.lock().unwrap().push((conn_id, tx.clone()));
    let remove_sink = || {
        env.sinks.lock().unwrap().retain(|(id, _s)| *id != conn_id);
    };
    for text in initial_frames {
        if ws.send(tungstenite::Message::Text(text)).is_err() {
            remove_sink();
            return;
        }
    }
    // 单线程事件循环：出站排水（try_recv）+ 入站匹配 pending。
    loop {
        while let Ok(text) = rx.try_recv() {
            if ws.send(tungstenite::Message::Text(text)).is_err() {
                remove_sink();
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
                    match value.get("type").and_then(|v| v.as_str()) {
                        Some("diag") => {
                            if let Some(msg) = value
                                .get("payload")
                                .and_then(|p| p.get("message"))
                                .and_then(|m| m.as_str())
                            {
                                log_at("debug", &format!("page diag: {}", msg));
                            }
                            continue;
                        }
                        // laneacc_sync：页面刷新/重连主动要本局全量（只发给这个连接）。
                        Some("laneacc_sync") => {
                            hooks.on_laneacc_sync(&tx);
                            continue;
                        }
                        // replay_pauses_sync：页面刷新/重连主动要当前回放的暂停数据（只发给这个连接）。
                        Some("replay_pauses_sync") => {
                            for text in hooks.on_replay_pauses_sync() {
                                if ws.send(tungstenite::Message::Text(text)).is_err() {
                                    remove_sink();
                                    return;
                                }
                            }
                            continue;
                        }
                        // control 帧（契约 v2）：页面 → 壳窗口控制。
                        Some("control") => {
                            if let Some(payload) = value.get("payload") {
                                if let Ok(ctrl) = serde_json::from_value::<ControlInbound>(payload.clone()) {
                                    hooks.on_control(&ctrl);
                                }
                            }
                            continue;
                        }
                        _ => {}
                    }
                }
                // result 帧按 Envelope 包裹（payload 内层）；兼容裸帧两种情况。
                if let Some(inbound) = parse_result_payload(&text) {
                    // result 帧提到 info：分析失败时用户无需开 debug 即可看到
                    // 具体错误文本（页面 No data 的排障入口）。
                    let errs = inbound
                        .errors
                        .as_ref()
                        .map(|e| e.join(";"))
                        .unwrap_or_default();
                    let hint = inbound.status_hint.as_deref().unwrap_or("");
                    if hint == "analysis-failed" || !errs.is_empty() {
                        log_at("error", &format!("page result: req={} hint={} active={} errors={}", inbound.request_id.as_deref().unwrap_or(""), hint,
                            inbound.active_source.as_deref().unwrap_or(""),
                            errs.chars().take(400).collect::<String>(),
                        ));
                    } else {
                        log_at("debug", &format!(
                            "page result: req={} hint={} active={} errors={}",
                            inbound.request_id.as_deref().unwrap_or(""),
                            hint,
                            inbound.active_source.as_deref().unwrap_or(""),
                            errs.chars().take(160).collect::<String>(),
                        ));
                    }
                    if let Some(rid) = inbound.request_id {
                        if let Some(tx) = env.pending.lock().unwrap().remove(&rid) {
                            let _ = tx.send(text);
                        }
                    }
                }
            }
            Ok(tungstenite::Message::Close(_)) | Err(_) => break,
            _ => {}
        }
    }
    remove_sink();
}

pub fn handle_ws(shared: Arc<Shared>, stream: TcpStream) {
    let mut initial_frames = vec![serde_json::to_string(&hello_frame(&shared)).unwrap_or_default()];
    // 重连补发（契约 v4，只发给**这个**连接）：一帧 state + （桥在选曲/游玩/结算态时）
    // 当前谱面的 song 帧。song 帧只在真实事件时广播一次，而 broadcast 只发给已注册的
    // sink ⇒ 页面刷新 / WS 重连 / 先开游戏后开壳时，当前选中的谱面否则再也不会被送来
    // （旧通道"偶发 No Data"的正面修复）。
    initial_frames.extend(crate::server::bridge::reconnect_messages(&shared));
    let env = WsLoopEnv {
        seq: &shared.seq,
        sinks: &shared.sinks,
        pending: &shared.pending,
    };
    serve_ws_connection(&env, stream, initial_frames, &SharedWsHooks(&shared));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::osu::lazer::{HitEventLog, RawHitEvent, RawReplayFrame, ReplayFrameLog};
    use crate::server::{hit_events, replay_frames, LaneAccHub};
    use serde_json::{json, Value};
    use std::net::TcpListener;

    // Pure-Rust stand-in for Shared: referencing Shared or handle_control pulls tauri/muda
    // into the Windows --lib test binary, which lacks the bin target's ComCtl32 v6 manifest.
    #[derive(Default)]
    struct TestHub {
        seq: AtomicU64,
        sinks: Mutex<Vec<(u64, mpsc::Sender<String>)>>,
        pending: Mutex<HashMap<String, mpsc::Sender<String>>>,
        delivery: Mutex<()>,
        hit_events: HitEventLog,
        replay_frames: ReplayFrameLog,
    }

    impl TestHub {
        fn hub(&self) -> LaneAccHub<'_> {
            LaneAccHub {
                seq: &self.seq,
                sinks: &self.sinks,
                delivery: &self.delivery,
                hit_events: &self.hit_events,
                replay_frames: &self.replay_frames,
            }
        }
    }

    impl WsHooks for TestHub {
        fn on_laneacc_sync(&self, tx: &mpsc::Sender<String>) {
            let hub = self.hub();
            hit_events::sync_hub(&hub, tx);
            replay_frames::sync_hub(&hub, tx);
        }

        fn on_replay_pauses_sync(&self) -> Vec<String> {
            Vec::new()
        }

        fn on_control(&self, _ctrl: &ControlInbound) {}
    }

    fn collect_lane_messages(rx: &mpsc::Receiver<String>, field: &str) -> (usize, usize) {
        let (mut next_index, mut resets) = (0usize, 0usize);
        for text in rx.try_iter() {
            let msg: Value = serde_json::from_str(&text).unwrap();
            let payload = &msg["payload"];
            let Some(items) = payload[field].as_array() else {
                continue;
            };
            if payload["reset"] == true {
                next_index = 0;
                resets += 1;
            }
            assert_eq!(payload["offset"].as_u64(), Some(next_index as u64));
            for item in items {
                let marker = if field == "events" {
                    item["note"].as_u64().unwrap()
                } else {
                    item["time"].as_f64().unwrap() as u64
                };
                assert_eq!(marker, next_index as u64);
                next_index += 1;
            }
        }
        (next_index, resets)
    }

    #[test]
    fn sync_and_broadcast_preserve_continuous_session_offsets() {
        let state = TestHub::default();
        let (tx_sync, rx_sync) = mpsc::channel();
        let (tx_live, rx_live) = mpsc::channel();
        state.sinks.lock().unwrap().extend([(1, tx_sync.clone()), (2, tx_live)]);

        let hits: Vec<_> = (0..5000)
            .map(|i| RawHitEvent { result: 6, note: Some(i), ..RawHitEvent::default() })
            .collect();
        state.hit_events.push(true, &hits[..2000]);
        for batch in hits[2000..].chunks(500) {
            state.hit_events.push(false, batch);
            hit_events::sync_hub(&state.hub(), &tx_sync);
        }
        assert_eq!(collect_lane_messages(&rx_sync, "events"), (5000, 7));
        assert_eq!(collect_lane_messages(&rx_live, "events"), (5000, 1));

        let frames: Vec<_> = (0..10000)
            .map(|i| RawReplayFrame { time: i as f64, keys: 0 })
            .collect();
        state.replay_frames.push(true, &frames);
        replay_frames::sync_hub(&state.hub(), &tx_sync);
        assert_eq!(collect_lane_messages(&rx_sync, "frames"), (10000, 2));
        assert_eq!(collect_lane_messages(&rx_live, "frames"), (10000, 1));

        state.hit_events.clear();
        hit_events::sync_hub(&state.hub(), &tx_sync);
        assert_eq!(collect_lane_messages(&rx_sync, "events"), (0, 2));
        assert_eq!(collect_lane_messages(&rx_live, "events"), (0, 1));
    }

    #[test]
    fn ws_laneacc_sync_is_unicast_and_continuous_across_chunks() {
        let state = Arc::new(TestHub::default());
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server_state = state.clone();
        let accept_thread = std::thread::spawn(move || {
            let mut workers = Vec::new();
            for sock in listener.incoming().take(2) {
                let srv = server_state.clone();
                workers.push(std::thread::spawn(move || {
                    let env = WsLoopEnv {
                        seq: &srv.seq,
                        sinks: &srv.sinks,
                        pending: &srv.pending,
                    };
                    let hello = serde_json::to_string(&crate::frames::Envelope::new(
                        "hello",
                        next_seq_in(&srv.seq),
                        Some(json!({ "contract": 14 })),
                    ))
                    .unwrap();
                    serve_ws_connection(&env, sock.unwrap(), vec![hello], srv.as_ref());
                }));
            }
            for worker in workers {
                let _ = worker.join();
            }
        });

        let connect = || {
            let stream = TcpStream::connect(addr).unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let (mut ws, _) = tungstenite::client(format!("ws://{addr}/ws"), stream).unwrap();
            let hello: Value = serde_json::from_str(ws.read().unwrap().to_text().unwrap()).unwrap();
            assert_eq!(hello["type"], "hello");
            ws
        };
        let next_lane = |ws: &mut tungstenite::WebSocket<TcpStream>| -> Value {
            loop {
                let v: Value = serde_json::from_str(ws.read().unwrap().to_text().unwrap()).unwrap();
                if matches!(v["type"].as_str(), Some("mania_hit_events" | "mania_replay_frames")) {
                    return v;
                }
            }
        };

        let (mut client_a, mut client_b) = (connect(), connect());
        state.hit_events.push(true, &[RawHitEvent::default(); 2049]);
        state.replay_frames.push(true, &[RawReplayFrame::default(); 4097]);
        client_a
            .send(tungstenite::Message::Text(
                json!({ "type": "laneacc_sync", "v": 13, "seq": 1 }).to_string(),
            ))
            .unwrap();

        for (ws, repeats) in [(&mut client_a, 2), (&mut client_b, 1)] {
            for (field, max_chunk) in [("events", 2048), ("frames", 4096)] {
                for _ in 0..repeats {
                    let first = next_lane(ws);
                    assert_eq!(first["payload"]["reset"], true);
                    assert_eq!(first["payload"]["offset"], 0);
                    assert_eq!(first["payload"][field].as_array().unwrap().len(), max_chunk);

                    let second = next_lane(ws);
                    assert_eq!(second["payload"]["reset"], false);
                    assert_eq!(second["payload"]["offset"], max_chunk);
                    assert_eq!(second["payload"][field].as_array().unwrap().len(), 1);
                }
            }
        }

        state.hit_events.push(false, &[RawHitEvent::default()]);
        let delta = state.hit_events.drain();
        deliver_laneacc(
            &state.seq,
            &state.sinks,
            LaneAccBatch {
                kind: "mania_hit_events",
                key: "events",
                session: delta.session,
                offset: delta.offset,
                reset: delta.reset,
                items: &delta.events,
                chunk_size: hit_events::SNAPSHOT_CHUNK,
            },
            None,
        );
        for ws in [&mut client_a, &mut client_b] {
            let msg = next_lane(ws);
            assert_eq!(msg["payload"]["offset"], 2049);
            assert_eq!(msg["payload"]["reset"], false);
        }

        let _ = client_a.close(None);
        let _ = client_b.close(None);
        let _ = accept_thread.join();
    }
}
