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
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    fn mc_text(mode: u64, column: Option<u64>, title: &str, version: &str) -> String {
        let mode_ext = match column {
            Some(keys) => format!("{{\"column\":{keys}}}"),
            None => "{}".to_string(),
        };
        format!(
            "{{\"meta\":{{\"mode\":{mode},\"version\":\"{version}\",\"song\":{{\"title\":\"{title}\",\"artist\":\"Artist A\"}},\"mode_ext\":{mode_ext}}},\"note\":[]}}"
        )
    }

    /// 合成 `.osu`：真机 53 张的头部形态（`[General] Mode` / `[Metadata]` / `[Difficulty] CircleSize`）
    /// + 一行 note。`mode = None` = 整个 `Mode` 键缺失；`circle_size = None` = 缺 `CircleSize`。
    fn osu_text(
        mode: Option<u64>,
        circle_size: Option<&str>,
        title: &str,
        version: &str,
    ) -> String {
        let mut text = String::from("osu file format v14\n\n[General]\nAudioFilename: audio.mp3\n");
        if let Some(mode) = mode {
            text.push_str(&format!("Mode: {mode}\n"));
        }
        text.push_str(&format!(
            "\n[Metadata]\nTitle:{title}\nArtist:Artist Osu\nVersion:{version}\n\n[Difficulty]\nHPDrainRate:8.5\n"
        ));
        if let Some(keys) = circle_size {
            text.push_str(&format!("CircleSize:{keys}\n"));
        }
        text.push_str("\n[HitObjects]\n64,192,1000,1,0,0:0:0:0:\n");
        text
    }

    fn md5_of_text(text: &str) -> String {
        let mut hasher = Md5::new();
        hasher.update(text.as_bytes());
        hex16(hasher.finalize().as_slice())
    }

    fn tmp_root(tag: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "mma-malody4-lib-{}-{}-{}",
            tag,
            std::process::id(),
            stamp
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_file(path: &Path, text: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn rebuild_indexes_key_mc_and_counts_every_skip() {
        let root = tmp_root("tree");
        let base = root.join("beatmap");
        let album_a = base.join("Album A").join("0");
        let album_b = base.join("Album B").join("1");
        let key_text = mc_text(0, Some(4), "Song A", "Hard");
        // junk：__MACOSX 目录整棵跳过（记 1 条）+ 一个 ._ 前缀文件（记 1 条）
        write_file(&base.join("__MACOSX").join("._x.mc"), &mc_text(0, Some(4), "Junk", "Hard"));
        write_file(&album_a.join("._junk.mc"), &key_text);
        // 唯一的 Key 模式谱面
        write_file(&album_a.join("key.mc"), &key_text);
        // 非 column 式模式（mode = 7，Spectacle 及以后：没有 column）
        write_file(&album_a.join("notkey.mc"), &mc_text(7, None, "NonKey", "Hard"));
        // 没有 meta 对象的坏文件
        write_file(&album_a.join("broken.mc"), "{\"note\":[]}");
        // 不索引的扩展名（真机里的 .tja）
        write_file(&album_a.join("chart.tja"), "TITLE:x\n");
        // 认不出 osu 头部的 .osu（只有一行版本号，没有 [General] 段）→ 与坏 .mc 同档计 skipped_unparsed
        write_file(&album_a.join("chart.osu"), "osu file format v14\n");
        // 与 key.mc 同字节：相对路径更大，故落败
        write_file(&album_b.join("copy.mc"), &key_text);

        let mut lib = rebuild(&root);

        assert_eq!(
            lib.stats,
            LibraryStats {
                indexed: 1,
                skipped_non_key: 1,
                skipped_osu_not_mania: 0,
                skipped_osu_no_keys: 0,
                skipped_junk: 2,
                skipped_ext: 1,
                skipped_unparsed: 2,
                duplicate_md5: 1,
            }
        );
        assert_eq!(lib.len(), 1);
        assert!(!lib.is_empty());

        let key_path = album_a.join("key.mc");
        let md5 = md5_of_text(&key_text);
        let entry = lib.lookup(&md5).expect("Key 模式 .mc 必须入索引");
        assert_eq!(entry.path, key_path);
        assert_eq!(entry.slot, 0, "rebuild 时 slot 一律为 0，由调用方回填");
        assert_eq!(entry.md5, md5);
        assert!(lib.lookup(&md5_of_text("nothing")).is_none());

        assert_eq!(lib.meta.len(), 1);
        assert_eq!(
            lib.meta.get(&key_path),
            Some(&ChartMeta {
                title: "Song A".to_string(),
                artist: "Artist A".to_string(),
                version: "Hard".to_string(),
                keys: 4,
            })
        );
        assert!(!lib.meta.contains_key(&album_b.join("copy.mc")), "重复 md5 的落败文件不入 meta 表");

        lib.set_slot(&md5, 3);
        assert_eq!(lib.lookup(&md5).unwrap().slot, 3);
        lib.set_slot("missing", 9);

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 准入是"`mode` ∈ {0, 6} **且** `column` 存在"：
    /// - 真机 13 张 `mode=6` + `column=8` 必须入索引（它们的 note `column` 取值为 `0..=7`）；
    /// - **非键型模式即使带 `column` 也必须拒**：真机实测 `mode=5`(Taiko，note 带 `style`)
    ///   带 `column=4`、`mode=4`(Pad) 带 `column=1`，转换器处理不了，放进来只会变成"分析失败"；
    /// - `mode=7/8` 与所有缺 `column` 的谱面一律拒，`skipped_non_key` 的语义不变。
    #[test]
    fn key_modes_are_indexed_while_non_key_modes_and_missing_column_are_rejected() {
        let root = tmp_root("mode6");
        let base = root.join("beatmap").join("Album");
        // 真机实测：用户的 Key 谱里有 13 张是 `mode=6` + `column=8`，旧判据把它们全挡在索引外
        let eight_key = mc_text(6, Some(8), "Key 8K", "MX");
        write_file(&base.join("mode6.mc"), &eight_key);
        write_file(&base.join("mode0.mc"), &mc_text(0, Some(4), "Key 4K", "MX"));
        // 非键型模式：即便带 `column` 也必须拒（真机里 mode=5 就带 column=4）
        write_file(&base.join("mode5_col.mc"), &mc_text(5, Some(4), "TaikoWithColumn", "MX"));
        write_file(&base.join("mode1_col.mc"), &mc_text(1, Some(4), "Catch", "MX"));
        write_file(&base.join("mode4_col.mc"), &mc_text(4, Some(1), "PadColumn", "MX"));
        // mode 7/8：没有 `column`；即便带了也不在白名单里
        write_file(&base.join("mode7.mc"), &mc_text(7, None, "Live", "MX"));
        write_file(&base.join("mode8.mc"), &mc_text(8, None, "Spectacle", "MX"));
        write_file(&base.join("mode7_col.mc"), &mc_text(7, Some(4), "LiveWithColumn", "MX"));
        // Key 模式但缺 `column` → 拒
        write_file(&base.join("mode6_no_col.mc"), &mc_text(6, None, "NoColumn", "MX"));

        let lib = rebuild(&root);

        assert_eq!(lib.stats.indexed, 2, "只有 mode=0 / mode=6 且带 column 的两张入索引");
        assert_eq!(
            lib.stats.skipped_non_key, 7,
            "非白名单模式与缺 column 一律计 skipped_non_key（语义不变）"
        );
        assert_eq!(lib.stats.skipped_unparsed, 0);

        let eight = lib
            .lookup(&md5_of_text(&eight_key))
            .expect("mode=6 + column=8 必须入索引");
        assert_eq!(eight.path, base.join("mode6.mc"));
        assert_eq!(
            lib.meta.get(&eight.path),
            Some(&ChartMeta {
                title: "Key 8K".to_string(),
                artist: "Artist A".to_string(),
                version: "MX".to_string(),
                keys: 8,
            })
        );
        assert!(lib.lookup(&md5_of_text(&mc_text(0, Some(4), "Key 4K", "MX"))).is_some());
        assert!(lib.lookup(&md5_of_text(&mc_text(5, Some(4), "TaikoWithColumn", "MX"))).is_none());
        assert!(lib.lookup(&md5_of_text(&mc_text(1, Some(4), "Catch", "MX"))).is_none());
        assert!(lib.lookup(&md5_of_text(&mc_text(4, Some(1), "PadColumn", "MX"))).is_none());
        assert!(lib.lookup(&md5_of_text(&mc_text(7, None, "Live", "MX"))).is_none());
        assert!(lib.lookup(&md5_of_text(&mc_text(8, None, "Spectacle", "MX"))).is_none());
        assert!(lib.lookup(&md5_of_text(&mc_text(7, Some(4), "LiveWithColumn", "MX"))).is_none());
        assert!(lib.lookup(&md5_of_text(&mc_text(6, None, "NoColumn", "MX"))).is_none());

        let _ = std::fs::remove_dir_all(&root);
    }

    /// `.osu` 的准入是"`[General] Mode` = `3`（osu!mania）且 `[Difficulty] CircleSize` 给了键数"：
    /// 元信息全部取自文件自己的头部（`[Metadata]` 三键），键数取自 `CircleSize` 而不是 note 列数。
    #[test]
    fn osu_mode_3_is_indexed_with_its_header_metadata_and_key_count() {
        let root = tmp_root("osu-mania");
        // 真机路径形态：beatmap/<曲目(13)>/<艺术家 - 标题 (作者) [难度]>.osu
        let path = root
            .join("beatmap")
            .join("Uta Ni Katachi Wa Nai Keredo(13)")
            .join("Hanatan - Uta Ni Katachi Wa Nai Keredo (Wonki) [Koe no katachi].osu");
        let text = osu_text(
            Some(3),
            Some("7"),
            "Uta Ni Katachi Wa Nai Keredo",
            "Koe no katachi",
        );
        write_file(&path, &text);

        let lib = rebuild(&root);

        assert_eq!(lib.stats.indexed, 1);
        assert_eq!(lib.stats.skipped_osu_not_mania, 0);
        assert_eq!(lib.stats.skipped_osu_no_keys, 0);
        assert_eq!(lib.stats.skipped_unparsed, 0);

        let md5 = md5_of_text(&text);
        let entry = lib.lookup(&md5).expect("Mode: 3 的 .osu 必须入索引");
        assert_eq!(entry.path, path);
        assert_eq!(entry.slot, 0);
        assert_eq!(
            lib.meta.get(&path),
            Some(&ChartMeta {
                title: "Uta Ni Katachi Wa Nai Keredo".to_string(),
                artist: "Artist Osu".to_string(),
                version: "Koe no katachi".to_string(),
                keys: 7,
            })
        );
        assert!(lib.lookup(&md5_of_text("nothing")).is_none());

        let _ = std::fs::remove_dir_all(&root);
    }

    /// `Mode` 不是 `3`（osu!standard / Taiko / Catch / 未来模式）与整个 `Mode` 键缺失一律拒，
    /// 逐个计 `skipped_osu_not_mania`：放进来只会变成"分析失败"的噪声。
    #[test]
    fn osu_non_mania_modes_are_rejected_and_counted() {
        let root = tmp_root("osu-nonmania");
        let base = root.join("beatmap").join("Album");
        let cases: [(&str, Option<u64>); 5] = [
            ("standard.osu", Some(0)),
            ("taiko.osu", Some(1)),
            ("catch.osu", Some(2)),
            ("future.osu", Some(4)),
            ("no_mode_key.osu", None),
        ];
        for (name, mode) in cases {
            write_file(&base.join(name), &osu_text(mode, Some("4"), name, "MX"));
        }

        let lib = rebuild(&root);

        assert_eq!(lib.stats.indexed, 0);
        assert_eq!(lib.stats.skipped_osu_not_mania, 5);
        assert_eq!(lib.stats.skipped_osu_no_keys, 0);
        assert_eq!(lib.stats.skipped_unparsed, 0);
        for (name, mode) in cases {
            let md5 = md5_of_text(&osu_text(mode, Some("4"), name, "MX"));
            assert!(lib.lookup(&md5).is_none(), "{name} 不该入索引");
        }
        assert!(lib.is_empty());

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 是 osu!mania 但键数拿不到：`CircleSize` 缺失 / 空值 / 非数字 / `0` 一律拒，
    /// 计 `skipped_osu_no_keys`（与"不是 mania"分开）。
    #[test]
    fn osu_mania_without_a_parsable_circle_size_is_rejected_and_counted() {
        let root = tmp_root("osu-nokeys");
        let base = root.join("beatmap").join("Album");
        for (name, circle_size) in [
            ("missing.osu", None),
            ("empty.osu", Some("")),
            ("text.osu", Some("hard")),
            ("zero.osu", Some("0")),
        ] {
            write_file(&base.join(name), &osu_text(Some(3), circle_size, name, "MX"));
        }

        let lib = rebuild(&root);

        assert_eq!(lib.stats.indexed, 0);
        assert_eq!(lib.stats.skipped_osu_no_keys, 4);
        assert_eq!(lib.stats.skipped_osu_not_mania, 0);
        assert_eq!(lib.stats.skipped_unparsed, 0);

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 整数值的浮点写法（编辑器可能写出 `CircleSize:7.0`）按 7 收；非整数值仍拒。
    #[test]
    fn osu_integral_float_key_counts_are_accepted_while_fractional_ones_are_not() {
        let root = tmp_root("osu-float");
        let base = root.join("beatmap").join("Album");
        write_file(&base.join("integral.osu"), &osu_text(Some(3), Some("7.0"), "Integral", "MX"));
        write_file(&base.join("fractional.osu"), &osu_text(Some(3), Some("7.5"), "Fractional", "MX"));

        let lib = rebuild(&root);

        assert_eq!(lib.stats.indexed, 1);
        assert_eq!(lib.stats.skipped_osu_no_keys, 1);
        let path = base.join("integral.osu");
        assert_eq!(lib.meta.get(&path).unwrap().keys, 7);
        assert!(lib.lookup(&md5_of_text(&osu_text(Some(3), Some("7.5"), "Fractional", "MX"))).is_none());

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 只认三个段里的同名键：别的段（`[Events]` / `[HitObjects]`）里的 `Mode` / `CircleSize` /
    /// `Title` 都不参与判定——note 行本身带冒号，段不匹配是唯一的防线。
    #[test]
    fn osu_header_scan_reads_keys_only_from_their_own_sections() {
        let root = tmp_root("osu-sections");
        let base = root.join("beatmap").join("Album");
        let text = concat!(
            "osu file format v14\n\n[General]\nAudioFilename: audio.mp3\nMode: 3\n",
            "\n[Metadata]\nTitle:Real Title\nArtist:Real Artist\nVersion:MX\n",
            "\n[Difficulty]\nHPDrainRate:8.5\nCircleSize:4\n",
            "\n[Events]\nMode: 0\nTitle:Not This\nCircleSize:9\n",
            "\n[HitObjects]\n64,192,1000,1,0,0:0:0:0:\nMode: 0\nCircleSize:9\n"
        );
        let path = base.join("sections.osu");
        write_file(&path, text);

        let lib = rebuild(&root);

        assert_eq!(lib.stats.indexed, 1);
        assert_eq!(
            lib.meta.get(&path),
            Some(&ChartMeta {
                title: "Real Title".to_string(),
                artist: "Real Artist".to_string(),
                version: "MX".to_string(),
                keys: 4,
            })
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 同一棵树里的 `.mc` 与 `.osu` 各按自己的判据入索引：md5 不同、`lookup` 各自命中，
    /// 键数分别来自 `mode_ext.column` 与 `CircleSize`。
    #[test]
    fn mc_and_osu_in_the_same_tree_are_both_indexed_and_looked_up() {
        let root = tmp_root("mixed");
        let base = root.join("beatmap").join("Album").join("0");
        let mc_body = mc_text(0, Some(4), "Mc Song", "MX");
        let osu_body = osu_text(Some(3), Some("7"), "Osu Song", "MX");
        let mc_path = base.join("1716030285.mc");
        let osu_path = base.join("1716030286.osu");
        write_file(&mc_path, &mc_body);
        write_file(&osu_path, &osu_body);

        let lib = rebuild(&root);

        assert_eq!(lib.stats.indexed, 2);
        assert_eq!(lib.len(), 2);
        let mc_md5 = md5_of_text(&mc_body);
        let osu_md5 = md5_of_text(&osu_body);
        assert_ne!(mc_md5, osu_md5);
        assert_eq!(lib.lookup(&mc_md5).unwrap().path, mc_path);
        assert_eq!(lib.lookup(&osu_md5).unwrap().path, osu_path);
        assert_eq!(lib.meta.get(&mc_path).unwrap().keys, 4);
        assert_eq!(lib.meta.get(&mc_path).unwrap().title, "Mc Song");
        assert_eq!(lib.meta.get(&osu_path).unwrap().keys, 7);
        assert_eq!(lib.meta.get(&osu_path).unwrap().title, "Osu Song");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 大 `.osu` 与 `.mc` 同样走单趟流式哈希 + 有界前缀：md5 覆盖**整份文件**（不止前缀），
    /// 头部（含 `[Difficulty]`）整份落在前缀窗口里（真机 53 张最晚 732 B）。
    #[test]
    fn large_osu_is_streamed_whole_and_its_header_fits_the_bounded_prefix() {
        let root = tmp_root("osu-large");
        let path = root.join("beatmap").join("Big").join("big.osu");
        let header = osu_text(Some(3), Some("7"), "Big Osu", "MX");
        let mut body = header.clone();
        for index in 0..20_000u32 {
            body.push_str(&format!("64,192,{},1,0,0:0:0:0:\n", 1000 + index));
        }
        write_file(&path, &body);

        // 前缀窗口是本判据的前提：头部必须整份落在里面，否则大文件会被误判成坏文件
        let (_, prefix) = hash_prefix_and_digest(&path).unwrap();
        assert_eq!(prefix.len(), PREFIX_LIMIT, "大文件的前缀窗口必须打满");
        assert!(PREFIX_LIMIT >= 4096, "前缀窗口至少要有几 KB 才谈得上覆盖头部");
        assert!(
            header.len() < prefix.len(),
            "头部（{} B）必须整份落在前缀窗口（{} B）里",
            header.len(),
            prefix.len()
        );

        let lib = rebuild(&root);

        assert_eq!(lib.stats.indexed, 1);
        assert_eq!(lib.stats.skipped_unparsed, 0);
        let md5 = md5_of_text(&body);
        let entry = lib.lookup(&md5).expect("整份流式哈希（不止前缀）必须命中");
        assert_eq!(entry.path, path);
        assert_eq!(
            lib.meta.get(&path),
            Some(&ChartMeta {
                title: "Big Osu".to_string(),
                artist: "Artist Osu".to_string(),
                version: "MX".to_string(),
                keys: 7,
            })
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 跨扩展名的同 md5 重复：裁决规则不变（相对路径字典序最小者胜），`duplicate_md5` 照计。
    /// 只有"既能当 `.mc` 又能当 `.osu`"的同一份字节才构造得出这种重复。
    #[test]
    fn duplicate_md5_across_extensions_keeps_lexicographically_smallest_relative_path() {
        let root = tmp_root("dup-cross");
        let base = root.join("beatmap");
        let text = format!(
            "{}\n\n[General]\nMode: 3\n\n[Metadata]\nTitle:Dup\nArtist:Artist A\nVersion:MX\n\n[Difficulty]\nCircleSize:4\n",
            mc_text(0, Some(6), "Dup", "MX")
        );
        // 相对路径 "beatmap/Zeta/0/b.mc" 与 "beatmap/alpha/0/a.osu"：'Z' (0x5A) < 'a' (0x61)
        write_file(&base.join("Zeta").join("0").join("b.mc"), &text);
        write_file(&base.join("alpha").join("0").join("a.osu"), &text);

        let lib = rebuild(&root);
        let md5 = md5_of_text(&text);
        assert_eq!(lib.stats.indexed, 1, "两份字节相同，只有字典序最小者入索引");
        assert_eq!(lib.stats.duplicate_md5, 1);
        assert_eq!(lib.lookup(&md5).unwrap().path, base.join("Zeta").join("0").join("b.mc"));
        assert_eq!(lib.meta.len(), 1);
        assert!(!lib.meta.contains_key(&base.join("alpha").join("0").join("a.osu")));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn duplicate_md5_keeps_lexicographically_smallest_relative_path() {
        let root = tmp_root("dup");
        let base = root.join("beatmap");
        let text = mc_text(0, Some(6), "Dup", "MX");
        // 相对路径 "beatmap/Zeta/0/b.mc" 与 "beatmap/alpha/0/a.mc"：'Z' (0x5A) < 'a' (0x61)
        write_file(&base.join("Zeta").join("0").join("b.mc"), &text);
        write_file(&base.join("alpha").join("0").join("a.mc"), &text);

        let lib = rebuild(&root);
        let md5 = md5_of_text(&text);
        assert_eq!(lib.stats.indexed, 1);
        assert_eq!(lib.stats.duplicate_md5, 1);
        assert_eq!(lib.lookup(&md5).unwrap().path, base.join("Zeta").join("0").join("b.mc"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn large_chart_is_hashed_over_the_whole_file() {
        let root = tmp_root("large");
        let path = root.join("beatmap").join("Big").join("0").join("big.mc");
        let mut text = mc_text(0, Some(7), "Big Song", "MX");
        text.push_str(&"x".repeat(200 * 1024));
        write_file(&path, &text);

        let lib = rebuild(&root);
        assert_eq!(lib.stats.indexed, 1);
        assert_eq!(lib.stats.skipped_unparsed, 0);
        let md5 = md5_of_text(&text);
        let entry = lib.lookup(&md5).expect("整份流式哈希（不止前缀）必须命中");
        assert_eq!(entry.path, path);
        assert_eq!(lib.meta.get(&path).unwrap().keys, 7);
        assert_eq!(lib.meta.get(&path).unwrap().title, "Big Song");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_meta_or_unparsable_meta_is_counted_unparsed() {
        let root = tmp_root("badmeta");
        let base = root.join("beatmap").join("Album");
        write_file(&base.join("no_meta.mc"), "{\"note\":[]}");
        write_file(&base.join("truncated.mc"), "{\"meta\":{\"mode\":0,\"mode_ext\":{\"column\":4}");
        write_file(&base.join("no_column.mc"), &mc_text(0, None, "NoColumn", "Hard"));

        let lib = rebuild(&root);
        assert_eq!(lib.stats.indexed, 0);
        assert_eq!(lib.stats.skipped_unparsed, 2);
        assert_eq!(lib.stats.skipped_non_key, 1);
        assert!(lib.is_empty());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn empty_or_missing_beatmap_dir_does_not_panic() {
        let missing = std::env::temp_dir().join(format!(
            "mma-malody4-lib-missing-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let lib = rebuild(&missing);
        assert_eq!(lib.stats, LibraryStats::default());
        assert!(lib.is_empty());
        assert_eq!(lib.len(), 0);
        assert!(lib.lookup("00").is_none());

        let root = tmp_root("empty");
        std::fs::create_dir_all(root.join("beatmap")).unwrap();
        let lib = rebuild(&root);
        assert_eq!(lib.stats, LibraryStats::default());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn meta_scan_handles_braces_inside_strings() {
        let text = "{\"meta\":{\"mode\":0,\"version\":\"Ha}{rd\",\"song\":{\"title\":\"A}\\\"B\",\"artist\":\"C\"},\"mode_ext\":{\"column\":4}},\"note\":[]}";
        let root = tmp_root("braces");
        let path = root.join("beatmap").join("Q").join("0").join("q.mc");
        write_file(&path, text);

        let lib = rebuild(&root);
        assert_eq!(lib.stats.indexed, 1);
        let meta = lib.meta.get(&path).unwrap();
        assert_eq!(meta.version, "Ha}{rd");
        assert_eq!(meta.title, "A}\"B");
        assert_eq!(meta.keys, 4);

        let _ = std::fs::remove_dir_all(&root);
    }

    // ---- 变更探测（周期重扫的判据） ----

    /// 重建时记下的指纹必须与事后一遍**只读元数据**的扫描逐字段一致：否则周期重扫会把
    /// "没变"当成"变了"，每次 60s 又把 22 MB 白哈希一遍（正是本机制要消灭的开销）。
    /// 同时钉住口径：只有 `.mc` / `.osu` 且没被跳过的文件才进指纹。
    #[test]
    fn the_fingerprint_recorded_by_rebuild_matches_a_metadata_only_scan() {
        let root = tmp_root("fp-same");
        let base = root.join("beatmap").join("Album").join("0");
        write_file(&base.join("a.mc"), &mc_text(0, Some(4), "A", "MX"));
        write_file(&base.join("b.mc"), &mc_text(6, Some(8), "B", "MX"));
        write_file(&base.join("c.osu"), &osu_text(Some(3), Some("7"), "C", "MX"));
        // 不参与索引的目录项：封面、非谱面扩展名、junk（`__MACOSX` 目录与 `._` 文件）
        write_file(&base.join("cover.jpg"), "jpeg");
        write_file(&base.join("chart.tja"), "TITLE:x\n");
        write_file(&base.join("._junk.mc"), &mc_text(0, Some(4), "Junk", "MX"));
        write_file(
            &root.join("beatmap").join("__MACOSX").join("._x.mc"),
            &mc_text(0, Some(4), "Junk", "MX"),
        );

        let lib = rebuild(&root);
        let scanned = LibraryFingerprint::scan(&root);
        assert_eq!(
            lib.fingerprint, scanned,
            "rebuild 记下的指纹必须与只读元数据扫描逐字段相同"
        );
        assert_eq!(lib.fingerprint.charts, 3, "只有 3 个谱面文件进指纹");
        assert!(lib.fingerprint.bytes > 0);
        assert!(lib.fingerprint.newest.is_some());

        // 库里没动过 → 周期重扫一律"不重建"
        assert_eq!(rescan_change(&root, &lib.fingerprint), None);

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 三种变化都必须被看见：新增文件、文件大小变化、只有 mtime 变化（文件数与字节数都不变）。
    #[test]
    fn rescan_detects_added_files_size_changes_and_mtime_changes() {
        let root = tmp_root("fp-change");
        let base = root.join("beatmap").join("Album");
        let stable = base.join("a.mc");
        write_file(&stable, &mc_text(0, Some(4), "A", "MX"));
        let lib = rebuild(&root);
        assert_eq!(rescan_change(&root, &lib.fingerprint), None, "先确认静止态不改判");

        // ① 新增一张谱面
        write_file(&base.join("b.mc"), &mc_text(0, Some(6), "B", "MX"));
        let added = rescan_change(&root, &lib.fingerprint).expect("新增文件必须被看见");
        assert_eq!(added.charts, lib.fingerprint.charts + 1);

        // ② 已存在的谱面变大（总字节数变化）
        let grown = format!("{} ", mc_text(0, Some(6), "B", "MX"));
        write_file(&base.join("b.mc"), &grown);
        let after_grow = rescan_change(&root, &added).expect("字节数变化必须被看见");
        assert_eq!(after_grow.bytes, added.bytes + 1);

        // ③ 只有 mtime 变化：文件数与总字节数都不变，只有"最新 mtime"这一分量动
        let future = SystemTime::now() + Duration::from_secs(3600);
        let file = std::fs::OpenOptions::new().write(true).open(&stable).unwrap();
        file.set_modified(future).unwrap();
        drop(file);
        let after_mtime = rescan_change(&root, &after_grow).expect("最新 mtime 变化必须被看见");
        assert_eq!(after_mtime.charts, after_grow.charts);
        assert_eq!(after_mtime.bytes, after_grow.bytes);
        assert!(
            after_mtime.newest.unwrap() > after_grow.newest.unwrap(),
            "最新 mtime 必须前移"
        );
        // 新的静止态同样稳定
        assert_eq!(rescan_change(&root, &after_mtime), None);

        let _ = std::fs::remove_dir_all(&root);
    }
}
