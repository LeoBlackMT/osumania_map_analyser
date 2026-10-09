// osu::invariants::rules - 不变量规则定义、字段校验与评判器

use std::collections::HashSet;
use std::time::Duration;

use crate::osu::model::{Hits, Reason, Snapshot};
use crate::osu::stable;

pub const STATE_INDEX_MAX: i32 = 15;
pub const STRING_MAX_UNITS: usize = 512;
pub const ID_MAX: i32 = 1_000_000_000;
pub const PLAY_TIME_MAX_MS: i32 = 86_400_000;
pub const TIME_WINDOW_MS: i32 = 5_000;
pub const BACKWARD_JUMP_WINDOW_MS: u64 = 10_000;
pub const PLAY_TIME_LEAD_IN_MS: i32 = 10_000;
pub const PLAY_TIME_TAIL_MS: i32 = 5_000;
pub const IDENTITY_HOLD_GRACE: Duration = Duration::from_millis(2_500);
pub const HOLDABLE_HARD_FIELDS: &[&str] = &["beatmap.object"];
pub const KNOWN_MOD_BITS: u32 = (1u32 << 31) - 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Invariant {
    pub id: &'static str,
    pub kind: Kind,
    pub cadence: Cadence,
    pub field: &'static str,
    pub description: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Hard,
    Soft,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cadence {
    EveryFrame,
    OnTransition,
}

pub const INVARIANTS: &[Invariant] = &[
    Invariant {
        id: "I-01",
        kind: Kind::Hard,
        cadence: Cadence::EveryFrame,
        field: "state.name",
        description: "state.name 必须是 15 个已知场景名之一",
    },
    Invariant {
        id: "I-02",
        kind: Kind::Hard,
        cadence: Cadence::EveryFrame,
        field: "beatmap.checksum",
        description: "beatmap.checksum 非空时必须恰好 32 位 hex",
    },
    Invariant {
        id: "I-03",
        kind: Kind::Soft,
        cadence: Cadence::EveryFrame,
        field: "beatmap.path",
        description: "osuPath/folder/filename 组合必须是可信的相对路径",
    },
    Invariant {
        id: "I-04",
        kind: Kind::Soft,
        cadence: Cadence::EveryFrame,
        field: "play.time",
        description: "局内 play.time 单调前推（允许至多 10s 一次回跳）",
    },
    Invariant {
        id: "I-05",
        kind: Kind::Soft,
        cadence: Cadence::OnTransition,
        field: "play.time.transition",
        description: "进入 play/result 时 play.time 必须落在合理的初始窗口",
    },
    Invariant {
        id: "I-06",
        kind: Kind::Hard,
        cadence: Cadence::EveryFrame,
        field: "pointers",
        description: "当前场景要求的指针必须非空且落在用户区",
    },
    Invariant {
        id: "I-07",
        kind: Kind::Soft,
        cadence: Cadence::EveryFrame,
        field: "beatmap.id",
        description: "beatmap.id 与 setId 必须在合理正整数范围内",
    },
    Invariant {
        id: "I-08",
        kind: Kind::Soft,
        cadence: Cadence::EveryFrame,
        field: "strings",
        description: "所有文本字段不得含 NUL、长度不得超限",
    },
    Invariant {
        id: "I-09",
        kind: Kind::Soft,
        cadence: Cadence::EveryFrame,
        field: "beatmap.checksum.disk",
        description: "磁盘读出的 .osu 计算所得 md5 必须与内存 checksum 一致",
    },
    Invariant {
        id: "I-10",
        kind: Kind::Soft,
        cadence: Cadence::EveryFrame,
        field: "mods.bits",
        description: "mods 掩码不得含未定义的高位",
    },
];

pub fn invariant(id: &str) -> &'static Invariant {
    INVARIANTS
        .iter()
        .find(|inv| inv.id == id)
        .unwrap_or_else(|| panic!("unknown invariant {}", id))
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FrameVerdict {
    pub hard: Vec<&'static str>,
    pub soft: Vec<&'static str>,
    pub degraded_fields: Vec<String>,
}

impl FrameVerdict {
    pub fn is_clean(&self) -> bool {
        self.hard.is_empty() && self.soft.is_empty()
    }

    pub fn is_hard_failure(&self) -> bool {
        !self.hard.is_empty()
    }

    pub fn to_reason(&self) -> Option<Reason> {
        if let Some(&first) = self.hard.first() {
            return Some(Reason::InvariantViolated(first));
        }
        if let Some(&first) = self.soft.first() {
            return Some(Reason::InvariantViolated(first));
        }
        None
    }
}

pub fn is_holdable_failure(verdict: &FrameVerdict) -> bool {
    if verdict.hard.is_empty() {
        return false;
    }
    verdict
        .hard
        .iter()
        .all(|failed| HOLDABLE_HARD_FIELDS.contains(failed))
}

#[derive(Default)]
pub struct History {
    pub last_play_time: Option<i32>,
    pub last_jump_ms: Option<u64>,
}

impl History {
    pub fn reset(&mut self) {
        self.last_play_time = None;
        self.last_jump_ms = None;
    }

    pub fn check_play_time(&mut self, current: Option<i32>, now_ms: u64) -> Option<&'static str> {
        let current = current?;
        if current < -PLAY_TIME_MAX_MS || current > PLAY_TIME_MAX_MS {
            return Some(invariant("I-04").field);
        }
        if let Some(last) = self.last_play_time {
            if current < last {
                if let Some(jump) = self.last_jump_ms {
                    if now_ms.saturating_sub(jump) < BACKWARD_JUMP_WINDOW_MS {
                        return Some(invariant("I-04").field);
                    }
                }
                self.last_jump_ms = Some(now_ms);
            }
        }
        self.last_play_time = Some(current);
        None
    }

    pub fn play_time_in_range(live_ms: i32, length_ms: i32) -> bool {
        if length_ms <= 0 {
            return true;
        }
        let lower = -PLAY_TIME_LEAD_IN_MS;
        let upper = length_ms.saturating_add(PLAY_TIME_TAIL_MS);
        live_ms >= lower && live_ms <= upper
    }
}

pub fn i01_state_name(snapshot: &Snapshot) -> FrameVerdict {
    let mut verdict = FrameVerdict::default();
    let index = snapshot.state_index.unwrap_or(-1);
    if index < 0 || index > STATE_INDEX_MAX {
        verdict.hard.push(invariant("I-01").field);
    }
    verdict
}

pub fn i02_md5_shape(checksum: Option<&str>) -> FrameVerdict {
    let mut verdict = FrameVerdict::default();
    if let Some(cs) = checksum {
        let ok = cs.len() == 32 && cs.chars().all(|c| c.is_ascii_hexdigit());
        if !ok {
            verdict.hard.push(invariant("I-02").field);
        }
    }
    verdict
}

pub fn filename_is_plausible(filename: &str) -> bool {
    let trim = filename.trim();
    if trim.is_empty() {
        return false;
    }
    if !trim.ends_with(".osu") {
        return false;
    }
    if trim.contains('/') || trim.contains('\\') || trim.contains("..") {
        return false;
    }
    true
}

pub fn folder_is_plausible(folder: &str) -> bool {
    let trim = folder.trim();
    if trim.is_empty() {
        return false;
    }
    if trim.contains("..") {
        return false;
    }
    if trim.starts_with('/') || trim.starts_with('\\') {
        return false;
    }
    if trim.len() >= 2 && trim.as_bytes()[1] == b':' {
        return false;
    }
    true
}

pub fn i03_path_consistency(snapshot: &Snapshot) -> FrameVerdict {
    let mut verdict = FrameVerdict::default();
    if let Some(filename) = snapshot.beatmap_filename.as_deref() {
        if !filename_is_plausible(filename) {
            verdict.soft.push(invariant("I-03").field);
            verdict
                .degraded_fields
                .push("beatmap.filename.shape".to_string());
        }
    }
    if let Some(folder) = snapshot.beatmap_folder.as_deref() {
        if !folder_is_plausible(folder) {
            verdict.soft.push(invariant("I-03").field);
            verdict
                .degraded_fields
                .push("beatmap.folder.shape".to_string());
        }
    }
    verdict
}

pub fn i05_time_window(snapshot: &Snapshot) -> FrameVerdict {
    let mut verdict = FrameVerdict::default();
    if snapshot.is_play_state() {
        if let Some(time) = snapshot.play_time {
            if time.abs() > TIME_WINDOW_MS {
                verdict.soft.push(invariant("I-05").field);
            }
        }
    }
    verdict
}

pub fn pointer_is_plausible(pointer: u32) -> bool {
    (0x10000..=0x7FFE_0000).contains(&pointer) && (pointer & 0x3) == 0
}

pub fn pointer_is_plausible_u64(pointer: u64) -> bool {
    (0x10000..=0x7FFF_FFFF_0000).contains(&pointer) && (pointer & 0x7) == 0
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExpectedPointers {
    pub ruleset_ptr_valid: bool,
    pub beatmap_ptr_valid: bool,
}

impl ExpectedPointers {
    pub fn are_valid(&self) -> bool {
        self.ruleset_ptr_valid && self.beatmap_ptr_valid
    }
}

impl Default for ExpectedPointers {
    fn default() -> Self {
        Self {
            ruleset_ptr_valid: true,
            beatmap_ptr_valid: true,
        }
    }
}

pub fn expected_pointers(snapshot: &Snapshot) -> ExpectedPointers {
    if snapshot.client == Some(crate::osu::model::Client::Lazer) {
        return ExpectedPointers {
            ruleset_ptr_valid: true,
            beatmap_ptr_valid: match snapshot.beatmap_object_addr {
                Some(addr) => pointer_is_plausible_u64(addr),
                None => false,
            },
        };
    }
    let beatmap_ok = match snapshot.beatmap_ptr {
        Some(ptr) => pointer_is_plausible(ptr),
        None => true,
    };
    let ruleset_ok = if snapshot.is_play_state() || snapshot.is_result_state() {
        match snapshot.ruleset_ptr {
            Some(ptr) => pointer_is_plausible(ptr),
            None => false,
        }
    } else {
        true
    };
    ExpectedPointers {
        ruleset_ptr_valid: ruleset_ok,
        beatmap_ptr_valid: beatmap_ok,
    }
}

pub fn i06_pointers(snapshot: &Snapshot) -> FrameVerdict {
    let mut verdict = FrameVerdict::default();
    let expected = expected_pointers(snapshot);
    if !expected.ruleset_ptr_valid {
        verdict.hard.push("ruleset.pointer");
    }
    if !expected.beatmap_ptr_valid {
        verdict.hard.push("beatmap.object");
    }
    if let Some(mask) = snapshot.play_mods_mask {
        if !play_chain_valid(snapshot) {
            verdict.soft.push("play.chain");
            verdict
                .degraded_fields
                .push(format!("play.mods.chain-broken:0x{mask:08X}"));
        }
    }
    if let Some(mask) = snapshot.result_mods_mask {
        if !result_chain_valid(snapshot) {
            verdict.soft.push("resultsScreen.chain");
            verdict
                .degraded_fields
                .push(format!("resultsScreen.mods.chain-broken:0x{mask:08X}"));
        }
    }
    verdict
}

pub fn play_chain_valid(snapshot: &Snapshot) -> bool {
    snapshot.play_chain_valid.unwrap_or(true)
}

pub fn result_chain_valid(snapshot: &Snapshot) -> bool {
    snapshot.result_chain_valid.unwrap_or(true)
}

pub fn play_hits_publishable(snapshot: &Snapshot) -> bool {
    if !snapshot.is_play_state() {
        return false;
    }
    snapshot.play_hits.is_some() && play_chain_valid(snapshot)
}

pub fn result_hits_publishable(snapshot: &Snapshot) -> bool {
    if !snapshot.is_result_state() {
        return false;
    }
    snapshot.result_hits.is_some() && result_chain_valid(snapshot)
}

pub fn id_in_range(id: i32) -> bool {
    (0..=ID_MAX).contains(&id)
}

pub fn i07_id_range(snapshot: &Snapshot) -> FrameVerdict {
    let mut verdict = FrameVerdict::default();
    if let Some(id) = snapshot.beatmap_id {
        if !id_in_range(id) {
            verdict.soft.push(invariant("I-07").field);
        }
    }
    if let Some(set_id) = snapshot.beatmap_set_id {
        if !id_in_range(set_id) {
            verdict.soft.push(invariant("I-07").field);
        }
    }
    verdict
}

pub fn i08_strings(snapshot: &Snapshot) -> FrameVerdict {
    let mut verdict = FrameVerdict::default();
    let strings = [
        snapshot.beatmap_title.as_deref(),
        snapshot.beatmap_artist.as_deref(),
        snapshot.beatmap_mapper.as_deref(),
        snapshot.beatmap_version.as_deref(),
    ];
    for s in strings.into_iter().flatten() {
        if s.contains('\0') || s.len() > STRING_MAX_UNITS {
            verdict.soft.push(invariant("I-08").field);
            break;
        }
    }
    verdict
}

pub fn identity_corroborated(snapshot: &Snapshot) -> bool {
    snapshot.identity_corroborated.unwrap_or(true)
}

pub fn i09_md5_disk(snapshot: &Snapshot) -> FrameVerdict {
    let mut verdict = FrameVerdict::default();
    if !identity_corroborated(snapshot) {
        verdict.soft.push(invariant("I-09").field);
    }
    verdict
}

pub fn i10_mod_bits(snapshot: &Snapshot) -> FrameVerdict {
    let mut verdict = FrameVerdict::default();
    let unknown = [
        snapshot.menu_mods_mask,
        snapshot.play_mods_mask,
        snapshot.result_mods_mask,
    ]
    .into_iter()
    .flatten()
    .fold(0u32, |acc, mask| acc | (mask & !KNOWN_MOD_BITS));
    if unknown != 0 {
        verdict.soft.push(invariant("I-10").field);
        verdict.degraded_fields.push(format!("mods.unknown-bits:0x{unknown:08X}"));
    }
    verdict
}

pub fn evaluate(
    snapshot: &Snapshot,
    history: &mut History,
    now_ms: u64,
    transitioned: bool,
) -> FrameVerdict {
    let mut verdict = FrameVerdict::default();
    merge(&mut verdict, i01_state_name(snapshot));
    merge(&mut verdict, i02_md5_shape(snapshot.checksum.as_deref()));
    merge(&mut verdict, i03_path_consistency(snapshot));
    merge(&mut verdict, i04_play_time(snapshot, history, now_ms));
    if transitioned {
        merge(&mut verdict, i05_time_window(snapshot));
    }
    merge(&mut verdict, i06_pointers(snapshot));
    merge(&mut verdict, i07_id_range(snapshot));
    merge(&mut verdict, i08_strings(snapshot));
    merge(&mut verdict, i09_md5_disk(snapshot));
    merge(&mut verdict, i10_mod_bits(snapshot));
    for field in &snapshot.degraded_fields {
        if !verdict.degraded_fields.contains(field) {
            verdict.degraded_fields.push(field.clone());
        }
    }
    if snapshot.play_hits.is_some() || snapshot.result_hits.is_some() {
        if !stable::HITS_MAPPING_VERIFIED {
            verdict
                .degraded_fields
                .push("play.hits.mapping".to_string());
        }
    }
    verdict
}

pub fn i04_play_time(snapshot: &Snapshot, history: &mut History, now_ms: u64) -> FrameVerdict {
    let mut verdict = FrameVerdict::default();
    if let Some(field) = history.check_play_time(snapshot.play_time, now_ms) {
        verdict.soft.push(field);
    }
    if let Some(live) = snapshot.play_time {
        let in_play_or_result = snapshot.is_play_state() || snapshot.is_result_state();
        let length_ms = snapshot.mp3_length_ms.unwrap_or(0);
        if in_play_or_result && length_ms > 0 && !History::play_time_in_range(live, length_ms) {
            verdict.soft.push(invariant("I-04").field);
            verdict.degraded_fields.push(format!(
                "play.time.out-of-range:{live}ms/mp3:{length_ms}ms"
            ));
        }
    }
    verdict
}

pub fn merge(into: &mut FrameVerdict, from: FrameVerdict) {
    into.hard.extend(from.hard);
    for field in from.soft {
        if !into.soft.contains(&field) {
            into.soft.push(field);
        }
    }
    into.degraded_fields.extend(from.degraded_fields);
}
