// hit_events.rs —— 把读取线程的逐判定事件按 100ms 合并推给所有页面。
//
// 读取线程（`osu::runner`）只负责写 `HitEventLog`；本模块是唯一的 WS 出口：
// - 有 reset 待发 ⇒ 发全量快照（`reset:true`）；
// - 否则发增量（`reset:false`）；
// - 页面刷新/重连时由 `ws.rs` 的 `laneacc_sync` 单独要全量，不在这里广播。

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use serde_json::json;

use super::Shared;

/// 全量快照每帧最多 2048 条（5MB 帧上限的保守分块）。
pub const SNAPSHOT_CHUNK: usize = 2048;

/// 页面刷新/重连时主动取的本局全量快照（只发给发起连接）。
pub fn snapshot_messages(shared: &Shared) -> Vec<String> {
    let batch = shared.hit_events.snapshot();
    let chunks: Vec<&[crate::osu::lazer::RawHitEvent]> = if batch.events.is_empty() {
        Vec::new()
    } else {
        batch.events.chunks(SNAPSHOT_CHUNK).collect()
    };
    if chunks.is_empty() {
        let env = crate::frames::Envelope::new(
            "mania_hit_events",
            super::next_seq(shared),
            Some(json!({ "reset": true, "events": [] })),
        );
        return vec![serde_json::to_string(&env).unwrap_or_default()];
    }
    chunks
        .iter()
        .enumerate()
        .map(|(index, chunk)| {
            let events: Vec<serde_json::Value> = chunk
                .iter()
                .map(|event| {
                    json!({
                        "col": event.col,
                        "result": event.result,
                        "offset": event.offset,
                    })
                })
                .collect();
            let env = crate::frames::Envelope::new(
                "mania_hit_events",
                super::next_seq(shared),
                Some(json!({ "reset": index == 0, "events": events })),
            );
            serde_json::to_string(&env).unwrap_or_default()
        })
        .collect()
}

/// 启动发送线程；进程生命周期内常驻。
pub fn spawn_sender(shared: Arc<Shared>) {
    thread::spawn(move || loop {
        if !shared
            .hit_events
            .wait_for_change(Duration::from_millis(100))
        {
            continue;
        }
        let batch = shared.hit_events.drain();
        if !batch.reset && batch.events.is_empty() {
            continue;
        }
        let events: Vec<serde_json::Value> = batch
            .events
            .iter()
            .map(|event| {
                json!({
                    "col": event.col,
                    "result": event.result,
                    "offset": event.offset,
                })
            })
            .collect();
        super::broadcast(
            &shared,
            "mania_hit_events",
            Some(json!({ "reset": batch.reset, "events": events })),
        );
    });
}
