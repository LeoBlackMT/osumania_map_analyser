// selftest.rs —— 自测（**不需要 lazer、不需要 dump、不需要任何外部工具**）
//
// 自测跑的是**真实代码路径**，不是复制品：
//   合成 minidump（fixture 的 `Offset`/`Value` 列种进去）→ `minidump` 解析 → 锚点扫描
//   → `chain::walk`（fixture 版 `BlockSource`）→ dump 字节解引用自证 → SOS 中间件
//   → `emit` 双见证校验 → JSON 出表（再用自带的 JSON 解析器读回来核对）。
//
// 覆盖的失败用例：缺 SOS 行、SOS 类型缺失、同名字段多个偏移（歧义）、IL 缺行、
// IL 静态字段、IL 类型不符、IL 显式偏移不符、解引用不一致、dump/IL 版本不一致、
// 中间件格式错误、fixture 未加 `--allow-fixtures`、手写溯源（非 dump 非 fixture）、
// provenance=dump 但没做解引用自证、全部字段落空。
//
// fixture 规则（硬约束）：fixture 文件名带 `EXAMPLE-fixture-`，文件内带 `#fixture true`，
// 出的表**必然**带 `EXAMPLE-FIXTURE` 标记（文件名 + `verified_build`/`evidence` 里各一处）。

use crate::chain::{self, BlockSource};
use crate::emit;
use crate::ilmeta::{self, IlInventory};
use crate::minidump::Dump;
use crate::names;
use crate::runtime;
use crate::sos::{self, SosIntermediate, SosObject};
use crate::{parse_pattern, select_lazer, Candidate, Options};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const EMBEDDED_FIXTURE_IL: &str = include_str!("fixtures/EXAMPLE-fixture-il.tsv");
const EMBEDDED_FIXTURE_MALFORMED: &str = include_str!("fixtures/EXAMPLE-fixture-malformed.tsv");
const EMBEDDED_FIXTURE_RUNTIME: &str = include_str!("fixtures/EXAMPLE-fixture-runtime.txt");
const EMBEDDED_FIXTURE_SOS_CHAIN: &str = include_str!("fixtures/EXAMPLE-fixture-sos-chain.txt");

pub fn run(options: &Options) -> i32 {
    let (fixtures, work) = if let Some(f) = options.value("--fixtures") {
        let p = PathBuf::from(f);
        let w = options.value("--work").map(PathBuf::from).unwrap_or_else(|| p.join("out"));
        (p, w)
    } else {
        let local = PathBuf::from("tools/lazer-offsets-gen/fixtures");
        if local.join("EXAMPLE-fixture-il.tsv").is_file() {
            let w = options.value("--work").map(PathBuf::from).unwrap_or_else(|| local.join("out"));
            (local, w)
        } else {
            let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf()));
            let mut temp_fix = std::env::temp_dir().join("mma-gen-fixtures");
            if fs::create_dir_all(temp_fix.join("out")).is_err() {
                if let Some(ref ed) = exe_dir {
                    temp_fix = ed.join(".fixtures");
                    let _ = fs::create_dir_all(temp_fix.join("out"));
                }
            }
            let _ = fs::write(temp_fix.join("EXAMPLE-fixture-il.tsv"), EMBEDDED_FIXTURE_IL);
            let _ = fs::write(temp_fix.join("EXAMPLE-fixture-malformed.tsv"), EMBEDDED_FIXTURE_MALFORMED);
            let _ = fs::write(temp_fix.join("EXAMPLE-fixture-runtime.txt"), EMBEDDED_FIXTURE_RUNTIME);
            let _ = fs::write(temp_fix.join("EXAMPLE-fixture-sos-chain.txt"), EMBEDDED_FIXTURE_SOS_CHAIN);
            let w = options.value("--work").map(PathBuf::from).unwrap_or_else(|| temp_fix.join("out"));
            (temp_fix, w)
        }
    };
    if let Err(message) = crate::ensure_dir(&work) {
        eprintln!("[error] {message}");
        return 1;
    }
    let context = Context { fixtures, work };
    let mut passed = 0usize;
    let mut failed = 0usize;
    println!("== lazer-offsets-gen self-test ==");
    println!("  fixtures        : {}", context.fixtures.display());
    println!("  work dir        : {}", context.work.display());
    println!();
    for (name, case) in cases() {
        match case(&context) {
            Ok(detail) => {
                passed += 1;
                println!("PASS  {name:<34} {detail}");
            }
            Err(reason) => {
                failed += 1;
                println!("FAIL  {name:<34} {reason}");
            }
        }
    }
    println!();
    println!("self-test: {passed}/{} passed, {failed} failed", passed + failed);
    println!(
        "fixture rule    : the emitted fixture table is named EXAMPLE-FIXTURE-*.json and carries \
         the EXAMPLE-FIXTURE mark inside `verified_build` and `evidence`"
    );
    if failed == 0 {
        0
    } else {
        1
    }
}

struct Context {
    fixtures: PathBuf,
    work: PathBuf,
}

/// fixture 的解析几何（`EXAMPLE-fixture-sos-chain.txt` 的块 1/15/16）：
/// `GameBase 0xBF000FD8`、站点 `+0x6D8 = 0xBF0016B0`、锚点 `+0x6FC = 0xBF0016D4`
/// （= 站点 + `spec::ANCHOR_SITE_DELTA`），`ExternalLinkOpener 0xBF001A00`、
/// `APIAccess 0xBF001B00`。地址是虚构的，**关系**不是（与 P4 实测同形）。
const FIXTURE_GAME: u64 = 0xBF000FD8;
const FIXTURE_SITE: u64 = 0xBF0016B0;
const FIXTURE_ANCHOR: u64 = 0xBF0016D4;
const FIXTURE_ELO: u64 = 0xBF001A00;
const FIXTURE_API: u64 = 0xBF001B00;
const FIXTURE_HOP_API: u64 = 536;
const FIXTURE_HOP_GAME: u64 = 784;
const FIXTURE_GAME_MT: u64 = 0x0000_7ff9_e7ef_8970;

impl Context {
    fn path(&self, name: &str) -> PathBuf {
        self.fixtures.join(name)
    }

    fn read(&self, name: &str) -> Result<String, String> {
        let path = self.path(name);
        fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))
    }
}

type Case = (&'static str, fn(&Context) -> Result<String, String>);

fn cases() -> Vec<Case> {
    vec![
        ("names/canonical-table", case_names),
        ("sos/parse-transcript", case_parse_transcript),
        ("minidump/synthetic-roundtrip", case_minidump),
        ("minidump/anchor-miss", case_anchor_miss),
        ("chain/fixture-walk", case_chain),
        ("chain/type-mismatch", case_chain_type_mismatch),
        ("chain/resolve-multi-hop", case_resolve_multi_hop),
        ("chain/resolve-wrong-middle-hop", case_resolve_wrong_middle_hop),
        ("chain/candidate-validation", case_candidate_validation),
        ("deref/witness-clean", case_deref_clean),
        ("deref/witness-mismatch", case_deref_mismatch),
        ("emit/refuses-fixture-without-flag", case_refuse_fixture),
        ("emit/refuses-hand-written-provenance", case_refuse_provenance),
        ("emit/refuses-unchecked-dump", case_refuse_unchecked),
        ("emit/valid-fixture-table", case_emit_valid),
        ("emit/omits-missing-sos-row", case_omit_no_sos_row),
        ("emit/omits-missing-sos-type", case_omit_no_sos_type),
        ("emit/omits-ambiguous-row", case_omit_ambiguous),
        ("emit/omits-missing-il-row", case_omit_no_il_row),
        ("emit/omits-il-static", case_omit_il_static),
        ("emit/omits-il-type-mismatch", case_omit_il_type),
        ("emit/omits-il-explicit-offset", case_omit_il_explicit),
        ("emit/omits-deref-mismatch", case_omit_deref_mismatch),
        ("emit/version-mismatch-gate", case_version_gate),
        ("emit/refuses-empty-table", case_refuse_empty),
        ("runtime/parse-prints", case_runtime_parse),
        ("runtime/eetype-walk", case_runtime_walk),
        ("runtime/wrong-offset", case_runtime_wrong_offset),
        ("runtime/ambiguous", case_runtime_ambiguous),
        ("runtime/garbage-name", case_runtime_garbage_name),
        ("runtime/missing-row", case_runtime_missing_row),
        ("parse/malformed-intermediate", case_malformed),
        ("parse/intermediate-roundtrip", case_roundtrip),
        ("il/diff-structure", case_il_diff),
        ("discovery/select-lazer", case_discovery),
    ]
}

// ----------------------------------------------------------- 用例实现 ----

fn case_names(_context: &Context) -> Result<String, String> {
    let table: &[(&str, &str)] = &[
        ("System.String", "System.String"),
        ("osuTK.Vector2", "osuTK.Vector2"),
        (
            "osu.Game.Beatmaps.WorkingBeatmapCache+BeatmapManagerWorkingBeatmap",
            "osu.Game.Beatmaps.WorkingBeatmapCache+BeatmapManagerWorkingBeatmap",
        ),
        (
            "osu.Framework.Bindables.NonNullableBindable`1[[osu.Game.Beatmaps.WorkingBeatmap, osu.Game]]",
            "osu.Framework.Bindables.NonNullableBindable`1<osu.Game.Beatmaps.WorkingBeatmap>",
        ),
        (
            "osu.Framework.Bindables.BindableList`1[[osu.Game.Rulesets.Mods.Mod, osu.Game]]",
            "osu.Framework.Bindables.BindableList`1<osu.Game.Rulesets.Mods.Mod>",
        ),
        (
            "System.Collections.Generic.Dictionary`2[[System.String, System.Private.CoreLib],[System.Int32, System.Private.CoreLib]]",
            "System.Collections.Generic.Dictionary`2<System.String,System.Int32>",
        ),
        (
            "System.Collections.Generic.List`1[[System.String, System.Private.CoreLib]][]",
            "System.Collections.Generic.List`1<System.String>[]",
        ),
        ("..., osu.Framework]]", "...,osu.Framework]]"),
        (
            "osu.Framework.Bindables.Bindable`1[[osu.Game.Rulesets.RulesetInfo, osu.Game]]",
            "osu.Framework.Bindables.Bindable`1<osu.Game.Rulesets.RulesetInfo>",
        ),
    ];
    for (input, expected) in table {
        let actual = names::canonical_type(input);
        if actual != *expected {
            return Err(format!(
                "canonical_type({input:?}) = {actual:?}, expected {expected:?}"
            ));
        }
    }
    if names::open_generic("osu.Framework.Bindables.Bindable`1<A,B>") != "osu.Framework.Bindables.Bindable`1"
    {
        return Err("open_generic did not strip the argument list".to_string());
    }
    if names::open_generic("System.String") != "System.String" {
        return Err("open_generic changed a non-generic name".to_string());
    }
    if !names::is_truncated("..., osu.Framework]]") || names::is_truncated("System.String") {
        return Err("is_truncated misclassified".to_string());
    }
    Ok(format!("{} canonicalisation case(s)", table.len()))
}

fn case_parse_transcript(context: &Context) -> Result<String, String> {
    let text = context.read("EXAMPLE-fixture-sos-chain.txt")?;
    let parsed = sos::parse_transcript(&text);
    if !parsed.unparsed_rows.is_empty() {
        return Err(format!(
            "{} row(s) looked like fields but did not parse: {:?}",
            parsed.unparsed_rows.len(),
            parsed.unparsed_rows.first()
        ));
    }
    if parsed.noise_lines == 0 {
        return Err("expected some non-field output lines to be ignored".to_string());
    }
    let game = parsed
        .objects
        .iter()
        .find(|o| o.name == "osu.Desktop.OsuGameDesktop")
        .ok_or("the fixture has no OsuGameDesktop block")?;
    let storage = game
        .rows_named("<Storage>k__BackingField")
        .into_iter()
        .next()
        .ok_or("no <Storage> row")?;
    if storage.offset != 0x440 || storage.attr != "instance" || storage.vt != "No" {
        return Err(format!(
            "<Storage> parsed as offset {} attr {} vt {}",
            storage.offset, storage.attr, storage.vt
        ));
    }
    if !storage.sos_type.contains("Platform.Storage") {
        return Err(format!("<Storage> type parsed as {:?}", storage.sos_type));
    }
    let static_row = game
        .fields
        .iter()
        .find(|f| f.attr == "static")
        .ok_or("the fixture has no static row to parse")?;
    if static_row.name != "total_count" {
        return Err(format!("static row parsed as {:?}", static_row.name));
    }
    let string = parsed
        .objects
        .iter()
        .find(|o| o.string_value.is_some())
        .ok_or("no System.String block with a `String:` line")?;
    if string.string_value.as_deref() != Some("D:\\Games\\osu!lazer") {
        return Err(format!("string value parsed as {:?}", string.string_value));
    }
    let first_char = string
        .rows_named("_firstChar")
        .into_iter()
        .next()
        .ok_or("no _firstChar row")?;
    if first_char.offset != 0x0c || first_char.value.trim() != "44" {
        return Err(format!(
            "_firstChar parsed as offset {} value {:?}",
            first_char.offset, first_char.value
        ));
    }
    Ok(format!(
        "{} blocks / {} field rows",
        parsed.objects.len(),
        parsed.objects.iter().map(|o| o.fields.len()).sum::<usize>()
    ))
}

fn case_minidump(context: &Context) -> Result<String, String> {
    let blocks = fixture_blocks(context)?;
    let dump_path = context.work.join("EXAMPLE-fixture-dump.dmp");
    write_synthetic_dump(&dump_path, &blocks, Some(FIXTURE_ANCHOR), None)?;
    let mut dump = Dump::open(&dump_path)?;
    if dump.arch != "x64" {
        return Err(format!("arch parsed as {:?}", dump.arch));
    }
    if dump.pid != 1234 {
        return Err(format!("pid parsed as {}", dump.pid));
    }
    if dump.ranges.len() != 1 {
        return Err(format!("{} memory ranges, expected 1", dump.ranges.len()));
    }
    let module = dump
        .modules
        .iter()
        .find(|m| m.file_name().eq_ignore_ascii_case("osu!.dll"))
        .ok_or("osu!.dll module missing from the synthetic dump")?;
    if module.file_version.as_deref() != Some("2026.921.0.0") {
        return Err(format!(
            "module file version parsed as {:?}",
            module.file_version
        ));
    }
    let pattern = parse_pattern(crate::spec::ANCHOR_PATTERN)?;
    let scan = dump.scan(&pattern, 8, 4096)?;
    if scan.hits != vec![FIXTURE_ANCHOR] {
        return Err(format!("anchor scan hits = {:?}", scan.hits));
    }
    // GameBase 不是"锚点减一个常量"：锚点 − `spec::ANCHOR_SITE_DELTA` 只是**站点**，
    // 还要按 `spec::GAME_BASE_HOPS` 多跳才到 GameBase（同一份代码在真机上也走这条）。
    let resolution = chain::resolve_game_base(&mut dump, FIXTURE_ANCHOR, crate::spec::SITE_DELTAS);
    let (delta, site, game_base) = resolution
        .resolved
        .ok_or_else(|| format!("resolution failed: {:?}", resolution.attempts))?;
    if delta != crate::spec::ANCHOR_SITE_DELTA || site != FIXTURE_SITE || game_base != FIXTURE_GAME {
        return Err(format!(
            "resolution = delta {delta:#x} site {site:#x} game {game_base:#x}, expected \
             {:#x}/{FIXTURE_SITE:#x}/{FIXTURE_GAME:#x}",
            crate::spec::ANCHOR_SITE_DELTA
        ));
    }
    let string_addr = 0xBF000300u64;
    let value = dump
        .read_clr_string(string_addr, 0x08, 0x0C, 4096)
        .ok_or("read_clr_string failed on the planted string")?;
    if value != "D:\\Games\\osu!lazer" {
        return Err(format!("read_clr_string returned {value:?}"));
    }
    Ok(format!(
        "{} bytes, {} modules, 1 anchor hit at {FIXTURE_ANCHOR:#x}, multi-hop resolution to \
         {game_base:#x} (site {site:#x}), string layout +0x08/+0x0C verified",
        dump.bytes,
        dump.modules.len()
    ))
}

fn case_anchor_miss(context: &Context) -> Result<String, String> {
    let blocks = fixture_blocks(context)?;
    let dump_path = context.work.join("EXAMPLE-fixture-no-anchor.dmp");
    write_synthetic_dump(&dump_path, &blocks, None, None)?;
    let mut dump = Dump::open(&dump_path)?;
    let pattern = parse_pattern(crate::spec::ANCHOR_PATTERN)?;
    let scan = dump.scan(&pattern, 8, 4096)?;
    if !scan.hits.is_empty() {
        return Err(format!(
            "expected zero anchor hits in a dump without the pattern, got {:?}",
            scan.hits
        ));
    }
    Ok(format!("0 hits over {} bytes (the clear failure path)", scan.scanned))
}

fn case_chain(context: &Context) -> Result<String, String> {
    let (resolved, blocks) = walk_fixture(context)?;
    let mut bad: Vec<String> = Vec::new();
    for (label, item) in &resolved {
        if !item.status.starts_with("ok") {
            bad.push(format!("{label}: {}", item.status));
        }
    }
    if !bad.is_empty() {
        return Err(format!("chain steps not ok: {}", bad.join(" | ")));
    }
    let expected = crate::spec::CHAIN.len();
    if resolved.len() != expected {
        return Err(format!("{} steps resolved, expected {expected}", resolved.len()));
    }
    Ok(format!("{expected} steps ok over {} objects", blocks.len()))
}

fn case_chain_type_mismatch(context: &Context) -> Result<String, String> {
    let mut blocks = fixture_blocks(context)?;
    // 把"存储对象"的类型改成别的东西：`storage` 这一步必须失败，且它的子步骤必须被跳过。
    if let Some(object) = blocks.get_mut(&0xBF000200) {
        object.name = "osu.NotAStorage".to_string();
    }
    let game = blocks
        .get(&0xBF000FD8)
        .cloned()
        .ok_or("fixture has no game block")?;
    let mut source = FixtureBlocks { blocks };
    let (resolved, _) = chain::walk(&mut source, 0xBF000FD8, &game);
    let storage = resolved.get("storage").ok_or("no storage step")?;
    if !storage.status.starts_with("chain-type-mismatch") {
        return Err(format!("storage status = {}", storage.status));
    }
    let desktop = resolved.get("desktop_storage").ok_or("no desktop_storage step")?;
    if !desktop.status.starts_with("chain-skipped") {
        return Err(format!(
            "desktop_storage status = {} (expected chain-skipped)",
            desktop.status
        ));
    }
    Ok(format!("storage={}, desktop_storage={}", storage.status, desktop.status))
}

/// 多跳解析（P4 探针算法的移植）：`site → externalLinkOpener(+0x218) → APIAccess(+0x310) → GameBase`
/// 必须解通，且每一跳读出的地址都要与 fixture 里种下的值相等（不是"试出来的巧合"）。
fn case_resolve_multi_hop(context: &Context) -> Result<String, String> {
    // spec 的跳数/位移必须与 fixture（= P4 证据的誊写）一致：改了 spec 而没改 fixture ⇒ 这里失败。
    let hops: Vec<(&str, u64)> = crate::spec::GAME_BASE_HOPS
        .iter()
        .map(|hop| (hop.label, hop.offset))
        .collect();
    let expected_hops: Vec<(&str, u64)> = vec![
        ("external_link_opener", 0),
        ("api_access", FIXTURE_HOP_API),
        ("game", FIXTURE_HOP_GAME),
    ];
    if hops != expected_hops {
        return Err(format!(
            "spec::GAME_BASE_HOPS = {hops:?}, expected the P4 chain {expected_hops:?}"
        ));
    }
    let blocks = fixture_blocks(context)?;
    let dump_path = context.work.join("EXAMPLE-fixture-dump-multihop.dmp");
    write_synthetic_dump(&dump_path, &blocks, Some(FIXTURE_ANCHOR), None)?;
    let mut dump = Dump::open(&dump_path)?;
    let resolution = chain::resolve_game_base(&mut dump, FIXTURE_ANCHOR, crate::spec::SITE_DELTAS);

    let attempt = resolution
        .attempts
        .first()
        .ok_or("no attempt was recorded at all")?;
    if attempt.delta != crate::spec::ANCHOR_SITE_DELTA {
        return Err(format!(
            "first delta = {:#x}, expected spec::ANCHOR_SITE_DELTA = {:#x}",
            attempt.delta,
            crate::spec::ANCHOR_SITE_DELTA
        ));
    }
    if attempt.site != Some(FIXTURE_SITE) {
        return Err(format!("site = {:?}, expected {FIXTURE_SITE:#x}", attempt.site));
    }
    if attempt.external_link_opener != Some(FIXTURE_ELO)
        || attempt.api != Some(FIXTURE_API)
        || attempt.game_base != Some(FIXTURE_GAME)
    {
        return Err(format!(
            "hops read back as elo={:?} api={:?} game={:?}, expected {FIXTURE_ELO:#x}/{FIXTURE_API:#x}/{FIXTURE_GAME:#x} ({})",
            attempt.external_link_opener,
            attempt.api,
            attempt.game_base,
            attempt.path()
        ));
    }
    let (delta, site, game_base) = resolution
        .resolved
        .ok_or_else(|| format!("resolution reported no success: {:?}", resolution.attempts))?;
    if (delta, site, game_base) != (crate::spec::ANCHOR_SITE_DELTA, FIXTURE_SITE, FIXTURE_GAME) {
        return Err(format!(
            "resolved = delta {delta:#x} site {site:#x} game {game_base:#x}"
        ));
    }
    if resolution.candidates != vec![FIXTURE_GAME] {
        return Err(format!(
            "candidates = {:?}, expected exactly [{FIXTURE_GAME:#x}]",
            resolution
                .candidates
                .iter()
                .map(|c| format!("{c:#x}"))
                .collect::<Vec<_>>()
        ));
    }
    // 错误位移必须被挡下来（扫 delta 表的意义就在这里）：站点落在零字节上 ⇒ 指针不合理、无候选。
    let wrong_delta = 0x28i64;
    let wrong = chain::try_site(&mut dump, FIXTURE_ANCHOR, wrong_delta);
    if wrong.game_base.is_some() || !wrong.verdict.contains("implausible") {
        return Err(format!(
            "delta {wrong_delta:#x} produced game_base={:?} verdict={:?} (must fail closed)",
            wrong.game_base, wrong.verdict
        ));
    }
    // 多跳解出的候选必须同时过两条硬判据（类型 + 有表时的 vtable）。
    let game_object = blocks
        .get(&FIXTURE_GAME)
        .ok_or("the fixture has no game block to validate")?;
    let accepted = chain::candidate_verdict(
        game_object,
        dump.read_u64(FIXTURE_GAME),
        &[FIXTURE_GAME_MT],
    )
    .map_err(|reason| format!("the resolved candidate was rejected: {reason}"))?;
    Ok(format!(
        "delta {:#x} -> site {FIXTURE_SITE:#x} -> elo {FIXTURE_ELO:#x} -> api {FIXTURE_API:#x} -> game \
         {FIXTURE_GAME:#x}; {} delta(s) swept; delta {wrong_delta:#x} fails closed ({}); accepted: {accepted}",
        attempt.delta,
        resolution.attempts.len(),
        wrong.verdict
    ))
}

/// 中间跳被破坏（`APIAccess.game` 被改掉）⇒ **fail closed**：没有候选，
/// 判词逐字指出是哪一跳、读到了什么（前两跳仍要如实记下来）。
fn case_resolve_wrong_middle_hop(context: &Context) -> Result<String, String> {
    let blocks = fixture_blocks(context)?;
    let dump_path = context
        .work
        .join("EXAMPLE-fixture-dump-wrong-middle-hop.dmp");
    write_synthetic_dump(
        &dump_path,
        &blocks,
        Some(FIXTURE_ANCHOR),
        Some((FIXTURE_API, FIXTURE_HOP_GAME as i64)),
    )?;
    let mut dump = Dump::open(&dump_path)?;
    let resolution = chain::resolve_game_base(
        &mut dump,
        FIXTURE_ANCHOR,
        &[crate::spec::ANCHOR_SITE_DELTA],
    );
    if resolution.resolved.is_some() || !resolution.candidates.is_empty() {
        return Err(format!(
            "a corrupted middle hop still produced candidates {:?} / resolved {:?}",
            resolution
                .candidates
                .iter()
                .map(|c| format!("{c:#x}"))
                .collect::<Vec<_>>(),
            resolution.resolved
        ));
    }
    let attempt = resolution
        .attempts
        .first()
        .ok_or("no attempt was recorded")?;
    if attempt.site != Some(FIXTURE_SITE)
        || attempt.external_link_opener != Some(FIXTURE_ELO)
        || attempt.api != Some(FIXTURE_API)
    {
        return Err(format!(
            "the first two hops must still be recorded (site={:?} elo={:?} api={:?})",
            attempt.site, attempt.external_link_opener, attempt.api
        ));
    }
    if attempt.game_base != Some(0xEEEE_EEEE_EEEE_EEEE) {
        return Err(format!(
            "the failing hop should still be recorded verbatim (game_base={:?})",
            attempt.game_base
        ));
    }
    if attempt.resolved {
        return Err("the failed attempt is marked as resolved".to_string());
    }
    if !attempt.verdict.starts_with("game=0x") || !attempt.verdict.contains("implausible") {
        return Err(format!(
            "verdict {:?} does not pin the failing hop",
            attempt.verdict
        ));
    }
    Ok(format!(
        "no candidate; site {FIXTURE_SITE:#x} -> elo {FIXTURE_ELO:#x} -> api {FIXTURE_API:#x} kept, \
         the corrupted value is recorded but not accepted, verdict: {}",
        attempt.verdict
    ))
}

/// 候选验证的两条硬判据（`chain::candidate_verdict`）：类型不符 ⇒ 拒；给了表而 `[gameBase]`
/// 不符 ⇒ 拒；`dumpobj` 的 MethodTable 与 dump 字节不一致 ⇒ 拒；没有表 ⇒ 第 2 条如实记"不适用"。
fn case_candidate_validation(context: &Context) -> Result<String, String> {
    let blocks = fixture_blocks(context)?;
    let game_object = blocks
        .get(&FIXTURE_GAME)
        .cloned()
        .ok_or("the fixture has no game block to validate")?;
    if game_object.method_table != format!("{FIXTURE_GAME_MT:016x}") {
        return Err(format!(
            "fixture MethodTable {:?} != {FIXTURE_GAME_MT:016x}",
            game_object.method_table
        ));
    }
    let accepted = chain::candidate_verdict(&game_object, Some(FIXTURE_GAME_MT), &[FIXTURE_GAME_MT])
        .map_err(|reason| format!("the correct candidate was rejected: {reason}"))?;
    let no_table = chain::candidate_verdict(&game_object, Some(FIXTURE_GAME_MT), &[])
        .map_err(|reason| format!("a candidate was rejected without any table: {reason}"))?;
    if !no_table.contains("not applicable") {
        return Err(format!(
            "without a table the `[gameBase]` rule must be recorded as not applicable, got {no_table:?}"
        ));
    }
    let wrong_vtable = chain::candidate_verdict(&game_object, Some(FIXTURE_GAME_MT), &[FIXTURE_GAME_MT ^ 0x1_0000])
        .err()
        .ok_or("a wrong table vtable was accepted")?;
    let mut not_a_game = game_object.clone();
    not_a_game.name = "osu.Game.Beatmaps.BeatmapInfo".to_string();
    let type_reject = chain::candidate_verdict(&not_a_game, Some(FIXTURE_GAME_MT), &[FIXTURE_GAME_MT])
        .err()
        .ok_or("a wrong type was accepted")?;
    if !type_reject.contains("contains none of") {
        return Err(format!("type rejection reads {type_reject:?}"));
    }
    let mut inconsistent = game_object.clone();
    inconsistent.method_table = format!("{:016x}", FIXTURE_GAME_MT ^ 0x80);
    let base_reject = chain::candidate_verdict(
        &inconsistent,
        Some(FIXTURE_GAME_MT),
        &[FIXTURE_GAME_MT],
    )
    .err()
    .ok_or("a dumpobj/dump-byte disagreement was accepted")?;
    let unreadable = chain::candidate_verdict(&game_object, None, &[FIXTURE_GAME_MT])
        .err()
        .ok_or("an unreadable [gameBase] was accepted although a table was supplied")?;
    Ok(format!(
        "accepted={accepted:?}; no-table={no_table:?}; rejected: [{wrong_vtable}; {type_reject}; \
         {base_reject}; {unreadable}]"
    ))
}

fn case_deref_clean(context: &Context) -> Result<String, String> {
    let (intermediate, _) = intermediate_from_fixture(context, "EXAMPLE-fixture-dump-clean.dmp", None)?;
    let mismatch = intermediate.get("deref_mismatch").unwrap_or("?");
    let ok = intermediate
        .get("deref_ok")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);
    if mismatch != "0" {
        let rows: Vec<String> = intermediate
            .rows
            .iter()
            .filter(|row| row.deref.starts_with("mismatch"))
            .map(|row| format!("{}.{} @{} -> {}", row.object_type, row.field, row.offset, row.deref))
            .collect();
        return Err(format!(
            "deref_mismatch = {mismatch} on a clean synthetic dump: {}",
            rows.join(" | ")
        ));
    }
    if ok == 0 {
        return Err("no field row was dereferenced at all".to_string());
    }
    Ok(format!("{ok} row(s) proven against the dump bytes, 0 mismatches"))
}

fn case_deref_mismatch(context: &Context) -> Result<String, String> {
    // 把 game.<Beatmap> 指向的指针在 dump 里改掉 ⇒ 这一行必须报 mismatch。
    let (intermediate, _) = intermediate_from_fixture(
        context,
        "EXAMPLE-fixture-dump-corrupt.dmp",
        Some((0xBF000FD8, 0x450)),
    )?;
    let row = intermediate
        .rows
        .iter()
        .find(|row| row.address == 0xBF000FD8 && row.field == "<Beatmap>k__BackingField")
        .ok_or("no <Beatmap> row in the intermediate")?;
    if !row.deref.starts_with("mismatch") {
        return Err(format!("<Beatmap> deref status = {}", row.deref));
    }
    let total = intermediate.get("deref_mismatch").unwrap_or("0");
    if total == "0" {
        return Err("deref_mismatch counter stayed 0".to_string());
    }
    Ok(format!("<Beatmap> -> {} (counter {total})", row.deref))
}

fn case_refuse_fixture(context: &Context) -> Result<String, String> {
    let (intermediate, il) = inputs(context)?;
    match emit::emit(&intermediate, &il, false, false) {
        Ok(_) => Err("emit accepted a fixture without --allow-fixtures".to_string()),
        Err(emit::Refusal(message)) if message.contains("fixture") => {
            Ok(format!("refused: {}", first_line(&message)))
        }
        Err(emit::Refusal(message)) => Err(format!("refused for the wrong reason: {message}")),
    }
}

fn case_refuse_provenance(context: &Context) -> Result<String, String> {
    let (mut intermediate, il) = inputs(context)?;
    intermediate
        .provenance
        .insert("provenance".to_string(), "hand-written".to_string());
    intermediate
        .provenance
        .insert("fixture".to_string(), "false".to_string());
    // `--allow-fixtures` 打开（否则先撞的是 fixture 门）：这里要证的是**溯源门**。
    match emit::emit(&intermediate, &il, true, false) {
        Ok(_) => Err("emit accepted a hand-written provenance".to_string()),
        Err(emit::Refusal(message)) if message.contains("provenance") => {
            Ok(format!("refused: {}", first_line(&message)))
        }
        Err(emit::Refusal(message)) => Err(format!("refused for the wrong reason: {message}")),
    }
}

fn case_refuse_unchecked(context: &Context) -> Result<String, String> {
    let (mut intermediate, il) = inputs(context)?;
    intermediate
        .provenance
        .insert("provenance".to_string(), "dump".to_string());
    intermediate
        .provenance
        .insert("fixture".to_string(), "false".to_string());
    intermediate
        .provenance
        .insert("deref_checked".to_string(), "0".to_string());
    match emit::emit(&intermediate, &il, true, false) {
        Ok(_) => Err("emit accepted a dump-derived table with deref_checked=0".to_string()),
        Err(emit::Refusal(message)) if message.contains("deref_checked") => {
            Ok(format!("refused: {}", first_line(&message)))
        }
        Err(emit::Refusal(message)) => Err(format!("refused for the wrong reason: {message}")),
    }
}

fn case_emit_valid(context: &Context) -> Result<String, String> {
    let (intermediate, il) = inputs(context)?;
    let outcome = emit::emit(&intermediate, &il, true, false)
        .map_err(|emit::Refusal(message)| format!("emit refused: {message}"))?;
    // 先把证据落盘（报告 + 表），再断言——失败时能直接看到报告里的 omitted 清单。
    let json_path = context
        .work
        .join(format!("EXAMPLE-FIXTURE-{}.json", "selftest-valid"));
    fs::write(&json_path, outcome.to_json())
        .map_err(|e| format!("write {}: {e}", json_path.display()))?;
    let report_path = context.work.join("EXAMPLE-fixture-emit-report.txt");
    fs::write(
        &report_path,
        outcome.report("<fixture sos-intermediate>", "<fixture il-inventory>"),
    )
    .map_err(|e| format!("write {}: {e}", report_path.display()))?;
    // 中间件也落盘一份（fixture 标记齐全）：这样 `emit` 的 fixture 门可以用**命令行**再验一次。
    intermediate
        .write_tsv(&context.work.join("EXAMPLE-fixture-sos-intermediate.tsv"))
        .map_err(|e| format!("write fixture intermediate: {e}"))?;
    if !outcome.is_fixture() {
        return Err("the emitted outcome is not marked as a fixture".to_string());
    }
    if !outcome.file_name().starts_with("EXAMPLE-FIXTURE-") {
        return Err(format!("file name {:?} lacks the fixture prefix", outcome.file_name()));
    }
    if !outcome.evidence.contains(emit::FIXTURE_MARK) {
        return Err("`evidence` lacks the fixture mark".to_string());
    }
    let expected_offsets: &[(&str, &str, i64)] = &[
        ("osu.Game.Beatmaps.BeatmapInfo", "<MD5Hash>k__BackingField", 88),
        ("osu.Game.Beatmaps.BeatmapInfo", "<Length>k__BackingField", 112),
        ("osu.Game.Beatmaps.BeatmapDifficulty", "<CircleSize>k__BackingField", 44),
        ("osu.Game.IO.OsuStorage", "<BasePath>k__BackingField", 8),
        ("System.String", "_stringLength", 8),
        ("System.String", "_firstChar", 12),
        (
            "osu.Framework.Bindables.NonNullableBindable`1<osu.Game.Beatmaps.WorkingBeatmap>",
            "value",
            32,
        ),
        (
            "osu.Framework.Bindables.Bindable`1<System.Collections.Generic.IReadOnlyList`1<osu.Game.Rulesets.Mods.Mod>>",
            "value",
            32,
        ),
    ];
    let types = outcome.types();
    for (type_name, field, offset) in expected_offsets {
        match types.get(*type_name).and_then(|fields| fields.get(*field)) {
            Some(value) if *value == *offset => {}
            other => {
                return Err(format!(
                    "table[{type_name}][{field}] = {other:?}, expected {offset}"
                ))
            }
        }
    }
    // JSON 必须真的是 JSON：用自带的解析器读回来，并核对与内存结构的逐字段一致。
    let text = fs::read_to_string(&json_path).map_err(|e| e.to_string())?;
    let parsed = parse_json(&text)?;
    let root = parsed.as_object().ok_or("the emitted table is not a JSON object")?;
    for key in [
        "lazer_version",
        "runtime_version",
        "arch",
        "game_base_vtable",
        "types",
        "verified_build",
        "evidence",
    ] {
        if !root.contains_key(key) {
            return Err(format!("the emitted JSON has no `{key}` key"));
        }
    }
    let json_types = root
        .get("types")
        .and_then(|value| value.as_object())
        .ok_or("`types` is not an object")?;
    for (type_name, fields) in &types {
        let json_fields = json_types
            .get(type_name)
            .and_then(|value| value.as_object())
            .ok_or_else(|| format!("`types.{type_name}` missing from the JSON"))?;
        for (field, offset) in fields {
            let value = json_fields
                .get(field)
                .and_then(|value| value.as_number())
                .ok_or_else(|| format!("`types.{type_name}.{field}` is not a number"))?;
            if value != *offset as f64 {
                return Err(format!(
                    "JSON says {type_name}.{field} = {value}, memory says {offset}"
                ));
            }
        }
    }
    if let Some(vtable) = root.get("game_base_vtable").and_then(|v| v.as_number()) {
        if vtable != 0x0000_7ff9_e7ef_8970u64 as f64 {
            return Err(format!("game_base_vtable = {vtable}"));
        }
    } else {
        return Err("game_base_vtable is not a number".to_string());
    }
    Ok(format!(
        "{} field(s) over {} type(s); JSON parsed back; fixture marks present ({})",
        outcome.published.len(),
        types.len(),
        json_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default()
    ))
}

fn case_omit_no_sos_row(context: &Context) -> Result<String, String> {
    let (mut intermediate, il) = inputs(context)?;
    intermediate
        .rows
        .retain(|row| !(row.address == 0xBF000600 && row.field == "<Hash>k__BackingField"));
    let outcome = emit_fixture(&intermediate, &il)?;
    expect_omitted(&outcome, "beatmap_info", "<Hash>k__BackingField", "no-sos-row")
}

fn case_omit_no_sos_type(context: &Context) -> Result<String, String> {
    let (mut intermediate, il) = inputs(context)?;
    for row in intermediate.rows.iter_mut() {
        if row.address == 0xBF001200 && row.field == "<CircleSize>k__BackingField" {
            row.sos_type = String::new();
        }
    }
    let outcome = emit_fixture(&intermediate, &il)?;
    expect_omitted(&outcome, "difficulty", "<CircleSize>k__BackingField", "sos-type-missing")
}

fn case_omit_ambiguous(context: &Context) -> Result<String, String> {
    let (mut intermediate, il) = inputs(context)?;
    let row = intermediate
        .rows
        .iter()
        .find(|row| row.address == 0xBF000600 && row.field == "<MD5Hash>k__BackingField")
        .cloned()
        .ok_or("no <MD5Hash> row to duplicate")?;
    let mut second = row.clone();
    second.offset = 0x60; // 同一个字段、另一个偏移 ⇒ 歧义
    intermediate.rows.push(second);
    let outcome = emit_fixture(&intermediate, &il)?;
    expect_omitted(&outcome, "beatmap_info", "<MD5Hash>k__BackingField", "ambiguous-sos-row")
}

fn case_omit_no_il_row(context: &Context) -> Result<String, String> {
    let (intermediate, mut il) = inputs(context)?;
    il.fields
        .retain(|row| !(row.type_name == "osu.Game.Beatmaps.BeatmapMetadata" && row.field == "<Title>k__BackingField"));
    let outcome = emit_fixture(&intermediate, &il)?;
    expect_omitted(&outcome, "metadata", "<Title>k__BackingField", "no-il-row")
}

fn case_omit_il_static(context: &Context) -> Result<String, String> {
    let (intermediate, mut il) = inputs(context)?;
    for row in il.fields.iter_mut() {
        if row.type_name == "osu.Game.Beatmaps.BeatmapDifficulty"
            && row.field == "<ApproachRate>k__BackingField"
        {
            row.kind = "static".to_string();
        }
    }
    let outcome = emit_fixture(&intermediate, &il)?;
    expect_omitted(&outcome, "difficulty", "<ApproachRate>k__BackingField", "il-static")
}

fn case_omit_il_type(context: &Context) -> Result<String, String> {
    let (intermediate, mut il) = inputs(context)?;
    for row in il.fields.iter_mut() {
        if row.type_name == "osu.Game.Beatmaps.BeatmapInfo"
            && row.field == "<MD5Hash>k__BackingField"
        {
            row.type_display = "System.Int32".to_string();
            row.type_kind = "prim".to_string();
        }
    }
    let outcome = emit_fixture(&intermediate, &il)?;
    expect_omitted(&outcome, "beatmap_info", "<MD5Hash>k__BackingField", "il-type-mismatch")
}

fn case_omit_il_explicit(context: &Context) -> Result<String, String> {
    let (intermediate, mut il) = inputs(context)?;
    for row in il.fields.iter_mut() {
        if row.type_name == "osu.Game.Models.RealmUser" && row.field == "<Username>k__BackingField"
        {
            row.explicit_offset = Some(999);
        }
    }
    let outcome = emit_fixture(&intermediate, &il)?;
    expect_omitted(
        &outcome,
        "realm_user",
        "<Username>k__BackingField",
        "il-explicit-offset-mismatch",
    )
}

fn case_omit_deref_mismatch(context: &Context) -> Result<String, String> {
    let (mut intermediate, il) = inputs(context)?;
    for row in intermediate.rows.iter_mut() {
        if row.address == 0xBF000FD8 && row.field == "<VersionHash>k__BackingField" {
            row.deref = "mismatch-ref:0x1!=0x2".to_string();
        }
    }
    let outcome = emit_fixture(&intermediate, &il)?;
    expect_omitted(&outcome, "game", "<VersionHash>k__BackingField", "deref-mismatch")
}

fn case_version_gate(context: &Context) -> Result<String, String> {
    let (intermediate, mut il) = inputs(context)?;
    il.provenance.insert(
        "assembly:osu!.dll".to_string(),
        "2099.1.0.0\t1 fields\t<example>".to_string(),
    );
    match emit::emit(&intermediate, &il, true, false) {
        Ok(_) => return Err("emit accepted dump/IL version mismatch".to_string()),
        Err(emit::Refusal(message)) if message.contains("version mismatch") => {}
        Err(emit::Refusal(message)) => {
            return Err(format!("refused for the wrong reason: {message}"))
        }
    }
    let outcome = emit::emit(&intermediate, &il, true, true)
        .map_err(|emit::Refusal(message)| format!("override still refused: {message}"))?;
    if !outcome
        .warnings
        .iter()
        .any(|warning| warning.contains("version mismatch"))
    {
        return Err("the override did not record a warning".to_string());
    }
    if !outcome.evidence.contains("version mismatch") {
        return Err("the override is not visible in `evidence`".to_string());
    }
    Ok("refused, and with --allow-version-mismatch the mismatch is recorded".to_string())
}

fn case_refuse_empty(context: &Context) -> Result<String, String> {
    let (mut intermediate, il) = inputs(context)?;
    intermediate.rows.clear();
    match emit::emit(&intermediate, &il, true, false) {
        Ok(_) => Err("emit produced a table with zero fields".to_string()),
        Err(emit::Refusal(message)) if message.contains("no field survived") => {
            Ok(format!("refused: {}", first_line(&message)))
        }
        Err(emit::Refusal(message)) => Err(format!("refused for the wrong reason: {message}")),
    }
}

fn case_malformed(context: &Context) -> Result<String, String> {
    let path = context.path("EXAMPLE-fixture-malformed.tsv");
    match SosIntermediate::read_tsv(&path) {
        Ok(_) => Err("a malformed intermediate was accepted".to_string()),
        Err(message) => {
            if !message.contains("11 columns") && !message.contains("needs 12 columns") {
                return Err(format!("unexpected error text: {message}"));
            }
            Ok(format!("rejected: {}", first_line(&message)))
        }
    }
}

fn case_roundtrip(context: &Context) -> Result<String, String> {
    // 中间件「写盘 → 读回 → 再写盘」必须逐字节稳定：列序错位这类错误只有这条路能抓到
    // （内存里构造的中间件不会经过 TSV 读写）。
    let (intermediate, _) =
        intermediate_from_fixture(context, "EXAMPLE-fixture-dump-roundtrip.dmp", None)?;
    let first = context.work.join("EXAMPLE-fixture-roundtrip-1.tsv");
    let second = context.work.join("EXAMPLE-fixture-roundtrip-2.tsv");
    intermediate
        .write_tsv(&first)
        .map_err(|e| format!("write #1: {e}"))?;
    let read_back = SosIntermediate::read_tsv(&first)?;
    if read_back.rows.len() != intermediate.rows.len() {
        return Err(format!(
            "{} rows written but {} read back",
            intermediate.rows.len(),
            read_back.rows.len()
        ));
    }
    if read_back.objects.len() != intermediate.objects.len() {
        return Err(format!(
            "{} objects written but {} read back",
            intermediate.objects.len(),
            read_back.objects.len()
        ));
    }
    read_back
        .write_tsv(&second)
        .map_err(|e| format!("write #2: {e}"))?;
    let first_text = fs::read_to_string(&first).map_err(|e| e.to_string())?;
    let second_text = fs::read_to_string(&second).map_err(|e| e.to_string())?;
    if first_text != second_text {
        let position = first_text
            .lines()
            .zip(second_text.lines())
            .position(|(a, b)| a != b);
        return Err(format!(
            "the re-written intermediate differs from the first write (first differing line: {:?})",
            position.map(|index| index + 1)
        ));
    }
    Ok(format!(
        "{} field rows / {} objects round-trip byte-identically",
        read_back.rows.len(),
        read_back.objects.len()
    ))
}

fn case_il_diff(context: &Context) -> Result<String, String> {
    let old = IlInventory::read_tsv(&context.path("EXAMPLE-fixture-il.tsv"))?;
    let mut new = old.clone();
    // 加一个字段、删一个字段、改一个字段类型 —— diff 必须逐条报出来。
    new.fields.retain(|row| {
        !(row.type_name == "osu.Game.Beatmaps.BeatmapInfo" && row.field == "<Hash>k__BackingField")
    });
    for row in new.fields.iter_mut() {
        if row.type_name == "osu.Game.Beatmaps.BeatmapDifficulty"
            && row.field == "<CircleSize>k__BackingField"
        {
            row.type_display = "System.Double".to_string();
        }
    }
    new.fields.push(ilmeta::IlField {
        assembly: "osu.Game.dll".to_string(),
        type_name: "osu.Game.Beatmaps.BeatmapInfo".to_string(),
        field: "<NewField>k__BackingField".to_string(),
        kind: "instance".to_string(),
        type_kind: "prim".to_string(),
        type_display: "System.Int32".to_string(),
        token: "04009999".to_string(),
        explicit_offset: None,
        order: 99,
    });
    let text = ilmeta::diff(&old, &new);
    for needle in [
        "osu.Game.Beatmaps.BeatmapInfo\t<NewField>k__BackingField",
        "osu.Game.Beatmaps.BeatmapInfo\t<Hash>k__BackingField",
        "type System.Single -> System.Double",
    ] {
        if !text.contains(needle) {
            return Err(format!("the diff does not mention {needle:?}"));
        }
    }
    let path = context.work.join("EXAMPLE-fixture-il-diff.txt");
    fs::write(&path, &text).map_err(|e| e.to_string())?;
    Ok(format!(
        "added/removed/changed reported ({}), diff at {}",
        first_line(text.lines().nth(3).unwrap_or("")),
        path.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default()
    ))
}

fn case_discovery(_context: &Context) -> Result<String, String> {
    let empty: Vec<Candidate> = Vec::new();
    let message = select_lazer(&empty).err().unwrap_or_default();
    if !message.contains("lazer-not-running") || !message.contains("dotnet-dump") {
        return Err(format!("empty candidate message = {message:?}"));
    }
    let stable = vec![Candidate {
        pid: 111,
        path: PathBuf::from("D:\\Games\\osu!\\osu!.exe"),
        machine: Some(0x014C),
    }];
    let message = select_lazer(&stable).err().unwrap_or_default();
    if !message.contains("32-bit") {
        return Err(format!("stable-only message = {message:?}"));
    }
    let two = vec![
        Candidate {
            pid: 1,
            path: PathBuf::from("C:\\a\\osu!.exe"),
            machine: Some(0x8664),
        },
        Candidate {
            pid: 2,
            path: PathBuf::from("C:\\b\\osu!.exe"),
            machine: Some(0x8664),
        },
    ];
    let message = select_lazer(&two).err().unwrap_or_default();
    if !message.contains("multiple-instances") {
        return Err(format!("two-instance message = {message:?}"));
    }
    let one = vec![Candidate {
        pid: 4242,
        path: PathBuf::from("C:\\osulazer\\current\\osu!.exe"),
        machine: Some(0x8664),
    }];
    let target = select_lazer(&one).map_err(|e| format!("single lazer rejected: {e}"))?;
    if target.pid != 4242 {
        return Err(format!("selected pid {}", target.pid));
    }
    Ok("empty/stable-only/two-instances rejected with their own messages; single x64 selected".to_string())
}

// --------------------------------------------------------------- 辅助 ----

// ------------------------------------------------- 运行期结构（Step 10f）自测 ----

/// 从 `EXAMPLE-fixture-runtime.txt` 里取一段（`--- … ---` 之间的行；前缀匹配）。
fn runtime_section(context: &Context, prefix: &str) -> Result<String, String> {
    let text = context.read("EXAMPLE-fixture-runtime.txt")?;
    let mut out = String::new();
    let mut inside = false;
    for line in text.lines() {
        if let Some(title) = line.strip_prefix("--- ") {
            inside = title.trim_end_matches(" ---").trim().starts_with(prefix);
            continue;
        }
        if inside {
            out.push_str(line);
            out.push('\n');
        }
    }
    if out.trim().is_empty() {
        return Err(format!("fixture runtime section {prefix:?} is empty"));
    }
    Ok(out)
}

/// 合成 dump 里"结构块"的地址与几何（自测专用；真实 dump 上的数值见 `spec.rs::RUNTIME_PROBES`）。
const RT_SCREEN_A: u64 = 0xBF00_0200;
const RT_SCREEN_B: u64 = 0xBF00_0240;
const RT_SCREEN_C: u64 = 0xBF00_0280;
const RT_EETYPE_A: u64 = 0xBF00_0400;
const RT_EETYPE_A2: u64 = 0xBF00_0500;
const RT_EETYPE_B: u64 = 0xBF00_0600;
const RT_MODULE_A: u64 = 0xBF00_0800;
const RT_MODULE_B: u64 = 0xBF00_0900;
const RT_ARRAY: u64 = 0xBF00_0A00;
/// 自测里"真值"：位移与取值（推导必须把它们**搜出来**，而不是从常量读）。
const RT_TOKEN_OFFSET: i64 = 0x08;
const RT_MODULE_OFFSET: i64 = 0x18;
const RT_IMAGE_BASE_OFFSET: i64 = 0xC8;
const RT_ARRAY_DATA_OFFSET: i64 = 0x10;
const RT_MODULE_A_ADDRESS: u64 = 0x0000_7FFE_2F05_8918;
const RT_MODULE_B_ADDRESS: u64 = 0x0000_7FFE_2EFA_8B20;
const RT_IMAGE_BASE_A: u64 = 0x0000_01D7_07F6_0000;
const RT_IMAGE_BASE_B: u64 = 0x0000_01D7_0772_0000;
const RT_RID_A: u32 = 0x09A2;
const RT_RID_A2: u32 = 0x0998;
const RT_RID_B: u32 = 0x0F;

/// 一个合成结构块（`write_synthetic_dump` 只认 `MethodTable` + 字段的 `Offset`/`Value`/类型宽度）。
fn fake_object(
    _address: u64,
    name: &str,
    method_table: u64,
    fields: &[(&str, i64, &str, &str, &str)],
) -> sos::SosObject {
    sos::SosObject {
        name: name.to_string(),
        method_table: format!("{method_table:016x}"),
        size_text: String::new(),
        size: 0,
        file: String::new(),
        string_value: None,
        fields: fields
            .iter()
            .map(|(field, offset, sos_type, vt, value)| sos::SosField {
                object_type: name.to_string(),
                mt: "0000000000000000".to_string(),
                token: "04000000".to_string(),
                offset: *offset,
                sos_type: (*sos_type).to_string(),
                vt: (*vt).to_string(),
                attr: "instance".to_string(),
                value: (*value).to_string(),
                name: (*field).to_string(),
                raw: format!("{field} {offset} {value}"),
            })
            .collect(),
    }
}

/// 合成结构镜像：三枚屏幕对象（`[obj]` = 三个 EEType）＋ 三个 EEType ＋ 两个 Module ＋ 一个数组。
///
/// `token_offset_a2` / `duplicate_token` 用来制造失败路径（位移搜不出来 / 有歧义）。
fn runtime_blocks(token_offset_a2: i64, duplicate_token: bool) -> BTreeMap<u64, sos::SosObject> {
    let mut blocks: BTreeMap<u64, sos::SosObject> = BTreeMap::new();
    // 屏幕对象：`MethodTable:` 行就是 `[obj]`（dump 写入器把它种在对象地址上）。
    blocks.insert(RT_SCREEN_A, fake_object(RT_SCREEN_A, "osu.Game.Screens.Menu.MainMenu", RT_EETYPE_A, &[]));
    blocks.insert(RT_SCREEN_B, fake_object(RT_SCREEN_B, "osu.Game.Screens.Play.Player", RT_EETYPE_A2, &[]));
    blocks.insert(RT_SCREEN_C, fake_object(RT_SCREEN_C, "osu.Desktop.OsuGameDesktop", RT_EETYPE_B, &[]));
    let packed = |rid: u32, flags: u32| -> String { format!("{}", (rid << 8) | flags) };
    // EEType：token（打包的 RID）与 loader Module 两个字段。
    let eetype = |address: u64, name: &str, rid: u32, flags: u32, token_offset: i64, module: u64| {
        let mut fields = vec![
            ("token", token_offset, "System.UInt32", "Yes", packed(rid, flags)),
            ("loaderModule", RT_MODULE_OFFSET, "System.UInt64", "Yes", format!("{module}")),
        ];
        if duplicate_token {
            // 同一个打包值出现在两个位移 ⇒ 推导必须判"歧义"并拒绝。
            fields.push(("tokenDup", RT_TOKEN_OFFSET + 4, "System.UInt32", "Yes", packed(rid, flags)));
        }
        let owned: Vec<(String, i64, String, String, String)> = fields
            .into_iter()
            .map(|(field, offset, ty, vt, value)| {
                (field.to_string(), offset, ty.to_string(), vt.to_string(), value)
            })
            .collect();
        let refs: Vec<(&str, i64, &str, &str, &str)> = owned
            .iter()
            .map(|(field, offset, ty, vt, value)| {
                (field.as_str(), *offset, ty.as_str(), vt.as_str(), value.as_str())
            })
            .collect();
        fake_object(address, name, 0x7FF9_0000_0001, &refs)
    };
    blocks.insert(
        RT_EETYPE_A,
        eetype(RT_EETYPE_A, "osu.Game.Screens.Menu.MainMenu", RT_RID_A, 0x04, RT_TOKEN_OFFSET, RT_MODULE_A_ADDRESS),
    );
    blocks.insert(
        RT_EETYPE_A2,
        eetype(RT_EETYPE_A2, "osu.Game.Screens.Play.Player", RT_RID_A2, 0x04, token_offset_a2, RT_MODULE_A_ADDRESS),
    );
    blocks.insert(
        RT_EETYPE_B,
        eetype(RT_EETYPE_B, "osu.Desktop.OsuGameDesktop", RT_RID_B, 0x05, RT_TOKEN_OFFSET, RT_MODULE_B_ADDRESS),
    );
    // Module 对象：映像基址。
    blocks.insert(
        RT_MODULE_A,
        fake_object(RT_MODULE_A, "<module>", 0x7FFE_8ED2_E1E0, &[("imageBase", RT_IMAGE_BASE_OFFSET, "System.UInt64", "Yes", &format!("{RT_IMAGE_BASE_A}"))]),
    );
    blocks.insert(
        RT_MODULE_B,
        fake_object(RT_MODULE_B, "<module>", 0x7FFE_8ED2_E1E0, &[("imageBase", RT_IMAGE_BASE_OFFSET, "System.UInt64", "Yes", &format!("{RT_IMAGE_BASE_B}"))]),
    );
    // 数组对象：长度 + 三个元素（`dumparray` 印出来的那三个地址）。
    blocks.insert(
        RT_ARRAY,
        fake_object(
            RT_ARRAY,
            "osu.Framework.Screens.IScreen[]",
            0x7FFE_30FC_67B0,
            &[
                ("length", 0x08, "System.Int32", "Yes", "3"),
                ("e0", RT_ARRAY_DATA_OFFSET, "", "No", &format!("{RT_SCREEN_A:016X}")),
                ("e1", RT_ARRAY_DATA_OFFSET + 8, "", "No", &format!("{RT_SCREEN_B:016X}")),
                ("e2", RT_ARRAY_DATA_OFFSET + 16, "", "No", &format!("{RT_SCREEN_C:016X}")),
            ],
        ),
    );
    blocks
}

/// 三个 EEType 探针（`expected` 是 `dumpmt` 印的 RID / Module）。
fn runtime_probes() -> (Vec<runtime::Probe>, Vec<runtime::Probe>) {
    let token = vec![
        runtime::Probe { address: RT_EETYPE_A, expected: RT_RID_A as u64, source: "dumpmt A".to_string() },
        runtime::Probe { address: RT_EETYPE_A2, expected: RT_RID_A2 as u64, source: "dumpmt A2".to_string() },
        runtime::Probe { address: RT_EETYPE_B, expected: RT_RID_B as u64, source: "dumpmt B".to_string() },
    ];
    let module = vec![
        runtime::Probe { address: RT_EETYPE_A, expected: RT_MODULE_A_ADDRESS, source: "dumpmt A".to_string() },
        runtime::Probe { address: RT_EETYPE_A2, expected: RT_MODULE_A_ADDRESS, source: "dumpmt A2".to_string() },
        runtime::Probe { address: RT_EETYPE_B, expected: RT_MODULE_B_ADDRESS, source: "dumpmt B".to_string() },
    ];
    (token, module)
}

fn runtime_probe(group: &str, name: &str) -> &'static crate::spec::RuntimeProbe {
    crate::spec::RUNTIME_PROBES
        .iter()
        .find(|probe| probe.group == group && probe.name == name)
        .expect("spec has the probe")
}

/// `runtime/parse-prints`：三段打印的解析（含"名字为空"的那一段）。
fn case_runtime_parse(context: &Context) -> Result<String, String> {
    let mts = sos::parse_dumpmt(&runtime_section(context, "dumpmt (synthetic EEType A")?);
    let first = mts.first().ok_or("dumpmt section parsed to 0 blocks")?;
    if first.rid() != Some(0x09A2) || first.module_address() != Some(0x0000_7FFE_2F05_8918) {
        return Err(format!("dumpmt A parsed as rid={:?} module={:?}", first.rid(), first.module_address()));
    }
    if first.name != "osu.Game.Screens.Menu.MainMenu" || first.module_file() != "osu.Game.dll" {
        return Err(format!("dumpmt A name={:?} file={:?}", first.name, first.module_file()));
    }
    if first.base_size_value() != Some(0x468) {
        return Err(format!("BaseSize parsed as {:?}", first.base_size_value()));
    }
    let empty = sos::parse_dumpmt(&runtime_section(context, "dumpmt (empty name")?);
    if !empty.first().map(|mt| mt.name.trim().is_empty()).unwrap_or(false) {
        return Err("the empty-name dumpmt block did not parse as an empty name".to_string());
    }
    let module = sos::parse_dumpmodule(&runtime_section(context, "dumpmodule (osu.Game.dll")?)
        .ok_or("dumpmodule section unparsable")?;
    if module.base_address_value() != Some(0x0000_01D7_07F6_0000) || module.module_file() != "osu.Game.dll" {
        return Err(format!(
            "dumpmodule parsed as base={:?} file={:?}",
            module.base_address_value(),
            module.module_file()
        ));
    }
    let array = sos::parse_dumparray(&runtime_section(context, "dumparray")?)
        .ok_or("dumparray section unparsable")?;
    let expected = vec![Some(RT_SCREEN_A), Some(RT_SCREEN_B), Some(RT_SCREEN_C)];
    if array.elements != 3 || array.items != expected {
        return Err(format!(
            "dumparray parsed as {} element(s) {:?}, expected 3 {expected:?}",
            array.elements, array.items
        ));
    }
    Ok(format!(
        "3 printers parsed: dumpmt rid {:#X}/module {:#x}, dumpmodule base {:#x}, dumparray {} element(s)",
        0x09A2,
        0x0000_7FFE_2F05_8918u64,
        0x0000_01D7_07F6_0000u64,
        array.elements
    ))
}

/// `runtime/eetype-walk`：位移**搜出来**（token/module/image base/数组布局），
/// 且 emit 能把它们写进表的 `runtime` 段（含 RID→类型名的双见证收录）。
fn case_runtime_walk(context: &Context) -> Result<String, String> {
    let blocks = runtime_blocks(RT_TOKEN_OFFSET, false);
    let dump_path = context.work.join("EXAMPLE-fixture-runtime.dmp");
    write_synthetic_dump(&dump_path, &blocks, None, None)?;
    let mut dump = Dump::open(&dump_path)?;
    let (token_probes, module_probes) = runtime_probes();
    let derived_token =
        runtime::find_u32_shifted(&mut dump, RT_EETYPE_A, runtime_probe("eetype", "token"), &token_probes)?;
    if derived_token.offset != RT_TOKEN_OFFSET {
        return Err(format!(
            "eetype.token derived at +{:#x}, planted at +{RT_TOKEN_OFFSET:#x} ({})",
            derived_token.offset, derived_token.witness
        ));
    }
    let derived_module = runtime::find_u64(&mut dump, runtime_probe("eetype", "loader_module"), &module_probes)?;
    if derived_module.offset != RT_MODULE_OFFSET {
        return Err(format!("eetype.loader_module derived at +{:#x}", derived_module.offset));
    }
    let image_probes = vec![
        runtime::Probe { address: RT_MODULE_A, expected: RT_IMAGE_BASE_A, source: "dumpmodule A".to_string() },
        runtime::Probe { address: RT_MODULE_B, expected: RT_IMAGE_BASE_B, source: "dumpmodule B".to_string() },
    ];
    let derived_image = runtime::find_u64(&mut dump, runtime_probe("module", "image_base"), &image_probes)?;
    if derived_image.offset != RT_IMAGE_BASE_OFFSET {
        return Err(format!("module.image_base derived at +{:#x}", derived_image.offset));
    }
    let array_text = runtime_section(context, "dumparray")?;
    let parsed_array = sos::parse_dumparray(&array_text).ok_or("dumparray section unparsable")?;
    let layout = runtime::derive_array_layout(
        &mut dump,
        RT_ARRAY,
        parsed_array.elements,
        &parsed_array.items,
    )?;
    if layout.length_offset != 0x08 || layout.data_offset != RT_ARRAY_DATA_OFFSET || layout.stride != 8 {
        return Err(format!(
            "array layout derived as length +{:#x}, data +{:#x}, stride {}",
            layout.length_offset, layout.data_offset, layout.stride
        ));
    }

    // 端到端：把导出的位移 + 观察到的 RID→名字塞进 fixture 中间件，`emit` 出 `runtime` 段。
    let (mut intermediate, il) = inputs(context)?;
    intermediate.runtime = vec![
        sos::SosRuntimeRow {
            group: "eetype".to_string(),
            name: "token".to_string(),
            offset: derived_token.offset,
            shift: 8,
            stride: 0,
            provenance: "dumpmt+bytes".to_string(),
            witness: derived_token.witness,
            probes: derived_token.probes,
        },
        sos::SosRuntimeRow {
            group: "eetype".to_string(),
            name: "loader_module".to_string(),
            offset: derived_module.offset,
            shift: 0,
            stride: 0,
            provenance: "dumpmt+bytes".to_string(),
            witness: derived_module.witness,
            probes: derived_module.probes,
        },
        sos::SosRuntimeRow {
            group: "module".to_string(),
            name: "image_base".to_string(),
            offset: derived_image.offset,
            shift: 0,
            stride: 0,
            provenance: "dumpmodule+bytes".to_string(),
            witness: derived_image.witness,
            probes: derived_image.probes,
        },
        sos::SosRuntimeRow {
            group: "screen_array".to_string(),
            name: "length".to_string(),
            offset: layout.length_offset,
            shift: 0,
            stride: 0,
            provenance: "dumparray+bytes".to_string(),
            witness: layout.length_witness,
            probes: 3,
        },
        sos::SosRuntimeRow {
            group: "screen_array".to_string(),
            name: "elements".to_string(),
            offset: layout.data_offset,
            shift: 0,
            stride: layout.stride,
            provenance: "dumparray+bytes".to_string(),
            witness: layout.elements_witness,
            probes: 3,
        },
    ];
    intermediate.typedefs = runtime_typedef_rows("osu.Game.Screens.Menu.MainMenu", "osu.Desktop.OsuGameDesktop");
    let il = il_with_runtime_types(&il);
    let outcome = emit::emit(&intermediate, &il, true, false)
        .map_err(|emit::Refusal(message)| format!("emit refused: {message}"))?;
    let outcome_runtime = outcome
        .runtime
        .as_ref()
        .ok_or_else(|| format!("no runtime section: {:?}", outcome.warnings))?;
    if outcome_runtime.rows.len() != crate::spec::RUNTIME_PROBES.len() {
        return Err(format!("{} runtime row(s) published", outcome_runtime.rows.len()));
    }
    if outcome_runtime.typedefs.get("osu.Game.dll").and_then(|map| map.get("9A2")).map(|s| s.as_str())
        != Some("osu.Game.Screens.Menu.MainMenu")
    {
        return Err(format!("typedefs = {:?}", outcome_runtime.typedefs));
    }
    if outcome_runtime.typedefs.get("osu.Game.dll").and_then(|map| map.get("8B6")).map(|s| s.as_str())
        != Some("osu.Game.Screens.Select.SongSelect")
    {
        return Err(
            "the namespace selection rule did not pick up the IL-only screen type (osu.Game.Screens.*)"
                .to_string(),
        );
    }
    if outcome_runtime.observed.get("osu!.dll").and_then(|map| map.get("F")).map(|s| s.as_str())
        != Some("osu.Desktop.OsuGameDesktop")
    {
        return Err(format!("observed = {:?}", outcome_runtime.observed));
    }
    // 表 JSON 里也要有这一段（形状与 `offsets.rs::RuntimeSection` 一致）。
    let table_json = outcome.to_json();
    let json_path = context.work.join("EXAMPLE-FIXTURE-selftest-runtime.json");
    fs::write(&json_path, &table_json)
        .map_err(|e| format!("write {}: {e}", json_path.display()))?;
    let parsed = parse_json(&table_json)?;
    let root = parsed.as_object().ok_or("the emitted table is not a JSON object")?;
    let runtime_json = root
        .get("runtime")
        .and_then(|value| value.as_object())
        .ok_or("`runtime` is not an object in the emitted JSON")?;
    let entry = |group: &str, name: &str| -> Option<&Json> {
        runtime_json.get(group)?.as_object()?.get(name)
    };
    let token_json = entry("eetype", "token")
        .and_then(|value| value.as_object())
        .ok_or("runtime.eetype.token missing from the JSON")?;
    if token_json.get("offset").and_then(|value| value.as_number()) != Some(RT_TOKEN_OFFSET as f64)
        || token_json.get("shift").and_then(|value| value.as_number()) != Some(8.0)
    {
        return Err(format!("runtime.eetype.token JSON = {:?}", token_json));
    }
    if entry("eetype", "loader_module")
        .and_then(|value| value.as_object())
        .and_then(|map| map.get("offset"))
        .and_then(|value| value.as_number())
        != Some(RT_MODULE_OFFSET as f64)
    {
        return Err("runtime.eetype.loader_module offset missing/wrong in the JSON".to_string());
    }
    let elements_json = entry("screen_array", "elements")
        .and_then(|value| value.as_object())
        .ok_or("runtime.screen_array.elements missing from the JSON")?;
    if elements_json.get("offset").and_then(|value| value.as_number()) != Some(RT_ARRAY_DATA_OFFSET as f64)
        || elements_json.get("stride").and_then(|value| value.as_number()) != Some(8.0)
    {
        return Err(format!("runtime.screen_array.elements JSON = {:?}", elements_json));
    }
    Ok(format!(
        "derived token+{:#x}/module+{:#x}/image_base+{:#x}/array length+{:#x} data+{:#x} stride {}; \
         {} runtime row(s), {} typedef name(s) over {} module(s), {} observed",
        derived_token.offset,
        derived_module.offset,
        derived_image.offset,
        layout.length_offset,
        layout.data_offset,
        layout.stride,
        outcome_runtime.rows.len(),
        outcome_runtime.typedefs.values().map(|map| map.len()).sum::<usize>(),
        outcome_runtime.typedefs.len(),
        outcome_runtime.observed.values().map(|map| map.len()).sum::<usize>()
    ))
}

/// `runtime/wrong-offset`：第二个探针的 token 挪到另一个位移 ⇒ **搜不出共同位移**（拒绝发布）。
fn case_runtime_wrong_offset(context: &Context) -> Result<String, String> {
    let blocks = runtime_blocks(RT_TOKEN_OFFSET + 4, false);
    let dump_path = context.work.join("EXAMPLE-fixture-runtime-wrong.dmp");
    write_synthetic_dump(&dump_path, &blocks, None, None)?;
    let mut dump = Dump::open(&dump_path)?;
    let (token_probes, _) = runtime_probes();
    match runtime::find_u32_shifted(&mut dump, RT_EETYPE_A, runtime_probe("eetype", "token"), &token_probes) {
        Ok(derived) => Err(format!(
            "a wrong-offset probe still produced +{:#x} ({}) — the derivation must fail closed",
            derived.offset, derived.witness
        )),
        Err(reason) => Ok(format!("refused as expected: {reason}")),
    }
}

/// `runtime/ambiguous`：同一个打包值出现在两个位移 ⇒ 判**歧义**（同样拒绝）。
fn case_runtime_ambiguous(context: &Context) -> Result<String, String> {
    let blocks = runtime_blocks(RT_TOKEN_OFFSET, true);
    let dump_path = context.work.join("EXAMPLE-fixture-runtime-ambiguous.dmp");
    write_synthetic_dump(&dump_path, &blocks, None, None)?;
    let mut dump = Dump::open(&dump_path)?;
    let (token_probes, _) = runtime_probes();
    match runtime::find_u32_shifted(&mut dump, RT_EETYPE_A, runtime_probe("eetype", "token"), &token_probes) {
        Ok(derived) => Err(format!(
            "an ambiguous layout still produced +{:#x} — ambiguity must be refused",
            derived.offset
        )),
        Err(reason) if reason.contains("ambiguous") => Ok(format!("refused as expected: {reason}")),
        Err(other) => Err(format!("refused for the wrong reason: {other}")),
    }
}

/// `runtime/garbage-name`：观察行与元数据不一致（含**空名字**）⇒ 该行不进表（逐条列出），
/// 且因为 GameBase 的类型没被观察到，**整段不发布**（读侧的运行期证明需要那一条）。
fn case_runtime_garbage_name(context: &Context) -> Result<String, String> {
    let (mut intermediate, il) = inputs(context)?;
    intermediate.runtime = runtime_rows();
    intermediate.typedefs = runtime_typedef_rows("osu.Game.Screens.Menu.MainMenu", ""); // 空名字：谁都不该把它当类型名
    let il = il_with_runtime_types(&il);
    let outcome = emit::emit(&intermediate, &il, true, false)
        .map_err(|emit::Refusal(message)| format!("emit refused: {message}"))?;
    if let Some(runtime) = &outcome.runtime {
        if runtime.typedefs.contains_key("osu!.dll") {
            return Err(format!(
                "an empty observed name was published: {:?}",
                runtime.typedefs.get("osu!.dll")
            ));
        }
        return Err("the runtime section was published although GameBase was never observed".to_string());
    }
    if outcome
        .omitted
        .iter()
        .any(|row| row.label == "typedef:osu!.dll" && row.reason == "typedef-mismatch")
    {
        return Ok(format!(
            "empty observed name dropped ({} omitted row(s)); runtime not published; JSON `runtime` \
             = {:?}",
            outcome.omitted.len(),
            outcome.to_json().contains("\"runtime\": null")
        ));
    }
    Err(format!(
        "no typedef mismatch reported for the empty name: {:?}",
        outcome
            .omitted
            .iter()
            .map(|row| format!("{}.{}:{}", row.label, row.field, row.reason))
            .collect::<Vec<_>>()
    ))
}

/// `runtime/missing-row`：少一条位移行 ⇒ **整段不发布**（表里 `runtime` 为 null + 警告）。
fn case_runtime_missing_row(context: &Context) -> Result<String, String> {
    let (mut intermediate, il) = inputs(context)?;
    intermediate.runtime = runtime_rows()
        .into_iter()
        .filter(|row| !(row.group == "eetype" && row.name == "loader_module"))
        .collect();
    intermediate.typedefs = runtime_typedef_rows("osu.Game.Screens.Menu.MainMenu", "osu.Desktop.OsuGameDesktop");
    let il = il_with_runtime_types(&il);
    let outcome = emit::emit(&intermediate, &il, true, false)
        .map_err(|emit::Refusal(message)| format!("emit refused: {message}"))?;
    if outcome.runtime.is_some() {
        return Err("a runtime section was published with a missing structure offset".to_string());
    }
    if !outcome.warnings.iter().any(|warning| warning.contains("runtime section not published")) {
        return Err(format!("no warning about the missing row: {:?}", outcome.warnings));
    }
    let parsed = parse_json(&outcome.to_json())?;
    let root = parsed.as_object().ok_or("the emitted table is not a JSON object")?;
    if root.get("runtime") != Some(&Json::Null) {
        return Err(format!("JSON runtime = {:?}, expected null", root.get("runtime")));
    }
    Ok(format!(
        "runtime not published ({} omitted row(s)); JSON `runtime` = null; warning kept",
        outcome.omitted.len()
    ))
}

/// 运行期段的五条位移行（探针数达标；数值与 `case_runtime_walk` 的推导一致）。
fn runtime_rows() -> Vec<sos::SosRuntimeRow> {
    let row = |group: &str, name: &str, offset: i64, shift: u32, stride: i64, provenance: &str| {
        sos::SosRuntimeRow {
            group: group.to_string(),
            name: name.to_string(),
            offset,
            shift,
            stride,
            provenance: provenance.to_string(),
            witness: format!("EXAMPLE-FIXTURE witness for {group}.{name}"),
            probes: 3,
        }
    };
    vec![
        row("eetype", "token", RT_TOKEN_OFFSET, 8, 0, "dumpmt+bytes"),
        row("eetype", "loader_module", RT_MODULE_OFFSET, 0, 0, "dumpmt+bytes"),
        row("module", "image_base", RT_IMAGE_BASE_OFFSET, 0, 0, "dumpmodule+bytes"),
        row("screen_array", "length", 0x08, 0, 0, "dumparray+bytes"),
        row("screen_array", "elements", RT_ARRAY_DATA_OFFSET, 0, 8, "dumparray+bytes"),
    ]
}

/// 观察到的 `(模块, RID) → 名字` 行（fixture；两个名字都可被调用方改坏以测 fail-closed）。
fn runtime_typedef_rows(game_name: &str, game_base_name: &str) -> Vec<sos::SosTypedefRow> {
    vec![
        sos::SosTypedefRow {
            module: "osu.Game.dll".to_string(),
            rid: 0x9A2,
            name: game_name.to_string(),
            source: "observed".to_string(),
            witness: "EXAMPLE-FIXTURE dumpmt witness (osu.Game.Screens.Menu.MainMenu)".to_string(),
        },
        sos::SosTypedefRow {
            module: "osu!.dll".to_string(),
            rid: 0xF,
            name: game_base_name.to_string(),
            source: "observed".to_string(),
            witness: "EXAMPLE-FIXTURE dumpmt witness (osu.Desktop.OsuGameDesktop)".to_string(),
        },
    ]
}

/// fixture 的 IL 清单 + 运行期段要用的**类型行**（`RID → 名字`）。
fn il_with_runtime_types(il: &IlInventory) -> IlInventory {
    let mut il = il.clone();
    il.types = vec![
        ilmeta::IlType {
            assembly: "osu.Game.dll".to_string(),
            rid: 0x9A2,
            name: "osu.Game.Screens.Menu.MainMenu".to_string(),
        },
        ilmeta::IlType {
            assembly: "osu!.dll".to_string(),
            rid: 0xF,
            name: "osu.Desktop.OsuGameDesktop".to_string(),
        },
        ilmeta::IlType {
            assembly: "osu.Game.dll".to_string(),
            rid: 0x8B6,
            name: "osu.Game.Screens.Select.SongSelect".to_string(),
        },
        ilmeta::IlType {
            assembly: "osu.Game.dll".to_string(),
            rid: 0x123,
            name: "osu.Game.Beatmaps.BeatmapInfo".to_string(),
        },
    ];
    il
}

fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or("").to_string()
}

fn inputs(context: &Context) -> Result<(SosIntermediate, IlInventory), String> {
    let il = IlInventory::read_tsv(&context.path("EXAMPLE-fixture-il.tsv"))?;
    let (intermediate, _) = intermediate_from_fixture(context, "EXAMPLE-fixture-dump-clean.dmp", None)?;
    Ok((intermediate, il))
}

fn emit_fixture(intermediate: &SosIntermediate, il: &IlInventory) -> Result<emit::EmitOutcome, String> {
    emit::emit(intermediate, il, true, false)
        .map_err(|emit::Refusal(message)| format!("emit refused: {message}"))
}

fn expect_omitted(
    outcome: &emit::EmitOutcome,
    label: &str,
    field: &str,
    reason: &str,
) -> Result<String, String> {
    let omitted = outcome
        .omitted
        .iter()
        .find(|row| row.label == label && row.field == field)
        .ok_or_else(|| {
            format!(
                "{label}.{field} was not reported as omitted (published {} field(s))",
                outcome.published.len()
            )
        })?;
    if omitted.reason != reason {
        return Err(format!(
            "{label}.{field} omitted as {:?}, expected {reason:?} ({})",
            omitted.reason, omitted.detail
        ));
    }
    if outcome
        .published
        .iter()
        .any(|row| row.label == label && row.field == field)
    {
        return Err(format!("{label}.{field} was omitted *and* published"));
    }
    Ok(format!(
        "{label}.{field} omitted as {reason} ({} published, {} omitted)",
        outcome.published.len(),
        outcome.omitted.len()
    ))
}

/// fixture 的 `#fixture-address` 行把块钉在地址上（真实 transcript 没有这一行）。
fn fixture_blocks(context: &Context) -> Result<BTreeMap<u64, SosObject>, String> {
    let text = context.read("EXAMPLE-fixture-sos-chain.txt")?;
    let parsed = sos::parse_transcript(&text);
    let mut addresses: Vec<u64> = Vec::new();
    let mut pending: Option<u64> = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("#fixture-address") {
            let value = rest.trim();
            let value = value.strip_prefix("0x").unwrap_or(value);
            pending = Some(
                u64::from_str_radix(value, 16)
                    .map_err(|_| format!("bad #fixture-address {rest:?}"))?,
            );
        } else if trimmed.starts_with("Name:") {
            if let Some(address) = pending.take() {
                addresses.push(address);
            }
        }
    }
    if addresses.len() != parsed.objects.len() {
        return Err(format!(
            "{} #fixture-address marker(s) but {} object block(s)",
            addresses.len(),
            parsed.objects.len()
        ));
    }
    let mut blocks = BTreeMap::new();
    for (address, object) in addresses.into_iter().zip(parsed.objects.into_iter()) {
        blocks.insert(address, object);
    }
    Ok(blocks)
}

struct FixtureBlocks {
    blocks: BTreeMap<u64, SosObject>,
}

impl BlockSource for FixtureBlocks {
    fn fetch(&mut self, addresses: &[u64]) -> BTreeMap<u64, SosObject> {
        addresses
            .iter()
            .filter_map(|address| {
                self.blocks
                    .get(address)
                    .map(|object| (*address, object.clone()))
            })
            .collect()
    }
}

fn walk_fixture(context: &Context) -> Result<(BTreeMap<String, chain::Resolved>, BTreeMap<u64, SosObject>), String> {
    let blocks = fixture_blocks(context)?;
    let game = blocks
        .get(&0xBF000FD8)
        .cloned()
        .ok_or("the fixture has no game block at 0xBF000FD8")?;
    let mut source = FixtureBlocks { blocks };
    Ok(chain::walk(&mut source, 0xBF000FD8, &game))
}

/// 合成 dump + fixture 链 → SOS 中间件（与 `extract` 的后半段同一条代码）。
fn intermediate_from_fixture(
    context: &Context,
    dump_name: &str,
    corrupt: Option<(u64, i64)>,
) -> Result<(SosIntermediate, PathBuf), String> {
    let blocks = fixture_blocks(context)?;
    let dump_path = context.work.join(dump_name);
    write_synthetic_dump(&dump_path, &blocks, Some(FIXTURE_ANCHOR), corrupt)?;
    let mut dump = Dump::open(&dump_path)?;
    let game = blocks
        .get(&0xBF000FD8)
        .cloned()
        .ok_or("the fixture has no game block")?;
    let mut source = FixtureBlocks { blocks };
    let (resolved, blocks) = chain::walk(&mut source, 0xBF000FD8, &game);
    let mut notes = source.take_notes();
    notes.push(chain::offset_base_note(
        &mut dump,
        0xBF000FD8,
        &game.method_table,
    ));
    let mut intermediate = chain::build_intermediate(&mut dump, &blocks, &resolved, notes);
    // 与 `extract` 一样的溯源行（fixture 版本：provenance=fixture）。
    for (key, value) in [
        ("provenance", "fixture"),
        ("fixture", "true"),
        ("dump", "<synthetic fixture dump>"),
        ("dump_arch", "x64"),
        ("arch", "x64"),
        ("lazer_version", "2026.921.0.0"),
        ("lazer_version_source", "fixture"),
        ("runtime_version", "10.0.12"),
        ("runtime_version_source", "fixture"),
        ("dump_module_version:osu!.dll", "2026.921.0.0"),
        ("dump_module_version:osu!.exe", "2026.921.0.0"),
        ("lazer_exe", "C:\\Users\\EXAMPLE\\AppData\\Local\\osulazer\\current\\osu!.exe"),
        ("lazer_exe_sha256", "unavailable"),
        ("analyzer", "<fixture: no analyzer was run>"),
        ("anchor_pattern", crate::spec::ANCHOR_PATTERN),
        ("anchor_hits", "1"),
        ("anchor", "0xBF0016D4"),
        ("anchor_site_delta", "36"),
        ("resolve_site", "0xBF0016B0"),
        ("resolve_hops", "external_link_opener(+0x0) -> api_access(+0x218) -> game(+0x310)"),
        ("resolve_attempts", "7"),
        ("resolve_candidates", "1"),
        (
            "resolve_vtable_check",
            "not applicable: no table vtable was supplied (--expect-vtable / --table / \
             $MMA_LAZER_OFFSETS); only the dumpobj type check ran",
        ),
        ("game_base", "0xBF000FD8"),
        ("game_base_type", "osu.Desktop.OsuGameDesktop"),
        ("game_base_mt", "00007ff9e7ef8970"),
        ("transcript", "<fixture: EXAMPLE-fixture-sos-chain.txt>"),
    ] {
        intermediate
            .provenance
            .insert(key.to_string(), value.to_string());
    }
    Ok((intermediate, dump_path))
}

// -------------------------------------------------- 合成 minidump（自测用）----

const HEAP_BASE: u64 = 0xBF000000;
const HEAP_SIZE: u64 = 0x2000;

/// 写一份**合成 minidump**：把 fixture 字段行的 `Value` 逐条种进内存，
/// 让 `deref_status` 有真东西可比（含 `--type Heap` 的 Memory64ListStream 形态）。
fn write_synthetic_dump(
    path: &Path,
    blocks: &BTreeMap<u64, SosObject>,
    anchor: Option<u64>,
    corrupt: Option<(u64, i64)>,
) -> Result<(), String> {
    let mut heap = vec![0u8; HEAP_SIZE as usize];
    for (address, object) in blocks {
        if let Ok(method_table) = u64::from_str_radix(object.method_table.trim(), 16) {
            plant(&mut heap, *address, &method_table.to_le_bytes())?;
        }
        // `System.String` 的块：按**该块自己打印的** `_stringLength`/`_firstChar` 偏移把
        // `String:` 行的内容种进去（偏移驱动、内容一致 ⇒ 读回来必须逐字符相等）。
        if let Some(text) = &object.string_value {
            let length_offset = object
                .rows_named("_stringLength")
                .into_iter()
                .next()
                .map(|row| row.offset)
                .unwrap_or(0x08);
            let chars_offset = object
                .rows_named("_firstChar")
                .into_iter()
                .next()
                .map(|row| row.offset)
                .unwrap_or(0x0c);
            let units: Vec<u16> = text.encode_utf16().collect();
            plant(
                &mut heap,
                *address + length_offset as u64,
                &(units.len() as i32).to_le_bytes(),
            )?;
            for (index, unit) in units.iter().enumerate() {
                plant(
                    &mut heap,
                    *address + chars_offset as u64 + (index * 2) as u64,
                    &unit.to_le_bytes(),
                )?;
            }
        }
        for field in &object.fields {
            if field.attr != "instance" || field.offset < 0 {
                continue;
            }
            let target = *address + field.offset as u64;
            let bytes = encode_value(field);
            match bytes {
                Some(bytes) => plant(&mut heap, target, &bytes)?,
                None => {}
            }
        }
    }
    if let Some((address, offset)) = corrupt {
        plant(&mut heap, address + offset as u64, &[0xEEu8; 8])?;
    }
    if let Some(anchor) = anchor {
        let pattern = parse_pattern(crate::spec::ANCHOR_PATTERN)?;
        plant(&mut heap, anchor, &pattern)?;
    }

    // ---- 组装文件（头部 + 目录 + 4 个流 + 内存）----
    const STREAMS: u32 = 4;
    let mut out: Vec<u8> = Vec::with_capacity(HEAP_SIZE as usize + 4096);
    put_u32(&mut out, 0x504D_444D);
    put_u32(&mut out, 0x0000_A793);
    put_u32(&mut out, STREAMS);
    put_u32(&mut out, 32);
    put_u32(&mut out, 0);
    put_u32(&mut out, 0);
    put_u64(&mut out, 0);
    let directory_at = out.len();
    out.extend_from_slice(&[0u8; (STREAMS as usize) * 12]);

    // SystemInfoStream（56 字节；arch=9 = AMD64）
    let system_at = out.len();
    put_u16(&mut out, 9);
    put_u16(&mut out, 6);
    put_u16(&mut out, 0x3C00);
    out.push(32);
    out.push(1);
    put_u32(&mut out, 10);
    put_u32(&mut out, 0);
    put_u32(&mut out, 26200);
    put_u32(&mut out, 2);
    put_u32(&mut out, 0);
    put_u16(&mut out, 0);
    put_u16(&mut out, 0);
    out.extend_from_slice(&[0u8; 24]);
    let system_size = out.len() - system_at;

    // MiscInfoStream（24 字节；pid 与 create-time）
    let misc_at = out.len();
    put_u32(&mut out, 24);
    put_u32(&mut out, 0);
    put_u32(&mut out, 1234);
    put_u32(&mut out, 1_700_000_000);
    put_u32(&mut out, 0);
    put_u32(&mut out, 0);
    let misc_size = out.len() - misc_at;

    // ModuleListStream：两个模块（osu!.exe / osu!.dll），版本 2026.921.0.0
    let module_at = out.len();
    put_u32(&mut out, 2);
    let mut names: Vec<(usize, String)> = Vec::new();
    for (index, name) in ["osu!.exe", "osu!.dll"].iter().enumerate() {
        put_u64(&mut out, 0x7FF9_0000_0000 + (index as u64) * 0x10000);
        put_u32(&mut out, 0x1000);
        put_u32(&mut out, 0);
        put_u32(&mut out, 0);
        names.push((out.len(), name.to_string()));
        put_u32(&mut out, 0); // ModuleNameRva（稍后回填）
        put_u32(&mut out, 0xFEEF_04BD);
        put_u32(&mut out, 0x0001_0000);
        put_u32(&mut out, (2026u32 << 16) | 921);
        put_u32(&mut out, 0);
        out.extend_from_slice(&[0u8; 52 - 16]);
        out.extend_from_slice(&[0u8; 8]); // CvRecord
        out.extend_from_slice(&[0u8; 8]); // MiscRecord
        out.extend_from_slice(&[0u8; 16]); // Reserved
    }
    for (position, name) in &names {
        let rva = out.len() as u32;
        let units: Vec<u16> = name.encode_utf16().collect();
        put_u32(&mut out, (units.len() * 2) as u32);
        for unit in units {
            put_u16(&mut out, unit);
        }
        out[*position..*position + 4].copy_from_slice(&rva.to_le_bytes());
    }
    let module_size = out.len() - module_at;

    // Memory64ListStream：count + baseRva + 描述表 + 数据（数据紧跟描述表之后）
    let memory_at = out.len();
    put_u64(&mut out, 1);
    let base_rva_at = out.len();
    put_u64(&mut out, 0); // baseRva 占位（下面回填）
    let descriptor_at = out.len();
    out.extend_from_slice(&[0u8; 16]);
    let data_offset = out.len() as u64;
    out.extend_from_slice(&heap);
    out[base_rva_at..base_rva_at + 8].copy_from_slice(&data_offset.to_le_bytes());
    out[descriptor_at..descriptor_at + 8].copy_from_slice(&HEAP_BASE.to_le_bytes());
    out[descriptor_at + 8..descriptor_at + 16].copy_from_slice(&HEAP_SIZE.to_le_bytes());
    let memory_size = out.len() - memory_at;

    // 回填目录
    for (index, (stream_type, offset, size)) in [
        (7u32, system_at, system_size),
        (15, misc_at, misc_size),
        (4, module_at, module_size),
        (9, memory_at, memory_size),
    ]
    .into_iter()
    .enumerate()
    {
        let at = directory_at + index * 12;
        out[at..at + 4].copy_from_slice(&stream_type.to_le_bytes());
        out[at + 4..at + 8].copy_from_slice(&(size as u32).to_le_bytes());
        out[at + 8..at + 12].copy_from_slice(&(offset as u32).to_le_bytes());
    }
    fs::write(path, &out).map_err(|e| format!("write {}: {e}", path.display()))
}

fn plant(heap: &mut [u8], address: u64, bytes: &[u8]) -> Result<(), String> {
    let offset = address
        .checked_sub(HEAP_BASE)
        .ok_or_else(|| format!("address 0x{address:X} is below the synthetic heap"))? as usize;
    if offset + bytes.len() > heap.len() {
        return Err(format!(
            "planting {} bytes at 0x{address:X} runs past the synthetic heap",
            bytes.len()
        ));
    }
    heap[offset..offset + bytes.len()].copy_from_slice(bytes);
    Ok(())
}

/// 按 SOS 的值列形态编码一个字段的值（引用 = 指针，原始类型 = 内容，结构体 = 不种）。
fn encode_value(field: &sos::SosField) -> Option<Vec<u8>> {
    let text = field.value.trim();
    if field.vt == "No" {
        let value = u64::from_str_radix(text, 16).ok()?;
        return Some(value.to_le_bytes().to_vec());
    }
    if field.vt != "Yes" {
        return None;
    }
    match field.sos_type.as_str() {
        "System.Boolean" => {
            let value = matches!(text, "1" | "True" | "true");
            Some(vec![u8::from(value)])
        }
        "System.Char" => {
            // SOS 的 Value 列对 `System.Char` 是十六进制（`44` = 'D'）。
            let value = u32::from_str_radix(text.trim_start_matches("0x"), 16).ok()?;
            Some((value as u16).to_le_bytes().to_vec())
        }
        "System.Byte" => Some(vec![text.parse::<u8>().ok()?]),
        "System.SByte" => Some(vec![(text.parse::<i8>().ok()? as u8)]),
        "System.Int16" => Some(text.parse::<i16>().ok()?.to_le_bytes().to_vec()),
        "System.UInt16" => Some(text.parse::<u16>().ok()?.to_le_bytes().to_vec()),
        "System.Int32" => Some(text.parse::<i32>().ok()?.to_le_bytes().to_vec()),
        "System.UInt32" => Some(text.parse::<u32>().ok()?.to_le_bytes().to_vec()),
        "System.Int64" => Some(text.parse::<i64>().ok()?.to_le_bytes().to_vec()),
        "System.UInt64" => Some(text.parse::<u64>().ok()?.to_le_bytes().to_vec()),
        "System.Single" => Some(text.parse::<f32>().ok()?.to_le_bytes().to_vec()),
        "System.Double" => Some(text.parse::<f64>().ok()?.to_le_bytes().to_vec()),
        _ => None,
    }
}

fn put_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

// --------------------------------------------------------- 迷你 JSON ----

/// 极小的 JSON 解析器（**只给自测用**：出表的 JSON 必须能被真的解析回来）。
#[derive(Clone, Debug, PartialEq)]
enum Json {
    Null,
    Bool(bool),
    Number(f64),
    Str(String),
    Array(Vec<Json>),
    Object(BTreeMap<String, Json>),
}

impl Json {
    fn as_object(&self) -> Option<&BTreeMap<String, Json>> {
        match self {
            Json::Object(map) => Some(map),
            _ => None,
        }
    }

    fn as_number(&self) -> Option<f64> {
        match self {
            Json::Number(value) => Some(*value),
            _ => None,
        }
    }
}

fn parse_json(text: &str) -> Result<Json, String> {
    let bytes = text.as_bytes();
    let mut index = 0usize;
    let value = json_value(bytes, &mut index)?;
    json_ws(bytes, &mut index);
    if index != bytes.len() {
        return Err(format!("trailing garbage at byte {index}"));
    }
    Ok(value)
}

fn json_ws(bytes: &[u8], index: &mut usize) {
    while *index < bytes.len() && (bytes[*index] as char).is_whitespace() {
        *index += 1;
    }
}

fn json_value(bytes: &[u8], index: &mut usize) -> Result<Json, String> {
    json_ws(bytes, index);
    match bytes.get(*index) {
        Some(b'{') => {
            *index += 1;
            let mut map = BTreeMap::new();
            json_ws(bytes, index);
            if bytes.get(*index) == Some(&b'}') {
                *index += 1;
                return Ok(Json::Object(map));
            }
            loop {
                json_ws(bytes, index);
                let key = match json_value(bytes, index)? {
                    Json::Str(text) => text,
                    _ => return Err(format!("object key at byte {} is not a string", *index)),
                };
                json_ws(bytes, index);
                if bytes.get(*index) != Some(&b':') {
                    return Err(format!("expected ':' at byte {}", *index));
                }
                *index += 1;
                let value = json_value(bytes, index)?;
                map.insert(key, value);
                json_ws(bytes, index);
                match bytes.get(*index) {
                    Some(b',') => {
                        *index += 1;
                    }
                    Some(b'}') => {
                        *index += 1;
                        return Ok(Json::Object(map));
                    }
                    other => {
                        return Err(format!(
                            "expected ',' or '}}' at byte {} (got {other:?})",
                            *index
                        ))
                    }
                }
            }
        }
        Some(b'[') => {
            *index += 1;
            let mut items = Vec::new();
            json_ws(bytes, index);
            if bytes.get(*index) == Some(&b']') {
                *index += 1;
                return Ok(Json::Array(items));
            }
            loop {
                items.push(json_value(bytes, index)?);
                json_ws(bytes, index);
                match bytes.get(*index) {
                    Some(b',') => {
                        *index += 1;
                    }
                    Some(b']') => {
                        *index += 1;
                        return Ok(Json::Array(items));
                    }
                    other => {
                        return Err(format!(
                            "expected ',' or ']' at byte {} (got {other:?})",
                            *index
                        ))
                    }
                }
            }
        }
        Some(b'"') => {
            *index += 1;
            let mut out = String::new();
            loop {
                let byte = *bytes
                    .get(*index)
                    .ok_or_else(|| "unterminated string".to_string())?;
                *index += 1;
                match byte {
                    b'"' => return Ok(Json::Str(out)),
                    b'\\' => {
                        let escape = *bytes
                            .get(*index)
                            .ok_or_else(|| "unterminated escape".to_string())?;
                        *index += 1;
                        match escape {
                            b'"' => out.push('"'),
                            b'\\' => out.push('\\'),
                            b'/' => out.push('/'),
                            b'b' => out.push('\u{8}'),
                            b'f' => out.push('\u{c}'),
                            b'n' => out.push('\n'),
                            b'r' => out.push('\r'),
                            b't' => out.push('\t'),
                            b'u' => {
                                let digits = bytes
                                    .get(*index..*index + 4)
                                    .ok_or_else(|| "short \\u escape".to_string())?;
                                let hex = String::from_utf8_lossy(digits).to_string();
                                *index += 4;
                                let code = u32::from_str_radix(&hex, 16)
                                    .map_err(|_| format!("bad \\u escape {hex:?}"))?;
                                out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                            }
                            other => return Err(format!("bad escape \\{}", other as char)),
                        }
                    }
                    _ => {
                        // UTF-8 多字节：原样收进来的字节流已经是 &str 的一部分。
                        let start = *index - 1;
                        let mut end = *index;
                        while end < bytes.len() && (bytes[end] & 0xC0) == 0x80 {
                            end += 1;
                        }
                        *index = end;
                        out.push_str(&String::from_utf8_lossy(&bytes[start..end]));
                    }
                }
            }
        }
        Some(b't') => {
            expect_word(bytes, index, "true")?;
            Ok(Json::Bool(true))
        }
        Some(b'f') => {
            expect_word(bytes, index, "false")?;
            Ok(Json::Bool(false))
        }
        Some(b'n') => {
            expect_word(bytes, index, "null")?;
            Ok(Json::Null)
        }
        Some(_) => {
            let start = *index;
            while *index < bytes.len()
                && matches!(bytes[*index], b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E')
            {
                *index += 1;
            }
            let text = std::str::from_utf8(&bytes[start..*index])
                .map_err(|_| "number is not UTF-8".to_string())?;
            text.parse::<f64>()
                .map(Json::Number)
                .map_err(|_| format!("bad number {text:?} at byte {start}"))
        }
        None => Err("unexpected end of input".to_string()),
    }
}

fn expect_word(bytes: &[u8], index: &mut usize, word: &str) -> Result<(), String> {
    if bytes.len() < *index + word.len() || &bytes[*index..*index + word.len()] != word.as_bytes() {
        return Err(format!("expected {word:?} at byte {}", *index));
    }
    *index += word.len();
    Ok(())
}
