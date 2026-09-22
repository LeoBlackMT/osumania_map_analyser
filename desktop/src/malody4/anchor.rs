// Malody 4.3.7 锚点：版本表、身份键解析、PE 版本校验（纯逻辑部分）。
//
// 本文件只依赖 std：Step 8 的诊断工具用裸 rustc 编译本文件，故不得 `use serde`、
// 不得 `use md5`、不得引用 crate 内其他模块。Step 3 在本文件末尾追加 Win32 层
// （进程枚举 / ReadProcessMemory），不重复定义这里的类型与常量。
//
// 版本常量（RVA / PE 时间戳 / 文件大小）在本文件出现且只出现一处。

use std::fs::File;
use std::io::Read;
use std::path::Path;

/// 已知客户端版本。`label` 仅用于日志；`rva` 是锚点指针相对模块基址的偏移。
#[derive(Debug, Clone, PartialEq)]
pub struct ClientSpec {
    pub label: &'static str,
    pub rva: u32,
    pub pe_timestamp: u32,
    pub file_size: u64,
}

/// 已知客户端版本表（4.3.7 本机实测：`e_lfanew = 0x160`、`TimeDateStamp = 0x5D79AC91`、
/// 文件大小 4,750,848）。
pub const KNOWN_CLIENTS: &[ClientSpec] = &[ClientSpec {
    label: "4.3.7",
    rva: 0x8310EC_u32,
    pe_timestamp: 0x5D79AC91_u32,
    file_size: 4_750_848_u64,
}];

impl ClientSpec {
    /// 当前支持的版本（版本表首项）。
    pub fn current() -> &'static ClientSpec {
        &KNOWN_CLIENTS[0]
    }
}

// ==================== 设置单例（判定档 / 变速位）的构建专用 RVA 与偏移 ====================
//
// 下面这些常量**只对 `KNOWN_CLIENTS` 里那一个构建成立**（4.3.7，`TimeDateStamp 0x5D79AC91`、
// 文件大小 4,750,848）：它们来自对该构建的反汇编。取证的 disassembly 与用户本机那份
// `malody.exe` 不是同一个文件（大小与 SHA256 都不同、时间戳相同），但两者 `.text` 逐字节相同、
// config 键字符串的虚拟地址也相同，故同一条链对用户本机构建成立。
//
// 为什么放在这里：RVA / 字段偏移是"版本的一部分"，与 `KNOWN_CLIENTS` 同处一个文件——全仓库
// 只有这一处字面量，消费者一律按**名字**引用。将来遇到别的构建 → 版本门（`validate_pe_header`）
// 直接 fail closed，绝不按偏移量猜着读。
//
// 链条（两个独立对象，都在惰性单例后面，拆解路径会把指针清零 ⇒ **每 tick 都要重读指针**）：
//
//     module_base + 0x8311C4  →  S（用户设置单例）      judge = *(i32*)(S + 0x9C)   mods = *(u32*)(S + 0xD0)
//     module_base + 0x8310C4  →  P（本局 play-config）  judge = *(i32*)(P + 0x14)   mods = *(u32*)(P + 0x00)
//
// - `S` 就是 config 加载器（`0x5FA400`）与保存器（`0x5FBB00`）逐键读写的那个对象，所以
//   `config.json` 里的 `user_judge_level` / `user_mods` 正是从这里写出去的。
// - `P` 是本局的 play-config：开局从 `S` 复制，此后**局内改动只落在 `P` 上**——反汇编里
//   `user_mods` 的全部写入点就是 `S` 的构造函数、开局那次 `S + 0xD0 → P + 0x00` 的提交、
//   以及 `P + 0x00` 自身（mods 面板改的是 `P`）；判定档的 UI 处理器倒是把**两个**对象都写
//   （写在相隔 4 条指令处），所以判定档两边恒等。发布取 `P`（见 `MemoryProbe::published`）：
//   它才是游戏真正会用到的值，`S` 要等下一次提交才跟上。
// - `P` 在首个场景**之前为 0**（惰性单例）：读到 0 只表示"现在读不到"，既不是错误也不 latch。
// - 这里只读、不写：本源是零注入的只读观察，这几个偏移同样只用于 `ReadProcessMemory`。

/// `S`（用户设置单例）的槽位 RVA：`module_base + rva` 处是 4 字节指针。
pub const USER_SETTINGS_RVA: u32 = 0x8311C4;
/// `S + 0x9C`：`user_judge_level`（i32，合法 `0..=4`）。
pub const USER_SETTINGS_JUDGE_OFF: u32 = 0x9C;
/// `S + 0xD0`：`user_mods`（u32 位掩码）。
pub const USER_SETTINGS_MODS_OFF: u32 = 0xD0;
/// `P`（本局 play-config 单例）的槽位 RVA：**首个场景前为 0**。
pub const PLAY_CONFIG_RVA: u32 = 0x8310C4;
/// `P + 0x00`：本局的 `user_mods`（创建时从 `S + 0xD0` 复制）。
pub const PLAY_CONFIG_MODS_OFF: u32 = 0x00;
/// `P + 0x04`：由 `P + 0x00` 复制后**只清位**得来的派生掩码 ⇒ `P+0x04 & !P+0x00 == 0` 恒成立。
/// 这是 `P` 的**免费身份证明**（见 `decode_play_config`）。
pub const PLAY_CONFIG_DERIVED_OFF: u32 = 0x04;
/// `P + 0x14`：本局的判定档（创建时从 `S + 0x9C` 复制）。
pub const PLAY_CONFIG_JUDGE_OFF: u32 = 0x14;
/// 一次读入的对象块长度：覆盖两条链里最靠后的字段（`S + 0xD0`）再加 4 字节。
pub const SETTINGS_BLOCK_LEN: usize = 0xD8;
/// 32 位进程用户地址空间下界：Windows 保留最低 64 KiB ⇒ 低于它的"指针"必是垃圾。
pub const USER_ADDR_MIN: u32 = 0x0001_0000;
/// 32 位用户地址空间上界（4 字节对齐的最后一个地址）。
///
/// 刻意**不**收紧到 `0x7FFF_FFFF`：带 `LARGEADDRESSAWARE` 的 32 位进程能在 2 GiB 以上分配堆，
/// 收紧只会把合法指针挡掉、让本特性静默退回 `config.json`（行为仍正确，但功能失效）。
pub const USER_ADDR_MAX: u32 = 0xFFFF_FFFC;
/// 判定档的合法上界（`0..=4` → `A`~`E`）；越界 ⇒ 整份读数作废、回落 `config.json`。
pub const MAX_JUDGE_LEVEL: u8 = 4;

/// 内存里读到的一对设置（判定档 + `user_mods` 位掩码）：**已过全部 fail-closed 校验**，未作解释。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawSettings {
    /// 判定档，已校验 `0..=MAX_JUDGE_LEVEL`（越界的读数在解码阶段就整份作废）。
    pub judge: u8,
    /// `user_mods` 位掩码**原值**：未知位一律原样保留（只有变速位与 FAIR 位被解释，绝不裁剪）。
    pub user_mods: u32,
}

/// 发布取值的来源链（诊断与日志用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemorySource {
    /// `S`：用户设置单例——`config.json` 的读写对象，**兜底**（`P` 不可用时）与**旁证**。
    UserSettings,
    /// `P`：本局 play-config 单例——本局真正会用的值（**发布源**），首个场景之前为 0。
    PlayConfig,
}

/// 一次内存采样的产物：两条链各自的读数（`None` = 该链本 tick 不可确定）。
///
/// `Default` = 两条链都读不到（未附着 / 指针为 0 / 校验没过 / 短读）⇒ 调用方回落 `config.json`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MemoryProbe {
    pub user_settings: Option<RawSettings>,
    pub play_config: Option<RawSettings>,
}

impl MemoryProbe {
    /// 发布取值：**优先 `P`**（本局 play-config），`P` 读不到时才退到 `S`；两条都读不到 → `None`。
    ///
    /// 为什么发布 `P`（真机复测修正，旧实现发布 `S`）：`P` 才是"这一局真正会用的值"——游戏读的是
    /// `P + 0x04`（由 `P + 0x00` 只清位得来的派生速率）。反汇编里 `user_mods` 的写入点只有三处：
    /// `S` 的构造函数、开局时把 `S + 0xD0` 抄进 `P + 0x00` 的那次提交、以及 `P + 0x00` 自身
    /// ——**mods 面板改的是 `P`**，`S` 要等下一次提交才跟上。真机复测正是这个形态：判定档在游戏里
    /// 一改日志立刻跟着变（该 UI 处理器两个对象都写），而改变速位时日志里的 `speed_rate` 一直停在
    /// `1.00`（旧实现只发布 `S`，看不到 `P` 上的新值）。
    ///
    /// 守卫不在这里，也不该在这里：能进 `MemoryProbe` 的值都已经在 `read_chain` / `decode_user_settings`
    /// / `decode_play_config` 里过完全部 fail-closed 校验（指针为 0 / 不对齐 / 超出 32 位用户地址空间 /
    /// 块短读 / 判定档越界 / `P` 的子集不变量不成立），`None` 就表示"这条链本 tick 不可确定"。
    /// 因此这里选择的是**整条链的值**，绝不把两条链的字段拼起来（判定档两边恒等，`user_mods` 以 `P`
    /// 为准；拼装只会在两条链不一致时造出一个谁都没读到的组合）。
    ///
    /// `S` 的角色降为兜底与旁证：`P` 在首个场景之前是 0 ⇒ 那时读到的是 `S`（与旧行为一致）；
    /// 两条链同时读到时的一致性检查仍在 `disagreement`。
    pub fn published(&self) -> Option<(MemorySource, RawSettings)> {
        if let Some(raw) = self.play_config {
            return Some((MemorySource::PlayConfig, raw));
        }
        self.user_settings
            .map(|raw| (MemorySource::UserSettings, raw))
    }

    /// 两条链都读到、但判定档或 `user_mods` 不一致 → `Some((S, P))`（调用方据此记**一次**告警）。
    ///
    /// 不一致说明其中一条链的偏移理解有误。单拍的不一致可能只是"两次 `ReadProcessMemory` 之间
    /// 游戏真的改了一次设置"，所以判定"持续不一致"是调用方的事（见 `mod.rs` 的连续计数）。
    pub fn disagreement(&self) -> Option<(RawSettings, RawSettings)> {
        let (user, play) = (self.user_settings?, self.play_config?);
        (user != play).then_some((user, play))
    }
}

/// 块内取 4 字节小端 u32（越界 → `None`）。
fn read_u32_at(block: &[u8], off: u32) -> Option<u32> {
    let at = off as usize;
    let bytes = block.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// 判定档：按 i32 读、按 `0..=MAX_JUDGE_LEVEL` 校验（负数以 u32 表示必然越界）。
/// 越界 → `None`：整份读数作废，**绝不猜档位**、绝不发布一个可能错的判定。
fn valid_judge(raw: u32) -> Option<u8> {
    let level = u8::try_from(raw).ok()?;
    (level <= MAX_JUDGE_LEVEL).then_some(level)
}

/// `S` 的纯解码（合成缓冲即可测）：判定档 @0x9C、`user_mods` @0xD0。
/// 任一处读不出来、或判定档越界 → `None`。
pub fn decode_user_settings(block: &[u8]) -> Option<RawSettings> {
    let judge = valid_judge(read_u32_at(block, USER_SETTINGS_JUDGE_OFF)?)?;
    let user_mods = read_u32_at(block, USER_SETTINGS_MODS_OFF)?;
    Some(RawSettings { judge, user_mods })
}

/// `P` 的纯解码（合成缓冲即可测）：先过**身份证明**，再取 `P+0x00` / `P+0x14`。
///
/// 证明：`P+0x04` 是由 `P+0x00` 复制后**只清位**得来的 ⇒ `P+0x04 & !P+0x00 == 0` 必须恒成立。
/// 不成立说明这个地址上的东西不是我们以为的那个对象 → `None`（绝不用它发布任何数字）。
pub fn decode_play_config(block: &[u8]) -> Option<RawSettings> {
    let user_mods = read_u32_at(block, PLAY_CONFIG_MODS_OFF)?;
    let derived = read_u32_at(block, PLAY_CONFIG_DERIVED_OFF)?;
    if derived & !user_mods != 0 {
        return None;
    }
    let judge = valid_judge(read_u32_at(block, PLAY_CONFIG_JUDGE_OFF)?)?;
    Some(RawSettings { judge, user_mods })
}

/// 指针可用性（纯函数）：非 0、4 字节对齐、落在 32 位用户地址空间内。
///
/// 0 是"现在读不到"的正常取值（惰性单例），由调用方与"不可信指针"区分开（两者都走回落）。
pub fn plausible_pointer(ptr: u32) -> bool {
    ptr & 3 == 0 && (USER_ADDR_MIN..=USER_ADDR_MAX).contains(&ptr)
}

/// 锚点内存里的身份键：`<md5>_<slot>`。
///
/// `slot` 只作诊断（进 `LibraryEntry.slot`、日志与工具输出），不参与 `path` /
/// `chart_hash` / `identity` 计算。
#[derive(Debug, Clone, PartialEq)]
pub struct IdentityKey {
    pub md5: String,
    pub slot: u32,
}

/// 锚点不可用的原因。
#[derive(Debug, Clone, PartialEq)]
pub enum AnchorError {
    NotFound,
    MultipleInstances,
    AccessDenied,
    BadRead,
    TargetMismatch(&'static str),
    PlatformUnsupported,
}

impl AnchorError {
    /// 稳定短句（壳 state 帧 `reason` 的闭集来源）。
    pub fn reason(&self) -> &'static str {
        match self {
            AnchorError::NotFound => "process-not-found",
            AnchorError::MultipleInstances => "multiple-instances",
            AnchorError::AccessDenied => "access-denied",
            AnchorError::BadRead => "bad-read",
            AnchorError::TargetMismatch(inner) => inner,
            AnchorError::PlatformUnsupported => "platform-unsupported",
        }
    }
}

/// 解析身份键（严格文法）：
///
/// - 缓冲 ≥ 34 字节；
/// - 前 32 字节全为 `[0-9a-fA-F]`；
/// - 第 33 字节为 `_`；
/// - 其后 1..=9 位 ASCII 数字，直到 NUL 或缓冲末尾；
/// - md5 归一化为小写。
///
/// 任何一条不满足 → `None`（绝不用文法失败的内容凑出身份键）。
pub fn parse_identity_key(buf: &[u8]) -> Option<IdentityKey> {
    const MD5_LEN: usize = 32;
    if buf.len() < MD5_LEN + 2 {
        return None;
    }
    if !buf[..MD5_LEN].iter().all(u8::is_ascii_hexdigit) {
        return None;
    }
    if buf[MD5_LEN] != b'_' {
        return None;
    }
    let mut slot: u32 = 0;
    let mut digits = 0usize;
    for &b in &buf[MD5_LEN + 1..] {
        if b == 0 {
            break;
        }
        if !b.is_ascii_digit() {
            return None;
        }
        digits += 1;
        if digits > 9 {
            return None;
        }
        slot = slot * 10 + u32::from(b - b'0');
    }
    if digits == 0 {
        return None;
    }
    let md5 = String::from_utf8(buf[..MD5_LEN].to_vec()).ok()?.to_ascii_lowercase();
    Some(IdentityKey { md5, slot })
}

/// 校验 PE 头：`e_lfanew` @0x3C（u32 LE）、`TimeDateStamp` @`e_lfanew + 8`。
///
/// 顺序固定：越界 → 时间戳 → 文件大小。`head` 不足 2048 字节时以零填充，
/// 因此短文件会落到 `file_size_mismatch`。
pub fn validate_pe_header(
    head: &[u8; 2048],
    file_size: u64,
    spec: &ClientSpec,
) -> Result<(), &'static str> {
    let e_lfanew = u32::from_le_bytes([head[0x3C], head[0x3D], head[0x3E], head[0x3F]]) as usize;
    let stamp_at = e_lfanew.checked_add(8).ok_or("pe_header_out_of_range")?;
    if stamp_at + 4 > head.len() {
        return Err("pe_header_out_of_range");
    }
    let stamp = u32::from_le_bytes([
        head[stamp_at],
        head[stamp_at + 1],
        head[stamp_at + 2],
        head[stamp_at + 3],
    ]);
    if stamp != spec.pe_timestamp {
        return Err("pe_timestamp_mismatch");
    }
    if file_size != spec.file_size {
        return Err("file_size_mismatch");
    }
    Ok(())
}

/// 从磁盘文件校验版本（独立于进程/内存）。
///
/// 文件大小取 `fs::metadata(path).len()`——进程侧同理取 `GetModuleFileNameW` 的映像路径，
/// **绝不使用** `MODULEENTRY32W.modBaseSize`（那是 `SizeOfImage`，恒不等于真实文件大小）。
///
/// 前 2048 字节用普通 `File::read` 读取（不是 `read_exact`）：短文件读到多少算多少、
/// 其余为零，于是 `file_size != spec.file_size` 必然成立 → `TargetMismatch("file_size_mismatch")`，
/// 不会以 IO 错误的形式暴露。
pub fn validate_pe_file(path: &Path, spec: &ClientSpec) -> Result<(), AnchorError> {
    let file_size = std::fs::metadata(path).map_err(|_| AnchorError::BadRead)?.len();
    let mut head = [0u8; 2048];
    let mut file = File::open(path).map_err(|_| AnchorError::BadRead)?;
    let _ = file.read(&mut head);
    validate_pe_header(&head, file_size, spec).map_err(AnchorError::TargetMismatch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    const MD5_LOWER: &str = "08c940c9e3d3b6a5b4a2c1d0e9f8a7b6";

    fn key_buf(md5: &str, tail: &[u8]) -> Vec<u8> {
        let mut buf = md5.as_bytes().to_vec();
        buf.extend_from_slice(tail);
        buf
    }

    fn pe_head(timestamp: u32, e_lfanew: u32) -> [u8; 2048] {
        let mut head = [0u8; 2048];
        head[0x3C..0x40].copy_from_slice(&e_lfanew.to_le_bytes());
        let at = e_lfanew as usize + 8;
        if at + 4 <= head.len() {
            head[at..at + 4].copy_from_slice(&timestamp.to_le_bytes());
        }
        head
    }

    fn tmp_file(tag: &str, bytes: &[u8]) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "mma-malody4-anchor-{}-{}-{}.bin",
            tag,
            std::process::id(),
            stamp
        ));
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn version_table_has_single_known_client() {
        assert_eq!(KNOWN_CLIENTS.len(), 1);
        let spec = ClientSpec::current();
        assert_eq!(spec.label, "4.3.7");
        assert_eq!(spec.rva, 0x8310EC);
        assert_eq!(spec.pe_timestamp, 0x5D79AC91);
        assert_eq!(spec.file_size, 4_750_848);
    }

    #[test]
    fn parse_identity_key_lowercase() {
        let got = parse_identity_key(&key_buf(MD5_LOWER, b"_0\0")).unwrap();
        assert_eq!(got.md5, MD5_LOWER);
        assert_eq!(got.slot, 0);
    }

    #[test]
    fn parse_identity_key_uppercase_is_lowercased() {
        let got = parse_identity_key(&key_buf(&MD5_LOWER.to_uppercase(), b"_999999999\0")).unwrap();
        assert_eq!(got.md5, MD5_LOWER);
        assert_eq!(got.slot, 999_999_999);
    }

    #[test]
    fn parse_identity_key_mixed_case_ends_at_buffer_end() {
        let mixed = "08C940c9E3d3B6a5B4A2c1D0e9F8a7B6";
        let got = parse_identity_key(&key_buf(mixed, b"_12")).unwrap();
        assert_eq!(got.md5, MD5_LOWER);
        assert_eq!(got.slot, 12);
    }

    #[test]
    fn parse_identity_key_rejects_malformed_buffers() {
        // 31 位 hex：第 32 字节不是 `_`
        assert!(parse_identity_key(&key_buf(&MD5_LOWER[..31], b"__1\0")).is_none());
        // 33 位 hex：第 33 字节是 hex 而非 `_`
        assert!(parse_identity_key(&key_buf(&format!("{MD5_LOWER}a"), b"_1\0")).is_none());
        // 缺下划线
        assert!(parse_identity_key(&key_buf(MD5_LOWER, b"0\0")).is_none());
        // 下划线后非数字
        assert!(parse_identity_key(&key_buf(MD5_LOWER, b"_x\0")).is_none());
        assert!(parse_identity_key(&key_buf(MD5_LOWER, b"_-1\0")).is_none());
        // 下划线后为空
        assert!(parse_identity_key(&key_buf(MD5_LOWER, b"_\0")).is_none());
        // 无 NUL 且超长（10 位数字）
        assert!(parse_identity_key(&key_buf(MD5_LOWER, b"_1234567890")).is_none());
        // 空缓冲与不足 34 字节
        assert!(parse_identity_key(&[]).is_none());
        assert!(parse_identity_key(&key_buf(MD5_LOWER, b"_")).is_none());
        // 非 hex 字符
        assert!(parse_identity_key(&key_buf(&format!("g{}", &MD5_LOWER[1..]), b"_1\0")).is_none());
    }

    #[test]
    fn validate_pe_header_accepts_matching_spec() {
        let head = pe_head(0x5D79AC91, 0x160);
        assert_eq!(
            validate_pe_header(&head, 4_750_848, ClientSpec::current()),
            Ok(())
        );
    }

    #[test]
    fn validate_pe_header_rejects_timestamp_mismatch() {
        let head = pe_head(0x5D79AC92, 0x160);
        assert_eq!(
            validate_pe_header(&head, 4_750_848, ClientSpec::current()),
            Err("pe_timestamp_mismatch")
        );
    }

    #[test]
    fn validate_pe_header_rejects_file_size_mismatch() {
        let head = pe_head(0x5D79AC91, 0x160);
        assert_eq!(
            validate_pe_header(&head, 4_750_847, ClientSpec::current()),
            Err("file_size_mismatch")
        );
    }

    #[test]
    fn validate_pe_header_rejects_size_of_image_as_file_size() {
        // MODULEENTRY32W.modBaseSize（SizeOfImage）不是磁盘文件大小，必须被挡下
        let head = pe_head(0x5D79AC91, 0x160);
        assert_eq!(
            validate_pe_header(&head, 0x1094000, ClientSpec::current()),
            Err("file_size_mismatch")
        );
    }

    #[test]
    fn validate_pe_header_rejects_out_of_range_lfanew() {
        assert_eq!(
            validate_pe_header(&pe_head(0x5D79AC91, 0x1000), 4_750_848, ClientSpec::current()),
            Err("pe_header_out_of_range")
        );
        // 边界：stamp_at + 4 == 2048 刚好越界
        assert_eq!(
            validate_pe_header(&pe_head(0x5D79AC91, 0x7FC), 4_750_848, ClientSpec::current()),
            Err("pe_header_out_of_range")
        );
        assert_eq!(
            validate_pe_header(&pe_head(0x5D79AC91, 0xFFFFFFFF), 4_750_848, ClientSpec::current()),
            Err("pe_header_out_of_range")
        );
    }

    #[test]
    fn validate_pe_file_short_file_is_not_an_io_error() {
        // 16 字节：e_lfanew = 0、时间戳正确 → 唯一不匹配的是文件大小（短读只会被零填充）
        let mut bytes = vec![0u8; 16];
        bytes[8..12].copy_from_slice(&0x5D79AC91u32.to_le_bytes());
        let path = tmp_file("short", &bytes);
        assert_eq!(
            validate_pe_file(&path, ClientSpec::current()),
            Err(AnchorError::TargetMismatch("file_size_mismatch"))
        );
        let _ = std::fs::remove_file(&path);

        // 全零短文件按固定顺序先撞时间戳，同样不是 IO 错误
        let path = tmp_file("short_zero", &[0u8; 16]);
        assert_eq!(
            validate_pe_file(&path, ClientSpec::current()),
            Err(AnchorError::TargetMismatch("pe_timestamp_mismatch"))
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn validate_pe_file_accepts_crafted_header() {
        let spec = ClientSpec {
            label: "test",
            rva: 0,
            pe_timestamp: 0x1234_5678,
            file_size: 80,
        };
        let mut bytes = vec![0u8; 80];
        bytes[0x3C..0x40].copy_from_slice(&0x20u32.to_le_bytes());
        bytes[0x28..0x2C].copy_from_slice(&0x1234_5678u32.to_le_bytes());
        let path = tmp_file("good", &bytes);
        assert_eq!(validate_pe_file(&path, &spec), Ok(()));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn validate_pe_file_missing_path_is_bad_read() {
        let path = std::env::temp_dir().join(format!(
            "mma-malody4-anchor-missing-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        assert_eq!(validate_pe_file(&path, ClientSpec::current()), Err(AnchorError::BadRead));
    }

    #[test]
    fn anchor_error_reasons_are_non_empty_and_distinct() {
        let reasons = [
            AnchorError::NotFound.reason(),
            AnchorError::MultipleInstances.reason(),
            AnchorError::AccessDenied.reason(),
            AnchorError::BadRead.reason(),
            AnchorError::TargetMismatch("pe_timestamp_mismatch").reason(),
            AnchorError::TargetMismatch("file_size_mismatch").reason(),
            AnchorError::TargetMismatch("pe_header_out_of_range").reason(),
            AnchorError::PlatformUnsupported.reason(),
        ];
        for r in reasons {
            assert!(!r.is_empty(), "reason must not be empty");
        }
        for (i, a) in reasons.iter().enumerate() {
            for b in reasons.iter().skip(i + 1) {
                assert_ne!(a, b, "reasons must be pairwise distinct");
            }
        }
        assert_eq!(AnchorError::TargetMismatch("custom").reason(), "custom");
    }

    // ---- 设置单例：偏移常量与纯解码（全部用合成缓冲，不碰任何进程） ----

    /// `S` 的合成块：判定档与 `user_mods` 摆在反汇编给出的偏移上。
    fn s_block(judge: u32, mods: u32) -> Vec<u8> {
        let mut block = vec![0u8; SETTINGS_BLOCK_LEN];
        block[USER_SETTINGS_JUDGE_OFF as usize..][..4].copy_from_slice(&judge.to_le_bytes());
        block[USER_SETTINGS_MODS_OFF as usize..][..4].copy_from_slice(&mods.to_le_bytes());
        block
    }

    /// `P` 的合成块：`mods@0x00`、派生掩码 `@0x04`、判定档 `@0x14`。
    fn p_block(mods: u32, derived: u32, judge: u32) -> Vec<u8> {
        let mut block = vec![0u8; SETTINGS_BLOCK_LEN];
        block[PLAY_CONFIG_MODS_OFF as usize..][..4].copy_from_slice(&mods.to_le_bytes());
        block[PLAY_CONFIG_DERIVED_OFF as usize..][..4].copy_from_slice(&derived.to_le_bytes());
        block[PLAY_CONFIG_JUDGE_OFF as usize..][..4].copy_from_slice(&judge.to_le_bytes());
        block
    }

    #[test]
    fn settings_chain_constants_are_pinned_to_the_4_3_7_build() {
        // 这些数字是**构建专用**的：改动它们等于换一个二进制，必须重新取证（见文件头注释）。
        assert_eq!(USER_SETTINGS_RVA, 0x8311C4);
        assert_eq!(USER_SETTINGS_JUDGE_OFF, 0x9C);
        assert_eq!(USER_SETTINGS_MODS_OFF, 0xD0);
        assert_eq!(PLAY_CONFIG_RVA, 0x8310C4);
        assert_eq!(PLAY_CONFIG_MODS_OFF, 0x00);
        assert_eq!(PLAY_CONFIG_DERIVED_OFF, 0x04);
        assert_eq!(PLAY_CONFIG_JUDGE_OFF, 0x14);
        assert_eq!(MAX_JUDGE_LEVEL, 4);
        // 块长必须盖住两条链里最靠后的字段（S + 0xD0 起 4 字节）
        assert!(SETTINGS_BLOCK_LEN >= USER_SETTINGS_MODS_OFF as usize + 4);
        assert!(SETTINGS_BLOCK_LEN >= PLAY_CONFIG_JUDGE_OFF as usize + 4);
        // 地址空间边界
        assert_eq!(USER_ADDR_MIN, 0x0001_0000);
        assert_eq!(USER_ADDR_MAX, 0xFFFF_FFFC);
    }

    #[test]
    fn decode_user_settings_reads_judge_and_mods() {
        // 合法判定档 0..=4 逐个都能读出来（0..=4 → A~E 的字母映射在 model.rs 里单测）
        for judge in 0u32..=u32::from(MAX_JUDGE_LEVEL) {
            let got = decode_user_settings(&s_block(judge, 0x20)).unwrap();
            assert_eq!(got.judge, judge as u8);
            assert_eq!(got.user_mods, 0x20);
        }
        // 未知位一律原样保留（只有变速位与 FAIR 位被解释；这里不做任何裁剪）
        let raw = decode_user_settings(&s_block(1, 0xFFFF_FFFF)).unwrap();
        assert_eq!(raw.user_mods, 0xFFFF_FFFF);
        assert_eq!(raw.judge, 1);
        // 变速位与 FAIR 位同时置上时也照原样读出来
        assert_eq!(decode_user_settings(&s_block(4, 0x400 | 0x10)).unwrap().user_mods, 0x410);
    }

    #[test]
    fn decode_user_settings_rejects_out_of_range_judge() {
        // 越界（含负数：按 u32 表示必然超界）→ 整份读数作废，绝不猜档位
        for raw in [5u32, 6, 255, 256, u32::MAX, 0x8000_0000, 0xFFFF_FFFF] {
            assert_eq!(
                decode_user_settings(&s_block(raw, 0x20)),
                None,
                "judge={raw:#x} 必须判为不可用"
            );
        }
    }

    #[test]
    fn decode_user_settings_rejects_blocks_that_are_too_short() {
        let block = s_block(2, 0x10);
        let judge_at = USER_SETTINGS_JUDGE_OFF as usize;
        let mods_at = USER_SETTINGS_MODS_OFF as usize;
        // 截到判定档之前 / 判定档差 1 字节 / mods 之前 / mods 差 1 字节
        for len in [0usize, 4, judge_at, judge_at + 3, mods_at, mods_at + 3] {
            assert_eq!(
                decode_user_settings(&block[..len]),
                None,
                "len={len} 的块不得解出设置"
            );
        }
        // 恰好盖住最后一个字段（0xD0 + 4）即可解出
        assert!(decode_user_settings(&block[..mods_at + 4]).is_some());
        assert!(decode_user_settings(&block).is_some());
    }

    #[test]
    fn decode_play_config_reads_judge_and_mods() {
        let got = decode_play_config(&p_block(0x100, 0x100, 3)).unwrap();
        assert_eq!(got, RawSettings { judge: 3, user_mods: 0x100 });
        // 派生掩码 = 0（清掉了所有位）同样满足子集不变量
        assert_eq!(
            decode_play_config(&p_block(0x400, 0, 0)).unwrap(),
            RawSettings { judge: 0, user_mods: 0x400 }
        );
        // 派生掩码 = 原值的子集（清位）也合法
        assert_eq!(
            decode_play_config(&p_block(0xF0, 0x30, 4)).unwrap(),
            RawSettings { judge: 4, user_mods: 0xF0 }
        );
    }

    #[test]
    fn decode_play_config_rejects_a_broken_subset_invariant() {
        // P+0x04 & !P+0x00 != 0 ⇒ 这个地址上的东西不是我们以为的那个对象
        for (mods, derived) in [(0x10u32, 0x20u32), (0, 1), (0xFFFF_FFFE, 0xFFFF_FFFF)] {
            assert_eq!(
                decode_play_config(&p_block(mods, derived, 1)),
                None,
                "mods={mods:#x} derived={derived:#x} 必须判为不可用"
            );
        }
        // 判定档越界的 P 同样作废
        for raw in [5u32, u32::MAX, 0x8000_0001] {
            assert_eq!(decode_play_config(&p_block(0x10, 0x10, raw)), None);
        }
        // 块太短（连不变量都读不全）→ 不可用
        assert_eq!(decode_play_config(&[0u8; 4]), None);
        assert_eq!(
            decode_play_config(&p_block(0x10, 0x10, 2)[..PLAY_CONFIG_JUDGE_OFF as usize + 3]),
            None
        );
    }

    #[test]
    fn plausible_pointer_rejects_null_misaligned_and_out_of_space() {
        // 0 = "现在读不到"（惰性单例），不可信指针里第一类
        assert!(!plausible_pointer(0));
        // 对齐：非 4 字节对齐一律不可信
        assert!(!plausible_pointer(1));
        assert!(!plausible_pointer(2));
        assert!(!plausible_pointer(0x1002));
        assert!(!plausible_pointer(0xFFFF_FFFE));
        // 地址空间：低于用户下界（含被误读成指针的小整数）与高于上界都不可信
        assert!(!plausible_pointer(0x0000_FFFC));
        assert!(plausible_pointer(0x0001_0000));
        assert!(plausible_pointer(0x0040_0000));
        assert!(plausible_pointer(0x7FFF_FFFC));
        assert!(plausible_pointer(0x8000_0000), "LAA 32 位进程可在 2 GiB 以上分配");
        assert!(plausible_pointer(0xFFFF_FFFC));
    }

    #[test]
    fn published_prefers_play_config_and_falls_back_to_user_settings() {
        let s = RawSettings { judge: 1, user_mods: 0x20 };
        let p = RawSettings { judge: 4, user_mods: 0x100 };

        // 两条链都读到 ⇒ 发布 `P`（即使与 `S` 不同）：`P` 才是本局真正会用的值，
        // mods 面板只写 `P`，`S` 要等开局那次提交才跟上
        let both = MemoryProbe { user_settings: Some(s), play_config: Some(p) };
        assert_eq!(both.published(), Some((MemorySource::PlayConfig, p)));

        // `P` 在首个场景之前是 0/null ⇒ 退到 `S`（与旧行为一致）
        let only_s = MemoryProbe { user_settings: Some(s), play_config: None };
        assert_eq!(only_s.published(), Some((MemorySource::UserSettings, s)));

        // `P` 读不到（旧构建 / 拆解中 / 不变量不成立）⇒ 由 `S` 顶上
        let only_p = MemoryProbe { user_settings: None, play_config: Some(p) };
        assert_eq!(only_p.published(), Some((MemorySource::PlayConfig, p)));

        // 两条都没有 ⇒ "现在不可确定"，由调用方回落 config.json
        assert_eq!(MemoryProbe::default().published(), None);

        // 反向对照：本测试必须能区分新旧优先级（旧实现发布 `S`，这里会失败）
        assert_ne!(both.published(), Some((MemorySource::UserSettings, s)));
    }

    #[test]
    fn disagreement_needs_both_chains_and_a_real_difference() {
        let s = RawSettings { judge: 1, user_mods: 0x20 };
        let same = MemoryProbe { user_settings: Some(s), play_config: Some(s) };
        assert_eq!(same.disagreement(), None, "两条链一致时没有告警");

        let judge_differs = MemoryProbe {
            user_settings: Some(s),
            play_config: Some(RawSettings { judge: 2, user_mods: 0x20 }),
        };
        assert_eq!(
            judge_differs.disagreement(),
            Some((s, RawSettings { judge: 2, user_mods: 0x20 }))
        );

        let mods_differ = MemoryProbe {
            user_settings: Some(s),
            play_config: Some(RawSettings { judge: 1, user_mods: 0x100 }),
        };
        assert!(mods_differ.disagreement().is_some());

        // 只有一条链时谈不上"不一致"（发布的是 `P`，但缺了另一条就没有可比对象）
        assert_eq!(MemoryProbe { user_settings: Some(s), play_config: None }.disagreement(), None);
        assert_eq!(MemoryProbe { user_settings: None, play_config: Some(s) }.disagreement(), None);
        assert_eq!(MemoryProbe::default().disagreement(), None);
    }
}

// ===========================================================================
// Win32 层（Step 3 追加）：目标进程/模块枚举 + 锚点两段读取。
//
// 本层仍只依赖 std——Step 8 的诊断工具用 `#[path]` 引入本文件、以裸 rustc 编译；
// 非 Windows 平台由下方的 `#[cfg(not(windows))]` 桩提供同名同签名 API，使 crate 在
// Linux 上照常编译（本源在那里优雅不可用：`PlatformUnsupported`，不是"找不到进程"）。
// ===========================================================================

use std::path::PathBuf;

/// 目标进程名（不区分大小写比较）。
#[cfg(windows)]
const MALODY_EXE: &str = "malody.exe";

/// Win32 句柄（`HANDLE = isize`）。
#[cfg(windows)]
type Handle = isize;

/// `OpenProcess` / `CreateToolhelp32Snapshot` 的失败句柄。
#[cfg(windows)]
const INVALID_HANDLE_VALUE: Handle = -1isize;

/// `OpenProcess` 权限：查映像路径 + 读内存（同用户下无需提权）。
#[cfg(windows)]
const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
#[cfg(windows)]
const PROCESS_VM_READ: u32 = 0x0010;
/// 模块快照标志：壳是 64 位、游戏是 32 位，**两个标志都必需**才能看到 32 位模块。
#[cfg(windows)]
const TH32CS_SNAPMODULE: u32 = 0x8;
#[cfg(windows)]
const TH32CS_SNAPMODULE32: u32 = 0x10;
/// 进程快照标志。
#[cfg(windows)]
const TH32CS_SNAPPROCESS: u32 = 0x2;
/// 宽字符缓冲长度（MAX_PATH）。
#[cfg(windows)]
const WIDE_PATH: usize = 260;

/// `MODULEENTRY32W`：**x64 调用方布局**（align 8，`size_of == 1080`）。
///
/// 字段名就是 Win32 的（`dwSize` / `modBaseAddr` / `szModule` / `szExePath`），
/// 必须 `allow(non_snake_case)`：该 lint 不受 `-A dead_code` 影响。
#[cfg(windows)]
#[repr(C)]
#[allow(non_snake_case)]
struct MODULEENTRY32W {
    dwSize: u32,           // @0
    th32ModuleID: u32,     // @4
    th32ProcessID: u32,    // @8
    GlblcntUsage: u32,     // @12
    ProccntUsage: u32,     // @16
    modBaseAddr: *mut u8,  // @24（16 之后 4 字节填充）
    modBaseSize: u32,      // @32：**SizeOfImage**，绝不是磁盘文件大小
    hModule: *mut u8,      // @40
    szModule: [u16; 256],  // @48
    szExePath: [u16; 260], // @560 ⇒ size_of 1080
}

/// `PROCESSENTRY32W`（MSVC x64 布局，align 8）。
///
/// `szExeFile` 结束于 564，`size_of` 因尾部 ABI 补白为 **568**——`dwSize` 必须传 568。
#[cfg(windows)]
#[repr(C)]
#[allow(non_snake_case)]
struct PROCESSENTRY32W {
    dwSize: u32,               // @0
    cntUsage: u32,             // @4
    th32ProcessID: u32,        // @8
    th32DefaultHeapID: usize,  // @16（8 之后 4 字节填充）
    th32ModuleID: u32,         // @24
    cntThreads: u32,           // @28
    th32ParentProcessID: u32,  // @32
    pcPriClassBase: i32,       // @36
    dwFlags: u32,              // @40
    szExeFile: [u16; 260],     // @44 ⇒ 结束于 564
}

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn OpenProcess(dwDesiredAccess: u32, bInheritHandle: i32, dwProcessId: u32) -> Handle;
    fn CloseHandle(hObject: Handle) -> i32;
    fn ReadProcessMemory(
        hProcess: Handle,
        lpBaseAddress: *const u8,
        lpBuffer: *mut u8,
        nSize: usize,
        lpNumberOfBytesRead: *mut usize,
    ) -> i32;
    fn CreateToolhelp32Snapshot(dwFlags: u32, th32ProcessID: u32) -> Handle;
    fn Module32FirstW(hSnapshot: Handle, lpme: *mut MODULEENTRY32W) -> i32;
    fn Module32NextW(hSnapshot: Handle, lpme: *mut MODULEENTRY32W) -> i32;
    fn Process32FirstW(hSnapshot: Handle, lppe: *mut PROCESSENTRY32W) -> i32;
    fn Process32NextW(hSnapshot: Handle, lppe: *mut PROCESSENTRY32W) -> i32;
    /// `hModule = NULL` 时返回**当前进程**的映像路径；跨进程要用下面的
    /// `QueryFullProcessImageNameW`（`GetModuleFileNameW` 只对已加载进本进程的模块有效）。
    fn GetModuleFileNameW(hModule: *mut u8, lpFilename: *mut u16, nSize: u32) -> u32;
    /// 跨进程映像路径（需 `PROCESS_QUERY_LIMITED_INFORMATION`）。
    fn QueryFullProcessImageNameW(
        hProcess: Handle,
        dwFlags: u32,
        lpExeName: *mut u16,
        lpdwSize: *mut u32,
    ) -> i32;
}

/// 定长宽字符字段 → `String`（首个 NUL 截止）。
#[cfg(windows)]
fn wide_to_string(field: &[u16]) -> String {
    let end = field.iter().position(|&c| c == 0).unwrap_or(field.len());
    String::from_utf16_lossy(&field[..end])
}

/// 路径比较：整体、不区分大小写（Windows 路径语义）。
#[cfg(windows)]
fn same_path_ignore_case(a: &Path, b: &Path) -> bool {
    a.to_string_lossy().eq_ignore_ascii_case(&b.to_string_lossy())
}

/// 目标进程的映像路径（跨进程）。
///
/// 主路径是 `QueryFullProcessImageNameW`（进程句柄 + `PROCESS_QUERY_LIMITED_INFORMATION`）；
/// `GetModuleFileNameW` 只能取调用进程自己的映像路径，故仅当目标就是本进程时才用它兜底。
#[cfg(windows)]
fn live_image_path(handle: Handle, pid: u32) -> Option<PathBuf> {
    let mut buf = [0u16; WIDE_PATH];
    let mut len = buf.len() as u32;
    if unsafe { QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut len) } != 0 && len > 0 {
        return Some(PathBuf::from(String::from_utf16_lossy(&buf[..len as usize])));
    }
    if pid != std::process::id() {
        return None;
    }
    let got = unsafe { GetModuleFileNameW(std::ptr::null_mut(), buf.as_mut_ptr(), buf.len() as u32) };
    if got == 0 {
        return None;
    }
    Some(PathBuf::from(String::from_utf16_lossy(&buf[..got as usize])))
}

/// 读目标进程内存，返回实际读到的字节数；调用失败或零字节 → `BadRead`。
#[cfg(windows)]
fn read_bytes(handle: Handle, addr: u64, buf: &mut [u8]) -> Result<usize, AnchorError> {
    let mut got: usize = 0;
    let ok = unsafe {
        ReadProcessMemory(
            handle,
            addr as *const u8,
            buf.as_mut_ptr(),
            buf.len(),
            &mut got,
        )
    };
    if ok == 0 || got == 0 {
        return Err(AnchorError::BadRead);
    }
    Ok(got)
}

/// 枚举进程快照，返回唯一的 `malody.exe` PID。
///
/// 0 个 → `NotFound`；≥2 个 → `MultipleInstances`（绝不猜该跟哪一个实例）。
#[cfg(windows)]
fn find_single_pid() -> Result<u32, AnchorError> {
    let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snap == INVALID_HANDLE_VALUE || snap == 0 {
        return Err(AnchorError::BadRead);
    }
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
    let mut first: Option<u32> = None;
    let mut hits = 0u32;
    let mut more = unsafe { Process32FirstW(snap, &mut entry) } != 0;
    while more {
        if wide_to_string(&entry.szExeFile).eq_ignore_ascii_case(MALODY_EXE) {
            hits += 1;
            first = first.or(Some(entry.th32ProcessID));
        }
        more = unsafe { Process32NextW(snap, &mut entry) } != 0;
    }
    unsafe { CloseHandle(snap) };
    match (first, hits) {
        (Some(pid), 1) => Ok(pid),
        (None, _) => Err(AnchorError::NotFound),
        _ => Err(AnchorError::MultipleInstances),
    }
}

/// 枚举目标进程模块，返回 `malody.exe` 的 32 位基址与磁盘映像路径。
#[cfg(windows)]
fn find_module(pid: u32, handle: Handle) -> Result<(u32, PathBuf), AnchorError> {
    let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid) };
    if snap == INVALID_HANDLE_VALUE || snap == 0 {
        return Err(AnchorError::BadRead);
    }
    let mut entry: MODULEENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of::<MODULEENTRY32W>() as u32;
    let mut hit: Option<Result<(u32, PathBuf), AnchorError>> = None;
    let mut more = unsafe { Module32FirstW(snap, &mut entry) } != 0;
    while more {
        if wide_to_string(&entry.szModule).eq_ignore_ascii_case(MALODY_EXE) {
            let base_u32 = entry.modBaseAddr as usize as u32;
            let image = wide_to_string(&entry.szExePath);
            hit = Some(if image.is_empty() {
                live_image_path(handle, pid)
                    .map(|path| (base_u32, path))
                    .ok_or(AnchorError::BadRead)
            } else {
                Ok((base_u32, PathBuf::from(image)))
            });
            break;
        }
        more = unsafe { Module32NextW(snap, &mut entry) } != 0;
    }
    unsafe { CloseHandle(snap) };
    hit.unwrap_or(Err(AnchorError::BadRead))
}

/// 目标进程（`find_target()` 的产物）。句柄由本结构体独占，`Drop` 时关闭。
#[cfg(windows)]
pub struct Target {
    handle: Handle,
    pub pid: u32,
    pub base_u32: u32,
    pub exe_path: PathBuf,
    pub file_size: u64,
}

#[cfg(windows)]
impl Drop for Target {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.handle) };
    }
}

/// 已 attach 的目标进程（`open()` 的产物）。句柄由本结构体独占，`Drop` 时关闭。
#[cfg(windows)]
pub struct Attachment {
    handle: Handle,
    pub pid: u32,
    pub base_u32: u32,
    pub exe_path: PathBuf,
    pub file_size: u64,
}

#[cfg(windows)]
impl Drop for Attachment {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.handle) };
    }
}

/// 找到唯一运行中的 `malody.exe`，取其基址、映像路径与磁盘文件大小。
///
/// 本函数**不做版本校验**：版本门在 `open()`，这样"版本不符"才会以
/// `TargetMismatch(...)` 现形，而不是被伪装成"找不到进程"。
#[cfg(windows)]
pub fn find_target() -> Result<Target, AnchorError> {
    let pid = find_single_pid()?;
    let handle = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ,
            0,
            pid,
        )
    };
    if handle == 0 || handle == INVALID_HANDLE_VALUE {
        return Err(AnchorError::AccessDenied);
    }
    let (base_u32, exe_path) = match find_module(pid, handle) {
        Ok(found) => found,
        Err(e) => {
            unsafe { CloseHandle(handle) };
            return Err(e);
        }
    };
    // 磁盘文件大小是版本门的一半，来源只有 `fs::metadata`——**绝不**用
    // `MODULEENTRY32W.modBaseSize`（那是 SizeOfImage，本机实测 0x1094000，恒不等于 4,750,848）。
    let file_size = match std::fs::metadata(&exe_path) {
        Ok(meta) => meta.len(),
        Err(_) => {
            unsafe { CloseHandle(handle) };
            return Err(AnchorError::BadRead);
        }
    };
    Ok(Target {
        handle,
        pid,
        base_u32,
        exe_path,
        file_size,
    })
}

/// 交叉校验活体映像路径 + 磁盘 PE 版本（`TargetMismatch(...)` 原样上抛）。
#[cfg(windows)]
fn verify_target(handle: Handle, target: &Target) -> Result<(), AnchorError> {
    let live = live_image_path(handle, target.pid).ok_or(AnchorError::BadRead)?;
    if !same_path_ignore_case(&live, &target.exe_path) {
        return Err(AnchorError::BadRead);
    }
    validate_pe_file(&target.exe_path, ClientSpec::current())
}

/// 打开目标进程（自己的句柄，不共用 `Target` 的），校验通过后返回 `Attachment`。
#[cfg(windows)]
pub fn open(target: &Target) -> Result<Attachment, AnchorError> {
    let handle = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ,
            0,
            target.pid,
        )
    };
    if handle == 0 || handle == INVALID_HANDLE_VALUE {
        return Err(AnchorError::AccessDenied);
    }
    if let Err(e) = verify_target(handle, target) {
        unsafe { CloseHandle(handle) };
        return Err(e);
    }
    Ok(Attachment {
        handle,
        pid: target.pid,
        base_u32: target.base_u32,
        exe_path: target.exe_path.clone(),
        file_size: target.file_size,
    })
}

/// 锚点一次读取的分类结果（`read_identity_classified` 的产物）。
///
/// 判"硬失败 / 软失败"的唯一分界是**锚点指针槽位本身**是否读到：指针槽位（`base + rva`）
/// 读成功 ⇒ 进程与模块仍在，此后任何不顺（字符串缓冲读不出来 / 文法不成立）都是**软失败**
/// ——游戏过场瞬间该地址可能指向别的内容。进程真的退出时第一段读必然失败，故不会漏判。
#[derive(Debug, Clone, PartialEq)]
pub enum IdentityRead {
    /// 指针为 0：当前没有选中谱面（正常态，不是失败）。
    Empty,
    /// 解析出身份键。
    Key(IdentityKey),
    /// 指针非 0，但目标缓冲当前不是身份键（软失败）。
    Unparsable,
}

/// 读锚点身份键并**区分软失败**：`Err` 一律是硬失败（指针槽位读不到 / 平台不支持）。
///
/// 读法与 `read_identity` 逐字相同（指针 4 字节必须全读到、字符串缓冲预置零 128 字节、
/// 严格文法解析），只多出"软失败"这一档供调用方按连续次数容忍。
#[cfg(windows)]
pub fn read_identity_classified(at: &Attachment) -> Result<IdentityRead, AnchorError> {
    let addr = at.base_u32 as u64 + ClientSpec::current().rva as u64;
    let mut ptr_buf = [0u8; 4];
    // 指针必须 4 字节全读到：短读的指针是垃圾，且此处失败即"进程 / 模块 / 句柄已不在"
    if read_bytes(at.handle, addr, &mut ptr_buf)? != ptr_buf.len() {
        return Err(AnchorError::BadRead);
    }
    let ptr = u32::from_le_bytes(ptr_buf);
    if ptr == 0 {
        return Ok(IdentityRead::Empty);
    }
    // 字符串缓冲预置零：短读时尾部保持零（= NUL 终止），但文法仍必须成立
    let mut buf = [0u8; 128];
    if read_bytes(at.handle, u64::from(ptr), &mut buf).is_err() {
        // 指针槽位已读成功 ⇒ 进程与模块仍在：陈旧 / 半写的指针只是软失败
        return Ok(IdentityRead::Unparsable);
    }
    Ok(match parse_identity_key(&buf) {
        Some(key) => IdentityRead::Key(key),
        None => IdentityRead::Unparsable,
    })
}

/// 读锚点身份键：`base + rva` 处 4 字节指针 → 该地址 128 字节字符串 → 严格文法解析。
///
/// 指针为 0 → `Ok(None)`（当前没选中谱面）；文法不成立 → `BadRead`——
/// **绝不**用文法失败的内容凑出身份键。
#[cfg(windows)]
pub fn read_identity(at: &Attachment) -> Result<Option<IdentityKey>, AnchorError> {
    match read_identity_classified(at)? {
        IdentityRead::Empty => Ok(None),
        IdentityRead::Key(key) => Ok(Some(key)),
        // 软失败按原语义对上层呈现为 BadRead（poller 用分类版本区分硬/软）
        IdentityRead::Unparsable => Err(AnchorError::BadRead),
    }
}

/// 读一条设置链：指针槽位（`base + rva`）→ 指针 → 对象块 → 纯解码。
///
/// 除了"指针槽位本身读失败"之外，**一切不顺都是 `Ok(None)`**（现在不可确定，不是错误、
/// 不 latch、调用方回落 `config.json`）：
/// - 指针为 0（`P` 在首个场景之前就是 0）；
/// - 指针不可信（不对齐 / 不在 32 位用户地址空间内）；
/// - 对象块读不出来或**只读到一部分**；
/// - 解码不过（判定档越界 / `P` 的子集不变量不成立）。
#[cfg(windows)]
fn read_chain(
    handle: Handle,
    base_u32: u32,
    rva: u32,
    decode: fn(&[u8]) -> Option<RawSettings>,
) -> Result<Option<RawSettings>, AnchorError> {
    let slot = base_u32 as u64 + u64::from(rva);
    let mut ptr_buf = [0u8; 4];
    // 指针必须 4 字节全读到：短读的指针是垃圾；此处失败即"进程 / 模块 / 句柄已不在"
    if read_bytes(handle, slot, &mut ptr_buf)? != ptr_buf.len() {
        return Err(AnchorError::BadRead);
    }
    let ptr = u32::from_le_bytes(ptr_buf);
    if !plausible_pointer(ptr) {
        return Ok(None);
    }
    let mut block = [0u8; SETTINGS_BLOCK_LEN];
    match read_bytes(handle, u64::from(ptr), &mut block) {
        // **必须整块读到**：短读留下的零会把"没读到的字段"伪装成 0（判定 A、无 mod），
        // 那正是本特性最不该发生的错误 —— 宁可不发布，也不发布一个错的。
        Ok(got) if got == block.len() => Ok(decode(&block)),
        // 指针槽位已读成功 ⇒ 进程与模块仍在：陈旧 / 半写的指针只是"现在不可确定"
        _ => Ok(None),
    }
}

/// 采样设置单例：`P`（发布源）+ `S`（兜底与旁证）。
///
/// **每 tick 都重新读指针、绝不缓存**：两个对象都在惰性单例后面，拆解路径会把指针清零；
/// 缓存指针会把"上一局的设置"当成现役值。
///
/// `P` 的指针槽位读失败 → `Err`（进程 / 模块已不在）；`S` 只是兜底与旁证，整条链读失败不影响
/// 发布 `P`（用 `.ok().flatten()` 吞掉）。两个槽位（`0x8311C4` / `0x8310C4`）在同一模块映像的
/// 同一页里，实际不会一个读得到一个读不到——这里只是让"发布链必须可读"这条口径成立。
#[cfg(windows)]
pub fn read_settings(at: &Attachment) -> Result<MemoryProbe, AnchorError> {
    let play_config = read_chain(
        at.handle,
        at.base_u32,
        PLAY_CONFIG_RVA,
        decode_play_config,
    )?;
    let user_settings = read_chain(
        at.handle,
        at.base_u32,
        USER_SETTINGS_RVA,
        decode_user_settings,
    )
    .ok()
    .flatten();
    Ok(MemoryProbe {
        user_settings,
        play_config,
    })
}

// --------------------------------------------------- 非 Windows（同名桩） --

/// 目标进程（非 Windows 桩：可构造、字段同签名，本源恒不可用）。
#[cfg(not(windows))]
pub struct Target {
    pub pid: u32,
    pub base_u32: u32,
    pub exe_path: PathBuf,
    pub file_size: u64,
}

/// 已 attach 的目标进程（非 Windows 桩）。
#[cfg(not(windows))]
pub struct Attachment {
    pub pid: u32,
    pub base_u32: u32,
    pub exe_path: PathBuf,
    pub file_size: u64,
}

/// 非 Windows：该数据源优雅不可用（诊断文案是"平台不支持"，不是"找不到进程"）。
#[cfg(not(windows))]
pub fn find_target() -> Result<Target, AnchorError> {
    Err(AnchorError::PlatformUnsupported)
}

#[cfg(not(windows))]
pub fn open(_target: &Target) -> Result<Attachment, AnchorError> {
    Err(AnchorError::PlatformUnsupported)
}

#[cfg(not(windows))]
pub fn read_identity(_at: &Attachment) -> Result<Option<IdentityKey>, AnchorError> {
    Err(AnchorError::PlatformUnsupported)
}

#[cfg(not(windows))]
pub fn read_identity_classified(_at: &Attachment) -> Result<IdentityRead, AnchorError> {
    Err(AnchorError::PlatformUnsupported)
}

/// 非 Windows：内存通道整条不可用（本源本身也 `PlatformUnsupported`）。
#[cfg(not(windows))]
pub fn read_settings(_at: &Attachment) -> Result<MemoryProbe, AnchorError> {
    Err(AnchorError::PlatformUnsupported)
}

/// Win32 层测试（布局只在 Windows 调用方下有效，故整体 cfg 门控）。
#[cfg(all(test, windows))]
mod win32_tests {
    use super::*;

    #[test]
    fn module_entry32w_layout_matches_x64_caller() {
        use std::mem::{align_of, offset_of, size_of};
        // dwSize 必须传 size_of（1080），否则 Module32FirstW 直接失败
        assert_eq!(size_of::<MODULEENTRY32W>(), 1080);
        assert_eq!(align_of::<MODULEENTRY32W>(), 8);
        assert_eq!(offset_of!(MODULEENTRY32W, dwSize), 0);
        assert_eq!(offset_of!(MODULEENTRY32W, th32ModuleID), 4);
        assert_eq!(offset_of!(MODULEENTRY32W, th32ProcessID), 8);
        assert_eq!(offset_of!(MODULEENTRY32W, GlblcntUsage), 12);
        assert_eq!(offset_of!(MODULEENTRY32W, ProccntUsage), 16);
        assert_eq!(offset_of!(MODULEENTRY32W, modBaseAddr), 24);
        assert_eq!(offset_of!(MODULEENTRY32W, modBaseSize), 32);
        assert_eq!(offset_of!(MODULEENTRY32W, hModule), 40);
        assert_eq!(offset_of!(MODULEENTRY32W, szModule), 48);
        assert_eq!(offset_of!(MODULEENTRY32W, szExePath), 560);
    }

    #[test]
    fn process_entry32w_layout_matches_x64_caller() {
        use std::mem::{align_of, offset_of, size_of};
        // szExeFile 结束于 44 + 260 * 2 = 564，其后 4 字节 ABI 补白 ⇒ size_of 568。
        // `dwSize` 必须传 568（MSVC 的 sizeof(PROCESSENTRY32W) 同样是 568），传 564 会被拒。
        assert_eq!(offset_of!(PROCESSENTRY32W, szExeFile) + 260 * 2, 564);
        assert_eq!(size_of::<PROCESSENTRY32W>(), 568);
        assert_eq!(align_of::<PROCESSENTRY32W>(), 8);
        assert_eq!(offset_of!(PROCESSENTRY32W, dwSize), 0);
        assert_eq!(offset_of!(PROCESSENTRY32W, cntUsage), 4);
        assert_eq!(offset_of!(PROCESSENTRY32W, th32ProcessID), 8);
        assert_eq!(offset_of!(PROCESSENTRY32W, th32DefaultHeapID), 16);
        assert_eq!(offset_of!(PROCESSENTRY32W, th32ModuleID), 24);
        assert_eq!(offset_of!(PROCESSENTRY32W, cntThreads), 28);
        assert_eq!(offset_of!(PROCESSENTRY32W, th32ParentProcessID), 32);
        assert_eq!(offset_of!(PROCESSENTRY32W, pcPriClassBase), 36);
        assert_eq!(offset_of!(PROCESSENTRY32W, dwFlags), 40);
        assert_eq!(offset_of!(PROCESSENTRY32W, szExeFile), 44);
    }

    #[test]
    fn wide_to_string_stops_at_nul() {
        let mut field = [0u16; 260];
        for (i, c) in "malody.exe".encode_utf16().enumerate() {
            field[i] = c;
        }
        assert!(wide_to_string(&field).eq_ignore_ascii_case(MALODY_EXE));
        field[0] = u16::from(b'M');
        assert_eq!(wide_to_string(&field), "Malody.exe");
        assert!(wide_to_string(&[0u16; 260]).is_empty());
    }

    #[test]
    fn same_path_ignore_case_requires_full_match() {
        assert!(same_path_ignore_case(
            Path::new(r"C:\tmp\a\malody.exe"),
            Path::new(r"c:\TMP\A\MALODY.EXE")
        ));
        assert!(!same_path_ignore_case(
            Path::new(r"C:\tmp\a\malody.exe"),
            Path::new(r"C:\tmp\b\malody.exe")
        ));
    }

    #[test]
    fn read_bytes_on_null_handle_is_bad_read() {
        // 空句柄：ReadProcessMemory 必然失败 → BadRead（不 panic、不把垃圾当数据）
        assert_eq!(read_bytes(0, 0, &mut [0u8; 4]), Err(AnchorError::BadRead));
        assert_eq!(read_bytes(0, 0, &mut [0u8; 128]), Err(AnchorError::BadRead));
    }

    /// §7：无 `malody.exe` 的环境下 `find_target()` 返回 `Err(NotFound)` 而非 panic。
    /// 本机可能真的开着游戏，故先问进程快照，再断言该分支下的对应结果。
    #[cfg(windows)]
    #[test]
    fn find_target_without_a_running_game_is_not_found_instead_of_panicking() {
        match find_single_pid() {
            Err(AnchorError::NotFound) => {
                assert_eq!(find_target().err(), Some(AnchorError::NotFound));
            }
            Err(AnchorError::MultipleInstances) => {
                assert_eq!(find_target().err(), Some(AnchorError::MultipleInstances));
            }
            Ok(pid) => {
                // 游戏在跑：`Ok` / `AccessDenied` 都是合法结果，本用例只断言"不 panic"。
                eprintln!("malody.exe 正在运行 (pid={pid})，跳过 NotFound 断言");
                let _ = find_target();
            }
            Err(other) => panic!("进程快照不应产生 {other:?}"),
        }
    }
}
