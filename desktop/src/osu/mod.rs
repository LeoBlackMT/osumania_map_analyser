// `osu` 模块门面与状态定义（Wave C2：stable 全字段 + 门状态机）。
//
// 本模块是 osu! 内存读取子系统的入口：
// - runner: 发现 -> 附着 -> 定址 -> 每 tick 读 + 过门 + 发布的后台主循环
// - invariants: 校验门与健康状态机
// - stable / lazer: 对应客户端的特定内存拓扑与解引用逻辑
// - offsets: 偏移表与锚点特征

pub mod anchor_cache;
pub mod beatmap_file;
pub mod compare;
pub mod discovery;
pub mod invariants;
pub mod keys;
pub mod lazer;
pub mod model;
pub mod offsets;
pub mod packet;
pub mod patterns;
pub mod runner;
pub mod scan;
pub mod shadow;
pub mod stable;
pub mod win;

pub use runner::run;

use crate::osu::invariants::HealthState;
use crate::osu::model::Snapshot;
use serde_json::Value;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// 附着失败后的重试间隔（§3.4：附着重试 2 s）。
pub const ATTACH_RETRY: Duration = Duration::from_secs(2);
/// 兜底路径（PID 扫描）失败后的重试间隔：一次全量扫描是秒级开销，不能 2 秒来一次。
pub const SWEEP_RETRY: Duration = Duration::from_secs(30);
/// 快重连窗口（Step 9d）：曾附着过的目标刚丢的头这 30 s 里，附着失败只等 1 s。
pub const LOSS_FAST_WINDOW_MS: u64 = 30_000;
/// 快重连窗口内的重试间隔。
pub const LOSS_FAST_RETRY: Duration = Duration::from_millis(1_000);

/// 正常 tick。
pub const TICK: Duration = Duration::from_millis(250);
/// 锚点扫描失败后的重试间隔。
pub const SCAN_RETRY: Duration = Duration::from_secs(5);
/// 区域过滤：先用参考过滤器（RW/RWX），关键锚点没命中再退化到宽过滤器。
pub const FILTER_READY: &[u32] = &[win::FILTER_RW, win::FILTER_READABLE];
/// 单进程区域数上限。
pub const REGION_LIMIT: usize = 65_536;
/// 单锚点的命中列表上限。
pub const ANCHOR_HIT_LIMIT: usize = 16;
/// 状态整数上界。
pub const STATE_INDEX_MAX: u32 = invariants::STATE_INDEX_MAX as u32;

/// 一次附着失败之后、下一次尝试之前的等待时间（纯函数）。
pub fn sweep_delay(
    lost_from_attached: bool,
    elapsed_since_loss_ms: u64,
    consecutive_failures: u32,
    retry_soon: bool,
    used_sweep: bool,
) -> Duration {
    if lost_from_attached && elapsed_since_loss_ms < LOSS_FAST_WINDOW_MS {
        return LOSS_FAST_RETRY;
    }
    if retry_soon {
        return ATTACH_RETRY;
    }
    if !lost_from_attached && used_sweep && consecutive_failures == 0 {
        return SWEEP_RETRY;
    }
    let step = if lost_from_attached {
        consecutive_failures
    } else {
        consecutive_failures.saturating_sub(1)
    };
    invariants::backoff(step as usize)
}

/// L0/L1 相位（Step 9e）：`sources.osu.phase` 的字面量来源。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    WaitingForGame,
    Attaching,
    Scanning,
    Healthy,
    Unavailable,
}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Phase::WaitingForGame => "waiting-for-game",
            Phase::Attaching => "attaching",
            Phase::Scanning => "scanning",
            Phase::Healthy => "healthy",
            Phase::Unavailable => "unavailable",
        }
    }

    pub fn notice(self) -> &'static str {
        match self {
            Phase::Scanning => "Scanning osu! memory… (about 10–20 s)",
            Phase::Attaching => "Re-attaching to osu!…",
            Phase::WaitingForGame => "Waiting for osu! to start…",
            Phase::Healthy | Phase::Unavailable => "",
        }
    }
}

/// 相位派生的唯一实现（纯函数）。
pub fn phase_for(
    published: bool,
    attached: bool,
    anchors_resolved: bool,
    ever_attached: bool,
    no_game: bool,
) -> Phase {
    if published {
        return Phase::Healthy;
    }
    if attached {
        return if anchors_resolved {
            Phase::Unavailable
        } else {
            Phase::Scanning
        };
    }
    if ever_attached || !no_game {
        Phase::Attaching
    } else {
        Phase::WaitingForGame
    }
}

/// L0 全量锚点扫描的实测进度。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScanProgress {
    pub filter: usize,
    pub regions: usize,
    pub bytes: u64,
    pub elapsed_ms: u64,
}

/// 读取线程对外可见的状态。
#[derive(Clone, Debug)]
pub struct ReaderState {
    pub snapshot: Option<Snapshot>,
    pub packet: Option<Value>,
    pub holding: bool,
    pub held_packet: Option<Value>,
    pub frame_at_ms: u64,
    pub reason: Option<String>,
    pub health: String,
    pub frozen: bool,
    pub degraded_fields: Vec<String>,
    pub strikes: u32,
    pub pid: Option<u32>,
    pub image_path: Option<String>,
    pub client: Option<String>,
    pub anchors: Option<AnchorAddrs>,
    pub scan_ms: Option<u128>,
    pub phase: String,
    pub phase_notice: String,
    pub scan: Option<ScanProgress>,
    pub lazer: Option<lazer::Diagnostics>,
}

impl Default for ReaderState {
    fn default() -> Self {
        ReaderState {
            snapshot: None,
            packet: None,
            holding: false,
            held_packet: None,
            frame_at_ms: 0,
            reason: None,
            health: HealthState::Idle.as_str().to_string(),
            frozen: false,
            degraded_fields: Vec::new(),
            strikes: 0,
            pid: None,
            image_path: None,
            client: None,
            anchors: None,
            scan_ms: None,
            phase: String::new(),
            phase_notice: String::new(),
            scan: None,
            lazer: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AnchorAddrs {
    pub status_ptr: u32,
    pub base_addr: u32,
    pub play_time_addr: u32,
    pub ruleset_addr: u32,
    pub menu_mods_ptr: u32,
    pub audio_length_ptr: u32,
    pub settings_class_addr: u32,
}

impl AnchorAddrs {
    pub fn from_table(table: &patterns::AnchorTable) -> AnchorAddrs {
        AnchorAddrs {
            status_ptr: table.status_ptr.unwrap_or(0),
            base_addr: table.base_addr.unwrap_or(0),
            play_time_addr: table.play_time_addr.unwrap_or(0),
            ruleset_addr: table.rulesets_addr.unwrap_or(0),
            menu_mods_ptr: table.menu_mods_ptr.unwrap_or(0),
            audio_length_ptr: table.audio_length_ptr.unwrap_or(0),
            settings_class_addr: table.settings_class_addr.unwrap_or(0),
        }
    }
}

/// 读取线程的句柄。
#[derive(Clone)]
pub struct Reader {
    pub(crate) inner: Arc<Mutex<ReaderState>>,
}

impl Reader {
    pub fn latest(&self) -> ReaderState {
        self.inner.lock().unwrap().clone()
    }

    pub fn snapshot(&self) -> Option<Snapshot> {
        self.inner.lock().unwrap().snapshot.clone()
    }

    pub fn reason(&self) -> Option<String> {
        self.inner.lock().unwrap().reason.clone()
    }

    pub fn packet(&self) -> Option<Value> {
        self.inner.lock().unwrap().packet.clone()
    }

    pub fn state(&self) -> ReaderState {
        self.latest()
    }
}

/// 启动读取线程。
pub fn spawn_osu_reader() -> Reader {
    let reader = Reader {
        inner: Arc::new(Mutex::new(ReaderState::default())),
    };
    let handle = reader.clone();
    thread::spawn(move || runner::run(handle));
    reader
}

pub static INSTANCE: std::sync::OnceLock<Reader> = std::sync::OnceLock::new();

pub fn instance() -> Option<Reader> {
    INSTANCE.get().cloned()
}

/// 逐候选自证（L1 结构校验）。
pub fn anchor_proves_out(
    target: &win::Target,
    key: &str,
    addr: u32,
    regions: &[scan::Region],
) -> bool {
    anchor_proves_out_with_topo(target, key, addr, regions, None)
}

pub fn anchor_proves_out_with_topo(
    target: &win::Target,
    key: &str,
    addr: u32,
    regions: &[scan::Region],
    topo: Option<&offsets::StableTopology>,
) -> bool {
    match key {
        "statusPtr" => win::read_pointer(target, addr)
            .map(|value| value <= STATE_INDEX_MAX)
            .unwrap_or(false),
        "baseAddr" => {
            let offset = topo.map(|t| t.beatmap_from_base).unwrap_or(0xC);
            win::read_pointer(target, addr.wrapping_sub(offset))
                .map(|object| object != 0 && region_contains(regions, object))
                .unwrap_or(false)
        }
        "playTimeAddr" => {
            let offset = topo.map(|t| t.play_time_from_anchor).unwrap_or(0x5);
            match win::read_u32(target, addr.wrapping_add(offset)) {
                Ok(slot) => {
                    if let Some(why) = plausible_object(regions, slot) {
                        eprintln!(
                            "[osu] diag {key} candidate=0x{addr:08X} reject=slot slot=0x{slot:08X} why={why}"
                        );
                        return false;
                    }
                    match win::read_u32(target, slot) {
                        Ok(value) => {
                            (value as i32).unsigned_abs()
                                <= invariants::PLAY_TIME_MAX_MS as u32
                        }
                        Err(_) => false,
                    }
                }
                Err(_) => false,
            }
        }
        "rulesetsAddr" => {
            let anchor_off = topo.map(|t| t.ruleset_from_anchor).unwrap_or(0xB);
            let list_off = topo.map(|t| t.ruleset_list_offset).unwrap_or(0x4);
            match win::read_u32(target, addr.wrapping_sub(anchor_off)) {
                Ok(slot) => {
                    if let Some(why) = plausible_object(regions, slot) {
                        eprintln!(
                            "[osu] diag {key} candidate=0x{addr:08X} reject=slot slot=0x{slot:08X} why={why}"
                        );
                        return false;
                    }
                    match win::read_u32(target, slot.wrapping_add(list_off)) {
                        Ok(ruleset) => {
                            if let Some(why) = plausible_object(regions, ruleset) {
                                eprintln!(
                                    "[osu] diag {key} candidate=0x{addr:08X} reject=ruleset ruleset=0x{ruleset:08X} why={why}"
                                );
                                return false;
                            }
                            match win::read_u32(target, ruleset) {
                                Ok(vtable) => plausible_object(regions, vtable).is_none(),
                                Err(_) => false,
                            }
                        }
                        Err(_) => false,
                    }
                }
                Err(_) => false,
            }
        }
        "menuModsPtr" => win::read_u32(target, addr)
            .map(|pointer| pointer != 0 && region_contains(regions, pointer))
            .unwrap_or(false),
        "getAudioLengthPtr" => {
            let offset = topo.map(|t| t.mp3_length_from_anchor).unwrap_or(0x7);
            match win::read_u32(target, addr.wrapping_add(offset)) {
                Ok(slot) => match plausible_object(regions, slot) {
                    None => match win::read_u32(target, slot) {
                        Ok(0) => true,
                        Ok(obj) => match plausible_object(regions, obj) {
                            None => true,
                            Some(why) => {
                                eprintln!(
                                    "[osu] diag {key} candidate=0x{addr:08X} reject=object obj=0x{obj:08X} why={why}"
                                );
                                false
                            }
                        },
                        Err(_) => false,
                    },
                    Some(why) => {
                        eprintln!(
                            "[osu] diag {key} candidate=0x{addr:08X} reject=slot slot=0x{slot:08X} why={why}"
                        );
                        false
                    }
                },
                Err(_) => false,
            }
        }
        "settingsClassAddr" => match win::read_pointer(target, addr) {
            Ok(object) => {
                if let Some(why) = plausible_object(regions, object) {
                    eprintln!(
                        "[osu] diag {key} candidate=0x{addr:08X} reject=object object=0x{object:08X} why={why}"
                    );
                    return false;
                }
                match win::read_u32(target, object) {
                    Ok(vtable) => {
                        if let Some(why) = plausible_object(regions, vtable) {
                            eprintln!(
                                "[osu] diag {key} candidate=0x{addr:08X} reject=vtable object=0x{object:08X} vtable=0x{vtable:08X} why={why}"
                            );
                            return false;
                        }
                        true
                    }
                    Err(reason) => {
                        eprintln!(
                            "[osu] diag {key} candidate=0x{addr:08X} reject=vtable-read object=0x{object:08X} reason={}",
                            reason.as_str()
                        );
                        false
                    }
                }
            }
            Err(reason) => {
                eprintln!(
                    "[osu] diag {key} candidate=0x{addr:08X} reject=object-read reason={}",
                    reason.as_str()
                );
                false
            }
        },
        _ => false,
    }
}

pub fn plausible_object(regions: &[scan::Region], pointer: u32) -> Option<&'static str> {
    if pointer == 0 {
        return Some("null");
    }
    if !patterns::is_aligned(pointer) {
        return Some("unaligned");
    }
    if pointer < 0x1_0000 {
        return Some("too-low");
    }
    if !region_contains(regions, pointer) {
        return Some("not-in-region");
    }
    None
}

pub fn region_contains(regions: &[scan::Region], addr: u32) -> bool {
    let addr = addr as usize;
    regions
        .iter()
        .any(|r| addr >= r.base && addr < r.base + r.size)
}

pub fn apply_hits(snapshot: &mut Snapshot) {
    apply_hits_with_topo(snapshot, None);
}

pub fn apply_hits_with_topo(snapshot: &mut Snapshot, topo: Option<&offsets::StableTopology>) {
    snapshot.play_hits = if invariants::play_hits_publishable(snapshot) {
        stable::hits_from_candidates_topo(&snapshot.hits_candidates, topo)
    } else {
        None
    };
    snapshot.result_hits = if invariants::result_hits_publishable(snapshot) {
        stable::hits_from_candidates_topo(&snapshot.result_hits_candidates, topo)
    } else {
        None
    };
}

pub fn fallback_songs_folder(image_path: &std::path::Path) -> Option<String> {
    let dir = image_path.parent()?;
    for candidate in [
        dir.join("Songs"),
        dir.join("..").join("Songs"),
        dir.join("Data").join("Songs"),
    ] {
        if candidate.is_dir() {
            return Some(candidate.to_string_lossy().to_string());
        }
    }
    None
}

pub fn resolve_songs_folder(image_path: &std::path::Path, value: &str) -> String {
    let candidate = match image_path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir.join(value),
        _ => std::path::PathBuf::from(value),
    };
    if candidate.is_dir() {
        candidate.to_string_lossy().to_string()
    } else {
        value.to_string()
    }
}

pub fn healthy(state: &ReaderState) -> bool {
    state.packet.is_some() && state.health != HealthState::Unhealthy.as_str()
}

pub fn describe(state: &ReaderState) -> String {
    match (&state.snapshot, &state.reason) {
        (Some(snapshot), _) => {
            let keys = keys::derive(snapshot);
            format!(
                "pid={:?} health={} frozen={} state={:?} live={:?} md5={:?} identity={}",
                state.pid,
                state.health,
                state.frozen,
                snapshot.state_name,
                snapshot.play_time,
                snapshot.checksum,
                keys.identity
            )
        }
        (_, Some(reason)) => format!("unavailable: {reason}"),
        _ => format!("unavailable: <no snapshot> (health={})", state.health),
    }
}

pub fn latest_pair(reader: &Reader) -> (Option<Snapshot>, Option<String>) {
    let state = reader.latest();
    (state.snapshot, state.reason)
}

pub fn state_json(reader: &Reader) -> Value {
    let state = reader.latest();
    let keys = state
        .snapshot
        .as_ref()
        .map(keys::derive)
        .unwrap_or_default();
    serde_json::json!({
        "pid": state.pid,
        "client": state.client,
        "health": state.health,
        "frozen": state.frozen,
        "holding": state.holding,
        "strikes": state.strikes,
        "reason": state.reason,
        "packet_present": state.packet.is_some(),
        "state_name": state.snapshot.as_ref().and_then(|s| s.state_name.clone()),
        "checksum": state.snapshot.as_ref().and_then(|s| s.checksum.clone()),
        "identity": keys.identity,
        "mod_signature": keys.mod_signature,
        "songs_cfg_value": state.snapshot.as_ref().and_then(|s| s.songs_cfg_value.clone()),
        "songs_folder": state.snapshot.as_ref().and_then(|s| s.songs_folder.clone()),
        "first_object": state.snapshot.as_ref().and_then(|s| s.first_object()),
        "last_object": state.snapshot.as_ref().and_then(|s| s.last_object()),
        "degraded_fields": state.snapshot.as_ref().map(|s| s.degraded_fields.clone()).unwrap_or_default(),
        "lazer": state.lazer.as_ref().map(lazer::Diagnostics::to_json),
    })
}

#[cfg(test)]
#[path = "../../tests-local/osu_beatmap_corpus.rs"]
mod tests_beatmap_corpus;
#[cfg(all(test, windows))]
#[path = "../../tests-local/osu_hold.rs"]
mod tests_hold;
#[cfg(test)]
#[path = "../../tests-local/osu_keys.rs"]
mod tests_keys;
#[cfg(test)]
#[path = "../../tests-local/osu_model.rs"]
mod tests_model;
#[cfg(test)]
#[path = "../../tests-local/osu_scan.rs"]
mod tests_scan;
#[cfg(test)]
#[path = "../../tests-local/osu_anchor_cache.rs"]
mod tests_anchor_cache;
#[cfg(test)]
#[path = "../../tests-local/osu_sweep.rs"]
mod tests_sweep;
#[cfg(test)]
#[path = "../../tests-local/osu_phase.rs"]
mod tests_phase;
