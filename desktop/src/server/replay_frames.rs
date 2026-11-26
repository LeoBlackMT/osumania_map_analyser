use std::sync::{mpsc::Sender, Arc};
use std::thread;
use std::time::Duration;

use crate::osu::lazer::ReplayFrameBatch;
use super::Shared;

pub const SNAPSHOT_CHUNK: usize = 4096;

fn deliver(shared: &Shared, batch: ReplayFrameBatch, target: Option<&Sender<String>>) {
    super::ws::deliver_laneacc(
        shared,
        super::ws::LaneAccBatch {
            kind: "mania_replay_frames",
            key: "frames",
            session: batch.session,
            offset: batch.offset,
            reset: batch.reset,
            items: &batch.frames,
            chunk_size: SNAPSHOT_CHUNK,
        },
        target,
    );
}

pub fn sync(shared: &Shared, target: &Sender<String>) {
    let _order = shared.laneacc_delivery.lock().unwrap();
    let (pending, snapshot) = shared.replay_frames.drain_and_snapshot();
    deliver(shared, pending, None);
    deliver(shared, snapshot, Some(target));
}

pub fn spawn_sender(shared: Arc<Shared>) {
    thread::spawn(move || loop {
        if shared.replay_frames.wait_for_change(Duration::from_millis(100)) {
            let _order = shared.laneacc_delivery.lock().unwrap();
            deliver(&shared, shared.replay_frames.drain(), None);
        }
    });
}
