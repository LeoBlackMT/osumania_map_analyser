use crate::osu::model::{Client, Reason, Snapshot};
use crate::osu::offsets::{LookupError, OffsetTable};
use super::fields::*;
use super::resolve::*;
use super::screen::{read_screen_state, runtime_probe, ScreenOutcome};
use super::source::*;
use super::table::*;
use super::types::*;
use std::path::PathBuf;

/// 降级标记：`<载荷字段>:<原因>`。
pub fn gap(field: &str, why: &str) -> String {
    format!("{field}:{why}")
}

/// 查表失败的降级标记。
pub fn offsets_gap(field: &str, error: &LookupError) -> String {
    gap(field, &format!("offsets-{error}"))
}

/// 每帧的 L1 结构证明：(b) `[gameBase]` 对齐且可读 → (c) 会话内 MT 稳定。
pub fn prove_object(
    source: &dyn Source,
    table: &OffsetTable,
    game_base: u64,
    anchors: &[u64],
    proof: &mut SessionProof,
) -> Result<(u64, u64, Option<VtableChange>), Reason> {
    let observed = read_u64(source, game_base);
    if let Some(method_table) = observed.filter(|value| method_table_plausible(source, *value)) {
        match proof.observe(method_table) {
            ProofStep::First | ProofStep::Stable => return Ok((game_base, method_table, None)),
            ProofStep::Changed { previous } => eprintln!(
                "[osu] lazer: L1 object re-identified — MethodTable at gameBase=0x{game_base:016X} \
                 changed 0x{previous:016X} -> 0x{method_table:016X}; re-resolving GameBase from {} \
                 anchor(s)",
                anchors.len()
            ),
        }
    } else {
        eprintln!(
            "[osu] lazer: L1 object re-identified — `[gameBase]` at 0x{game_base:016X} is {}; \
             re-resolving GameBase from {} anchor(s)",
            observed
                .map(|value| format!("0x{value:016X} (not an aligned+readable MethodTable)"))
                .unwrap_or_else(|| "unreadable".to_string()),
            anchors.len()
        );
    }

    let previous = proof.session_vtable();
    let resolution = resolve_game_base(source, anchors, SITE_DELTAS);
    log_resolution(&resolution);
    let Some(candidate) = resolution.accepted() else {
        eprintln!(
            "[osu] lazer: L1 re-identification failed — no GameBase candidate for {} anchor(s) \
             (signature-miss:{ANCHOR_KEY})",
            anchors.len()
        );
        return Err(Reason::SignatureMiss(ANCHOR_KEY));
    };
    let (new_base, new_vtable) = (
        candidate.game_base.unwrap_or(0),
        candidate.method_table.unwrap_or(0),
    );
    proof.adopt(new_vtable);
    eprintln!(
        "[osu] lazer: L1 re-identified object — gameBase=0x{new_base:016X} delta={:#x} \
         [gameBase]=0x{new_vtable:016X} (table={})",
        candidate.delta,
        table.key()
    );
    Ok((
        new_base,
        new_vtable,
        Some(VtableChange { previous, observed }),
    ))
}

/// 单次采样：(a)+(b)+(c) 的 L1 结构证明 → 解引用链 → §3.3 的 lazer 侧字段。
pub fn read_frame(input: FrameInput<'_>) -> Result<LazerFrame, Reason> {
    let FrameInput {
        table,
        source,
        pid,
        game_base,
        anchors,
        proof,
        songs_folder,
        game_folder,
        modules,
    } = input;
    let resolved = Resolved::from_table(table);
    let mut gaps: Vec<String> = Vec::new();
    let mut snapshot = Snapshot {
        client: Some(Client::Lazer),
        pid,
        ..Default::default()
    };

    // ① L1 结构证明
    let (game_base, vtable, reidentified) = prove_object(source, table, game_base, anchors, proof)?;
    let mut chain = ChainAddrs {
        game_base,
        vtable,
        ..Default::default()
    };

    // ② 字符串布局
    let layout = match resolved.string_layout() {
        Ok(layout) => Some(layout),
        Err(error) => {
            gaps.push(gap("strings", &format!("offsets-{error}")));
            None
        }
    };

    // ③ 存储链
    if let Ok(offset) = resolved.storage {
        if let Some(storage) = read_ptr_field(source, game_base, offset) {
            chain.storage = Some(storage);
            if let Ok(base_path_offset) = resolved.base_path {
                chain.base_path = read_ptr_field(source, storage, base_path_offset);
            }
        }
    }
    let memory_songs = chain
        .base_path
        .and_then(|pointer| layout.and_then(|layout| read_string(source, pointer, layout)))
        .map(|root| format!("{root}\\{}", FILES_DIR));
    snapshot.folder = Some(FOLDER_DOT.to_string());
    snapshot.songs_folder = match (&songs_folder, &memory_songs) {
        (Some(ini), Some(memory)) if ini != memory => {
            eprintln!(
                "[osu] lazer: folders.songs mismatch — storage.ini says '{ini}', memory <BasePath> says '{memory}' (using storage.ini per P7)"
            );
            Some(ini.clone())
        }
        (Some(ini), _) => Some(ini.clone()),
        (None, Some(memory)) => Some(memory.clone()),
        (None, None) => {
            gaps.push(gap("folders.songs", "offsets-missing-BasePath"));
            None
        }
    };
    snapshot.game_folder = game_folder;

    // ④ 谱面链
    let mut beatmap_info = None;
    match (
        resolved.beatmap.clone(),
        resolved.bindable_value.clone(),
        resolved.working_beatmap_info.clone(),
    ) {
        (Ok(beatmap), Ok(value), Ok(info)) => {
            chain.beatmap_bindable = read_ptr_field(source, game_base, beatmap);
            chain.working_beatmap = chain
                .beatmap_bindable
                .and_then(|bindable| read_ptr_field(source, bindable, value));
            chain.beatmap_info = chain
                .working_beatmap
                .and_then(|working| read_ptr_field(source, working, info));
            beatmap_info = chain.beatmap_info;
            if beatmap_info.is_none() {
                gaps.push(GAP_NO_BEATMAP.to_string());
            }
        }
        (beatmap, value, info) => {
            for (offset, field) in [
                (beatmap, "beatmap.object"),
                (value, "beatmap.bindable.value"),
                (info, "beatmap.working"),
            ] {
                if let Err(error) = offset {
                    gaps.push(offsets_gap(field, &error));
                }
            }
        }
    }

    // ⑤ identity / 元数据
    if let Some(info) = beatmap_info {
        if let Some(md5) = read_text_field(source, info, &resolved.beatmap_md5, "beatmap.md5", layout, &mut gaps) {
            if is_md5_hex(&md5) {
                snapshot.checksum = Some(md5);
            } else {
                gaps.push(gap("beatmap.md5", "shape"));
            }
        }
        if let Some(hash) = read_text_field(source, info, &resolved.beatmap_hash, "files.beatmap", layout, &mut gaps) {
            match to_lazer_path(&hash) {
                Some(path) => snapshot.filename = Some(path),
                None => gaps.push(gap("files.beatmap", "shape")),
            }
        }
        snapshot.map_id = field_i32(source, info, &resolved.beatmap_online_id, "beatmap.id", &mut gaps);
        snapshot.version = read_text_field(
            source,
            info,
            &resolved.beatmap_difficulty_name,
            "beatmap.version",
            layout,
            &mut gaps,
        );
        chain.metadata = resolved
            .beatmap_metadata
            .as_ref()
            .ok()
            .and_then(|offset| read_ptr_field(source, info, *offset));
        chain.beatmap_set = resolved
            .beatmap_set
            .as_ref()
            .ok()
            .and_then(|offset| read_ptr_field(source, info, *offset));
        if let Some(set) = chain.beatmap_set {
            snapshot.set_id = field_i32(source, set, &resolved.set_online_id, "beatmap.set", &mut gaps);
            if let Some(layout) = layout {
                snapshot.lazer_files = read_beatmap_set_files(source, set, layout);
            }
        } else if let Err(error) = &resolved.beatmap_set {
            gaps.push(offsets_gap("beatmap.set", error));
        } else {
            gaps.push(gap("beatmap.set", "read"));
        }
        let mut bg_name_from_meta = None;
        if let Some(metadata) = chain.metadata {
            snapshot.title = read_text_field(source, metadata, &resolved.metadata_title, "beatmap.title", layout, &mut gaps);
            snapshot.artist =
                read_text_field(source, metadata, &resolved.metadata_artist, "beatmap.artist", layout, &mut gaps);
            if let Some(layout) = layout {
                bg_name_from_meta = read_ptr_field(source, metadata, 96)
                    .and_then(|ptr| read_string(source, ptr, layout));
            }
            chain.realm_user = resolved
                .metadata_author
                .as_ref()
                .ok()
                .and_then(|offset| read_ptr_field(source, metadata, *offset));
            if let Some(user) = chain.realm_user {
                snapshot.mapper =
                    read_text_field(source, user, &resolved.realm_username, "beatmap.mapper", layout, &mut gaps);
            } else if let Err(error) = &resolved.metadata_author {
                gaps.push(offsets_gap("beatmap.mapper", error));
            } else {
                gaps.push(gap("beatmap.mapper", "read"));
            }
        } else if let Err(error) = &resolved.beatmap_metadata {
            gaps.push(offsets_gap("beatmap.title", error));
            gaps.push(offsets_gap("beatmap.artist", error));
            gaps.push(offsets_gap("beatmap.mapper", error));
        } else {
            gaps.push(gap("beatmap.title", "read"));
            gaps.push(gap("beatmap.artist", "read"));
            gaps.push(gap("beatmap.mapper", "read"));
        }
        if !snapshot.lazer_files.is_empty() {
            if let Some(bg_path) = find_background_file(&snapshot.lazer_files, bg_name_from_meta.as_deref()) {
                snapshot.background = Some(bg_path);
            }
        }
    }

    // ⑥ 屏幕栈
    match read_screen_state(source, table, game_base, &resolved, &mut chain, modules) {
        ScreenOutcome::Mapped(state) => {
            snapshot.state_number = Some(state.number);
            snapshot.state_name = Some(state.name.clone());
        }
        ScreenOutcome::Unmapped(type_name) => {
            snapshot.state_name = Some(String::new());
            gaps.push(gap(
                "state.name",
                &format!("{type_name}-{UNMAPPED_SCREEN_SUFFIX}"),
            ));
            gaps.push(gap("state.number", "screen-type-unmapped"));
        }
        ScreenOutcome::Unresolved(reason) => {
            gaps.push(gap("state.name", &reason));
            gaps.push(gap("state.number", &reason));
        }
    }
    match resolved.selected_mods.clone() {
        Ok(offset) => {
            chain.selected_mods = read_ptr_field(source, game_base, offset);
            snapshot.lazer_mods = None;
            if chain.selected_mods.is_none() {
                gaps.push(gap("play.mods", "read"));
                gaps.push(gap("menu.mods", "read"));
                gaps.push(gap("resultsScreen.mods", "read"));
            } else {
                for field in ["play.mods", "menu.mods", "resultsScreen.mods"] {
                    gaps.push(gap(field, "offsets-missing-ScoreInfo.ModsJson"));
                }
            }
        }
        Err(error) => {
            gaps.push(offsets_gap("play.mods", &error));
            gaps.push(offsets_gap("menu.mods", &error));
            gaps.push(offsets_gap("resultsScreen.mods", &error));
        }
    }
    // ⑦ 播放时钟
    snapshot.play_time = read_live_time(source, game_base, &resolved, &mut chain, &mut gaps);
    gaps.push(GAP_PLAY_HITS.to_string());
    gaps.push(GAP_RESULTS_HITS.to_string());
    gaps.push(GAP_BACKGROUND.to_string());
    gaps.push(GAP_AUDIO.to_string());

    snapshot.lazer_chain = Some(chain);
    gaps.sort();
    gaps.dedup();
    snapshot.degraded_fields = gaps.clone();
    Ok(LazerFrame {
        snapshot,
        chain,
        gaps,
        reidentified,
    })
}

fn read_text_field(
    source: &dyn Source,
    base: u64,
    offset: &Result<i64, LookupError>,
    field: &str,
    layout: Option<StringLayout>,
    gaps: &mut Vec<String>,
) -> Option<String> {
    let layout = layout?;
    let offset = match offset {
        Ok(offset) => *offset,
        Err(error) => {
            gaps.push(offsets_gap(field, error));
            return None;
        }
    };
    let Some(pointer) = read_ptr_field(source, base, offset) else {
        gaps.push(gap(field, "read"));
        return None;
    };
    match read_string(source, pointer, layout) {
        Some(text) => Some(text),
        None => {
            gaps.push(gap(field, "string-layout"));
            None
        }
    }
}

fn read_live_time(
    source: &dyn Source,
    game_base: u64,
    resolved: &Resolved,
    chain: &mut ChainAddrs,
    gaps: &mut Vec<String>,
) -> Option<i32> {
    const FIELD: &str = "beatmap.time.live";
    let mut offsets = [0i64; 3];
    for (index, lookup) in [
        resolved.beatmap_clock.clone(),
        resolved.beatmap_track_clock.clone(),
        resolved.beatmap_clock_time.clone(),
    ]
    .into_iter()
    .enumerate()
    {
        match lookup {
            Ok(offset) => offsets[index] = offset,
            Err(error) => {
                gaps.push(offsets_gap(FIELD, &error));
                return None;
            }
        }
    }
    let (clock_offset, track_offset, time_offset) = (offsets[0], offsets[1], offsets[2]);
    chain.beatmap_clock = read_ptr_field(source, game_base, clock_offset);
    let Some(clock) = chain.beatmap_clock else {
        gaps.push(gap(FIELD, "read-beatmapClock"));
        return None;
    };
    chain.beatmap_track_clock = read_ptr_field(source, clock, track_offset);
    let Some(track) = chain.beatmap_track_clock else {
        gaps.push(gap(FIELD, "read-interpolatedTrack"));
        return None;
    };
    let Some(address) = field_addr(track, time_offset) else {
        gaps.push(gap(FIELD, "read"));
        return None;
    };
    match read_f64(source, address) {
        Some(value) if value.is_finite() && value.abs() <= LIVE_TIME_MAX_MS => {
            Some(value.round() as i32)
        }
        _ => {
            gaps.push(gap(FIELD, "domain"));
            None
        }
    }
}

fn field_i32(
    source: &dyn Source,
    base: u64,
    offset: &Result<i64, LookupError>,
    field: &str,
    gaps: &mut Vec<String>,
) -> Option<i32> {
    let offset = match offset {
        Ok(offset) => *offset,
        Err(error) => {
            gaps.push(offsets_gap(field, error));
            return None;
        }
    };
    match field_addr(base, offset).and_then(|addr| read_i32(source, addr)) {
        Some(value) => Some(value),
        None => {
            gaps.push(gap(field, "read"));
            None
        }
    }
}

#[cfg(windows)]
pub fn scan_markers(
    target: &crate::osu::win::Target,
    regions: &mut crate::osu::scan::RegionCache,
    limit: usize,
) -> Result<(Vec<u64>, u128), Reason> {
    use crate::osu::scan;
    let pattern = scan::Pattern::parse(MARKER_PATTERN).map_err(|_| Reason::SignatureMiss(ANCHOR_KEY))?;
    let started = std::time::Instant::now();
    let mut stats = scan::ScanStats::default();
    for mask in crate::osu::FILTER_READY {
        let list = target.regions_cached(regions, *mask, crate::osu::REGION_LIMIT);
        let hits = scan::find_in_regions64(target.handle(), &list, &pattern, 0, limit, &mut stats);
        if !hits.is_empty() {
            eprintln!(
                "[osu] lazer scan: filter#{} regions={} bytes={} hits={} elapsed={}ms",
                mask,
                list.len(),
                stats.bytes_read,
                hits.len(),
                started.elapsed().as_millis()
            );
            return Ok((hits, started.elapsed().as_millis()));
        }
    }
    Err(Reason::SignatureMiss(ANCHOR_KEY))
}

#[cfg(windows)]
pub fn attach(
    target: &crate::osu::win::Target,
    regions: &mut crate::osu::scan::RegionCache,
    env_path: Option<PathBuf>,
) -> Result<Attach, Reason> {
    let env = TargetEnv {
        exe_dir: target
            .image_path
            .parent()
            .map(|dir| dir.to_path_buf())
            .unwrap_or_default(),
        storage_ini: storage_ini_path(),
        arch: arch_for_bitness(target.bitness).to_string(),
    };
    let files = RealFiles;
    let info = target_from_files(&files, &env);
    let table_dir = shell_exe_dir();
    eprintln!(
        "[osu] lazer target: version={:?} runtime={:?} arch={} storage_root={:?} table_dir={}",
        info.lazer_version,
        info.runtime_version,
        info.arch,
        info.storage_root,
        table_dir.display()
    );
    let source: &dyn Source = target;
    let (anchors, scan_ms) = scan_markers(target, regions, MARKER_HIT_LIMIT)?;
    let resolution = resolve_game_base(source, &anchors, SITE_DELTAS);
    log_resolution(&resolution);
    let modules = match crate::osu::win::module_list(target.pid) {
        Ok(list) => {
            eprintln!(
                "[osu] lazer modules: {} module(s) enumerated for pid {} (state.name resolves the \
                 assembly by image base)",
                list.len(),
                target.pid
            );
            list
        }
        Err(reason) => {
            eprintln!(
                "[osu] lazer modules: enumeration failed ({reason:?}) — state.name will degrade to \
                 `module-unresolved:…` until the module table is readable"
            );
            Vec::new()
        }
    };
    let prove = |table: &OffsetTable| -> bool {
        let Some(candidate) = resolution.accepted() else {
            eprintln!(
                "[osu] lazer offsets: structural proof for {} refused — no GameBase candidate \
                 resolved from {} anchor(s)",
                table.key(),
                anchors.len()
            );
            return false;
        };
        let game_base = candidate.game_base.unwrap_or(0);
        match table_probe(source, table, game_base) {
            Ok(detail) => eprintln!(
                "[osu] lazer offsets: structural proof for {} passed — {detail}",
                table.key()
            ),
            Err(why) => {
                eprintln!(
                    "[osu] lazer offsets: structural proof for {} refused — {why}",
                    table.key()
                );
                return false;
            }
        }
        match runtime_probe(source, table, game_base, &modules) {
            Ok(detail) => {
                eprintln!(
                    "[osu] lazer offsets: runtime structure proof for {} passed — {detail}",
                    table.key()
                );
                true
            }
            Err(why) => {
                eprintln!(
                    "[osu] lazer offsets: runtime structure proof for {} refused — {why}",
                    table.key()
                );
                false
            }
        }
    };
    let loaded = load_table(&files, env_path.as_deref(), &table_dir, &info, &prove)?;
    let Some(candidate) = resolution.accepted() else {
        eprintln!(
            "[osu] lazer: L1 resolution failed — no GameBase candidate for {} anchor(s); \
              detaching (signature-miss:{ANCHOR_KEY})",
            anchors.len()
        );
        return Err(Reason::SignatureMiss(ANCHOR_KEY));
    };
    let game_base = candidate.game_base.unwrap_or(0);
    let vtable = candidate.method_table.unwrap_or(0);
    eprintln!("[osu] lazer: {}", vtable_witness_note(&loaded.table, vtable));
    if let Some(runtime) = loaded.table.runtime() {
        eprintln!(
            "[osu] lazer: runtime section present ({} typedef name(s) over {} module(s), \
             {} observed): {}",
            runtime.typedefs.values().map(|map| map.len()).sum::<usize>(),
            runtime.typedefs.len(),
            runtime.observed.values().map(|map| map.len()).sum::<usize>(),
            runtime.witness
        );
    } else {
        eprintln!(
            "[osu] lazer: the table carries NO runtime section — state.name/state.number will \
             degrade to field level"
        );
    }
    eprintln!(
        "[osu] lazer: L1 proof passed (structural) — gameBase=0x{game_base:016X} delta={:#x} \
         site=0x{:016X} [gameBase]=0x{vtable:016X} anchors={} scan_ms={scan_ms} table={} origin={}",
        candidate.delta,
        candidate.site.unwrap_or(0),
        anchors.len(),
        loaded.table.key(),
        loaded.origin.as_str()
    );
    let marker_hits = anchors.len();
    Ok(Attach {
        table: loaded.table,
        origin: loaded.origin,
        table_path: loaded.path,
        table_mismatch: loaded.mismatch,
        target: info,
        game_base,
        anchors,
        proof: SessionProof::default(),
        modules,
        marker_hits,
        scan_ms,
    })
}

#[cfg(windows)]
pub fn read_tick(
    attach: &mut Attach,
    target: &crate::osu::win::Target,
) -> Result<LazerFrame, Reason> {
    let source: &dyn Source = target;
    if attach.modules.is_empty() {
        attach.modules = crate::osu::win::module_list(target.pid).unwrap_or_default();
        eprintln!(
            "[osu] lazer modules: (re-)enumerated {} module(s) — state.name needs the module table",
            attach.modules.len()
        );
    }
    let mut frame = read_frame(FrameInput {
        table: &attach.table,
        source,
        pid: target.pid,
        game_base: attach.game_base,
        anchors: &attach.anchors,
        proof: &mut attach.proof,
        songs_folder: attach.target.songs_folder(),
        game_folder: crate::osu::stable::game_folder(&target.image_path),
        modules: &attach.modules,
    })?;
    if frame
        .gaps
        .iter()
        .any(|gap| gap.contains("module-unresolved"))
    {
        let refreshed = crate::osu::win::module_list(target.pid).unwrap_or_default();
        if refreshed.len() != attach.modules.len() {
            eprintln!(
                "[osu] lazer modules: module table changed ({} -> {}) — re-reading this frame",
                attach.modules.len(),
                refreshed.len()
            );
            attach.modules = refreshed;
            frame = read_frame(FrameInput {
                table: &attach.table,
                source,
                pid: target.pid,
                game_base: attach.game_base,
                anchors: &attach.anchors,
                proof: &mut attach.proof,
                songs_folder: attach.target.songs_folder(),
                game_folder: crate::osu::stable::game_folder(&target.image_path),
                modules: &attach.modules,
            })?;
        }
    }
    attach.game_base = frame.chain.game_base;
    Ok(frame)
}

#[cfg(not(windows))]
pub fn attach(
    _target: &crate::osu::win::Target,
    _regions: &mut crate::osu::scan::RegionCache,
    _env_path: Option<PathBuf>,
) -> Result<Attach, Reason> {
    Err(Reason::PlatformUnsupported)
}

#[cfg(not(windows))]
pub fn read_tick(
    _attach: &mut Attach,
    _target: &crate::osu::win::Target,
) -> Result<LazerFrame, Reason> {
    Err(Reason::PlatformUnsupported)
}
