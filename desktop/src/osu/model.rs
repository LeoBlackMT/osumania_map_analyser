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
/// C2 起**整集**都在册（§3.4 的 11 个字面量）：前七条是 stable 读取路径真正会发的；
/// `unsupported-build:<hash>` 是**人工诊断标签**（不是自动门，消解与 DEC-05 的冲突——
/// 没有任何哈希白名单会让它自动触发）；`lazer-offsets-missing:<ver>` 与
/// `platform-unsupported` 属 E 步（lazer/非 Windows）；`shadow-mismatch:<field>` 是 L3
/// 影子比对的升级出口（默认只记录，配置升级后回落时用）。在册即被单测钉住字面量。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reason {
    ProcessNotFound,
    MultipleInstances,
    ClientAmbiguous,
    /// 锚点扫描没命中：`signature-miss:<key>`（key = `patterns::Anchor::key`）。
    SignatureMiss(&'static str),
    /// 构建不在已验证台账里：`unsupported-build:<hash>`（**人工诊断标签**，非自动门）。
    UnsupportedBuild(String),
    /// lazer 偏移表缺失/版本不匹配：`lazer-offsets-missing:<ver>`（E 步）。
    LazerOffsetsMissing(String),
    AccessDenied,
    /// 非 Windows 平台：`platform-unsupported`（DEC-03）。
    PlatformUnsupported,
    ReadError,
    /// L1/L2 校验不过：`invariant-failed:<field>`。
    InvariantFailed(&'static str),
    /// L3 影子比对连续不一致（升级后回落时用）：`shadow-mismatch:<field>`。
    ShadowMismatch(&'static str),
}

impl Reason {
    pub fn as_str(&self) -> String {
        match self {
            Reason::ProcessNotFound => "process-not-found".to_string(),
            Reason::MultipleInstances => "multiple-instances".to_string(),
            Reason::ClientAmbiguous => "client-ambiguous".to_string(),
            Reason::SignatureMiss(key) => format!("signature-miss:{key}"),
            Reason::UnsupportedBuild(hash) => format!("unsupported-build:{hash}"),
            Reason::LazerOffsetsMissing(version) => format!("lazer-offsets-missing:{version}"),
            Reason::AccessDenied => "access-denied".to_string(),
            Reason::PlatformUnsupported => "platform-unsupported".to_string(),
            Reason::ReadError => "read-error".to_string(),
            Reason::InvariantFailed(field) => format!("invariant-failed:{field}"),
            Reason::ShadowMismatch(field) => format!("shadow-mismatch:{field}"),
        }
    }

    /// 整集的字面量前缀（单测与文档用；**顺序即枚举顺序**）。
    pub fn all_literals() -> Vec<String> {
        vec![
            Reason::ProcessNotFound.as_str(),
            Reason::MultipleInstances.as_str(),
            Reason::ClientAmbiguous.as_str(),
            Reason::SignatureMiss("statusPtr").as_str(),
            Reason::UnsupportedBuild("0".repeat(32)).as_str(),
            Reason::LazerOffsetsMissing("0".to_string()).as_str(),
            Reason::AccessDenied.as_str(),
            Reason::PlatformUnsupported.as_str(),
            Reason::ReadError.as_str(),
            Reason::InvariantFailed("state.name").as_str(),
            Reason::ShadowMismatch("identity").as_str(),
        ]
    }

    /// Win32 错误码 → reason（`ERROR_ACCESS_DENIED = 5`）。
    pub fn from_win32_error(code: u32) -> Reason {
        if code == 5 {
            Reason::AccessDenied
        } else {
            Reason::ReadError
        }
    }

    /// **进程选择**层面的原因（"选不出唯一目标"）：描述的是**上一次发现时的进程集合**，
    /// 一旦附着成功就不再成立 —— 门必须在附着时把它清掉（Step 9b 缺陷 ①：真机日志里
    /// 附着成功后 `reason=multiple-instances` 一直挂着，冻结帧不带 `beatmap`，24062 的两条
    /// 文件路由因此持续 404）。
    ///
    /// 读取/定址层面的原因（`read-error` / `signature-miss:<key>` / `invariant-failed:<field>`）
    /// **不在此列**：那类冻结要按 §3.4 的"锚点重解析 + `RECOVERY_CLEAN_FRAMES` 帧清白"才退出。
    /// `access-denied` 同样不在列——它有两个来源（`OpenProcess` 被拒 vs. 某段内存读不动），
    /// 后者是读取层面的失败，**不能**因为"换了个附着"就清掉。
    pub fn is_discovery(&self) -> bool {
        matches!(
            self,
            Reason::ProcessNotFound | Reason::MultipleInstances | Reason::ClientAmbiguous
        )
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

    /// **位数分派**（DEC-19）：32 位（PE machine `0x014C`）⇒ stable、64 位（`0x8664`）⇒ lazer。
    ///
    /// 两者进程名逐字相同（`osu!.exe`），位数是唯一判据；其它 machine（ARM64 等）⇒ `None`
    /// （分不出 ⇒ 调用方按既有规则报 reason，绝不猜）。表驱动单测逐行钉住。
    pub fn from_bitness(pe_machine: u16) -> Option<Client> {
        match pe_machine {
            crate::osu::win::PE_MACHINE_I386 => Some(Client::Stable),
            crate::osu::win::PE_MACHINE_AMD64 => Some(Client::Lazer),
            _ => None,
        }
    }
}

/// lazer 的一个 mod（页面只需要三件：`acronym` + `settings` 里的两个数值）。
///
/// 为什么不是位掩码：lazer 的 mod 不是位域（`ModsJson` 是 acronym 列表），`modSignature` 的
/// `speedRate`/`odFlag` 取值在 lazer 分支读的是 `array[].settings.{speed_change,
/// overall_difficulty}`（`js/app/modData.js:150-180`），位掩码表达不了自定义速率。
#[derive(Clone, Debug, PartialEq)]
pub struct LazerMod {
    /// 大写 acronym（逐字来自内存里的 `ModsJson`，**不查白名单**）。
    pub acronym: String,
    /// `settings.speed_change`（DT/HT 等速率 mod 的倍率；`None` = 该 mod 没有这个设置）。
    pub speed_change: Option<f64>,
    /// `settings.overall_difficulty`（DA 的 OD 值）。
    pub overall_difficulty: Option<f64>,
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

/// 六键命中数（页面消费的**唯一**键集：`livePpCounts.js:11-18` 的
/// `{geki,300,katu,100,50,0}`）。tosu v2 另发 `sliderBreaks` 等键，页面不读，本步不发。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Hits {
    pub n300: u32,
    pub n100: u32,
    pub n50: u32,
    pub geki: u32,
    pub katu: u32,
    pub miss: u32,
}

impl Hits {
    /// v2 形状（键名逐字对齐 tosu：字符串键 `"300"`/`"100"`/`"50"`/`"0"` + `geki`/`katu`）。
    pub fn to_json(&self) -> Value {
        json!({
            "geki": self.geki,
            "300": self.n300,
            "katu": self.katu,
            "100": self.n100,
            "50": self.n50,
            "0": self.miss,
        })
    }

    /// 页面会看到的 6 键称重串（对拍用；`livePpCounts.js` 的读取顺序）。
    pub fn canonical(&self) -> String {
        format!(
            "geki={},300={},katu={},100={},50={},0={}",
            self.geki, self.n300, self.katu, self.n100, self.n50, self.miss
        )
    }

    /// 全零（`livePp.js:151-153` 的首帧回退语义）。tosu 在 `live < firstObject-100`
    /// 时也发全零（本步镜像该门）。
    pub fn is_zero(&self) -> bool {
        *self == Hits::default()
    }
}

/// 一帧原生快照（C2 起覆盖 §3.3 的 stable 字段表）。
///
/// 命名对齐 §3.3 的载荷字段名（`state_name` ↔ `state.name`、`checksum` ↔
/// `beatmap.md5`/`beatmap.checksum`…），便于 `packet.rs` 与对拍记录直接引用。
///
/// 所有字段都是 `Option`：`None` = 该跳读失败 ⇒ 字段**不出现在载荷里**（`packet.rs`），
/// 由门状态机（`invariants.rs`）决定该缺失是"降级"还是"失败"。
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
    // ---- C2 新增：规则集 / 局内 / 结算三条链 ----
    /// `ruleset = [[rulesetsAddr-0xB]+0x4]`（诊断 + 后续两链的起点）。
    pub ruleset_base: Option<u32>,
    /// `gameplay_base = [ruleset+0x64]`。
    pub gameplay_base: Option<u32>,
    /// `score_base = [gameplay_base+0x38]`。
    pub score_base: Option<u32>,
    /// `result_base = [ruleset+0x38]`。
    pub result_base: Option<u32>,
    /// 局内 mod 掩码：`[container+0xC] ^ [container+0x8]`，`[score_base+0x54] != 0` 时
    /// 再或上 ScoreV2 位（`0x2000_0000`）。
    pub play_mods_mask: Option<u32>,
    /// 结算 mod 掩码：同一条 `+0x1C` 容器的 XOR 形状（**不加** ScoreV2 位）。
    pub result_mods_mask: Option<u32>,
    /// `beatmap.time.live`（毫秒）。P3 已裁决：内存值**已是按 rate 缩放**的播放位置，
    /// 原样发布、**不得**自除 rate。
    pub play_time: Option<i32>,
    /// `game.paused`：停滞推导（`previousPlayTime == playTime`，镜像 tosu
    /// `states/global.ts:68`）。`None` = 尚未有前一帧（首发帧恒 false）。
    pub paused: Option<bool>,
    /// 局内 hits（`score_base` 链的 u16 槽）。
    pub play_hits: Option<Hits>,
    /// 结算 hits（`result_base` 链的同形槽）。
    pub result_hits: Option<Hits>,
    /// **诊断**：8 个候选 u16 槽的原始值（`+0x88/0x8A/0x8C/0x8E/0x90/0x92/0x94` + `+0x68`），
    /// 顺序与 `stable::HITS_CANDIDATE_OFFSETS` 一致。只进对拍证据，不进载荷。
    pub hits_candidates: Vec<u16>,
    /// 局内候选槽是否**整块**读满（8 个槽全成功）。
    ///
    /// 为什么要有它：读不满时 `hits_candidates` 里会有 0 占位，直接按 6 键发布就是
    /// **半截值**（"0 个 100"与"没读到 100"在页面上无法区分）。发布门要求本标志为真
    /// （见 `invariants::play_hits_publishable`）。
    pub hits_candidates_complete: bool,
    /// 结算屏的同一组候选槽（`result_base` 上的 8 个 u16）。
    pub result_hits_candidates: Vec<u16>,
    /// 结算候选槽是否整块读满（语义同 `hits_candidates_complete`）。
    pub result_hits_candidates_complete: bool,
    /// `[[baseAddr-0x33]+0x8]`。
    pub retries: Option<i32>,
    /// `[[baseAddr-0x33]+0xC]`。
    pub plays: Option<i32>,
    /// `folders.game` = `dirname(osu!.exe)`（P1 实测 == tosu 的 `folders.game`）。
    pub game_folder: Option<String>,
    /// `files.background`（来自 `.osu` 的 `[Events]` 行；tosu 亦解析同一文件）。
    pub background: Option<String>,
    /// `files.audio`（来自 `.osu` 的 `[General] AudioFilename`）。
    pub audio: Option<String>,
    /// `beatmap.time.mp3Length`（内存浮点时长，取整毫秒；best-effort 锚点）。
    pub mp3_length: Option<i64>,
    /// 磁盘 `.osu` 的 MD5（I-09：与内存 checksum 对照；stable 侧为**软**不变量）。
    pub beatmap_file_md5: Option<String>,
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
    // ---- Step 10B：lazer 读取路径（全部由偏移表驱动，见 `osu/lazer.rs`）----
    /// lazer 当前态的 mod 列表（`SelectedMods` 链；`None` = 本帧没有 mods 信息 ⇒ 发 `null`）。
    ///
    /// **不映射到位掩码**：页面在 lazer 分支读 `array[].acronym` 与
    /// `array[].settings.{speed_change,overall_difficulty}`（`modData.js:150-180`）。
    pub lazer_mods: Option<Vec<LazerMod>>,
    /// lazer 解引用链的地址（诊断/证据用；**不进载荷**）。
    pub lazer_chain: Option<crate::osu::lazer::ChainAddrs>,
    /// 观测集外的状态索引（I-01 的降级上报）。
    pub degraded_fields: Vec<String>,
}

impl Snapshot {
    /// 载荷形态的 JSON（`{client, state:{number,name}, beatmap:{…}, files:{…}, …}`）——
    /// §3.3 的完整字段表，见 `packet.rs`。
    pub fn to_packet(&self) -> Value {
        crate::osu::packet::packet_from_snapshot(self)
    }

    /// **字段级冻结**用的载荷（§3.4）：只发 `client` + `state`，**省略 `beatmap`**。
    ///
    /// 为什么不能整包停发：页面 L1 的 `isInPlayState` 靠 `state.name` 维生，
    /// 60s 的 L2 窗口（`sourceManager.js:27`）一过期就把路由甩离 osu
    /// （`socketHandlers.js:192-194` 的 early-return 已支持"只有 state"的形态）。
    pub fn to_frozen_packet(&self) -> Value {
        crate::osu::packet::frozen_packet_from_snapshot(self)
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

    /// §3.3 的字段名 → 值（`None` = 该字段本帧不出现）。
    ///
    /// 冻结形态（`beatmap` 不可信）由调用方决定**不**取 `beatmap.*` 这几项
    /// （见 `to_frozen_packet`）；其余字段照发（`state.name` 必须活着）。
    pub fn is_play_state(&self) -> bool {
        self.state_name.as_deref() == Some("play")
    }

    pub fn is_result_state(&self) -> bool {
        self.state_name.as_deref() == Some("resultScreen")
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
            "play_mods_mask": self.play_mods_mask,
            "result_mods_mask": self.result_mods_mask,
            "play_time": self.play_time,
            "paused": self.paused,
            "play_hits": self.play_hits.map(|h| h.canonical()),
            "result_hits": self.result_hits.map(|h| h.canonical()),
            "hits_candidates": self.hits_candidates,
            "retries": self.retries,
            "plays": self.plays,
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
