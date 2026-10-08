// minidump.rs —— 最小 minidump 读取器（**只读**，零依赖）
//
// 为什么生成器要自己读 dump：`dotnet-dump analyze` 需要**具体对象地址**才肯打印字段表
// （P4b 的结论：本机 lazer 自包含 runtime 的 dump 上 ClrMD/SOS 的堆枚举命令不可用，
// 只有 `dumpobj <addr>` 这条路；而地址必须自己找）。所以生成器自己解析 dump 的
// 内存范围表、在内存里扫锚点模式、再逐级解引用——这样**不需要游戏在跑**，也不需要
// 任何已删除的 temp/ 探针。
//
// 支持两种内存流（createdump 视 dump 大小择一）：
// - `Memory64ListStream`(9)：大 dump（3 GB 级）用的形态，数据紧跟在描述表之后；
// - `MemoryListStream`(5)：小 dump 用的形态，每个范围带自己的文件 RVA。
// 两者都存在时按地址合并（同一地址以先出现的为准）。
//
// 只读保证：本模块只 `File::open` + `Seek` + `Read`，不写 dump、不碰任何进程。

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// dump 里的一个可读内存范围：虚拟地址 → 文件偏移。
#[derive(Clone, Copy, Debug)]
pub struct MemRange {
    pub start: u64,
    pub size: u64,
    pub file_offset: u64,
}

/// dump 模块表里的一项。
#[derive(Clone, Debug)]
pub struct Module {
    pub base: u64,
    pub size: u32,
    /// 模块路径（dump 里记的绝对路径；可能不存在于本机，例如别人的机器上采的 dump）。
    pub path: String,
    /// `VS_FIXEDFILEINFO` 的文件版本（`a.b.c.d`）。
    pub file_version: Option<String>,
}

impl Module {
    /// 模块文件名（小写），例如 `osu!.dll`。
    pub fn file_name(&self) -> String {
        Path::new(&self.path)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| self.path.clone())
    }
}

/// 打开的 dump（持有文件句柄 + 内存范围表）。
pub struct Dump {
    file: File,
    pub path: PathBuf,
    pub bytes: u64,
    pub ranges: Vec<MemRange>,
    pub modules: Vec<Module>,
    /// `SystemInfoStream.ProcessorArchitecture`：9 = AMD64 → x64，0 = x86。
    pub arch: String,
    /// `MiscInfoStream.ProcessId`（0 = 未记录）。
    pub pid: u32,
    /// `MiscInfoStream` 的进程创建时间（Unix 秒；0 = 未记录）。
    pub process_create_time: u32,
}

const STREAM_MODULE_LIST: u32 = 4;
const STREAM_MEMORY_LIST: u32 = 5;
const STREAM_SYSTEM_INFO: u32 = 7;
const STREAM_MEMORY64_LIST: u32 = 9;
const STREAM_MISC_INFO: u32 = 15;

impl Dump {
    /// 打开并解析 dump 的目录（不读内存正文）。
    pub fn open(path: &Path) -> Result<Dump, String> {
        let mut file = File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
        let bytes = file
            .metadata()
            .map_err(|e| format!("stat {}: {e}", path.display()))?
            .len();
        let mut header = [0u8; 32];
        read_exact_at(&mut file, 0, &mut header)?;
        let signature = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
        if signature != 0x504D_444D {
            return Err(format!(
                "not a minidump: signature 0x{signature:08X} (expected 0x504D444D \"MDMP\")"
            ));
        }
        let stream_count = u32::from_le_bytes([header[8], header[9], header[10], header[11]]);
        let directory_rva = u32::from_le_bytes([header[12], header[13], header[14], header[15]]);
        let mut dump = Dump {
            file,
            path: path.to_path_buf(),
            bytes,
            ranges: Vec::new(),
            modules: Vec::new(),
            arch: "unknown".to_string(),
            pid: 0,
            process_create_time: 0,
        };
        let mut directory = vec![0u8; (stream_count as usize) * 12];
        read_exact_at(&mut dump.file, directory_rva as u64, &mut directory)?;
        let mut memory64: Option<(u64, u64, u64)> = None; // (count, base_rva, stream_rva)
        let mut memory_list: Vec<(u32, u32)> = Vec::new(); // (rva, size)
        for index in 0..stream_count as usize {
            let entry = &directory[index * 12..index * 12 + 12];
            let stream_type = u32::from_le_bytes([entry[0], entry[1], entry[2], entry[3]]);
            let data_size = u32::from_le_bytes([entry[4], entry[5], entry[6], entry[7]]);
            let rva = u32::from_le_bytes([entry[8], entry[9], entry[10], entry[11]]);
            match stream_type {
                STREAM_MEMORY64_LIST => {
                    let mut head = [0u8; 16];
                    read_exact_at(&mut dump.file, rva as u64, &mut head)?;
                    let count = u64::from_le_bytes(head[0..8].try_into().unwrap());
                    let base_rva = u64::from_le_bytes(head[8..16].try_into().unwrap());
                    memory64 = Some((count, base_rva, rva as u64));
                }
                STREAM_MEMORY_LIST => memory_list.push((rva, data_size)),
                STREAM_MODULE_LIST => dump.read_module_list(rva, data_size)?,
                STREAM_SYSTEM_INFO => dump.read_system_info(rva)?,
                STREAM_MISC_INFO => dump.read_misc_info(rva)?,
                _ => {}
            }
        }
        if let Some((count, base_rva, stream_rva)) = memory64 {
            dump.read_memory64_list(count, base_rva, stream_rva)?;
        }
        for (rva, size) in memory_list {
            dump.read_memory_list(rva, size)?;
        }
        if dump.ranges.is_empty() {
            return Err(
                "dump has no memory ranges (neither Memory64ListStream nor MemoryListStream)"
                    .to_string(),
            );
        }
        dump.ranges.sort_by_key(|r| r.start);
        dump.ranges.dedup_by_key(|r| r.start);
        Ok(dump)
    }

    fn read_memory64_list(&mut self, count: u64, base_rva: u64, stream_rva: u64) -> Result<(), String> {
        // `MINIDUMP_MEMORY64_LIST` = { NumberOfMemoryRanges(8), BaseRva(8), MemoryRanges[] }：
        // 描述表紧跟流头部（stream_rva + 16），而 `BaseRva` 指向**第一段数据**的起始，
        // 第 i 段的数据 = BaseRva + 前面各段长度之和。
        if count == 0 || count > 1_000_000 {
            return Err(format!("Memory64ListStream has {count} ranges"));
        }
        let mut table = vec![0u8; (count as usize) * 16];
        read_exact_at(&mut self.file, stream_rva + 16, &mut table)?;
        let mut data_offset = base_rva;
        for index in 0..count as usize {
            let entry = &table[index * 16..index * 16 + 16];
            let start = u64::from_le_bytes(entry[0..8].try_into().unwrap());
            let size = u64::from_le_bytes(entry[8..16].try_into().unwrap());
            self.ranges.push(MemRange {
                start,
                size,
                file_offset: data_offset,
            });
            data_offset += size;
        }
        Ok(())
    }

    fn read_memory_list(&mut self, rva: u32, size: u32) -> Result<(), String> {
        if size < 4 {
            return Ok(());
        }
        let mut head = [0u8; 4];
        read_exact_at(&mut self.file, rva as u64, &mut head)?;
        let count = u32::from_le_bytes(head) as usize;
        let available = (size as usize - 4) / 16;
        let count = count.min(available);
        let mut table = vec![0u8; count * 16];
        read_exact_at(&mut self.file, rva as u64 + 4, &mut table)?;
        for index in 0..count {
            let entry = &table[index * 16..index * 16 + 16];
            let start = u64::from_le_bytes(entry[0..8].try_into().unwrap());
            let length = u32::from_le_bytes(entry[8..12].try_into().unwrap());
            let data_rva = u32::from_le_bytes(entry[12..16].try_into().unwrap());
            self.ranges.push(MemRange {
                start,
                size: length as u64,
                file_offset: data_rva as u64,
            });
        }
        Ok(())
    }

    fn read_module_list(&mut self, rva: u32, _size: u32) -> Result<(), String> {
        let mut head = [0u8; 4];
        read_exact_at(&mut self.file, rva as u64, &mut head)?;
        let count = u32::from_le_bytes(head) as usize;
        // MINIDUMP_MODULE = 108 字节：BaseOfImage(8) SizeOfImage(4) CheckSum(4)
        // TimeDateStamp(4) ModuleNameRva(4) VersionInfo(52) CvRecord(8) MiscRecord(8) Reserved(16)
        let mut table = vec![0u8; count * 108];
        read_exact_at(&mut self.file, rva as u64 + 4, &mut table)?;
        for index in 0..count {
            let entry = &table[index * 108..index * 108 + 108];
            let base = u64::from_le_bytes(entry[0..8].try_into().unwrap());
            let size = u32::from_le_bytes(entry[8..12].try_into().unwrap());
            let name_rva = u32::from_le_bytes(entry[20..24].try_into().unwrap());
            // VS_FIXEDFILEINFO：dwSignature@24, dwStrucVersion@28, dwFileVersionMS@32, dwFileVersionLS@36
            let ms = u32::from_le_bytes(entry[32..36].try_into().unwrap());
            let ls = u32::from_le_bytes(entry[36..40].try_into().unwrap());
            let signature = u32::from_le_bytes(entry[24..28].try_into().unwrap());
            let file_version = (signature == 0xFEEF_04BD).then(|| {
                format!(
                    "{}.{}.{}.{}",
                    ms >> 16,
                    ms & 0xFFFF,
                    ls >> 16,
                    ls & 0xFFFF
                )
            });
            let path = self.read_minidump_string(name_rva).unwrap_or_default();
            self.modules.push(Module {
                base,
                size,
                path,
                file_version,
            });
        }
        Ok(())
    }

    fn read_system_info(&mut self, rva: u32) -> Result<(), String> {
        let mut head = [0u8; 8];
        read_exact_at(&mut self.file, rva as u64, &mut head)?;
        let architecture = u16::from_le_bytes([head[0], head[1]]);
        self.arch = match architecture {
            9 => "x64".to_string(),
            0 => "x86".to_string(),
            5 => "arm".to_string(),
            12 => "arm64".to_string(),
            other => format!("unknown({other})"),
        };
        Ok(())
    }

    fn read_misc_info(&mut self, rva: u32) -> Result<(), String> {
        let mut head = [0u8; 24];
        read_exact_at(&mut self.file, rva as u64, &mut head)?;
        let size_of_info = u32::from_le_bytes(head[0..4].try_into().unwrap());
        if size_of_info < 24 {
            return Ok(());
        }
        self.pid = u32::from_le_bytes(head[8..12].try_into().unwrap());
        self.process_create_time = u32::from_le_bytes(head[12..16].try_into().unwrap());
        Ok(())
    }

    /// `MINIDUMP_STRING`（u32 字节长度 + UTF-16LE）。
    fn read_minidump_string(&mut self, rva: u32) -> Option<String> {
        let mut head = [0u8; 4];
        read_exact_at(&mut self.file, rva as u64, &mut head).ok()?;
        let len = u32::from_le_bytes(head) as usize;
        if len == 0 || len > 4096 {
            return None;
        }
        let mut raw = vec![0u8; len];
        read_exact_at(&mut self.file, rva as u64 + 4, &mut raw).ok()?;
        let units: Vec<u16> = raw
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        Some(String::from_utf16_lossy(&units))
    }

    /// 虚拟地址 → 内存范围（找不到 = 地址不在 dump 覆盖范围内）。
    pub fn range_for(&self, addr: u64) -> Option<&MemRange> {
        let index = match self.ranges.binary_search_by(|r| r.start.cmp(&addr)) {
            Ok(index) => index,
            Err(0) => return None,
            Err(index) => index - 1,
        };
        let range = &self.ranges[index];
        (addr >= range.start && addr - range.start < range.size).then_some(range)
    }

    /// 读一段 dump 内存（必须整段落在同一个范围里；跨范围 = `None`）。
    pub fn read_at(&mut self, addr: u64, length: usize) -> Option<Vec<u8>> {
        let range = *self.range_for(addr)?;
        if (addr - range.start) + (length as u64) > range.size {
            return None;
        }
        let offset = range.file_offset + (addr - range.start);
        let mut buffer = vec![0u8; length];
        read_exact_at(&mut self.file, offset, &mut buffer).ok()?;
        Some(buffer)
    }

    /// 读一个小端 u64（读不到 = `None`）。
    pub fn read_u64(&mut self, addr: u64) -> Option<u64> {
        let bytes = self.read_at(addr, 8)?;
        Some(u64::from_le_bytes(bytes[0..8].try_into().unwrap()))
    }

    /// 按 `System.String` 的布局（x64：`_stringLength`@+0x08、`_firstChar`@+0x0C）读字符串。
    ///
    /// 这里**故意**把 `length_offset` / `chars_offset` 作为参数：两个值来自 SOS 字段表，
    /// 正是"生成的表"要证明的东西——用刚提取出来的偏移去读一个字符串，读得出且长度对得上，
    /// 就是这条表在**同一份 dump 上**的结构自证（P4b 8/8 的第 8 项）。
    pub fn read_clr_string(
        &mut self,
        addr: u64,
        length_offset: i64,
        chars_offset: i64,
        max_chars: usize,
    ) -> Option<String> {
        let mut length_bytes = [0u8; 4];
        length_bytes.copy_from_slice(&self.read_at(addr + length_offset as u64, 4)?);
        let length = i32::from_le_bytes(length_bytes);
        if length < 0 || length as usize > max_chars {
            return None;
        }
        let raw = self.read_at(addr + chars_offset as u64, length as usize * 2)?;
        let units: Vec<u16> = raw
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        Some(String::from_utf16_lossy(&units))
    }

    /// 在 dump 的**全部内存范围**里扫一个字节模式（锚点用）。
    ///
    /// 返回（命中地址、扫过字节数、耗时毫秒、是否因命中上限提前停止）。
    pub fn scan(
        &mut self,
        pattern: &[u8],
        max_hits: usize,
        chunk: usize,
    ) -> Result<ScanOutcome, String> {
        if pattern.is_empty() {
            return Err("empty scan pattern".to_string());
        }
        let ranges = self.ranges.clone();
        let mut hits = Vec::new();
        let mut scanned = 0u64;
        let mut carry: Vec<u8> = Vec::new();
        let mut capped = false;
        let mut buffer = vec![0u8; chunk];
        for range in ranges {
            if capped {
                break;
            }
            let mut cursor = range.start;
            while cursor < range.start + range.size {
                let want = chunk.min((range.start + range.size - cursor) as usize);
                let offset = range.file_offset + (cursor - range.start);
                let got = match read_some_at(&mut self.file, offset, &mut buffer[..want]) {
                    Some(n) => n,
                    None => {
                        carry.clear();
                        cursor += want as u64;
                        continue;
                    }
                };
                if got == 0 {
                    break;
                }
                let window_len = carry.len() + got;
                let mut window = Vec::with_capacity(window_len);
                window.extend_from_slice(&carry);
                window.extend_from_slice(&buffer[..got]);
                let window_addr = cursor - carry.len() as u64;
                let mut index = 0usize;
                while index + pattern.len() <= window.len() {
                    if window[index] == pattern[0]
                        && &window[index..index + pattern.len()] == pattern
                    {
                        let addr = window_addr + index as u64;
                        if !hits.contains(&addr) {
                            hits.push(addr);
                        }
                        if hits.len() >= max_hits {
                            capped = true;
                            break;
                        }
                    }
                    index += 1;
                }
                scanned += got as u64;
                let keep = (pattern.len() - 1).min(window.len());
                carry.clear();
                carry.extend_from_slice(&window[window.len() - keep..]);
                cursor += got as u64;
                if capped {
                    break;
                }
            }
        }
        Ok(ScanOutcome {
            hits,
            scanned,
            capped,
        })
    }
}

/// 一次锚点扫描的结果。
pub struct ScanOutcome {
    pub hits: Vec<u64>,
    pub scanned: u64,
    pub capped: bool,
}

fn read_exact_at(file: &mut File, offset: u64, buffer: &mut [u8]) -> Result<(), String> {
    file.seek(SeekFrom::Start(offset))
        .map_err(|e| format!("seek {offset}: {e}"))?;
    file.read_exact(buffer)
        .map_err(|e| format!("read {offset}+{}: {e}", buffer.len()))
}

/// 尽力读（短读允许）：返回实际读到的字节数。
fn read_some_at(file: &mut File, offset: u64, buffer: &mut [u8]) -> Option<usize> {
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut total = 0usize;
    while total < buffer.len() {
        match file.read(&mut buffer[total..]) {
            Ok(0) => break,
            Ok(n) => total += n,
            Err(_) => return if total > 0 { Some(total) } else { None },
        }
    }
    Some(total)
}

// ---------------------------------------------------------------- 磁盘 PE ----

/// 读磁盘上 PE 文件的 `IMAGE_FILE_HEADER.Machine`（`0x8664` = x64，`0x014C` = x86）。
///
/// 只读文件头，不碰进程。`collect` 用它区分"x64 的 lazer"与"32 位的 stable"——
/// 两者进程名都是 `osu!.exe`（计划 A4 的位数分派）。
pub fn pe_machine(path: &Path) -> Option<u16> {
    let mut file = File::open(path).ok()?;
    let mut head = vec![0u8; 0x1000];
    let got = read_some_at(&mut file, 0, &mut head)?;
    head.truncate(got);
    if head.len() < 0x40 || head[0] != b'M' || head[1] != b'Z' {
        return None;
    }
    let e_lfanew = u32::from_le_bytes([head[0x3C], head[0x3D], head[0x3E], head[0x3F]]) as usize;
    let machine_at = e_lfanew + 4;
    if machine_at + 2 > head.len() {
        return None;
    }
    Some(u16::from_le_bytes([head[machine_at], head[machine_at + 1]]))
}

/// 机器码的显示名。
pub fn machine_name(machine: u16) -> &'static str {
    match machine {
        0x8664 => "x86-64/64-bit",
        0x014C => "i386/32-bit",
        0xAA64 => "arm64",
        _ => "unknown",
    }
}