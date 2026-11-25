use gtk::prelude::*;
use std::path::PathBuf;
use std::process::Command;
use std::sync::LazyLock;

const RULE_GROUP: &str = "mma-shell-fullscreen-overlay";
const OVERLAY_TITLE: &str = "ManiaMapAnalyser (Overlay)";
static RULE_READY: LazyLock<bool> = LazyLock::new(|| match install_rule() {
    Ok(()) => true,
    Err(error) => {
        crate::server::log::log_at("warn", &format!("Cannot enable KDE fullscreen overlay: {error}"));
        false
    }
});

/// 保持普通窗口类型，使用仅匹配主窗标题的 KWin 层级规则保留拖动与缩放。
pub(super) fn apply(window: &gtk::ApplicationWindow, topmost: bool) {
    if !std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|desktop| {
        desktop.split(':').any(|name| name.eq_ignore_ascii_case("KDE"))
    }) {
        return;
    }
    let Some(native) = window.window() else { return };
    if native.display().type_().name() != "GdkX11Display" {
        return;
    }
    if topmost && !*RULE_READY {
        return;
    }
    // KWin 会在标题改变时重新匹配规则；无需每次开关都重载整个桌面配置。
    window.set_title(if topmost { OVERLAY_TITLE } else { "ManiaMapAnalyser" });
    window.set_keep_above(topmost);
}

fn read_setting(group: &str, key: &str) -> Result<String, String> {
    let output = Command::new("kreadconfig6")
        .args(["--file", "kwinrulesrc", "--group", group, "--key", key, "--default", ""])
        .output().map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!("kreadconfig6 failed for {group}/{key}"));
    }
    let mut value = String::from_utf8(output.stdout).map_err(|error| error.to_string())?;
    value.truncate(value.trim_end().len());
    Ok(value)
}

fn write_setting(group: &str, key: &str, value: &str) -> Result<(), String> {
    let status = Command::new("kwriteconfig6")
        .args(["--file", "kwinrulesrc", "--group", group, "--key", key, value])
        .status().map_err(|error| error.to_string())?;
    if status.success() { Ok(()) } else { Err(format!("kwriteconfig6 failed for {group}/{key}")) }
}

fn install_rule() -> Result<(), String> {
    if read_setting(RULE_GROUP, "X-MMA-Version")? == "1" {
        return Ok(());
    }
    let config_home = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(path) => PathBuf::from(path),
        None => PathBuf::from(std::env::var_os("HOME").ok_or("HOME is unavailable")?).join(".config"),
    };
    let rules_file = config_home.join("kwinrulesrc");
    if rules_file.exists() {
        let timestamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| error.to_string())?.as_nanos();
        let backup = crate::config::plugin_dir().join("backup").join(format!("{timestamp}-kwin-overlay"));
        std::fs::create_dir_all(&backup).map_err(|error| error.to_string())?;
        std::fs::copy(&rules_file, backup.join("kwinrulesrc")).map_err(|error| error.to_string())?;
    }
    for (key, value) in [
        ("Description", "ManiaMapAnalyser fullscreen overlay"),
        ("wmclass", "Mma-shell"),
        ("wmclassmatch", "1"),
        ("wmclasscomplete", "false"),
        ("title", OVERLAY_TITLE),
        ("titlematch", "1"),
        ("types", "1"),
        ("layer", "critical-notification"),
        ("layerrule", "2"),
    ] {
        write_setting(RULE_GROUP, key, value)?;
    }
    // 保留现有顺序；旧版配置仍用 rules，KWin 自己负责向 Order 迁移。
    let legacy = read_setting("General", "rules")?;
    let (key, mut order) = if legacy.is_empty() {
        ("Order", read_setting("General", "Order")?)
    } else {
        ("rules", legacy)
    };
    if !order.split(',').any(|group| group == RULE_GROUP) {
        if !order.is_empty() { order.push(','); }
        order.push_str(RULE_GROUP);
        write_setting("General", key, &order)?;
    }
    let status = Command::new("qdbus6")
        .args(["org.kde.KWin", "/KWin", "org.kde.KWin.reconfigure"])
        .status().map_err(|error| error.to_string())?;
    if !status.success() { return Err("KWin did not reload its window rules".to_string()); }
    write_setting(RULE_GROUP, "X-MMA-Version", "1")
}
