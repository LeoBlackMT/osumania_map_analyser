// config::detect - 外部游戏目录（Etterna, Malody V, Malody 4）启发探测与 Steam 库扫描

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::common::normalize_path;

/// 盘符预检。
pub fn drive_root_ready(p: &Path) -> bool {
    let Some(first) = p.iter().next() else {
        return false;
    };
    let first = first.to_string_lossy();
    let bytes = first.as_bytes();
    if bytes.len() == 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        fs::metadata(format!("{}/", &first[..2])).is_ok()
    } else {
        true
    }
}

pub const DETECT_CACHE_TTL: Duration = Duration::from_secs(30);

static ETTERNA_DETECT_CACHE: Mutex<Option<(Instant, Option<PathBuf>)>> = Mutex::new(None);
static MALODY_DETECT_CACHE: Mutex<Option<(Instant, Option<PathBuf>)>> = Mutex::new(None);
static MALODY4_DETECT_CACHE: Mutex<Option<(Instant, Option<PathBuf>)>> = Mutex::new(None);

/// 清空三个探测缓存（壳配置改了根目录后立即失效，否则 ≤30s 内仍用旧结果）。
pub fn clear_detect_caches() {
    if let Ok(mut c) = ETTERNA_DETECT_CACHE.lock() {
        *c = None;
    }
    if let Ok(mut c) = MALODY_DETECT_CACHE.lock() {
        *c = None;
    }
    if let Ok(mut c) = MALODY4_DETECT_CACHE.lock() {
        *c = None;
    }
}

pub fn detect_cached<F: Fn() -> Option<PathBuf>>(
    cache: &Mutex<Option<(Instant, Option<PathBuf>)>>,
    once: F,
) -> Option<PathBuf> {
    if let Ok(guard) = cache.lock() {
        if let Some((at, hit)) = guard.as_ref() {
            if at.elapsed() < DETECT_CACHE_TTL {
                return hit.clone();
            }
        }
    }
    let hit = once();
    if let Ok(mut guard) = cache.lock() {
        *guard = Some((Instant::now(), hit.clone()));
    }
    hit
}

pub fn steam_library_roots() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    fn push_unique(roots: &mut Vec<PathBuf>, p: &str) {
        let p = p.trim();
        if !p.is_empty() && !roots.iter().any(|r| r.to_string_lossy().eq_ignore_ascii_case(p)) {
            roots.push(PathBuf::from(p));
        }
    }
    #[cfg(windows)]
    {
        let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
            .open_subkey("Software\\Valve\\Steam")
            .ok();
        if let Some(key) = hkcu {
            if let Ok(p) = key.get_value::<String, _>("SteamPath") {
                push_unique(&mut roots, &p);
            }
        }
        for sub in ["SOFTWARE\\WOW6432Node\\Valve\\Steam", "SOFTWARE\\Valve\\Steam"] {
            let hklm = winreg::RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE)
                .open_subkey(sub)
                .ok();
            if let Some(key) = hklm {
                if let Ok(p) = key.get_value::<String, _>("InstallPath") {
                    push_unique(&mut roots, &p);
                }
            }
        }
    }
    let initial = roots.clone();
    for root in initial {
        let vdf = root.join("steamapps").join("libraryfolders.vdf");
        if let Ok(text) = fs::read_to_string(&vdf) {
            for line in text.lines() {
                let line = line.trim();
                if let Some(idx) = line.find("\"path\"") {
                    let rest = &line[idx + "\"path\"".len()..];
                    if let Some(vq) = rest.find('"') {
                        let after = &rest[vq + 1..];
                        if let Some(end) = after.find('"') {
                            let raw = &after[..end];
                            let decoded = raw.replace("\\\\", "\\").replace("\\/", "/");
                            push_unique(&mut roots, &decoded);
                        }
                    }
                }
            }
        }
    }
    roots
}

/// Etterna：Steam 库（appid 607810 的 common/Etterna）→ 常见路径 → env。
pub fn detect_etterna_root() -> Option<PathBuf> {
    detect_cached(&ETTERNA_DETECT_CACHE, detect_etterna_root_once)
}

fn detect_etterna_root_once() -> Option<PathBuf> {
    if let Ok(over) = env::var("MMA_ETTERNA_ROOT") {
        let p = PathBuf::from(normalize_path(&over));
        if drive_root_ready(&p) && p.join("Save").is_dir() {
            return Some(p);
        }
    }
    for lib in steam_library_roots() {
        let dir = lib.join("steamapps").join("common").join("Etterna");
        if drive_root_ready(&dir) && dir.join("Save").is_dir() {
            return Some(dir);
        }
    }
    let candidates = [
        "D:/Games/Etterna",
        "C:/Games/Etterna",
        "D:/Etterna",
        "C:/Etterna",
    ];
    for c in candidates {
        let dir = PathBuf::from(c);
        if drive_root_ready(&dir) && dir.join("Save").is_dir() {
            return Some(dir);
        }
    }
    None
}

/// MalodyV：Steam 库（common/MalodyV）→ 常见路径 → env。
pub fn detect_malody_root() -> Option<PathBuf> {
    detect_cached(&MALODY_DETECT_CACHE, detect_malody_root_once)
}

fn detect_malody_root_once() -> Option<PathBuf> {
    if let Ok(over) = env::var("MMA_MALODY_ROOT") {
        let p = PathBuf::from(normalize_path(&over));
        if drive_root_ready(&p) && p.join("chart").is_dir() && p.join("skin").is_dir() {
            return Some(p);
        }
    }
    for lib in steam_library_roots() {
        let dir = lib.join("steamapps").join("common").join("MalodyV");
        if drive_root_ready(&dir) && dir.join("chart").is_dir() && dir.join("skin").is_dir() {
            return Some(dir);
        }
    }
    let candidates = [
        "D:/Steam/steamapps/common/MalodyV",
        "D:/SteamLibrary/steamapps/common/MalodyV",
        "C:/Program Files (x86)/Steam/steamapps/common/MalodyV",
        "C:/SteamLibrary/steamapps/common/MalodyV",
    ];
    for c in candidates {
        let dir = PathBuf::from(c);
        if drive_root_ready(&dir) && dir.join("chart").is_dir() && dir.join("skin").is_dir() {
            return Some(dir);
        }
    }
    None
}

/// Malody 4.3.7 的启发候选。
pub const MALODY4_CANDIDATES: [&str; 5] = [
    "D:/Games/Malody-4.3.7",
    "C:/Games/Malody-4.3.7",
    "D:/Malody-4.3.7",
    "D:/Games/Malody",
    "C:/Malody-4.3.7",
];

/// Malody 4.3.7 根目录的启发式尾部。
pub fn detect_malody4_root(process_exe: Option<&Path>) -> Option<PathBuf> {
    detect_cached(&MALODY4_DETECT_CACHE, || detect_malody4_root_once(process_exe))
}

fn detect_malody4_root_once(process_exe: Option<&Path>) -> Option<PathBuf> {
    let hit = scan_malody4_candidates(&MALODY4_CANDIDATES);
    crate::server::log::log_at(
        "debug",
        &format!(
            "malody4 heuristic scan: process_exe={} -> {}",
            process_exe
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "none".to_string()),
            hit.as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "no candidate validated".to_string())
        ),
    );
    hit
}

/// 候选的三重检查。
pub fn scan_malody4_candidates(candidates: &[&str]) -> Option<PathBuf> {
    for candidate in candidates {
        let dir = PathBuf::from(candidate);
        if !drive_root_ready(&dir) {
            continue;
        }
        let exe = dir.join("malody.exe");
        if !dir.join("beatmap").is_dir() || !exe.is_file() {
            continue;
        }
        if crate::malody4::anchor::validate_pe_file(&exe, crate::malody4::anchor::ClientSpec::current())
            .is_err()
        {
            continue;
        }
        return Some(dir);
    }
    None
}
