// config::json - 壳配置与插件离线设置 JSON 文件解析、合并与写入

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use super::common::{exe_dir, normalize_path};
use super::tosu::{plugin_dir, read_tosu_settings, TosuInfo};

pub const SHELL_CONFIG_FILE: &str = "mma-shell-config.json";
pub const PLUGIN_SETTINGS_FILE: &str = "mma-settings.json";

/// 读取 exe 所在目录下的 mma-shell-config.json（不存在/损坏 → 空对象；损坏时记录警告到日志）。
pub fn read_shell_config() -> serde_json::Value {
    read_exe_json(SHELL_CONFIG_FILE, "mma-shell-config.json")
}

/// 读取 exe 旁 mma-settings.json（全量插件设置；无 tosu 用户手动编辑）。
pub fn read_plugin_settings() -> serde_json::Value {
    read_exe_json(PLUGIN_SETTINGS_FILE, "mma-settings.json")
}

pub fn read_exe_json(file: &str, display: &str) -> serde_json::Value {
    match exe_dir() {
        Some(dir) => read_json_file(&dir.join(file), display),
        None => serde_json::Value::Null,
    }
}

/// 读一份 JSON 文件：不存在/读不到 → `Null`；解析失败 → 记 stderr 警告并 `Null`。
pub fn read_json_file(path: &Path, display: &str) -> serde_json::Value {
    match fs::read_to_string(path) {
        Ok(s) => match serde_json::from_str(&s) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("{} parse failed (using defaults): {}", display, e);
                serde_json::Value::Null
            }
        },
        Err(_) => serde_json::Value::Null,
    }
}

/// 日志级别（mma-shell-config.json 的 logLevel；默认 info）。
pub fn log_level() -> String {
    read_shell_config()
        .get("logLevel")
        .and_then(|v| v.as_str())
        .map(|s| s.to_lowercase())
        .filter(|s| matches!(s.as_str(), "debug" | "info" | "warn" | "error" | "off"))
        .unwrap_or_else(|| "info".to_string())
}

/// 取配置里的路径字段（归一化）；缺失/空 → None。
pub fn config_path(value: &serde_json::Value, key: &str) -> Option<PathBuf> {
    let raw = value.get(key).and_then(|v| v.as_str())?;
    let norm = normalize_path(raw);
    if norm.is_empty() {
        None
    } else {
        Some(PathBuf::from(norm))
    }
}

/// 启动时确保 mma-shell-config.json 存在（无 tosu 用户可发现并直接编辑）。
/// 骨架含 hotkeys 与 logLevel（用户验收项 1：骨架必须完整可编辑）。
pub fn ensure_shell_config() {
    let Some(dir) = exe_dir() else {
        return;
    };
    ensure_shell_config_in(&dir);
}

/// `ensure_shell_config` 的可注入路径版（单测打在这条缝上，绝不碰 exe 旁的真配置）。
pub fn ensure_shell_config_in(dir: &Path) {
    let path = dir.join(SHELL_CONFIG_FILE);
    if !path.exists() {
        let _ = fs::write(
            &path,
            "{\n  \"gameClient\": \"Auto\",\n  \"osuTransport\": \"auto\",\n  \"etternaRoot\": \"\",\n  \"malodyRoot\": \"\",\n  \"malody4Root\": \"\",\n  \"hotkeys\": {\n    \"topmost\": \"Ctrl+Shift+T\",\n    \"clickThrough\": \"Ctrl+Shift+C\",\n    \"close\": \"Ctrl+Q\",\n    \"settings\": \"Ctrl+Shift+S\"\n  },\n  \"logLevel\": \"info\"\n}\n",
        );
        return;
    }
    let mut existing = read_json_file(&path, "mma-shell-config.json");
    if backfill_shell_hotkeys(&mut existing) {
        let _ = write_exe_json(dir.to_path_buf(), SHELL_CONFIG_FILE, &existing);
    }
}

/// 骨架里的 hotkeys 默认值（**必须与上面骨架的字面量一致**：写骨架与补缺是同一套默认）。
pub const DEFAULT_HOTKEYS: [(&str, &str); 4] = [
    ("topmost", "Ctrl+Shift+T"),
    ("clickThrough", "Ctrl+Shift+C"),
    ("close", "Ctrl+Q"),
    ("settings", "Ctrl+Shift+S"),
];

/// 给已有配置补 hotkeys 缺键（只加不改）：返回是否真的补了（→ 是否需要落盘）。
pub fn backfill_shell_hotkeys(config: &mut serde_json::Value) -> bool {
    let Some(root) = config.as_object_mut() else {
        return false;
    };
    let mut changed = false;
    if !root.get("hotkeys").map(|v| v.is_object()).unwrap_or(false) {
        root.insert(
            "hotkeys".to_string(),
            serde_json::Value::Object(serde_json::Map::new()),
        );
        changed = true;
    }
    let Some(hotkeys) = root.get_mut("hotkeys").and_then(|v| v.as_object_mut()) else {
        return changed;
    };
    for (key, default) in DEFAULT_HOTKEYS {
        let present = hotkeys
            .get(key)
            .and_then(|v| v.as_str())
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        if !present {
            hotkeys.insert(
                key.to_string(),
                serde_json::Value::String(default.to_string()),
            );
            changed = true;
        }
    }
    changed
}

pub fn write_exe_json(dir: PathBuf, file: &str, value: &serde_json::Value) -> bool {
    let path = dir.join(file);
    let tmp = path.with_extension("json.tmp");
    let ok = fs::write(&tmp, serde_json::to_string_pretty(value).unwrap_or_default()).is_ok()
        && fs::rename(&tmp, &path).is_ok();
    ok
}

/// 全量插件设置解析（优先级链，**单一在线门控**）。
pub fn resolve_plugin_settings(shared: &crate::server::Shared) -> serde_json::Value {
    if shared.tosu.is_some() && *shared.tosu_online.lock().unwrap() {
        if let Some(info) = shared.tosu.as_ref() {
            let from_tosu = read_tosu_settings(info);
            if from_tosu.is_object() && !from_tosu.as_object().map(|m| m.is_empty()).unwrap_or(true) {
                return from_tosu;
            }
        }
    }
    let local = read_plugin_settings();
    if local.is_object() {
        return local;
    }
    let defaults = generate_default_plugin_settings();
    if defaults.is_object() {
        let _ = write_plugin_settings(&defaults);
    }
    defaults
}

/// 是否需要生成 mma-settings.json 本地骨架（纯判定，单测四象限）。
pub fn should_seed_local_settings(online: bool, has_local: bool) -> bool {
    !online && !has_local
}

/// 启动时确保 mma-settings.json 存在：**仅**离线且无本地设置文件时生成默认骨架。
pub fn ensure_plugin_settings(tosu: &Option<TosuInfo>, online: bool) {
    let _ = tosu;
    let has_local = read_plugin_settings().is_object();
    if !should_seed_local_settings(online, has_local) {
        return;
    }
    let defaults = generate_default_plugin_settings();
    if defaults.is_object() {
        let _ = write_plugin_settings(&defaults);
    }
}

/// 从插件 settings.json 生成默认设置骨架（所有条目 value 字段 → 顶层键）。
pub fn generate_default_plugin_settings() -> serde_json::Value {
    let mut out = serde_json::Map::new();
    let path = plugin_dir().join("settings.json");
    if let Ok(text) = fs::read_to_string(&path) {
        if let Ok(entries) = serde_json::from_str::<Vec<serde_json::Value>>(&text) {
            for entry in entries {
                if let (Some(id), Some(val)) = (
                    entry.get("uniqueID").and_then(|v| v.as_str()),
                    entry.get("value").cloned(),
                ) {
                    out.insert(id.to_string(), val);
                }
            }
        }
    }
    serde_json::Value::Object(out)
}

/// 离线 /settings POST 落盘：写入 mma-settings.json（全量插件设置）。
pub fn write_plugin_settings(value: &serde_json::Value) -> bool {
    let Some(dir) = exe_dir() else {
        return false;
    };
    write_exe_json(dir, PLUGIN_SETTINGS_FILE, value)
}

pub fn plugin_settings_path() -> Option<PathBuf> {
    Some(exe_dir()?.join(PLUGIN_SETTINGS_FILE))
}

pub fn shell_config_path() -> Option<PathBuf> {
    Some(exe_dir()?.join(SHELL_CONFIG_FILE))
}

pub fn write_shell_config(value: &serde_json::Value) -> bool {
    match exe_dir() {
        Some(dir) => write_exe_json(dir, SHELL_CONFIG_FILE, value),
        None => false,
    }
}

static WRITE_LOCK: Mutex<()> = Mutex::new(());

/// 对象递归合并。
pub fn merge_json(base: &serde_json::Value, patch: &serde_json::Value) -> serde_json::Value {
    match (base, patch) {
        (serde_json::Value::Object(b), serde_json::Value::Object(p)) => {
            let mut out = b.clone();
            for (k, v) in p {
                if v.is_null() {
                    continue;
                }
                let merged = match out.get(k) {
                    Some(existing) => merge_json(existing, v),
                    None => v.clone(),
                };
                out.insert(k.clone(), merged);
            }
            serde_json::Value::Object(out)
        }
        _ => patch.clone(),
    }
}

pub fn merge_exe_json(
    dir: &Path,
    file: &str,
    display: &str,
    patch: &serde_json::Value,
) -> Option<serde_json::Value> {
    let _guard = WRITE_LOCK.lock().unwrap();
    let base = read_json_file(&dir.join(file), display);
    if !base.is_object() {
        return None;
    }
    let merged = merge_json(&base, patch);
    if !write_exe_json(dir.to_path_buf(), file, &merged) {
        return None;
    }
    Some(merged)
}

pub fn merge_plugin_settings(patch: &serde_json::Value) -> Option<serde_json::Value> {
    let dir = exe_dir()?;
    merge_exe_json(dir.as_path(), PLUGIN_SETTINGS_FILE, "mma-settings.json", patch)
}

pub fn patch_shell_config(patch: &serde_json::Value) -> Option<serde_json::Value> {
    let dir = exe_dir()?;
    merge_exe_json(dir.as_path(), SHELL_CONFIG_FILE, "mma-shell-config.json", patch)
}
