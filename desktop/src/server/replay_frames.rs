// replay_frames.rs —— 把读取线程的原始 ReplayFrame 按 100ms 合并推给所有页面。
//
// 数据链路：Player.Score.Replay.Frames（List<ManiaReplayFrame>）→ Time/Actions。
// 页面用这份原始按键帧重算 Stable ScoreV1；hitEvents 仍只负责 Lazer 305 口径。

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use serde_json::json;

use super::Shared;

/// 全量快照每帧最多 4096 条（80k 帧的 18 分钟谱面分块推送）。
pub const SNAPSHOT_CHUNK: usize = 4096;

fn frame_json(frame: &crate::osu::lazer::RawReplayFrame) -> serde_json::Value {
    json!({ "time": frame.time, "keys": frame.keys })
}

pub fn snapshot_messages(shared: &Shared) -> Vec<String> {
    let batch = shared.replay_frames.snapshot();
    let chunks: Vec<&[crate::osu::lazer::RawReplayFrame]> = if batch.frames.is_empty() {
        Vec::new()
    } else {
        batch.frames.chunks(SNAPSHOT_CHUNK).collect()
    };
    if chunks.is_empty() {
        let env = crate::frames::Envelope::new(
            "mania_replay_frames",
            super::next_seq(shared),
            Some(json!({ "reset": true, "frames": [] })),
        );
        return vec![serde_json::to_string(&env).unwrap_or_default()];
    }
    chunks
        .iter()
        .enumerate()
        .map(|(index, chunk)| {
            let frames: Vec<serde_json::Value> = chunk.iter().map(frame_json).collect();
            let env = crate::frames::Envelope::new(
                "mania_replay_frames",
                super::next_seq(shared),
                Some(json!({ "reset": index == 0, "frames": frames })),
            );
            serde_json::to_string(&env).unwrap_or_default()
        })
        .collect()
}

pub fn spawn_sender(shared: Arc<Shared>) {
    thread::spawn(move || loop {
        if !shared
            .replay_frames
            .wait_for_change(Duration::from_millis(100))
        {
            continue;
        }
        let batch = shared.replay_frames.drain();
        if !batch.reset && batch.frames.is_empty() {
            continue;
        }
        let frames: Vec<serde_json::Value> = batch.frames.iter().map(frame_json).collect();
        super::broadcast(
            &shared,
            "mania_replay_frames",
            Some(json!({ "reset": batch.reset, "frames": frames })),
        );
    });
}
