// sos.rs —— `dotnet-dump analyze` 驱动、SOS 文本解析、解引用自证、SOS 中间件读写
//
// 为什么走 SOS 而不是 ClrMD（`DECISIONS.md` OPEN-03 的结案，P4b 实测）：
// 本机 lazer 是**自包含**发布，`coreclr.dll` 的版本资源是占位值 `42.42.42.42424`，
// ClrMD 因此拒绝 DAC（`Mismatched dac`），堆/类型查询全灭（0/21）；同一份 dump 上
// `dotnet-dump analyze` 的 `dumpobj <addr>` **可用**，8/8 字段与运行期实读一致。
//
// 本模块只做三件事：
// 1. 用 `-c` 逐条命令驱动分析器，把**原始输出**落到输出目录（每个数字可回溯）；
// 2. 解析 `dumpobj` 的字段表（`MT Field Offset Type VT Attr Value Name` 八列）；
// 3. 拿 dump 的**原始字节**核对 SOS 打印的每一个值 —— 这一步同时证明
//    "SOS 的 Offset 基准 = 对象地址"（P4b 只能"未核实"的那一条，这里变成机械证明）。

use crate::minidump::Dump;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

/// `dumpobj` 打印的一个字段行。
#[derive(Clone, Debug)]
pub struct SosField {
    /// 字段所属对象的类型（`Name:` 行，逐字）。
    pub object_type: String,
    /// `MethodTable` 列（16 hex；SOS 对 `System.String` 之类的字段会打 0）。
    pub mt: String,
    /// `Field` 列（元数据 token，8 hex）。
    pub token: String,
    /// `Offset` 列（对象地址基准）。
    pub offset: i64,
    /// `Type` 列（可能被 SOS 列宽截断成 `..., osu.Framework]]`，也可能是空）。
    pub sos_type: String,
    /// `VT` 列（`Yes` = 值类型、`No` = 引用类型、空 = 未打印）。
    pub vt: String,
    /// `Attr` 列（`instance` / `static` / `TLstatic`）。
    pub attr: String,
    /// `Value` 列（引用 = 指针；原始值 = 内容；结构体 = 该字段自身的地址；可能为空）。
    pub value: String,
    /// `Name` 列。
    pub name: String,
    /// 原始行（逐字，留作可回溯证据）。
    pub raw: String,
}

/// `dumpobj` 的一个对象块。
#[derive(Clone, Debug)]
pub struct SosObject {
    pub name: String,
    pub method_table: String,
    pub size_text: String,
    /// `Size: 1840(0x730) bytes` 里的十六进制值（0 = 未解析）。
    pub size: u64,
    pub file: String,
    /// `System.String` 特有：`String: <内容>`。
    pub string_value: Option<String>,
    pub fields: Vec<SosField>,
}

impl SosObject {
    /// 模块文件名（小写，`File:` 行的 basename）。
    pub fn module(&self) -> String {
        Path::new(&self.file)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default()
    }

    /// 按字段名找一行（同名多行 = 返回全部，由调用方判"歧义"）。
    pub fn rows_named(&self, name: &str) -> Vec<&SosField> {
        self.fields.iter().filter(|f| f.name == name).collect()
    }
}

/// 一次解析的结果。
#[derive(Debug, Default)]
pub struct ParsedTranscript {
    pub objects: Vec<SosObject>,
    /// 看起来像字段行但解析不了的原始行（**绝不静默丢弃**：计数进中间件）。
    pub unparsed_rows: Vec<String>,
    /// 与字段无关的输出行数（分析器横幅、`Invalid object`、thread-static 行等）。
    pub noise_lines: usize,
}

const HEX16: usize = 16;
const HEX8: usize = 8;

fn is_hex(text: &str, len: usize) -> bool {
    text.len() == len && text.bytes().all(|b| b.is_ascii_hexdigit())
}

/// 解析一段 `dumpobj` 输出（可含多个对象块，按出现顺序）。
pub fn parse_transcript(text: &str) -> ParsedTranscript {
    let mut parsed = ParsedTranscript::default();
    let mut current: Option<SosObject> = None;
    for raw_line in text.lines() {
        let line = raw_line.trim_end();
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("Name:") {
            if let Some(object) = current.take() {
                parsed.objects.push(object);
            }
            current = Some(SosObject {
                name: rest.trim().to_string(),
                method_table: String::new(),
                size_text: String::new(),
                size: 0,
                file: String::new(),
                string_value: None,
                fields: Vec::new(),
            });
            continue;
        }
        let Some(object) = current.as_mut() else {
            parsed.noise_lines += 1;
            continue;
        };
        if let Some(rest) = trimmed.strip_prefix("MethodTable:") {
            object.method_table = rest.trim().to_string();
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("Size:") {
            object.size_text = rest.trim().to_string();
            object.size = parse_size(&object.size_text);
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("File:") {
            object.file = rest.trim().to_string();
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("String:") {
            object.string_value = Some(rest.trim().to_string());
            continue;
        }
        if trimmed.starts_with("Fields:") {
            continue;
        }
        // 字段行：以 16 hex 的 MT 列 + 8 hex 的 Field 列开头。
        let tokens: Vec<&str> = trimmed.split_whitespace().collect();
        if tokens.len() >= 6 && is_hex(tokens[0], HEX16) && is_hex(tokens[1], HEX8) {
            match parse_field_row(object.name.clone(), &tokens, line) {
                Some(field) => object.fields.push(field),
                None => parsed.unparsed_rows.push(line.to_string()),
            }
        } else {
            parsed.noise_lines += 1;
        }
    }
    if let Some(object) = current.take() {
        parsed.objects.push(object);
    }
    parsed
}

fn parse_size(text: &str) -> u64 {
    // 形态：`1840(0x730) bytes`
    if let Some(start) = text.find("0x") {
        let digits: String = text[start + 2..]
            .chars()
            .take_while(|c| c.is_ascii_hexdigit())
            .collect();
        if let Ok(value) = u64::from_str_radix(&digits, 16) {
            return value;
        }
    }
    if let Some(start) = text.find('(') {
        if let Some(end) = text[start + 1..].find(')') {
            if let Ok(value) = text[start + 1..start + 1 + end].trim().parse::<u64>() {
                return value;
            }
        }
    }
    0
}

/// 八列字段行 → [`SosField`]。
///
/// 列序（SOS 固定）：`MT Field Offset Type VT Attr Value Name`；`Type` 里可能含空格
/// （泛型显示名），`Value` 可能为空（thread-static 行）。因此从右往左定位 `Attr`，
/// 再从它左边一格拿 `VT`，左边剩下的就是 `Type`。
fn parse_field_row(object_type: String, tokens: &[&str], raw: &str) -> Option<SosField> {
    let offset = i64::from_str_radix(tokens[2], 16).ok()?;
    let attr_index = tokens
        .iter()
        .rposition(|t| *t == "instance" || *t == "static" || *t == "TLstatic")?;
    if attr_index < 4 {
        return None;
    }
    let name = tokens.last()?.to_string();
    let vt = tokens[attr_index - 1].to_string();
    let sos_type = tokens[3..attr_index - 1].join(" ");
    let value = if attr_index + 1 < tokens.len() - 1 {
        tokens[attr_index + 1..tokens.len() - 1].join(" ")
    } else {
        String::new()
    };
    Some(SosField {
        object_type,
        mt: tokens[0].to_string(),
        token: tokens[1].to_string(),
        offset,
        sos_type,
        vt,
        attr: tokens[attr_index].to_string(),
        value,
        name,
        raw: raw.to_string(),
    })
}

// ------------------------------------------------------------ 分析器驱动 ----

/// 一次 `dotnet-dump analyze` 运行留下的痕迹。
#[derive(Debug)]
pub struct AnalyzerRun {
    pub out_path: PathBuf,
    pub err_path: PathBuf,
    pub transcript_path: PathBuf,
    pub commands_path: PathBuf,
    pub text: String,
    pub exit_code: i32,
    pub elapsed_ms: u128,
}

/// 跑一次分析器：把 `commands` 逐条用 `-c` 传进去，输出**全部落盘**。
///
/// 刻意不用管道：stdout/stderr 各自重定向到文件（避免管道缓冲/权限这类与工具无关的干扰）。
/// 最后一条命令固定 `exit`（dotnet-dump 不 `exit` 会停在交互态）。
pub fn run_analyzer(
    analyzer: &Path,
    dump: &Path,
    commands: &[String],
    out_dir: &Path,
    tag: &str,
) -> Result<AnalyzerRun, String> {
    fs::create_dir_all(out_dir).map_err(|e| format!("create {}: {e}", out_dir.display()))?;
    let out_path = out_dir.join(format!("{tag}.out.txt"));
    let err_path = out_dir.join(format!("{tag}.err.txt"));
    let transcript_path = out_dir.join(format!("{tag}.txt"));
    let commands_path = out_dir.join(format!("{tag}.cmd.txt"));

    let mut command_file = String::new();
    let mut args: Vec<String> = vec![
        "analyze".to_string(),
        dump.to_string_lossy().to_string(),
    ];
    for item in commands {
        args.push("-c".to_string());
        args.push(item.clone());
        command_file.push_str(&format!("-c {:?}\n", item));
    }
    args.push("-c".to_string());
    args.push("exit".to_string());
    command_file.push_str("-c \"exit\"\n");
    fs::write(&commands_path, command_file)
        .map_err(|e| format!("write {}: {e}", commands_path.display()))?;

    let stdout = fs::File::create(&out_path).map_err(|e| format!("create {}: {e}", out_path.display()))?;
    let stderr = fs::File::create(&err_path).map_err(|e| format!("create {}: {e}", err_path.display()))?;
    let started = Instant::now();
    let status = Command::new(analyzer)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .status()
        .map_err(|e| format!("spawn {}: {e}", analyzer.display()))?;
    let elapsed_ms = started.elapsed().as_millis();

    let out_text = fs::read_to_string(&out_path).unwrap_or_default();
    let err_text = fs::read_to_string(&err_path).unwrap_or_default();
    let mut combined = String::new();
    combined.push_str(&out_text);
    if !err_text.trim().is_empty() {
        combined.push_str("\n----- stderr -----\n");
        combined.push_str(&err_text);
    }
    fs::write(&transcript_path, &combined)
        .map_err(|e| format!("write {}: {e}", transcript_path.display()))?;
    Ok(AnalyzerRun {
        out_path,
        err_path,
        transcript_path,
        commands_path,
        text: combined,
        exit_code: status.code().unwrap_or(-1),
        elapsed_ms,
    })
}

// -------------------------------------------------------- 解引用（结构自证）----

/// 用 dump 的原始字节核对一个字段行，返回第三见证状态。
///
/// 状态三族（`emit` 只看前缀，规则写在 README 的判据表里）：
/// - `ok-*`：dump 字节与 SOS 打印一致 ⇒ 这条偏移在**同一份 dump** 上解得通；
/// - `mismatch-*`：不一致 ⇒ **丢弃该字段**（偏移基准或字段表本身有问题）；
/// - `skip-*`：这一行没有可比对的内容（静态字段、值列空、越界……）⇒ 如实记录，不假装验证过。
pub fn deref_status(dump: &mut Dump, object_address: u64, object_size: u64, field: &SosField) -> String {
    if field.attr != "instance" {
        return format!("skip-{}", field.attr);
    }
    if field.offset < 0 {
        return "skip-negative-offset".to_string();
    }
    if object_size > 0 && field.offset as u64 >= object_size {
        return "skip-out-of-bounds".to_string();
    }
    let target = object_address.wrapping_add(field.offset as u64);
    match field.vt.as_str() {
        "No" => {
            let Some(expected) = parse_hex(&field.value) else {
                return "skip-value-unparsable".to_string();
            };
            match dump.read_u64(target) {
                Some(actual) if actual == expected => "ok-ref".to_string(),
                Some(actual) => format!("mismatch-ref:0x{expected:016X}!=0x{actual:016X}"),
                None => "skip-unreadable".to_string(),
            }
        }
        "Yes" => {
            if let Some(kind) = primitive_kind(&field.sos_type) {
                return compare_primitive(dump, target, kind, &field.value);
            }
            // 结构体：SOS 打印的是**字段自身的地址**（= 对象地址 + Offset），这恰好
            // 直接验证了"Offset 基准 = 对象地址"；非结构体且非原始类型时按跳过处理。
            let Some(value) = parse_hex(&field.value) else {
                return "skip-value-unparsable".to_string();
            };
            if value == 0 {
                return "skip-zero-struct".to_string();
            }
            if value == target {
                "ok-struct".to_string()
            } else {
                format!("mismatch-struct:0x{value:016X}!=0x{target:016X}")
            }
        }
        _ => "skip-no-vt".to_string(),
    }
}

/// SOS 会按"原始类型"打印值的那些类型名。
fn primitive_kind(sos_type: &str) -> Option<&'static str> {
    Some(match sos_type {
        "System.Boolean" => "bool",
        "System.Byte" => "u8",
        "System.SByte" => "i8",
        "System.Int16" => "i16",
        "System.UInt16" => "u16",
        "System.Int32" => "i32",
        "System.UInt32" => "u32",
        "System.Int64" => "i64",
        "System.UInt64" => "u64",
        "System.Char" => "char",
        "System.Single" => "f32",
        "System.Double" => "f64",
        _ => return None,
    })
}

fn compare_primitive(dump: &mut Dump, target: u64, kind: &str, printed: &str) -> String {
    let printed = printed.trim();
    match kind {
        "bool" => {
            let expected = match printed {
                "1" | "True" | "true" => true,
                "0" | "False" | "false" => false,
                _ => return "skip-value-unparsable".to_string(),
            };
            match dump.read_at(target, 1) {
                Some(bytes) => {
                    let actual = bytes[0] != 0;
                    if actual == expected {
                        "ok-prim".to_string()
                    } else {
                        format!("mismatch-prim:{expected}!={actual}")
                    }
                }
                None => "skip-unreadable".to_string(),
            }
        }
        "char" => {
            // **注意**：SOS 的 Value 列对 `System.Char` 打印的是**十六进制**
            // （P4b 的 `_firstChar ... 44` 就是 `0x44 = 'D'`），不是十进制。
            let Ok(expected) = u32::from_str_radix(printed.trim_start_matches("0x"), 16) else {
                return "skip-value-unparsable".to_string();
            };
            match dump.read_at(target, 2) {
                Some(bytes) => {
                    let actual = u16::from_le_bytes([bytes[0], bytes[1]]) as u32;
                    if actual == expected {
                        "ok-prim".to_string()
                    } else {
                        format!("mismatch-prim:{expected}!={actual}")
                    }
                }
                None => "skip-unreadable".to_string(),
            }
        }
        "i8" | "u8" | "i16" | "u16" | "i32" | "u32" | "i64" | "u64" => {
            let width = match kind {
                "i8" | "u8" => 1,
                "i16" | "u16" => 2,
                "i32" | "u32" => 4,
                _ => 8,
            };
            let normalized = printed.replace('_', "");
            let expected: u64 = match kind {
                "i8" | "i16" | "i32" | "i64" => match normalized.parse::<i64>() {
                    Ok(value) => value as u64,
                    Err(_) => return "skip-value-unparsable".to_string(),
                },
                _ => match normalized.parse::<u64>() {
                    Ok(value) => value,
                    Err(_) => return "skip-value-unparsable".to_string(),
                },
            };
            match dump.read_at(target, width) {
                Some(bytes) => {
                    let mut buffer = [0u8; 8];
                    buffer[..width].copy_from_slice(&bytes);
                    let actual = u64::from_le_bytes(buffer);
                    if actual == expected {
                        "ok-prim".to_string()
                    } else {
                        format!("mismatch-prim:{expected}!={actual}")
                    }
                }
                None => "skip-unreadable".to_string(),
            }
        }
        // 浮点：SOS 用固定 6 位小数打印，所以**渲染成同样的字符串**再比（不比较位模式，
        // 否则打印精度损失会被误判成不一致）。
        "f32" | "f64" => {
            let expected = match kind {
                "f32" => printed.parse::<f32>().map(|v| format!("{v:.6}")),
                _ => printed.parse::<f64>().map(|v| format!("{v:.6}")),
            };
            let Ok(expected) = expected else {
                return "skip-value-unparsable".to_string();
            };
            let width = if kind == "f32" { 4 } else { 8 };
            match dump.read_at(target, width) {
                Some(bytes) => {
                    let actual = if kind == "f32" {
                        let mut buffer = [0u8; 4];
                        buffer.copy_from_slice(&bytes);
                        format!("{:.6}", f32::from_le_bytes(buffer))
                    } else {
                        let mut buffer = [0u8; 8];
                        buffer.copy_from_slice(&bytes);
                        format!("{:.6}", f64::from_le_bytes(buffer))
                    };
                    if actual == expected {
                        "ok-prim".to_string()
                    } else {
                        format!("mismatch-prim:{expected}!={actual}")
                    }
                }
                None => "skip-unreadable".to_string(),
            }
        }
        _ => "skip-value-unparsable".to_string(),
    }
}

fn parse_hex(text: &str) -> Option<u64> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    u64::from_str_radix(text, 16).ok()
}

// --------------------------------------------------- 运行期结构打印（Step 10f）----
//
// `dumpmt` / `dumpmodule` / `dumparray` 三个打印是"运行期结构位移"的 SOS 侧见证：位移由
// **搜出来**（见 `runtime.rs`），唯一性靠"打印值 vs dump 字节"的逐字比对 + 多探针一致。
// 三个解析器都**只读文本**：地址来自调用方的命令表（`dumpmt` 不回显地址 ⇒ 按顺序配对）。

/// `dumpmt <MT>` 的一行块。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DumpMt {
    pub parent: String,
    pub canonical: String,
    pub module: String,
    /// `Name:` 行的类型名（逐字；`dumpmt` 与 `dumpobj` 打印的是同一个名字）。
    pub name: String,
    /// `mdToken:`（8 hex，含表位 `0x02`）。
    pub token: String,
    /// `File:` 行的程序集路径。
    pub file: String,
    pub base_size: String,
    pub methods: String,
    pub ifaces: String,
}

impl DumpMt {
    /// `mdToken` 的 **TypeDef RID**（`0x02xxxxxx & 0x00FFFFFF`）。
    pub fn rid(&self) -> Option<u32> {
        let token = u64::from_str_radix(self.token.trim(), 16).ok()?;
        let rid = (token & 0x00FF_FFFF) as u32;
        (rid != 0).then_some(rid)
    }

    pub fn module_address(&self) -> Option<u64> {
        parse_address(&self.module)
    }

    /// `File:` 的 basename（小写保留原形；表里的键用它）。
    pub fn module_file(&self) -> String {
        Path::new(&self.file)
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| self.file.clone())
    }

    /// `BaseSize: 0x730` 的十六进制值（交叉核对用；不发布）。
    pub fn base_size_value(&self) -> Option<u64> {
        let text = self.base_size.trim();
        let text = text.strip_prefix("0x").unwrap_or(text);
        u64::from_str_radix(text, 16).ok()
    }
}

/// `dumpmodule <Module>` 的打印块（只取我们见证要用的几行）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DumpModule {
    /// `Name:` 行的模块路径。
    pub name: String,
    pub assembly: String,
    pub base_address: String,
    pub loader_heap: String,
    pub type_def_map: String,
    pub metadata_start: String,
}

impl DumpModule {
    pub fn base_address_value(&self) -> Option<u64> {
        parse_address(&self.base_address)
    }

    pub fn module_file(&self) -> String {
        Path::new(&self.name)
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| self.name.clone())
    }
}

/// `dumparray <array>` 的打印块。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DumpArray {
    pub name: String,
    pub method_table: String,
    /// `Array: Rank 1, Number of elements 8, Type CLASS` 里的元素个数。
    pub elements: usize,
    /// `[i] <addr>` 行（`null` ⇒ `None`）。
    pub items: Vec<Option<u64>>,
    /// `Element Methodtable:`（交叉核对用；不发布）。
    pub element_method_table: String,
}

/// 解析 `dumpmt` 输出（可含多个块，按出现顺序——`dumpmt` 不回显地址，调用方按命令顺序配对）。
pub fn parse_dumpmt(text: &str) -> Vec<DumpMt> {
    let mut out: Vec<DumpMt> = Vec::new();
    let mut current: Option<DumpMt> = None;
    for raw in text.lines() {
        let line = raw.trim();
        if line.starts_with("Loading core dump") || line.starts_with("----- stderr -----") {
            continue;
        }
        if let Some(rest) = line.strip_prefix("Parent:") {
            if let Some(done) = current.take() {
                out.push(done);
            }
            current = Some(DumpMt {
                parent: rest.trim().to_string(),
                ..DumpMt::default()
            });
            continue;
        }
        let Some(block) = current.as_mut() else {
            continue;
        };
        for (key, field) in [
            ("Name:", &mut block.name),
            ("mdToken:", &mut block.token),
            ("File:", &mut block.file),
            ("BaseSize:", &mut block.base_size),
            ("Number of Methods:", &mut block.methods),
            ("Number of IFaces in IFaceMap:", &mut block.ifaces),
            ("Module:", &mut block.module),
            ("Canonical:", &mut block.canonical),
        ] {
            if let Some(rest) = line.strip_prefix(key) {
                *field = rest.trim().to_string();
                break;
            }
        }
    }
    if let Some(done) = current.take() {
        out.push(done);
    }
    out
}

/// 解析 `dumpmodule` 输出（单个块）。
pub fn parse_dumpmodule(text: &str) -> Option<DumpModule> {
    let mut out = DumpModule::default();
    let mut seen = false;
    for raw in text.lines() {
        let line = raw.trim();
        for (key, field) in [
            ("Name:", &mut out.name),
            ("Assembly:", &mut out.assembly),
            ("BaseAddress:", &mut out.base_address),
            ("LoaderHeap:", &mut out.loader_heap),
            ("TypeDefToMethodTableMap:", &mut out.type_def_map),
            ("MetaData start address:", &mut out.metadata_start),
        ] {
            if let Some(rest) = line.strip_prefix(key) {
                *field = rest.trim().to_string();
                seen = true;
                break;
            }
        }
    }
    seen.then_some(out)
}

/// 解析 `dumparray` 输出（单个数组块）。
pub fn parse_dumparray(text: &str) -> Option<DumpArray> {
    let mut out = DumpArray::default();
    let mut seen = false;
    for raw in text.lines() {
        let line = raw.trim();
        if let Some(rest) = line.strip_prefix("Name:") {
            out.name = rest.trim().to_string();
            seen = true;
            continue;
        }
        if let Some(rest) = line.strip_prefix("MethodTable:") {
            out.method_table = rest.trim().to_string();
            continue;
        }
        if let Some(rest) = line.strip_prefix("Element Methodtable:") {
            out.element_method_table = rest.trim().to_string();
            continue;
        }
        if let Some(rest) = line.strip_prefix("Array:") {
            if let Some(at) = rest.find("Number of elements") {
                let tail = &rest[at + "Number of elements".len()..];
                let digits: String = tail
                    .trim_start()
                    .chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect();
                if let Ok(value) = digits.parse::<usize>() {
                    out.elements = value;
                }
            }
            continue;
        }
        if line.starts_with('[') {
            let Some(close) = line.find(']') else { continue };
            let index: usize = match line[1..close].trim().parse() {
                Ok(value) => value,
                Err(_) => continue,
            };
            let value = line[close + 1..].trim();
            let parsed = if value.eq_ignore_ascii_case("null") || value.is_empty() {
                None
            } else {
                parse_address(value)
            };
            if out.items.len() == index {
                out.items.push(parsed);
            } else if index < out.items.len() {
                out.items[index] = parsed;
            }
            seen = true;
        }
    }
    seen.then_some(out)
}

// ------------------------------------------------------------ SOS 中间件 ----

/// 中间件里的一个对象记录（链上的一步）。
#[derive(Clone, Debug)]
pub struct SosObjectRecord {
    pub label: String,
    pub address: u64,
    pub object_type: String,
    pub status: String,
    pub why: String,
}

/// 中间件里的一行字段。
#[derive(Clone, Debug)]
pub struct SosRow {
    pub address: u64,
    pub object_type: String,
    pub field: String,
    pub offset: i64,
    pub sos_type: String,
    pub vt: String,
    pub attr: String,
    pub value: String,
    pub module: String,
    pub token: String,
    pub deref: String,
}

/// 机器可读的 SOS 中间件（TSV；行首标签 + 固定列，无 JSON 依赖）。
#[derive(Clone, Debug, Default)]
pub struct SosIntermediate {
    pub provenance: BTreeMap<String, String>,
    pub objects: Vec<SosObjectRecord>,
    pub rows: Vec<SosRow>,
    /// **运行期结构行**（Step 10f）：`runtime.<组>.<名字>` 的位移 + 见证（见 `runtime.rs`）。
    pub runtime: Vec<SosRuntimeRow>,
    /// **RID→类型名**行（Step 10f）：IL 元数据的 TypeDef 行（`source=il`），
    /// 外加 `dumpmt` 直接印出来的那一部分（`source=il+observed`）。
    pub typedefs: Vec<SosTypedefRow>,
    pub notes: Vec<String>,
}

/// 中间件里的一行**运行期结构**（位移由 dump 搜出来；见 `spec.rs::RUNTIME_PROBES`）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SosRuntimeRow {
    /// `eetype` / `module` / `screen_array`。
    pub group: String,
    /// `token` / `loader_module` / `image_base` / `length` / `elements`。
    pub name: String,
    pub offset: i64,
    pub shift: u32,
    pub stride: i64,
    /// 这个位移是怎么来的（`dumpmt+bytes` / `dumpmodule+bytes` / `dumparray+bytes`）。
    pub provenance: String,
    /// 见证行（逐字；SOS 打印值 + dump 字节值）。
    pub witness: String,
    /// 互相印证的探针个数（≥ [`crate::spec::RUNTIME_MIN_PROBES`] 才算数）。
    pub probes: usize,
}

/// 中间件里的一行 `RID → 类型名`（`source=il` = 元数据的 TypeDef 行；`il+observed` = 另有 dumpmt 见证）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SosTypedefRow {
    /// 模块文件名（`osu.Game.dll`）。
    pub module: String,
    pub rid: u32,
    pub name: String,
    /// `il` / `il+observed`。
    pub source: String,
    /// 见证（元数据那一侧的行 + 观察到的 SOS 行）。
    pub witness: String,
}

impl SosIntermediate {
    /// 运行期结构行（按 `(组, 名字)` 取；调用方自己判缺）。
    pub fn runtime_row(&self, group: &str, name: &str) -> Option<&SosRuntimeRow> {
        self.runtime
            .iter()
            .find(|row| row.group == group && row.name == name)
    }
}

pub const SOS_FORMAT: &str = "lazer-offsets-gen sos-intermediate v1";

impl SosIntermediate {
    /// 是否来自**真实 dump**（`provenance: dump`）。`emit` 的放行判据之一。
    pub fn is_from_dump(&self) -> bool {
        self.provenance
            .get("provenance")
            .map(|v| v == "dump")
            .unwrap_or(false)
    }

    /// 是否 fixture（示例数据）。**fixture 必须显式标记**，且只能配 `--allow-fixtures` 出表。
    pub fn is_fixture(&self) -> bool {
        self.provenance
            .get("fixture")
            .map(|v| v == "true")
            .unwrap_or(false)
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.provenance.get(key).map(|s| s.as_str())
    }

    /// 某标签指向的对象（`None` = 该步没跑成/不在中间件里）。
    pub fn object(&self, label: &str) -> Option<&SosObjectRecord> {
        self.objects.iter().find(|o| o.label == label)
    }

    /// 某地址上某字段的全部行（同名多行 = 由调用方判歧义）。
    pub fn rows_for(&self, address: u64, field: &str) -> Vec<&SosRow> {
        self.rows
            .iter()
            .filter(|r| r.address == address && r.field == field)
            .collect()
    }

    /// 表里出现的去重对象类型（诊断用）。
    pub fn object_types(&self) -> Vec<String> {
        let mut types: Vec<String> = self.rows.iter().map(|r| r.object_type.clone()).collect();
        types.sort();
        types.dedup();
        types
    }

    pub fn write_tsv(&self, path: &Path) -> Result<(), String> {
        let mut out = String::new();
        out.push_str(&format!("# {SOS_FORMAT}\n"));
        for (key, value) in &self.provenance {
            out.push_str(&format!("#\t{key}\t{value}\n"));
        }
        out.push_str("#format\tobject\tlabel\taddress\ttype\tstatus\twhy\n");
        for object in &self.objects {
            out.push_str(&format!(
                "object\t{}\t0x{:X}\t{}\t{}\t{}\n",
                object.label, object.address, object.object_type, object.status, object.why
            ));
        }
        out.push_str("#format\tfield\taddress\tobject_type\tfield\toffset\tsos_type\tvt\tattr\tvalue\tmodule\ttoken\tderef\n");
        for row in &self.rows {
            out.push_str(&format!(
                "field\t0x{:X}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                row.address,
                row.object_type,
                row.field,
                row.offset,
                row.sos_type,
                row.vt,
                row.attr,
                row.value,
                row.module,
                row.token,
                row.deref
            ));
        }
        out.push_str("#format\truntime\tgroup\tname\toffset\tshift\tstride\tprovenance\twitness\tprobes\n");
        for row in &self.runtime {
            out.push_str(&format!(
                "runtime\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                row.group,
                row.name,
                row.offset,
                row.shift,
                row.stride,
                one_line(&row.provenance),
                one_line(&row.witness),
                row.probes
            ));
        }
        out.push_str("#format\ttypedef\tmodule\trid\tname\tsource\twitness\n");
        for row in &self.typedefs {
            out.push_str(&format!(
                "typedef\t{}\t{:X}\t{}\t{}\t{}\n",
                row.module,
                row.rid,
                one_line(&row.name),
                one_line(&row.source),
                one_line(&row.witness)
            ));
        }
        for note in &self.notes {
            out.push_str(&format!("#note\t{note}\n"));
        }
        fs::write(path, out).map_err(|e| format!("write {}: {e}", path.display()))
    }

    /// 读中间件。**严格**：列数不对/标签未知/地址不是十六进制 → 报错并给出行号
    /// （"malformed input 必须显式失败"，不猜也不跳过）。
    pub fn read_tsv(path: &Path) -> Result<SosIntermediate, String> {
        let text = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
        let mut parsed = SosIntermediate::default();
        let mut header_seen = false;
        for (index, line) in text.lines().enumerate() {
            let number = index + 1;
            if line.trim().is_empty() {
                continue;
            }
            if let Some(rest) = line.strip_prefix("# ") {
                if rest.contains(SOS_FORMAT) {
                    header_seen = true;
                }
                // 其他 `# ` 行是注释（fixture 文件的说明块、`#format` 说明……）；
                // 真正决定"这是不是中间件"的是上面那行格式头，缺它一律报错。
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
            // 其它 `#` 开头的行一律是注释（说明块、`#format` 列说明……）；决定"这是不是
            // 中间件"的是下面那个格式头，缺它一律报错。
            if let Some(rest) = line.strip_prefix("#note\t") {
                parsed.notes.push(rest.to_string());
                continue;
            }
            if let Some(rest) = line.strip_prefix('#') {
                if rest.contains(SOS_FORMAT) {
                    header_seen = true;
                }
                continue;
            }
            let columns: Vec<&str> = line.split('\t').collect();
            match columns.first().copied() {
                Some("object") => {
                    if columns.len() != 6 {
                        return Err(format!(
                            "{path:?}:{number}: object row needs 6 columns, got {}",
                            columns.len()
                        ));
                    }
                    parsed.objects.push(SosObjectRecord {
                        label: columns[1].to_string(),
                        address: parse_address(columns[2])
                            .ok_or_else(|| format!("{path:?}:{number}: bad address {:?}", columns[2]))?,
                        object_type: columns[3].to_string(),
                        status: columns[4].to_string(),
                        why: columns[5].to_string(),
                    });
                }
                Some("field") => {
                    if columns.len() != 12 {
                        return Err(format!(
                            "{path:?}:{number}: field row needs 12 columns, got {}",
                            columns.len()
                        ));
                    }
                    let offset = columns[4]
                        .parse::<i64>()
                        .map_err(|_| format!("{path:?}:{number}: bad offset {:?}", columns[4]))?;
                    parsed.rows.push(SosRow {
                        address: parse_address(columns[1]).ok_or_else(|| {
                            format!("{path:?}:{number}: bad address {:?}", columns[1])
                        })?,
                        object_type: columns[2].to_string(),
                        field: columns[3].to_string(),
                        offset,
                        sos_type: columns[5].to_string(),
                        vt: columns[6].to_string(),
                        attr: columns[7].to_string(),
                        value: columns[8].to_string(),
                        module: columns[9].to_string(),
                        token: columns[10].to_string(),
                        deref: columns[11].to_string(),
                    });
                }
                Some("runtime") => {
                    if columns.len() != 9 {
                        return Err(format!(
                            "{path:?}:{number}: runtime row needs 9 columns, got {}",
                            columns.len()
                        ));
                    }
                    let parse_i = |text: &str, what: &str| -> Result<i64, String> {
                        text.parse::<i64>()
                            .map_err(|_| format!("{path:?}:{number}: bad {what} {text:?}"))
                    };
                    parsed.runtime.push(SosRuntimeRow {
                        group: columns[1].to_string(),
                        name: columns[2].to_string(),
                        offset: parse_i(columns[3], "offset")?,
                        shift: columns[4]
                            .parse::<u32>()
                            .map_err(|_| format!("{path:?}:{number}: bad shift {:?}", columns[4]))?,
                        stride: parse_i(columns[5], "stride")?,
                        provenance: columns[6].to_string(),
                        witness: columns[7].to_string(),
                        probes: columns[8].parse::<usize>().map_err(|_| {
                            format!("{path:?}:{number}: bad probe count {:?}", columns[8])
                        })?,
                    });
                }
                Some("typedef") => {
                    if columns.len() != 6 {
                        return Err(format!(
                            "{path:?}:{number}: typedef row needs 6 columns, got {}",
                            columns.len()
                        ));
                    }
                    parsed.typedefs.push(SosTypedefRow {
                        module: columns[1].to_string(),
                        rid: u32::from_str_radix(columns[2].trim_start_matches("0x"), 16)
                            .map_err(|_| format!("{path:?}:{number}: bad rid {:?}", columns[2]))?,
                        name: columns[3].to_string(),
                        source: columns[4].to_string(),
                        witness: columns[5].to_string(),
                    });
                }
                other => {
                    return Err(format!(
                        "{path:?}:{number}: unknown row tag {other:?} (expected `object`, `field`, `runtime` or `typedef`)"
                    ))
                }
            }
        }
        if !header_seen {
            return Err(format!(
                "{path:?}: missing `# {SOS_FORMAT}` header (not a sos-intermediate file)"
            ));
        }
        Ok(parsed)
    }
}

fn parse_address(text: &str) -> Option<u64> {
    let text = text.trim();
    let text = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")).unwrap_or(text);
    u64::from_str_radix(text, 16).ok()
}

/// TSV 行里的自由文本（见证行）不允许带制表符/换行（列数契约是硬的）。
fn one_line(text: &str) -> String {
    text.replace(['\t', '\n', '\r'], " ")
}