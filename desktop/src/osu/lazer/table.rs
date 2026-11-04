use std::path::{Path, PathBuf};
use crate::osu::model::{Client, Reason};
use crate::osu::offsets::{OffsetTable, Target, ValidationPolicy};
use super::fields::{ARCH_X64, ARCH_X86, ENV_TABLE, FILES_DIR, NEAREST_MAX_DISTANCE, TABLE_DIR};

/// 只读文件视图。
pub trait TableFiles {
    fn read(&self, path: &Path) -> Option<Vec<u8>>;
    fn list_json(&self, dir: &Path) -> Vec<PathBuf>;
}

/// 真实文件系统（产品路径）。
pub struct RealFiles;

impl TableFiles for RealFiles {
    fn read(&self, path: &Path) -> Option<Vec<u8>> {
        std::fs::read(path).ok()
    }

    fn list_json(&self, dir: &Path) -> Vec<PathBuf> {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut out: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .map(|ext| ext.eq_ignore_ascii_case("json"))
                    .unwrap_or(false)
            })
            .collect();
        out.sort();
        out
    }
}

/// 目标环境（版本键 + 两个只读文件的落点）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TargetEnv {
    pub exe_dir: PathBuf,
    pub storage_ini: Option<PathBuf>,
    pub arch: String,
}

/// 目标环境的实测读数。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TargetInfo {
    pub lazer_version: String,
    pub runtime_version: String,
    pub arch: String,
    pub storage_root: Option<String>,
}

impl TargetInfo {
    pub fn target(&self) -> Target {
        OffsetTable::target(&self.lazer_version, &self.runtime_version, &self.arch)
    }

    pub fn songs_folder(&self) -> Option<String> {
        self.storage_root
            .as_deref()
            .map(|root| format!("{root}\\{}", FILES_DIR))
    }
}

pub fn target_from_files(files: &dyn TableFiles, env: &TargetEnv) -> TargetInfo {
    let lazer_version = files
        .read(&env.exe_dir.join("sq.version"))
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|text| tag_value(&text, "version"))
        .unwrap_or_default();
    let runtime_version = files
        .read(&env.exe_dir.join("osu!.runtimeconfig.json"))
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|text| runtime_version_from_config(&text))
        .unwrap_or_default();
    let storage_root = env.storage_ini.as_deref().and_then(|path| {
        let bytes = files.read(path)?;
        let text = String::from_utf8(bytes).ok()?;
        ini_value(&text, "FullPath")
    });
    TargetInfo {
        lazer_version,
        runtime_version,
        arch: env.arch.clone(),
        storage_root,
    }
}

fn tag_value(text: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = text.find(&open)? + open.len();
    let end = text[start..].find(&close)? + start;
    let value = text[start..end].trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn runtime_version_from_config(text: &str) -> Option<String> {
    for marker in ["includedFrameworks", "\"framework\""] {
        let Some(at) = text.find(marker) else {
            continue;
        };
        let rest = &text[at..];
        let Some(key_at) = rest.find("\"version\"") else {
            continue;
        };
        let after = &rest[key_at + "\"version\"".len()..];
        let colon = after.find(':')?;
        let quoted = after[colon + 1..].trim_start();
        if let Some(value) = quoted_string(quoted) {
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    None
}

fn ini_value(text: &str, key: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        let Some((left, right)) = line.split_once('=') else {
            continue;
        };
        if left.trim().eq_ignore_ascii_case(key) {
            let value = right.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn quoted_string(text: &str) -> Option<String> {
    let text = text.strip_prefix('"')?;
    let end = text.find('"')?;
    Some(text[..end].to_string())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableOrigin {
    Env,
    Exact,
    SameKey,
    Nearest,
}

impl TableOrigin {
    pub fn as_str(self) -> &'static str {
        match self {
            TableOrigin::Env => "env",
            TableOrigin::Exact => "exe-dir",
            TableOrigin::SameKey => "same-key",
            TableOrigin::Nearest => "nearest",
        }
    }
}

#[derive(Clone, Debug)]
pub struct LoadedTable {
    pub table: OffsetTable,
    pub origin: TableOrigin,
    pub path: PathBuf,
    pub mismatch: Option<String>,
}

pub fn table_file_name(lazer_version: &str, runtime_version: &str, arch: &str) -> String {
    format!(
        "{}__{}__{}.json",
        sanitize(lazer_version),
        sanitize(runtime_version),
        sanitize(arch)
    )
}

fn sanitize(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

pub fn load_table(
    files: &dyn TableFiles,
    env_path: Option<&Path>,
    table_dir: &Path,
    info: &TargetInfo,
    prove: &dyn Fn(&OffsetTable) -> bool,
) -> Result<LoadedTable, Reason> {
    let missing = || Reason::LazerOffsetsMissing(info.lazer_version.clone());

    // ① 显式路径（$MMA_LAZER_OFFSETS）
    if let Some(path) = env_path {
        match files.read(path).map(|bytes| OffsetTable::load(&bytes)) {
            Some(Ok(table)) => match usable(&table) {
                Ok(()) => {
                    let mismatch = table.mismatch(
                        &info.lazer_version,
                        &info.runtime_version,
                        &info.arch,
                    );
                    if let Some(detail) = &mismatch {
                        eprintln!(
                            "[osu] lazer offsets: table from ${ENV_TABLE} ({}) does not match the target key ({detail}) — accepting because the operator pointed at it explicitly",
                            table.key()
                        );
                    }
                    return Ok(LoadedTable {
                        table,
                        origin: TableOrigin::Env,
                        path: path.to_path_buf(),
                        mismatch,
                    });
                }
                Err(why) => eprintln!(
                    "[osu] lazer offsets: table from ${ENV_TABLE} rejected: {why} ({})",
                    table.key()
                ),
            },
            Some(Err(error)) => eprintln!(
                "[osu] lazer offsets: table from ${ENV_TABLE} unreadable: {error} ({})",
                path.display()
            ),
            None => eprintln!(
                "[osu] lazer offsets: ${ENV_TABLE} points at {} but it is not readable — falling through the ladder",
                path.display()
            ),
        }
    }

    let mut candidate_dirs: Vec<PathBuf> = Vec::new();
    if let Ok(offsets_dir) = std::env::var("MMA_OFFSETS_DIR") {
        candidate_dirs.push(PathBuf::from(&offsets_dir).join("lazer"));
        candidate_dirs.push(PathBuf::from(&offsets_dir));
    }
    if let Ok(appdata) = std::env::var("APPDATA") {
        candidate_dirs.push(
            PathBuf::from(appdata)
                .join("ManiaMapAnalyser")
                .join("offsets")
                .join("lazer"),
        );
    }
    candidate_dirs.push(table_dir.join("offsets").join("lazer"));
    candidate_dirs.push(table_dir.join("offset").join("lazer"));
    candidate_dirs.push(table_dir.join(TABLE_DIR));

    let clean_ver = crate::osu::offsets::lazer_table::normalize_version(&info.lazer_version);
    let expected_names = [
        table_file_name(
            &info.lazer_version,
            &info.runtime_version,
            &info.arch,
        ),
        format!("{clean_ver}__{}__{}.json", info.runtime_version, info.arch),
        format!("{clean_ver}.0__{}__{}.json", info.runtime_version, info.arch),
    ];

    // ② 文件命名约定的精确命中
    //
    // 同一文件名可能同时存在于 APPDATA 缓存和壳目录：APPDATA 优先是产品约定，
    // 但旧版 `gen.exe` 在选歌页生成过缺 `score`/`results` 的局部表。不能因为
    // APPDATA 里先找到局部表就丢掉壳目录里的完整表；同键候选按可选段完整度
    // 选最完整的一张（平手仍保持候选目录的原始优先级）。
    let mut exact_best: Option<(PathBuf, OffsetTable, u8)> = None;
    for dir in &candidate_dirs {
        for name in &expected_names {
            let expected = dir.join(name);
            if let Some(Ok(table)) = files.read(&expected).map(|bytes| OffsetTable::load(&bytes)) {
                if usable(&table).is_ok() {
                    let score = optional_section_score(&table);
                    if exact_best.as_ref().map_or(true, |(_, _, best)| score > *best) {
                        exact_best = Some((expected, table, score));
                    }
                } else {
                    eprintln!(
                        "[osu] lazer offsets: {} rejected: {}",
                        expected.display(),
                        usable(&table).err().unwrap_or_default()
                    );
                }
            }
        }
    }
    if let Some((path, table, _)) = exact_best {
        return Ok(LoadedTable {
            mismatch: table.mismatch(&info.lazer_version, &info.runtime_version, &info.arch),
            table,
            origin: TableOrigin::Exact,
            path,
        });
    }

    // ③ 同目录下任意同键表
    // ④ 同一批候选里挑最近的
    let mut tables: Vec<(PathBuf, OffsetTable)> = Vec::new();
    let mut seen_paths: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    for dir in &candidate_dirs {
        for path in files.list_json(dir) {
            if !seen_paths.insert(path.clone()) {
                continue;
            }
            let Some(Ok(table)) = files.read(&path).map(|bytes| OffsetTable::load(&bytes)) else {
                continue;
            };
            if usable(&table).is_ok() {
                tables.push((path, table));
            }
        }
    }

    // 内嵌默认表
    let embedded_default = crate::osu::offsets::default_lazer_table();
    if !tables.iter().any(|(_, t)| t.key() == embedded_default.key()) {
        tables.push((PathBuf::from("<embedded>"), embedded_default));
    }

    let target = info.target();
    let mut same_key_best: Option<&(PathBuf, OffsetTable)> = None;
    for candidate in &tables {
        let (_, table) = candidate;
        if table.mismatch(&info.lazer_version, &info.runtime_version, &info.arch).is_some() {
            continue;
        }
        let better = same_key_best
            .map(|(_, best)| optional_section_score(table) > optional_section_score(best))
            .unwrap_or(true);
        if better {
            same_key_best = Some(candidate);
        }
    }
    if let Some((path, table)) = same_key_best {
        return Ok(LoadedTable {
            table: table.clone(),
            origin: if path == &PathBuf::from("<embedded>") { TableOrigin::Exact } else { TableOrigin::SameKey },
            path: path.clone(),
            mismatch: None,
        });
    }
    let candidates: Vec<OffsetTable> = tables.iter().map(|(_, table)| table.clone()).collect();
    let distance_labels: Vec<String> = candidates.iter().map(|table| table.key()).collect();
    let policy = ValidationPolicy::new(prove);
    match OffsetTable::nearest_table(&candidates, &target, NEAREST_MAX_DISTANCE, &policy) {
        Ok(nearest) => {
            let key = nearest.key();
            let path = tables
                .iter()
                .find(|(_, table)| table.key() == key)
                .map(|(path, _)| path.clone())
                .unwrap_or_else(|| candidate_dirs.first().cloned().unwrap_or_default());
            let mismatch = nearest.mismatch(&info.lazer_version, &info.runtime_version, &info.arch);
            eprintln!(
                "[osu] lazer offsets: FALLBACK TABLE — target key {}/{}/{} not present; using nearest {} (distance<= {}) because the L1 structural proof (site chain + table field probe) passed. candidates=[{}]",
                info.lazer_version,
                info.runtime_version,
                info.arch,
                key,
                NEAREST_MAX_DISTANCE,
                distance_labels.join(", ")
            );
            Ok(LoadedTable {
                table: nearest.clone(),
                origin: TableOrigin::Nearest,
                path,
                mismatch,
            })
        }
        Err(error) => {
            if !distance_labels.is_empty() {
                eprintln!(
                    "[osu] lazer offsets: nearest fallback refused ({error}); candidates=[{}]",
                    distance_labels.join(", ")
                );
            }
            Err(missing())
        }
    }
}

pub fn usable(table: &OffsetTable) -> Result<(), String> {
    if table.types.is_empty() {
        return Err("types is empty".to_string());
    }
    Ok(())
}

/// 可选段完整度：用于同键多张表时优先完整表，避免 APPDATA 里的旧版局部
/// 生成结果（缺 `score`/`results`）遮住壳目录里的随包完整表。
fn optional_section_score(table: &OffsetTable) -> u8 {
    let mut score = 0;
    if table.results.is_some() {
        score += 2;
    }
    if table.score.is_some() {
        score += 2;
    }
    if table.mods.is_some() {
        score += 1;
    }
    score
}

pub fn shell_exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.to_path_buf()))
        .unwrap_or_default()
}

pub fn arch_for_bitness(pe_machine: u16) -> &'static str {
    match Client::from_bitness(pe_machine) {
        Some(Client::Lazer) => ARCH_X64,
        _ => ARCH_X86,
    }
}

#[cfg(windows)]
pub fn storage_ini_path() -> Option<PathBuf> {
    std::env::var("APPDATA")
        .ok()
        .map(|appdata| PathBuf::from(appdata).join("osu").join("storage.ini"))
}

#[cfg(not(windows))]
pub fn storage_ini_path() -> Option<PathBuf> {
    None
}

pub fn env_table_path() -> Option<PathBuf> {
    std::env::var(ENV_TABLE)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

#[cfg(test)]
mod optional_section_tests {
    use super::*;

    #[test]
    fn full_table_outranks_a_partial_one() {
        let full = crate::osu::offsets::default_lazer_table();
        let mut partial = full.clone();
        partial.results = None;
        partial.score = None;
        partial.mods = None;
        assert!(optional_section_score(&full) > optional_section_score(&partial));
    }
}
