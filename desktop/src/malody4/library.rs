// Malody 4.3.7 谱面库索引：遍历 `<root>/beatmap/**`，流式 md5 + 有界前缀 meta 提取。
//
// 只索引 `.mc`：入索引的准入判据是 `.mc` 自己的 `meta.mode ∈ 0..=6` **且**
// `meta.mode_ext.column` 存在。收 `.osu` 会把 osu! 其他模式的谱面一并放进索引——这是对外部
// 规格"只索引 .mc；.osu 亦可用"建议的有意偏离，理由与限制记在 docs/features/malody4-source.md。
//
// 准入为何是 `0..=6` 而不是 `== 0`（真机 214 张 `.mc` 的分类证据）：column 式游戏模式是
// mode 0–6（0 Key / 1 Catch / 2 Pad / 3 Taiko / 4 Ring / 5 Slide / 6 Live），mode 7/8
// （Spectacle 及以后）没有 `column`。旧判据 `mode == 0` 把 17 张真 Key 谱挡在索引外
// （其中 13 张是 `mode=6` + `column=8`）。mode 区间只是**廉价预筛**：真正的守卫是"谱面自带
// `column` 字段"，没有 `column` 的一律不收（mode 7/8 因此天然被拒）。这不是语义重分类。
//
// 逐文件单趟：BufReader 以固定块喂 `Md5`（增量更新，绝不整份读进内存），同时保留
// 前 64 KiB 前缀供 meta 提取（手工括号配对，不整份 JSON 解析）。

use crate::malody4::model::hex16;
use md5::{Digest, Md5};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// 目录递归深度上限（`<root>/beatmap` 记 0 层）。
pub const MAX_DEPTH: usize = 8;
/// 喂给 `Md5` 的固定块大小。
const HASH_CHUNK: usize = 64 * 1024;
/// 保留的文件前缀上限（`meta` 对象落在这段文本里）。
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
    pub skipped_non_key: usize,
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

/// 谱面元信息（`.mc` 的 `meta` 对象）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChartMeta {
    pub title: String,
    pub artist: String,
    pub version: String,
    pub keys: u64,
}

/// 谱面库索引。帧组装直接从索引取元信息，不再为每帧重读重解析谱面。
#[derive(Debug, Clone)]
pub struct ChartLibrary {
    pub root: PathBuf,
    pub by_md5: HashMap<String, LibraryEntry>,
    /// 按谱面路径存放元信息（只有入索引的文件才有）。
    pub meta: HashMap<PathBuf, ChartMeta>,
    pub built_at: Instant,
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

/// 重建索引：迭代式 DFS 遍历 `<root>/beatmap/**`。
pub fn rebuild(root: &Path) -> ChartLibrary {
    let mut lib = ChartLibrary {
        root: root.to_path_buf(),
        by_md5: HashMap::new(),
        meta: HashMap::new(),
        built_at: Instant::now(),
        stats: LibraryStats::default(),
    };
    // (相对路径, 绝对路径, md5, meta)：先收集再按相对路径排序裁决重复 md5
    let mut candidates: Vec<(String, PathBuf, String, ChartMeta)> = Vec::new();
    // 迭代式 DFS（不用递归）：(目录, 深度)
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
                lib.stats.skipped_junk += 1;
                continue;
            }
            if meta.is_dir() {
                if name == "__MACOSX" {
                    lib.stats.skipped_junk += 1;
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
                lib.stats.skipped_junk += 1;
                continue;
            }
            if !has_mc_extension(&path) {
                lib.stats.skipped_ext += 1;
                continue;
            }
            let (md5, prefix) = match hash_prefix_and_digest(&path) {
                Ok(value) => value,
                Err(_) => {
                    lib.stats.skipped_unparsed += 1;
                    continue;
                }
            };
            let chart_meta = match scan_meta(&prefix) {
                MetaScan::Key(chart_meta) => chart_meta,
                MetaScan::NonKey => {
                    lib.stats.skipped_non_key += 1;
                    continue;
                }
                MetaScan::Unparsed => {
                    lib.stats.skipped_unparsed += 1;
                    continue;
                }
            };
            candidates.push((relative_name(root, &path), path, md5, chart_meta));
        }
    }

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
    lib.stats.duplicate_md5 = duplicates;
    lib.stats.indexed = lib.by_md5.len();
    if duplicates > 0 {
        eprintln!(
            "[malody4] library: {duplicates} duplicate md5 chart(s) skipped (kept lexicographically smallest path)"
        );
    }
    lib
}

fn has_mc_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("mc"))
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
    use std::time::{SystemTime, UNIX_EPOCH};

    fn mc_text(mode: u64, column: Option<u64>, title: &str, version: &str) -> String {
        let mode_ext = match column {
            Some(keys) => format!("{{\"column\":{keys}}}"),
            None => "{}".to_string(),
        };
        format!(
            "{{\"meta\":{{\"mode\":{mode},\"version\":\"{version}\",\"song\":{{\"title\":\"{title}\",\"artist\":\"Artist A\"}},\"mode_ext\":{mode_ext}}},\"note\":[]}}"
        )
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
    fn rebuild_indexes_only_key_mode_mc_and_counts_every_skip() {
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
        // 非 .mc 扩展名
        write_file(&album_a.join("chart.tja"), "TITLE:x\n");
        write_file(&album_a.join("chart.osu"), "osu file format v14\n");
        // 与 key.mc 同字节：相对路径更大，故落败
        write_file(&album_b.join("copy.mc"), &key_text);

        let mut lib = rebuild(&root);

        assert_eq!(
            lib.stats,
            LibraryStats {
                indexed: 1,
                skipped_non_key: 1,
                skipped_junk: 2,
                skipped_ext: 2,
                skipped_unparsed: 1,
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
}
