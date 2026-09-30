// ilmeta.rs —— 直接从 CLR 程序集元数据里读**结构**（IL 侧的第二见证，零依赖）
//
// 为什么自己读 PE/CLI 元数据，而不是调 `ilspycmd` / Mono.Cecil：
// 生成器要在**离线、无网络、无额外工具**的环境里跑（lazer 更新当天就要用），
// 而且第三方反编译器只会给出"另一个数字"，不是"结构"。ECMA-335 的元数据里
// 逐字写着：某类型有哪些字段、字段名、静态还是实例、字段类型是什么、字段在类型里的
// 声明顺序、以及显式布局（`FieldLayout`）里的字面偏移。这就是我们要的第二见证。
//
// **元数据推导的偏移永不作为偏移发布**（计划 Step 10 原话）：本模块只提供
// ① 字段存在性/名字/静态性/类型 的结构行；② 显式布局类型（`[StructLayout(LayoutKind.Explicit)]`
// 或 `FieldOffset`）里**字面写死的**偏移。自动布局的偏移由运行时的布局器决定，
// 只能由 dump/SOS 给出——两者的分工写在 README 的判据表里。

use crate::names::canonical_type;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// IL 侧的一行：`(程序集, 类型, 字段) → 结构`。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IlField {
    pub assembly: String,
    pub type_name: String,
    pub field: String,
    /// `instance` / `static`。
    pub kind: String,
    /// 字段类型的种类（`prim` / `string` / `class` / `valuetype` / `var` / `array` / …）。
    pub type_kind: String,
    /// 字段类型的规范显示名（如 `System.String`、`osu.Game.Models.RealmUser`、`!0`）。
    pub type_display: String,
    /// 元数据 token（`0400xxxx`，同一程序集内唯一，跨版本 diff 用）。
    pub token: String,
    /// 显式布局里字面写死的偏移（`-` = 自动布局）。
    pub explicit_offset: Option<i32>,
    /// 该字段在类型里的声明顺序（1 起）。
    pub order: usize,
}

/// IL 侧的**类型行**（Step 10f）：`(程序集, TypeDef RID) → 完整类型名`。
///
/// 为什么需要它：运行期只能从 MethodTable 头部拿到 **TypeDef RID**（见 `spec.rs::RUNTIME_PROBES`
/// 的 `eetype.token`）；`RID → 名字` 是**构建期**的事实，只有元数据里有。RID 是
/// `TypeDef` 表的行号（1 起），与 SOS 的 `mdToken: 0200xxxx` 低 24 位逐位相等。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IlType {
    pub assembly: String,
    pub rid: u32,
    /// 完整类型名（嵌套用 `+`，与 SOS 的打印形态一致）。
    pub name: String,
}

/// IL 清单（可写盘、可读回、可 diff）。
#[derive(Clone, Debug, Default)]
pub struct IlInventory {
    pub provenance: BTreeMap<String, String>,
    pub fields: Vec<IlField>,
    /// **类型行**（Step 10f；`state.name` 的 `RID → 类型名` 来源）。旧清单可能没有这一段。
    pub types: Vec<IlType>,
    /// 继承链：派生类型 → 基类型（**开放泛型**形态）。必需，因为 SOS 把继承来的字段
    /// 打印在**对象自己的类型名**下，而元数据把字段挂在**声明它的那个类型**上
    /// （`dumpobj` 的 `<Storage>` 属于 `osu.Game.OsuGameBase`，而对象类型是
    /// `osu.Desktop.OsuGameDesktop`）。
    pub extends: BTreeMap<String, String>,
    pub notes: Vec<String>,
}

pub const IL_FORMAT: &str = "lazer-offsets-gen il-inventory v1";

impl IlInventory {
    pub fn is_fixture(&self) -> bool {
        self.provenance
            .get("fixture")
            .map(|v| v == "true")
            .unwrap_or(false)
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.provenance.get(key).map(|s| s.as_str())
    }

    /// 按规范类型名找字段：**沿基类链**找（SOS 的继承字段落在基类上），
    /// 泛型优先用开放定义键（元数据里只有开放定义）。返回全部命中，由调用方判歧义。
    pub fn find(&self, canonical: &str, open: &str, field: &str) -> Vec<&IlField> {
        let mut current = if self.has_type(open) {
            open.to_string()
        } else if self.has_type(canonical) {
            canonical.to_string()
        } else {
            open.to_string()
        };
        let mut guard = 0usize;
        loop {
            guard += 1;
            if guard > 64 {
                return Vec::new();
            }
            let hits: Vec<&IlField> = self
                .fields
                .iter()
                .filter(|row| row.field == field && row.type_name == current)
                .collect();
            if !hits.is_empty() {
                return hits;
            }
            match self.extends.get(&current) {
                Some(base) => current = crate::names::open_generic(base).to_string(),
                None => return Vec::new(),
            }
        }
    }

    fn has_type(&self, type_name: &str) -> bool {
        self.fields.iter().any(|row| row.type_name == type_name)
            || self.extends.contains_key(type_name)
    }

    /// `(程序集文件名, TypeDef RID)` → 完整类型名（Step 10f）。
    pub fn type_name(&self, assembly: &str, rid: u32) -> Option<&str> {
        self.types
            .iter()
            .find(|row| row.assembly.eq_ignore_ascii_case(assembly) && row.rid == rid)
            .map(|row| row.name.as_str())
            .filter(|name| !name.is_empty())
    }

    /// 某程序集里名字以 `prefixes` 之一开头的类型（`spec.rs::RUNTIME_TYPEDEF_NAMESPACES` 的用法）。
    pub fn types_with_prefixes(&self, prefixes: &[&str]) -> Vec<&IlType> {
        self.types
            .iter()
            .filter(|row| prefixes.iter().any(|prefix| row.name.starts_with(prefix)))
            .collect()
    }

    pub fn write_tsv(&self, path: &Path) -> Result<(), String> {
        let mut out = String::new();
        out.push_str(&format!("# {IL_FORMAT}\n"));
        for (key, value) in &self.provenance {
            out.push_str(&format!("#\t{key}\t{value}\n"));
        }
        out.push_str("#format\textends\tderived\tbase\n");
        for (derived, base) in &self.extends {
            out.push_str(&format!("extends\t{derived}\t{base}\n"));
        }
        out.push_str("#format\ttype\tassembly\trid\tname\n");
        for row in &self.types {
            out.push_str(&format!(
                "type\t{}\t{:X}\t{}\n",
                row.assembly,
                row.rid,
                row.name.replace(['\t', '\n', '\r'], " ")
            ));
        }
        out.push_str("#format\tfield\tassembly\ttype\tfield\tkind\ttype_kind\ttype_display\ttoken\texplicit_offset\torder\n");
        for row in &self.fields {
            out.push_str(&format!(
                "field\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                row.assembly,
                row.type_name,
                row.field,
                row.kind,
                row.type_kind,
                row.type_display,
                row.token,
                row.explicit_offset
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "-".to_string()),
                row.order
            ));
        }
        for note in &self.notes {
            out.push_str(&format!("#note\t{note}\n"));
        }
        fs::write(path, out).map_err(|e| format!("write {}: {e}", path.display()))
    }

    pub fn read_tsv(path: &Path) -> Result<IlInventory, String> {
        let text = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
        let mut parsed = IlInventory::default();
        let mut header_seen = false;
        for (index, line) in text.lines().enumerate() {
            let number = index + 1;
            if line.trim().is_empty() {
                continue;
            }
            if let Some(rest) = line.strip_prefix("#\t") {
                let mut parts = rest.splitn(2, '\t');
                let key = parts.next().unwrap_or_default().to_string();
                let value = parts.next().unwrap_or_default().to_string();
                if key == "format" {
                    continue;
                }
                if key == "note" {
                    parsed.notes.push(value);
                    continue;
                }
                parsed.provenance.insert(key, value);
                continue;
            }
            // 其它 `#` 开头的行一律是注释；缺格式头一律报错（见循环结束后的检查）。
            if let Some(rest) = line.strip_prefix("#note\t") {
                parsed.notes.push(rest.to_string());
                continue;
            }
            if let Some(rest) = line.strip_prefix('#') {
                if rest.contains(IL_FORMAT) {
                    header_seen = true;
                }
                continue;
            }
            let columns: Vec<&str> = line.split('\t').collect();
            if columns.first().copied() == Some("extends") {
                if columns.len() != 3 {
                    return Err(format!(
                        "{path:?}:{number}: extends row needs 3 columns, got {}",
                        columns.len()
                    ));
                }
                parsed
                    .extends
                    .insert(columns[1].to_string(), columns[2].to_string());
                continue;
            }
            if columns.first().copied() == Some("type") {
                if columns.len() != 4 {
                    return Err(format!(
                        "{path:?}:{number}: type row needs 4 columns, got {}",
                        columns.len()
                    ));
                }
                parsed.types.push(IlType {
                    assembly: columns[1].to_string(),
                    rid: u32::from_str_radix(columns[2].trim_start_matches("0x"), 16)
                        .map_err(|_| format!("{path:?}:{number}: bad rid {:?}", columns[2]))?,
                    name: columns[3].to_string(),
                });
                continue;
            }
            if columns.first().copied() != Some("field") {
                return Err(format!(
                    "{path:?}:{number}: unknown row tag {:?} (expected `field`, `extends` or `type`)",
                    columns.first().copied().unwrap_or("")
                ));
            }
            if columns.len() != 10 {
                return Err(format!(
                    "{path:?}:{number}: field row needs 10 columns, got {}",
                    columns.len()
                ));
            }
            let offset = match columns[8] {
                "-" => None,
                other => Some(
                    other
                        .parse::<i32>()
                        .map_err(|_| format!("{path:?}:{number}: bad explicit offset {other:?}"))?,
                ),
            };
            let order = columns[9]
                .parse::<usize>()
                .map_err(|_| format!("{path:?}:{number}: bad order {:?}", columns[9]))?;
            parsed.fields.push(IlField {
                assembly: columns[1].to_string(),
                type_name: columns[2].to_string(),
                field: columns[3].to_string(),
                kind: columns[4].to_string(),
                type_kind: columns[5].to_string(),
                type_display: columns[6].to_string(),
                token: columns[7].to_string(),
                explicit_offset: offset,
                order,
            });
        }
        if !header_seen {
            return Err(format!(
                "{path:?}: missing `# {IL_FORMAT}` header (not an il-inventory file)"
            ));
        }
        Ok(parsed)
    }
}

// ------------------------------------------------------------------ 采集 ----

/// 采集一个目录下所有托管程序集的字段结构。
///
/// 只读磁盘文件；不打开进程、不写任何东西。返回（清单、每个文件的处理结果）。
pub fn collect(lazer_dir: &Path, only: &[PathBuf]) -> Result<(IlInventory, Vec<String>), String> {
    let mut inventory = IlInventory::default();
    let mut report = Vec::new();
    inventory
        .provenance
        .insert("provenance".to_string(), "assembly".to_string());
    inventory
        .provenance
        .insert("lazer_dir".to_string(), lazer_dir.display().to_string());

    let mut candidates: Vec<PathBuf> = Vec::new();
    if only.is_empty() {
        let entries = fs::read_dir(lazer_dir)
            .map_err(|e| format!("read_dir {}: {e}", lazer_dir.display()))?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path
                .extension()
                .map(|e| e.eq_ignore_ascii_case("dll"))
                .unwrap_or(false)
            {
                candidates.push(path);
            }
        }
        candidates.sort();
    } else {
        candidates.extend(only.iter().cloned());
    }

    let mut managed = 0usize;
    let mut native = 0usize;
    let mut failed = 0usize;
    for path in candidates {
        match parse_assembly(&path) {
            Ok(Some(assembly)) => {
                managed += 1;
                let file_name = assembly.file_name.clone();
                let version = assembly.version.clone();
                let count = assembly.fields.len();
                let bases = assembly.extends.len();
                inventory.provenance.insert(
                    format!("assembly:{file_name}"),
                    format!("{version}\t{count} fields\t{}", path.display()),
                );
                report.push(format!(
                    "  {file_name:<40} {version:<16} {} field rows, {} base links",
                    count, bases
                ));
                inventory.fields.extend(assembly.fields);
                inventory.extends.extend(assembly.extends);
                inventory.types.extend(assembly.types);
            }
            Ok(None) => native += 1,
            Err(error) => {
                failed += 1;
                report.push(format!("  {}: {error}", path.display()));
            }
        }
    }
    inventory.provenance.insert(
        "assemblies_managed".to_string(),
        managed.to_string(),
    );
    inventory
        .provenance
        .insert("assemblies_native".to_string(), native.to_string());
    inventory
        .provenance
        .insert("assemblies_failed".to_string(), failed.to_string());
    inventory.provenance.insert(
        "field_rows".to_string(),
        inventory.fields.len().to_string(),
    );
    inventory.provenance.insert(
        "type_rows".to_string(),
        inventory.types.len().to_string(),
    );
    Ok((inventory, report))
}

/// 跨版本 diff：字段增删改（维护承诺的核心：lazer 每次更新跑一遍）。
pub fn diff(old: &IlInventory, new: &IlInventory) -> String {
    let key = |row: &IlField| format!("{}\t{}\t{}", row.assembly, row.type_name, row.field);
    let mut old_map: BTreeMap<String, &IlField> = BTreeMap::new();
    for row in &old.fields {
        old_map.insert(key(row), row);
    }
    let mut new_map: BTreeMap<String, &IlField> = BTreeMap::new();
    for row in &new.fields {
        new_map.insert(key(row), row);
    }
    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut changed = Vec::new();
    for (k, row) in &new_map {
        match old_map.get(k) {
            None => added.push(format!("{k}  ({})", row.type_display)),
            Some(previous) => {
                let mut differences = Vec::new();
                if previous.kind != row.kind {
                    differences.push(format!("kind {} -> {}", previous.kind, row.kind));
                }
                if previous.type_display != row.type_display {
                    differences.push(format!(
                        "type {} -> {}",
                        previous.type_display, row.type_display
                    ));
                }
                if previous.explicit_offset != row.explicit_offset {
                    differences.push(format!(
                        "explicit_offset {:?} -> {:?}",
                        previous.explicit_offset, row.explicit_offset
                    ));
                }
                if previous.order != row.order {
                    differences.push(format!("order {} -> {}", previous.order, row.order));
                }
                if !differences.is_empty() {
                    changed.push(format!("{k}  {}", differences.join("; ")));
                }
            }
        }
    }
    for (k, row) in &old_map {
        if !new_map.contains_key(k) {
            removed.push(format!("{k}  ({})", row.type_display));
        }
    }
    let old_label = old
        .get("lazer_dir")
        .unwrap_or("-")
        .to_string();
    let new_label = new.get("lazer_dir").unwrap_or("-").to_string();
    let mut out = String::new();
    out.push_str("=== lazer-offsets-gen il-diff (结构差分：字段增删改) ===\n");
    out.push_str(&format!("old : {old_label}\n"));
    out.push_str(&format!("new : {new_label}\n"));
    out.push_str(&format!(
        "rows: old {} -> new {} (added {} / removed {} / changed {})\n\n",
        old.fields.len(),
        new.fields.len(),
        added.len(),
        removed.len(),
        changed.len()
    ));
    push_section(&mut out, "ADDED", &added);
    push_section(&mut out, "REMOVED", &removed);
    push_section(&mut out, "CHANGED", &changed);
    out
}

fn push_section(out: &mut String, title: &str, rows: &[String]) {
    out.push_str(&format!("--- {title} ({}) ---\n", rows.len()));
    if rows.is_empty() {
        out.push_str("  (none)\n");
    }
    for row in rows {
        out.push_str(&format!("  {row}\n"));
    }
    out.push('\n');
}

// --------------------------------------------------------- 元数据解析器 ----

struct ParsedAssembly {
    file_name: String,
    version: String,
    fields: Vec<IlField>,
    extends: BTreeMap<String, String>,
    types: Vec<IlType>,
}

/// 读一个文件的元数据；`Ok(None)` = 不是托管程序集（没有 CLI 头）。
fn parse_assembly(path: &Path) -> Result<Option<ParsedAssembly>, String> {
    let bytes = fs::read(path).map_err(|e| format!("read: {e}"))?;
    if bytes.len() < 0x100 {
        return Ok(None);
    }
    let pe_offset = u32::from_le_bytes([bytes[0x3C], bytes[0x3D], bytes[0x3E], bytes[0x3F]]) as usize;
    if pe_offset + 0x20 > bytes.len() || &bytes[pe_offset..pe_offset + 4] != b"PE\0\0" {
        return Ok(None);
    }
    let sections = u16::from_le_bytes([bytes[pe_offset + 6], bytes[pe_offset + 7]]) as usize;
    let optional_size =
        u16::from_le_bytes([bytes[pe_offset + 20], bytes[pe_offset + 21]]) as usize;
    let optional = pe_offset + 24;
    if optional + 2 > bytes.len() {
        return Ok(None);
    }
    let magic = u16::from_le_bytes([bytes[optional], bytes[optional + 1]]);
    let directories = match magic {
        0x10B => optional + 96,
        0x20B => optional + 112,
        _ => return Ok(None),
    };
    let cli_directory = directories + 14 * 8;
    if cli_directory + 8 > bytes.len() {
        return Ok(None);
    }
    let cli_rva = u32::from_le_bytes([
        bytes[cli_directory],
        bytes[cli_directory + 1],
        bytes[cli_directory + 2],
        bytes[cli_directory + 3],
    ]);
    if cli_rva == 0 {
        return Ok(None); // 本机 DLL（没有 CLI 头）
    }
    let section_table = optional + optional_size;
    let mut sections_list: Vec<(u32, u32, u32, u32)> = Vec::new(); // (va, vsize, raw_ptr, raw_size)
    for index in 0..sections {
        let at = section_table + index * 40;
        if at + 40 > bytes.len() {
            return Err("section table runs past end of file".to_string());
        }
        let virtual_size = u32::from_le_bytes([bytes[at + 8], bytes[at + 9], bytes[at + 10], bytes[at + 11]]);
        let virtual_address =
            u32::from_le_bytes([bytes[at + 12], bytes[at + 13], bytes[at + 14], bytes[at + 15]]);
        let raw_size = u32::from_le_bytes([bytes[at + 16], bytes[at + 17], bytes[at + 18], bytes[at + 19]]);
        let raw_ptr = u32::from_le_bytes([bytes[at + 20], bytes[at + 21], bytes[at + 22], bytes[at + 23]]);
        sections_list.push((virtual_address, virtual_size.max(raw_size), raw_ptr, raw_size));
    }
    let rva_to_offset = |rva: u32| -> Option<usize> {
        for (va, size, raw_ptr, _) in &sections_list {
            if rva >= *va && rva < va + size {
                return Some((raw_ptr + (rva - va)) as usize);
            }
        }
        None
    };
    let cli_offset = rva_to_offset(cli_rva).ok_or("CLI header RVA is outside every section")?;
    if cli_offset + 16 > bytes.len() {
        return Err("CLI header runs past end of file".to_string());
    }
    let metadata_rva = u32::from_le_bytes([
        bytes[cli_offset + 8],
        bytes[cli_offset + 9],
        bytes[cli_offset + 10],
        bytes[cli_offset + 11],
    ]);
    let metadata_size = u32::from_le_bytes([
        bytes[cli_offset + 12],
        bytes[cli_offset + 13],
        bytes[cli_offset + 14],
        bytes[cli_offset + 15],
    ]) as usize;
    let metadata_offset =
        rva_to_offset(metadata_rva).ok_or("metadata RVA is outside every section")?;
    if metadata_offset + metadata_size > bytes.len() {
        return Err("metadata blob runs past end of file".to_string());
    }
    // 元数据 blob 单独拷出来：之后所有偏移都以 blob 起点为基准。
    let blob = bytes[metadata_offset..metadata_offset + metadata_size].to_vec();
    let file_name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let metadata = Metadata::parse(&blob, &file_name)?;
    let fields = metadata.build_fields(&file_name)?;
    let extends = metadata.extends_map();
    // 类型行（Step 10f）：`RID → 完整类型名`。RID = TypeDef 表行号（1 起），与运行期的
    // `mdToken & 0x00FFFFFF` 逐位相等。
    let types = (0..metadata.types.len())
        .map(|index| IlType {
            assembly: file_name.clone(),
            rid: index as u32 + 1,
            name: metadata.full_name(index),
        })
        .collect();
    Ok(Some(ParsedAssembly {
        file_name,
        version: metadata.assembly_version.clone(),
        fields,
        extends,
        types,
    }))
}

/// 表流里的一行区间（起止字节偏移）。
#[derive(Clone, Copy)]
struct TableLayout {
    offset: usize,
    row_size: usize,
    rows: u64,
}

struct Metadata {
    blob: Vec<u8>,
    strings_offset: usize,
    blob_offset: usize,
    string_index: usize,
    guid_index: usize,
    blob_index: usize,
    rows: [u64; 64],
    tables: Vec<Option<TableLayout>>,
    assembly_version: String,
    /// 每个 TypeDef 的（名字、命名空间、字段区间起、字段区间止）。
    types: Vec<TypeDefRow>,
    /// 每个 TypeDef 的 `Extends`（编码后的 TypeDefOrRef；0 = 没有基类）。
    type_extends: Vec<u64>,
    /// TypeRef 的（名字、命名空间）。
    type_refs: Vec<(String, String)>,
    /// 字段行（名字、签名偏移、flags）。
    field_rows: Vec<FieldRow>,
    /// 嵌套关系：嵌套 TypeDef index → 外层 TypeDef index。
    nesting: BTreeMap<usize, usize>,
    /// 显式布局：FieldDef index → 偏移。
    explicit: BTreeMap<usize, i32>,
}

struct TypeDefRow {
    name: String,
    namespace: String,
    field_start: usize, // 1 起的 FieldDef index
    field_end: usize,
}

struct FieldRow {
    name: String,
    flags: u16,
    signature_offset: usize,
}

const TABLE_MODULE: usize = 0x00;
const TABLE_TYPE_REF: usize = 0x01;
const TABLE_TYPE_DEF: usize = 0x02;
const TABLE_FIELD: usize = 0x04;
const TABLE_FIELD_LAYOUT: usize = 0x10;
const TABLE_ASSEMBLY: usize = 0x20;
const TABLE_NESTED_CLASS: usize = 0x29;

impl Metadata {
    fn parse(blob: &[u8], _file: &str) -> Result<Metadata, String> {
        if blob.len() < 0x20 || &blob[0..4] != b"BSJB" {
            return Err("metadata root signature is not BSJB".to_string());
        }
        let version_length = u32::from_le_bytes([blob[12], blob[13], blob[14], blob[15]]) as usize;
        let mut cursor = 16 + version_length;
        cursor = (cursor + 3) & !3;
        if cursor + 4 > blob.len() {
            return Err("metadata root is truncated".to_string());
        }
        let streams = u16::from_le_bytes([blob[cursor + 2], blob[cursor + 3]]) as usize;
        cursor += 4;
        let mut tables_stream: Option<(usize, usize)> = None;
        let mut strings_offset = 0usize;
        let mut blob_heap_offset = 0usize;
        for _ in 0..streams {
            if cursor + 8 > blob.len() {
                return Err("stream header runs past end of metadata".to_string());
            }
            let offset = u32::from_le_bytes([blob[cursor], blob[cursor + 1], blob[cursor + 2], blob[cursor + 3]]) as usize;
            let size = u32::from_le_bytes([blob[cursor + 4], blob[cursor + 5], blob[cursor + 6], blob[cursor + 7]]) as usize;
            let name_start = cursor + 8;
            let mut end = name_start;
            while end < blob.len() && blob[end] != 0 {
                end += 1;
            }
            let name = String::from_utf8_lossy(&blob[name_start..end]).to_string();
            match name.as_str() {
                "#~" => tables_stream = Some((offset, size)),
                "#-" => {
                    return Err(
                        "uncompressed metadata stream (#-) is not supported (only #~)".to_string(),
                    )
                }
                "#Strings" => strings_offset = offset,
                "#Blob" => blob_heap_offset = offset,
                _ => {}
            }
            cursor = (end + 4) & !3;
        }
        let (tables_offset, _tables_size) =
            tables_stream.ok_or("metadata has no #~ tables stream")?;
        let heap_sizes = blob[tables_offset + 6];
        let valid = u64::from_le_bytes(
            blob[tables_offset + 8..tables_offset + 16]
                .try_into()
                .unwrap(),
        );
        let mut rows = [0u64; 64];
        let mut cursor = tables_offset + 24;
        for (index, slot) in rows.iter_mut().enumerate() {
            if valid & (1u64 << index) != 0 {
                if cursor + 4 > blob.len() {
                    return Err("row-count table runs past end of metadata".to_string());
                }
                *slot = u32::from_le_bytes([
                    blob[cursor],
                    blob[cursor + 1],
                    blob[cursor + 2],
                    blob[cursor + 3],
                ]) as u64;
                cursor += 4;
            }
        }
        let string_index = if heap_sizes & 0x01 != 0 { 4 } else { 2 };
        let guid_index = if heap_sizes & 0x02 != 0 { 4 } else { 2 };
        let blob_index = if heap_sizes & 0x04 != 0 { 4 } else { 2 };
        let mut tables: Vec<Option<TableLayout>> = vec![None; 64];
        let mut table_cursor = cursor;
        for index in 0..64usize {
            if valid & (1u64 << index) == 0 {
                continue;
            }
            let Some(row_size) = row_size(index, &rows, string_index, guid_index, blob_index) else {
                return Err(format!(
                    "unsupported metadata table 0x{index:02X} (fail-closed: refusing to guess row sizes)"
                ));
            };
            tables[index] = Some(TableLayout {
                offset: table_cursor,
                row_size,
                rows: rows[index],
            });
            table_cursor += row_size * rows[index] as usize;
        }
        let mut metadata = Metadata {
            blob: blob.to_vec(),
            strings_offset,
            blob_offset: blob_heap_offset,
            string_index,
            guid_index,
            blob_index,
            rows,
            tables,
            assembly_version: String::new(),
            types: Vec::new(),
            type_extends: Vec::new(),
            type_refs: Vec::new(),
            field_rows: Vec::new(),
            nesting: BTreeMap::new(),
            explicit: BTreeMap::new(),
        };
        metadata.read_assembly()?;
        metadata.read_type_refs()?;
        metadata.read_types()?;
        metadata.read_fields()?;
        metadata.read_nesting()?;
        metadata.read_field_layout()?;
        Ok(metadata)
    }

    fn table(&self, index: usize) -> Option<&TableLayout> {
        self.tables.get(index).and_then(|slot| slot.as_ref())
    }

    fn row_start(&self, table: usize, row: u64) -> Option<usize> {
        let layout = self.table(table)?;
        if row == 0 || row > layout.rows {
            return None;
        }
        Some(layout.offset + (row as usize - 1) * layout.row_size)
    }

    fn read_string(&self, index: usize) -> Option<String> {
        let start = self.strings_offset.checked_add(index)?;
        if index == 0 || start >= self.blob.len() {
            return Some(String::new());
        }
        let mut end = start;
        while end < self.blob.len() && self.blob[end] != 0 {
            end += 1;
        }
        Some(String::from_utf8_lossy(&self.blob[start..end]).to_string())
    }

    fn read_index(&self, at: usize, width: usize) -> u64 {
        let mut value = 0u64;
        for byte in 0..width {
            value |= (self.blob[at + byte] as u64) << (8 * byte);
        }
        value
    }

    fn read_compressed(&self, at: usize) -> Option<(u32, usize)> {
        let first = *self.blob.get(at)?;
        if first & 0x80 == 0 {
            return Some((first as u32, 1));
        }
        if first & 0xC0 == 0x80 {
            let second = *self.blob.get(at + 1)?;
            return Some(((((first & 0x3F) as u32) << 8) | second as u32, 2));
        }
        if first & 0xE0 == 0xC0 {
            let b1 = *self.blob.get(at + 1)? as u32;
            let b2 = *self.blob.get(at + 2)? as u32;
            let b3 = *self.blob.get(at + 3)? as u32;
            return Some((((first & 0x1F) as u32) << 24 | b1 << 16 | b2 << 8 | b3, 4));
        }
        None
    }

    fn read_assembly(&mut self) -> Result<(), String> {
        let Some(layout) = self.table(TABLE_ASSEMBLY).copied() else {
            return Ok(());
        };
        if layout.rows == 0 {
            return Ok(());
        }
        let at = layout.offset;
        // HashAlgId(4) + Major(2) + Minor(2) + Build(2) + Rev(2) + Flags(4)
        let mut cursor = at + 16;
        // PublicKey 是 **#Blob 堆索引**（不是内联的压缩 blob——这里踩过一次）：
        // 索引宽度由 HeapSizes 决定，只跳过即可（我们不需要公钥本身）。
        cursor += self.blob_index;
        if cursor + self.string_index > self.blob.len() {
            return Err("assembly row runs past the end of the metadata".to_string());
        }
        let name_index = self.read_index(cursor, self.string_index) as usize;
        let name = self.read_string(name_index).unwrap_or_default();
        let major = u16::from_le_bytes([self.blob[at + 4], self.blob[at + 5]]);
        let minor = u16::from_le_bytes([self.blob[at + 6], self.blob[at + 7]]);
        let build = u16::from_le_bytes([self.blob[at + 8], self.blob[at + 9]]);
        let revision = u16::from_le_bytes([self.blob[at + 10], self.blob[at + 11]]);
        self.assembly_version = format!("{major}.{minor}.{build}.{revision}");
        let _ = name;
        Ok(())
    }

    fn read_type_refs(&mut self) -> Result<(), String> {
        let Some(layout) = self.table(TABLE_TYPE_REF).copied() else {
            return Ok(());
        };
        let coded = coded_size(2, &[TABLE_MODULE, 0x1A, 0x23, TABLE_TYPE_REF], &self.rows);
        let mut refs: Vec<(String, String)> = Vec::new();
        for row in 1..=layout.rows {
            let at = layout.offset + (row as usize - 1) * layout.row_size;
            let mut cursor = at + coded;
            let name_index = self.read_index(cursor, self.string_index) as usize;
            cursor += self.string_index;
            let namespace_index = self.read_index(cursor, self.string_index) as usize;
            let name = self.read_string(name_index).unwrap_or_default();
            let namespace = self.read_string(namespace_index).unwrap_or_default();
            refs.push((
                if namespace.is_empty() {
                    name
                } else {
                    format!("{namespace}.{name}")
                },
                namespace,
            ));
        }
        self.type_refs = refs;
        Ok(())
    }

    fn read_types(&mut self) -> Result<(), String> {
        let Some(layout) = self.table(TABLE_TYPE_DEF).copied() else {
            return Ok(());
        };
        let coded = coded_size(2, &[TABLE_TYPE_DEF, TABLE_TYPE_REF, 0x1B], &self.rows);
        let field_index_size = simple_size(TABLE_FIELD, &self.rows);
        let method_index_size = simple_size(0x06, &self.rows);
        let mut types: Vec<TypeDefRow> = Vec::new();
        let mut extends_list: Vec<u64> = Vec::new();
        for row in 1..=layout.rows {
            let at = layout.offset + (row as usize - 1) * layout.row_size;
            let mut cursor = at + 4;
            let name_index = self.read_index(cursor, self.string_index) as usize;
            cursor += self.string_index;
            let namespace_index = self.read_index(cursor, self.string_index) as usize;
            cursor += self.string_index;
            let extends = self.read_index(cursor, coded);
            cursor += coded; // Extends
            let field_start = self.read_index(cursor, field_index_size) as usize;
            cursor += field_index_size;
            let _method_start = self.read_index(cursor, method_index_size) as usize;
            let name = self.read_string(name_index).unwrap_or_default();
            let namespace = self.read_string(namespace_index).unwrap_or_default();
            extends_list.push(extends);
            types.push(TypeDefRow {
                name,
                namespace,
                field_start,
                field_end: 0, // 读完所有类型后再补
            });
        }
        self.type_extends = extends_list;
        self.types = types;
        let total_fields = self.rows[TABLE_FIELD] as usize;
        for index in 0..self.types.len() {
            let end = if index + 1 < self.types.len() {
                self.types[index + 1].field_start
            } else {
                total_fields + 1
            };
            self.types[index].field_end = end;
        }
        Ok(())
    }

    fn read_fields(&mut self) -> Result<(), String> {
        let Some(layout) = self.table(TABLE_FIELD).copied() else {
            return Ok(());
        };
        for row in 1..=layout.rows {
            let at = layout.offset + (row as usize - 1) * layout.row_size;
            let flags = u16::from_le_bytes([self.blob[at], self.blob[at + 1]]);
            let name_index = self.read_index(at + 2, self.string_index) as usize;
            let signature_index = self.read_index(at + 2 + self.string_index, self.blob_index) as usize;
            let name = self.read_string(name_index).unwrap_or_default();
            self.field_rows.push(FieldRow {
                name,
                flags,
                signature_offset: self.blob_offset + signature_index,
            });
        }
        Ok(())
    }

    fn read_nesting(&mut self) -> Result<(), String> {
        let Some(layout) = self.table(TABLE_NESTED_CLASS).copied() else {
            return Ok(());
        };
        let index_size = simple_size(TABLE_TYPE_DEF, &self.rows);
        for row in 1..=layout.rows {
            let at = layout.offset + (row as usize - 1) * layout.row_size;
            let nested = self.read_index(at, index_size) as usize;
            let enclosing = self.read_index(at + index_size, index_size) as usize;
            self.nesting.insert(nested, enclosing);
        }
        Ok(())
    }

    fn read_field_layout(&mut self) -> Result<(), String> {
        let Some(layout) = self.table(TABLE_FIELD_LAYOUT).copied() else {
            return Ok(());
        };
        let index_size = simple_size(TABLE_FIELD, &self.rows);
        for row in 1..=layout.rows {
            let at = layout.offset + (row as usize - 1) * layout.row_size;
            let offset = u32::from_le_bytes([
                self.blob[at],
                self.blob[at + 1],
                self.blob[at + 2],
                self.blob[at + 3],
            ]) as i32;
            let field = self.read_index(at + 4, index_size) as usize;
            self.explicit.insert(field, offset);
        }
        Ok(())
    }

    /// 继承链（派生 → 基，**开放泛型**形态）：SOS 把继承来的字段打印在对象自己的类型名下，
    /// 而元数据把字段挂在声明它的类型上，所以出表校验必须能沿基类链找。
    pub fn extends_map(&self) -> BTreeMap<String, String> {
        let mut map = BTreeMap::new();
        for (index, encoded) in self.type_extends.iter().enumerate() {
            if *encoded == 0 {
                continue;
            }
            let derived = self.full_name(index);
            let Some(base) = self.resolve_coded(*encoded) else {
                continue;
            };
            if derived.is_empty() || base.is_empty() {
                continue;
            }
            map.insert(
                derived,
                crate::names::open_generic(&crate::names::canonical_type(&base)).to_string(),
            );
        }
        map
    }

    /// `TypeDefOrRef` 编码值 → 类型名。
    fn resolve_coded(&self, encoded: u64) -> Option<String> {
        if encoded == 0 {
            return None;
        }
        let tag = encoded & 0x03;
        let index = (encoded >> 2) as usize;
        if index == 0 {
            return None;
        }
        match tag {
            0 => Some(self.full_name(index - 1)),
            1 => self.type_refs.get(index - 1).map(|(name, _)| name.clone()),
            _ => self.type_spec_name(index),
        }
    }

    /// `TypeSpec` 行 → 其签名解码出的类型名（泛型基类走这条路）。
    fn type_spec_name(&self, index: usize) -> Option<String> {
        let at = self.row_start(0x1B, index as u64)?;
        let blob_index = self.read_index(at, self.blob_index) as usize;
        let offset = self.blob_offset.checked_add(blob_index)?;
        let (length, prefix) = self.read_compressed(offset)?;
        let start = offset + prefix;
        let end = start + length as usize;
        if end > self.blob.len() {
            return None;
        }
        let mut cursor = start;
        let (name, _) = self.decode_type(&mut cursor, end).ok()?;
        Some(name)
    }

    /// 类型全名（嵌套类型用 `+`；与 SOS 的打印形态一致）。
    fn full_name(&self, index: usize) -> String {
        let Some(row) = self.types.get(index) else {
            return String::new();
        };
        if let Some(enclosing) = self.nesting.get(&(index + 1)) {
            let parent = self.full_name(*enclosing - 1);
            return format!("{parent}+{}", row.name);
        }
        if row.namespace.is_empty() {
            row.name.clone()
        } else {
            format!("{}.{}", row.namespace, row.name)
        }
    }

    /// 产出字段结构行。
    fn build_fields(&self, file_name: &str) -> Result<Vec<IlField>, String> {
        let mut fields = Vec::new();
        for (index, _) in self.types.iter().enumerate() {
            let type_name = self.full_name(index);
            let start = self.types[index].field_start;
            let end = self.types[index].field_end.max(start);
            let mut order = 0usize;
            for field_index in start..end {
                let Some(row) = self.field_rows.get(field_index - 1) else {
                    continue;
                };
                order += 1;
                let (type_display, type_kind) = self.decode_field_signature(row.signature_offset)?;
                fields.push(IlField {
                    assembly: file_name.to_string(),
                    type_name: type_name.clone(),
                    field: row.name.clone(),
                    kind: if row.flags & 0x0010 != 0 {
                        "static".to_string()
                    } else {
                        "instance".to_string()
                    },
                    type_kind,
                    type_display,
                    token: format!("{:08X}", 0x0400_0000u32 + field_index as u32),
                    explicit_offset: self.explicit.get(&field_index).copied(),
                    order,
                });
            }
        }
        Ok(fields)
    }

    /// 解析字段签名 blob：`FIELD(0x06) <Type>`。
    fn decode_field_signature(&self, offset: usize) -> Result<(String, String), String> {
        let (length, prefix) = self
            .read_compressed(offset)
            .ok_or("field signature blob is malformed")?;
        let start = offset + prefix;
        let end = start + length as usize;
        if end > self.blob.len() {
            return Err("field signature runs past end of metadata".to_string());
        }
        let mut cursor = start;
        if self.blob.get(cursor) != Some(&0x06) {
            return Err(format!(
                "field signature at {start:#x} does not start with FIELD (0x06)"
            ));
        }
        cursor += 1;
        self.decode_type(&mut cursor, end)
    }

    /// 解码一个 `Type`，返回（显示名、种类）。
    fn decode_type(&self, cursor: &mut usize, end: usize) -> Result<(String, String), String> {
        if *cursor >= end {
            return Err("type signature is truncated".to_string());
        }
        let element = self.blob[*cursor];
        *cursor += 1;
        let simple = |name: &str, kind: &str| Ok((name.to_string(), kind.to_string()));
        match element {
            0x01 => simple("System.Void", "void"),
            0x02 => simple("System.Boolean", "prim"),
            0x03 => simple("System.Char", "prim"),
            0x04 => simple("System.SByte", "prim"),
            0x05 => simple("System.Byte", "prim"),
            0x06 => simple("System.Int16", "prim"),
            0x07 => simple("System.UInt16", "prim"),
            0x08 => simple("System.Int32", "prim"),
            0x09 => simple("System.UInt32", "prim"),
            0x0A => simple("System.Int64", "prim"),
            0x0B => simple("System.UInt64", "prim"),
            0x0C => simple("System.Single", "prim"),
            0x0D => simple("System.Double", "prim"),
            0x0E => simple("System.String", "string"),
            0x16 => simple("System.TypedReference", "typedref"),
            0x18 => simple("System.IntPtr", "prim"),
            0x19 => simple("System.UIntPtr", "prim"),
            0x1C => simple("System.Object", "object"),
            0x0F => {
                let (inner, _) = self.decode_type(cursor, end)?;
                Ok((format!("{inner}*"), "ptr".to_string()))
            }
            0x10 => {
                let (inner, _) = self.decode_type(cursor, end)?;
                Ok((format!("{inner}&"), "byref".to_string()))
            }
            0x1D => {
                let (inner, _) = self.decode_type(cursor, end)?;
                Ok((format!("{inner}[]"), "array".to_string()))
            }
            0x11 | 0x12 => {
                let (name, _) = self.decode_type_reference(cursor)?;
                let kind = if element == 0x11 { "valuetype" } else { "class" };
                Ok((name, kind.to_string()))
            }
            0x13 | 0x1E => {
                let (number, size) = self.read_compressed(*cursor).ok_or("var number malformed")?;
                *cursor += size;
                let prefix = if element == 0x13 { "!" } else { "!!" };
                Ok((format!("{prefix}{number}"), "var".to_string()))
            }
            0x14 => {
                let (inner, _) = self.decode_type(cursor, end)?;
                let (rank, size) = self.read_compressed(*cursor).ok_or("array rank malformed")?;
                *cursor += size;
                let (count, size) = self.read_compressed(*cursor).ok_or("array size count malformed")?;
                *cursor += size;
                for _ in 0..count {
                    let (_, size) = self.read_compressed(*cursor).ok_or("array size malformed")?;
                    *cursor += size;
                }
                let (bounded, size) = self
                    .read_compressed(*cursor)
                    .ok_or("array bound count malformed")?;
                *cursor += size;
                for _ in 0..bounded * 2 {
                    let (_, size) = self.read_compressed(*cursor).ok_or("array bound malformed")?;
                    *cursor += size;
                }
                let commas = ",".repeat(rank.saturating_sub(1) as usize);
                Ok((format!("{inner}[{commas}]"), "array".to_string()))
            }
            0x15 => {
                let inner_element = *self.blob.get(*cursor).ok_or("generic inst truncated")?;
                *cursor += 1;
                if inner_element != 0x11 && inner_element != 0x12 {
                    return Err("generic instance type is neither CLASS nor VALUETYPE".to_string());
                }
                let (name, _) = self.decode_type_reference(cursor)?;
                let (count, size) = self
                    .read_compressed(*cursor)
                    .ok_or("generic argument count malformed")?;
                *cursor += size;
                let mut args = Vec::new();
                for _ in 0..count {
                    let (arg, _) = self.decode_type(cursor, end)?;
                    args.push(arg);
                }
                Ok((
                    format!("{name}<{}>", args.join(",")),
                    if inner_element == 0x11 {
                        "valuetype".to_string()
                    } else {
                        "class".to_string()
                    },
                ))
            }
            0x1F | 0x20 => {
                let _ = self.decode_type_reference(cursor)?;
                self.decode_type(cursor, end)
            }
            0x45 => self.decode_type(cursor, end),
            0x1B => {
                // 函数指针：跳过调用约定与签名（字段里极少见）。
                while *cursor < end && self.blob[*cursor] != 0x06 {
                    *cursor += 1;
                }
                Ok(("method*".to_string(), "fnptr".to_string()))
            }
            other => Err(format!("unsupported element type 0x{other:02X}")),
        }
    }

    /// 解码 `TypeDefOrRef`（压缩 uint，低 2 位是 tag）。
    fn decode_type_reference(&self, cursor: &mut usize) -> Result<(String, String), String> {
        let (encoded, size) = self
            .read_compressed(*cursor)
            .ok_or("type reference is malformed")?;
        *cursor += size;
        let tag = encoded & 0x03;
        let index = (encoded >> 2) as usize;
        let name = match tag {
            0 => self.full_name(index - 1),
            1 => self
                .type_refs
                .get(index - 1)
                .map(|(name, _)| name.clone())
                .unwrap_or_else(|| format!("TypeRef#{index}")),
            _ => format!("TypeSpec#{index}"),
        };
        Ok((canonical_type(&name), tag.to_string()))
    }
}

// ------------------------------------------------------- 表行尺寸（ECMA-335）----

fn simple_size(table: usize, rows: &[u64; 64]) -> usize {
    if rows[table] < 0x1_0000 {
        2
    } else {
        4
    }
}

fn coded_size(tag_bits: u32, tables: &[usize], rows: &[u64; 64]) -> usize {
    let max = tables
        .iter()
        .map(|table| rows[*table])
        .max()
        .unwrap_or(0);
    if max < (1u64 << (16 - tag_bits)) {
        2
    } else {
        4
    }
}

/// 一张表一行的字节数。**未知表一律返回 `None`**（fail-closed：宁可报错也不猜）。
fn row_size(
    table: usize,
    rows: &[u64; 64],
    string_index: usize,
    guid_index: usize,
    blob_index: usize,
) -> Option<usize> {
    let s = string_index;
    let g = guid_index;
    let b = blob_index;
    // 常用 coded index 的宽度（按 ECMA-335 II.24.2.6 的表集合）。
    let type_def_or_ref = coded_size(2, &[TABLE_TYPE_DEF, TABLE_TYPE_REF, 0x1B], rows);
    let has_constant = coded_size(2, &[TABLE_FIELD, 0x08, 0x17], rows);
    let has_custom_attribute = coded_size(5, &[
        0x06, TABLE_FIELD, 0x08, 0x17, 0x02, 0x01, 0x1A, 0x00, 0x1B, 0x20, 0x23, 0x26, 0x27,
        0x28, 0x2A, 0x2B, 0x21, 0x22, 0x24, 0x25, 0x2C, 0x2D,
    ], rows);
    let has_field_marshal = coded_size(1, &[TABLE_FIELD, 0x08], rows);
    let has_decl_security = coded_size(2, &[TABLE_TYPE_DEF, 0x06, TABLE_ASSEMBLY], rows);
    let member_ref_parent = coded_size(3, &[TABLE_TYPE_DEF, TABLE_TYPE_REF, 0x1A, 0x06, 0x1B], rows);
    let has_semantics = coded_size(1, &[0x14, 0x17], rows);
    let method_def_or_ref = coded_size(1, &[0x06, 0x0A], rows);
    let member_forwarded = coded_size(1, &[TABLE_FIELD, 0x06], rows);
    let implementation = coded_size(2, &[0x26, 0x23, 0x27], rows);
    let custom_attribute_type = coded_size(3, &[0x00, 0x00, 0x06, 0x0A, 0x00], rows);
    let resolution_scope = coded_size(2, &[TABLE_MODULE, 0x1A, 0x23, TABLE_TYPE_REF], rows);
    let type_or_method_def = coded_size(1, &[TABLE_TYPE_DEF, 0x06], rows);
    let type_def_index = simple_size(TABLE_TYPE_DEF, rows);
    let field_index = simple_size(TABLE_FIELD, rows);
    let method_def_index = simple_size(0x06, rows);
    let param_index = simple_size(0x08, rows);
    let event_index = simple_size(0x14, rows);
    let property_index = simple_size(0x17, rows);
    let module_ref_index = simple_size(0x1A, rows);
    let assembly_ref_index = simple_size(0x23, rows);
    let generic_param_index = simple_size(0x2A, rows);

    Some(match table {
        0x00 => 2 + s + g * 3,                                  // Module
        0x01 => resolution_scope + s + s,                       // TypeRef
        0x02 => 4 + s + s + type_def_or_ref + field_index + method_def_index, // TypeDef
        0x03 => field_index,                                    // FieldPtr
        0x04 => 2 + s + b,                                      // Field
        0x05 => method_def_index,                               // MethodPtr
        0x06 => 4 + 2 + 2 + s + b + param_index,                // MethodDef
        0x07 => param_index,                                    // ParamPtr
        0x08 => 2 + 2 + s,                                      // Param
        0x09 => type_def_index + type_def_or_ref,               // InterfaceImpl
        0x0A => member_ref_parent + s + b,                       // MemberRef
        0x0B => 1 + 1 + has_constant + b,                        // Constant
        0x0C => has_custom_attribute + custom_attribute_type + b, // CustomAttribute
        0x0D => has_field_marshal + b,                           // FieldMarshal
        0x0E => 2 + has_decl_security + b,                       // DeclSecurity
        0x0F => 2 + 4 + type_def_index,                          // ClassLayout
        0x10 => 4 + field_index,                                 // FieldLayout
        0x11 => b,                                               // StandAloneSig
        0x12 => type_def_index + event_index,                    // EventMap
        0x13 => event_index,                                     // EventPtr
        0x14 => 2 + s + type_def_or_ref,                         // Event
        0x15 => type_def_index + property_index,                 // PropertyMap
        0x16 => property_index,                                  // PropertyPtr
        0x17 => 2 + s + b,                                       // Property
        0x18 => 2 + method_def_index + has_semantics,            // MethodSemantics
        0x19 => type_def_index + method_def_or_ref + method_def_or_ref, // MethodImpl
        0x1A => s,                                               // ModuleRef
        0x1B => b,                                               // TypeSpec
        0x1C => 2 + member_forwarded + s + module_ref_index,     // ImplMap
        0x1D => 4 + field_index,                                 // FieldRVA
        0x1E | 0x1F => 0,                                        // EncLog / EncMap（#~ 里不出现）
        0x20 => 4 + 2 + 2 + 2 + 2 + 4 + b + s + s,               // Assembly
        0x21 => 4,                                               // AssemblyProcessor
        0x22 => 4 + 4 + 4,                                       // AssemblyOS
        0x23 => 2 + 2 + 2 + 2 + 4 + b + s + s + b,               // AssemblyRef
        0x24 => 4 + assembly_ref_index,                          // AssemblyRefProcessor
        0x25 => 4 + 4 + 4 + assembly_ref_index,                  // AssemblyRefOS
        0x26 => 4 + s + b,                                       // File
        0x27 => 4 + 4 + s + s + implementation,                  // ExportedType
        0x28 => 4 + 4 + s + implementation,                      // ManifestResource
        0x29 => type_def_index + type_def_index,                 // NestedClass
        0x2A => 2 + 2 + type_or_method_def + s,                  // GenericParam
        0x2B => method_def_or_ref + b,                           // MethodSpec
        0x2C => generic_param_index + type_def_or_ref,           // GenericParamConstraint
        _ => return None,
    })
}
