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
        storage_ini: None,
        arch: "x64".to_string(),
    });
    if info.lazer_version.is_empty() || info.runtime_version.is_empty() {
        return Err("Cannot identify the installed osu!lazer and .NET versions".to_string());
    }
    let loaded = load_table(&files, env_table_path().as_deref(), &shell_exe_dir(), &info, &|_| false)
        .map_err(|reason| reason.as_str().to_string())?;
    if loaded.mismatch.is_some() || loaded.table.score().is_none() {
        return Err(format!("LaneAcc needs a matching score offset table for {} / {}", info.lazer_version, info.runtime_version));
    }
    let table = loaded.table;
    let fields = Resolved::from_table(&table);
    let desktop = DiskPe::open(&exe.with_file_name("osu!.dll"));
    let (game_base, game_type, screen_module) = locate_game(&memory, &table, &fields, desktop.as_ref())
        .ok_or("Cannot resolve the osu!lazer game object and screen stack")?;
    Ok(Session { memory, table, fields, game_base, game_type, screen_module })
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

pub(crate) fn run(reader: Reader, hits: Arc<HitEventLog>, replays: Arc<ReplayFrameLog>) {
    let mut game_child: Option<Child> = None;
    if std::env::args_os().any(|arg| arg == "--launch-lazer") {
        match Command::new("osu-lazer").spawn() {
            Ok(child) => game_child = Some(child),
            Err(error) => crate::server::log::log_at("warn", &format!("[osu] Cannot launch osu-lazer: {error}")),
        }
    }
    let mut last_error = None;
    loop {
        if game_child.as_mut().is_some_and(|child| child.try_wait().ok().flatten().is_some()) {
            game_child = None;
        }
        let session = match find_game().and_then(|game| game.map(|(dir, exe)| attach(dir, &exe)).transpose()) {
            Ok(Some(session)) => session,
            Ok(None) => {
                last_error = None;
                thread::sleep(ATTACH_RETRY);
                continue;
            }
            Err(error) => {
                if last_error.as_ref() != Some(&error) {
                    crate::server::log::log_at("warn", &format!("[osu] Linux LaneAcc: {error}"));
                    last_error = Some(error);
                }
                thread::sleep(SCAN_RETRY);
                continue;
            }
        };
        last_error = None;
        crate::server::log::log_at("info", &format!("[osu] Linux LaneAcc attached ({})", session.table.key()));
        let section = session.table.score().expect("attach requires score offsets");
        let mut hit_state = PollState::default();
        let mut replay_state = ReplayPollState::default();
        reader.inner.lock().unwrap().lazer_lane_acc_ready = true;
        while read_u64(&session.memory, session.game_base) == Some(session.game_type) {
            if let Some(player) = session.player() {
                let _ = score::poll(&session.memory, section, player, &hits, &mut hit_state);
                let _ = score::poll_replay(&session.memory, section, player, &replays, &mut replay_state);
            }
            thread::sleep(POLL_INTERVAL);
        }
        reader.inner.lock().unwrap().lazer_lane_acc_ready = false;
        hits.clear();
        replays.clear();
    }
}
