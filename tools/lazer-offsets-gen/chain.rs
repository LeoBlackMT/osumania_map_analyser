// chain.rs —— 解引用链（`spec::CHAIN`）的通用走法 + 中间件装配
//
// 单独成模块的理由：**真实路径与自测走同一条代码**。
// - 真实路径的 `BlockSource` 是 `dotnet-dump analyze`（`main.rs` 的 `AnalyzerBlocks`）；
// - 自测的 `BlockSource` 是 fixture（`selftest.rs` 的 `FixtureBlocks`，无游戏、无分析器）。
// 两者之外的一切（分层、按类型验证、按字段取指针、dump 字节解引用自证、中间件装配）
// 只有这一份实现。

use crate::minidump::Dump;
use crate::sos::{SosIntermediate, SosObject, SosObjectRecord, SosRow};
use crate::spec;
use std::collections::{BTreeMap, BTreeSet};

/// 链上一步的解析结果。
#[derive(Clone, Debug)]
pub struct Resolved {
    pub label: String,
    pub address: u64,
    pub status: String,
    pub object_type: String,
    pub why: String,
}

/// 对象块来源（真实 = 分析器；自测 = fixture）。
pub trait BlockSource {
    fn fetch(&mut self, addresses: &[u64]) -> BTreeMap<u64, SosObject>;
    fn take_notes(&mut self) -> Vec<String> {
        Vec::new()
    }
}

/// 走完整条链：返回（每一步的解析结果、地址 → 对象块）。
pub fn walk(
    source: &mut dyn BlockSource,
    game_address: u64,
    game_object: &SosObject,
) -> (BTreeMap<String, Resolved>, BTreeMap<u64, SosObject>) {
    let mut blocks: BTreeMap<u64, SosObject> = BTreeMap::new();
    blocks.insert(game_address, game_object.clone());
    let mut resolved: BTreeMap<String, Resolved> = BTreeMap::new();
    resolved.insert(
        "game".to_string(),
        Resolved {
            label: "game".to_string(),
            address: game_address,
            status: "ok".to_string(),
            object_type: game_object.name.clone(),
            why: "anchor -> game_base (type verified by dumpobj)".to_string(),
        },
    );

    let max_depth = spec::CHAIN
        .iter()
        .map(|step| depth(step.label))
        .max()
        .unwrap_or(0);
    for level in 1..=max_depth {
        let steps: Vec<&spec::ChainStep> = spec::CHAIN
            .iter()
            .filter(|step| step.parent.is_some() && depth(step.label) == level)
            .collect();
        let mut wanted: Vec<(String, u64, Vec<String>)> = Vec::new();
        for step in &steps {
            let parent_label = step.parent.unwrap_or("");
            let field = step.field.unwrap_or("");
            let Some(parent) = resolved.get(parent_label).cloned() else {
                set(
                    &mut resolved,
                    step,
                    format!("chain-skipped:{parent_label}-missing"),
                );
                continue;
            };
            // **fail-closed**：父对象没通过类型验证 ⇒ 它那个偏移上的指针不可信，
            // 子步骤一律跳过（绝不顺着一个"类型不对"的对象继续解引用）。
            if !parent.status.starts_with("ok") {
                set(
                    &mut resolved,
                    step,
                    format!("chain-skipped:{parent_label}-{}", parent.status),
                );
                continue;
            }
            let Some(block) = blocks.get(&parent.address) else {
                set(
                    &mut resolved,
                    step,
                    format!("chain-skipped:{parent_label}-no-block"),
                );
                continue;
            };
            let pointer = block
                .rows_named(field)
                .into_iter()
                .filter(|row| row.attr == "instance")
                .filter_map(|row| u64::from_str_radix(row.value.trim(), 16).ok())
                .next();
            match pointer {
                Some(0) => set(
                    &mut resolved,
                    step,
                    format!("chain-null:{field} is null in the dump"),
                ),
                Some(address) => wanted.push((
                    step.label.to_string(),
                    address,
                    step.contains.iter().map(|s| s.to_string()).collect(),
                )),
                None => set(
                    &mut resolved,
                    step,
                    format!("chain-no-field:{parent_label}.{field}"),
                ),
            }
        }
        let unique: Vec<u64> = {
            let mut set: BTreeSet<u64> = BTreeSet::new();
            for (_, address, _) in &wanted {
                set.insert(*address);
            }
            set.into_iter().collect()
        };
        if unique.is_empty() {
            continue;
        }
        let fetched = source.fetch(&unique);
        for (address, object) in &fetched {
            blocks.insert(*address, object.clone());
        }
        for (label, address, contains) in &wanted {
            let step = spec::CHAIN
                .iter()
                .find(|s| s.label == label)
                .expect("chain step");
            let status = match fetched.get(address) {
                None => "chain-unreadable:no dumpobj block".to_string(),
                Some(object) => {
                    if contains.iter().all(|needle| object.name.contains(needle)) {
                        "ok".to_string()
                    } else {
                        format!(
                            "chain-type-mismatch:0x{address:X} is {} (expected {:?})",
                            object.name, contains
                        )
                    }
                }
            };
            resolved.insert(
                label.clone(),
                Resolved {
                    label: label.clone(),
                    address: *address,
                    status,
                    object_type: fetched
                        .get(address)
                        .map(|o| o.name.clone())
                        .unwrap_or_default(),
                    why: step.why.to_string(),
                },
            );
        }
    }
    (resolved, blocks)
}

fn set(resolved: &mut BTreeMap<String, Resolved>, step: &spec::ChainStep, status: String) {
    resolved.insert(
        step.label.to_string(),
        Resolved {
            label: step.label.to_string(),
            address: 0,
            status,
            object_type: String::new(),
            why: step.why.to_string(),
        },
    );
}

fn depth(label: &str) -> usize {
    let mut depth = 0usize;
    let mut current = label;
    let mut guard = 0usize;
    loop {
        guard += 1;
        if guard > 64 {
            return depth;
        }
        let Some(step) = spec::CHAIN.iter().find(|step| step.label == current) else {
            return depth;
        };
        match step.parent {
            Some(parent) => {
                depth += 1;
                current = parent;
            }
            None => return depth,
        }
    }
}

/// 用 dump 字节对**每一个字段行**做解引用自证，并装配 SOS 中间件。
///
/// 这里刻意把"对象的全部字段"都写进中间件（不只是 `WANTED`）：这样改 `spec::WANTED`
/// 之后可以**只重跑 `emit`**（不必重新采 dump / 重跑分析器）。
pub fn build_intermediate(
    dump: &mut Dump,
    blocks: &BTreeMap<u64, SosObject>,
    resolved: &BTreeMap<String, Resolved>,
    notes: Vec<String>,
) -> SosIntermediate {
    let mut intermediate = SosIntermediate::default();
    let mut rows: Vec<SosRow> = Vec::new();
    let mut deref_checked = 0u64;
    let mut deref_ok = 0u64;
    let mut deref_mismatch = 0u64;
    for (address, object) in blocks {
        for field in &object.fields {
            let status = crate::sos::deref_status(dump, *address, object.size, field);
            if status.starts_with("ok") {
                deref_checked += 1;
                deref_ok += 1;
            } else if status.starts_with("mismatch") {
                deref_checked += 1;
                deref_mismatch += 1;
            }
            rows.push(SosRow {
                address: *address,
                object_type: object.name.clone(),
                field: field.name.clone(),
                offset: field.offset,
                sos_type: field.sos_type.clone(),
                vt: field.vt.clone(),
                attr: field.attr.clone(),
                value: field.value.clone(),
                module: object.module(),
                token: field.token.clone(),
                deref: status,
            });
        }
    }
    for (label, item) in resolved {
        intermediate.objects.push(SosObjectRecord {
            label: label.clone(),
            address: item.address,
            object_type: item.object_type.clone(),
            status: item.status.clone(),
            why: item.why.clone(),
        });
    }
    intermediate.rows = rows;
    intermediate.notes = notes;
    intermediate
        .provenance
        .insert("deref_checked".to_string(), deref_checked.to_string());
    intermediate
        .provenance
        .insert("deref_ok".to_string(), deref_ok.to_string());
    intermediate
        .provenance
        .insert("deref_mismatch".to_string(), deref_mismatch.to_string());
    intermediate
        .provenance
        .insert("dump_objects".to_string(), blocks.len().to_string());
    intermediate
}

/// 解引用基准自证：`game_base` 的首 qword（vtable）必须等于 `dumpobj` 打印的 MethodTable。
pub fn offset_base_note(dump: &mut Dump, game_address: u64, method_table: &str) -> String {
    let expected =
        u64::from_str_radix(method_table.trim().trim_start_matches("0x"), 16).unwrap_or(0);
    match dump.read_u64(game_address) {
        Some(first) if first == expected && expected != 0 => format!(
            "offset-base proof: qword at game_base 0x{game_address:X} == dumpobj MethodTable \
             0x{first:X} (SOS offsets are object-address based)"
        ),
        Some(first) => format!(
            "WARNING offset-base mismatch: qword at game_base = 0x{first:X} but dumpobj printed \
             MethodTable {method_table}"
        ),
        None => format!("WARNING game_base 0x{game_address:X} is not readable in the dump"),
    }
}

// --------------------------------------- GameBase 解析（P4 探针算法的逐字移植）----
//
// 这一段是 `temp/osu-native-memory/probe/lazer-anchors/src/main.rs::try_delta` 的移植：
// 同一批判据、同一批失败文案，只把"读活进程内存（ReadProcessMemory）"换成"读 dump 字节"。
// 真机与自测**走同一份代码**：真实路径喂真 `Dump`，自测喂合成 dump。
//
// 为什么必须移植而不是"anchor 减一个常量"：`0x24` 落在**站点**（一个指针字段）上，
// `GameBase` 要由站点多跳解引用才拿得到（见 `spec.rs::GAME_BASE_HOPS` 的溯源与 D-notes §16）。

/// x64 用户态指针的"合理性"判据（探针 `read.rs::plausible_ptr` 的逐字移植：非空、在空页
/// 之上、在内核分割之下、8 字节对齐）。只用来在解引用前挡掉垃圾值，**不是**类型判据。
pub fn plausible_ptr(p: u64) -> bool {
    p >= 0x1_0000 && p < 0x0000_8000_0000_0000 && p % 8 == 0
}

/// 一次 `(anchor, delta)` 解析尝试的完整记录（**没有静默失败**：每个字段都可为 `None`，
/// `verdict` 逐字说明被哪一步挡下、当时读到了什么）。
#[derive(Clone, Debug, Default)]
pub struct ResolveAttempt {
    pub delta: i64,
    /// `anchor - delta`（探针的 `site`）。
    pub site: Option<u64>,
    /// 站点读出的指针（探针的 `elo`）。
    pub external_link_opener: Option<u64>,
    /// `APIAccess`（探针的 `api`）。
    pub api: Option<u64>,
    /// 最后一跳读出的值（探针的 `gameBase`）。**只有 [`ResolveAttempt::resolved`] 为真时它才是候选**：
    /// 被合理性判据挡下的值也会记在这里（便于报告逐字复现），但绝不算解出的候选。
    pub game_base: Option<u64>,
    /// 走完了全部跳且每一跳的指针都通过合理性判据。
    pub resolved: bool,
    pub verdict: String,
}

impl ResolveAttempt {
    /// 打印一行用的通路描述（`site -> … -> gameBase`，读不到的部分写 `<none>`）。
    pub fn path(&self) -> String {
        let mut parts: Vec<String> = vec![format!(
            "site={}",
            self.site.map(|v| format!("{v:#x}")).unwrap_or_else(|| "<none>".into())
        )];
        for hop in crate::spec::GAME_BASE_HOPS {
            parts.push(format!(
                "{}={}",
                hop.label,
                self.hop_value(hop.label)
                    .map(|v| format!("{v:#x}"))
                    .unwrap_or_else(|| "<none>".into())
            ));
        }
        parts.join(" -> ")
    }

    /// 按跳标签取读到的值（第一条跳 = `external_link_opener`）。
    pub fn hop_value(&self, label: &str) -> Option<u64> {
        crate::spec::GAME_BASE_HOPS
            .iter()
            .position(|hop| hop.label == label)
            .and_then(|index| match index {
                0 => self.external_link_opener,
                1 => self.api,
                _ => self.game_base,
            })
    }
}

/// 一趟解析的全部尝试 + 去重后的候选地址。
#[derive(Clone, Debug, Default)]
pub struct Resolution {
    /// 逐条尝试（顺序 = delta 表顺序），报告**逐字**列出。
    pub attempts: Vec<ResolveAttempt>,
    /// 所有成功尝试里解出的候选（去重、按发现顺序）——每个都要过 [`candidate_verdict`]。
    pub candidates: Vec<u64>,
    /// 第一条成功尝试的 `(delta, site, game_base)`（探针 `resolved=yes` 的语义）。
    pub resolved: Option<(i64, u64, u64)>,
}

/// 对单个 `(anchor, delta)` 走一遍 `site → … → gameBase`。
///
/// `delta` 为正 ⇒ `site = anchor - delta`；为负 ⇒ `site = anchor + |delta|`（`--anchor-delta`
/// 允许负数，但**不允许**回绕：下溢/上溢都算失败）。
pub fn try_site(dump: &mut Dump, anchor: u64, delta: i64) -> ResolveAttempt {
    let mut attempt = ResolveAttempt {
        delta,
        ..ResolveAttempt::default()
    };
    let site = if delta >= 0 {
        match anchor.checked_sub(delta as u64) {
            Some(site) => site,
            None => {
                attempt.verdict = "anchor-delta underflow".to_string();
                return attempt;
            }
        }
    } else {
        match anchor.checked_add((-delta) as u64) {
            Some(site) => site,
            None => {
                attempt.verdict = "anchor+delta overflow".to_string();
                return attempt;
            }
        }
    };
    attempt.site = Some(site);

    let mut current = site;
    for (index, hop) in crate::spec::GAME_BASE_HOPS.iter().enumerate() {
        let field_address = current.wrapping_add(hop.offset);
        match dump.read_u64(field_address) {
            Some(value) if plausible_ptr(value) => {
                current = value;
                record_hop(&mut attempt, index, value);
            }
            Some(value) => {
                record_hop(&mut attempt, index, value);
                attempt.verdict = format!(
                    "{}={value:#x} implausible (read at {field_address:#x})",
                    hop.label
                );
                return attempt;
            }
            None => {
                attempt.verdict = format!(
                    "{} unreadable at {field_address:#x} (address is outside the dump's memory ranges)",
                    hop.label
                );
                return attempt;
            }
        }
    }
    attempt.verdict = format!(
        "resolved via {} hop(s) from site {site:#x}",
        crate::spec::GAME_BASE_HOPS.len()
    );
    attempt.resolved = true;
    attempt
}

fn record_hop(attempt: &mut ResolveAttempt, index: usize, value: u64) {
    match index {
        0 => attempt.external_link_opener = Some(value),
        1 => attempt.api = Some(value),
        _ => attempt.game_base = Some(value),
    }
}

/// 扫完整个 delta 列表（**不提前收手**：每条尝试都留在 `attempts` 里，报告逐字列出）。
pub fn resolve_game_base(dump: &mut Dump, anchor: u64, deltas: &[i64]) -> Resolution {
    let mut resolution = Resolution::default();
    for delta in deltas {
        let attempt = try_site(dump, anchor, *delta);
        // **只有 `resolved`**（走完全部跳、每跳指针都合理）才算候选：被合理性判据挡下的
        // 值虽然留在 `attempt.game_base` 里（报告要逐字复现），但绝不进候选表。
        if attempt.resolved {
            let game_base = attempt.game_base.expect("resolved attempt has a game_base");
            if resolution.resolved.is_none() {
                resolution.resolved = Some((*delta, attempt.site.unwrap_or(0), game_base));
            }
            if !resolution.candidates.contains(&game_base) {
                resolution.candidates.push(game_base);
            }
        }
        resolution.attempts.push(attempt);
    }
    resolution
}

/// 候选 `gameBase` 的两条硬判据（任一不过 ⇒ `Err(逐字原因)`，调用方必须**拒绝**该候选）。
///
/// - `dump_qword`：dump 里候选地址上的首 qword = `[gameBase]`（也就是 SOS 会打印的
///   `MethodTable`；产品侧 `osu/lazer.rs` 的 L1 结构证明比的就是它）。
/// - `expected_vtables` 为空 = 这次没有表可比：第 2 条如实记成"不适用"（绝不假装过了）；
///   非空时 `[gameBase]` 必须命中其中之一，且必须与 `dumpobj` 打印的 MethodTable 相等。
pub fn candidate_verdict(
    object: &SosObject,
    dump_qword: Option<u64>,
    expected_vtables: &[u64],
) -> Result<String, String> {
    if !spec::GAME_BASE_CONTAINS
        .iter()
        .any(|needle| object.name.contains(needle))
    {
        return Err(format!(
            "type `{}` contains none of {:?}",
            object.name,
            spec::GAME_BASE_CONTAINS
        ));
    }
    if expected_vtables.is_empty() {
        return Ok(format!(
            "type ok (`{}`); no table vtable supplied -> `[gameBase]` check not applicable",
            object.name
        ));
    }
    let Some(qword) = dump_qword else {
        return Err(format!(
            "`[gameBase]` (qword at the candidate address) is not readable in the dump; \
             expected one of {}",
            hex_list(expected_vtables)
        ));
    };
    if !expected_vtables.contains(&qword) {
        return Err(format!(
            "`[gameBase]`=0x{qword:X} != table vtable {}",
            hex_list(expected_vtables)
        ));
    }
    let printed = u64::from_str_radix(object.method_table.trim().trim_start_matches("0x"), 16)
        .map_err(|_| {
            format!(
                "dumpobj printed no parsable MethodTable ({:?})",
                object.method_table
            )
        })?;
    if printed != qword {
        return Err(format!(
            "dumpobj MethodTable=0x{printed:X} != dump qword 0x{qword:X} (offset-base proof failed)"
        ));
    }
    Ok(format!(
        "type ok (`{}`); `[gameBase]`=0x{qword:X} == table vtable 0x{printed:X}",
        object.name
    ))
}

fn hex_list(values: &[u64]) -> String {
    values
        .iter()
        .map(|v| format!("0x{v:X}"))
        .collect::<Vec<_>>()
        .join(", ")
}