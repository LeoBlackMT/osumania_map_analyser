use std::sync::{mpsc::Sender, Arc};
use std::thread;
use std::time::Duration;

use crate::osu::lazer::HitEventBatch;
use super::Shared;

pub const SNAPSHOT_CHUNK: usize = 2048;

fn deliver(shared: &Shared, batch: HitEventBatch, target: Option<&Sender<String>>) {
    super::ws::deliver_laneacc(
        shared,
        super::ws::LaneAccBatch {
            kind: "mania_hit_events",
            key: "events",
            session: batch.session,
            offset: batch.offset,
            reset: batch.reset,
            items: &batch.events,
            chunk_size: SNAPSHOT_CHUNK,
        },
        target,
    );
}

pub fn sync(shared: &Shared, target: &Sender<String>) {
    let _order = shared.laneacc_delivery.lock().unwrap();
    let (pending, snapshot) = shared.hit_events.drain_and_snapshot();
    deliver(shared, pending, None);
    deliver(shared, snapshot, Some(target));
}

pub fn spawn_sender(shared: Arc<Shared>) {
    thread::spawn(move || loop {
        if shared.hit_events.wait_for_change(Duration::from_millis(100)) {
            let _order = shared.laneacc_delivery.lock().unwrap();
            deliver(&shared, shared.hit_events.drain(), None);
        }
    });
}
