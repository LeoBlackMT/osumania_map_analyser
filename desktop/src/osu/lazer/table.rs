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

    /// `storage.ini` 明确给出的存储根（原始值，不检查目录是否存在）。
    pub fn songs_folder(&self) -> Option<String> {
        self.storage_root
            .as_deref()
            .map(|root| format!("{root}\\{}", FILES_DIR))
    }

    /// 实际用于载荷 `folders.songs` 的目录：先信 `storage.ini`，
    /// 缺失/失效时回落到默认 Lazer 数据目录；两者都必须真实存在。
    pub fn effective_songs_folder(&self) -> Option<String> {
        resolve_songs_folder(self.storage_root.as_deref(), default_lazer_files_dirs())
    }

    /// 覆盖 `storage.ini` 的存储根（壳设置的 `lazerStorageRoot`）。
    /// 允许用户直接选 `...\files`，统一归一化到其父目录。
    pub fn with_storage_root_override(mut self, root: Option<&Path>) -> Self {
        if let Some(root) = root {
            self.storage_root = Some(
                normalize_storage_root(root)
                    .to_string_lossy()
                    .into_owned(),
            );
        }
        self
    }
}

/// 把“Lazer 数据目录”或“数据目录下的 `files` 目录”统一成数据根。
pub fn normalize_storage_root(path: &Path) -> PathBuf {
    let is_files = path
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.eq_ignore_ascii_case(FILES_DIR))
        .unwrap_or(false);
    if is_files {
        path.parent().unwrap_or(path).to_path_buf()
    } else {
        path.to_path_buf()
    }
}

/// `storage.ini` 的目录优先；它缺失或失效时再尝试默认 Lazer 数据目录。
///
/// 只返回实际存在的目录，避免把不存在的路径交给 24062 文件路由
/// （那会把“没有存储根”伪装成 500 读盘失败）。
fn resolve_songs_folder(
    storage_root: Option<&str>,
    default_dirs: impl IntoIterator<Item = PathBuf>,
) -> Option<String> {
    let configured = storage_root.map(|root| Path::new(root).join(FILES_DIR));
    configured
        .into_iter()
        .chain(default_dirs)
        .find(|dir| dir.is_dir())
        .map(|dir| dir.to_string_lossy().into_owned())
}

/// Lazer 默认数据目录下的 `files` 目录候选。
///
/// 默认安装不会生成 `storage.ini`，此时必须能回落到
/// `%APPDATA%\osu\files`；旧版/便携环境可能使用带感叹号的
/// `%APPDATA%\osu!\files`，一并作为候选（存在性由调用方检查）。
fn default_lazer_files_dirs() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        let Some(appdata) = std::env::var_os("APPDATA") else {
            return Vec::new();
        };
        let appdata = PathBuf::from(appdata);
        vec![
            appdata.join("osu").join(FILES_DIR),
            appdata.join("osu!").join(FILES_DIR),
        ]
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

/// 不依赖读取器快照的存储根探测（设置页 `resolved` 与显式覆盖共用）：
/// `storage.ini` 的 `FullPath` 优先，其次默认 `%APPDATA%\osu[!]\files` 的父目录。
pub fn detected_lazer_storage_root() -> Option<PathBuf> {
    let from_ini = storage_ini_path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| ini_value(&text, "FullPath"))
        .map(PathBuf::from)
        .filter(|root| root.join(FILES_DIR).is_dir());
    from_ini.or_else(|| {
        default_lazer_files_dirs()
            .into_iter()
            .find(|files| files.is_dir())
            .and_then(|files| files.parent().map(|root| root.to_path_buf()))
    })
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
    let appdata = std::env::var_os("APPDATA").map(PathBuf::from)?;
    let candidates = [
        appdata.join("osu").join("storage.ini"),
        appdata.join("osu!").join("storage.ini"),
    ];
    for candidate in &candidates {
        if candidate.is_file() {
            return Some(candidate.clone());
        }
    }
    candidates.into_iter().next()
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

#[cfg(test)]
mod storage_tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(name: &str) -> PathBuf {
        let unique = format!(
            "mma-lazer-storage-{name}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        std::env::temp_dir().join(unique)
    }

    #[test]
    fn missing_storage_ini_falls_back_to_default_lazer_files() {
        let root = temp_root("fallback");
        let default = root.join("default");
        let files = default.join(FILES_DIR);
        std::fs::create_dir_all(&files).unwrap();

        let got = resolve_songs_folder(
            None,
            vec![default.join("missing").join(FILES_DIR), files.clone()],
        );
        assert_eq!(got, Some(files.to_string_lossy().into_owned()));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn configured_storage_root_wins_when_files_exists() {
        let root = temp_root("configured");
        let configured = root.join("configured");
        let default = root.join("default");
        std::fs::create_dir_all(configured.join(FILES_DIR)).unwrap();
        std::fs::create_dir_all(default.join(FILES_DIR)).unwrap();

        let got = resolve_songs_folder(
            Some(configured.to_string_lossy().as_ref()),
            vec![default.join(FILES_DIR)],
        );
        assert_eq!(
            got,
            Some(configured.join(FILES_DIR).to_string_lossy().into_owned())
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn invalid_storage_ini_falls_back_to_default_lazer_files() {
        let root = temp_root("invalid-configured");
        let missing = root.join("missing");
        let default = root.join("default");
        let files = default.join(FILES_DIR);
        std::fs::create_dir_all(&files).unwrap();

        let got = resolve_songs_folder(
            Some(missing.to_string_lossy().as_ref()),
            vec![files.clone()],
        );
        assert_eq!(got, Some(files.to_string_lossy().into_owned()));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn storage_root_override_accepts_the_files_directory() {
        let root = temp_root("override-files");
        let data = root.join("osu");
        let files = data.join(FILES_DIR);
        std::fs::create_dir_all(&files).unwrap();

        let info = TargetInfo::default().with_storage_root_override(Some(&files));

        assert_eq!(
            info.songs_folder(),
            Some(files.to_string_lossy().into_owned())
        );
        assert_eq!(
            info.effective_songs_folder(),
            Some(files.to_string_lossy().into_owned())
        );

        let _ = std::fs::remove_dir_all(&root);
    }
}
