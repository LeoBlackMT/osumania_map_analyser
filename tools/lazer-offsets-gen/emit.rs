// emit.rs —— 校验（SOS × IL 双见证）+ 出表（`desktop/src/osu/offsets.rs::load` 的 JSON 形状）
//
// 验收判据（README 里也有同一张表）：
//   每一个出表的偏移都必须同时有
//   (a) **SOS 行**：`dumpobj` 打印的 `Offset`（来自真实 dump，`extract` 产出）；
//   (b) **IL 结构行**：`osu.Game.dll` / `osu.Framework.dll` 元数据里同名字段的结构
//       （存在性 + instance/static + 字段类型 + 显式布局偏移）。
//   **两边对不上 ⇒ 丢弃该字段并在报告里逐条列出**（`emit-report` 的 omitted 段），
//   绝不"取其一"或"取平均值"。
//
// 出表**永不**包含元数据推导出来的偏移（计划 Step 10 原话）：IL 侧的显式布局偏移只做
// **一致性核对**（若某个字段在元数据里写死了偏移而 SOS 给出另一个数 ⇒ 丢弃并报告）。

use crate::ilmeta::IlInventory;
use crate::names::{canonical_type, is_truncated, open_generic};
use crate::sos::{SosIntermediate, SosRow, SosRuntimeRow};
use crate::spec::{GAME_BASE_CONTAINS, RUNTIME_MIN_PROBES, RUNTIME_TYPEDEF_NAMESPACES, WANTED};
use std::collections::BTreeMap;
use std::collections::BTreeSet;

/// fixture 标记：出现在文件名与表内 `verified_build`/`evidence` 两处。
///
/// 存在的理由（硬约束）：**绝不允许**把 fixture 的数当成真实偏移表。
/// 只要输入里任何一个带 `#fixture true`，出的表就必然带这个标记；不想要的
/// 程度由调用方的 `--allow-fixtures` 控制（没这个开关就直接拒跑）。
pub const FIXTURE_MARK: &str = "EXAMPLE-FIXTURE";

/// identity 关键字段（报告里单独一行给结论；缺哪个直接点名）。
const IDENTITY_CRITICAL: &[(&str, &str)] = &[
    ("beatmap_info", "<MD5Hash>k__BackingField"),
    ("beatmap_info", "<Hash>k__BackingField"),
    ("beatmap_info", "<OnlineID>k__BackingField"),
    ("beatmap_info", "<DifficultyName>k__BackingField"),
    ("beatmap_info", "<Metadata>k__BackingField"),
    ("beatmap_info", "<BeatmapSet>k__BackingField"),
    ("beatmap_set", "<OnlineID>k__BackingField"),
    ("metadata", "<Title>k__BackingField"),
];

/// 一个出表字段（含两侧见证行，便于报告与追溯）。
#[derive(Clone, Debug)]
pub struct Published {
    pub label: String,
    pub object_type: String,
    pub key_type: String,
    pub field: String,
    pub offset: i64,
    pub sos_line: String,
    pub il_line: String,
    pub deref: String,
}

/// 一个被丢弃的字段 + 逐字原因。
#[derive(Clone, Debug)]
pub struct Omitted {
    pub label: String,
    pub field: String,
    pub reason: String,
    pub detail: String,
}

/// 出表结果。
#[derive(Clone, Debug)]
pub struct EmitOutcome {
    pub lazer_version: String,
    pub runtime_version: String,
    pub arch: String,
    pub game_base_vtable: Option<u64>,
    pub verified_build: String,
    pub evidence: String,
    pub published: Vec<Published>,
    pub omitted: Vec<Omitted>,
    pub warnings: Vec<String>,
    /// **运行期结构段**（Step 10f；`None` = 本次输入没有这条链的见证 ⇒ 不写 `runtime` 键，
    /// 读侧随后对 `state.{name,number}` 按字段级降级）。
    pub runtime: Option<RuntimeOutcome>,
}

/// 出表的 `runtime` 段（Step 10f；形状见 `offsets.rs` 的文件头）。
#[derive(Clone, Debug)]
pub struct RuntimeOutcome {
    /// 发布的位移行（`spec.rs::RUNTIME_PROBES` 的每一组都要在）。
    pub rows: Vec<SosRuntimeRow>,
    pub typedefs: BTreeMap<String, BTreeMap<String, String>>,
    pub observed: BTreeMap<String, BTreeMap<String, String>>,
    pub witness: String,
}

impl EmitOutcome {
    /// `types` 段：规范类型名 → 字段 → 偏移（`BTreeMap` 保证输出可复现）。
    pub fn types(&self) -> BTreeMap<String, BTreeMap<String, i64>> {
        let mut map: BTreeMap<String, BTreeMap<String, i64>> = BTreeMap::new();
        for row in &self.published {
            map.entry(row.key_type.clone())
                .or_default()
                .insert(row.field.clone(), row.offset);
        }
        map
    }

    /// 是否带 fixture 标记。
    pub fn is_fixture(&self) -> bool {
        self.verified_build.contains(FIXTURE_MARK) || self.evidence.contains(FIXTURE_MARK)
    }

    /// 表文件名（`<ver>__<rt>__<arch>.json`；fixture 带前缀）。
    pub fn file_name(&self) -> String {
        let prefix = if self.is_fixture() {
            format!("{FIXTURE_MARK}-")
        } else {
            String::new()
        };
        format!(
            "{prefix}{}__{}__{}.json",
            sanitize(&self.lazer_version),
            sanitize(&self.runtime_version),
            sanitize(&self.arch)
        )
    }

    /// 表 JSON（与 `offsets.rs` 的 `OffsetTable` 逐字段同形）。
    pub fn to_json(&self) -> String {
        let mut out = String::new();
        out.push_str("{\n");
        out.push_str(&format!("  \"lazer_version\": {},\n", json_string(&self.lazer_version)));
        out.push_str(&format!(
            "  \"runtime_version\": {},\n",
            json_string(&self.runtime_version)
        ));
        out.push_str(&format!("  \"arch\": {},\n", json_string(&self.arch)));
        match self.game_base_vtable {
            Some(value) => out.push_str(&format!("  \"game_base_vtable\": {value},\n")),
            None => out.push_str("  \"game_base_vtable\": null,\n"),
        }
        out.push_str("  \"types\": {\n");
        let types = self.types();
        let type_count = types.len();
        for (index, (type_name, fields)) in types.iter().enumerate() {
            out.push_str(&format!("    {}: {{\n", json_string(type_name)));
            let field_count = fields.len();
            for (field_index, (field, offset)) in fields.iter().enumerate() {
                out.push_str(&format!(
                    "      {}: {}{}\n",
                    json_string(field),
                    offset,
                    if field_index + 1 == field_count { "" } else { "," }
                ));
            }
            out.push_str(&format!(
                "    }}{}\n",
                if index + 1 == type_count { "" } else { "," }
            ));
        }
        out.push_str("  },\n");
        match &self.runtime {
            Some(runtime) => out.push_str(&Self::runtime_json(runtime)),
            None => out.push_str("  \"runtime\": null,\n"),
        }
        out.push_str(&format!(
            "  \"verified_build\": {},\n",
            json_string(&self.verified_build)
        ));
        out.push_str(&format!("  \"evidence\": {}\n", json_string(&self.evidence)));
        out.push_str("}\n");
        out
    }

    /// `runtime` 段的 JSON（形状与 `offsets.rs::RuntimeSection` 逐字段同形）。
    fn runtime_json(runtime: &RuntimeOutcome) -> String {
        let mut out = String::new();
        out.push_str("  \"runtime\": {\n");
        for (group, heading) in [
            ("eetype", "eetype"),
            ("module", "module"),
            ("screen_array", "screen_array"),
        ] {
            out.push_str(&format!("    {}: {{\n", json_string(heading)));
            let rows: Vec<&SosRuntimeRow> = runtime
                .rows
                .iter()
                .filter(|row| row.group == group)
                .collect();
            for (index, row) in rows.iter().enumerate() {
                let mut fields = vec![format!("\"offset\": {}", row.offset)];
                if row.shift != 0 {
                    fields.push(format!("\"shift\": {}", row.shift));
                }
                if row.stride != 0 {
                    fields.push(format!("\"stride\": {}", row.stride));
                }
                fields.push(format!("\"witness\": {}", json_string(&row.witness)));
                out.push_str(&format!(
                    "      {}: {{{}}}{}\n",
                    json_string(&row.name),
                    fields.join(", "),
                    if index + 1 == rows.len() { "" } else { "," }
                ));
            }
            out.push_str("    },\n");
        }
        out.push_str("    \"typedefs\": ");
        out.push_str(&runtime_map_json(&runtime.typedefs, 4));
        out.push_str(",\n");
        out.push_str("    \"observed\": ");
        out.push_str(&runtime_map_json(&runtime.observed, 4));
        out.push_str(",\n");
        out.push_str(&format!(
            "    \"witness\": {}\n",
            json_string(&runtime.witness)
        ));
        out.push_str("  },\n");
        out
    }

    /// 报告文本（出表 + 丢弃逐条）。
    pub fn report(&self, sos_path: &str, il_path: &str) -> String {
        let mut out = String::new();
        out.push_str("=== lazer-offsets-gen emit report ===\n");
        out.push_str(&format!("sos intermediate : {sos_path}\n"));
        out.push_str(&format!("il inventory     : {il_path}\n"));
        out.push_str(&format!(
            "table key        : lazer={} runtime={} arch={}\n",
            self.lazer_version, self.runtime_version, self.arch
        ));
        out.push_str(&format!(
            "game_base_vtable : {}\n",
            self.game_base_vtable
                .map(|v| format!("0x{v:X}"))
                .unwrap_or_else(|| "null".to_string())
        ));
        out.push_str(&format!(
            "fixture          : {}\n",
            if self.is_fixture() { FIXTURE_MARK } else { "no" }
        ));
        out.push_str(&format!(
            "published        : {} field(s) over {} type(s)\n",
            self.published.len(),
            self.types().len()
        ));
        for row in &self.published {
            out.push_str(&format!(
                "  + {}.{}\t= {}\t[{} {}]\tsos: {}\til: {}\n",
                row.key_type, row.field, row.offset, row.label, row.deref, row.sos_line, row.il_line
            ));
        }
        out.push_str(&format!("omitted          : {}\n", self.omitted.len()));
        for row in &self.omitted {
            out.push_str(&format!(
                "  - {}.{}\t{}\t{}\n",
                row.label, row.field, row.reason, row.detail
            ));
        }
        match &self.runtime {
            Some(runtime) => {
                let typedef_count: usize =
                    runtime.typedefs.values().map(|map| map.len()).sum();
                let observed_count: usize =
                    runtime.observed.values().map(|map| map.len()).sum();
                out.push_str(&format!(
                    "runtime          : {} structure offset(s), {} typedef name(s) over {} module(s), \
                     {} observed by dumpmt\n",
                    runtime.rows.len(),
                    typedef_count,
                    runtime.typedefs.len(),
                    observed_count
                ));
                for row in &runtime.rows {
                    out.push_str(&format!(
                        "  = {}.{}\t= {}\tshift {}\tstride {}\t[{} probe(s)]\t{}\n",
                        row.group, row.name, row.offset, row.shift, row.stride, row.probes, row.witness
                    ));
                }
                out.push_str(&format!("  witness\t{}\n", runtime.witness));
            }
            None => out.push_str("runtime          : NOT published (see the omitted list)\n"),
        }
        let mut missing = Vec::new();
        for (label, field) in IDENTITY_CRITICAL {
            if !self
                .published
                .iter()
                .any(|row| row.label == *label && row.field == *field)
            {
                missing.push(format!("{label}.{field}"));
            }
        }
        out.push_str(&format!(
            "identity-critical: {}/{} published\n",
            IDENTITY_CRITICAL.len() - missing.len(),
            IDENTITY_CRITICAL.len()
        ));
        for item in &missing {
            out.push_str(&format!("  ! MISSING {item}\n"));
        }
        for warning in &self.warnings {
            out.push_str(&format!("warning: {warning}\n"));
        }
        out
    }
}

/// `模块名 → {RID → 类型名}` 的 JSON（嵌套对象；缩进由调用方给一级）。
fn runtime_map_json(map: &BTreeMap<String, BTreeMap<String, String>>, indent: usize) -> String {
    if map.is_empty() {
        return "{}".to_string();
    }
    let inner = " ".repeat(indent);
    let entry_pad = " ".repeat(indent + 2);
    let mut out = String::from("{\n");
    let total = map.len();
    for (index, (module, entries)) in map.iter().enumerate() {
        out.push_str(&format!("{entry_pad}{}: {{\n", json_string(module)));
        let inner_total = entries.len();
        for (entry_index, (rid, name)) in entries.iter().enumerate() {
            out.push_str(&format!(
                "{entry_pad}  {}: {}{}\n",
                json_string(rid),
                json_string(name),
                if entry_index + 1 == inner_total { "" } else { "," }
            ));
        }
        out.push_str(&format!(
            "{entry_pad}}}{}\n",
            if index + 1 == total { "" } else { "," }
        ));
    }
    out.push_str(&format!("{inner}}}"));
    out
}

fn sanitize(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// 出表的前置检查错误（调用方把它当"拒绝"处理，退出码 3）。
pub struct Refusal(pub String);

/// 执行校验 + 出表。
///
/// `allow_fixtures`：允许 fixture 输入（**且必然给结果打上 fixture 标记**）。
/// `allow_version_mismatch`：允许 IL 侧与 dump 侧的版本不一致（用于"按上游 tag 自建
/// 程序集"的 IL 来源；不一致会写进 `evidence`，不会消失）。
pub fn emit(
    sos: &SosIntermediate,
    il: &IlInventory,
    allow_fixtures: bool,
    allow_version_mismatch: bool,
) -> Result<EmitOutcome, Refusal> {
    // ① fixture 门：不带标记的输出只可能来自真实 dump。
    let sos_fixture = sos.is_fixture();
    let fixture = sos_fixture || il.is_fixture();
    if fixture && !allow_fixtures {
        return Err(Refusal(format!(
            "input is marked as a fixture (`#fixture true`): refusing to emit a table from it \
             (pass --allow-fixtures only to exercise the pipeline; the emitted table will be \
             stamped {FIXTURE_MARK} in its file name and inside `evidence`/`verified_build`)"
        )));
    }
    // ② 溯源门：真实表必须来自 dump；且 dump 派生的中间件必须带解引用自证。
    //    判据只看 **SOS 中间件**（IL 侧的 fixture 标记不该影响对"偏移从哪来"的判断）。
    if !sos.is_from_dump() && !sos_fixture {
        return Err(Refusal(format!(
            "sos intermediate provenance is {:?}, not `dump`: refusing (a table may only come \
             from a real dump or from a marked fixture)",
            sos.get("provenance").unwrap_or("<missing>")
        )));
    }
    if sos.is_from_dump() {
        let checked: u64 = sos
            .get("deref_checked")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        if checked == 0 {
            return Err(Refusal(
                "sos intermediate says provenance=dump but deref_checked=0: the dump-byte \
                 witness step did not run (refusing to emit a table whose offsets were never \
                 dereferenced against the dump)"
                    .to_string(),
            ));
        }
    }
    // ③ 版本门（`offsets.rs::load` 也要求这三个字段非空）。
    let lazer_version = required(sos, "lazer_version")?;
    let runtime_version = required(sos, "runtime_version")?;
    let arch = required(sos, "arch")?;
    let mut warnings = Vec::new();
    if let Some(il_dir) = il.get("lazer_dir") {
        if !il
            .get("assemblies_managed")
            .map(|v| v != "0")
            .unwrap_or(true)
        {
            return Err(Refusal(format!(
                "il inventory from {il_dir} contains no managed assembly (assemblies_managed=0)"
            )));
        }
    }
    // 逐模块版本核对：dump 的模块文件版本 vs IL 那边的程序集版本（同名文件才比）。
    for key in sos.provenance.keys() {
        let Some(module) = key.strip_prefix("dump_module_version:") else {
            continue;
        };
        let Some(dump_version) = sos.get(key) else {
            continue;
        };
        let Some(il_value) = il.get(&format!("assembly:{module}")) else {
            continue;
        };
        let il_version = il_value.split('\t').next().unwrap_or_default();
        if il_version.is_empty() || dump_version.is_empty() {
            continue;
        }
        if !versions_agree(dump_version, il_version) {
            let message = format!(
                "version mismatch for {module}: dump={dump_version} il={il_version}"
            );
            if allow_version_mismatch {
                warnings.push(format!("{message} (allowed by --allow-version-mismatch)"));
            } else {
                return Err(Refusal(format!(
                    "{message} — the SOS and IL witnesses come from different builds; \
                     re-collect one of them, or pass --allow-version-mismatch to record the \
                     mismatch in `evidence` and continue"
                )));
            }
        }
    }

    // ④ 逐字段双见证校验。
    let mut published: Vec<Published> = Vec::new();
    let mut omitted: Vec<Omitted> = Vec::new();
    let mut seen: BTreeMap<(String, String), i64> = BTreeMap::new();
    for wanted in WANTED {
        let mut push_omit = |reason: &str, detail: String| {
            omitted.push(Omitted {
                label: wanted.object.to_string(),
                field: wanted.field.to_string(),
                reason: reason.to_string(),
                detail,
            });
        };
        let Some(object) = sos.object(wanted.object) else {
            push_omit("chain-missing", format!("no object `{}` in the intermediate", wanted.object));
            continue;
        };
        let rows = sos.rows_for(object.address, wanted.field);
        if rows.is_empty() {
            push_omit(
                "no-sos-row",
                format!(
                    "`{}` has no field `{}` in the dumpobj output ({})",
                    object.object_type, wanted.field, object.status
                ),
            );
            continue;
        }
        if !starts_with_witness(&object.status) {
            push_omit(
                "chain-status",
                format!("object `{}` status is {}", wanted.object, object.status),
            );
            continue;
        }
        let mut offsets: BTreeSet<i64> = BTreeSet::new();
        for row in &rows {
            offsets.insert(row.offset);
        }
        if offsets.len() > 1 {
            push_omit(
                "ambiguous-sos-row",
                format!(
                    "`{}` prints field `{}` with {} different offsets: {}",
                    object.object_type,
                    wanted.field,
                    offsets.len(),
                    offsets
                        .iter()
                        .map(|v| v.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            );
            continue;
        }
        let row: &SosRow = rows[0];
        if row.attr != "instance" {
            push_omit(
                "sos-non-instance",
                format!("SOS attr is `{}` (offset {} is not an instance layout offset)", row.attr, row.offset),
            );
            continue;
        }
        if row.deref.starts_with("mismatch") {
            push_omit("deref-mismatch", row.deref.clone());
            continue;
        }
        let key_type = canonical_type(&object.object_type);
        let open = open_generic(&key_type).to_string();
        let il_rows = il.find(&key_type, &open, wanted.field);
        if il_rows.is_empty() {
            push_omit(
                "no-il-row",
                format!(
                    "no field `{}` on `{}`/`{}` in the IL inventory",
                    wanted.field, key_type, open
                ),
            );
            continue;
        }
        let mut kinds: BTreeSet<String> = BTreeSet::new();
        for candidate in &il_rows {
            kinds.insert(format!("{} {}", candidate.kind, candidate.type_display));
        }
        if kinds.len() > 1 {
            push_omit(
                "il-ambiguous",
                format!(
                    "the IL inventory has {} different declarations of `{}` on `{open}`: {}",
                    kinds.len(),
                    wanted.field,
                    kinds.iter().cloned().collect::<Vec<_>>().join(" | ")
                ),
            );
            continue;
        }
        let il_row = il_rows[0];
        if il_row.kind != "instance" {
            push_omit(
                "il-static",
                format!(
                    "metadata says `{}.{}` is {} (assembly {}, token {})",
                    il_row.type_name, il_row.field, il_row.kind, il_row.assembly, il_row.token
                ),
            );
            continue;
        }
        if row.sos_type.trim().is_empty() {
            push_omit(
                "sos-type-missing",
                format!(
                    "SOS printed no type for `{}` (nothing to corroborate against the IL row `{}`)",
                    wanted.field, il_row.type_display
                ),
            );
            continue;
        }
        if let Err(detail) = type_agrees(&row.sos_type, &row.vt, il_row) {
            push_omit("il-type-mismatch", detail);
            continue;
        }
        if let Some(explicit) = il_row.explicit_offset {
            if explicit as i64 != row.offset {
                push_omit(
                    "il-explicit-offset-mismatch",
                    format!(
                        "metadata writes an explicit layout offset {explicit} but SOS says {}",
                        row.offset
                    ),
                );
                continue;
            }
        }
        let key = (key_type.clone(), wanted.field.to_string());
        if let Some(previous) = seen.get(&key) {
            if *previous != row.offset {
                push_omit(
                    "duplicate-key-conflict",
                    format!(
                        "`{key_type}.{}` was already published with offset {previous} but this \
                         object (`{}`) gives {}",
                        wanted.field, wanted.object, row.offset
                    ),
                );
                continue;
            }
        }
        seen.insert(key, row.offset);
        published.push(Published {
            label: wanted.object.to_string(),
            object_type: object.object_type.clone(),
            key_type,
            field: wanted.field.to_string(),
            offset: row.offset,
            sos_line: format!(
                "{} offset {} type `{}` (attr {}, {} bytes at dump-address 0x{:X})",
                row.token, row.offset, row.sos_type, row.attr, row.module, object.address
            ),
            il_line: format!(
                "{} `{}.{}` {} {} token {} order {}{}",
                il_row.assembly,
                il_row.type_name,
                il_row.field,
                il_row.kind,
                il_row.type_display,
                il_row.token,
                il_row.order,
                il_row
                    .explicit_offset
                    .map(|v| format!(" explicit-offset {v}"))
                    .unwrap_or_default()
            ),
            deref: row.deref.clone(),
        });
    }

    if published.is_empty() {
        return Err(Refusal(
            "no field survived the two-witness check (see the omitted list above)".to_string(),
        ));
    }

    // ④b **运行期结构段**（Step 10f）：`state.name` 那条链的位移与 RID→类型名 表。
    //    - 位移行必须齐（`spec.rs::RUNTIME_PROBES` 的每一组）且探针数达标；
    //    - RID→类型名必须同时有 IL 元数据行与 `dumpmt` 的观察行（`source=il+observed`），
    //      且两边逐字相等（否则该行**不进表**）；
    //    - `[gameBase]` 的类型必须被观察到（读侧的 `runtime_probe` 要靠它做结构证明）。
    let (runtime, runtime_omitted, runtime_warning) = build_runtime(sos, il);
    omitted.extend(runtime_omitted);
    if let Some(warning) = runtime_warning {
        warnings.push(warning);
    }

    // ⑤ `verified_build` 与 `evidence`：写清"哪个构建、怎么提取、怎么自证"。
    let pointer_proof = sos
        .get("game_base_mt")
        .map(|mt| format!("game_base vtable {mt}"))
        .unwrap_or_else(|| "game_base vtable <not extracted>".to_string());
    let verified_build_core = match sos.get("lazer_exe_sha256") {
        Some(hash) if hash != "unavailable" => format!(
            "osu!lazer {} / .NET {} / {} / {} sha256:{} / {}",
            lazer_version,
            runtime_version,
            arch,
            sos.get("lazer_exe").unwrap_or("osu!.exe"),
            hash,
            pointer_proof
        ),
        _ => format!(
            "osu!lazer {} / .NET {} / {} / {} hash unavailable / {}",
            lazer_version,
            runtime_version,
            arch,
            sos.get("lazer_exe").unwrap_or("osu!.exe"),
            pointer_proof
        ),
    };
    let verified_build = if fixture {
        format!("{FIXTURE_MARK}/ {verified_build_core}")
    } else {
        verified_build_core
    };
    let deref_checked = sos.get("deref_checked").unwrap_or("0");
    let deref_ok = sos.get("deref_ok").unwrap_or("0");
    let deref_mismatch = sos.get("deref_mismatch").unwrap_or("0");
    let dump = sos.get("dump").unwrap_or("<unknown dump>");
    let transcript = sos.get("transcript").unwrap_or("<unknown transcript>");
    let analyzer = sos.get("analyzer").unwrap_or("<unknown analyzer>");
    let il_dir = il.get("lazer_dir").unwrap_or("<unknown lazer dir>");
    let mut evidence = format!(
        "{}SOS offsets from dump `{}` (pid {}, {}, anchor {} hit(s) at {}, game_base {}), \
         transcript `{}`, analyzer `{}`; SOS offsets proven against the dump bytes \
         ({} checked / {} ok / {} mismatch — the same dump proves the object-address base \
         convention); IL structure from assemblies under `{}` ({} managed); \
         IL corroborates field existence, instance-ness and type; metadata-derived offsets are \
         never published; generator tools/lazer-offsets-gen (DEC-23). \
         Emitted {} field(s) of {} wanted; omitted {} (see the emit report).",
        if fixture { format!("{FIXTURE_MARK} EXAMPLE DATA — not a real dump. ") } else { String::new() },
        dump,
        sos.get("dump_pid").unwrap_or("?"),
        arch,
        sos.get("anchor_hits").unwrap_or("?"),
        sos.get("anchor").unwrap_or("?"),
        sos.get("game_base").unwrap_or("?"),
        transcript,
        analyzer,
        deref_checked,
        deref_ok,
        deref_mismatch,
        il_dir,
        il.get("assemblies_managed").unwrap_or("?"),
        published.len(),
        WANTED.len(),
        omitted.len()
    );
    if !warnings.is_empty() {
        evidence.push_str(" Warnings: ");
        evidence.push_str(&warnings.join("; "));
        evidence.push('.');
    }
    // Step 10f：运行期结构段的自证（哪些位移、多少个 RID、观察了几个）。
    match &runtime {
        Some(outcome) => {
            let typedef_count: usize = outcome.typedefs.values().map(|map| map.len()).sum();
            let observed_count: usize = outcome.observed.values().map(|map| map.len()).sum();
            evidence.push_str(&format!(
                " Runtime structure section (state.name): {} offset(s) derived from the same dump \
                 (`dumpmt`/`dumpmodule`/`dumparray` prints vs dump bytes, >= {} agreeing probe(s) each: \
                 {}), {} TypeDef RID→name row(s) over {} module(s) from the IL metadata \
                 ({} corroborated by dumpmt; IL-derived values are not offsets).",
                outcome.rows.len(),
                RUNTIME_MIN_PROBES,
                outcome
                    .rows
                    .iter()
                    .map(|row| format!("{}.{}=+{:#x}", row.group, row.name, row.offset))
                    .collect::<Vec<_>>()
                    .join(", "),
                typedef_count,
                outcome.typedefs.len(),
                observed_count
            ));
        }
        None => evidence.push_str(
            " Runtime structure section (state.name) was NOT published in this run (see the emit \
             report): state.name/state.number degrade to field-level on the reader side.",
        ),
    }

    let game_base_vtable = sos
        .get("game_base_mt")
        .and_then(|text| {
            let text = text.trim().trim_start_matches("0x");
            u64::from_str_radix(text, 16).ok()
        });

    Ok(EmitOutcome {
        lazer_version,
        runtime_version,
        arch,
        game_base_vtable,
        verified_build,
        evidence,
        published,
        omitted,
        warnings,
        runtime,
    })
}

/// **运行期结构段的双见证校验**（Step 10f；规则写在文件头与 `spec.rs::RUNTIME_PROBES`）。
///
/// 返回 `(段, 丢弃清单, 警告)`：
/// - 五条位移行的探针数必须 ≥ [`RUNTIME_MIN_PROBES`]，缺一条就**整段不发布**（`None`），
///   并把缺失逐条写进报告（读侧随后按字段级降级）；
/// - RID→类型名：IL 元数据的 `type` 行是**结构**见证，`dumpmt` 的观察是**SOS**见证；
///   每一条被观察到的 `(模块, RID)` 都必须在 IL 里存在且名字逐字相等（不等 ⇒ 该行丢弃 +
///   报告），被观察到的行一律收录；此外按 [`RUNTIME_TYPEDEF_NAMESPACES`] 收录"屏幕实现"的
///   全量类型名（`source=il`）。
fn build_runtime(
    sos: &SosIntermediate,
    il: &IlInventory,
) -> (Option<RuntimeOutcome>, Vec<Omitted>, Option<String>) {
    let mut omitted: Vec<Omitted> = Vec::new();
    let mut rows: Vec<SosRuntimeRow> = Vec::new();
    for probe in crate::spec::RUNTIME_PROBES {
        match sos.runtime_row(probe.group, probe.name) {
            None => omitted.push(Omitted {
                label: probe.group.to_string(),
                field: probe.name.to_string(),
                reason: "no-runtime-row".to_string(),
                detail: "the extract step published no runtime row for this structure offset"
                    .to_string(),
            }),
            Some(row) if row.probes < RUNTIME_MIN_PROBES => omitted.push(Omitted {
                label: probe.group.to_string(),
                field: probe.name.to_string(),
                reason: "runtime-probes-too-few".to_string(),
                detail: format!(
                    "{} probe(s) agreed (need >= {RUNTIME_MIN_PROBES}): {}",
                    row.probes, row.witness
                ),
            }),
            Some(row) if row.offset < 0 => omitted.push(Omitted {
                label: probe.group.to_string(),
                field: probe.name.to_string(),
                reason: "runtime-offset-out-of-range".to_string(),
                detail: format!("offset {} ({})", row.offset, row.witness),
            }),
            Some(row) => rows.push(row.clone()),
        }
    }
    if rows.len() != crate::spec::RUNTIME_PROBES.len() {
        return (
            None,
            omitted,
            Some(format!(
                "runtime section not published: {} of {} structure offset(s) missing or ambiguous \
                 (state.name/state.number will degrade on the reader side)",
                crate::spec::RUNTIME_PROBES.len() - rows.len(),
                crate::spec::RUNTIME_PROBES.len()
            )),
        );
    }

    // RID→类型名：观察行（SOS 见证）与 IL 行（结构见证）逐字对照。
    let mut typedefs: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    let mut observed: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    let mut observed_rows = 0usize;
    let mut game_observed = false;
    for row in sos.typedefs.iter().filter(|row| row.source.contains("observed")) {
        let key = format!("{:X}", row.rid);
        match il.type_name(&row.module, row.rid) {
            None => {
                omitted.push(Omitted {
                    label: format!("typedef:{}", row.module),
                    field: key,
                    reason: "typedef-no-il-row".to_string(),
                    detail: format!(
                        "dumpmt observed `{}` but the IL inventory has no TypeDef RID {:#X} in {} \
                         ({})",
                        row.name, row.rid, row.module, row.witness
                    ),
                });
                continue;
            }
            Some(il_name) if il_name != row.name => {
                omitted.push(Omitted {
                    label: format!("typedef:{}", row.module),
                    field: key,
                    reason: "typedef-mismatch".to_string(),
                    detail: format!("dumpmt says `{}` but metadata says `{il_name}`", row.name),
                });
                continue;
            }
            Some(_) => {}
        }
        observed_rows += 1;
        if GAME_BASE_CONTAINS
            .iter()
            .any(|needle| row.name.contains(needle))
        {
            game_observed = true;
        }
        typedefs
            .entry(row.module.clone())
            .or_default()
            .insert(key.clone(), row.name.clone());
        observed
            .entry(row.module.clone())
            .or_default()
            .insert(key, row.name.clone());
    }
    if observed_rows == 0 || !game_observed {
        return (
            None,
            omitted,
            Some(format!(
                "runtime section not published: {} observed TypeDef row(s) (GameBase observed: \
                 {game_observed}) — the RID→name mechanism needs at least one dumpmt-observed row, \
                 including the GameBase type (the reader's runtime proof uses it)",
                observed_rows
            )),
        );
    }
    // 收录集：观察到的（上面已进表）+ "屏幕实现"命名空间的全量类型名（`source=il`）。
    for row in il.types_with_prefixes(RUNTIME_TYPEDEF_NAMESPACES) {
        typedefs
            .entry(row.assembly.clone())
            .or_default()
            .insert(format!("{:X}", row.rid), row.name.clone());
    }
    let typedef_count: usize = typedefs.values().map(|map| map.len()).sum();
    let witness = format!(
        "runtime offsets witnessed on the same dump ({} probe(s) per offset; SOS prints vs dump \
         bytes): {}; typedefs: {} name(s) over {} module(s), {} of them observed by dumpmt \
         (IL metadata rows corroborate every observed RID)",
        RUNTIME_MIN_PROBES,
        rows.iter()
            .map(|row| format!(
                "{}.{}=+{:#x}{}{}",
                row.group,
                row.name,
                row.offset,
                if row.stride != 0 {
                    format!("(stride {})", row.stride)
                } else {
                    String::new()
                },
                if row.shift != 0 {
                    format!("(shift {})", row.shift)
                } else {
                    String::new()
                }
            ))
            .collect::<Vec<_>>()
            .join(", "),
        typedef_count,
        typedefs.len(),
        observed_rows
    );
    (
        Some(RuntimeOutcome {
            rows,
            typedefs,
            observed,
            witness,
        }),
        omitted,
        None,
    )
}

fn required(sos: &SosIntermediate, key: &str) -> Result<String, Refusal> {
    match sos.get(key) {
        Some(value) if !value.trim().is_empty() => Ok(value.to_string()),
        _ => Err(Refusal(format!(
            "sos intermediate has no `{key}` (the emitted table would fail offsets.rs::load: empty {key})"
        ))),
    }
}

/// 对象状态是否算"这一步的结构见证成立"。
fn starts_with_witness(status: &str) -> bool {
    status.starts_with("ok") || status.starts_with("lenient")
}

/// 版本一致性：逐段比较，缺段按 0；任一段不同 ⇒ 不一致。
fn versions_agree(left: &str, right: &str) -> bool {
    let parse = |text: &str| -> Vec<u32> {
        text.split('.')
            .map(|part| part.trim().parse::<u32>().unwrap_or(0))
            .collect()
    };
    let left = parse(left);
    let right = parse(right);
    let len = left.len().max(right.len());
    for index in 0..len {
        if left.get(index).copied().unwrap_or(0) != right.get(index).copied().unwrap_or(0) {
            return false;
        }
    }
    true
}

/// 字段类型的跨见证一致性（规则写在 README 的判据表里）。
fn type_agrees(sos_type: &str, sos_vt: &str, il: &crate::ilmeta::IlField) -> Result<(), String> {
    let sos_type = sos_type.trim();
    if sos_type.is_empty() {
        return Err(format!(
            "SOS printed no type for `{}` (nothing to corroborate against IL `{}`)",
            il.field, il.type_display
        ));
    }
    if sos_type.contains("__Canon") {
        // `System.__Canon` = 泛型实参被规范化（实例化的字段类型印不出来）；`System.__Canon[]`
        // = **规范化元素的数组**（`Stack<T>._array` 实测就是它，IL 侧是 `!0[]` ⇒ kind `array`）。
        if il.type_kind == "var" {
            return Ok(());
        }
        if sos_type.contains("[]") && il.type_kind == "array" {
            return Ok(());
        }
        return Err(format!(
            "SOS prints `{sos_type}` (canonicalized generic instance) but metadata says `{}` ({})",
            il.type_display, il.type_kind
        ));
    }
    if is_truncated(sos_type) {
        // 列宽截断：只有"引用/值类型"这一层可比。截断尾巴里的程序集名不带类型信息，
        // 所以这里刻意**不**做字符串后缀匹配（那会变成一种猜测）。
        let reference = sos_vt == "No";
        let compatible = match il.type_kind.as_str() {
            "var" => true,
            "class" | "object" | "string" | "array" | "ptr" => reference,
            "valuetype" | "prim" => !reference,
            _ => false,
        };
        if compatible {
            return Ok(());
        }
        return Err(format!(
            "SOS type is truncated (`{sos_type}`, vt={sos_vt}) but metadata says `{}` ({})",
            il.type_display, il.type_kind
        ));
    }
    let left = canonical_type(sos_type);
    let right = canonical_type(&il.type_display);
    if left == right {
        return Ok(());
    }
    Err(format!(
        "SOS type `{left}` != metadata type `{right}` (metadata {})",
        il.type_display
    ))
}