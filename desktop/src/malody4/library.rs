// Malody 4.3.7 谱面库索引：遍历 `<root>/beatmap/**`，流式 md5 + 有界前缀 meta 提取。
//
// 索引两类文件，准入判据各自取文件自己的头部：
// - `.mc`（Malody 原生）：`meta.mode` 在 Key 白名单里（`KEY_MODES`）**且**
//   `meta.mode_ext.column` 存在；
// - `.osu`（Malody 4.3.7 也直接播放 osu! 谱面）：`[General] Mode` = `3`（osu!mania）**且**
//   `[Difficulty] CircleSize` 给了键数。osu! 的其它模式（`Mode` 0/1/2）一律不收。
//
// 为什么要收 `.osu`（真机 md5 对照）：玩家选中库里的 `.osu` 谱面时，锚点给出的是**该 `.osu`
// 文件自己的 md5**，而"只索引 `.mc`"的旧判据（当时假设只有 `.mc` 才带 Key 模式信息）必然对
// 它报 `chart-not-indexed`——实测同目录的一对谱面里，osu!mania 7K 的
// `Hanatan - Uta Ni Katachi Wa Nai Keredo (Wonki) [Koe no katachi].osu`
// （md5=`604ed696987793aa4dbae63e0b79622f`）命中不了索引，而 `1716030285.mc`
// （md5=`623efe49c55d94bf0e3dc2e941eaed69`）能命中。`.osu` 恰好又是页面要的目标格式：
// `externalSource.js` 的 `looksLikeOsu()` 对 `.osu` 原文直通、不做任何转换，因此收它对
// 源头来说是零成本的一类谱面（真机 53 张全部是 `Mode: 3`）。
//
// 准入为何是 `0..=6` 而不是 `== 0`（真机 214 张 `.mc` 的分类证据）：column 式游戏模式是
// mode 0–6（0 Key / 1 Catch / 2 Pad / 3 Taiko / 4 Ring / 5 Slide / 6 Live），mode 7/8
// （Spectacle 及以后）没有 `column`。旧判据 `mode == 0` 把 17 张真 Key 谱挡在索引外
// （其中 13 张是 `mode=6` + `column=8`）。mode 区间只是**廉价预筛**：真正的守卫是"谱面自带
// `column` 字段"，没有 `column` 的一律不收（mode 7/8 因此天然被拒）。这不是语义重分类。
//
// 逐文件单趟：BufReader 以固定块喂 `Md5`（增量更新，绝不整份读进内存），同时保留
// 前 64 KiB 前缀供 meta 提取（手工括号配对，不整份 JSON 解析）。
//
// 变更探测：`LibraryFingerprint`（只读元数据）与 `rescan_change` 供周期重扫判断"树变了没有"，
// 与 `rebuild` 共用同一套遍历（`walk_chart_files`）——两者的文件集合因此逐字段一致。

use crate::malody4::model::hex16;
use md5::{Digest, Md5};
use std::collections::HashMap;
use std::fs::{File, Metadata};
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

/// 目录递归深度上限（`<root>/beatmap` 记 0 层）。
pub const MAX_DEPTH: usize = 8;
/// 喂给 `Md5` 的固定块大小。
const HASH_CHUNK: usize = 64 * 1024;
/// 保留的文件前缀上限（`.mc` 的 `meta` 对象与 `.osu` 的头部都落在这段文本里；真机 53 张
/// `.osu` 的 `[Difficulty]` 段最晚出现在第 732 字节，头部离这段窗口还有 90 倍余量）。
const PREFIX_LIMIT: usize = 64 * 1024;
/// 准入的 Key 模式白名单。
///
/// 只有这两种 mode 是 Key 模式的谱面，且转换器（`mcToOsuConverter.js`）只支持这两种：
/// - `0`：标准 Key 模式（实测 132 张，`mode_ext.column=4`，note 的 `column` 取值为 `0..=4`，与声明一致）
/// - `6`：另一种 Key 模式（实测 13 张，`mode_ext.column=8`，note 的 `column` 取值为 `0..=7`，与声明一致）
///
/// **不在白名单里的都不是键型谱面**（实测按 note 结构判定，与游戏内模式名逐一对应）：
/// `1` Catch / `2` Pad / `3` Taiko / `4` Pad(interval) / `5` Taiko(`style` 音符) /
/// `7` Live(`x` 坐标) / `8` 及以后无 `column`。它们即使带 `mode_ext.column` 也不能进来：
/// 转换器处理不了，放进来只会变成"分析失败"的噪声。
const KEY_MODES: [u64; 2] = [0, 6];

/// 索引统计。
///
/// `skipped_junk` 计的是被跳过的 **目录项**：`__MACOSX` 目录按 1 条计（不展开其内容），
/// 每个 `._*` 文件、每个 symlink / reparse point 各 1 条。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LibraryStats {
    pub indexed: usize,
    /// `.mc`：有 `meta` 对象但不是 Key 模式（`mode` 不在白名单，或缺 `mode_ext.column`）。
    pub skipped_non_key: usize,
    /// `.osu`：认得出是 osu 文件（有 `[General]` 段），但 `Mode` 不是 `3`（osu!mania）。
    /// Taiko / Catch / Standard 谱面放进来也只会变成"分析失败"的噪声，故与 `.mc` 的非键型模式同档。
    pub skipped_osu_not_mania: usize,
    /// `.osu`：`Mode` 是 `3`（osu!mania），但 `[Difficulty] CircleSize` 缺失或不是正整数
    /// ——键数未知，而键数是下游分析必需的，故拒收（与 `skipped_osu_not_mania` 分开计数，
    /// 否则"不是 mania"与"是 mania 但头部坏了"在日志里分不开）。
    pub skipped_osu_no_keys: usize,
    pub skipped_junk: usize,
    pub skipped_ext: usize,
    pub skipped_unparsed: usize,
    pub duplicate_md5: usize,
}

/// 索引里的一行。`slot` 来自锚点身份键（诊断用），见 `ChartLibrary::set_slot`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryEntry {
    pub path: PathBuf,
    pub slot: u32,
    pub md5: String,
}

/// 谱面元信息：`.mc` 取自其 `meta` 对象，`.osu` 取自文件头部（`[Metadata]` 三键 + `[Difficulty] CircleSize`）。
///
/// 只有这四个字段：`.mc` 的 `mode` / `mode_ext` 不参与帧，`.osu` 也没有等价字段。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChartMeta {
    pub title: String,
    pub artist: String,
    pub version: String,
    /// 键数（`.mc`：`mode_ext.column`；`.osu`：`[Difficulty] CircleSize`）。
    pub keys: u64,
}

/// 索引树指纹：**只读元数据**的变更探测器（只用 `std`，不打开文件、不哈希任何字节）。
///
/// 为什么存在：周期重扫（60s）原先无条件重建整库——真机 ~22 MB 的谱面每轮都要整份流式喂 MD5，
/// 而实测一个会话里树的内容根本没变（22 代索引里绝大多数是白哈希）。指纹把"该不该重建"的
/// 判断成本从"哈希整库"降到"读一遍目录元数据"（真机 ~600 个目录项）。
///
/// 结构是遍历的副产品：`rebuild` 本来就要 `symlink_metadata`，顺手记下谱面文件数、总字节数与
/// 最新 mtime，因此记录指纹本身不增加系统调用；`scan` 用**同一套跳过规则**
/// （`walk_chart_files`）走一遍同一棵树，两者的文件集合因此逐字段一致——这是"指纹一致 ⇒
/// 索引一致"成立的前提。
///
/// **已知盲区（有意接受）**：同大小、且 mtime 落进文件系统时间戳粒度的**原地改写**看不见
/// （三个分量都没变化）。兜底不在这里（在指纹上比字节等于每 60s 读一遍 22 MB，正是要避免的
/// 成本），而在 **miss 路径**：玩家真的选中一张查不到的谱面时，壳绕开指纹强制执行有限次重建
/// （`MISS_REBUILD_ATTEMPTS`），因此"用户看得见的那次失败"仍会被真正重哈希覆盖。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LibraryFingerprint {
    /// 遍历见到的谱面文件数（`.mc` / `.osu`；跳过规则与 `rebuild` 完全一致）。
    pub charts: u64,
    /// 这些文件的字节数总和。
    pub bytes: u64,
    /// 其中最新的 mtime（一个谱面文件都没有时为 `None`）。
    pub newest: Option<SystemTime>,
}

impl LibraryFingerprint {
    /// 只读元数据地走一遍同一棵树，得到与 `rebuild` 同口径的指纹。
    pub fn scan(root: &Path) -> Self {
        let mut fingerprint = LibraryFingerprint::default();
        walk_chart_files(root, |_, meta, _| fingerprint.record(meta));
        fingerprint
    }

    /// 记一个谱面文件（`rebuild` 与 `scan` 共用同一口径）。
    fn record(&mut self, meta: &Metadata) {
        self.charts += 1;
        self.bytes = self.bytes.saturating_add(meta.len());
        if let Ok(mtime) = meta.modified() {
            if self.newest.is_none_or(|newest| mtime > newest) {
                self.newest = Some(mtime);
            }
        }
    }
}

/// 周期重扫的判定：走一遍**元数据**指纹，与上次构建的指纹比。
///
/// `None` = 三个分量逐字段一致（文件增删、总字节数、最新 mtime 都没动）⇒ 不重建；
/// `Some(new)` = 树有可观测变化 ⇒ 该重建。绝不在这里哈希任何文件。
pub fn rescan_change(root: &Path, previous: &LibraryFingerprint) -> Option<LibraryFingerprint> {
    let current = LibraryFingerprint::scan(root);
    (current != *previous).then_some(current)
}

/// 谱面库索引。帧组装直接从索引取元信息，不再为每帧重读重解析谱面。
#[derive(Debug, Clone)]
pub struct ChartLibrary {
    pub root: PathBuf,
    pub by_md5: HashMap<String, LibraryEntry>,
    /// 按谱面路径存放元信息（只有入索引的文件才有）。
    pub meta: HashMap<PathBuf, ChartMeta>,
    pub built_at: Instant,
    /// 构建这次索引时所见到的树指纹（周期重扫拿它比"树变没变"）。
    pub fingerprint: LibraryFingerprint,
    pub stats: LibraryStats,
}

impl ChartLibrary {
    pub fn lookup(&self, md5: &str) -> Option<&LibraryEntry> {
        self.by_md5.get(md5)
    }

    pub fn len(&self) -> usize {
        self.by_md5.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_md5.is_empty()
    }

    /// 回填锚点身份键里的槽位（`rebuild` 时不可知，故一律先置 0）。
    pub fn set_slot(&mut self, md5: &str, slot: u32) {
        if let Some(entry) = self.by_md5.get_mut(md5) {
            entry.slot = slot;
        }
    }
}

/// meta 扫描结果。
enum MetaScan {
    /// column 式模式（`mode ∈ 0..=6` 且 `mode_ext.column` 存在）。
    Key(ChartMeta),
    /// 有 meta 对象但不是 column 式模式的谱面（含所有没有 `column` 的谱面）。
    NonKey,
    /// 找不到 meta 对象或无法解析。
    Unparsed,
}

/// `<root>/beatmap/**` 的迭代式 DFS（不用递归）：对每个**谱面文件**调用 `visit(路径, 元数据, 类型)`。
///
/// 跳过规则在这里只有一份——symlink / reparse point、`__MACOSX` 目录、`._*` 文件、超过
/// `MAX_DEPTH` 的目录、非谱面扩展名——`rebuild`（真哈希）与 `LibraryFingerprint::scan`
/// （只读元数据）都从这里走，因此两者看到的文件集合逐字段一致。
/// 返回被跳过的目录项计数 `(junk, ext)`（`rebuild` 的 `LibraryStats` 用；指纹扫描直接丢弃）。
fn walk_chart_files<F>(root: &Path, mut visit: F) -> (usize, usize)
where
    F: FnMut(&Path, &Metadata, ChartKind),
{
    let mut junk = 0usize;
    let mut ext = 0usize;
    let mut stack: Vec<(PathBuf, usize)> = vec![(root.join("beatmap"), 0)];
    while let Some((dir, depth)) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            // 目录不存在或不可读 → 当作空目录
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            // symlink_metadata 不跟随链接，故 is_symlink 判定可靠
            let meta = match std::fs::symlink_metadata(&path) {
                Ok(meta) => meta,
                Err(_) => continue,
            };
            if meta.file_type().is_symlink() {
                junk += 1;
                continue;
            }
            if meta.is_dir() {
                if name == "__MACOSX" {
                    junk += 1;
                    continue;
                }
                if depth + 1 <= MAX_DEPTH {
                    stack.push((path, depth + 1));
                }
                continue;
            }
            if !meta.is_file() {
                continue;
            }
            if name.starts_with("._") {
                junk += 1;
                continue;
            }
            match chart_kind(&path) {
                Some(kind) => visit(&path, &meta, kind),
                None => ext += 1,
            }
        }
    }
    (junk, ext)
}

/// 重建索引：迭代式 DFS 遍历 `<root>/beatmap/**`，顺手记下树指纹。
pub fn rebuild(root: &Path) -> ChartLibrary {
    let mut lib = ChartLibrary {
        root: root.to_path_buf(),
        by_md5: HashMap::new(),
        meta: HashMap::new(),
        built_at: Instant::now(),
        fingerprint: LibraryFingerprint::default(),
        stats: LibraryStats::default(),
    };
    // (相对路径, 绝对路径, md5, meta)：先收集再按相对路径排序裁决重复 md5
    let mut candidates: Vec<(String, PathBuf, String, ChartMeta)> = Vec::new();
    let mut stats = LibraryStats::default();
    let mut fingerprint = LibraryFingerprint::default();
    let (junk, ext) = walk_chart_files(root, |path, meta, kind| {
        // 指纹是这次遍历的副产品（元数据本来就要读），不额外增加系统调用
        fingerprint.record(meta);
        let (md5, prefix) = match hash_prefix_and_digest(path) {
            Ok(value) => value,
            Err(_) => {
                stats.skipped_unparsed += 1;
                return;
            }
        };
        let Some(chart_meta) = scan_chart(kind, &prefix, &mut stats) else {
            return;
        };
        candidates.push((relative_name(root, path), path.to_path_buf(), md5, chart_meta));
    });
    stats.skipped_junk = junk;
    stats.skipped_ext = ext;

    // 重复 md5 的确定性裁决：相对路径字典序最小者胜（升序遍历 → 先到者保留）
    candidates.sort_by(|a, b| a.0.cmp(&b.0));
    let mut duplicates = 0usize;
    for (_, path, md5, chart_meta) in candidates {
        if lib.by_md5.contains_key(&md5) {
            duplicates += 1;
            continue;
        }
        lib.by_md5.insert(
            md5.clone(),
            LibraryEntry {
                path: path.clone(),
                slot: 0,
                md5,
            },
        );
        lib.meta.insert(path, chart_meta);
    }
    lib.stats = stats;
    lib.stats.duplicate_md5 = duplicates;
    lib.stats.indexed = lib.by_md5.len();
    lib.fingerprint = fingerprint;
    if duplicates > 0 {
        eprintln!(
            "[malody4] library: {duplicates} duplicate md5 chart(s) skipped (kept lexicographically smallest path)"
        );
    }
    lib
}

/// 索引接受的谱面类型：`.mc`（Malody 原生）与 `.osu`（Malody 4.3.7 也能播放的 osu! 谱面）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChartKind {
    Mc,
    Osu,
}

/// 扩展名 → 谱面类型（大小写不敏感）；其它扩展名一律不收（真机里的 `.tja` / 封面 / 音频）。
fn chart_kind(path: &Path) -> Option<ChartKind> {
    let ext = path.extension().and_then(|ext| ext.to_str())?;
    if ext.eq_ignore_ascii_case("mc") {
        Some(ChartKind::Mc)
    } else if ext.eq_ignore_ascii_case("osu") {
        Some(ChartKind::Osu)
    } else {
        None
    }
}

/// 前缀 → 元信息；被拒时顺手把对应的统计计数加一（`None` = 不入索引）。
fn scan_chart(kind: ChartKind, prefix: &[u8], stats: &mut LibraryStats) -> Option<ChartMeta> {
    match kind {
        ChartKind::Mc => match scan_meta(prefix) {
            MetaScan::Key(chart_meta) => Some(chart_meta),
            MetaScan::NonKey => {
                stats.skipped_non_key += 1;
                None
            }
            MetaScan::Unparsed => {
                stats.skipped_unparsed += 1;
                None
            }
        },
        ChartKind::Osu => match scan_osu(prefix) {
            OsuScan::Mania(chart_meta) => Some(chart_meta),
            OsuScan::NotMania => {
                stats.skipped_osu_not_mania += 1;
                None
            }
            OsuScan::NoKeys => {
                stats.skipped_osu_no_keys += 1;
                None
            }
            OsuScan::Unparsed => {
                stats.skipped_unparsed += 1;
                None
            }
        },
    }
}

fn relative_name(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string()
}

/// 单趟读文件：固定块喂 `Md5` 增量的同时保留前 `PREFIX_LIMIT` 字节。
fn hash_prefix_and_digest(path: &Path) -> std::io::Result<(String, Vec<u8>)> {
    let file = File::open(path)?;
    let mut reader = BufReader::with_capacity(HASH_CHUNK, file);
    let mut hasher = Md5::new();
    let mut prefix: Vec<u8> = Vec::with_capacity(PREFIX_LIMIT);
    let mut buf = vec![0u8; HASH_CHUNK];
    loop {
        let read = reader.read(&mut buf)?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
        if prefix.len() < PREFIX_LIMIT {
            let take = read.min(PREFIX_LIMIT - prefix.len());
            prefix.extend_from_slice(&buf[..take]);
        }
    }
    Ok((hex16(hasher.finalize().as_slice()), prefix))
}

/// 从保留的前缀文本里提取 `meta` 对象并判定准入。
fn scan_meta(prefix: &[u8]) -> MetaScan {
    let text = String::from_utf8_lossy(prefix);
    let raw = match find_object(&text, "meta") {
        Some(raw) => raw,
        None => return MetaScan::Unparsed,
    };
    let value: serde_json::Value = match serde_json::from_str(raw) {
        Ok(value) => value,
        // 前缀被截断导致对象不完整时同样落这里
        Err(_) => return MetaScan::Unparsed,
    };
    let meta = match value.as_object() {
        Some(meta) => meta,
        None => return MetaScan::Unparsed,
    };
    let mode = meta.get("mode").and_then(|v| v.as_u64());
    let column = meta
        .get("mode_ext")
        .and_then(|ext| ext.get("column"))
        .and_then(|v| v.as_u64());
    let song = meta.get("song");
    match (mode, column) {
        // 准入 = `mode` 在 Key 白名单里（0 或 6）**且** `mode_ext.column` 存在；缺一不可。
        // 两个条件都是必需的：白名单挡掉 Taiko/Catch/Live/Pad 等非键型模式，
        // `column` 则是"这张谱是 column 式谱面"的实证（mode 7/8 本来就没有它）。
        (Some(mode), Some(keys)) if KEY_MODES.contains(&mode) => MetaScan::Key(ChartMeta {
            title: song
                .and_then(|s| s.get("title"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            artist: song
                .and_then(|s| s.get("artist"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            version: meta
                .get("version")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            keys,
        }),
        _ => MetaScan::NonKey,
    }
}

/// `.osu` 头部扫描结果。
enum OsuScan {
    /// `[General] Mode` = `3`（osu!mania）且 `[Difficulty] CircleSize` 给了键数。
    Mania(ChartMeta),
    /// 认得出是 osu 文件（有 `[General]` 段），但 `Mode` 不是 `3`（含整个 `Mode` 键缺失）。
    NotMania,
    /// 是 osu!mania，但 `CircleSize` 缺失或不是正整数：键数未知。
    NoKeys,
    /// 连 `[General]` 段都没有：不是 osu 谱面文件（头部超出前缀窗口时同样落这里）。
    Unparsed,
}

/// 从保留的前缀文本里读 `.osu` 头部（纯 INI 文本，无 JSON 解析）。
///
/// 只认 `[General] Mode`、`[Metadata] Title/Artist/Version`、`[Difficulty] CircleSize` 五个键，
/// 且**只在各自的段里**认（`[HitObjects]` 的 note 行里带冒号，别的段里的同名键都不算）。
/// **不解析 note 行**：谱面原文照旧整份交给页面（`looksLikeOsu()` 直通），这里只为帧 `meta` 取字段。
fn scan_osu(prefix: &[u8]) -> OsuScan {
    let text = String::from_utf8_lossy(prefix);
    let mut section = "";
    let mut saw_general = false;
    let mut mode: Option<&str> = None;
    let mut title = "";
    let mut artist = "";
    let mut version = "";
    let mut circle_size: Option<&str> = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')) {
            section = name.trim();
            saw_general |= section == "General";
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        match (section, key.trim()) {
            ("General", "Mode") => mode = Some(value.trim()),
            ("Metadata", "Title") => title = value.trim(),
            ("Metadata", "Artist") => artist = value.trim(),
            ("Metadata", "Version") => version = value.trim(),
            ("Difficulty", "CircleSize") => circle_size = Some(value.trim()),
            _ => {}
        }
    }
    if !saw_general {
        return OsuScan::Unparsed;
    }
    if parse_positive_int(mode.unwrap_or_default()) != Some(3) {
        return OsuScan::NotMania;
    }
    let Some(keys) = circle_size.and_then(parse_positive_int) else {
        return OsuScan::NoKeys;
    };
    OsuScan::Mania(ChartMeta {
        title: title.to_string(),
        artist: artist.to_string(),
        version: version.to_string(),
        keys,
    })
}

/// `.osu` 的 `Mode` / `CircleSize` 取整数：接受 `3`，也接受编辑器可能写出的整数值浮点（`3.0`）；
/// `0`、负数、非数字一律 `None`（`Mode` 因此落 `NotMania`，`CircleSize` 落 `NoKeys`）。
fn parse_positive_int(value: &str) -> Option<u64> {
    if let Ok(int) = value.parse::<u64>() {
        return (int > 0).then_some(int);
    }
    let float: f64 = value.parse().ok()?;
    (float.fract() == 0.0 && (1.0..=u64::MAX as f64).contains(&float)).then_some(float as u64)
}

/// 定位 `"<key>"` 之后紧跟的 JSON 对象字面量（含花括号），手工配对。
fn find_object<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("\"{key}\"");
    let bytes = text.as_bytes();
    let mut search_from = 0usize;
    while let Some(offset) = text[search_from..].find(&needle) {
        let after_key = search_from + offset + needle.len();
        search_from = after_key;
        let mut cursor = after_key;
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor >= bytes.len() || bytes[cursor] != b':' {
            continue;
        }
        cursor += 1;
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor >= bytes.len() || bytes[cursor] != b'{' {
            continue;
        }
        if let Some(end) = match_braces(text, cursor) {
            return Some(&text[cursor..end]);
        }
    }
    None
}

/// 从 `start`（必须是 `{`）起做括号配对，跳过字符串字面量与转义；
/// 返回对象结束后的字节下标，未闭合返回 `None`。
fn match_braces(text: &str, start: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    let mut cursor = start;
    while cursor < bytes.len() {
        let byte = bytes[cursor];
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
        } else if byte == b'"' {
            in_string = true;
        } else if byte == b'{' {
            depth += 1;
        } else if byte == b'}' {
            depth -= 1;
            if depth == 0 {
                return Some(cursor + 1);
            }
        }
        cursor += 1;
    }
    None
}

#[cfg(test)]
#[path = "../../tests-local/malody4_library.rs"]
mod tests;
