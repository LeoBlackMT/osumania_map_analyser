use crate::osu::model::Snapshot;
use crate::osu::offsets::{FieldLookup, LookupError, OffsetTable};
use super::fields::*;
use super::source::{read_ptr_field, StringLayout};
use super::table::{LoadedTable, TableOrigin, TargetInfo};
use std::path::PathBuf;

/// lazer 解引用链的地址（诊断/证据用；不进载荷）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChainAddrs {
    pub game_base: u64,
    pub vtable: u64,
    pub storage: Option<u64>,
    pub base_path: Option<u64>,
    pub beatmap_bindable: Option<u64>,
    pub working_beatmap: Option<u64>,
    pub beatmap_info: Option<u64>,
    pub beatmap_set: Option<u64>,
    pub metadata: Option<u64>,
    pub realm_user: Option<u64>,
    pub screen_stack: Option<u64>,
    pub screen_stack_list: Option<u64>,
    pub screen_stack_array: Option<u64>,
    pub screen_top: Option<u64>,
    pub screen_top_vtable: Option<u64>,
    pub screen_top_module: Option<u64>,
    pub screen_top_image_base: Option<u64>,
    pub selected_mods: Option<u64>,
    pub beatmap_clock: Option<u64>,
    pub beatmap_track_clock: Option<u64>,
}

impl ChainAddrs {
    pub fn to_json(&self) -> serde_json::Value {
        let hex = |value: Option<u64>| value.map(|v| format!("0x{v:016X}"));
        serde_json::json!({
            "game_base": format!("0x{:016X}", self.game_base),
            "vtable": format!("0x{:016X}", self.vtable),
            "storage": hex(self.storage),
            "base_path": hex(self.base_path),
            "beatmap_bindable": hex(self.beatmap_bindable),
            "working_beatmap": hex(self.working_beatmap),
            "beatmap_info": hex(self.beatmap_info),
            "beatmap_set": hex(self.beatmap_set),
            "metadata": hex(self.metadata),
            "realm_user": hex(self.realm_user),
            "screen_stack": hex(self.screen_stack),
            "screen_stack_list": hex(self.screen_stack_list),
            "screen_stack_array": hex(self.screen_stack_array),
            "screen_top": hex(self.screen_top),
            "screen_top_vtable": hex(self.screen_top_vtable),
            "screen_top_module": hex(self.screen_top_module),
            "screen_top_image_base": hex(self.screen_top_image_base),
            "selected_mods": hex(self.selected_mods),
            "beatmap_clock": hex(self.beatmap_clock),
            "beatmap_track_clock": hex(self.beatmap_track_clock),
        })
    }
}

/// L0 的产物：表 + gameBase + 会话内的 L1 证明状态。
#[derive(Clone, Debug)]
pub struct Attach {
    pub table: OffsetTable,
    pub origin: TableOrigin,
    pub table_path: PathBuf,
    pub table_mismatch: Option<String>,
    pub target: TargetInfo,
    pub game_base: u64,
    pub anchors: Vec<u64>,
    pub proof: SessionProof,
    pub modules: Vec<(u64, String)>,
    pub marker_hits: usize,
    pub scan_ms: u128,
}

/// 逐帧诊断。
#[derive(Clone, Debug, Default)]
pub struct Diagnostics {
    pub table_key: Option<String>,
    pub table_origin: Option<String>,
    pub table_path: Option<String>,
    pub table_mismatch: Option<String>,
    pub game_base: Option<u64>,
    pub marker_hits: Option<usize>,
    pub scan_ms: Option<u128>,
    pub chain: Option<ChainAddrs>,
    pub modules: Option<usize>,
    pub gaps: Vec<String>,
}

impl Diagnostics {
    pub fn from_attach(attach: &Attach) -> Diagnostics {
        Diagnostics {
            table_key: Some(attach.table.key()),
            table_origin: Some(attach.origin.as_str().to_string()),
            table_path: Some(attach.table_path.to_string_lossy().to_string()),
            table_mismatch: attach.table_mismatch.clone(),
            game_base: Some(attach.game_base),
            marker_hits: Some(attach.marker_hits),
            scan_ms: Some(attach.scan_ms),
            chain: None,
            modules: Some(attach.modules.len()),
            gaps: Vec::new(),
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "table_key": self.table_key,
            "table_origin": self.table_origin,
            "table_path": self.table_path,
            "table_mismatch": self.table_mismatch,
            "game_base": self.game_base.map(|value| format!("0x{value:016X}")),
            "marker_hits": self.marker_hits,
            "scan_ms": self.scan_ms,
            "chain": self.chain.map(|chain| chain.to_json()),
            "modules": self.modules,
            "gaps": self.gaps,
        })
    }
}

/// 会话内的 L1 证明状态。
#[derive(Clone, Debug, Default)]
pub struct SessionProof {
    vtable: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProofStep {
    First,
    Stable,
    Changed { previous: u64 },
}

impl SessionProof {
    pub fn session_vtable(&self) -> Option<u64> {
        self.vtable
    }

    pub fn observe(&mut self, method_table: u64) -> ProofStep {
        match self.vtable {
            None => {
                self.vtable = Some(method_table);
                ProofStep::First
            }
            Some(current) if current == method_table => ProofStep::Stable,
            Some(current) => ProofStep::Changed {
                previous: current,
            },
        }
    }

    pub fn adopt(&mut self, method_table: u64) {
        self.vtable = Some(method_table);
    }
}

/// 表里的字段偏移。
#[derive(Clone, Debug)]
pub struct Resolved {
    pub storage: Result<i64, LookupError>,
    pub base_path: Result<i64, LookupError>,
    pub beatmap: Result<i64, LookupError>,
    pub bindable_value: Result<i64, LookupError>,
    pub working_beatmap_info: Result<i64, LookupError>,
    pub beatmap_md5: Result<i64, LookupError>,
    pub beatmap_hash: Result<i64, LookupError>,
    pub beatmap_online_id: Result<i64, LookupError>,
    pub beatmap_difficulty_name: Result<i64, LookupError>,
    pub beatmap_metadata: Result<i64, LookupError>,
    pub beatmap_set: Result<i64, LookupError>,
    pub set_online_id: Result<i64, LookupError>,
    pub metadata_title: Result<i64, LookupError>,
    pub metadata_artist: Result<i64, LookupError>,
    pub metadata_author: Result<i64, LookupError>,
    pub realm_username: Result<i64, LookupError>,
    pub screen_stack: Result<i64, LookupError>,
    pub screen_stack_list: Result<i64, LookupError>,
    pub screen_stack_array: Result<i64, LookupError>,
    pub screen_stack_size: Result<i64, LookupError>,
    pub selected_mods: Result<i64, LookupError>,
    pub beatmap_clock: Result<i64, LookupError>,
    pub beatmap_track_clock: Result<i64, LookupError>,
    pub beatmap_clock_time: Result<i64, LookupError>,
    pub string_length: Result<i64, LookupError>,
    pub string_chars: Result<i64, LookupError>,
}

impl Resolved {
    pub fn from_table(table: &OffsetTable) -> Resolved {
        Resolved {
            storage: table.offset_for(&F_STORAGE),
            base_path: table.offset_for(&F_STORAGE_BASE_PATH),
            beatmap: table.offset_for(&F_BEATMAP),
            bindable_value: table.offset_for(&F_BEATMAP_BINDABLE_VALUE),
            working_beatmap_info: table.offset_for(&F_WORKING_BEATMAP_INFO),
            beatmap_md5: table.offset_for(&F_BEATMAP_INFO_MD5),
            beatmap_hash: table.offset_for(&F_BEATMAP_INFO_HASH),
            beatmap_online_id: table.offset_for(&F_BEATMAP_INFO_ONLINE_ID),
            beatmap_difficulty_name: table.offset_for(&F_BEATMAP_INFO_DIFFICULTY_NAME),
            beatmap_metadata: table.offset_for(&F_BEATMAP_INFO_METADATA),
            beatmap_set: table.offset_for(&F_BEATMAP_INFO_SET),
            set_online_id: table.offset_for(&F_SET_ONLINE_ID),
            metadata_title: table.offset_for(&F_METADATA_TITLE),
            metadata_artist: table.offset_for(&F_METADATA_ARTIST),
            metadata_author: table.offset_for(&F_METADATA_AUTHOR),
            realm_username: table.offset_for(&F_REALM_USERNAME),
            screen_stack: table.offset_for(&F_SCREEN_STACK),
            screen_stack_list: table.offset_for(&F_SCREEN_STACK_LIST),
            screen_stack_array: table.offset_for(&F_SCREEN_STACK_ARRAY),
            screen_stack_size: table.offset_for(&F_SCREEN_STACK_SIZE),
            selected_mods: table.offset_for(&F_SELECTED_MODS),
            beatmap_clock: table.offset_for(&F_BEATMAP_CLOCK),
            beatmap_track_clock: table.offset_for(&F_BEATMAP_TRACK_CLOCK),
            beatmap_clock_time: table.offset_for(&F_BEATMAP_CLOCK_TIME),
            string_length: table.offset_for(&F_STRING_LENGTH),
            string_chars: table.offset_for(&F_STRING_CHARS),
        }
    }

    pub fn string_layout(&self) -> Result<StringLayout, LookupError> {
        let length = self.string_length.clone()?;
        let chars = self.string_chars.clone()?;
        Ok(StringLayout { length, chars })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VtableChange {
    pub previous: Option<u64>,
    pub observed: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct LazerFrame {
    pub snapshot: Snapshot,
    pub chain: ChainAddrs,
    pub gaps: Vec<String>,
    pub reidentified: Option<VtableChange>,
}

pub struct FrameInput<'a> {
    pub table: &'a OffsetTable,
    pub source: &'a dyn super::source::Source,
    pub pid: u32,
    pub game_base: u64,
    pub anchors: &'a [u64],
    pub proof: &'a mut SessionProof,
    pub songs_folder: Option<String>,
    pub game_folder: Option<String>,
    pub modules: &'a [(u64, String)],
}
