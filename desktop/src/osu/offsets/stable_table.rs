use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use crate::osu::model::Reason;
use super::lazer_table::LoadError;
use super::validation::validate_stable_schema;

pub const DEFAULT_STABLE_TABLE_JSON: &str = include_str!("../../../offsets/stable/stable__x86.json");

/// stable 锚点定义
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct StableAnchorDef {
    pub pattern: String,
    pub offset: i32,
    #[serde(default)]
    pub derivation: String,
    #[serde(default)]
    pub evidence: String,
}

/// stable 多跳链路与偏移拓扑
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct StableTopology {
    #[serde(default = "default_play_time_from_anchor")]
    pub play_time_from_anchor: u32,
    #[serde(default = "default_beatmap_from_base")]
    pub beatmap_from_base: u32,
    #[serde(default = "default_info_from_base")]
    pub info_from_base: u32,
    #[serde(default = "default_retries_offset")]
    pub retries_offset: u32,
    #[serde(default = "default_plays_offset")]
    pub plays_offset: u32,
    #[serde(default = "default_ruleset_from_anchor")]
    pub ruleset_from_anchor: u32,
    #[serde(default = "default_ruleset_list_offset")]
    pub ruleset_list_offset: u32,
    #[serde(default = "default_gameplay_from_ruleset")]
    pub gameplay_from_ruleset: u32,
    #[serde(default = "default_result_from_ruleset")]
    pub result_from_ruleset: u32,
    #[serde(default = "default_score_from_gameplay")]
    pub score_from_gameplay: u32,
    #[serde(default = "default_mods_container")]
    pub mods_container: u32,
    #[serde(default = "default_mods_xor_high")]
    pub mods_xor_high: u32,
    #[serde(default = "default_mods_xor_low")]
    pub mods_xor_low: u32,
    #[serde(default = "default_score_processor_from_score")]
    pub score_processor_from_score: u32,
    #[serde(default = "default_scorev2_bit")]
    pub scorev2_bit: u32,
    #[serde(default = "default_result_score_offset")]
    pub result_score_offset: u32,
    #[serde(default = "default_result_max_combo_offset")]
    pub result_max_combo_offset: u32,
    #[serde(default = "default_result_player_name_offset")]
    pub result_player_name_offset: u32,
    #[serde(default = "default_result_online_id_offset")]
    pub result_online_id_offset: u32,
    #[serde(default = "default_mp3_length_from_anchor")]
    pub mp3_length_from_anchor: u32,
    #[serde(default = "default_mp3_length_field")]
    pub mp3_length_field: u32,
    #[serde(default = "default_hits_candidate_offsets")]
    pub hits_candidate_offsets: Vec<u32>,
    #[serde(default = "default_hits_slot_mapping")]
    pub hits_slot_mapping: Vec<(usize, String)>,
    #[serde(default = "default_beatmap_md5")]
    pub beatmap_md5: u32,
    #[serde(default = "default_beatmap_filename")]
    pub beatmap_filename: u32,
    #[serde(default = "default_beatmap_folder")]
    pub beatmap_folder: u32,
    #[serde(default = "default_beatmap_version")]
    pub beatmap_version: u32,
    #[serde(default = "default_beatmap_artist")]
    pub beatmap_artist: u32,
    #[serde(default = "default_beatmap_title")]
    pub beatmap_title: u32,
    #[serde(default = "default_beatmap_mapper")]
    pub beatmap_mapper: u32,
    #[serde(default = "default_beatmap_id")]
    pub beatmap_id: u32,
    #[serde(default = "default_beatmap_set_id")]
    pub beatmap_set_id: u32,
}

fn default_play_time_from_anchor() -> u32 { 0x5 }
fn default_beatmap_from_base() -> u32 { 0xC }
fn default_info_from_base() -> u32 { 0x33 }
fn default_retries_offset() -> u32 { 0x8 }
fn default_plays_offset() -> u32 { 0xC }
fn default_ruleset_from_anchor() -> u32 { 0xB }
fn default_ruleset_list_offset() -> u32 { 0x4 }
fn default_gameplay_from_ruleset() -> u32 { 0x64 }
fn default_result_from_ruleset() -> u32 { 0x38 }
fn default_score_from_gameplay() -> u32 { 0x38 }
fn default_mods_container() -> u32 { 0x1C }
fn default_mods_xor_high() -> u32 { 0xC }
fn default_mods_xor_low() -> u32 { 0x8 }
fn default_score_processor_from_score() -> u32 { 0x54 }
fn default_scorev2_bit() -> u32 { 0x2000_0000 }
fn default_result_score_offset() -> u32 { 0x78 }
fn default_result_max_combo_offset() -> u32 { 0x68 }
fn default_result_player_name_offset() -> u32 { 0x28 }
fn default_result_online_id_offset() -> u32 { 0x4 }
fn default_mp3_length_from_anchor() -> u32 { 0x7 }
fn default_mp3_length_field() -> u32 { 0x4 }
fn default_beatmap_md5() -> u32 { 0x6C }
fn default_beatmap_filename() -> u32 { 0x90 }
fn default_beatmap_folder() -> u32 { 0x78 }
fn default_beatmap_version() -> u32 { 0xAC }
fn default_beatmap_artist() -> u32 { 0x18 }
fn default_beatmap_title() -> u32 { 0x24 }
fn default_beatmap_mapper() -> u32 { 0x7C }
fn default_beatmap_id() -> u32 { 0xC8 }
fn default_beatmap_set_id() -> u32 { 0xCC }
fn default_hits_candidate_offsets() -> Vec<u32> {
    vec![0x88, 0x8A, 0x8C, 0x8E, 0x90, 0x92, 0x94, 0x68]
}
fn default_hits_slot_mapping() -> Vec<(usize, String)> {
    vec![
        (0, "100".to_string()),
        (1, "300".to_string()),
        (2, "50".to_string()),
        (3, "geki".to_string()),
        (4, "katu".to_string()),
        (5, "miss".to_string()),
    ]
}

impl Default for StableTopology {
    fn default() -> Self {
        StableTopology {
            play_time_from_anchor: default_play_time_from_anchor(),
            beatmap_from_base: default_beatmap_from_base(),
            info_from_base: default_info_from_base(),
            retries_offset: default_retries_offset(),
            plays_offset: default_plays_offset(),
            ruleset_from_anchor: default_ruleset_from_anchor(),
            ruleset_list_offset: default_ruleset_list_offset(),
            gameplay_from_ruleset: default_gameplay_from_ruleset(),
            result_from_ruleset: default_result_from_ruleset(),
            score_from_gameplay: default_score_from_gameplay(),
            mods_container: default_mods_container(),
            mods_xor_high: default_mods_xor_high(),
            mods_xor_low: default_mods_xor_low(),
            score_processor_from_score: default_score_processor_from_score(),
            scorev2_bit: default_scorev2_bit(),
            result_score_offset: default_result_score_offset(),
            result_max_combo_offset: default_result_max_combo_offset(),
            result_player_name_offset: default_result_player_name_offset(),
            result_online_id_offset: default_result_online_id_offset(),
            mp3_length_from_anchor: default_mp3_length_from_anchor(),
            mp3_length_field: default_mp3_length_field(),
            hits_candidate_offsets: default_hits_candidate_offsets(),
            hits_slot_mapping: default_hits_slot_mapping(),
            beatmap_md5: default_beatmap_md5(),
            beatmap_filename: default_beatmap_filename(),
            beatmap_folder: default_beatmap_folder(),
            beatmap_version: default_beatmap_version(),
            beatmap_artist: default_beatmap_artist(),
            beatmap_title: default_beatmap_title(),
            beatmap_mapper: default_beatmap_mapper(),
            beatmap_id: default_beatmap_id(),
            beatmap_set_id: default_beatmap_set_id(),
        }
    }
}

/// stable 映射段（状态名枚举、mods 映射等）
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct StableMappings {
    #[serde(default = "default_stable_states")]
    pub states: BTreeMap<String, String>,
}

fn default_stable_states() -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    map.insert("0".to_string(), "menu".to_string());
    map.insert("1".to_string(), "edit".to_string());
    map.insert("2".to_string(), "play".to_string());
    map.insert("4".to_string(), "selectEdit".to_string());
    map.insert("5".to_string(), "selectPlay".to_string());
    map.insert("7".to_string(), "resultScreen".to_string());
    map
}

impl Default for StableMappings {
    fn default() -> Self {
        StableMappings {
            states: default_stable_states(),
        }
    }
}

/// stable 偏移表（纯数据模型，对应 stable__x86.json）
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct StableTable {
    #[serde(default = "default_stable_client")]
    pub client: String,
    pub version: String,
    pub arch: String,
    pub anchors: BTreeMap<String, StableAnchorDef>,
    #[serde(default)]
    pub topology: StableTopology,
    #[serde(default)]
    pub mappings: StableMappings,
    pub verified_build: String,
    pub evidence: String,
}

fn default_stable_client() -> String {
    "stable".to_string()
}

impl StableTable {
    pub fn load(bytes: &[u8]) -> Result<StableTable, LoadError> {
        let table: StableTable =
            serde_json::from_slice(bytes).map_err(|e| LoadError::Json(e.to_string()))?;
        for (field, value) in [
            ("client", &table.client),
            ("version", &table.version),
            ("arch", &table.arch),
            ("verified_build", &table.verified_build),
            ("evidence", &table.evidence),
        ] {
            if value.trim().is_empty() {
                return Err(LoadError::EmptyField(field));
            }
        }
        validate_stable_schema(&table).map_err(|e| LoadError::Validation(e.to_string()))?;
        Ok(table)
    }

    pub fn anchor(&self, key: &str) -> Option<&StableAnchorDef> {
        self.anchors.get(key)
    }

    pub fn state_name(&self, index: i32) -> Option<&str> {
        self.mappings.states.get(&index.to_string()).map(|s| s.as_str())
    }
}

pub fn default_stable_table() -> StableTable {
    StableTable::load(DEFAULT_STABLE_TABLE_JSON.as_bytes())
        .expect("compiled-in stable table must be valid")
}

/// stable 表发现梯子
pub fn find_stable_table(dir_hint: Option<&Path>) -> Result<StableTable, Reason> {
    // 1. 显式环境变量 $MMA_STABLE_OFFSETS
    if let Ok(env_file) = std::env::var("MMA_STABLE_OFFSETS") {
        let p = PathBuf::from(env_file);
        if p.exists() {
            if let Ok(bytes) = std::fs::read(&p) {
                if let Ok(table) = StableTable::load(&bytes) {
                    return Ok(table);
                }
            }
        }
    }

    // 2. 统一偏移目录环境变量 $MMA_OFFSETS_DIR
    if let Ok(offsets_dir) = std::env::var("MMA_OFFSETS_DIR") {
        let p = PathBuf::from(offsets_dir).join("stable").join("stable__x86.json");
        if p.exists() {
            if let Ok(bytes) = std::fs::read(&p) {
                if let Ok(table) = StableTable::load(&bytes) {
                    return Ok(table);
                }
            }
        }
    }

    // 3. APPDATA 缓存目录
    if let Ok(appdata) = std::env::var("APPDATA") {
        let p = PathBuf::from(appdata)
            .join("ManiaMapAnalyser")
            .join("offsets")
            .join("stable")
            .join("stable__x86.json");
        if p.exists() {
            if let Ok(bytes) = std::fs::read(&p) {
                if let Ok(table) = StableTable::load(&bytes) {
                    return Ok(table);
                }
            }
        }
    }

    // 4. dir_hint 目录（如 exe 同级目录）
    if let Some(hint) = dir_hint {
        let candidates = [
            hint.join("offsets").join("stable").join("stable__x86.json"),
            hint.join("offset").join("stable").join("stable__x86.json"),
        ];
        for p in &candidates {
            if p.exists() {
                if let Ok(bytes) = std::fs::read(p) {
                    if let Ok(table) = StableTable::load(&bytes) {
                        return Ok(table);
                    }
                }
            }
        }
    }

    // 5. 当前可执行文件同级目录
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            let p = exe_dir.join("offsets").join("stable").join("stable__x86.json");
            if p.exists() {
                if let Ok(bytes) = std::fs::read(&p) {
                    if let Ok(table) = StableTable::load(&bytes) {
                        return Ok(table);
                    }
                }
            }
        }
    }

    // 6. 内置编译期兜底（Zero IO 永不失败）
    Ok(default_stable_table())
}
