// osu! 原生源的模型层：进程/快照结构与 reason 闭集。
//
// 本步（B2）只发布 §3.3 的两枚字段（`state.name` + `beatmap.md5`）以及 B2 对拍
// 需要的最小附带字段；完整字段表在 C2。结构体字段全部是"读到了就是读到"的原始
// 值，**降级只有两种表达**：
// - 字符串类：`Option<String>`（`None` = 读失败，同时进 `degraded_fields`）
// - 数值类：`Option<i32>` / `Option<u32>`（同上）
// 「整块读满才算数」的 fail-closed 规则在 `win.rs::read_exact_at` 里：短读一律当失败。

use serde_json::{json, Value};

/// 壳侧 reason 闭集（唯一权威）。形状照 `malody4/model.rs`：enum + `as_str()` 返回
/// `String`，因为部分分支带参数（`signature-miss:<key>` / `invariant-failed:<field>`）。
///
/// 本步只实现真正会用到的那一子集；其余字面量（`unsupported-build:<hash>`、
/// `lazer-offsets-missing:<ver>`、`platform-unsupported`、`shadow-mismatch:<field>`）
/// 属 D/E 步，不预先声明（`CLAUDE.md` §2：不做投机抽象）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reason {
    ProcessNotFound,
    MultipleInstances,
    ClientAmbiguous,
    /// 锚点扫描没命中：`signature-miss:<key>`（key = `patterns::Anchor::key`）。
    SignatureMiss(&'static str),
    AccessDenied,
    ReadError,
    /// L1/L2 校验不过：`invariant-failed:<field>`。
    InvariantFailed(&'static str),
}

impl Reason {
    pub fn as_str(&self) -> String {
        match self {
            Reason::ProcessNotFound => "process-not-found".to_string(),
            Reason::MultipleInstances => "multiple-instances".to_string(),
            Reason::ClientAmbiguous => "client-ambiguous".to_string(),
            Reason::SignatureMiss(key) => format!("signature-miss:{key}"),
            Reason::AccessDenied => "access-denied".to_string(),
            Reason::ReadError => "read-error".to_string(),
            Reason::InvariantFailed(field) => format!("invariant-failed:{field}"),
        }
    }

    /// Win32 错误码 → reason（`ERROR_ACCESS_DENIED = 5`）。
    pub fn from_win32_error(code: u32) -> Reason {
        if code == 5 {
            Reason::AccessDenied
        } else {
            Reason::ReadError
        }
    }
}

/// 进程分派结果（`client` 字段的来源）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Client {
    Stable,
    Lazer,
}

impl Client {
    pub fn as_str(&self) -> &'static str {
        match self {
            Client::Stable => "stable",
            Client::Lazer => "lazer",
        }
    }
}

/// `GameState` 名的**观测集**（唯一权威：P8 的 `(state.number, state.name)` 台账）。
///
/// 硬约束（计划 §3.3 注 + §3.4 I-01）：名字表**不得**从 tosu 源码抄写，只能来自本机实测观测；
/// 观测集外的索引一律返回 `""`（对齐 tosu `GameState[status] || ''`，`buildResultV2.ts:144`），
/// 并上报 `degraded_fields: ["state.name"]`。
///
/// 台账来源（`temp/osu-native-memory/evidence/P8/sweep-20260928b/observations.jsonl`，4025 行）：
/// 六个状态各自被采到 —— `0/menu` 2802 行、`1/edit` 54、`2/play` 601、`4/selectEdit` 5、
/// `5/selectPlay` 512、`7/resultScreen` 51；同一目录下六个状态各有一份完整 golden 帧。
/// P1 的两次 stable 采样（`play`=2 / `selectPlay`=5）另证 `statusPtr` 的整数与 `state.number` 逐位相等。
pub const OBSERVED_STATE_NAMES: &[(i32, &str)] = &[
    (0, "menu"),
    (1, "edit"),
    (2, "play"),
    (4, "selectEdit"),
    (5, "selectPlay"),
    (7, "resultScreen"),
];

/// 观测集内 → 名字；观测集外 → `""`（调用方负责加 `state.name` 降级标记）。
pub fn state_name_for(index: i32) -> &'static str {
    OBSERVED_STATE_NAMES
        .iter()
        .find(|(n, _)| *n == index)
        .map(|(_, name)| *name)
        .unwrap_or("")
}

/// 一帧原生快照（本步的字段子集）。
///
/// 命名对齐 §3.3 的载荷字段名（`state_name` ↔ `state.name`、`checksum` ↔
/// `beatmap.md5`/`beatmap.checksum`…），便于 `packet.rs` 与对拍记录直接引用。
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    /// `stable` | `lazer`。
    pub client: Option<Client>,
    pub pid: u32,
    /// `statePtr` 的原始整数（未查表）；`None` = 读失败。
    pub state_number: Option<i32>,
    /// 查表后的名字（观测集外为 `""`）；`None` = `state_number` 读失败。
    pub state_name: Option<String>,
    /// `baseAddr - 0xC` 二次解引用得到的 Beatmap 对象地址。
    pub beatmap_object: Option<u32>,
    /// `[beatmap]+0x6C`（稳定构建的 md5 字符串对象地址 → C# 字符串）。
    pub checksum: Option<String>,
    pub map_id: Option<i32>,
    pub set_id: Option<i32>,
    pub filename: Option<String>,
    pub folder: Option<String>,
    pub version: Option<String>,
    pub artist: Option<String>,
    pub title: Option<String>,
    pub mapper: Option<String>,
    /// `[baseAddr]+0x0` 的原始读数（identity 的 id 段不用它，仅证据留档）。
    pub map_id_bits: Option<u32>,
    /// 菜单 mod 位掩码（`menuModsPtr` 链；`0` = NM）。
    pub menu_mods_mask: Option<u32>,
    /// `[[[settingsClassAddr+0x8]+0xB8]+0x4]` 的 C# 字符串 = osu! cfg 的 `BeatmapDirectory`
    /// **值**（本机观测 = `"Songs"`，是**名字不是路径**；P1-notes F2）。
    pub songs_cfg_value: Option<String>,
    /// 绝对 songs 目录：`join(dirname(osu!.exe), cfg 值)` 存在则用它，否则用 cfg 值本身
    /// （镜像参照实现的规则；见 `mod.rs::resolve_songs_folder`）。
    pub songs_folder: Option<String>,
    /// 当前谱面文件的解析结果（`.osu`，C6 的两个值来源）；`None` = 未解析/解析失败。
    pub beatmap_file: Option<crate::osu::beatmap_file::BeatmapFile>,
    /// 当前解析结果对应的 `.osu` 绝对路径（诊断/证据用）。
    pub beatmap_file_path: Option<String>,
    /// 解析失败的原因（读取/编码问题；`None` = 成功或尚未尝试）。
    pub beatmap_file_error: Option<String>,
    /// `.osu` 头字段与内存值的交叉校验结果（不一致的字段名；空 = 全等或无法比较）。
    pub beatmap_file_mismatches: Vec<String>,
    /// 观测集外的状态索引（I-01 的降级上报）。
    pub degraded_fields: Vec<String>,
}

impl Snapshot {
    /// 载荷形态的 JSON（`{client, state:{number,name}, beatmap:{…}, files:{…}, …}`）。
    /// 本步只填 `baseAddr` + `statusPtr` 能给出的字段，其余留空（C2 补）。
    pub fn to_packet(&self) -> Value {
        crate::osu::packet::packet_from_snapshot(self)
    }

    /// `.osu` 解析出的 `firstObject`（C6；未解析到 ⇒ `None`）。
    pub fn first_object(&self) -> Option<i32> {
        self.beatmap_file.as_ref().map(|file| file.first_object())
    }

    /// `.osu` 解析出的 `lastObject`（C6；谱面没有对象/未解析 ⇒ `None`）。
    pub fn last_object(&self) -> Option<i32> {
        self.beatmap_file
            .as_ref()
            .and_then(|file| file.last_object())
    }

    /// 对拍记录用的紧凑形态（`identity`/`mod_signature` 由 `keys.rs` 现算）。
    pub fn to_compare_json(&self) -> Value {
        let keys = crate::osu::keys::derive(self);
        json!({
            "client": self.client.map(|c| c.as_str()),
            "state_number": self.state_number,
            "state_name": self.state_name,
            "checksum": self.checksum,
            "id": self.map_id,
            "set": self.set_id,
            "filename": self.filename,
            "folder": self.folder,
            "version": self.version,
            "mapID_bits": self.map_id_bits,
            "menu_mods_mask": self.menu_mods_mask,
            "songs_cfg_value": self.songs_cfg_value,
            "songs_folder": self.songs_folder,
            "first_object": self.first_object(),
            "last_object": self.last_object(),
            "identity": keys.identity,
            "mod_signature": keys.mod_signature,
        })
    }
}

/// `keys::derive` 的结果（页面 `socketHandlers.js` / `modData.js` 的两个字符串）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DerivedKeys {
    pub identity: String,
    pub mod_signature: String,
}
