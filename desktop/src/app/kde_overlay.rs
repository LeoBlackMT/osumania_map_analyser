use gtk::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::LazyLock;

const RULE_GROUP: &str = "mma-shell-fullscreen-overlay";
const OVERLAY_TITLE: &str = "ManiaMapAnalyser (Overlay)";
static RULE_READY: LazyLock<bool> = LazyLock::new(|| {
    match gtk::gdk::program_class()
        .ok_or_else(|| "GTK class unavailable".to_owned())
        .and_then(|class| install_rule(&class, reload_rules))
    {
        Ok(()) => true,
        Err(error) => {
            crate::server::log::log_at(
                "warn",
                &format!("Cannot enable KDE fullscreen overlay: {error}"),
            );
            false
        }
    }
});

/// 保持普通窗口类型，使用仅匹配主窗标题的 KWin 层级规则保留拖动与缩放。
pub(super) fn apply(window: &gtk::ApplicationWindow, topmost: bool) {
    if !std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|desktop| {
        desktop
            .split(':')
            .any(|name| name.eq_ignore_ascii_case("KDE"))
    }) {
        return;
    }
    let Some(native) = window.window() else {
        return;
    };
    if native.display().type_().name() != "GdkX11Display" {
        return;
    }
    window.set_keep_above(topmost);
    if topmost && !*RULE_READY {
        return;
    }
    // KWin 会在标题改变时重新匹配规则；无需每次开关都重载整个桌面配置。
    window.set_title(if topmost {
        OVERLAY_TITLE
    } else {
        "ManiaMapAnalyser"
    });
}

fn read_setting(group: &str, key: &str) -> Result<String, String> {
    let output = Command::new("kreadconfig6")
        .args([
            "--file",
            "kwinrulesrc",
            "--group",
            group,
            "--key",
            key,
            "--default",
            "",
        ])
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!("kreadconfig6 failed for {group}/{key}"));
    }
    let mut value = String::from_utf8(output.stdout).map_err(|error| error.to_string())?;
    value.truncate(value.trim_end().len());
    Ok(value)
}

fn write_setting(group: &str, key: &str, value: &str) -> Result<(), String> {
    let status = Command::new("kwriteconfig6")
        .args([
            "--file",
            "kwinrulesrc",
            "--group",
            group,
            "--key",
            key,
            value,
        ])
        .status()
        .map_err(|error| error.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("kwriteconfig6 failed for {group}/{key}"))
    }
}

fn user_dir(var: &str, default: &str) -> Result<PathBuf, String> {
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(default)))
        .ok_or_else(|| format!("Cannot locate {var}"))
}

// Keep one copy of the original file, even if a later write or reload fails.
fn backup_once(rules: &Path) -> Result<(), std::io::Error> {
    let mut source = match fs::File::open(rules) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        other => other?,
    };
    let dir = user_dir("XDG_STATE_HOME", ".local/state")
        .map_err(std::io::Error::other)?
        .join("ManiaMapAnalyser/backup");
    fs::create_dir_all(&dir)?;
    let backup = dir.join("kwinrulesrc.before-overlay");
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&backup)
    {
        Ok(mut file) => {
            if let Err(e) = std::io::copy(&mut source, &mut file) {
                fs::remove_file(backup)?;
                return Err(e);
            }
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(e),
    }
}

fn order_key() -> Result<&'static str, String> {
    let binary = if std::env::var("XDG_SESSION_TYPE").as_deref() == Ok("wayland") {
        "kwin_wayland"
    } else {
        "kwin_x11"
    };
    let output = Command::new(binary)
        .arg("--version")
        .output()
        .map_err(|e| e.to_string())?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut version = stdout.split_whitespace().last().unwrap_or("").split('.');
    match (
        version.next(),
        version.next().and_then(|s| s.parse::<u32>().ok()),
    ) {
        (Some("6"), Some(minor)) if output.status.success() => {
            Ok(if minor >= 8 { "Order" } else { "rules" })
        }
        _ => Err("Fullscreen window rules require KWin 6; using ordinary keep-above".into()),
    }
}

fn reload_rules() -> Result<(), String> {
    use gtk::gio::{self, BusType, DBusCallFlags};
    let bus =
        gio::bus_get_sync(BusType::Session, gio::Cancellable::NONE).map_err(|e| e.to_string())?;
    bus.call_sync(
        Some("org.kde.KWin"),
        "/KWin",
        "org.kde.KWin",
        "reconfigure",
        None,
        None,
        DBusCallFlags::NONE,
        2000,
        gio::Cancellable::NONE,
    )
    .map(|_| ())
    .map_err(|e| format!("KWin could not reload its window rules: {e}"))
}

fn install_rule(class: &str, reload: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
    let current_key = order_key()?;
    // When upgrading, KWin 6.8 migrates a nonempty legacy list before reading Order.
    let legacy = read_setting("General", "rules")?;
    let (key, mut order) = if current_key == "rules" || !legacy.is_empty() {
        ("rules", legacy)
    } else {
        (current_key, read_setting("General", current_key)?)
    };
    let registered = order.split(',').any(|entry| entry == RULE_GROUP);
    let marked = read_setting(RULE_GROUP, "X-MMA-Version")? == "2";
    if marked && registered && read_setting(RULE_GROUP, "wmclass")? == class {
        return Ok(());
    }
    let settings = [
        ("Description", "ManiaMapAnalyser fullscreen overlay"),
        ("wmclass", class),
        ("wmclassmatch", "1"),
        ("wmclasscomplete", "false"),
        ("title", OVERLAY_TITLE),
        ("titlematch", "1"),
        ("types", "1"),
        ("layer", "critical-notification"),
        ("layerrule", "2"),
    ];
    let mut changes = Vec::new();
    for (name, value) in settings {
        if read_setting(RULE_GROUP, name)? != value {
            changes.push((name, value));
        }
    }
    if !changes.is_empty() || !registered {
        backup_once(&user_dir("XDG_CONFIG_HOME", ".config")?.join("kwinrulesrc"))
            .map_err(|e| e.to_string())?;
        if marked {
            write_setting(RULE_GROUP, "X-MMA-Version", "pending")?;
        }
        for (name, value) in changes {
            write_setting(RULE_GROUP, name, value)?;
        }
        if !registered {
            if !order.is_empty() {
                order.push(',');
            }
            order.push_str(RULE_GROUP);
            write_setting("General", key, &order)?;
        }
    }
    reload()?;
    write_setting(RULE_GROUP, "X-MMA-Version", "2")
}
