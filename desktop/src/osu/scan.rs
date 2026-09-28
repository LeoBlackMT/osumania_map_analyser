// 掩码签名匹配（IDA 风格 `??` 通配）+ 虚拟区扫描驱动。
//
// 两层分开，便于单测：
// - **纯函数层**（本文件上半）：`Pattern` + `find_first`——只吃 `&[u8]`，零进程依赖，
//   合成缓冲即可覆盖跨块/通配/负位移/零命中边界。
// - **IO 层**（本文件下半）：区域遍历 + 分块读。读取策略照计划 §3.4：
//   单次 `ReadProcessMemory` 硬上限 1 MiB；失败就**对半缩小重试**（`ERROR_PARTIAL_COPY` 的
//   典型处置）；**绝不零填充**——失败的块直接跳过并计数，不拿去匹配（否则 0 字节会伪造命中）。

#[cfg(windows)]
use crate::osu::win;

/// 单次 `ReadProcessMemory` 的硬上限（1 MiB）。
pub const CHUNK_MAX: usize = 1024 * 1024;
/// 缩小重试的下限：低于此值不再重试（再小也读不到就没有意义）。
pub const CHUNK_MIN: usize = 4096;

/// 一个可扫区域（`VirtualQueryEx` 的产物）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region {
    pub base: usize,
    pub size: usize,
}

/// 解析好的掩码签名：`sig` 与 `mask` 等长，`mask[i] == 0` 表示该位是通配。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pattern {
    pub sig: Vec<u8>,
    pub mask: Vec<u8>,
}

impl Pattern {
    /// 解析 `"F8 01 ?? 04"` 这样的 IDA 风格串（`??` 与 `?` 都是单字节通配）。
    /// 出现空 token 之外的非十六进制内容即报错（不静默跳过——静默跳过会让签名变短而误命中）。
    pub fn parse(text: &str) -> Result<Pattern, String> {
        let mut sig = Vec::new();
        let mut mask = Vec::new();
        for token in text.split_whitespace() {
            if token == "??" || token == "?" {
                sig.push(0);
                mask.push(0);
                continue;
            }
            let byte =
                u8::from_str_radix(token, 16).map_err(|_| format!("bad pattern byte: {token}"))?;
            sig.push(byte);
            mask.push(1);
        }
        if sig.is_empty() {
            return Err("empty pattern".to_string());
        }
        Ok(Pattern { sig, mask })
    }

    pub fn len(&self) -> usize {
        self.sig.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sig.is_empty()
    }

    /// `window[at..]` 是否与签名逐字节相符（通配位无条件通过）。
    pub fn matches_at(&self, window: &[u8], at: usize) -> bool {
        if at + self.sig.len() > window.len() {
            return false;
        }
        let mut i = 0;
        while i < self.sig.len() {
            if self.mask[i] != 0 && window[at + i] != self.sig[i] {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 首个命中在 `window` 内的偏移（从左到右，取第一个）。零命中返回 `None`。
    pub fn find_first(&self, window: &[u8]) -> Option<usize> {
        if self.sig.is_empty() || window.len() < self.sig.len() {
            return None;
        }
        let mut at = 0;
        // 只扫到"最后一个可能放得下签名的位置"为止。
        while at + self.sig.len() <= window.len() {
            if self.matches_at(window, at) {
                return Some(at);
            }
            at += 1;
        }
        None
    }
}

// ---- IO 层 ----

#[cfg(windows)]
#[derive(Default, Debug, Clone)]
pub struct ScanStats {
    pub regions: usize,
    pub chunks_ok: u64,
    pub chunks_failed: u64,
    pub bytes_read: u64,
    pub elapsed_ms: u128,
}

/// 读一块：从 `want`（已被 `CHUNK_MAX` 夹取）开始，失败即对半缩小重试，直到 `CHUNK_MIN`。
/// 返回**实际读满**的缓冲（长度即请求长度），失败返回 `None`。**绝不返回零填充的假数据。**
#[cfg(windows)]
fn read_chunk(handle: win::Handle, addr: usize, want: usize) -> Option<Vec<u8>> {
    let mut len = want.min(CHUNK_MAX);
    while len >= CHUNK_MIN {
        let mut buf = vec![0u8; len];
        if win::read_exact_at(handle, addr as u32, &mut buf).is_ok() {
            return Some(buf);
        }
        len /= 2;
    }
    None
}

/// 在给定区域里收集 `pattern` 的全部命中（最多 `limit` 个），返回 `命中地址 + offset`。
///
/// 分块读时携带 `pattern.len() - 1` 字节的尾接（overlap），保证跨块边界的签名不被漏掉；
/// 读失败的块**推进 `CHUNK_MAX` 并丢弃尾接**（不能拿上一块的尾巴去接下一块的头冒充连续内存）。
///
/// 热点路径：先用签名的**首字节**做快速筛（x86 签名首字节是 opcode，命中率低），
/// 命中候选才逐个做掩码比较——否则每个窗口位置都进一次掩码循环，1.4 GB 的扫描会到秒级。
#[cfg(windows)]
pub fn find_in_regions(
    handle: win::Handle,
    regions: &[Region],
    pattern: &Pattern,
    offset: i32,
    limit: usize,
    stats: &mut ScanStats,
) -> Vec<u32> {
    let sig_len = pattern.len();
    if sig_len == 0 || limit == 0 {
        return Vec::new();
    }
    let carry_len = sig_len - 1;
    let first_byte = pattern.sig[0];
    let mut hits: Vec<u32> = Vec::new();
    'regions: for region in regions {
        let mut addr = region.base;
        let mut remaining = region.size;
        let mut carry: Vec<u8> = Vec::new();
        while remaining > 0 {
            let want = remaining.min(CHUNK_MAX);
            match read_chunk(handle, addr, want) {
                Some(buf) => {
                    stats.chunks_ok += 1;
                    stats.bytes_read += buf.len() as u64;
                    let window_base = addr - carry.len();
                    let mut window = Vec::with_capacity(carry.len() + buf.len());
                    window.extend_from_slice(&carry);
                    window.extend_from_slice(&buf);
                    let mut at = 0usize;
                    while at + sig_len <= window.len() {
                        if window[at] == first_byte && pattern.matches_at(&window, at) {
                            // 有符号位移必须先在 i64 里加：`statusPtr` 是 -0x4，命中地址可能是
                            // 0x0…3（u32 相减会下溢 panic，debug 构建直接崩）。
                            let hit = (window_base + at) as i64;
                            hits.push((hit + offset as i64) as u32);
                            if hits.len() >= limit {
                                break 'regions;
                            }
                        }
                        at += 1;
                    }
                    let keep = carry_len.min(window.len());
                    carry = window[window.len() - keep..].to_vec();
                    addr += buf.len();
                    remaining -= buf.len();
                }
                None => {
                    stats.chunks_failed += 1;
                    addr += want;
                    remaining -= want;
                    carry.clear();
                }
            }
        }
    }
    hits
}

/// 首个命中（`find_in_regions` 的 `limit == 1` 便捷形态）。仅供诊断使用；
/// **产品路径用 `find_in_regions` + 逐候选自证**（首个命中可能是假阳性）。
#[cfg(windows)]
pub fn find_first_in_regions(
    handle: win::Handle,
    regions: &[Region],
    pattern: &Pattern,
    offset: i32,
    stats: &mut ScanStats,
) -> Option<u32> {
    find_in_regions(handle, regions, pattern, offset, 1, stats)
        .into_iter()
        .next()
}
