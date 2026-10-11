use crate::osu::model::OBSERVED_STATE_NAMES;
use crate::osu::offsets::{LookupError, OffsetTable};
use super::fields::{screen_state_for, GAME_TYPE_ANY, SCREEN_STACK_MAX};
use super::resolve::plausible_ptr;
use super::source::{field_addr, read_i32, read_ptr_field, read_u32, read_u64, Source};
use super::types::{ChainAddrs, Resolved};

/// 一次 EEType→类型名 解析的全部中间读数。
#[derive(Clone, Debug, Default)]
pub struct EetypeRead {
    pub object: u64,
    pub method_table: u64,
    pub rid: u32,
    pub module: u64,
    pub image_base: u64,
    pub module_name: Option<String>,
    pub type_name: Option<String>,
}

impl EetypeRead {
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "object": format!("0x{:016X}", self.object),
            "method_table": format!("0x{:016X}", self.method_table),
            "rid": self.rid,
            "module": format!("0x{:016X}", self.module),
            "image_base": format!("0x{:016X}", self.image_base),
            "module_name": self.module_name,
            "type_name": self.type_name,
        })
    }
}

pub fn read_eetype(
    source: &dyn Source,
    table: &OffsetTable,
    object: u64,
    modules: &[(u64, String)],
) -> Result<EetypeRead, String> {
    let token = table
        .runtime_entry("eetype", "token")
        .ok_or_else(|| "offsets-missing-runtime:eetype.token".to_string())?;
    if token.shift > 24 {
        return Err("token-domain".to_string());
    }
    let loader_module = table
        .runtime_entry("eetype", "loader_module")
        .ok_or_else(|| "offsets-missing-runtime:eetype.loader_module".to_string())?;
    let image_base_entry = table
        .runtime_entry("module", "image_base")
        .ok_or_else(|| "offsets-missing-runtime:module.image_base".to_string())?;

    let mut read = EetypeRead {
        object,
        ..EetypeRead::default()
    };
    read.method_table = read_u64(source, object).ok_or_else(|| "read-methodtable".to_string())?;
    if !plausible_ptr(read.method_table) {
        return Err("methodtable-implausible".to_string());
    }
    let token_addr = field_addr(read.method_table, token.offset).ok_or_else(|| "token-domain".to_string())?;
    let packed = read_u32(source, token_addr).ok_or_else(|| "read-token".to_string())?;
    read.rid = packed >> token.shift;
    if read.rid == 0 {
        return Err("token-domain".to_string());
    }
    let module_addr =
        field_addr(read.method_table, loader_module.offset).ok_or_else(|| "module-implausible".to_string())?;
    read.module = read_u64(source, module_addr).ok_or_else(|| "read-loader-module".to_string())?;
    if !plausible_ptr(read.module) {
        return Err("module-implausible".to_string());
    }
    let image_addr =
        field_addr(read.module, image_base_entry.offset).ok_or_else(|| "image-base-implausible".to_string())?;
    read.image_base = read_u64(source, image_addr).ok_or_else(|| "read-image-base".to_string())?;
    if !plausible_ptr(read.image_base) {
        return Err("image-base-implausible".to_string());
    }
    let module_name = modules
        .iter()
        .find(|(base, _)| *base == read.image_base)
        .map(|(_, name)| name.clone())
        .ok_or_else(|| format!("module-unresolved:0x{:016X}", read.image_base))?;
    let type_name = match table.runtime_type_name(&module_name, read.rid) {
        Some(name) => name.to_string(),
        None => read_pe_typedef_name(source, read.image_base, read.rid)
            .ok_or_else(|| format!("type-unresolved:{module_name}#{:X}", read.rid))?,
    };
    read.module_name = Some(module_name);
    read.type_name = Some(type_name);
    Ok(read)
}

/// 直接从已加载 PE 模块的 CLI 元数据表解析 TypeDef 名字。
///
/// 当偏移表未收录特定类型（例如具体规则集的 Mod 类）时，此函数作为自省回退，
/// 允许零表依赖解析任意托管对象类型名。
pub fn read_pe_typedef_name(source: &dyn Source, image_base: u64, rid: u32) -> Option<String> {
    if rid == 0 {
        return None;
    }
    // 1. DOS header -> e_lfanew
    let e_lfanew = super::source::read_u32(source, image_base + 0x3C)? as u64;
    if e_lfanew > 0x1000 {
        return None;
    }
    // 2. PE signature ("PE\0\0" = 0x00004550)
    let pe_sig = super::source::read_u32(source, image_base + e_lfanew)?;
    if pe_sig != 0x00004550 {
        return None;
    }
    // 3. Optional header magic (PE32: 0x10B, PE32+: 0x20B)
    let opt_magic = super::source::read_u16(source, image_base + e_lfanew + 24)?;
    let dir_offset: u64 = match opt_magic {
        0x20B => 112,
        0x10B => 96,
        _ => return None,
    };
    // 4. CLI Data Directory (目录项 14)
    let cli_dir_addr = image_base + e_lfanew + 24 + dir_offset + 14 * 8;
    let cli_rva = super::source::read_u32(source, cli_dir_addr)? as u64;
    if cli_rva == 0 {
        return None;
    }
    // 5. CLI Header: metadata RVA & size
    let cli_header = image_base + cli_rva;
    let metadata_rva = super::source::read_u32(source, cli_header + 8)? as u64;
    let metadata_size = super::source::read_u32(source, cli_header + 12)? as usize;
    if metadata_rva == 0 || metadata_size < 32 {
        return None;
    }
    let metadata_addr = image_base + metadata_rva;
    let bsjb = super::source::read_u32(source, metadata_addr)?;
    if bsjb != 0x424A5342 {
        return None;
    }
    // 6. Streams
    let v_len = super::source::read_u32(source, metadata_addr + 12)? as u64;
    let v_len_aligned = (v_len + 3) & !3;
    let streams_hdr = metadata_addr + 16 + v_len_aligned;
    let streams_count = super::source::read_u16(source, streams_hdr + 2)? as usize;
    if streams_count > 16 {
        return None;
    }
    let mut cursor = streams_hdr + 4;
    let mut tilde_stream: Option<(u64, u64)> = None;
    let mut strings_stream: Option<(u64, u64)> = None;
    for _ in 0..streams_count {
        let stream_offset = super::source::read_u32(source, cursor)? as u64;
        let stream_size = super::source::read_u32(source, cursor + 4)? as u64;
        cursor += 8;
        let mut name_bytes = Vec::new();
        let mut name_len = 0usize;
        for _ in 0..32 {
            let b = super::source::read_u8(source, cursor + name_len as u64)?;
            name_len += 1;
            if b == 0 {
                break;
            }
            name_bytes.push(b);
        }
        let padded_len = (name_len + 3) & !3;
        cursor += padded_len as u64;
        let name = String::from_utf8_lossy(&name_bytes);
        if name == "#~" {
            tilde_stream = Some((stream_offset, stream_size));
        } else if name == "#Strings" {
            strings_stream = Some((stream_offset, stream_size));
        }
    }
    let (tilde_offset, _) = tilde_stream?;
    let (strings_offset, strings_size) = strings_stream?;

    let tilde_addr = metadata_addr + tilde_offset;
    let heap_sizes = super::source::read_u8(source, tilde_addr + 6)?;
    let string_index_size: usize = if (heap_sizes & 1) != 0 { 4 } else { 2 };
    let guid_index_size: usize = if (heap_sizes & 2) != 0 { 4 } else { 2 };

    let valid = super::source::read_u64(source, tilde_addr + 8)?;
    if (valid & (1 << 2)) == 0 {
        return None;
    }

    let mut row_counts = [0u64; 64];
    let mut tables_cursor = tilde_addr + 24;
    for b in 0..64 {
        if (valid & (1 << b)) != 0 {
            row_counts[b] = super::source::read_u32(source, tables_cursor)? as u64;
            tables_cursor += 4;
        }
    }

    let typedef_rows = row_counts[2];
    if (rid as u64) > typedef_rows {
        return None;
    }

    // Table 0 (Module)
    let module_row_size = 2 + string_index_size + 3 * guid_index_size;
    let table_0_offset = if (valid & (1 << 0)) != 0 {
        row_counts[0] as usize * module_row_size
    } else {
        0
    };

    // Table 1 (TypeRef)
    let max_res = row_counts[0].max(row_counts[0x1A]).max(row_counts[0x23]).max(row_counts[1]);
    let resolution_scope_size: usize = if max_res < (1 << 14) { 2 } else { 4 };
    let typeref_row_size = resolution_scope_size + 2 * string_index_size;
    let table_1_offset = if (valid & (1 << 1)) != 0 {
        row_counts[1] as usize * typeref_row_size
    } else {
        0
    };

    let typedef_table_addr = tables_cursor + (table_0_offset + table_1_offset) as u64;

    // Table 2 (TypeDef)
    let max_typedef_or_ref = row_counts[2].max(row_counts[1]).max(row_counts[0x1B]);
    let typedeforref_size: usize = if max_typedef_or_ref < (1 << 14) { 2 } else { 4 };
    let field_index_size: usize = if row_counts[4] < (1 << 16) { 2 } else { 4 };
    let method_index_size: usize = if row_counts[6] < (1 << 16) { 2 } else { 4 };

    let typedef_row_size = 4 + 2 * string_index_size + typedeforref_size + field_index_size + method_index_size;
    let target_row_addr = typedef_table_addr + ((rid - 1) as usize * typedef_row_size) as u64;

    let read_index = |addr: u64, size: usize| -> Option<u32> {
        if size == 4 {
            super::source::read_u32(source, addr)
        } else {
            super::source::read_u16(source, addr).map(|v| v as u32)
        }
    };

    let name_idx = read_index(target_row_addr + 4, string_index_size)? as u64;
    let ns_idx = read_index(target_row_addr + 4 + string_index_size as u64, string_index_size)? as u64;

    let read_string = |idx: u64| -> Option<String> {
        if idx >= strings_size {
            return None;
        }
        let str_addr = metadata_addr + strings_offset + idx;
        let mut s_bytes = Vec::new();
        for i in 0..128 {
            let b = super::source::read_u8(source, str_addr + i)?;
            if b == 0 {
                break;
            }
            s_bytes.push(b);
        }
        Some(String::from_utf8_lossy(&s_bytes).to_string())
    };

    let name = read_string(name_idx)?;
    let namespace = read_string(ns_idx).unwrap_or_default();
    if namespace.is_empty() {
        Some(name)
    } else {
        Some(format!("{namespace}.{name}"))
    }
}

/// state.{name,number} 的读数。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScreenState {
    pub number: i32,
    pub name: String,
    pub type_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScreenOutcome {
    Mapped(ScreenState),
    Unmapped(String),
    Unresolved(String),
}

pub fn read_screen_state(
    source: &dyn Source,
    table: &OffsetTable,
    game_base: u64,
    resolved: &Resolved,
    chain: &mut ChainAddrs,
    modules: &[(u64, String)],
) -> ScreenOutcome {
    let unresolved = |reason: String| ScreenOutcome::Unresolved(reason);
    let lookup = |offset: &Result<i64, LookupError>| -> Result<i64, String> {
        offset.clone().map_err(|error| format!("offsets-{error}"))
    };
    let stack_offset = match lookup(&resolved.screen_stack) {
        Ok(value) => value,
        Err(reason) => return unresolved(reason),
    };
    let list_offset = match lookup(&resolved.screen_stack_list) {
        Ok(value) => value,
        Err(reason) => return unresolved(reason),
    };
    let array_offset = match lookup(&resolved.screen_stack_array) {
        Ok(value) => value,
        Err(reason) => return unresolved(reason),
    };
    let size_offset = match lookup(&resolved.screen_stack_size) {
        Ok(value) => value,
        Err(reason) => return unresolved(reason),
    };

    chain.screen_stack = read_ptr_field(source, game_base, stack_offset);
    let Some(screen_stack) = chain.screen_stack else {
        return unresolved("read-<ScreenStack>".to_string());
    };
    chain.screen_stack_list = read_ptr_field(source, screen_stack, list_offset);
    let Some(stack) = chain.screen_stack_list else {
        return unresolved("read-OsuScreenStack.stack".to_string());
    };
    chain.screen_stack_array = read_ptr_field(source, stack, array_offset);
    let Some(array) = chain.screen_stack_array else {
        return unresolved("read-Stack._array".to_string());
    };
    let array_entry = match table.runtime_entry("screen_array", "elements") {
        Some(entry) => entry,
        None => return unresolved("offsets-missing-runtime:screen_array.elements".to_string()),
    };
    let Some(size_addr) = field_addr(stack, size_offset) else {
        return unresolved("read-Stack._size".to_string());
    };
    let Some(size) = read_i32(source, size_addr) else {
        return unresolved("read-Stack._size".to_string());
    };
    if size <= 0 || size > SCREEN_STACK_MAX {
        return unresolved(format!("screen-stack-empty:{size}"));
    }
    let index = (size - 1) as i64;
    let stride = array_entry.stride.max(8);
    let Some(element_addr) = field_addr(array, array_entry.offset + index.saturating_mul(stride)) else {
        return unresolved("element-address".to_string());
    };
    let Some(element) = read_u64(source, element_addr) else {
        return unresolved("read-screen-element".to_string());
    };
    if !plausible_ptr(element) {
        return unresolved("screen-element-implausible".to_string());
    }
    chain.screen_top = Some(element);
    let read = match read_eetype(source, table, element, modules) {
        Ok(read) => read,
        Err(reason) => {
            chain.screen_top_vtable = read_u64(source, element);
            return unresolved(reason);
        }
    };
    chain.screen_top_vtable = Some(read.method_table);
    chain.screen_top_module = Some(read.module);
    chain.screen_top_image_base = Some(read.image_base);
    let type_name = read.type_name.clone().unwrap_or_default();
    let mapped_state = table
        .screen_state_for(&type_name)
        .or_else(|| screen_state_for(&type_name));
    match mapped_state {
        Some(state) => match OBSERVED_STATE_NAMES
            .iter()
            .find(|(_, name)| *name == state)
            .map(|(number, _)| *number)
        {
            Some(number) => ScreenOutcome::Mapped(ScreenState {
                number,
                name: state.to_string(),
                type_name,
            }),
            None => unresolved(format!("state-not-in-observed-set:{state}")),
        },
        None => ScreenOutcome::Unmapped(type_name),
    }
}

pub fn runtime_probe(
    source: &dyn Source,
    table: &OffsetTable,
    game_base: u64,
    modules: &[(u64, String)],
) -> Result<String, String> {
    let read = read_eetype(source, table, game_base, modules)?;
    let name = read.type_name.clone().unwrap_or_default();
    if !GAME_TYPE_ANY.iter().any(|needle| name.contains(needle)) {
        return Err(format!(
            "`[gameBase]` (EEType 0x{:016X}) resolves to `{name}` in {}# {:X} — expected one of {:?}",
            read.method_table, read.module_name.as_deref().unwrap_or("<unknown>"), read.rid, GAME_TYPE_ANY
        ));
    }
    Ok(format!(
        "[gameBase] EEType 0x{:016X} -> {}# {:X} -> `{name}` (image base 0x{:016X})",
        read.method_table,
        read.module_name.as_deref().unwrap_or("<unknown>"),
        read.rid,
        read.image_base
    ))
}
