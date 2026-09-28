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

/// 单次 `ReadProcessMemory` 的硬上限（1 MiB）——**权威定义在 `win::READ_CALL_MAX`**，
/// §3.4 的"单次读上限 1 MiB"只有这一个数值来源（两处各写一个字面量迟早会漂移）。
pub const CHUNK_MAX: usize = win::READ_CALL_MAX;
/// 缩小重试的下限：低于此值不再重试（再小也读不到就没有意义）。
pub const CHUNK_MIN: usize = 4096;

/// 一个可扫区域（`VirtualQueryEx` 的产物）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region {
    pub base: usize,
    pub size: usize,
}

/// **按附着缓存的区域列表**（C1 补：此前每次 `resolve_anchors` 都重走 `VirtualQueryEx`，
/// 且两种过滤器各走一遍）。
///
/// 生命周期（写死，C1 落地）：
/// - **缓存上限 = 一次附着**：`Reader` 的主循环在附着成功时 `RegionCache::new()`、
///   detach/重附着时连同 `Target` 一起丢弃（缓存里装的是那个进程的 VAD 快照，换进程即失效）；
/// - 键 = `VirtualQueryEx` 的 `access_mask`（两种过滤器的区域集不同，各自缓存）；
/// - `refresh` 只覆盖**同 mask** 的条目：切换过滤器回到前一档时不必重扫 2000 个区域。
///
/// ⚠️ 故意**不**做失效检测（真机实测：菜单态下区域集没有可见变化；进程活着时 VAD 变动
/// 也是低频事件）——锚点扫描失败时调用方 detach → 缓存随附着一起重建，那是唯一的刷新路径。
#[cfg(windows)]
#[derive(Debug, Default, Clone)]
pub struct RegionCache {
    entries: Vec<(u32, Vec<Region>)>,
}

#[cfg(windows)]
impl RegionCache {
    pub fn new() -> RegionCache {
        RegionCache::default()
    }

    /// 命中即返回（**不**重走系统调用）；未命中 ⇒ `None`（调用方的固定处置：`refresh`）。
    pub fn get(&self, access_mask: u32) -> Option<&[Region]> {
        self.entries
            .iter()
            .find(|(mask, _)| *mask == access_mask)
            .map(|(_, regions)| regions.as_slice())
    }

    /// 写入/覆盖某档过滤器的区域列表（同一 mask 只留最新一份）。
    pub fn refresh(&mut self, access_mask: u32, regions: Vec<Region>) {
        match self.entries.iter_mut().find(|(mask, _)| *mask == access_mask) {
            Some(slot) => slot.1 = regions,
            None => self.entries.push((access_mask, regions)),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 已缓存的过滤器档位（证据/日志用）。
    pub fn masks(&self) -> Vec<u32> {
        self.entries.iter().map(|(mask, _)| *mask).collect()
    }
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
    ///
    /// `at` 越界或签名放不下 ⇒ `false`（用 `checked_add`：`at = usize::MAX` 这类调用
    /// 不能靠溢出回绕成"放得下"）。
    pub fn matches_at(&self, window: &[u8], at: usize) -> bool {
        let Some(end) = at.checked_add(self.sig.len()) else {
            return false;
        };
        if end > window.len() {
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
        // 只扫到"最后一个可能放得下签名的位置"为止：`at <= window.len() - sig.len()`
        // （**减法在左**——旧写法 `at + len <= window.len()` 在 `at = len - 1` 时通过，
        // 于是 `matches_at` 拿 `end = len*2 - 1 > window.len()` 直接判 false，
        // 令"命中正好落在窗口末尾"永远扫不到；C1 修）。
        let last = window.len() - self.sig.len();
        let mut at = 0;
        while at <= last {
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
///
/// 请求长度的序列由 `win::plan_shrink_sequence` 给出（纯函数、可单测）；本函数只负责
/// 按计划下发。**每一块都必须整段读满**（`read_exact_at` 的 fail-closed 规则）。
#[cfg(windows)]
fn read_chunk(handle: win::Handle, addr: usize, want: usize) -> Option<Vec<u8>> {
    for len in win::plan_shrink_sequence(want, CHUNK_MIN) {
        let mut buf = vec![0u8; len];
        if win::read_exact_at(handle, addr as u32, &mut buf).is_ok() {
            return Some(buf);
        }
    }
    None
}

/// 在给定区域里收集 `pattern` 的全部命中（最多 `limit` 个），返回 `命中地址 + offset`。
///
/// 分块读时携带 `pattern.len() - 1` 字节的尾接（overlap），保证跨块边界的签名不被漏掉；
/// 读失败的块**推进 `CHUNK_MAX` 并丢弃尾接**（不能拿上一块的尾巴去接下一块的头冒充连续内存）。
///
/// 热点路径：**首字节是实字节**时先用它做快速筛（x86 签名首字节是 opcode，命中率低），
/// 命中候选才逐个做掩码比较——否则每个窗口位置都进一次掩码循环，1.4 GB 的扫描会到秒级。
/// 首字节是通配（`??`）时**跳过**快筛（它不携带信息，拿它筛会丢真命中）。
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
    // 首字节快速筛：**只在首字节是实字节时**才能用它当必要条件。首字节是通配（`??`）时
    // 它不携带任何信息，拿它筛会把所有"该位 ≠ 0"的真命中全丢掉（C1 修）。
    let first_byte = pattern.mask[0] != 0;
    let first_value = pattern.sig[0];
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
                    let last = window.len() - sig_len;
                    let mut at = 0usize;
                    while at <= last {
                        let candidate = if first_byte {
                            window[at] == first_value && pattern.matches_at(&window, at)
                        } else {
                            pattern.matches_at(&window, at)
                        };
                        if candidate {
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

// 纯函数用例（掩码匹配/跨块尾接/负位移/零命中/畸形模式 + 读取策略算术）挂在
// **`osu/patterns.rs`** 的 `mod tests_patterns` 上——`RegionCache` 属 IO 层
// （`cfg(windows)`），非 Windows 构建不进单测面。
