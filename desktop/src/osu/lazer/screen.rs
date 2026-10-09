use crate::osu::model::OBSERVED_STATE_NAMES;
use crate::osu::offsets::{LookupError, OffsetTable};
use super::fields::{screen_state_for, GAME_TYPE_ANY, SCREEN_STACK_MAX, UNMAPPED_SCREEN_SUFFIX};
use super::source::{field_addr, plausible_ptr, read_i32, read_ptr_field, read_u32, read_u64, Source};
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
    let type_name = table
        .runtime_type_name(&module_name, read.rid)
        .ok_or_else(|| format!("type-unresolved:{module_name}#{:X}", read.rid))?
        .to_string();
    read.module_name = Some(module_name);
    read.type_name = Some(type_name);
    Ok(read)
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
