// replay_pauses.rs —— 回放暂停数据面（`.osr` → 24061 `replay_pauses`）。
//
// 链路：读取线程的 `Snapshot.replay_hash`
//   → `<songs_folder>\h\hh\<hash>`（`to_lazer_path`）
//   → `osr::read_replay_pauses`（只解 `scoreInfo`，不碰按键帧）
//   → 状态变化时广播；同 hash 只解析一次，失败按 hash 记忆并退避重试。
//
// 只读：不写游戏目录、不注入、不改任何数据库。

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::Shared;
use crate::osu::lazer::osr;

/// 采样周期（与 LaneAcc 的两个数据面一致）。
const POLL_INTERVAL: Duration = Duration::from_millis(100);
/// 同一 hash 解析失败后的重试间隔（文件可能稍后才落盘）。
const FAILURE_BACKOFF: Duration = Duration::from_secs(2);
/// 连续多少个 tick 读不到 hash 才清空状态（≈1 s）：抗内存瞬时读失败。
const MISSING_TICKS_BEFORE_CLEAR: u32 = 10;
/// `pauses` 上限（正常成绩远小于此；防御坏数据撑爆 5 MB 帧）。
const MAX_PAUSES: usize = 4096;

/// 一帧回放暂停数据（也是 `Shared` 里保留的"当前状态"）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReplayPausesFrame {
    pub hash: String,
    pub pauses: Vec<i32>,
    pub available: bool,
    pub speed_rate: Option<f64>,
}

impl ReplayPausesFrame {
    pub fn empty() -> Self {
        Self::default()
    }

    fn failed(hash: &str) -> Self {
        Self {
            hash: hash.to_string(),
            pauses: Vec::new(),
            available: false,
            speed_rate: None,
        }
    }

    fn to_payload(&self) -> Value {
        json!({
            "hash": self.hash,
            "pauses": self.pauses,
            "available": self.available,
            "speedRate": self.speed_rate,
        })
    }
}

/// `Shared` 上的当前状态（读取线程/发送线程之间唯一的交接点）。
#[derive(Default)]
pub struct ReplayPausesState {
    frame: Mutex<ReplayPausesFrame>,
}

impl ReplayPausesState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn snapshot(&self) -> ReplayPausesFrame {
        self.frame.lock().unwrap().clone()
    }

    /// 写入并返回是否与上一帧不同（不同才广播）。
    fn store(&self, next: ReplayPausesFrame) -> bool {
        let mut current = self.frame.lock().unwrap();
        if *current == next {
            return false;
        }
        *current = next;
        true
    }
}

/// 页面刷新/重连时主动要的当前快照（只发给发起连接）。
pub fn snapshot_messages(shared: &Shared) -> Vec<String> {
    let frame = shared.replay_pauses.snapshot();
    let envelope = crate::frames::Envelope::new(
        "replay_pauses",
        super::next_seq(shared),
        Some(frame.to_payload()),
    );
    vec![serde_json::to_string(&envelope).unwrap_or_default()]
}

/// 启动采集线程；进程生命周期内常驻。
pub fn spawn_sender(shared: Arc<Shared>) {
    thread::spawn(move || {
        let mut published = ReplayPausesFrame::empty();
        let mut last_attempt: Option<(String, Instant)> = None;
        let mut missing_ticks = 0u32;

        loop {
            thread::sleep(POLL_INTERVAL);

            let Some(reader) = crate::osu::instance() else {
                continue;
            };
            let context = reader.replay_context();
            let active = context.client == Some(crate::osu::model::Client::Lazer)
                && (context.replay || context.state_name == "resultScreen");

            if !active {
                missing_ticks = missing_ticks.saturating_add(1);
                if missing_ticks >= MISSING_TICKS_BEFORE_CLEAR && !published.hash.is_empty() {
                    published = ReplayPausesFrame::empty();
                    last_attempt = None;
                    if shared.replay_pauses.store(published.clone()) {
                        broadcast(&shared, &published);
                    }
                }
                continue;
            }
            missing_ticks = 0;

            let hash = context
                .replay_hash
                .filter(|hash| crate::osu::lazer::is_store_hash(hash));
            let Some(hash) = hash else {
                // 本拍读不到：保留上一帧，不清空（等 `MISSING_TICKS_BEFORE_CLEAR` 的判定）。
                continue;
            };

            if hash == published.hash {
                let retry = !published.available
                    && last_attempt.as_ref().map_or(true, |(attempted, at)| {
                        attempted == &hash && at.elapsed() >= FAILURE_BACKOFF
                    });
                if !retry {
                    continue;
                }
            }

            let frame = read_frame(context.songs_folder.as_deref(), &hash);
            last_attempt = Some((hash.clone(), Instant::now()));
            let ok = frame.available;
            published = frame;
            if shared.replay_pauses.store(published.clone()) {
                broadcast(&shared, &published);
            }
            if ok {
                eprintln!(
                    "[replay_pauses] {} pauses={} rate={:?}",
                    short_hash(&published.hash),
                    published.pauses.len(),
                    published.speed_rate
                );
            } else {
                eprintln!("[replay_pauses] unavailable {}", short_hash(&hash));
            }
        }
    });
}

fn broadcast(shared: &Shared, frame: &ReplayPausesFrame) {
    super::broadcast(shared, "replay_pauses", Some(frame.to_payload()));
}

fn read_frame(songs_folder: Option<&str>, hash: &str) -> ReplayPausesFrame {
    let Some(folder) = songs_folder else {
        return ReplayPausesFrame::failed(hash);
    };
    let Some(relative) = crate::osu::lazer::to_lazer_path(hash) else {
        return ReplayPausesFrame::failed(hash);
    };
    let path = PathBuf::from(folder).join(relative);
    let Ok(metadata) = std::fs::metadata(&path) else {
        return ReplayPausesFrame::failed(hash);
    };
    if metadata.len() > osr::MAX_OSR_BYTES as u64 {
        return ReplayPausesFrame::failed(hash);
    }
    let Ok(bytes) = std::fs::read(&path) else {
        return ReplayPausesFrame::failed(hash);
    };
    match osr::read_replay_pauses(&bytes) {
        Ok(read) => {
            let mut pauses = read.pauses;
            if pauses.len() > MAX_PAUSES {
                pauses.truncate(MAX_PAUSES);
            }
            ReplayPausesFrame {
                hash: hash.to_string(),
                pauses,
                available: read.available,
                speed_rate: read.speed_rate,
            }
        }
        Err(_) => ReplayPausesFrame::failed(hash),
    }
}

fn short_hash(hash: &str) -> &str {
    hash.get(..8).unwrap_or(hash)
}
