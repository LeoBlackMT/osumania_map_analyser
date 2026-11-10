// osr.rs —— lazer `.osr` 尾部的 `scoreInfo` 读取（只取 `pauses` 与 mods 倍速）。
//
// 格式（lazer `LegacyScoreEncoder`，version >= 30000000）：
//   byte   ruleset
//   int32  version
//   string beatmapMd5            // 0x0B | ULEB128(byte len) | UTF-8
//   string playerName
//   string replayMd5             // 32-hex，**不是** ScoreInfo.Hash
//   u16 × 6 判定计数
//   int32  totalScore
//   u16    maxCombo
//   u8     perfect
//   int32  mods
//   string hpGraph
//   int64  dateTicks
//   int32  replayDataLength
//   byte[] replayData            // 只按长度跳过，不解析
//   int64  legacyOnlineId
//   int32  scoreInfoLength
//   byte[] scoreInfo             // [5B props][8B 原始长度 LE][LZMA 流]
//
// 全程只读、纯内存；不写盘、不依赖游戏进程。

use std::io::Cursor;

use serde_json::Value;

/// `scoreInfo` 只存在于这个版本之后。
const MIN_VERSION_WITH_SCORE_INFO: i32 = 30_000_000;
/// `.osr` 文件上限（壳侧也有一道；这里再兜一层）。
pub const MAX_OSR_BYTES: usize = 64 * 1024 * 1024;
/// `scoreInfo` 压缩段上限。
const MAX_SCORE_INFO_BYTES: usize = 1024 * 1024;
/// `scoreInfo` 解压后上限（防解压炸弹）。
const MAX_DECOMPRESSED_BYTES: i64 = 8 * 1024 * 1024;

/// 一次 `.osr` 读取的结果。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReplayPauses {
    /// 暂停开始时刻，**物理谱面时间**（`.osu` 毫秒轴）；保留负值与越界值，由页面过滤。
    pub pauses: Vec<i32>,
    /// `true` = 已完整读到 `pauses` 数组（空数组合法）；`false` = 旧版/缺字段/读不到。
    pub available: bool,
    /// `.osr` 里 mods 推导出的游戏倍速；`None` = mods 结构异常。
    pub speed_rate: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OsrError {
    TooLarge,
    Truncated(&'static str),
    BadString,
    BadLzma,
    BadJson,
}

/// 读一个 `.osr` 的暂停数据。
///
/// 语义：
/// - `version < 30_000_000` / 缺 `scoreInfo` / 缺 `pauses` / 类型不对 ⇒ `Ok(available=false)`；
/// - 结构性损坏（截断、坏字符串、坏 LZMA、坏 JSON）⇒ `Err`（调用方同样降级为不可用）。
pub fn read_replay_pauses(bytes: &[u8]) -> Result<ReplayPauses, OsrError> {
    if bytes.len() > MAX_OSR_BYTES {
        return Err(OsrError::TooLarge);
    }
    let mut reader = Reader::new(bytes);
    let _ruleset = reader.u8()?;
    let version = reader.i32()?;
    let _beatmap_md5 = reader.string()?;
    let _player_name = reader.string()?;
    let _replay_md5 = reader.string()?;
    for _ in 0..6 {
        reader.u16()?;
    }
    let _total_score = reader.i32()?;
    let _max_combo = reader.u16()?;
    let _perfect = reader.u8()?;
    let _mods = reader.i32()?;
    let _hp_graph = reader.string()?;
    let _date_ticks = reader.i64()?;
    let replay_length = reader.i32()?;
    if replay_length < 0 {
        return Err(OsrError::Truncated("replayDataLength"));
    }
    reader.take(replay_length as usize)?;

    if version < MIN_VERSION_WITH_SCORE_INFO {
        return Ok(ReplayPauses::default());
    }

    let _legacy_online_id = reader.i64()?;
    let info_length = reader.i32()?;
    if info_length <= 0 {
        return Err(OsrError::Truncated("scoreInfoLength"));
    }
    if info_length as usize > MAX_SCORE_INFO_BYTES {
        return Err(OsrError::TooLarge);
    }
    let info = reader.take(info_length as usize)?;

    // 13 字节头：5 字节 LZMA properties + 8 字节解压后长度（小端 i64）。
    // `-1` = 流式写入的"未知长度"（lzma-rs 的默认选项），此时靠 `LimitedBuffer` 兜底。
    if info.len() < 13 {
        return Err(OsrError::Truncated("scoreInfoHeader"));
    }
    let raw_length = i64::from_le_bytes(info[5..13].try_into().expect("8 bytes"));
    if raw_length < -1 || raw_length > MAX_DECOMPRESSED_BYTES {
        return Err(OsrError::TooLarge);
    }

    let capacity = if raw_length > 0 {
        raw_length as usize
    } else {
        0
    };
    let mut buffer = LimitedBuffer::new(capacity, MAX_DECOMPRESSED_BYTES as usize);
    match lzma_rs::lzma_decompress(&mut Cursor::new(info), &mut buffer) {
        Ok(()) => {}
        Err(_) if buffer.limit_hit => return Err(OsrError::TooLarge),
        Err(_) => return Err(OsrError::BadLzma),
    }
    let json = buffer.data;
    let value: Value = serde_json::from_slice(&json).map_err(|_| OsrError::BadJson)?;

    let Some(pauses_value) = value.get("pauses") else {
        return Ok(ReplayPauses::default());
    };
    let Some(array) = pauses_value.as_array() else {
        return Ok(ReplayPauses::default());
    };
    let mut pauses = Vec::with_capacity(array.len());
    for item in array {
        let Some(number) = item.as_i64() else {
            return Ok(ReplayPauses::default());
        };
        if number < i32::MIN as i64 || number > i32::MAX as i64 {
            return Ok(ReplayPauses::default());
        }
        pauses.push(number as i32);
    }

    Ok(ReplayPauses {
        pauses,
        available: true,
        speed_rate: speed_rate_from_mods(value.get("mods")),
    })
}

/// `mods` → 游戏倍速。
///
/// 优先 `settings.speed_change`（自定义倍速），否则按缩写默认值（`DT/NC=1.5`、`HT/DC=0.75`）。
/// 无 `mods` 键或空数组 ⇒ `Some(1.0)`；`mods` 不是数组 ⇒ `None`（结构异常）。
fn speed_rate_from_mods(mods: Option<&Value>) -> Option<f64> {
    let Some(mods) = mods else {
        return Some(1.0);
    };
    let Some(array) = mods.as_array() else {
        return None;
    };
    let mut rate = 1.0_f64;
    let mut found = false;
    for entry in array {
        let acronym = entry
            .get("acronym")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let default = match acronym {
            "DT" | "NC" => Some(1.5_f64),
            "HT" | "DC" => Some(0.75_f64),
            _ => None,
        };
        let Some(default) = default else {
            continue;
        };
        found = true;
        rate = entry
            .get("settings")
            .and_then(|settings| settings.get("speed_change"))
            .and_then(Value::as_f64)
            .filter(|value| value.is_finite() && *value > 0.0)
            .unwrap_or(default);
    }
    Some(if found { rate } else { 1.0 })
}

/// 带上限的写缓冲：`lzma_decompress` 的输出不能超过 `limit`（防解压炸弹）。
struct LimitedBuffer {
    data: Vec<u8>,
    limit: usize,
    limit_hit: bool,
}

impl LimitedBuffer {
    fn new(capacity: usize, limit: usize) -> Self {
        Self {
            data: Vec::with_capacity(capacity.min(limit)),
            limit,
            limit_hit: false,
        }
    }
}

impl std::io::Write for LimitedBuffer {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.data.len().saturating_add(buf.len()) > self.limit {
            self.limit_hit = true;
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "decompressed size limit",
            ));
        }
        self.data.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// 顺序读游标；所有长度先做边界检查，绝不 panic。
struct Reader<'a> {    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], OsrError> {
        let end = self
            .position
            .checked_add(length)
            .ok_or(OsrError::Truncated("length-overflow"))?;
        if end > self.bytes.len() {
            return Err(OsrError::Truncated("payload"));
        }
        let slice = &self.bytes[self.position..end];
        self.position = end;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, OsrError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, OsrError> {
        let bytes = self.take(2)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    fn i32(&mut self) -> Result<i32, OsrError> {
        let bytes = self.take(4)?;
        Ok(i32::from_le_bytes(bytes.try_into().expect("4 bytes")))
    }

    fn i64(&mut self) -> Result<i64, OsrError> {
        let bytes = self.take(8)?;
        Ok(i64::from_le_bytes(bytes.try_into().expect("8 bytes")))
    }

    /// `0x0B | ULEB128(byte length) | UTF-8`。
    fn string(&mut self) -> Result<String, OsrError> {
        if self.u8()? != 0x0B {
            return Err(OsrError::BadString);
        }
        let mut length: u64 = 0;
        let mut shift = 0;
        loop {
            let byte = self.u8()?;
            length |= u64::from(byte & 0x7F) << shift;
            if byte & 0x80 == 0 {
                break;
            }
            shift += 7;
            if shift > 63 {
                return Err(OsrError::BadString);
            }
        }
        let bytes = self.take(length as usize)?;
        String::from_utf8(bytes.to_vec()).map_err(|_| OsrError::BadString)
    }
}

#[cfg(test)]
#[path = "../../../tests-local/osu_lazer_osr.rs"]
mod tests_osr;
