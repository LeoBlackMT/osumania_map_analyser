use super::fields::{MD5_HEX_LEN, STORE_KEY_HEX_LEN};

/// 一块只读内存视图的唯一抽象。
pub trait Source {
    fn read(&self, addr: u64, len: usize) -> Option<Vec<u8>>;
}

#[cfg(windows)]
impl Source for crate::osu::win::Target {
    fn read(&self, addr: u64, len: usize) -> Option<Vec<u8>> {
        let mut buf = vec![0u8; len];
        crate::osu::win::read_exact_at64(self.handle(), addr, &mut buf).ok()?;
        Some(buf)
    }
}

#[cfg(not(windows))]
impl Source for crate::osu::win::Target {
    fn read(&self, _addr: u64, _len: usize) -> Option<Vec<u8>> {
        None
    }
}

/// 8 字节小端读（指针/long）。
pub fn read_u64(source: &dyn Source, addr: u64) -> Option<u64> {
    let bytes = source.read(addr, 8)?;
    Some(u64::from_le_bytes(bytes.try_into().ok()?))
}

/// 4 字节小端读（int）。
pub fn read_i32(source: &dyn Source, addr: u64) -> Option<i32> {
    let bytes = source.read(addr, 4)?;
    Some(i32::from_le_bytes(bytes.try_into().ok()?))
}

/// 4 字节小端读（uint；EEType 的 TypeDef RID 打包字段用）。
pub fn read_u32(source: &dyn Source, addr: u64) -> Option<u32> {
    let bytes = source.read(addr, 4)?;
    Some(u32::from_le_bytes(bytes.try_into().ok()?))
}

/// 8 字节小端读（double；beatmap.time.live 用）。
pub fn read_f64(source: &dyn Source, addr: u64) -> Option<f64> {
    let bytes = source.read(addr, 8)?;
    Some(f64::from_le_bytes(bytes.try_into().ok()?))
}

/// 对象字段的指针读：base + offset、非空、8 字节对齐。
pub fn read_ptr_field(source: &dyn Source, base: u64, offset: i64) -> Option<u64> {
    let addr = field_addr(base, offset)?;
    let pointer = read_u64(source, addr)?;
    if pointer == 0 || pointer % 8 != 0 {
        return None;
    }
    Some(pointer)
}

/// base + offset
pub fn field_addr(base: u64, offset: i64) -> Option<u64> {
    if offset < 0 {
        return None;
    }
    base.checked_add(offset as u64)
}

/// System.String 的两个位移。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StringLayout {
    pub length: i64,
    pub chars: i64,
}

/// CoreCLR（x64）字符串读法。
pub fn read_string(source: &dyn Source, addr: u64, layout: StringLayout) -> Option<String> {
    if addr == 0 {
        return None;
    }
    let length_addr = field_addr(addr, layout.length)?;
    let len = read_i32(source, length_addr)?;
    if len <= 0 || len > crate::osu::win::MAX_CSHARP_STRING_UNITS as i32 {
        return None;
    }
    let body_addr = field_addr(addr, layout.chars)?;
    let bytes = source.read(body_addr, len as usize * 2)?;
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    if units.contains(&0) {
        return None;
    }
    String::from_utf16(&units).ok()
}

/// 文件仓键 → 载荷里那句相对路径：h[0]\h[0:2]\<hash>。
pub fn to_lazer_path(hash: &str) -> Option<String> {
    let trimmed = hash.trim();
    if trimmed.len() != STORE_KEY_HEX_LEN
        || !trimmed
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b) || (b'A'..=b'F').contains(&b))
    {
        return None;
    }
    let lower = trimmed.to_lowercase();
    Some(format!(
        "{}\\{}\\{}",
        &lower[0..1],
        &lower[0..2],
        lower
    ))
}

pub fn is_md5_hex(text: &str) -> bool {
    text.len() == MD5_HEX_LEN
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub fn is_store_key(text: &str) -> bool {
    let lower = text.to_lowercase();
    let mut parts = lower.split('\\');
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(head), Some(prefix), Some(leaf), None) => {
            is_hex(head)
                && is_hex(prefix)
                && is_hex(leaf)
                && head.len() == 1
                && prefix.len() == 2
                && leaf.len() == STORE_KEY_HEX_LEN
                && prefix.starts_with(head)
                && leaf.starts_with(prefix)
        }
        _ => false,
    }
}

fn is_hex(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// 从 BeatmapSetInfo.<Files> 读取文件表：`Vec<(filename, lazer_store_path)>`。
pub fn read_beatmap_set_files(
    source: &dyn Source,
    beatmap_set: u64,
    layout: StringLayout,
) -> Vec<(String, String)> {
    let mut files = Vec::new();
    let Some(files_list) = read_ptr_field(source, beatmap_set, 0x20) else {
        return files;
    };
    let Some(size_addr) = field_addr(files_list, 0x10) else {
        return files;
    };
    let Some(size) = read_i32(source, size_addr) else {
        return files;
    };
    if size <= 0 || size > 1000 {
        return files;
    }
    let Some(items_array) = read_ptr_field(source, files_list, 0x08) else {
        return files;
    };
    let Some(len_addr) = field_addr(items_array, 0x08) else {
        return files;
    };
    let Some(arr_len) = read_i32(source, len_addr) else {
        return files;
    };
    if arr_len <= 0 {
        return files;
    }
    let count = (size.min(arr_len) as usize).min(1000);
    for i in 0..count {
        let elem_offset = 0x10 + (i as i64) * 8;
        let Some(usage_ptr) = read_ptr_field(source, items_array, elem_offset) else {
            continue;
        };
        let ptr_a = read_ptr_field(source, usage_ptr, 0x18);
        let ptr_b = read_ptr_field(source, usage_ptr, 0x20);
        let (filename_opt, file_ptr_opt) = match (ptr_a, ptr_b) {
            (Some(a), Some(b)) => {
                if let Some(s) = read_string(source, a, layout) {
                    (Some(s), Some(b))
                } else if let Some(s) = read_string(source, b, layout) {
                    (Some(s), Some(a))
                } else {
                    (None, None)
                }
            }
            _ => (None, None),
        };
        let Some(filename) = filename_opt else {
            continue;
        };
        let Some(file_ptr) = file_ptr_opt else {
            continue;
        };
        let hash_opt = read_ptr_field(source, file_ptr, 0x18)
            .and_then(|h_ptr| read_string(source, h_ptr, layout))
            .or_else(|| {
                read_ptr_field(source, file_ptr, 0x20)
                    .and_then(|h_ptr| read_string(source, h_ptr, layout))
            });
        let Some(hash) = hash_opt else {
            continue;
        };
        if let Some(lazer_path) = to_lazer_path(&hash) {
            files.push((filename, lazer_path));
        }
    }
    files
}

/// 在文件表里按背景图文件名检索仓库路径；未指定或未匹配时按图像后缀回退。
pub fn find_background_file(files: &[(String, String)], target_name: Option<&str>) -> Option<String> {
    if let Some(target) = target_name {
        let clean = target.trim().trim_matches('"');
        if !clean.is_empty() {
            for (filename, path) in files {
                if filename.eq_ignore_ascii_case(clean) {
                    return Some(path.clone());
                }
            }
            let leaf = std::path::Path::new(clean)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(clean);
            for (filename, path) in files {
                if filename.eq_ignore_ascii_case(leaf) {
                    return Some(path.clone());
                }
            }
        }
    }
    for (filename, path) in files {
        let lower = filename.to_lowercase();
        if lower.ends_with(".jpg") || lower.ends_with(".png") || lower.ends_with(".jpeg") || lower.ends_with(".webp") {
            return Some(path.clone());
        }
    }
    None
}
