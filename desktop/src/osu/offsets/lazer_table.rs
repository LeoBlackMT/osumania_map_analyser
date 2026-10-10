use std::collections::BTreeMap;
use super::lookup::{canonical_type, FieldLookup, LookupError, MAX_FIELD_OFFSET, MIN_FIELD_OFFSET};
use super::validation::validate_lazer_schema;

pub const DEFAULT_LAZER_TABLE_JSON: &str = include_str!("../../../offsets/lazer/2026.1005.0.0__10.0.12__x64.json");

/// lazer 锚点与多跳拓扑段（可选，未配置时使用既有静态默认值）
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct LazerAnchorsSection {
    #[serde(default = "default_lazer_marker_pattern")]
    pub marker_pattern: String,
    #[serde(default = "default_lazer_site_deltas")]
    pub site_deltas: Vec<i64>,
    #[serde(default = "default_lazer_game_base_hops")]
    pub game_base_hops: Vec<(String, u64)>,
}

fn default_lazer_marker_pattern() -> String {
    "01 01 00 00 00 00 80 44 00 00 40 44".to_string()
}

fn default_lazer_site_deltas() -> Vec<i64> {
    vec![0x24, 0x28, 0x2c, 0x20, 0x30, 0x1c, 0x34]
}

fn default_lazer_game_base_hops() -> Vec<(String, u64)> {
    vec![
        ("external_link_opener".to_string(), 0x0),
        ("api_access".to_string(), 0x218),
        ("game".to_string(), 0x310),
    ]
}

/// 一张偏移表（一个 `(lazer 版本, runtime 版本, 架构)` 组合一份）。
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct OffsetTable {
    pub lazer_version: String,
    pub runtime_version: String,
    pub arch: String,
    pub game_base_vtable: Option<u64>,
    #[serde(default)]
    pub anchors: Option<LazerAnchorsSection>,
    #[serde(default)]
    pub screen_states: Option<BTreeMap<String, String>>,
    pub types: BTreeMap<String, BTreeMap<String, i64>>,
    #[serde(default)]
    pub runtime: Option<RuntimeSection>,
    pub verified_build: String,
    pub evidence: String,
}

/// `runtime` 段里的一个位移。
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct RuntimeEntry {
    pub offset: i64,
    #[serde(default)]
    pub shift: u32,
    #[serde(default)]
    pub stride: i64,
    #[serde(default)]
    pub witness: String,
}

impl RuntimeEntry {
    pub fn usable(&self) -> bool {
        self.offset >= 0 && self.offset <= MAX_FIELD_OFFSET
    }
}

/// 运行时结构段。每一组都是 `名字 → 位移`。
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct RuntimeSection {
    #[serde(default)]
    pub eetype: BTreeMap<String, RuntimeEntry>,
    #[serde(default)]
    pub module: BTreeMap<String, RuntimeEntry>,
    #[serde(default)]
    pub screen_array: BTreeMap<String, RuntimeEntry>,
    #[serde(default)]
    pub typedefs: BTreeMap<String, BTreeMap<String, String>>,
    #[serde(default)]
    pub observed: BTreeMap<String, BTreeMap<String, String>>,
    #[serde(default)]
    pub witness: String,
}

impl RuntimeSection {
    pub fn entry(&self, group: &str, name: &str) -> Option<&RuntimeEntry> {
        let map = match group {
            "eetype" => &self.eetype,
            "module" => &self.module,
            "screen_array" => &self.screen_array,
            _ => return None,
        };
        map.get(name).filter(|entry| entry.usable())
    }

    pub fn type_name(&self, module: &str, rid: u32) -> Option<&str> {
        self.typedefs
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(module))
            .and_then(|(_, map)| map.get(&format!("{rid:X}")))
            .map(|value| value.as_str())
            .filter(|value| !value.trim().is_empty())
    }

    pub fn modules(&self) -> Vec<&str> {
        self.typedefs.keys().map(|key| key.as_str()).collect()
    }
}

pub const TABLE_KEYS: &[&str] = &[
    "lazer_version",
    "runtime_version",
    "arch",
    "game_base_vtable",
    "types",
    "runtime",
    "verified_build",
    "evidence",
];

/// 表的加载错误：字面量。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadError {
    Json(String),
    EmptyField(&'static str),
    Validation(String),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Json(message) => write!(f, "offsets-load: {message}"),
            LoadError::EmptyField(field) => write!(f, "offsets-load: empty {field}"),
            LoadError::Validation(message) => write!(f, "offsets-load: {message}"),
        }
    }
}

impl OffsetTable {
    pub fn load(bytes: &[u8]) -> Result<OffsetTable, LoadError> {
        let table: OffsetTable =
            serde_json::from_slice(bytes).map_err(|e| LoadError::Json(e.to_string()))?;
        for (field, value) in [
            ("lazer_version", &table.lazer_version),
            ("runtime_version", &table.runtime_version),
            ("arch", &table.arch),
            ("verified_build", &table.verified_build),
            ("evidence", &table.evidence),
        ] {
            if value.trim().is_empty() {
                return Err(LoadError::EmptyField(field));
            }
        }
        validate_lazer_schema(&table).map_err(|e| LoadError::Validation(e.to_string()))?;
        Ok(table)
    }

    pub fn marker_pattern(&self) -> &str {
        self.anchors
            .as_ref()
            .map(|a| a.marker_pattern.as_str())
            .unwrap_or("01 01 00 00 00 00 80 44 00 00 40 44")
    }

    pub fn site_deltas(&self) -> &[i64] {
        self.anchors
            .as_ref()
            .map(|a| a.site_deltas.as_slice())
            .unwrap_or(&[0x24, 0x28, 0x2c, 0x20, 0x30, 0x1c, 0x34])
    }

    pub fn game_base_hops(&self) -> Vec<(&str, u64)> {
        if let Some(anchors) = &self.anchors {
            anchors
                .game_base_hops
                .iter()
                .map(|(label, offset)| (label.as_str(), *offset))
                .collect()
        } else {
            vec![
                ("external_link_opener", 0x0),
                ("api_access", 0x218),
                ("game", 0x310),
            ]
        }
    }

    pub fn screen_state_for(&self, type_name: &str) -> Option<&str> {
        if let Some(states) = &self.screen_states {
            if let Some(name) = states.get(type_name) {
                return Some(name.as_str());
            }
        }
        None
    }

    pub fn key(&self) -> String {
        format!(
            "{}/{}/{}",
            self.lazer_version, self.runtime_version, self.arch
        )
    }

    pub fn offset(&self, type_name: &str, field: &str) -> Option<i64> {
        self.types.get(type_name)?.get(field).copied()
    }

    pub fn runtime(&self) -> Option<&RuntimeSection> {
        self.runtime.as_ref()
    }

    pub fn runtime_entry(&self, group: &str, name: &str) -> Option<&RuntimeEntry> {
        self.runtime.as_ref()?.entry(group, name)
    }

    pub fn runtime_type_name(&self, module: &str, rid: u32) -> Option<&str> {
        self.runtime.as_ref()?.type_name(module, rid)
    }

    pub fn offset_for(&self, lookup: &FieldLookup) -> Result<i64, LookupError> {
        let all: Vec<String> = lookup
            .type_all
            .iter()
            .map(|text| canonical_type(text))
            .collect();
        let any: Vec<String> = lookup
            .type_any
            .iter()
            .map(|text| canonical_type(text))
            .collect();
        let mut matches: Vec<(String, i64)> = Vec::new();
        for (key, fields) in &self.types {
            let canonical = canonical_type(key);
            let all_hit = all.iter().all(|needle| canonical.contains(needle.as_str()));
            let any_hit = any.is_empty() || any.iter().any(|needle| canonical.contains(needle.as_str()));
            if !(all_hit && any_hit) {
                continue;
            }
            if let Some(offset) = fields.get(lookup.field).copied() {
                matches.push((canonical, offset));
            } else {
                matches.push((canonical, i64::MIN));
            }
        }
        match matches.len() {
            0 => Err(LookupError::NoType(lookup.label())),
            1 => {
                let (key, offset) = matches.remove(0);
                if offset == i64::MIN {
                    return Err(LookupError::NoField(key, lookup.field.to_string()));
                }
                if !(MIN_FIELD_OFFSET..=MAX_FIELD_OFFSET).contains(&offset) {
                    return Err(LookupError::OutOfRange(lookup.label(), offset));
                }
                Ok(offset)
            }
            _ => {
                let keys: Vec<String> = matches.into_iter().map(|(key, _)| key).collect();
                Err(LookupError::Ambiguous(lookup.label(), keys))
            }
        }
    }

    pub fn mismatch(&self, lazer_version: &str, runtime_version: &str, arch: &str) -> Option<String> {
        if !versions_match(&self.lazer_version, lazer_version) {
            return Some(format!("version:{}", self.lazer_version));
        }
        if !versions_match(&self.runtime_version, runtime_version) {
            return Some(format!("runtime:{}", self.runtime_version));
        }
        if !self.arch.eq_ignore_ascii_case(arch) {
            return Some(format!("arch:{}", self.arch));
        }
        None
    }

    pub fn target(lazer_version: &str, runtime_version: &str, arch: &str) -> Target {
        Target {
            lazer_version: lazer_version.to_string(),
            runtime_version: runtime_version.to_string(),
            arch: arch.to_string(),
        }
    }

    pub fn nearest_table<'a>(
        tables: &'a [OffsetTable],
        target: &Target,
        max_distance: u32,
        policy: &ValidationPolicy<'_>,
    ) -> Result<&'a OffsetTable, NearestError> {
        if tables.is_empty() {
            return Err(NearestError::NoCandidates);
        }
        let mut ranked: Vec<(u32, &OffsetTable)> = tables
            .iter()
            .filter_map(|table| version_distance(table, target).map(|d| (d, table)))
            .collect();
        if ranked.is_empty() {
            return Err(NearestError::NoCandidates);
        }
        ranked.sort_by(|a, b| {
            a.0.cmp(&b.0)
                .then_with(|| a.1.lazer_version.cmp(&b.1.lazer_version))
                .then_with(|| a.1.runtime_version.cmp(&b.1.runtime_version))
        });
        let (distance, nearest) = ranked[0];
        if distance > max_distance {
            return Err(NearestError::TooFar {
                distance,
                max_distance,
            });
        }
        match policy.validate(nearest) {
            true => Ok(nearest),
            false => Err(NearestError::Refused(nearest.key())),
        }
    }

    pub fn field_labels(&self) -> Vec<String> {
        let mut labels: Vec<String> = Vec::new();
        for (key, fields) in &self.types {
            for field in fields.keys() {
                labels.push(format!("{key}.{field}"));
            }
        }
        labels
    }
}

/// 目标环境（就回落要匹配的键）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub lazer_version: String,
    pub runtime_version: String,
    pub arch: String,
}

/// 回落被拒的原因。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NearestError {
    NoCandidates,
    TooFar { distance: u32, max_distance: u32 },
    Refused(String),
}

impl std::fmt::Display for NearestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NearestError::NoCandidates => write!(f, "nearest: no candidate"),
            NearestError::TooFar {
                distance,
                max_distance,
            } => write!(f, "nearest: distance {distance} > {max_distance}"),
            NearestError::Refused(key) => write!(f, "nearest: refused by policy ({key})"),
        }
    }
}

/// 校验钩子。默认实现恒拒。
pub struct ValidationPolicy<'a> {
    validate: &'a dyn Fn(&OffsetTable) -> bool,
}

impl Default for ValidationPolicy<'static> {
    fn default() -> Self {
        Self::refuse()
    }
}

impl<'a> ValidationPolicy<'a> {
    pub fn new(validate: &'a dyn Fn(&OffsetTable) -> bool) -> Self {
        ValidationPolicy { validate }
    }

    pub fn refuse() -> Self {
        ValidationPolicy {
            validate: &|_table: &OffsetTable| false,
        }
    }

    pub fn validate(&self, table: &OffsetTable) -> bool {
        (self.validate)(table)
    }
}

pub fn normalize_version(v: &str) -> String {
    let clean = v.split('-').next().unwrap_or(v).trim();
    let parts: Vec<u32> = clean
        .split('.')
        .map(|p| p.parse::<u32>().unwrap_or(0))
        .collect();
    if parts.is_empty() {
        return clean.to_string();
    }
    let mut end = parts.len();
    while end > 3 && parts[end - 1] == 0 {
        end -= 1;
    }
    parts[..end]
        .iter()
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(".")
}

pub fn versions_match(a: &str, b: &str) -> bool {
    if a.eq_ignore_ascii_case(b) {
        return true;
    }
    normalize_version(a) == normalize_version(b)
}

fn version_distance(table: &OffsetTable, target: &Target) -> Option<u32> {
    if !table.arch.eq_ignore_ascii_case(&target.arch) {
        return None;
    }
    Some(component_distance(&table.lazer_version, &target.lazer_version).saturating_add(
        component_distance(&table.runtime_version, &target.runtime_version),
    ))
}

fn component_distance(left: &str, right: &str) -> u32 {
    let left: Vec<u32> = left.split('.').map(parse_component).collect();
    let right: Vec<u32> = right.split('.').map(parse_component).collect();
    let len = left.len().max(right.len());
    let mut total = 0u32;
    for index in 0..len {
        let a = left.get(index).copied().unwrap_or(0);
        let b = right.get(index).copied().unwrap_or(0);
        total = total.saturating_add(a.abs_diff(b));
    }
    total
}

fn parse_component(text: &str) -> u32 {
    text.trim().parse::<u32>().unwrap_or(0)
}

pub fn default_lazer_table() -> OffsetTable {
    OffsetTable::load(DEFAULT_LAZER_TABLE_JSON.as_bytes())
        .expect("compiled-in lazer table must be valid")
}
