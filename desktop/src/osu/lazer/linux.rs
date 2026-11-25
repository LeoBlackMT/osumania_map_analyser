use std::fs::{self, File};
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use super::score::{self, HitEventLog, PollState, ReplayFrameLog, ReplayPollState};
use super::screen::{read_eetype_identity, read_pe_typedef_name, read_screen_top};
use super::{
    env_table_path, load_table, read_u64, screen_state_for, shell_exe_dir, table_probe,
    target_from_files, try_site, ChainAddrs, RealFiles, Resolved, Source, TargetEnv,
    MARKER_PATTERN, SITE_DELTAS,
};
use crate::osu::offsets::OffsetTable;
use crate::osu::{Reader, ATTACH_RETRY, SCAN_RETRY};

const GAME_ASSEMBLY: &str = "osu.Game.dll";
const POLL_INTERVAL: Duration = Duration::from_millis(100);
const SCAN_CHUNK: usize = 1024 * 1024;

struct ProcessMemory {
    mem: File,
    proc_dir: PathBuf,
}

impl Source for ProcessMemory {
    fn read(&self, addr: u64, len: usize) -> Option<Vec<u8>> {
        let mut bytes = vec![0; len];
        self.mem.read_exact_at(&mut bytes, addr).ok()?;
        Some(bytes)
    }
}

struct Session {
    memory: ProcessMemory,
    table: OffsetTable,
    fields: Resolved,
    game_base: u64,
    game_type: u64,
    screen_module: u64,
    table_path: PathBuf,
    storage_root: Option<PathBuf>,
    default_files: Option<PathBuf>,
}

// The upstream table may only list osu.Game.dll types. Read the desktop
// assembly's TypeDef metadata using the same RVA-based parser as Windows.
struct DiskPe {
    bytes: Vec<u8>,
    sections: Vec<(usize, usize, usize)>, // RVA, raw file offset, raw size
}

impl DiskPe {
    fn open(path: &Path) -> Option<Self> {
        let bytes = fs::read(path).ok()?;
        let u16_at = |p| Some(u16::from_le_bytes(bytes.get(p..p + 2)?.try_into().ok()?) as usize);
        let u32_at = |p| Some(u32::from_le_bytes(bytes.get(p..p + 4)?.try_into().ok()?) as usize);
        let pe = u32_at(0x3c)?;
        if bytes.get(pe..pe + 4)? != b"PE\0\0" { return None; }
        let n = u16_at(pe + 6)?;
        let first = pe + 24 + u16_at(pe + 20)?;
        let mut sections = vec![(0, 0, first + n * 40)];
        for i in 0..n {
            let s = first + i * 40;
            sections.push((u32_at(s + 12)?, u32_at(s + 20)?, u32_at(s + 16)?));
        }
        Some(Self { bytes, sections })
    }
}

impl Source for DiskPe {
    fn read(&self, addr: u64, len: usize) -> Option<Vec<u8>> {
        self.sections.iter().find_map(|&(rva, file, size)| {
            let offset = usize::try_from(addr).ok()?.checked_sub(rva)?;
            if offset.checked_add(len)? > size { return None; }
            Some(self.bytes.get(file + offset..file + offset + len)?.to_vec())
        })
    }
}

fn find_game() -> Result<Option<(PathBuf, PathBuf)>, String> {
    let mut game = None;
    for entry in fs::read_dir("/proc").map_err(|error| error.to_string())?.flatten() {
        if entry.file_name().to_str().and_then(|name| name.parse::<u32>().ok()).is_none() {
            continue;
        }
        let dir = entry.path();
        if !fs::read_to_string(dir.join("comm")).is_ok_and(|name| name.trim() == "osu!") {
            continue;
        }
        let Ok(exe) = fs::read_link(dir.join("exe")) else { continue };
        if exe.file_name().and_then(|name| name.to_str()) != Some("osu!") {
            continue;
        }
        if game.is_some() {
            return Err("More than one native osu!lazer process is running".to_string());
        }
        game = Some((dir, exe));
    }
    Ok(game)
}

fn attach(proc_dir: PathBuf, exe: &Path) -> Result<Session, String> {
    let mem = File::open(proc_dir.join("mem")).map_err(|error| {
        format!("Cannot read osu!lazer memory ({error}). Start the shell with --launch-lazer to launch osu!lazer as its child.")
    })?;
    let memory = ProcessMemory { mem, proc_dir };
    let files = RealFiles;
    let info = target_from_files(&files, &TargetEnv {
        exe_dir: exe.parent().ok_or("Missing osu!lazer executable directory")?.to_path_buf(),
        storage_ini: super::storage_ini_path(),
        arch: "x64".to_string(),
    }).with_storage_root_override(crate::config::lazer_storage_root().as_deref());
    if info.lazer_version.is_empty() || info.runtime_version.is_empty() {
        return Err("Cannot identify the installed osu!lazer and .NET versions".to_string());
    }
    let loaded = load_table(&files, env_table_path().as_deref(), &shell_exe_dir(), &info, &|_| false)
        .map_err(|_| format!("No matching offset table for osu!lazer {} / .NET {}. Check Remote Updates in Shell Configuration.", info.lazer_version, info.runtime_version))?;
    if loaded.mismatch.is_some() || loaded.table.score().is_none() {
        return Err(format!("LaneAcc needs a matching score offset table for {} / {}", info.lazer_version, info.runtime_version));
    }
    let table_path = loaded.path;
    let table = loaded.table;
    let fields = Resolved::from_table(&table);
    let desktop = DiskPe::open(&exe.with_file_name("osu!.dll"));
    let (game_base, game_type, screen_module) = locate_game(&memory, &table, &fields, desktop.as_ref())
        .ok_or("Cannot resolve the osu!lazer game object and screen stack")?;
    Ok(Session { memory, table, fields, game_base, game_type, screen_module, table_path,
        default_files: info.effective_songs_folder().map(PathBuf::from),
        storage_root: info.storage_root.map(PathBuf::from) })
}

fn locate_game(memory: &ProcessMemory, table: &OffsetTable, fields: &Resolved, desktop: Option<&DiskPe>) -> Option<(u64, u64, u64)> {
    let maps = fs::read_to_string(memory.proc_dir.join("maps")).ok()?;
    let pattern = crate::osu::scan::Pattern::parse(MARKER_PATTERN).ok()?;
    let mut slab = vec![0; SCAN_CHUNK];
    for line in maps.lines() {
        let mut parts = line.split_whitespace();
        let Some((start, end)) = parts.next().and_then(|range| range.split_once('-')) else { continue };
        if parts.next() != Some("rw-p") || parts.nth(3).is_some_and(|name| !name.starts_with('[')) {
            continue;
        }
        let (Ok(mut address), Ok(end)) = (u64::from_str_radix(start, 16), u64::from_str_radix(end, 16)) else { continue };
        while end.saturating_sub(address) >= 12 {
            let len = (end - address).min(SCAN_CHUNK as u64) as usize;
            if memory.mem.read_exact_at(&mut slab[..len], address).is_ok() {
                for index in (0..=len - 12).step_by(4) {
                    if slab[index] != 1 || !pattern.matches_at(&slab[..len], index) {
                        continue;
                    }
                    let anchor = address + index as u64;
                    for &delta in SITE_DELTAS {
                        let Some(base) = try_site(memory, anchor, delta).game_base else { continue };
                        if let Some((game_type, screen_module)) = prove_game(memory, table, fields, base, desktop) {
                            return Some((base, game_type, screen_module));
                        }
                    }
                }
            }
            if address + len as u64 == end {
                break;
            }
            address += (len - 8) as u64;
        }
    }
    None
}

fn prove_game(memory: &ProcessMemory, table: &OffsetTable, fields: &Resolved, base: u64, desktop: Option<&DiskPe>) -> Option<(u64, u64)> {
    let game = read_eetype_identity(memory, table, base).ok()?;
    let name = table.runtime_type_name("osu!.dll", game.rid).map(str::to_owned)
        .or_else(|| read_pe_typedef_name(desktop?, 0, game.rid));
    if name.as_deref() != Some("osu.Desktop.OsuGameDesktop") {
        return None;
    }
    table_probe(memory, table, base).ok()?;
    let mut chain = ChainAddrs::default();
    let top = read_screen_top(memory, table, base, fields, &mut chain).ok()?;
    let stack = read_eetype_identity(memory, table, chain.screen_stack?).ok()?;
    let screen = read_eetype_identity(memory, table, top).ok()?;
    // OsuScreenStack and Player are defined in the same managed assembly.
    // Linux's loader Module does not have the Windows PE image base layout.
    if table.runtime_type_name(GAME_ASSEMBLY, stack.rid) != Some("osu.Game.Screens.OsuScreenStack")
        || stack.module != screen.module
        || table.runtime_type_name(GAME_ASSEMBLY, screen.rid).is_none()
    {
        return None;
    }
    Some((game.method_table, stack.module))
}

impl Session {
    fn player(&self) -> Option<u64> {
        let mut chain = ChainAddrs::default();
        let top = read_screen_top(&self.memory, &self.table, self.game_base, &self.fields, &mut chain).ok()?;
        let screen = read_eetype_identity(&self.memory, &self.table, top).ok()?;
        if screen.module != self.screen_module {
            return None;
        }
        let name = self.table.runtime_type_name(GAME_ASSEMBLY, screen.rid)?;
        (self.table.screen_state_for(name).or_else(|| screen_state_for(name)) == Some("play")).then_some(top)
    }
}

// Identify native mania from the game's current content-addressed chart, before
// either poller interprets a hit object or a replay action as a mania column.
impl Session {
    fn beatmap_info(&self) -> Option<u64> {
        let f = &self.fields;
        let mut object = self.game_base;
        for field in [&f.beatmap, &f.bindable_value, &f.working_beatmap_info] {
            object = super::read_ptr_field(&self.memory, object, *field.as_ref().ok()?)?;
        }
        Some(object)
    }

    fn chart_path(&self, info: u64) -> Option<PathBuf> {
        use super::{read_ptr_field, read_string, to_lazer_path};
        let f = &self.fields;
        let ptr = |base, field: &Result<i64, crate::osu::offsets::LookupError>|
            read_ptr_field(&self.memory, base, *field.as_ref().ok()?);
        let layout = f.string_layout().ok()?;
        let hash = read_string(&self.memory, ptr(info, &f.beatmap_hash)?, layout)?;
        let rel = to_lazer_path(&hash)?.replace('\\', "/");
        let live_root = || {
            let storage = ptr(self.game_base, &f.storage)?;
            read_string(&self.memory, ptr(storage, &f.base_path)?, layout).map(PathBuf::from)
        };
        if let Some(path) = self.storage_root.as_ref().map(|root| root.join("files").join(&rel)).filter(|p| p.is_file()) {
            return Some(path);
        }
        live_root().map(|p| p.join("files")).into_iter().chain(self.default_files.clone())
            .map(|dir| dir.join(&rel)).find(|p| p.is_file())
    }
}

fn native_mania_chart(path: &Path) -> std::io::Result<bool> {
    use std::io::{BufRead, BufReader};
    let mut general = String::new();
    for line in BufReader::new(File::open(path)?).lines() {
        let line = line?;
        let text = line.trim().trim_start_matches('\u{feff}');
        if text.starts_with('[') {
            if !general.is_empty() { break; }
            if text != "[General]" { continue; }
        }
        if text == "[General]" || !general.is_empty() {
            general.push_str(text);
            general.push('\n');
        }
    }
    Ok(crate::osu::beatmap_file::parse(&general).is_mania())
}

fn launch_game() -> Result<Child, String> {
    if find_game()?.is_some() {
        return Err("osu!lazer is already running. Close it and run the launcher again.".into());
    }
    let exe = std::env::var_os("MMA_LAZER_EXECUTABLE").unwrap_or_else(|| "osu-lazer".into());
    Command::new(&exe).spawn().map_err(|e| format!("Cannot start {}: {e}. Set MMA_LAZER_EXECUTABLE to the game executable.", Path::new(&exe).display()))
}

pub(crate) fn run(reader: Reader, hits: Arc<HitEventLog>, replays: Arc<ReplayFrameLog>) {
    let mut child = None;
    let mut launch_error = None;
    if std::env::args_os().any(|arg| arg == "--launch-lazer") {
        match launch_game() {
            Ok(game) => child = Some(game),
            Err(error) => {
                crate::server::log::log_at("warn", &error);
                launch_error = Some(error);
            }
        }
    }
    let mut last_error = None;
    loop {
        if let Some(status) = child.as_mut().and_then(|c| c.try_wait().ok().flatten()) {
            if !status.success() {
                launch_error = Some(format!("osu!lazer launcher exited with {status}. Check MMA_LAZER_EXECUTABLE and run the game executable directly."));
            }
            child = None;
        }
        let session = match find_game().and_then(|game| game.map(|(dir, exe)| attach(dir, &exe)).transpose()) {
            Ok(Some(session)) => session,
            Ok(None) => {
                reader.inner.lock().unwrap().reason = Some(launch_error.as_deref().unwrap_or("Waiting for osu!lazer.").into());
                last_error = None;
                thread::sleep(ATTACH_RETRY);
                continue;
            }
            Err(error) => {
                if last_error.as_ref() != Some(&error) {
                    crate::server::log::log_at("warn", &format!("[osu] Linux LaneAcc: {error}"));
                    reader.inner.lock().unwrap().reason = Some(error.clone());
                    last_error = Some(error);
                }
                thread::sleep(SCAN_RETRY);
                continue;
            }
        };
        launch_error = None;
        last_error = None;
        crate::server::log::log_at("info", &format!("[osu] Linux LaneAcc attached ({})", session.table.key()));
        let section = session.table.score().expect("attach requires score offsets");
        let mut hit_state = PollState::default();
        let mut replay_state = ReplayPollState::default();
        {
            let mut state = reader.inner.lock().unwrap();
            state.reason = None;
            state.lazer_lane_acc_ready = true;
            state.lazer = Some(super::Diagnostics {
                table_key: Some(session.table.key()),
                table_path: Some(session.table_path.display().to_string()),
                active_table: Some((&session.table).into()),
                ..Default::default()
            });
        }
        let mut chart_mode = None;
        while read_u64(&session.memory, session.game_base) == Some(session.game_type) {
            let mut notice = None;
            if let Some(player) = session.player() {
                // Cache the mode for this player and beatmap. Never carry that decision
                // across retries, map changes, or a return to the menu.
                let key = session.beatmap_info().map(|info| (player, info));
                if key != chart_mode.map(|(k, _)| k) {
                    chart_mode = key.and_then(|k| {
                        let mode = native_mania_chart(&session.chart_path(k.1)?).ok()?;
                        Some((k, mode))
                    });
                }
                match chart_mode {
                    Some((_, true)) => {
                        let _ = score::poll(&session.memory, section, player, &hits, &mut hit_state);
                        let _ = score::poll_replay(&session.memory, section, player, &replays, &mut replay_state);
                    }
                    Some((_, false)) => notice = Some("LaneAcc supports native mania maps only."),
                    None => notice = Some("Cannot read the active chart. Check the lazer storage folder in Shell Configuration."),
                }
            } else {
                chart_mode = None;
            }
            let mut state = reader.inner.lock().unwrap();
            if state.reason.as_deref() != notice { state.reason = notice.map(str::to_owned); }
            drop(state);
            thread::sleep(POLL_INTERVAL);
        }
        {
            let mut state = reader.inner.lock().unwrap();
            state.lazer_lane_acc_ready = false;
            state.lazer = None;
            state.reason = None;
        }
        hits.clear();
        replays.clear();
    }
}
