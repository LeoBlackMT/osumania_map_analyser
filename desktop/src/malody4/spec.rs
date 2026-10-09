// malody4::spec - Malody 4.3.7 规格、已知版本表、PE 静态校验与文法解析（纯标准库依赖）

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
pub const PLAY_CONFIG_DERIVED_OFF: u32 = 0x04;
/// `P + 0x14`：本局的判定档（创建时从 `S + 0x9C` 复制）。
pub const PLAY_CONFIG_JUDGE_OFF: u32 = 0x14;
/// 一次读入的对象块长度：覆盖两条链里最靠后的字段（`S + 0xD0`）再加 4 字节。
pub const SETTINGS_BLOCK_LEN: usize = 0xD8;
/// 32 位进程用户地址空间下界：Windows 保留最低 64 KiB ⇒ 低于它的"指针"必是垃圾。
pub const USER_ADDR_MIN: u32 = 0x0001_0000;
/// 32 位用户地址空间上界（4 字节对齐的最后一个地址）。
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MemoryProbe {
    pub user_settings: Option<RawSettings>,
    pub play_config: Option<RawSettings>,
}

impl MemoryProbe {
    /// 发布取值：优先 `P`，`P` 读不到退到 `S`。
    pub fn published(&self) -> Option<(MemorySource, RawSettings)> {
        if let Some(raw) = self.play_config {
            return Some((MemorySource::PlayConfig, raw));
        }
        self.user_settings.map(|raw| (MemorySource::UserSettings, raw))
    }

    /// 两条链都读到、但判定档或 `user_mods` 不一致。
    pub fn disagreement(&self) -> Option<(RawSettings, RawSettings)> {
        let (user, play) = (self.user_settings?, self.play_config?);
        (user != play).then_some((user, play))
    }
}

pub fn read_u32_at(block: &[u8], off: u32) -> Option<u32> {
    let at = off as usize;
    let bytes = block.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

pub fn valid_judge(raw: u32) -> Option<u8> {
    let level = u8::try_from(raw).ok()?;
    (level <= MAX_JUDGE_LEVEL).then_some(level)
}

pub fn decode_user_settings(block: &[u8]) -> Option<RawSettings> {
    let judge = valid_judge(read_u32_at(block, USER_SETTINGS_JUDGE_OFF)?)?;
    let user_mods = read_u32_at(block, USER_SETTINGS_MODS_OFF)?;
    Some(RawSettings { judge, user_mods })
}

pub fn decode_play_config(block: &[u8]) -> Option<RawSettings> {
    let user_mods = read_u32_at(block, PLAY_CONFIG_MODS_OFF)?;
    let derived = read_u32_at(block, PLAY_CONFIG_DERIVED_OFF)?;
    if derived & !user_mods != 0 {
        return None;
    }
    let judge = valid_judge(read_u32_at(block, PLAY_CONFIG_JUDGE_OFF)?)?;
    Some(RawSettings { judge, user_mods })
}

pub fn plausible_pointer(ptr: u32) -> bool {
    ptr & 3 == 0 && (USER_ADDR_MIN..=USER_ADDR_MAX).contains(&ptr)
}

/// 锚点内存里的身份键：`<md5>_<slot>`。
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

pub fn validate_pe_file(path: &Path, spec: &ClientSpec) -> Result<(), AnchorError> {
    let file_size = std::fs::metadata(path).map_err(|_| AnchorError::BadRead)?.len();
    let mut head = [0u8; 2048];
    let mut file = File::open(path).map_err(|_| AnchorError::BadRead)?;
    let _ = file.read(&mut head);
    validate_pe_header(&head, file_size, spec).map_err(AnchorError::TargetMismatch)
}
