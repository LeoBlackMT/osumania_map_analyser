// server::bridge::chart - 谱面路径沙箱校验、元数据与键数派生以及 song 帧生成

use std::fs;
use std::path::{Path, PathBuf};
use crate::config;
use crate::frames::CHART_MAX_BYTES;

/// 路径沙箱的拒绝原因（**每类一个可区分的错误**：单测逐条断言）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxError {
    /// `malodyRoot` 未配置或无法解析。
    RootUnavailable,
    /// 目标无法解析（不存在 / 权限 / 坏路径）。
    Unresolvable,
    /// 解析后落在 `{malodyRoot}/chart/` 之外。
    Escape,
    /// 后缀不是 `.mc` / `.osu`。
    BadSuffix,
    /// 不是普通文件（目录等）。
    NotAFile,
    /// 0 字节。
    Empty,
    /// 超过 50 MB。
    TooLarge,
}

impl SandboxError {
    /// 403 响应体与壳日志共用的文案（`path outside` 类记录是验收判据）。
    pub fn message(self) -> &'static str {
        match self {
            SandboxError::RootUnavailable => "malodyRoot unavailable",
            SandboxError::Unresolvable => "chart path cannot be resolved",
            SandboxError::Escape => "path outside malodyRoot/chart",
            SandboxError::BadSuffix => "chart suffix must be .mc or .osu",
            SandboxError::NotAFile => "chart path is not a file",
            SandboxError::Empty => "chart file is empty",
            SandboxError::TooLarge => "chart file exceeds 50MB",
        }
    }
}

/// 路径沙箱（`screen ∈ {selection, playing, result}` 时必过；`other` 跳过）。
///
/// 顺序固定：`normalize_path` → `canonicalize`（失败即拒）→ 必须
/// `starts_with(canonicalize(malody_root)/chart/)` → 后缀 ∈ `{.mc, .osu}`（大小写不敏感）
/// → `is_file()` → `1 B <= len <= 50 MB`。
///
/// **不得放宽前缀校验**：`starts_with` 是"本地 HTTP 端点不得成为任意文件读取原语"的
/// 唯一保证（同 `post.rs::handle_resolve` 的判据，只是这里额外收紧到 `chart/`）。
pub fn sandbox_chart_path(root: &Path, raw_path: &str) -> Result<PathBuf, SandboxError> {
    let root_canon = root.canonicalize().map_err(|_| SandboxError::RootUnavailable)?;
    let chart_root = root_canon.join("chart");
    let normalized = config::normalize_path(raw_path);
    if normalized.is_empty() {
        return Err(SandboxError::Unresolvable);
    }
    let candidate = PathBuf::from(&normalized)
        .canonicalize()
        .map_err(|_| SandboxError::Unresolvable)?;
    if !candidate.starts_with(&chart_root) {
        return Err(SandboxError::Escape);
    }
    let suffix_ok = candidate
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| matches!(e.to_ascii_lowercase().as_str(), "mc" | "osu"))
        .unwrap_or(false);
    if !suffix_ok {
        return Err(SandboxError::BadSuffix);
    }
    let meta = fs::metadata(&candidate).map_err(|_| SandboxError::Unresolvable)?;
    if !meta.is_file() {
        return Err(SandboxError::NotAFile);
    }
    let len = meta.len();
    if len == 0 {
        return Err(SandboxError::Empty);
    }
    if len > CHART_MAX_BYTES {
        return Err(SandboxError::TooLarge);
    }
    Ok(candidate)
}

/// `song` 帧的元信息（`.mc` / `.osu` 的差异全部收敛在这里）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChartFields {
    pub title: String,
    pub artist: String,
    pub level: String,
    pub keys: u64,
}

/// `.osu` 的 `[Difficulty]` 段 `CircleSize` 扫描（壳侧简单扫描；页面侧 `analysis.js`
/// 在本通道不可用）。只在 `[Difficulty]` 段内认键，遇到下一个 `[Section]` 即停。
pub fn osu_circle_size(text: &str) -> u64 {
    let mut in_difficulty = false;
    for line in text.lines() {
        let line = line.trim().trim_start_matches('\u{feff}');
        if line.starts_with('[') && line.ends_with(']') {
            in_difficulty = line.eq_ignore_ascii_case("[Difficulty]");
            continue;
        }
        if !in_difficulty {
            continue;
        }
        let Some(rest) = line.strip_prefix("CircleSize") else {
            continue;
        };
        let Some(rest) = rest.trim_start().strip_prefix(':') else {
            continue;
        };
        return rest
            .trim()
            .parse::<f64>()
            .ok()
            .map(|v| v.round() as u64)
            .unwrap_or(0);
    }
    0
}

/// 谱面字段派生（**字段来源钉死**，见 CONTRACT.md §11 的来源表）：
///
/// | 字段 | 首选 | 回落 |
/// |---|---|---|
/// | `level` | payload 的 `version`（游戏里的 `DisplayVersion`） | `.mc` 的 `/meta/version` → 文件名 stem（**不从 `.osu` 里猜**：osu 无 level 概念） |
/// | `keys` | `.mc` 的 `/meta/mode_ext/column` | `.osu` 的 `[Difficulty]/CircleSize` → `0`（`0` 再由调用方回落 `4`，与 `post.rs` 同法） |
/// | `title` / `artist` | `.mc` 的 `/meta/song/{title,artist}` | 文件名 stem / `""` |
///
/// `chart_hash` 不使用（仅诊断）；identity 用壳自算的 `contentMd5`。
pub fn derive_fields(text: &str, chart_path: &Path, payload_version: &str) -> ChartFields {
    let stem = chart_path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let is_mc = chart_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("mc"))
        .unwrap_or(false);
    let mc = if is_mc {
        serde_json::from_str::<serde_json::Value>(text).ok()
    } else {
        None
    };
    let mc_str = |pointer: &str| -> Option<String> {
        mc.as_ref()
            .and_then(|v| v.pointer(pointer))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty())
    };

    let level = if !payload_version.is_empty() {
        payload_version.to_string()
    } else {
        mc_str("/meta/version").unwrap_or(stem.clone())
    };
    let keys = mc
        .as_ref()
        .and_then(|v| v.pointer("/meta/mode_ext/column"))
        .and_then(|v| v.as_u64())
        .filter(|k| *k > 0)
        .unwrap_or_else(|| {
            if is_mc {
                0
            } else {
                osu_circle_size(text)
            }
        });
    ChartFields {
        title: mc_str("/meta/song/title").unwrap_or(stem),
        artist: mc_str("/meta/song/artist").unwrap_or_default(),
        level,
        keys,
    }
}

/// identity：`mdy:{title}:{level}:{keys}:{contentMd5}u`。
///
/// `u` 后缀 = "上游桥通道"来源标记，与 Lua 通道的 `mdy:…:{contentMd5}` 区分。代价是同一
/// 谱面在两条通道间切换时会多算一次（LRU 里两条快照）——刻意的来源可追溯性取舍。
pub fn identity_of(fields: &ChartFields, content_md5: &str) -> String {
    format!(
        "mdy:{}:{}:{}:{}u",
        if fields.title.is_empty() {
            "untitled"
        } else {
            &fields.title
        },
        if fields.level.is_empty() {
            "Unknown"
        } else {
            &fields.level
        },
        if fields.keys > 0 { fields.keys } else { 4 },
        content_md5
    )
}

/// `song` 帧构造（真实事件与重连补发**共用此唯一构造点**）。
///
/// `judge` / `pro` / `turbo` / `winScale` 是 v5 的桥通道字段：**未知一律是 `null`**（不是省略、
/// 不是 `1.0`）——页面按 `null` 关闭动态 OD 并在状态行说明原因。
pub fn song_frame(
    request_id: &str,
    fields: &ChartFields,
    identity: &str,
    rate_text: &str,
    raw_text: String,
    screen: &str,
    judge: Option<u8>,
    pro: Option<bool>,
    turbo: Option<bool>,
    win_scale: Option<f64>,
) -> serde_json::Value {
    let keys = if fields.keys > 0 { fields.keys } else { 4 };
    serde_json::json!({
        "requestId": request_id,
        "source": "malody",
        "identity": identity,
        "modData": { "speedRate": rate_text },
        "meta": {
            "title": fields.title,
            "artist": fields.artist,
            "version": if fields.level.is_empty() { "Unknown" } else { &fields.level },
            "keys": keys,
            "devMsd8": [],
        },
        "cover": null,
        "rawText": raw_text,
        "screen": screen,
        "judge": judge,
        "pro": pro,
        "turbo": turbo,
        "winScale": win_scale,
    })
}
