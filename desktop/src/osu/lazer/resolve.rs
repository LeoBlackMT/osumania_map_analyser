use crate::osu::offsets::OffsetTable;
use super::fields::{GAME_BASE_HOPS, LIVE_TIME_MAX_MS, MAX_RESOLUTION_LINES};
use super::source::{field_addr, read_f64, read_ptr_field, read_u64, Source};
use super::types::Resolved;

/// x64 用户态指针的合理性判据。
pub fn plausible_ptr(pointer: u64) -> bool {
    pointer >= 0x1_0000 && pointer < 0x0000_8000_0000_0000 && pointer % 8 == 0
}

/// (b) 判据：[gameBase] 必须是结构上像 MethodTable 的值。
pub fn method_table_plausible(source: &dyn Source, method_table: u64) -> bool {
    plausible_ptr(method_table) && source.read(method_table, 8).is_some()
}

/// 一次 (anchor, delta) 解析尝试的完整记录。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResolveAttempt {
    pub anchor: u64,
    pub delta: i64,
    pub site: Option<u64>,
    pub external_link_opener: Option<u64>,
    pub api_access: Option<u64>,
    pub game_base: Option<u64>,
    pub method_table: Option<u64>,
    pub resolved: bool,
    pub candidate: bool,
    pub verdict: String,
}

impl ResolveAttempt {
    pub fn path(&self) -> String {
        let mut parts: Vec<String> = vec![format!(
            "site={}",
            self.site
                .map(|value| format!("0x{value:016X}"))
                .unwrap_or_else(|| "<none>".into())
        )];
        for (index, (label, _)) in GAME_BASE_HOPS.iter().enumerate() {
            let value = match index {
                0 => self.external_link_opener,
                1 => self.api_access,
                _ => self.game_base,
            };
            parts.push(format!(
                "{label}={}",
                value
                    .map(|value| format!("0x{value:016X}"))
                    .unwrap_or_else(|| "<none>".into())
            ));
        }
        parts.join(" -> ")
    }

    fn record_hop(&mut self, index: usize, value: u64) {
        match index {
            0 => self.external_link_opener = Some(value),
            1 => self.api_access = Some(value),
            _ => self.game_base = Some(value),
        }
    }
}

/// 一趟解析的全部尝试 + 去重后的候选。
#[derive(Clone, Debug, Default)]
pub struct Resolution {
    pub attempts: Vec<ResolveAttempt>,
    pub candidates: Vec<ResolveAttempt>,
}

impl Resolution {
    pub fn accepted(&self) -> Option<&ResolveAttempt> {
        self.candidates.first()
    }
}

pub fn site_from_anchor(anchor: u64, delta: i64) -> Option<u64> {
    if delta >= 0 {
        anchor.checked_sub(delta as u64)
    } else {
        anchor.checked_add((-delta) as u64)
    }
}

pub fn try_site(source: &dyn Source, anchor: u64, delta: i64) -> ResolveAttempt {
    let mut attempt = ResolveAttempt {
        anchor,
        delta,
        ..ResolveAttempt::default()
    };
    let Some(site) = site_from_anchor(anchor, delta) else {
        attempt.verdict = "anchor-delta underflow".to_string();
        return attempt;
    };
    attempt.site = Some(site);

    let mut current = site;
    for (index, (label, offset)) in GAME_BASE_HOPS.iter().enumerate() {
        let field_address = current.wrapping_add(*offset);
        match read_u64(source, field_address) {
            Some(value) if plausible_ptr(value) => {
                attempt.record_hop(index, value);
                current = value;
            }
            Some(value) => {
                attempt.record_hop(index, value);
                attempt.verdict =
                    format!("{label}={value:#x} implausible (read at {field_address:#x})");
                return attempt;
            }
            None => {
                attempt.verdict = format!(
                    "{label} unreadable at {field_address:#x} (outside the target's readable regions)"
                );
                return attempt;
            }
        }
    }
    attempt.resolved = true;
    attempt.game_base = Some(current);
    match read_u64(source, current) {
        Some(method_table) if method_table_plausible(source, method_table) => {
            attempt.method_table = Some(method_table);
            attempt.candidate = true;
            attempt.verdict = format!(
                "candidate: {} hop(s) from site 0x{site:016X}; [gameBase]=0x{method_table:016X} is aligned and readable",
                GAME_BASE_HOPS.len()
            );
        }
        Some(method_table) => {
            attempt.method_table = Some(method_table);
            attempt.verdict = format!(
                "[gameBase]=0x{method_table:016X} is not a plausible MethodTable (aligned + readable)"
            );
        }
        None => {
            attempt.verdict = format!("`[gameBase]` unreadable at 0x{current:016X}");
        }
    }
    attempt
}

pub fn resolve_game_base(source: &dyn Source, anchors: &[u64], deltas: &[i64]) -> Resolution {
    let mut resolution = Resolution::default();
    for anchor in anchors {
        for delta in deltas {
            let attempt = try_site(source, *anchor, *delta);
            if attempt.candidate {
                let known = resolution
                    .candidates
                    .iter()
                    .any(|candidate| candidate.game_base == attempt.game_base);
                if !known {
                    resolution.candidates.push(attempt.clone());
                }
            }
            resolution.attempts.push(attempt);
        }
    }
    resolution
}

pub fn log_resolution(resolution: &Resolution) {
    match resolution.accepted() {
        Some(accepted) => eprintln!(
            "[osu] lazer: L1 resolution — {} -> gameBase ({} attempt(s), {} candidate(s))",
            accepted.path(),
            resolution.attempts.len(),
            resolution.candidates.len()
        ),
        None => {
            eprintln!(
                "[osu] lazer: L1 resolution failed — no candidate among {} attempt(s):",
                resolution.attempts.len()
            );
            for attempt in resolution.attempts.iter().take(MAX_RESOLUTION_LINES) {
                eprintln!(
                    "    anchor=0x{:016X} delta={:#x} {} -> {}",
                    attempt.anchor,
                    attempt.delta,
                    attempt.path(),
                    attempt.verdict
                );
            }
            if resolution.attempts.len() > MAX_RESOLUTION_LINES {
                eprintln!(
                    "    … {} more attempt(s) not printed",
                    resolution.attempts.len() - MAX_RESOLUTION_LINES
                );
            }
        }
    }
}

pub fn table_probe(
    source: &dyn Source,
    table: &OffsetTable,
    game_base: u64,
) -> Result<String, String> {
    let resolved = Resolved::from_table(table);
    let storage_offset = resolved
        .storage
        .clone()
        .map_err(|error| format!("offsets-{error} for `<Storage>`"))?;
    let storage = read_ptr_field(source, game_base, storage_offset).ok_or_else(|| {
        format!("`[gameBase + {storage_offset:#x}]` (<Storage>) is not a plausible pointer")
    })?;
    let mut hops = format!("<Storage>@+{storage_offset:#x}=0x{storage:016X}");
    match &resolved.base_path {
        Ok(offset) => {
            let base_path = read_ptr_field(source, storage, *offset).ok_or_else(|| {
                format!("`[storage + {offset:#x}]` (<BasePath>) is not a plausible pointer")
            })?;
            hops.push_str(&format!(" -> <BasePath>@+{offset:#x}=0x{base_path:016X}"));
        }
        Err(error) => hops.push_str(&format!(" (<BasePath> not in the table: offsets-{error})")),
    }
    if let Some(clock) = clock_chain_probe(source, &resolved, game_base)? {
        hops.push_str(" -> ");
        hops.push_str(&clock);
    }
    Ok(hops)
}

/// 播放时钟链（`beatmap.time.live`）的结构自证。
///
/// 为什么需要它：`OsuGameDesktop` 的字段会随 lazer 更新整体后移。若表里
/// `beatmapClock` 仍指向上一个版本的位移，这一跳会落到**另一个托管对象**上；
/// 再按 `interpolatedTrack.<CurrentTime>` 读 8 字节时，读到的往往只是指针位，
/// 作为 double 是一个次正规小数，`round()` 后恒为 0——图表游标会一直停在
/// 谱面起点，而且载荷里看不到任何 `offsets-*` 降级标记。
///
/// 合法的 `CurrentTime` 只有两种：0（时钟尚未启动）或 >= 0.5ms 的真实毫秒值。
/// 指针位读成的次正规数（|v| < 0.5 且 != 0）一律拒绝，避免“错误的表也能通过”。
fn clock_chain_probe(
    source: &dyn Source,
    resolved: &Resolved,
    game_base: u64,
) -> Result<Option<String>, String> {
    use crate::osu::offsets::LookupError;
    let offsets = [&resolved.beatmap_clock, &resolved.beatmap_track_clock, &resolved.beatmap_clock_time];
    let [Ok(&clock_offset), Ok(&track_offset), Ok(&time_offset)] = offsets.map(Result::as_ref) else {
        // A legacy table may omit the clock. A supplied but invalid offset is not an omitted field.
        for error in offsets.iter().filter_map(|offset| offset.as_ref().err()) {
            if matches!(error, LookupError::OutOfRange(..) | LookupError::Ambiguous(..)) {
                return Err(format!("invalid beatmap clock: {error}"));
            }
        }
        return Ok(None);
    };

    let clock = read_ptr_field(source, game_base, clock_offset).ok_or_else(|| {
        format!("`[gameBase + {clock_offset:#x}]` (<beatmapClock>) is not a plausible pointer")
    })?;
    let track = read_ptr_field(source, clock, track_offset).ok_or_else(|| {
        format!("`[beatmapClock + {track_offset:#x}]` (<interpolatedTrack>) is not a plausible pointer")
    })?;
    let address = field_addr(track, time_offset)
        .ok_or_else(|| format!("`interpolatedTrack + {time_offset:#x}` overflows"))?;
    let current = read_f64(source, address).ok_or_else(|| {
        format!("`[interpolatedTrack + {time_offset:#x}]` (<CurrentTime>) is unreadable")
    })?;
    if !clock_value_is_sane(current) {
        return Err(format!(
            "`[interpolatedTrack + {time_offset:#x}]` (<CurrentTime>) = {current} — \
             not a plausible millisecond clock value (pointer bits read as double?)"
        ));
    }

    Ok(Some(format!(
        "<beatmapClock>@+{clock_offset:#x}=0x{clock:016X} -> \
         <interpolatedTrack>@+{track_offset:#x}=0x{track:016X} -> <CurrentTime>={current:.1}ms"
    )))
}

fn clock_value_is_sane(value: f64) -> bool {
    value.is_finite()
        && value.abs() <= LIVE_TIME_MAX_MS
        && (value == 0.0 || value.abs() >= 0.5)
}

#[cfg(test)]
mod clock_probe_tests {
    use super::{clock_value_is_sane, LIVE_TIME_MAX_MS};

    #[test]
    fn clock_value_accepts_zero_and_real_milliseconds() {
        assert!(clock_value_is_sane(0.0));
        assert!(clock_value_is_sane(0.5));
        assert!(clock_value_is_sane(24137.2527));
    }

    #[test]
    fn clock_value_rejects_pointer_bits_read_as_double() {
        assert!(!clock_value_is_sane(1.0908187067103574e-232));
        assert!(!clock_value_is_sane(6.9531711693787e-310));
        assert!(!clock_value_is_sane(f64::NAN));
        assert!(!clock_value_is_sane(f64::INFINITY));
        assert!(!clock_value_is_sane(LIVE_TIME_MAX_MS + 1.0));
    }
}

pub fn vtable_witness_note(table: &OffsetTable, observed: u64) -> String {
    match table.game_base_vtable {
        Some(expected) if expected == observed => format!(
            "vtable witness confirmed — table game_base_vtable=0x{expected:016X} == observed \
             MethodTable (same-process regeneration)"
        ),
        Some(expected) => format!(
            "vtable witness differs (expected across launches) — table 0x{expected:016X} vs \
             observed 0x{observed:016X}"
        ),
        None => "table carries no game_base_vtable witness — the structural proof carries the \
                 identification"
            .to_string(),
    }
}
