//! Optional game launch for the first Linux shell instance. The osu! reader only observes.
use std::ffi::OsStr;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::Mutex;

use crate::osu::lazer::linux::find_game;

static LAUNCH_ERROR: Mutex<Option<String>> = Mutex::new(None);

pub(crate) fn take_error() -> Option<String> {
    LAUNCH_ERROR.lock().unwrap().take()
}

pub(crate) fn launch_if_requested() {
    if !std::env::args_os().any(|arg| arg == "--launch-lazer") {
        return;
    }
    std::thread::spawn(|| {
        let result = launch_game().and_then(|mut child| {
            let status = child.wait().map_err(|e| e.to_string())?;
            if status.success() { Ok(()) } else {
                Err(format!("osu!lazer launcher exited with {status}. Check MMA_LAZER_EXECUTABLE and run the game executable directly."))
            }
        });
        if let Err(error) = result {
            crate::server::log::log_at("warn", &error);
            *LAUNCH_ERROR.lock().unwrap() = Some(error);
        }
    });
}

fn launch_game() -> Result<Child, String> {
    if find_game()?.is_some() {
        return Err("osu!lazer is already running. Close it and run the launcher again.".into());
    }
    let program = PathBuf::from(
        std::env::var_os("MMA_LAZER_EXECUTABLE").unwrap_or_else(|| "osu-lazer".into()),
    );
    // Resolve before changing cwd: package launchers and portable builds can load
    // assets relative to the installation, even when invoked by name on PATH.
    use std::os::unix::fs::PermissionsExt;
    let path = if program.components().count() > 1
        || program.file_name() == Some(OsStr::new("osu!.dll"))
    {
        Some(std::path::absolute(&program).map_err(|e| e.to_string())?)
    } else {
        std::env::var_os("PATH").and_then(|paths| {
            std::env::split_paths(&paths)
                .map(|dir| dir.join(&program))
                .find(|p| {
                    p.metadata()
                        .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
                })
                .and_then(|p| std::path::absolute(p).ok())
        })
    };
    let mut cmd = if let Some(path) = path {
        let mut cmd = if path.file_name() == Some(OsStr::new("osu!.dll")) {
            let mut cmd = Command::new("dotnet");
            cmd.arg(&path);
            cmd
        } else {
            Command::new(&path)
        };
        let install_path = path.canonicalize().unwrap_or(path);
        if let Some(dir) = install_path.parent() {
            cmd.current_dir(dir);
        }
        cmd
    } else {
        Command::new(&program)
    };
    cmd.spawn().map_err(|e| {
        format!(
            "Cannot start {}: {e}. Set MMA_LAZER_EXECUTABLE to the game executable.",
            program.display()
        )
    })
}
