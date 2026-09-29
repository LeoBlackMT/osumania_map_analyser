// 锚点定址结果的**进程内**缓存（Step 9c）：同一映像的重复定址只做**验证**，不重扫。
//
// 背景（真机实测，`%TEMP%\mma-shell-step9\shell-err3.txt`）：一次全量锚点扫描要遍历
// 842–1132 个区域、读 ~4 GB（16.6 s / 18.6 s 两个实例）。读取器主循环里有三条会重新定址的
// 路径——读失败 detach 后重附着、冻结窗口（30 s）到期强制重解析、进程重启换目标——其中
// **同一进程实例**（映像没换、加载基址没换）的锚点地址是不变的，重扫纯属浪费。
//
// ⚠️ 为什么不能跨进程实例复用：锚点地址是**绝对地址**（真机两个实例的 `statusPtr` 分别是
// `0x052CEB34` / `0x0369EDB4`，且与模块基址的差不是同一个位移 ⇒ 不是"整块映像平移"，
// 而是堆上的 .NET 静态对象），所以键里必须有 `module_base`，且进程重启一律重扫。
//
// 判据（**安全口径写死**，单测逐条钉住）：
// - 键 = `(exe md5, bitness, module_base)`：三者任一变化 ⇒ 条目**立即丢弃**（`observe`），
//   绝不复用另一次加载/另一个映像的地址；
// - 缓存的**唯一出口**是 [`AnchorCache::resolve`]：键命中 **且** 每个已解析锚点都在**当前**
//   进程上重新通过验证才返回表 ⇒ **缓存命中绝不绕过验证**（没有任何"未验证也能拿到表"的
//   API），任一锚点不成立就是 `Stale`（调用方丢弃条目 + 全量重扫 + 记日志说明是哪一枚、
//   哪一项检查失败）；
// - 验证本身 = ① **签名复核**（把 match 地址处 `pattern.len()` 字节读回来，用台账里同一份
//   掩码签名再匹配一次）② **结构自证**（`mod.rs::anchor_proves_out`，与扫描时选候选的判据
//   逐字相同）。两项都在调用方组合（`mod.rs::cached_anchors`）——本文件只规定"每个已解析
//   锚点都必须过验证"，不规定验证的实现；
// - 未解析的锚点（best-effort，如 `getAudioLengthPtr`）**按状态一起缓存**：重附着不再为它们
//   重扫，表里依旧是 `None`（"未解析"这一事实来自上一次扫描，不是这次验证出来的）。
//
// 台账语义（`patterns.rs`）**不变**：缓存来的锚点仍然是"在 build Y 的地址 X 上签名成立"——
// 它刚刚在当前进程上重新通过了同一份签名复核与结构自证；差别只在日志里写明
// `anchors from cache validated in Nms`（而不是 `anchors resolved in Nms`）。

use crate::osu::patterns::{self, AnchorTable};

/// 缓存键：映像身份 + 位数 + 加载基址。三者任一不同就不能复用上一次的地址。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CacheKey {
    /// `osu!.exe` 的磁盘 MD5（小写 32 hex；与 `patterns::VERIFIED_BUILD_STABLE_MD5` 同域）。
    pub exe_md5: String,
    /// PE machine（stable = `0x014C`）。
    pub bitness: u16,
    /// 该映像在本进程里的加载基址（`MODULEENTRY32W.modBaseAddr`；ASLR ⇒ 每次启动都可能变）。
    pub module_base: u32,
}

impl CacheKey {
    pub fn new(exe_md5: String, bitness: u16, module_base: u32) -> CacheKey {
        CacheKey {
            exe_md5,
            bitness,
            module_base,
        }
    }
}

/// 一次成功定址的结果（缓存条目）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnchorEntry {
    /// 已解析的地址（未解析的键是 `None`）。
    pub table: AnchorTable,
    /// 上一次扫描里"最终未解析"的 best-effort 键（按状态缓存，顺序 = `BEST_EFFORT_KEYS`）。
    pub unresolved: Vec<&'static str>,
}

impl AnchorEntry {
    /// 从一份**必需锚点齐全**的表推导未解析的 best-effort 键（顺序照 [`patterns::BEST_EFFORT_KEYS`]）。
    pub fn from_table(table: AnchorTable) -> AnchorEntry {
        let unresolved = patterns::BEST_EFFORT_KEYS
            .iter()
            .copied()
            .filter(|key| table.get(key).is_none())
            .collect();
        AnchorEntry { table, unresolved }
    }

    /// 日志/证据用的地址摘要（与扫描成功那一行逐字段相同 ⇒ 两条日志可直接 diff）。
    pub fn describe(&self) -> String {
        format!(
            "statusPtr=0x{:08X} baseAddr=0x{:08X} playTimeAddr=0x{:08X} rulesetsAddr=0x{:08X} menuModsPtr=0x{:08X} getAudioLengthPtr=0x{:08X} settingsClassAddr=0x{:08X}",
            self.table.status_ptr.unwrap_or(0),
            self.table.base_addr.unwrap_or(0),
            self.table.play_time_addr.unwrap_or(0),
            self.table.rulesets_addr.unwrap_or(0),
            self.table.menu_mods_ptr.unwrap_or(0),
            self.table.audio_length_ptr.unwrap_or(0),
            self.table.settings_class_addr.unwrap_or(0),
        )
    }
}

/// 缓存查询的结果（**纯值**）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cached {
    /// 键没命中（没缓存过 / 键变了）⇒ 必须全量扫描。
    NoEntry,
    /// 键命中但该锚点在当前进程上**不再成立**（返回它）⇒ 条目作废 + 全量扫描。
    Stale(&'static str),
    /// 每个已解析锚点都重新验证通过 ⇒ 可以直接用这份表（未解析项沿用缓存状态）。
    Validated(AnchorEntry),
}

/// 一次 `observe` 的结果（键变了要能说清"丢掉了哪一个"，日志/证据需要）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyChange {
    /// 第一次见到这个键（没有旧条目可丢）。
    First,
    /// 同键：条目保留（可以试缓存）。
    Same,
    /// 键变了：旧条目**已丢弃**（返回被丢掉的那个键）。
    Dropped(CacheKey),
}

/// 单槽缓存：只保留最近一次定址结果（键一变即丢，见 [`AnchorCache::observe`]）。
#[derive(Clone, Debug, Default)]
pub struct AnchorCache {
    key: Option<CacheKey>,
    entry: Option<AnchorEntry>,
}

impl AnchorCache {
    pub const fn new() -> AnchorCache {
        AnchorCache {
            key: None,
            entry: None,
        }
    }

    /// 定址前调用：键与缓存里的不同 ⇒ **丢掉条目**（绝不复用另一次加载的地址）。
    pub fn observe(&mut self, key: &CacheKey) -> KeyChange {
        match self.key.take() {
            None => KeyChange::First,
            Some(previous) if &previous == key => {
                self.key = Some(previous);
                KeyChange::Same
            }
            Some(previous) => {
                self.entry = None;
                KeyChange::Dropped(previous)
            }
        }
    }

    /// 丢弃条目（验证失败时调用：那一份地址已被证伪，留着只会每次白验证一遍）。
    pub fn clear(&mut self) {
        self.key = None;
        self.entry = None;
    }

    /// 缓存路径的**唯一出口**。
    ///
    /// `proves_out(key, addr)` = "该锚点此刻在这个地址上仍然成立吗"（由调用方给出真实的
    /// 读内存检查）。语义：
    /// - 键没命中 ⇒ [`Cached::NoEntry`]（**不**返回任何地址）；
    /// - 逐条验证 [`patterns::ANCHORS`] 里**已解析**的锚点（顺序 = 台账顺序），首个不成立
    ///   即 [`Cached::Stale`]（**不**返回表 —— 缓存命中绝不发布未通过验证的地址）；
    /// - 全部通过 ⇒ [`Cached::Validated`]（未解析项沿用缓存状态，本函数不为它们发明地址）。
    pub fn resolve<F>(&self, key: &CacheKey, mut proves_out: F) -> Cached
    where
        F: FnMut(&str, u32) -> bool,
    {
        let (Some(cached_key), Some(entry)) = (self.key.as_ref(), self.entry.as_ref()) else {
            return Cached::NoEntry;
        };
        if cached_key != key {
            return Cached::NoEntry;
        }
        for anchor in patterns::ANCHORS {
            let Some(addr) = entry.table.get(anchor.key) else {
                continue;
            };
            if !proves_out(anchor.key, addr) {
                return Cached::Stale(anchor.key);
            }
        }
        Cached::Validated(entry.clone())
    }

    /// 全量扫描成功后写入（旧键的条目被替换 ⇒ "键变即丢"在写入侧也成立）。
    pub fn store(&mut self, key: CacheKey, entry: AnchorEntry) {
        self.key = Some(key);
        self.entry = Some(entry);
    }
}