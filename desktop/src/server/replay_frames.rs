use std::sync::{mpsc::Sender, Arc};
use std::thread;
use std::time::Duration;

use crate::osu::lazer::ReplayFrameBatch;
use super::{LaneAccHub, Shared};

pub const SNAPSHOT_CHUNK: usize = 4096;

fn deliver_hub(hub: &LaneAccHub<'_>, batch: ReplayFrameBatch, target: Option<&Sender<String>>) {
    super::ws::deliver_laneacc(
        hub.seq,
        hub.sinks,
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

pub(super) fn sync_hub(hub: &LaneAccHub<'_>, target: &Sender<String>) {
    let _order = hub.delivery.lock().unwrap();
    let (pending, snapshot) = hub.replay_frames.drain_and_snapshot();
    deliver_hub(hub, pending, None);
    deliver_hub(hub, snapshot, Some(target));
}

pub fn sync(shared: &Shared, target: &Sender<String>) {
    sync_hub(&shared.laneacc_hub(), target);
}

pub fn spawn_sender(shared: Arc<Shared>) {
    thread::spawn(move || loop {
        if shared.replay_frames.wait_for_change(Duration::from_millis(100)) {
            let hub = shared.laneacc_hub();
            let _order = hub.delivery.lock().unwrap();
            deliver_hub(&hub, hub.replay_frames.drain(), None);
        }
    });
}
