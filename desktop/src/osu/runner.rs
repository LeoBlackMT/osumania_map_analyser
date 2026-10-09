// osu! 引擎后台运行与轮询采样主循环
//
// 负责：
// - run(): 发现 -> 附着 -> 定址 -> 每 tick 读 + 过门 + 发布
// - L0 锚点定址 (resolve_anchors / cached_anchors)
// - stable 帧采样 (read_frame)
// - .osu 谱面文件解析与缓存 (attach_beatmap_file)
// - 状态发布辅助 (publish_phase / publish_outcome / publish_idle 等)

use std::sync::Arc;
use std::thread;
use crate::osu::invariants::{FrameAction, Gate};
use crate::osu::model::{Client, Reason, Snapshot};
use super::*;

/// 主循环：发现 → 附着 → 定址 → 每 tick 读 + 过门 + 发布。
#[cfg(windows)]
pub fn run(reader: Reader) {
    let mut attached: Option<win::Target> = None;
    let mut anchors: Option<patterns::AnchorTable> = None;
    let mut stable_table: Option<offsets::StableTable> = None;
    let mut lazer_attach: Option<lazer::Attach> = None;
    let mut regions: scan::RegionCache = scan::RegionCache::new();
    let mut anchor_cache: anchor_cache::AnchorCache = anchor_cache::AnchorCache::new();
    let mut last_scan_fail: Option<std::time::Instant> = None;
    let mut scan_fail_attempt: usize = 0;
    let mut lost_at: Option<std::time::Instant> = None;
    let mut attach_failures: u32 = 0;
    let mut gate = Gate::new();
    let mut previous_live: Option<i32> = None;

    let compare_on = compare::enabled();
    let feed = if compare_on {
        let feed = Arc::new(compare::TosuFeed::default());
        compare::spawn_tosu_client(feed.clone());
        Some(feed)
    } else {
        None
    };
    let mut writer = compare_on.then(|| compare::SampleWriter::new(compare::samples_path()));
    let compare_interval = if compare_on {
        compare::interval()
    } else {
        TICK
    };
    if compare_on {
        eprintln!(
            "[osu] compare: interval={}ms out={}",
            compare_interval.as_millis(),
            compare::samples_path().display()
        );
    }
    let mut osu_cache: Option<OsuFileCache> = None;

    loop {
        let now_ms = crate::server::now_ms();
        if gate.should_re_resolve(now_ms) && (anchors.is_some() || lazer_attach.is_some()) {
            eprintln!(
                "[osu] gate: freeze window expired ({}) — forcing anchor re-resolution (reason={:?})",
                gate.re_resolve_due_in_ms(now_ms).unwrap_or(0),
                gate.reason()
            );
            anchors = None;
            stable_table = None;
            lazer_attach = None;
            osu_cache = None;
            last_scan_fail = None;
            publish_phase(&reader, Phase::Scanning, None, "reason=freeze-window-expired");
        }
        if attached.is_none() {
            match win::select_target() {
                Ok(target) => {
                    let client = target.client().unwrap_or(Client::Stable);
                    gate.on_attach();
                    publish_idle(&reader, &gate, Some(target.pid), Some(&target), client);
                    eprintln!(
                        "[osu] attached pid={} client={} bitness=0x{:04X} module_base=0x{:08X} path={}",
                        target.pid,
                        client.as_str(),
                        target.bitness,
                        target.module_base,
                        target.image_path.display()
                    );
                    publish_phase(&reader, Phase::Scanning, None, &format!("pid={}", target.pid));
                    crate::server::log::log_at(
                        "info",
                        &format!(
                            "[osu] attached to {:?} (pid={}, image={})",
                            target.client(),
                            target.pid,
                            target.image_path.display()
                        ),
                    );
                    attached = Some(target);
                    anchors = None;
                    stable_table = None;
                    lazer_attach = None;
                    regions = scan::RegionCache::new();
                    previous_live = None;
                    if let Some(lost_at) = lost_at.take() {
                        eprintln!(
                            "[osu] recovered: attached {}ms after the previous target was lost (fast window {}ms @ {}ms retries)",
                            lost_at.elapsed().as_millis(),
                            LOSS_FAST_WINDOW_MS,
                            LOSS_FAST_RETRY.as_millis()
                        );
                    }
                    attach_failures = 0;
                }
                Err(reason) => {
                    let detail = reason.as_str();
                    let outcome = gate.on_anchor_failure(reason.clone(), now_ms);
                    if reason != Reason::ProcessNotFound {
                        eprintln!("[osu] no target: {detail}");
                    }
                    publish_outcome(&reader, &gate, &outcome, None, None, None);
                    log_transition(&outcome);
                    publish_phase(
                        &reader,
                        phase_for(false, false, false, lost_at.is_some(), reason == Reason::ProcessNotFound),
                        None,
                        &format!("reason={detail}"),
                    );
                    let (lost_from_attached, elapsed_ms) = match lost_at {
                        Some(at) => (true, at.elapsed().as_millis() as u64),
                        None => (false, 0),
                    };
                    let retry_soon = win::retry_soon();
                    let used_sweep = win::last_discovery_used_sweep();
                    let delay = sweep_delay(
                        lost_from_attached,
                        elapsed_ms,
                        attach_failures,
                        retry_soon,
                        used_sweep,
                    );
                    eprintln!(
                        "[osu] attach retry in {}ms (lost_from_attached={} since_loss={}ms failures={} retry_soon={} swept={})",
                        delay.as_millis(),
                        lost_from_attached,
                        elapsed_ms,
                        attach_failures,
                        retry_soon,
                        used_sweep
                    );
                    if !lost_from_attached || elapsed_ms >= LOSS_FAST_WINDOW_MS {
                        attach_failures += 1;
                    }
                    thread::sleep(delay);
                    continue;
                }
            }
        }

        if anchors.is_none() && lazer_attach.is_none() {
            let retry_due = last_scan_fail
                .map(|at| at.elapsed() >= SCAN_RETRY.max(invariants::backoff(scan_fail_attempt)))
                .unwrap_or(true);
            if retry_due {
                let target = attached.as_ref().expect("attached");
                let client = target.client().unwrap_or(Client::Stable);
                if client == Client::Lazer {
                    match lazer::attach(target, &mut regions, lazer::env_table_path()) {
                        Ok(attach) => {
                            eprintln!(
                                "[osu] lazer L0 resolved: table={} origin={} path={} gameBase=0x{:016X} anchors={} scan_ms={}",
                                attach.table.key(),
                                attach.origin.as_str(),
                                attach.table_path.display(),
                                attach.game_base,
                                attach.marker_hits,
                                attach.scan_ms
                            );
                            publish_lazer_state(
                                &reader,
                                Some(lazer::Diagnostics::from_attach(&attach)),
                                None,
                            );
                            lazer_attach = Some(attach);
                            last_scan_fail = None;
                            scan_fail_attempt = 0;
                            gate.on_anchor_resolved(now_ms);
                            thread::sleep(TICK);
                            continue;
                        }
                        Err(reason) => {
                            let detail = reason.as_str();
                            eprintln!(
                                "[osu] lazer L0 failed (attempt {}): {detail}",
                                scan_fail_attempt + 1
                            );
                            crate::server::log::log_at(
                                "warn",
                                &format!(
                                    "[osu] lazer L0 failed (attempt {}): {detail}",
                                    scan_fail_attempt + 1
                                ),
                            );
                            last_scan_fail = Some(std::time::Instant::now());
                            let outcome = gate.on_anchor_failure(reason.clone(), now_ms);
                            log_transition(&outcome);
                            publish_outcome(&reader, &gate, &outcome, None, None, Some(client));
                            let extra = matches!(reason, Reason::LazerOffsetsMissing(_))
                                .then(lazer::missing_table_degraded_fields);
                            publish_lazer_state(
                                &reader,
                                Some(lazer::Diagnostics {
                                    gaps: extra.clone().unwrap_or_default(),
                                    ..Default::default()
                                }),
                                extra,
                            );
                            write_record(
                                writer.as_mut(),
                                feed.as_ref(),
                                &CompareInput {
                                    snapshot: &Snapshot::default(),
                                    outcome: &outcome,
                                    probe: None,
                                    result: None,
                                },
                            );
                            scan_fail_attempt += 1;
                            attached = None;
                            regions = scan::RegionCache::new();
                            lost_at = Some(std::time::Instant::now());
                            attach_failures = 0;
                            publish_phase(&reader, Phase::Attaching, None, &format!("reason={detail}"));
                            thread::sleep(invariants::backoff(scan_fail_attempt));
                            continue;
                        }
                    }
                }
                if client == Client::Stable && stable_table.is_none() {
                    let exe_dir = target.image_path.parent();
                    let loaded = offsets::find_stable_table(exe_dir)
                        .unwrap_or_else(|_| offsets::default_stable_table());
                    eprintln!(
                        "[osu] stable table loaded: client={} ver={} verified_build={}",
                        loaded.client, loaded.version, loaded.verified_build
                    );
                    stable_table = Some(loaded);
                }
                if let Some((entry, validated_ms)) =
                    cached_anchors(target, &mut regions, &mut anchor_cache, stable_table.as_ref())
                {
                    eprintln!(
                        "[osu] anchors from cache validated in {}ms: {} unresolved={:?}",
                        validated_ms,
                        entry.describe(),
                        entry.unresolved
                    );
                    anchors = Some(entry.table);
                    last_scan_fail = None;
                    scan_fail_attempt = 0;
                    gate.on_anchor_resolved(now_ms);
                }
                if anchors.is_none() {
                    let mut on_progress =
                        |progress: ScanProgress| publish_scan_progress(&reader, progress);
                    match resolve_anchors(target, &mut regions, stable_table.as_ref(), &mut on_progress) {
                        Ok((table, elapsed_ms)) => {
                            eprintln!(
                                "[osu] anchors resolved in {}ms: statusPtr=0x{:08X} baseAddr=0x{:08X} playTimeAddr=0x{:08X} rulesetsAddr=0x{:08X} menuModsPtr=0x{:08X} getAudioLengthPtr=0x{:08X} settingsClassAddr=0x{:08X}",
                                elapsed_ms,
                                table.status_ptr.unwrap_or(0),
                                table.base_addr.unwrap_or(0),
                                table.play_time_addr.unwrap_or(0),
                                table.rulesets_addr.unwrap_or(0),
                                table.menu_mods_ptr.unwrap_or(0),
                                table.audio_length_ptr.unwrap_or(0),
                                table.settings_class_addr.unwrap_or(0)
                            );
                            if let Some(key) = anchor_cache_key(target) {
                                anchor_cache.store(
                                    key,
                                    anchor_cache::AnchorEntry::from_table(table.clone()),
                                );
                            }
                            anchors = Some(table);
                            last_scan_fail = None;
                            scan_fail_attempt = 0;
                            gate.on_anchor_resolved(now_ms);
                            if anchor_cache_selftest() {
                                let started = std::time::Instant::now();
                                match cached_anchors(target, &mut regions, &mut anchor_cache, stable_table.as_ref()) {
                                    Some((entry, validated_ms)) => eprintln!(
                                        "[osu] selftest: anchors from cache validated in {}ms ({}us): {} unresolved={:?}",
                                        validated_ms,
                                        started.elapsed().as_micros(),
                                        entry.describe(),
                                        entry.unresolved
                                    ),
                                    None => eprintln!(
                                        "[osu] selftest: cache validation returned no table (reason above)"
                                    ),
                                }
                            }
                        }
                        Err(reason) => {
                            let detail = reason.as_str();
                            eprintln!(
                                "[osu] anchor scan failed (attempt {}): {detail}",
                                scan_fail_attempt + 1
                            );
                            crate::server::log::log_at(
                                "warn",
                                &format!(
                                    "[osu] anchor scan failed (attempt {}): {detail}",
                                    scan_fail_attempt + 1
                                ),
                            );
                            last_scan_fail = Some(std::time::Instant::now());
                            let outcome = gate.on_anchor_failure(reason.clone(), now_ms);
                            log_transition(&outcome);
                            publish_outcome(&reader, &gate, &outcome, None, None, Some(client));
                            write_record(
                                writer.as_mut(),
                                feed.as_ref(),
                                &CompareInput {
                                    snapshot: &Snapshot::default(),
                                    outcome: &outcome,
                                    probe: None,
                                    result: None,
                                },
                            );
                            scan_fail_attempt += 1;
                            attached = None;
                            regions = scan::RegionCache::new();
                            lost_at = Some(std::time::Instant::now());
                            attach_failures = 0;
                            publish_phase(&reader, Phase::Attaching, None, &format!("reason={detail}"));
                            thread::sleep(invariants::backoff(scan_fail_attempt));
                            continue;
                        }
                    }
                }
            } else {
                thread::sleep(TICK);
                continue;
            }
        }

        {
            let target = attached.as_ref().expect("attached");
            let client = target.client().unwrap_or(Client::Stable);
            let read = match lazer_attach.as_mut() {
                Some(attach) => lazer::read_tick(attach, target).map(|frame| {
                    publish_lazer_state(
                        &reader,
                        Some(lazer::Diagnostics {
                            gaps: frame.gaps.clone(),
                            chain: Some(frame.chain),
                            ..lazer::Diagnostics::from_attach(attach)
                        }),
                        None,
                    );
                    FrameRead {
                        snapshot: frame.snapshot,
                        probe: None,
                        result: None,
                    }
                }),
                None => {
                    let table = anchors.as_ref().expect("anchors");
                    read_frame(target, table, stable_table.as_ref(), previous_live)
                }
            };
            match read {
                Ok(mut frame) => {
                    attach_beatmap_file(&mut frame.snapshot, &mut osu_cache);
                    apply_hits_with_topo(&mut frame.snapshot, stable_table.as_ref().map(|t| &t.topology));
                    previous_live = frame.snapshot.play_time;
                    let outcome = gate.on_frame(&frame.snapshot, now_ms);
                    log_transition(&outcome);
                    publish_outcome(
                        &reader,
                        &gate,
                        &outcome,
                        Some(&frame.snapshot),
                        anchors.as_ref().map(AnchorAddrs::from_table),
                        Some(client),
                    );
                    write_record(
                        writer.as_mut(),
                        feed.as_ref(),
                        &CompareInput {
                            snapshot: &frame.snapshot,
                            outcome: &outcome,
                            probe: frame.probe.as_ref(),
                            result: frame.result.as_ref(),
                        },
                    );
                    publish_phase(
                        &reader,
                        phase_for(
                            !matches!(outcome.action, FrameAction::Stop),
                            true,
                            true,
                            true,
                            false,
                        ),
                        None,
                        &format!("pid={}", target.pid),
                    );
                }
                Err(reason) => {
                    let detail = reason.as_str();
                    eprintln!("[osu] read failed, detaching: {detail}");
                    crate::server::log::log_at(
                        "warn",
                        &format!("[osu] read failed, detaching: {detail}"),
                    );
                    let outcome = gate.on_anchor_failure(reason.clone(), now_ms);
                    log_transition(&outcome);
                    publish_outcome(&reader, &gate, &outcome, None, None, Some(client));
                    write_record(
                        writer.as_mut(),
                        feed.as_ref(),
                        &CompareInput {
                            snapshot: &Snapshot::default(),
                            outcome: &outcome,
                            probe: None,
                            result: None,
                        },
                    );
                    attached = None;
                    anchors = None;
                    lazer_attach = None;
                    osu_cache = None;
                    previous_live = None;
                    regions = scan::RegionCache::new();
                    lost_at = Some(std::time::Instant::now());
                    attach_failures = 0;
                    publish_phase(&reader, Phase::Attaching, None, &format!("reason={detail}"));
                }
            }
        }
        thread::sleep(compare_interval);
    }
}

#[cfg(not(windows))]
pub fn run(reader: Reader) {
    let mut gate = Gate::new();
    let outcome = gate.on_anchor_failure(Reason::PlatformUnsupported, 0);
    publish_outcome(&reader, &gate, &outcome, None, None, None);
    loop {
        thread::sleep(ATTACH_RETRY);
    }
}

#[cfg(windows)]
pub fn publish_phase(reader: &Reader, phase: Phase, scan: Option<ScanProgress>, detail: &str) {
    let transition = {
        let mut state = reader.inner.lock().unwrap();
        let changed = state.phase != phase.as_str();
        let transition = if changed {
            let mut line = format!("[osu] phase={}", phase.as_str());
            if !detail.is_empty() {
                line.push(' ');
                line.push_str(detail);
            }
            if phase == Phase::Healthy {
                if let Some(scan) = state.scan {
                    line.push_str(&format!(
                        " scan_ms={} filter={} regions={} bytes={}",
                        scan.elapsed_ms, scan.filter, scan.regions, scan.bytes
                    ));
                }
            }
            Some(line)
        } else {
            None
        };
        state.phase = phase.as_str().to_string();
        state.phase_notice = phase.notice().to_string();
        state.scan = scan;
        transition
    };
    if let Some(line) = transition {
        eprintln!("{line}");
        crate::server::log::log_at("info", &format!("[osu] {line}"));
    }
}

#[cfg(windows)]
pub fn publish_scan_progress(reader: &Reader, progress: ScanProgress) {
    let mut state = reader.inner.lock().unwrap();
    if state.phase == Phase::Scanning.as_str() {
        state.scan = Some(progress);
    }
}

#[cfg(windows)]
pub fn publish_idle(
    reader: &Reader,
    gate: &Gate,
    pid: Option<u32>,
    target: Option<&win::Target>,
    client: Client,
) {
    let mut state = reader.inner.lock().unwrap();
    state.pid = pid.or(state.pid);
    if let Some(target) = target {
        state.image_path = Some(target.image_path.to_string_lossy().to_string());
    }
    state.client = Some(client.as_str().to_string());
    state.health = gate.state().as_str().to_string();
    state.degraded_fields = gate.degraded_fields();
    state.strikes = gate.strikes();
    state.frozen = gate.frozen();
    state.snapshot = None;
    state.packet = None;
    state.holding = false;
    state.held_packet = None;
    state.reason = None;
    state.lazer = None;
}

#[cfg(windows)]
pub fn publish_lazer_state(
    reader: &Reader,
    diagnostics: Option<lazer::Diagnostics>,
    extra_degraded: Option<Vec<String>>,
) {
    let mut state = reader.inner.lock().unwrap();
    if let Some(diagnostics) = diagnostics {
        state.lazer = Some(diagnostics);
    }
    if let Some(fields) = extra_degraded {
        for field in fields {
            if !state.degraded_fields.contains(&field) {
                state.degraded_fields.push(field);
            }
        }
    }
}

pub fn publish_outcome(
    reader: &Reader,
    gate: &Gate,
    outcome: &invariants::FrameOutcome,
    snapshot: Option<&Snapshot>,
    anchors: Option<AnchorAddrs>,
    client: Option<Client>,
) {
    let mut state = reader.inner.lock().unwrap();
    state.health = gate.state().as_str().to_string();
    state.strikes = gate.strikes();
    state.frozen = gate.frozen();
    state.frame_at_ms = crate::server::now_ms();
    state.degraded_fields = if outcome.degraded_fields.is_empty() {
        gate.degraded_fields()
    } else {
        outcome.degraded_fields.clone()
    };
    if let Some(client) = client {
        state.client = Some(client.as_str().to_string());
    }
    if let Some(anchors) = anchors {
        state.anchors = Some(anchors);
    }
    match outcome.action {
        FrameAction::Stop => {
            state.snapshot = snapshot.map(|snapshot| frozen_copy(snapshot));
            state.packet = None;
            state.holding = false;
            state.held_packet = None;
            state.reason = outcome.reason.as_ref().map(Reason::as_str);
        }
        FrameAction::FreezeStateOnly => {
            if let Some(snapshot) = snapshot {
                state.snapshot = Some(frozen_copy(snapshot));
                state.packet = Some(snapshot.to_frozen_packet());
            }
            state.holding = false;
            state.reason = outcome.reason.as_ref().map(Reason::as_str);
        }
        FrameAction::HoldLastGood => {
            let held = state.held_packet.clone();
            if let (Some(snapshot), Some(held)) = (snapshot, held) {
                state.snapshot = Some(frozen_copy(snapshot));
                state.packet = Some(crate::osu::packet::held_packet_from(&held, snapshot));
                state.holding = true;
            } else if let Some(snapshot) = snapshot {
                state.snapshot = Some(frozen_copy(snapshot));
                state.packet = Some(snapshot.to_frozen_packet());
                state.holding = false;
            }
            state.reason = outcome.reason.as_ref().map(Reason::as_str);
        }
        FrameAction::Publish => {
            if let Some(snapshot) = snapshot {
                state.snapshot = Some(snapshot.clone());
                let packet = snapshot.to_packet();
                state.held_packet = Some(packet.clone());
                state.packet = Some(packet);
            }
            state.holding = false;
            state.reason = outcome.reason.as_ref().map(Reason::as_str);
        }
    }
}

pub fn frozen_copy(snapshot: &Snapshot) -> Snapshot {
    let mut copy = snapshot.clone();
    copy.beatmap_object = None;
    copy.ruleset_base = None;
    copy.gameplay_base = None;
    copy.score_base = None;
    copy.result_base = None;
    copy.play_mods_mask = None;
    copy.result_mods_mask = None;
    copy.menu_mods_mask = None;
    copy.play_hits = None;
    copy.result_hits = None;
    copy.play_time = None;
    copy.paused = None;
    copy.checksum = None;
    copy.map_id = None;
    copy.set_id = None;
    copy.filename = None;
    copy.folder = None;
    copy.version = None;
    copy.artist = None;
    copy.title = None;
    copy.mapper = None;
    copy.beatmap_file = None;
    copy.lazer_mods = None;
    copy.lazer_chain = None;
    copy
}

pub fn log_transition(outcome: &invariants::FrameOutcome) {
    if let Some(reason) = outcome.transition.as_ref() {
        eprintln!(
            "[osu] gate: state={} action={:?} transition reason={}",
            outcome.state.map(|s| s.as_str()).unwrap_or("?"),
            outcome.action,
            reason.as_str()
        );
    }
}

#[cfg(windows)]
pub struct CompareInput<'a> {
    pub snapshot: &'a Snapshot,
    pub outcome: &'a invariants::FrameOutcome,
    pub probe: Option<&'a stable::ChainProbe>,
    pub result: Option<&'a stable::ResultRead>,
}

#[cfg(windows)]
pub fn write_record(
    writer: Option<&mut compare::SampleWriter>,
    feed: Option<&Arc<compare::TosuFeed>>,
    input: &CompareInput<'_>,
) {
    let Some(writer) = writer else {
        return;
    };
    let tosu = feed.and_then(|f| f.latest());
    let record = compare::record(input.snapshot, input.outcome, input.probe, input.result, tosu.as_ref());
    writer.write(&record);
}

#[cfg(windows)]
pub fn resolve_anchors(
    target: &win::Target,
    regions: &mut scan::RegionCache,
    stable_table: Option<&offsets::StableTable>,
    progress: &mut dyn FnMut(ScanProgress),
) -> Result<(patterns::AnchorTable, u128), Reason> {
    use std::time::Instant;
    let started = Instant::now();
    let mut table = patterns::AnchorTable::default();
    let mut order: Vec<&'static str> = vec![
        "statusPtr",
        "baseAddr",
        "playTimeAddr",
        "rulesetsAddr",
        "menuModsPtr",
        "getAudioLengthPtr",
        "settingsClassAddr",
    ];
    let mut degraded: Vec<&'static str> = Vec::new();

    for (index, mask) in FILTER_READY.iter().enumerate() {
        let regions = target.regions_cached(regions, *mask, REGION_LIMIT);
        let mut stats = scan::ScanStats {
            regions: regions.len(),
            ..Default::default()
        };
        progress(ScanProgress {
            filter: index,
            regions: stats.regions,
            bytes: 0,
            elapsed_ms: started.elapsed().as_millis() as u64,
        });
        for key in &mut order {
            if table.get(key).is_some() {
                continue;
            }
            let Some((pattern_str, offset)) = patterns::anchor_pattern_and_offset(key, stable_table) else {
                continue;
            };
            let pattern = scan::Pattern::parse(pattern_str)
                .map_err(|_| Reason::SignatureMiss(key))?;
            let candidates = scan::find_in_regions(
                target.handle(),
                &regions,
                &pattern,
                offset,
                ANCHOR_HIT_LIMIT,
                &mut stats,
            );
            let topo = stable_table.map(|t| &t.topology);
            let chosen = candidates
                .iter()
                .copied()
                .find(|addr| anchor_proves_out_with_topo(target, *key, *addr, &regions, topo));
            eprintln!(
                "[osu] scan {} -> {} hit(s) {} chosen={:?}",
                key,
                candidates.len(),
                candidates
                    .iter()
                    .map(|a| format!("0x{a:08X}"))
                    .collect::<Vec<_>>()
                    .join(" "),
                chosen.map(|a| format!("0x{a:08X}"))
            );
            match chosen {
                Some(addr) => table.set(key, addr),
                None => {
                    if !degraded.contains(key) {
                        degraded.push(key);
                    }
                }
            }
            progress(ScanProgress {
                filter: index,
                regions: stats.regions,
                bytes: stats.bytes_read,
                elapsed_ms: started.elapsed().as_millis() as u64,
            });
        }
        eprintln!(
            "[osu] scan filter#{} regions={} chunks_ok={} chunks_failed={} bytes={} elapsed={}ms unresolved={:?}",
            index,
            stats.regions,
            stats.chunks_ok,
            stats.chunks_failed,
            stats.bytes_read,
            started.elapsed().as_millis(),
            degraded
        );
        if table.missing().is_none() {
            break;
        }
    }
    match table.missing() {
        Some(key) => Err(Reason::SignatureMiss(key)),
        None => Ok((table, started.elapsed().as_millis())),
    }
}

#[cfg(windows)]
pub fn anchor_cache_selftest() -> bool {
    matches!(std::env::var("MMA_OSU_ANCHOR_CACHE_SELFTEST"), Ok(value) if value.trim() == "1")
}

#[cfg(windows)]
pub fn cached_anchors(
    target: &win::Target,
    regions: &mut scan::RegionCache,
    cache: &mut anchor_cache::AnchorCache,
    stable_table: Option<&offsets::StableTable>,
) -> Option<(anchor_cache::AnchorEntry, u128)> {
    use std::time::Instant;
    let key = anchor_cache_key(target)?;
    match cache.observe(&key) {
        anchor_cache::KeyChange::Same => {}
        anchor_cache::KeyChange::First => return None,
        anchor_cache::KeyChange::Dropped(previous) => {
            eprintln!(
                "[osu] anchor cache dropped: image identity changed (md5 {} → {}, bitness 0x{:04X} → 0x{:04X}, module_base 0x{:08X} → 0x{:08X})",
                previous.exe_md5,
                key.exe_md5,
                previous.bitness,
                key.bitness,
                previous.module_base,
                key.module_base
            );
            return None;
        }
    }
    let started = Instant::now();
    let region_list = target.regions_cached(regions, FILTER_READY[0], REGION_LIMIT);
    let topo = stable_table.map(|t| &t.topology);
    let outcome = cache.resolve(&key, |anchor_key, addr| {
        if !signature_still_matches(target, anchor_key, addr, stable_table) {
            eprintln!(
                "[osu] anchor cache invalid: {anchor_key} signature no longer matches at 0x{addr:08X}"
            );
            return false;
        }
        if !anchor_proves_out_with_topo(target, anchor_key, addr, &region_list, topo) {
            eprintln!(
                "[osu] anchor cache invalid: {anchor_key} structure check failed at 0x{addr:08X}"
            );
            return false;
        }
        true
    });
    match outcome {
        anchor_cache::Cached::Validated(entry) => Some((entry, started.elapsed().as_millis())),
        anchor_cache::Cached::Stale(key) => {
            eprintln!(
                "[osu] anchor cache stale at {key} — dropping entry, falling back to a full scan"
            );
            cache.clear();
            None
        }
        anchor_cache::Cached::NoEntry => None,
    }
}

#[cfg(windows)]
pub fn anchor_cache_key(target: &win::Target) -> Option<anchor_cache::CacheKey> {
    let bytes = std::fs::read(&target.image_path).ok()?;
    Some(anchor_cache::CacheKey::new(
        crate::server::md5_hex_bytes(&bytes),
        target.bitness,
        target.module_base,
    ))
}

#[cfg(windows)]
pub fn signature_still_matches(
    target: &win::Target,
    key: &str,
    addr: u32,
    stable_table: Option<&offsets::StableTable>,
) -> bool {
    let Some((pattern_str, offset)) = patterns::anchor_pattern_and_offset(key, stable_table) else {
        return false;
    };
    let Ok(pattern) = scan::Pattern::parse(pattern_str) else {
        return false;
    };
    let mut buf = vec![0u8; pattern.len()];
    let match_addr = (addr as i64 - offset as i64) as u32;
    if win::read_exact_at(target.handle(), match_addr, &mut buf).is_err() {
        return false;
    }
    pattern.matches_at(&buf, 0)
}

#[cfg(windows)]
pub struct FrameRead {
    pub snapshot: Snapshot,
    pub probe: Option<stable::ChainProbe>,
    pub result: Option<stable::ResultRead>,
}

#[cfg(windows)]
pub fn read_frame(
    target: &win::Target,
    table: &patterns::AnchorTable,
    stable_table: Option<&offsets::StableTable>,
    previous_live: Option<i32>,
) -> Result<FrameRead, Reason> {
    let mut snapshot = Snapshot {
        client: Some(Client::Stable),
        pid: target.pid,
        ..Default::default()
    };
    let mut degraded: Vec<String> = Vec::new();
    let topo = stable_table.map(|t| &t.topology);

    if let Some(status_ptr) = table.status_ptr {
        let raw = win::read_pointer(target, status_ptr)?;
        let index = raw as i32;
        snapshot.state_number = Some(index);
        let name = stable_table
            .and_then(|t| t.state_name(index))
            .unwrap_or_else(|| model::state_name_for(index));
        snapshot.state_name = Some(name.to_string());
    }

    if let Some(play_time_addr) = table.play_time_addr {
        match stable::read_play_time_with_topo(target, play_time_addr, topo) {
            Ok(Some(live)) => {
                snapshot.play_time = Some(live);
                snapshot.paused = Some(stable::paused_from_previous(previous_live, Some(live)));
            }
            Ok(None) => {
                degraded.push("beatmap.time.live".to_string());
            }
            Err(reason) => {
                eprintln!("[osu] playTime read failed: {}", reason.as_str());
                degraded.push("beatmap.time.live".to_string());
            }
        }
    }

    if let Some(base_addr) = table.base_addr {
        let beatmap_offset = topo.map(|t| t.beatmap_from_base).unwrap_or(stable::BEATMAP_FROM_BASE);
        let beatmap_addr = base_addr.wrapping_sub(beatmap_offset);
        let object = win::read_pointer(target, beatmap_addr)?;
        snapshot.beatmap_object = Some(object);
        if object != 0 {
            let mut read_string = |offset: u32, field: &'static str| -> Option<String> {
                let slot = object.wrapping_add(offset);
                let ptr = match win::read_u32(target, slot) {
                    Ok(p) => p,
                    Err(_) => {
                        degraded.push(field.to_string());
                        return None;
                    }
                };
                match win::read_csharp_string(target, ptr) {
                    Ok(text) => Some(text),
                    Err(_) => {
                        degraded.push(field.to_string());
                        None
                    }
                }
            };
            let md5_off = topo.map(|t| t.beatmap_md5).unwrap_or(0x6C);
            let fn_off = topo.map(|t| t.beatmap_filename).unwrap_or(0x90);
            let fold_off = topo.map(|t| t.beatmap_folder).unwrap_or(0x78);
            let ver_off = topo.map(|t| t.beatmap_version).unwrap_or(0xAC);
            let art_off = topo.map(|t| t.beatmap_artist).unwrap_or(0x18);
            let tit_off = topo.map(|t| t.beatmap_title).unwrap_or(0x24);
            let map_off = topo.map(|t| t.beatmap_mapper).unwrap_or(0x7C);
            let map_id_off = topo.map(|t| t.beatmap_id).unwrap_or(0xC8);
            let set_id_off = topo.map(|t| t.beatmap_set_id).unwrap_or(0xCC);

            snapshot.checksum = read_string(md5_off, "beatmap.md5");
            snapshot.filename = read_string(fn_off, "files.beatmap");
            snapshot.folder = read_string(fold_off, "folders.beatmap");
            snapshot.version = read_string(ver_off, "beatmap.version");
            snapshot.artist = read_string(art_off, "beatmap.artist");
            snapshot.title = read_string(tit_off, "beatmap.title");
            snapshot.mapper = read_string(map_off, "beatmap.mapper");
            snapshot.map_id = win::read_i32(target, object.wrapping_add(map_id_off)).ok();
            snapshot.set_id = win::read_i32(target, object.wrapping_add(set_id_off)).ok();
            snapshot.map_id_bits = win::read_u32(target, object.wrapping_add(map_id_off)).ok();
        }
    }

    if let Some(menu_mods_ptr) = table.menu_mods_ptr {
        match win::read_pointer(target, menu_mods_ptr) {
            Ok(mask) => snapshot.menu_mods_mask = Some(mask),
            Err(_) => degraded.push("menu.mods".to_string()),
        }
    }

    let mut probe = None;
    let mut result_read = None;
    if let Some(rulesets_addr) = table.rulesets_addr {
        match stable::resolve_ruleset_with_topo(target, rulesets_addr, topo) {
            Ok((ruleset, chain_probe)) => {
                snapshot.ruleset_base = Some(ruleset);
                probe = Some(chain_probe);
                if let Some(base_addr) = table.base_addr {
                    let in_game = stable::read_in_game_with_topo(target, ruleset, base_addr, topo);
                    snapshot.gameplay_base = in_game.gameplay_base;
                    snapshot.score_base = in_game.score_base;
                    snapshot.play_mods_mask = in_game.play_mods_mask;
                    snapshot.hits_candidates = in_game.hits_candidates.clone();
                    snapshot.hits_candidates_complete = in_game.hits_candidates_complete;
                    snapshot.retries = in_game.retries;
                    snapshot.plays = in_game.plays;
                    if in_game.gameplay_base.is_none() {
                        degraded.push("play.mods".to_string());
                    }
                }
                let result = stable::read_result_with_topo(target, ruleset, topo);
                snapshot.result_base = result.result_base;
                snapshot.result_mods_mask = result.result_mods_mask;
                snapshot.result_hits_candidates = result.hits_candidates.clone();
                snapshot.result_hits_candidates_complete = result.hits_candidates_complete;
                if result.result_base.is_none() {
                    degraded.push("resultsScreen.mods".to_string());
                }
                result_read = Some(result);
            }
            Err(reason) => {
                eprintln!("[osu] ruleset chain failed: {}", reason.as_str());
                degraded.push("play.mods".to_string());
                degraded.push("resultsScreen.mods".to_string());
            }
        }
    }

    if let Some(audio_length_ptr) = table.audio_length_ptr {
        match stable::read_mp3_length_with_topo(target, audio_length_ptr, topo) {
            Ok(length) => snapshot.mp3_length = Some(length),
            Err(_) => degraded.push("beatmap.time.mp3Length".to_string()),
        }
    }

    snapshot.game_folder = stable::game_folder(&target.image_path);

    if let Some(settings_class_addr) = table.settings_class_addr {
        snapshot.songs_cfg_value = read_songs_cfg_value(target, settings_class_addr);
        if snapshot.songs_cfg_value.is_none() {
            degraded.push("folders.songs".to_string());
        }
        snapshot.songs_folder = snapshot
            .songs_cfg_value
            .as_deref()
            .map(|value| resolve_songs_folder(&target.image_path, value));
    }
    if snapshot.songs_folder.is_none() {
        snapshot.songs_folder = fallback_songs_folder(&target.image_path);
    }

    snapshot.degraded_fields = degraded;
    Ok(FrameRead {
        snapshot,
        probe,
        result: result_read,
    })
}

#[cfg(windows)]
pub fn read_songs_cfg_value(target: &win::Target, settings_class_addr: u32) -> Option<String> {
    let first = win::read_pointer(target, settings_class_addr.wrapping_add(0x8)).ok()?;
    if first == 0 {
        return None;
    }
    let second = win::read_u32(target, first.wrapping_add(0xB8)).ok()?;
    if second == 0 {
        return None;
    }
    let slot = win::read_u32(target, second.wrapping_add(0x4)).ok()?;
    win::read_csharp_string(target, slot).ok()
}

#[cfg(not(windows))]
pub fn read_songs_cfg_value(_target: &win::Target, _addr: u32) -> Option<String> {
    None
}

pub fn attach_beatmap_file(snapshot: &mut Snapshot, cache: &mut Option<OsuFileCache>) {
    let previous_bg = snapshot.background.clone();
    snapshot.beatmap_file = None;
    snapshot.beatmap_file_error = None;
    snapshot.beatmap_file_md5 = None;
    snapshot.background = None;
    snapshot.audio = None;
    snapshot.beatmap_file_mismatches.clear();
    let (Some(songs), Some(folder), Some(filename), Some(checksum)) = (
        snapshot.songs_folder.as_deref(),
        snapshot.folder.as_deref(),
        snapshot.filename.as_deref(),
        snapshot.checksum.as_deref(),
    ) else {
        snapshot.beatmap_file_error = Some("songs-or-path-unavailable".to_string());
        return;
    };
    let path = std::path::Path::new(songs).join(folder).join(filename);
    snapshot.beatmap_file_path = Some(path.to_string_lossy().to_string());

    if cache.as_ref().map(|c| c.checksum.as_str()) != Some(checksum) {
        let loaded = match std::fs::read(&path) {
            Ok(bytes) => {
                let md5 = crate::server::md5_hex_bytes(&bytes);
                match String::from_utf8(bytes) {
                    Ok(text) => Ok((beatmap_file::parse(&text), md5)),
                    Err(e) => Err(format!("not-utf8: {e}")),
                }
            }
            Err(e) => Err(format!("read: {e}")),
        };
        *cache = Some(OsuFileCache {
            checksum: checksum.to_string(),
            path: path.to_string_lossy().to_string(),
            loaded,
        });
    }
    let entry = cache.as_ref().expect("cache filled above");
    match entry.loaded.as_ref() {
        Ok((file, md5)) => {
            snapshot.beatmap_file_md5 = Some(md5.clone());
            if snapshot.client == Some(Client::Lazer) {
                snapshot.background = previous_bg.or_else(|| {
                    lazer::find_background_file(
                        &snapshot.lazer_files,
                        file.background.as_deref(),
                    )
                });
            } else {
                snapshot.background = file.background.clone();
            }
            snapshot.audio = file.audio.clone();
            snapshot.beatmap_file_mismatches = mismatches_for(file, snapshot);
            snapshot.beatmap_file = Some(file.clone());
        }
        Err(error) => snapshot.beatmap_file_error = Some(error.clone()),
    }
}

pub struct OsuFileCache {
    pub checksum: String,
    #[allow(dead_code)]
    pub path: String,
    pub loaded: Result<(beatmap_file::BeatmapFile, String), String>,
}

pub fn mismatches_for(file: &beatmap_file::BeatmapFile, snapshot: &Snapshot) -> Vec<String> {
    file.header_mismatches(
        snapshot.title.as_deref(),
        snapshot.artist.as_deref(),
        snapshot.mapper.as_deref(),
        snapshot.version.as_deref(),
        snapshot.map_id,
        snapshot.set_id,
    )
    .into_iter()
    .map(|field| field.to_string())
    .collect()
}

#[cfg(not(windows))]
pub fn resolve_anchors(
    _target: &win::Target,
    _regions: &mut scan::RegionCache,
    _stable_table: Option<&offsets::StableTable>,
    _progress: &mut dyn FnMut(ScanProgress),
) -> Result<(patterns::AnchorTable, u128), Reason> {
    Err(Reason::PlatformUnsupported)
}
