// runtime.rs —— **运行期结构位移的导出**（Step 10f；`spec.rs::RUNTIME_PROBES` 的执行者）
//
// 这个模块回答的问题是："`state.name` 那条链上的每个位移到底是多少？"答案不是常量，而是
// 在真 dump 上**搜出来**的：SOS 的打印值（`dumpmt` 的 `mdToken`/`Module`、`dumpmodule` 的
// `BaseAddress`、`dumparray` 的 `[i] <addr>`）与 dump 的原始字节逐个候选位移相比，
// 只有"**全部探针都成立、且位移唯一**"的那一个才算（见 `spec.rs::RUNTIME_PROBES`）。
//
// 三条硬规则（与 `chain.rs` 同一批判据风格）：
// 1. **唯一**：候选位移多于一个 ⇒ 拒绝（不发布该位移，报告逐字列出候选）。
// 2. **一致**：每个探针都必须给出同一个位移；跨模块的探针（osu!.dll 与 osu.Game.dll）也必须一致。
// 3. **可比**：dump 字节与 SOS 打印值必须逐字相等（SOS 打印的是它从内存读出来的值；
//    不相等只可能是"位移找错了"或"读到的是另一个对象"）。
//
// 本模块只读 dump 字节 + 解析好的 SOS 文本，**不驱动分析器**（那是 `main.rs` 的活）。

use crate::minidump::Dump;
use crate::spec::RuntimeProbe;

/// 一个探针：`(对象地址, 期望值)`。`expected` 按 `width` 比较。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Probe {
    pub address: u64,
    pub expected: u64,
    /// 这个探针的出处（报告用；例如 `dumpmt 0x7FFE304DCCC0 (osu.Game.Screens.Menu.MainMenu)`）。
    pub source: String,
}

/// 一个位移的导出结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Derived {
    pub offset: i64,
    /// 见证行（SOS 打印值 + dump 字节值，逐字）。
    pub witness: String,
    /// 互相印证的探针个数。
    pub probes: usize,
}

/// 读一个小端 u64（读不到 ⇒ `None`）。
fn read_u64(dump: &mut Dump, address: u64) -> Option<u64> {
    dump.read_u64(address)
}

/// 读一个小端 u32（读不到 ⇒ `None`）。
fn read_u32(dump: &mut Dump, address: u64) -> Option<u32> {
    let bytes = dump.read_at(address, 4)?;
    Some(u32::from_le_bytes(bytes[0..4].try_into().ok()?))
}

/// 读一个小端 i32。
fn read_i32(dump: &mut Dump, address: u64) -> Option<i32> {
    let bytes = dump.read_at(address, 4)?;
    Some(i32::from_le_bytes(bytes[0..4].try_into().ok()?))
}

/// 按 `shift` 比较的 u32 位移搜索（`Probe::expected` 是**已经右移过**的值）。
pub fn find_u32_shifted(
    dump: &mut Dump,
    _base: u64,
    probe: &RuntimeProbe,
    probes: &[Probe],
) -> Result<Derived, String> {
    if probes.len() < crate::spec::RUNTIME_MIN_PROBES {
        return Err(format!(
            "{}:{} needs at least {} probe(s), got {}",
            probe.group,
            probe.name,
            crate::spec::RUNTIME_MIN_PROBES,
            probes.len()
        ));
    }
    let mut candidates: Vec<usize> = Vec::new();
    let mut cursor = probe.scan.0;
    while cursor + 4 <= probe.scan.1 {
        let mut ok = true;
        for item in probes {
            let Some(value) = read_u32(dump, item.address.wrapping_add(cursor as u64)) else {
                ok = false;
                break;
            };
            if (value >> probe.shift) as u64 != item.expected {
                ok = false;
                break;
            }
        }
        if ok {
            candidates.push(cursor);
        }
        cursor += probe.align.max(2);
    }
    let offset = match candidates.len() {
        0 => {
            return Err(format!(
                "{}:{}: no candidate offset in {}..{} satisfies all {} probe(s)",
                probe.group,
                probe.name,
                probe.scan.0,
                probe.scan.1,
                probes.len()
            ))
        }
        1 => candidates[0],
        _ => {
            return Err(format!(
                "{}:{}: {} candidate offsets in {}..{} (ambiguous): {}",
                probe.group,
                probe.name,
                candidates.len(),
                probe.scan.0,
                probe.scan.1,
                candidates
                    .iter()
                    .map(|value| format!("{value:#x}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        }
    };
    let first = &probes[0];
    let value = read_u32(dump, first.address.wrapping_add(offset as u64)).unwrap_or(0);
    let witness = format!(
        "{} + {} probe(s) agree: u32@+{offset:#x} (e.g. 0x{value:08X}@0x{:X}) >> {} == expected; \
         {}",
        first.source,
        probes.len(),
        first.address,
        probe.shift,
        probes
            .iter()
            .take(4)
            .map(|item| format!(
                "{} => 0x{:X}",
                item.source.split(' ').next().unwrap_or("probe"),
                item.expected
            ))
            .collect::<Vec<_>>()
            .join(", ")
    );
    Ok(Derived {
        offset: offset as i64,
        witness,
        probes: probes.len(),
    })
}

/// u64 位移搜索（`dumpmt` 的 `Module` / `dumpmodule` 的 `BaseAddress`）。
pub fn find_u64(
    dump: &mut Dump,
    probe: &RuntimeProbe,
    probes: &[Probe],
) -> Result<Derived, String> {
    if probes.len() < crate::spec::RUNTIME_MIN_PROBES {
        return Err(format!(
            "{}:{} needs at least {} probe(s), got {}",
            probe.group,
            probe.name,
            crate::spec::RUNTIME_MIN_PROBES,
            probes.len()
        ));
    }
    let mut candidates: Vec<usize> = Vec::new();
    let mut cursor = probe.scan.0;
    while cursor + 8 <= probe.scan.1 {
        let mut ok = true;
        for item in probes {
            let Some(value) = read_u64(dump, item.address.wrapping_add(cursor as u64)) else {
                ok = false;
                break;
            };
            if value != item.expected {
                ok = false;
                break;
            }
        }
        if ok {
            candidates.push(cursor);
        }
        cursor += probe.align.max(8);
    }
    let offset = match candidates.len() {
        0 => {
            return Err(format!(
                "{}:{}: no candidate offset in {}..{} satisfies all {} probe(s)",
                probe.group,
                probe.name,
                probe.scan.0,
                probe.scan.1,
                probes.len()
            ))
        }
        1 => candidates[0],
        _ => {
            return Err(format!(
                "{}:{}: {} candidate offsets in {}..{} (ambiguous): {}",
                probe.group,
                probe.name,
                candidates.len(),
                probe.scan.0,
                probe.scan.1,
                candidates
                    .iter()
                    .map(|value| format!("{value:#x}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        }
    };
    let first = &probes[0];
    let value = read_u64(dump, first.address.wrapping_add(offset as u64)).unwrap_or(0);
    let witness = format!(
        "{} + {} probe(s) agree: qword@+{offset:#x} (e.g. 0x{value:016X}@0x{:X} == expected); {}",
        first.source,
        probes.len(),
        first.address,
        probes
            .iter()
            .take(4)
            .map(|item| format!("0x{:X}", item.expected))
            .collect::<Vec<_>>()
            .join(", ")
    );
    Ok(Derived {
        offset: offset as i64,
        witness,
        probes: probes.len(),
    })
}

/// 数组布局（`length` + `elements`）的导出结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArrayLayout {
    pub length_offset: i64,
    pub data_offset: i64,
    pub stride: i64,
    pub length_witness: String,
    pub elements_witness: String,
}

/// 数组对象的元素布局：`(data, stride)` 必须让 `dumparray` 的每一条 `[i] <addr>` 行都成立。
///
/// `length_offset` 由 `Array: … Number of elements N` 与 `u32@array+off` 的相等关系定位
/// （与元素布局相互独立：一个错位不可能同时满足两者）。
pub fn derive_array_layout(
    dump: &mut Dump,
    array: u64,
    elements: usize,
    items: &[Option<u64>],
) -> Result<ArrayLayout, String> {
    let probe = crate::spec::RUNTIME_PROBES
        .iter()
        .find(|probe| probe.group == "screen_array" && probe.name == "length")
        .ok_or_else(|| "spec is missing screen_array.length".to_string())?;
    let mut length_candidates: Vec<usize> = Vec::new();
    let mut cursor = probe.scan.0;
    while cursor + 4 <= probe.scan.1 {
        if let Some(value) = read_i32(dump, array.wrapping_add(cursor as u64)) {
            if value == elements as i32 {
                length_candidates.push(cursor);
            }
        }
        cursor += probe.align.max(4);
    }
    let length_offset = match length_candidates.len() {
        0 => {
            return Err(format!(
                "screen_array.length: no offset in {}..{} holds the dumparray element count {elements}",
                probe.scan.0, probe.scan.1
            ))
        }
        1 => length_candidates[0],
        _ => {
            return Err(format!(
                "screen_array.length: {} candidate offsets for the element count: {}",
                length_candidates.len(),
                length_candidates
                    .iter()
                    .map(|value| format!("{value:#x}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        }
    };

    let probe = crate::spec::RUNTIME_PROBES
        .iter()
        .find(|probe| probe.group == "screen_array" && probe.name == "elements")
        .ok_or_else(|| "spec is missing screen_array.elements".to_string())?;
    let known: Vec<(usize, u64)> = items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| item.map(|value| (index, value)))
        .collect();
    if known.len() < 2 {
        return Err(format!(
            "screen_array.elements: dumparray printed only {} non-null element(s) — at least 2 are \
             needed to pin (data, stride) uniquely",
            known.len()
        ));
    }
    let mut candidates: Vec<(usize, usize)> = Vec::new();
    let mut data = probe.scan.0;
    while data + 8 <= probe.scan.1 {
        for stride in [8usize, 16, 24, 32] {
            let ok = known.iter().all(|(index, expected)| {
                let at = array.wrapping_add((data + index * stride) as u64);
                read_u64(dump, at) == Some(*expected)
            });
            if ok {
                candidates.push((data, stride));
            }
        }
        data += probe.align.max(8);
    }
    let (data_offset, stride) = match candidates.len() {
        0 => {
            return Err(format!(
                "screen_array.elements: no (data, stride) in {}..{} reproduces the dumparray element \
                 addresses",
                probe.scan.0, probe.scan.1
            ))
        }
        1 => candidates[0],
        _ => {
            return Err(format!(
                "screen_array.elements: {} candidate (data, stride) pairs: {}",
                candidates.len(),
                candidates
                    .iter()
                    .map(|(data, stride)| format!("(data={data:#x}, stride={stride})"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        }
    };
    let sample = &known[0];
    let value = read_u64(
        dump,
        array.wrapping_add((data_offset + sample.0 * stride) as u64),
    )
    .unwrap_or(0);
    Ok(ArrayLayout {
        length_offset: length_offset as i64,
        data_offset: data_offset as i64,
        stride: stride as i64,
        length_witness: format!(
            "dumparray `Number of elements {elements}` == i32@array+{length_offset:#x} \
             (array 0x{array:X}, {elements} element(s))"
        ),
        elements_witness: format!(
            "dumparray printed {} non-null element(s); qword@array+{data_offset:#x}+i*{stride} \
             reproduces them (e.g. [{}] = 0x{value:016X} at 0x{:X})",
            known.len(),
            sample.0,
            array.wrapping_add((data_offset + sample.0 * stride) as u64)
        ),
    })
}