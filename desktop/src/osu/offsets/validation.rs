use super::lazer_table::OffsetTable;
use super::lookup::{MAX_FIELD_OFFSET, MIN_FIELD_OFFSET};
use super::stable_table::StableTable;

/// Schema 不变量违规错误
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValidationError {
    ExecutableContent(String),
    HopDepthExceeded(usize),
    DisplacementOutOfRange(String, i64),
    StringTooLong(String, usize),
    InvalidPattern(String),
    UnknownStateName(String),
    InvalidAlignment(String, i64),
    MissingRequiredField(String),
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ValidationError::ExecutableContent(desc) => {
                write!(f, "validation-error: executable content detected ({desc})")
            }
            ValidationError::HopDepthExceeded(depth) => {
                write!(f, "validation-error: hop depth {depth} exceeds limit 16")
            }
            ValidationError::DisplacementOutOfRange(field, val) => {
                write!(f, "validation-error: displacement out of range: {field}={val}")
            }
            ValidationError::StringTooLong(field, len) => {
                write!(f, "validation-error: string too long: {field} (len {len} > 512)")
            }
            ValidationError::InvalidPattern(pat) => {
                write!(f, "validation-error: invalid pattern: {pat}")
            }
            ValidationError::UnknownStateName(name) => {
                write!(f, "validation-error: unknown state name '{name}' not in allowed set")
            }
            ValidationError::InvalidAlignment(field, val) => {
                write!(f, "validation-error: unaligned offset: {field}={val}")
            }
            ValidationError::MissingRequiredField(field) => {
                write!(f, "validation-error: missing required field '{field}'")
            }
        }
    }
}

pub const ALLOWED_STATE_NAMES: &[&str] = &[
    "menu",
    "edit",
    "play",
    "selectEdit",
    "selectPlay",
    "resultScreen",
    "resultsScreen",
    "multiplayer",
    "unknown",
    "",
];

pub fn check_string_safety(field: &str, s: &str) -> Result<(), ValidationError> {
    check_string_safety_bounded(field, s, 512)
}

pub fn check_long_string_safety(field: &str, s: &str) -> Result<(), ValidationError> {
    check_string_safety_bounded(field, s, 65_536)
}

pub fn check_string_safety_bounded(field: &str, s: &str, max_len: usize) -> Result<(), ValidationError> {
    if s.len() > max_len {
        return Err(ValidationError::StringTooLong(field.to_string(), s.len()));
    }
    let lower = s.to_ascii_lowercase();
    for needle in ["<script", "javascript:", "eval(", "exec(", "onload=", "onerror="] {
        if lower.contains(needle) {
            return Err(ValidationError::ExecutableContent(format!(
                "{field} contains forbidden token '{needle}'"
            )));
        }
    }
    Ok(())
}

pub fn check_pattern_safety(field: &str, pattern: &str) -> Result<(), ValidationError> {
    check_string_safety(field, pattern)?;
    let tokens: Vec<&str> = pattern.split_whitespace().collect();
    if tokens.is_empty() || tokens.len() > 64 {
        return Err(ValidationError::InvalidPattern(format!(
            "{field}: token count {} out of range 1..=64",
            tokens.len()
        )));
    }
    for token in tokens {
        if token == "??" {
            continue;
        }
        if token.len() != 2 || !token.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(ValidationError::InvalidPattern(format!(
                "{field}: invalid token '{token}' in pattern"
            )));
        }
    }
    Ok(())
}

pub fn validate_stable_schema(table: &StableTable) -> Result<(), ValidationError> {
    check_string_safety("client", &table.client)?;
    check_string_safety("version", &table.version)?;
    check_string_safety("arch", &table.arch)?;
    check_string_safety("verified_build", &table.verified_build)?;
    check_long_string_safety("evidence", &table.evidence)?;

    if table.anchors.is_empty() || table.anchors.len() > 16 {
        return Err(ValidationError::HopDepthExceeded(table.anchors.len()));
    }

    for (key, def) in &table.anchors {
        check_pattern_safety(&format!("anchor.{key}.pattern"), &def.pattern)?;
        check_long_string_safety(&format!("anchor.{key}.derivation"), &def.derivation)?;
        check_long_string_safety(&format!("anchor.{key}.evidence"), &def.evidence)?;
        if def.offset < -4096 || def.offset > 1_048_576 {
            return Err(ValidationError::DisplacementOutOfRange(
                format!("anchor.{key}.offset"),
                def.offset as i64,
            ));
        }
    }

    let topo = &table.topology;
    for (name, val) in [
        ("play_time_from_anchor", topo.play_time_from_anchor),
        ("beatmap_from_base", topo.beatmap_from_base),
        ("info_from_base", topo.info_from_base),
        ("retries_offset", topo.retries_offset),
        ("plays_offset", topo.plays_offset),
        ("ruleset_from_anchor", topo.ruleset_from_anchor),
        ("ruleset_list_offset", topo.ruleset_list_offset),
        ("gameplay_from_ruleset", topo.gameplay_from_ruleset),
        ("result_from_ruleset", topo.result_from_ruleset),
        ("score_from_gameplay", topo.score_from_gameplay),
        ("mods_container", topo.mods_container),
        ("mods_xor_high", topo.mods_xor_high),
        ("mods_xor_low", topo.mods_xor_low),
        ("score_processor_from_score", topo.score_processor_from_score),
        ("result_score_offset", topo.result_score_offset),
        ("result_max_combo_offset", topo.result_max_combo_offset),
        ("result_player_name_offset", topo.result_player_name_offset),
        ("result_online_id_offset", topo.result_online_id_offset),
        ("mp3_length_from_anchor", topo.mp3_length_from_anchor),
        ("mp3_length_field", topo.mp3_length_field),
        ("beatmap_md5", topo.beatmap_md5),
        ("beatmap_filename", topo.beatmap_filename),
        ("beatmap_folder", topo.beatmap_folder),
        ("beatmap_version", topo.beatmap_version),
        ("beatmap_artist", topo.beatmap_artist),
        ("beatmap_title", topo.beatmap_title),
        ("beatmap_mapper", topo.beatmap_mapper),
        ("beatmap_id", topo.beatmap_id),
        ("beatmap_set_id", topo.beatmap_set_id),
    ] {
        if val > 1_048_576 {
            return Err(ValidationError::DisplacementOutOfRange(name.to_string(), val as i64));
        }
    }

    if topo.hits_candidate_offsets.len() > 16 {
        return Err(ValidationError::HopDepthExceeded(topo.hits_candidate_offsets.len()));
    }
    for (i, offset) in topo.hits_candidate_offsets.iter().enumerate() {
        if *offset > 1_048_576 {
            return Err(ValidationError::DisplacementOutOfRange(
                format!("hits_candidate_offsets[{i}]"),
                *offset as i64,
            ));
        }
        if *offset % 2 != 0 {
            return Err(ValidationError::InvalidAlignment(
                format!("hits_candidate_offsets[{i}]"),
                *offset as i64,
            ));
        }
    }

    if topo.hits_slot_mapping.len() > 16 {
        return Err(ValidationError::HopDepthExceeded(topo.hits_slot_mapping.len()));
    }

    for (idx_str, name) in &table.mappings.states {
        check_string_safety("mappings.states.key", idx_str)?;
        check_string_safety("mappings.states.val", name)?;
        if !ALLOWED_STATE_NAMES.contains(&name.as_str()) {
            return Err(ValidationError::UnknownStateName(name.clone()));
        }
    }

    Ok(())
}

pub fn validate_lazer_schema(table: &OffsetTable) -> Result<(), ValidationError> {
    check_string_safety("lazer_version", &table.lazer_version)?;
    check_string_safety("runtime_version", &table.runtime_version)?;
    check_string_safety("arch", &table.arch)?;
    check_string_safety("verified_build", &table.verified_build)?;
    check_long_string_safety("evidence", &table.evidence)?;

    if let Some(anchors) = &table.anchors {
        check_pattern_safety("anchors.marker_pattern", &anchors.marker_pattern)?;
        if anchors.site_deltas.len() > 16 {
            return Err(ValidationError::HopDepthExceeded(anchors.site_deltas.len()));
        }
        for (i, delta) in anchors.site_deltas.iter().enumerate() {
            if *delta < -4096 || *delta > 4096 {
                return Err(ValidationError::DisplacementOutOfRange(
                    format!("anchors.site_deltas[{i}]"),
                    *delta,
                ));
            }
        }
        if anchors.game_base_hops.len() > 16 {
            return Err(ValidationError::HopDepthExceeded(anchors.game_base_hops.len()));
        }
        for (label, offset) in &anchors.game_base_hops {
            check_string_safety("anchors.game_base_hops.label", label)?;
            if *offset > 16_777_216 {
                return Err(ValidationError::DisplacementOutOfRange(
                    format!("anchors.game_base_hops.{label}"),
                    *offset as i64,
                ));
            }
        }
    }

    if let Some(states) = &table.screen_states {
        if states.len() > 128 {
            return Err(ValidationError::HopDepthExceeded(states.len()));
        }
        for (screen, state) in states {
            check_string_safety("screen_states.key", screen)?;
            check_string_safety("screen_states.val", state)?;
            if !ALLOWED_STATE_NAMES.contains(&state.as_str()) {
                return Err(ValidationError::UnknownStateName(state.clone()));
            }
        }
    }

    for (type_name, fields) in &table.types {
        check_string_safety("types.key", type_name)?;
        for (field_name, offset) in fields {
            check_string_safety("types.field", field_name)?;
            if *offset < 0 || *offset > MAX_FIELD_OFFSET * 16 {
                return Err(ValidationError::DisplacementOutOfRange(
                    format!("{type_name}.{field_name}"),
                    *offset,
                ));
            }
        }
    }

    if let Some(runtime) = &table.runtime {
        check_long_string_safety("runtime.witness", &runtime.witness)?;
        for (group_name, map) in [
            ("eetype", &runtime.eetype),
            ("module", &runtime.module),
            ("screen_array", &runtime.screen_array),
        ] {
            for (entry_name, entry) in map {
                check_long_string_safety(&format!("runtime.{group_name}.{entry_name}.witness"), &entry.witness)?;
                if entry.offset < 0 || entry.offset > MAX_FIELD_OFFSET * 16 {
                    return Err(ValidationError::DisplacementOutOfRange(
                        format!("runtime.{group_name}.{entry_name}.offset"),
                        entry.offset,
                    ));
                }
            }
        }
    }

    if let Some(results) = &table.results {
        check_string_safety("results.witness", &results.witness)?;
        check_results_offset("results.score_offset", results.score_offset)?;
        if let Some(offset) = results.statistics_offset {
            check_results_offset("results.statistics_offset", offset)?;
        }
        if let Some(offset) = results.player_offset {
            check_results_offset("results.player_offset", offset)?;
        }
        if let Some(offset) = results.statistics_dict_offset {
            check_results_offset("results.statistics_dict_offset", offset)?;
        }
        if let Some(offset) = results.dict_entries_offset {
            check_results_offset("results.dict_entries_offset", offset)?;
        }
        for (key, value) in &results.hit_result_values {
            check_string_safety("results.hit_result_values.key", key)?;
            if *value > 64 {
                return Err(ValidationError::DisplacementOutOfRange(
                    format!("results.hit_result_values.{key}"),
                    *value as i64,
                ));
            }
        }
        // `usable()` 的"至少一个 hits 来源"必须在这里也成立：否则一张只有
        // score_offset+player_offset 的表会通过校验、读侧却永远读不到 hits。
        if !results.usable() {
            return Err(ValidationError::DisplacementOutOfRange(
                "results (no usable hits source)".to_string(),
                0,
            ));
        }
        // 字典路径要么两个位移都有、要么都没有（只写一半等于坏了）。
        if results.statistics_dict_offset.is_some() != results.dict_entries_offset.is_some() {
            return Err(ValidationError::DisplacementOutOfRange(
                "results (statistics_dict_offset/dict_entries_offset must be paired)".to_string(),
                0,
            ));
        }
        // 有映射就必须六个判定键齐全且序号互不相同（缺键会把判定静默算成 0）。
        if !results.hit_result_values.is_empty() {
            let mut seen = std::collections::BTreeSet::new();
            for key in ["perfect", "great", "good", "ok", "meh", "miss"] {
                let Some(value) = results.hit_result_values.get(key) else {
                    return Err(ValidationError::DisplacementOutOfRange(
                        format!("results.hit_result_values.{key}"),
                        0,
                    ));
                };
                if !seen.insert(*value) {
                    return Err(ValidationError::DisplacementOutOfRange(
                        format!("results.hit_result_values.{key}"),
                        *value as i64,
                    ));
                }
            }
        }
    }

    if let Some(mods) = &table.mods {
        check_results_offset("mods.bindable_value", mods.bindable_value)?;
        check_results_offset("mods.difficulty_current", mods.difficulty_current)?;
        if mods.settings.is_empty() {
            return Err(ValidationError::DisplacementOutOfRange(
                "mods.settings (empty)".to_string(),
                0,
            ));
        }
        for (acronym, fields) in &mods.settings {
            check_string_safety("mods.settings.key", acronym)?;
            if acronym.is_empty()
                || acronym.len() > 8
                || !acronym
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
            {
                return Err(ValidationError::DisplacementOutOfRange(
                    format!("mods.settings.{acronym}"),
                    0,
                ));
            }
            if fields.is_empty() {
                return Err(ValidationError::DisplacementOutOfRange(
                    format!("mods.settings.{acronym} (empty)"),
                    0,
                ));
            }
            for (name, offset) in fields {
                check_string_safety("mods.settings.field", name)?;
                if !super::lazer_table::ModsSection::KNOWN_SETTINGS.contains(&name.as_str()) {
                    return Err(ValidationError::DisplacementOutOfRange(
                        format!("mods.settings.{acronym}.{name}"),
                        *offset,
                    ));
                }
                check_results_offset(&format!("mods.settings.{acronym}.{name}"), *offset)?;
            }
        }
        if !mods.usable() {
            return Err(ValidationError::DisplacementOutOfRange(
                "mods (unusable)".to_string(),
                0,
            ));
        }
    }

    Ok(())
}

/// `results` 段的位移判据：在字段合理域内，且 8 字节对齐（引用字段必然对齐）。
fn check_results_offset(field: &str, offset: i64) -> Result<(), ValidationError> {
    if !(MIN_FIELD_OFFSET..=MAX_FIELD_OFFSET).contains(&offset) {
        return Err(ValidationError::DisplacementOutOfRange(
            field.to_string(),
            offset,
        ));
    }
    if offset % 8 != 0 {
        return Err(ValidationError::InvalidAlignment(field.to_string(), offset));
    }
    Ok(())
}
