// osu!stable 的**全字段读取链**（C2 / Step 8）。
//
// 分工：`win.rs` 是读原语（`read_u32`/`read_pointer`/`read_csharp_string`…），本文件是
// **语义**——每一跳都是一条"待验证假设"（台账在 `patterns.rs`），窗口/门控常数集中在这里，
// 纯解码部分（mods XOR、hits 键映射、播放时间门）与 IO 分离以便单测。
//
// 链（全部 32 位目标，地址一律 `wrapping_*`；链里的位移**显式**书写，不再动
// `Anchor::offset`——它与 `baseAddr` 的 `-0xC` 同一约定）：
//
// ```text
//   status           = read_u32(read_u32(statusPtr))                 → state.number / state.name
//   live             = read_u32(read_u32(playTimeAddr + 0x5))        → beatmap.time.live
//   beatmap          = read_u32(read_u32(baseAddr - 0xC))            → beatmap.{id,set,md5,artist,title,version,mapper}
//   retries / plays  = [read_u32(baseAddr - 0x33) + 0x8 / + 0xC]    （**一级**间接）
//   menu mods        = read_u32(read_u32(menuModsPtr))               → menu.mods
//   ruleset          = read_u32(read_u32(rulesetsAddr - 0xB) + 0x4)  → 下面两条链的起点
//   gameplay         = read_u32(ruleset + 0x64)
//   score            = read_u32(gameplay + 0x38)
//   play mods        = read_u32(read_u32(score + 0x1C) + 0xC)
//                      ^ read_u32(read_u32(score + 0x1C) + 0x8)
//                      | (read_u32(score + 0x54) != 0 ? ScoreV2 : 0)
//   play hits        = u16 槽群（见 HITS_CANDIDATE_OFFSETS）
//   result           = read_u32(ruleset + 0x38)                      → resultsScreen.*
//   mp3Length        = round(f64(read_u32(read_u32(getAudioLengthPtr)) + 0x4))
// ```
//
// ⚠️ **"readPointer" 的两义**：本仓库的 `win::read_pointer` 是**两次读**
// （`[[addr]]`，与 `statusPtr`/`baseAddr` 的既有链一致），而链里像 `read_u32(x + k)`
// 这样的"再跳一跳"是**一次读**。两者混用会让链多/少一跳，所以每条链都逐字写明：
// 凡"两次读"一律用 `read_pointer`，凡"一次读"一律用 `read_u32`。
//
// ⚠️ **hits 键映射已在 Step 8c 关闭**（真机 play 样本反推 ⇒ 六个槽被唯一钉死，
// 与实现原有排列完全一致）：见 `HITS_SLOT_MAPPING` 的支撑表与 `HITS_MAPPING_VERIFIED`
// 的完整证据清单（工具 `evidence/C2-stable-full/tools/hits-mapping.ps1` + 会话
// `hits-samples-play.jsonl`）。映射常量本身**未改动**，改的只是"未验证"标记。

use crate::osu::model::{Hits, Reason};
use crate::osu::win;
use serde_json::{json, Value};

// ---- 链常数（全部写死，实现不得再选；每条都在 patterns.rs 有台账）----

/// `live = read_u32(read_u32(playTimeAddr + 0x5))`：`A1` 的地址立即数在 match+5。
pub const PLAY_TIME_FROM_ANCHOR: u32 = 0x5;
/// `beatmap = read_u32(read_u32(baseAddr - 0xC))`。
pub const BEATMAP_FROM_BASE: u32 = 0xC;
/// `retries`/`plays` 槽：`baseAddr - 0x33`。
pub const INFO_FROM_BASE: u32 = 0x33;
pub const RETRIES_OFFSET: u32 = 0x8;
pub const PLAYS_OFFSET: u32 = 0xC;
/// `ruleset = read_u32(read_u32(rulesetsAddr - 0xB) + 0x4)`。
pub const RULESET_FROM_ANCHOR: u32 = 0xB;
pub const RULESET_LIST_OFFSET: u32 = 0x4;
/// 规则集 → 玩法 / 结算。
pub const GAMEPLAY_FROM_RULESET: u32 = 0x64;
pub const RESULT_FROM_RULESET: u32 = 0x38;
/// 玩法 → 分数对象。
pub const SCORE_FROM_GAMEPLAY: u32 = 0x38;
/// mod 容器（局内与结算共用同一条 `+0x1C` 容器形状）。
pub const MODS_CONTAINER: u32 = 0x1C;
pub const MODS_XOR_HIGH: u32 = 0xC;
pub const MODS_XOR_LOW: u32 = 0x8;
/// ScoreV2 的特殊位：`[score + 0x54] != 0`（ScoreV2 处理器存在）时补上。
/// 为什么必须补：ScoreV2 作为**胜利条件**时不落进 mod 位掩码（两个 XOR 项都不含它），
/// 而页面用它判定 `classic`（`modData.js:219-220`）。
pub const SCORE_PROCESSOR_FROM_SCORE: u32 = 0x54;
pub const SCOREV2_BIT: u32 = 0x2000_0000;
/// 结算屏字段（诊断/自证用；页面只消费 `resultsScreen.{hits,mods}`）。
pub const RESULT_SCORE_OFFSET: u32 = 0x78;
pub const RESULT_MAX_COMBO_OFFSET: u32 = 0x68;
pub const RESULT_PLAYER_NAME_OFFSET: u32 = 0x28;
pub const RESULT_ONLINE_ID_OFFSET: u32 = 0x4;
/// `mp3Length = round(f64(read_u32(read_u32(getAudioLengthPtr)) + 0x4))`。
pub const MP3_LENGTH_FROM_ANCHOR: u32 = 0x7;
pub const MP3_LENGTH_FIELD: u32 = 0x4;
/// 音频时长的合法域（毫秒）：0..=24 h。超出即判该字段不可信（**不给假值**）。
pub const MP3_LENGTH_MAX_MS: f64 = 86_400_000.0;

/// 8 个候选 u16 槽（**顺序即诊断记录顺序**；`+0x94`/`+0x68` 是组合/总数域，不进 6 键）。
///
/// 槽位清单是 C2 的结构假设（同一 `score` 对象上的连续 u16 计数域 + `+0x68` 的组合域），
/// **键映射**由 `HITS_SLOT_MAPPING` 给出；该映射已在真机样本上关闭（见其注释）。
pub const HITS_CANDIDATE_OFFSETS: &[u32] = &[
    0x88, 0x8A, 0x8C, 0x8E, 0x90, 0x92, 0x94, 0x68,
];

/// 候选槽 → 页面使用的 6 键（**已用真机样本钉死**，见 [`HITS_MAPPING_VERIFIED`]）。
///
/// `candidate` 是 `HITS_CANDIDATE_OFFSETS` 的下标。映射按 `score` 对象的**结构顺序**
/// 取候选值（`+0x88` 起连续 6 个 u16 = 六种判定计数，`+0x94` 与 `+0x68` 是组合域，
/// 不进 6 键——它们恒 ≥ 各单键，正是工具用来排除"总数槽"的判据）。
///
/// ⚠️ **不得**凭结构顺序猜：本表由 `evidence/C2-stable-full/tools/hits-mapping.ps1`
/// 在真实 play 样本上反推并锁定（每一键都有一个被全部稳定窗口满足的唯一槽）。
/// 键盘映射的存在理由是 `livePpCounts.js:11-18` 的 6 键消费面。
/// 反例（本表**不**采用"候选顺序"）：若按顺序取 `0x88→100 0x8A→300 0x8C→50 …`，
/// 则 `0x8A` 在会话里被观测为 300 而 `0x88` 为 100 —— 顺序恰好与"结构顺序"不同，
/// 差别只在 `0x8A/0x8C` 与 `0x88` 的相对次序上（见 `HITS_MAPPING_VERIFIED` 的支撑表）。
pub const HITS_SLOT_MAPPING: &[(usize, HitKey)] = &[
    (0, HitKey::N100),
    (1, HitKey::N300),
    (2, HitKey::N50),
    (3, HitKey::Geki),
    (4, HitKey::Katu),
    (5, HitKey::Miss),
];

/// 6 键的枚举（`Hits` 的字段名）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitKey {
    N300,
    N100,
    N50,
    Geki,
    Katu,
    Miss,
}

/// 键映射是否已用真机样本关闭 —— **`true`（2026-09-29 关闭）**。
///
/// 证据（唯一权威；全部在 `temp/osu-native-memory/evidence/C2-stable-full/`）：
/// - 会话：`hits-samples-play.jsonl`（2026-09-29 的 play/resultScreen 真实对拍会话，
///   `our.play.hits` 与 tosu 同刻载荷逐帧同卷；文件指纹见 `C2-notes.md` §11.1）
/// - 工具：`tools/hits-mapping.ps1`（"稳定窗口 + 覆盖裁决"：窗口内要求我方 ≥ tosu 且
///   至少一行相等，逐键给出 pinned 槽与支撑计数；`hits-mapping.play-state.md` /
///   `hits-mapping.play-state-session.md`）
/// - 工具自证：`tools/selftest-windows.jsonl` 用**故意不同**的合成排列 ⇒ 工具报出该排列
///   ⇒ 工具有判别力（`hits-mapping.selftest-windows.md`）
///
/// 六个槽 → 键（下标 = [`HITS_CANDIDATE_OFFSETS`] 的下标）+ 支撑计数：
///
/// | 槽 | 偏移 | 键 | 支撑 |
/// |---|---|---|---|
/// | 3 | `+0x8E` | `geki` | 25/25（次高 `0x94` 9/25） |
/// | 1 | `+0x8A` | `300` | 17/17（次高 `0x94`/`0x68` 各 1/17） |
/// | 4 | `+0x90` | `katu` | 5/5（次高 `0x94` 1/5） |
/// | 0 | `+0x88` | `100` | 3/3（次高 `0x92` 2/3） |
/// | 2 | `+0x8C` | `50` | 1/1 |
/// | 5 | `+0x92` | `0` | 4/4（次高 `0x88` 2/4） |
///
/// **来源声明**：上表来自真实 play 数据的对照，**不是**结构假设或源码推导——六个槽互不
/// 相同，且每个键都被其稳定窗口唯一钉死；旁证是结算态 `result_hits_candidates`
/// `[2,195,0,630,13,2]` 与 tosu `{geki:630,300:195,katu:13,100:2,50:0,0:2}` 逐键相等。
/// 与实现里原有的排列**完全一致** ⇒ **未改动映射**，只是关闭了"未验证"标记。
///
/// `false` 时 `packet.rs` 仍会发布 `play.hits`（值来自内存，不是编造），但 `degradedFields`
/// 会带上 `play.hits.mapping` 且对拍记录里带 `mapping_verified: false`——**不静默**。
/// 现在为 `true` ⇒ 该降级标记不再出现（回归见 `tests-local/osu_invariants.rs`）。
pub const HITS_MAPPING_VERIFIED: bool = true;

// ---- 纯解码（可单测：合成字节 → 掩码 / 键值）----

/// mod 掩码的**纯解码**：`[container+0xC] ^ [container+0x8]`。
/// `score_processor` 只作诊断：V1 局它同样非零，不能据此补 `SCOREV2_BIT`，
/// 否则正常 Stable 会被误判成 SV2（顶部/分轨一起错）。
pub fn mods_mask_from_raw(container_high: u32, container_low: u32, _score_processor: u32) -> u32 {
    container_high ^ container_low
}

/// 候选槽 → 6 键（纯函数；`candidates` 短于映射所需下标时返回 `None`）。
pub fn hits_from_candidates(candidates: &[u16]) -> Option<Hits> {
    let longest = HITS_SLOT_MAPPING
        .iter()
        .map(|(index, _)| *index)
        .max()
        .unwrap_or(0);
    if candidates.len() <= longest {
        return None;
    }
    let mut hits = Hits::default();
    for (index, key) in HITS_SLOT_MAPPING {
        let value = candidates[*index] as u32;
        match key {
            HitKey::N300 => hits.n300 = value,
            HitKey::N100 => hits.n100 = value,
            HitKey::N50 => hits.n50 = value,
            HitKey::Geki => hits.geki = value,
            HitKey::Katu => hits.katu = value,
            HitKey::Miss => hits.miss = value,
        }
    }
    Some(hits)
}

/// 候选槽 → 6 键（由拓扑定义驱动）。
pub fn hits_from_candidates_topo(
    candidates: &[u16],
    topo: Option<&crate::osu::offsets::StableTopology>,
) -> Option<Hits> {
    let default_topo = crate::osu::offsets::StableTopology::default();
    let topo = topo.unwrap_or(&default_topo);
    let longest = topo
        .hits_slot_mapping
        .iter()
        .map(|(index, _)| *index)
        .max()
        .unwrap_or(0);
    if candidates.len() <= longest {
        return None;
    }
    let mut hits = Hits::default();
    for (index, key) in &topo.hits_slot_mapping {
        let value = candidates[*index] as u32;
        match key.as_str() {
            "300" => hits.n300 = value,
            "100" => hits.n100 = value,
            "50" => hits.n50 = value,
            "geki" => hits.geki = value,
            "katu" => hits.katu = value,
            "miss" | "0" => hits.miss = value,
            _ => {}
        }
    }
    Some(hits)
}

/// 候选槽的下标 → 槽偏移（诊断行里把下标写清楚，避免"第几个"读错）。
pub fn candidate_offset(index: usize) -> Option<u32> {
    HITS_CANDIDATE_OFFSETS.get(index).copied()
}

/// hits 的**时间门**（镜像 tosu `gameplay()` 的门）：`live >= firstObject - 100` 才读计数。
///
/// 为什么要有门：进图加载期（`live` 远小于第一个对象）内存里的计数域还是**上一局的残留**
/// （或全零），直接读会发出"看起来像真的"假计数。`firstObject` 未知（`.osu` 未解析）时
/// 不放行——宁可这一帧没有 `hits`，也不发可能串局的值。
pub fn hits_gate_passes(live: i32, first_object: Option<i32>) -> bool {
    match first_object {
        Some(first) => live >= first.saturating_sub(100),
        None => false,
    }
}

/// `game.paused` 的停滞推导：`previous == current`（镜像 tosu `states/global.ts:68`）。
///
/// 首发帧（没有前一帧）恒 `false`——不能把"还没有历史"读成"卡住了"。
pub fn paused_from_previous(previous: Option<i32>, current: Option<i32>) -> bool {
    matches!((previous, current), (Some(previous), Some(current)) if previous == current)
}

/// 音频时长的可用性（纯函数）：有限、非负、且 ≤ 24 h。
pub fn mp3_length_is_sane(seconds_ms: f64) -> bool {
    seconds_ms.is_finite() && seconds_ms >= 0.0 && seconds_ms <= MP3_LENGTH_MAX_MS
}

/// stable 安装目录里的**本机玩家名**候选（`osu!*.cfg` 里的 `Username = …`）。
///
/// 为什么需要它：stable 是闭源进程，没有 lazer 那样的"屏幕类型名"来识别回放；
/// 唯一稳定的判别事实是——结算对象里的 `player_name`（回放结算屏显示的是**被观看者**）
/// 与本机账号名（osu! 自己写进 cfg 的用户名）是否一致。
///
/// ⚠️ cfg **文件名**是 Windows 账户名（`osu!<windows-user>.cfg`），不是 osu! 用户名：
/// 实测 `osu!.Leo_BlackLT.cfg` 里的 `Username = Shirasu-Azusa`。所以必须读文件内容，
/// 且只认 `Username` 这一精确键（`SaveUsername` 之类不算）。
///
/// 规则：解析后为空的、`osu!.cfg`（无 Username 行）一律跳过；**候选存在且唯一地
/// "不含"结算玩家名**时调用方才判回放（见 runner）。目录读不了 / 没有任何候选 ⇒
/// 返回空 ⇒ 不标记（保持现状，绝不因为证据缺失误判）。
pub fn local_player_names(game_folder: &str) -> Vec<String> {
    let dir = std::path::Path::new(game_folder);
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names = Vec::new();
    for entry in entries.flatten() {
        let Ok(file_name) = entry.file_name().into_string() else {
            continue;
        };
        if !file_name.starts_with("osu!") || !file_name.ends_with(".cfg") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        for line in text.lines() {
            let line = line.trim_start_matches('\u{feff}');
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            if key.trim() != "Username" {
                continue;
            }
            let name = value.trim().trim_matches('"').trim();
            if !name.is_empty() {
                names.push(name.to_string());
            }
        }
    }
    names.sort();
    names.dedup();
    names
}

/// 回放判定（stable，纯函数）：结算玩家名非空、本机候选**非空**且全部不匹配 ⇒ 回放。
///
/// 保守边界（宁可漏判不可误判——误判会把真实游玩踢出反馈采集）：
/// - 候选为空（目录不可读 / 没有任何带用户名的 cfg）⇒ `false`；
/// - 玩家名与任一本机候选相等（看自己的回放 / 多账号）⇒ `false`；
/// - 拿不到玩家名 ⇒ `false`。
pub fn replay_from_player_name(player_name: Option<&str>, local_names: &[String]) -> bool {
    let Some(name) = player_name.map(str::trim).filter(|name| !name.is_empty()) else {
        return false;
    };
    !local_names.is_empty() && local_names.iter().all(|candidate| candidate != name)
}

/// [`local_player_names`] 的 10 s 目录缓存：结算屏每帧（250 ms）都会走到这里，
/// 不能每次都读盘；cfg 集合几乎不变，按目录键缓存即可。
pub fn local_player_names_cached(game_folder: &str) -> Vec<String> {
    static CACHE: std::sync::Mutex<Option<(std::time::Instant, String, Vec<String>)>> =
        std::sync::Mutex::new(None);
    const TTL: std::time::Duration = std::time::Duration::from_secs(10);

    let mut cache = CACHE.lock().unwrap_or_else(|error| error.into_inner());
    if let Some((at, folder, names)) = cache.as_ref() {
        if at.elapsed() < TTL && folder == game_folder {
            return names.clone();
        }
    }
    let names = local_player_names(game_folder);
    *cache = Some((std::time::Instant::now(), game_folder.to_string(), names.clone()));
    names
}

// ---- IO：逐条链 ----

/// 一条链的中间值（诊断/证据用；**不进载荷**）。
#[derive(Clone, Copy, Debug, Default)]
pub struct ChainProbe {
    /// `[rulesetsAddr - 0xB]`（槽里的原始指针值）。
    pub ruleset_slot: u32,
    /// `[[rulesetsAddr - 0xB]]`：另一种解释（多一跳）的中间值——只作证据，
    /// 用来证明"少一跳"的那一支才是对的（见 `patterns.rs::RULESETS_ADDR`）。
    pub ruleset_slot_alt: u32,
    /// `[[rulesetsAddr-0xB]+0x4]` = 规则集对象。
    pub ruleset: u32,
    /// `[[[rulesetsAddr-0xB]]+0x4]`：多一跳解释下的"规则集对象"（证据）。
    pub ruleset_alt: u32,
    /// `[ruleset + 0x64]`（玩法基址的**原始槽值**，未解引用）。
    pub gameplay_raw: u32,
    /// `[ruleset + 0x38]`（结算基址）。
    pub result_base: u32,
    /// `[score + 0x1C]`（mod 容器）。
    pub mods_container: u32,
    /// `[container + 0xC]` / `[container + 0x8]`（XOR 的两项）。
    pub mods_xor_high: u32,
    pub mods_xor_low: u32,
    /// `[score + 0x54]`（ScoreV2 处理器）。
    pub score_processor: u32,
}

impl ChainProbe {
    pub fn to_json(&self) -> Value {
        json!({
            "ruleset_slot": hex32(self.ruleset_slot),
            "ruleset_slot_alt": hex32(self.ruleset_slot_alt),
            "ruleset": hex32(self.ruleset),
            "ruleset_alt": hex32(self.ruleset_alt),
            "gameplay_raw": hex32(self.gameplay_raw),
            "result_base": hex32(self.result_base),
            "mods_container": hex32(self.mods_container),
            "mods_xor_high": hex32(self.mods_xor_high),
            "mods_xor_low": hex32(self.mods_xor_low),
            "score_processor": hex32(self.score_processor),
        })
    }
}

fn hex32(value: u32) -> String {
    format!("0x{value:08X}")
}

/// 局内（play/resultScreen 都成立）的一条链的快照。
///
/// `hits` **不在这里定**：时间门需要 `firstObject`（来自 `.osu` 解析），而那条解析在
/// 调用方（`mod.rs::apply_hits`）。本结构只保证"8 个候选槽读齐"。
#[derive(Clone, Debug, Default)]
pub struct InGameRead {
    pub ruleset: Option<u32>,
    pub gameplay_base: Option<u32>,
    pub score_base: Option<u32>,
    pub play_mods_mask: Option<u32>,
    pub hits_candidates: Vec<u16>,
    /// 8 个候选槽是否**整块**读满（发布 `play.hits` 的必要条件之一）。
    pub hits_candidates_complete: bool,
    pub retries: Option<i32>,
    pub plays: Option<i32>,
    pub probe: ChainProbe,
}

/// 结算屏读数（页面只消费 `mods` + `hits`；其余进证据）。
#[derive(Clone, Debug, Default)]
pub struct ResultRead {
    pub result_base: Option<u32>,
    pub result_mods_mask: Option<u32>,
    pub hits_candidates: Vec<u16>,
    /// 8 个候选槽是否整块读满（发布 `resultsScreen.hits` 的必要条件之一）。
    pub hits_candidates_complete: bool,
    pub score: Option<i32>,
    pub max_combo: Option<u16>,
    pub player_name: Option<String>,
    pub online_id: Option<i64>,
    pub probe: ChainProbe,
}

/// `Score+0x34` 原始帧的防御上限（18 分钟 4K 约 10 万帧）。
pub const MAX_REPLAY_FRAMES: usize = 1_000_000;
/// 每拍最多读多少帧（97k 帧分块追平，避免阻塞读取线程）。
pub const REPLAY_FRAMES_PER_TICK: usize = 4096;
/// 连续失败多少拍后清空日志并请求页面重置。
pub const REPLAY_MAX_FAILURES: u32 = 50;

/// `Score+0x34` 原始帧轮询游标。
#[derive(Clone, Debug, Default)]
#[cfg_attr(not(windows), allow(dead_code))]
pub struct StableReplayPollState {
    score: u32,
    list: u32,
    items: u32,
    size: usize,
    len: usize,
    version: i32,
    failures: u32,
    last_time: i32,
}

/// per-note 判定的防御上限（18K 马拉松 + LN 也远小于此）。
pub const MAX_NOTES: usize = 200_000;
/// 连续失败多少拍后清空日志并请求页面重置。
pub const NOTE_MAX_FAILURES: u32 = 50;

/// `emitted[i]` 记录第 i 个 note 的 head/tail 是否已发过事件（bit0=head/tap，bit1=tail）。
#[derive(Clone, Debug, Default)]
#[cfg_attr(not(windows), allow(dead_code))]
pub struct NoteJudgementPollState {
    gameplay: u32,
    list: u32,
    items: u32,
    size: usize,

    emitted: Vec<u8>,
    failures: u32,
}

/// stable 判定枚举 → 页面六桶的 `result`（1=miss … 6=MAX）。
///
/// 只映射**最终计分判定**；`0`（未判）、`7`（tick）、`-1048576`（LN 掉落伪判定）
/// 一律返回 `None`（不进六桶）。
pub fn stable_judgement_result(judgement: i32) -> Option<i32> {
    match judgement {
        268_435_456 => Some(6),  // 300g / MAX
        134_217_728 => Some(5),  // 300
        67_108_864 => Some(4),   // 200
        33_554_432 => Some(3),   // 100
        16_777_216 => Some(2),   // 50
        -65_536 | -131_072 => Some(1), // miss
        _ => None,
    }
}

/// Stable ScoreV2 mania 的六窗（[300g,300,200,100,50,miss]，整数截断）。
///
/// V2 与 V1 唯一的功能差异是 300g 窗（od≤5: 22.4−0.6·od；od>5: 24.9−1.1·od）；
/// HR ÷1.4 / EZ ×1.4 / DT ×1.5 / HT ×0.75 后 `(int)` 截断。
pub fn v2_windows(od: f64, mods_mask: u32) -> [i32; 6] {
    let od = od.clamp(0.0, 10.0);
    let rate = if mods_mask & 256 != 0 {
        0.75 // HT
    } else if mods_mask & (64 | 512) != 0 {
        1.5 // DT / NC
    } else {
        1.0
    };
    let difficulty = if mods_mask & 16 != 0 {
        1.4 // HR
    } else if mods_mask & 2 != 0 {
        1.0 / 1.4 // EZ
    } else {
        1.0
    };
    let tm = rate / difficulty;
    let scale = |value: f64| (value * tm) as i32;
    let perfect = if od <= 5.0 {
        22.4 - 0.6 * od
    } else {
        24.9 - 1.1 * od
    };
    [
        scale(perfect),
        scale(64.0 - 3.0 * od),
        scale(97.0 - 3.0 * od),
        scale(127.0 - 3.0 * od),
        scale(151.0 - 3.0 * od),
        scale(188.0 - 3.0 * od),
    ]
}

/// V2 LN 头档：`|press - time|` 与六窗比较；没有按下 ⇒ miss（1）。
pub fn v2_head_result(offset: i32, windows: &[i32; 6]) -> i32 {
    let error = offset.unsigned_abs();
    for (index, window) in windows.iter().enumerate() {
        if error <= (*window).max(0) as u32 {
            return 6 - index as i32;
        }
    }
    1
}

/// V2 LN 尾档：`|release - end| / 1.5` 与六窗比较。
///
/// stable-re `scorev2-scoring.md` §3.2：V2 回放的 LN 尾是**重算**的（释放判定
/// `num = |release − tail| / 1.5` 再比窗），不是 `+0x5C` 的最终档。
/// 未释放 ⇒ 50；游戏最终 miss ⇒ miss；掉落/超时由调用方按 `judgement` 传入。
pub fn v2_tail_result(release: i32, end: i32, judgement: i32, windows: &[i32; 6]) -> i32 {
    if judgement == -65_536 || judgement == -131_072 || judgement == -1_048_576 {
        return 1;
    }
    if judgement == 16_777_216 {
        return 2; // 游戏最终 50：上限 50
    }
    if release == 0 {
        return 2; // 未释放：上限 50
    }
    let error = (release - end).unsigned_abs() as f64 / 1.5;
    for (index, window) in windows.iter().enumerate() {
        if error <= (*window).max(0) as f64 {
            return 6 - index as i32;
        }
    }
    1
}

#[cfg(windows)]
mod win_io {
    use super::*;

    /// 播放位置（毫秒）：`read_u32(read_u32(playTimeAddr + 0x5))`。
    ///
    /// 槽地址为 0（`exit` 态：下一个函数的全局还没装载）⇒ `Ok(None)` = "本帧没有时间"，
    /// 与"读失败"（`Err`）分开——前者是**合法状态**，后者是结构问题。
    pub fn read_play_time(target: &win::Target, anchor: u32) -> Result<Option<i32>, Reason> {
        read_play_time_with_topo(target, anchor, None)
    }

    pub fn read_play_time_with_topo(
        target: &win::Target,
        anchor: u32,
        topo: Option<&crate::osu::offsets::StableTopology>,
    ) -> Result<Option<i32>, Reason> {
        let default_topo = crate::osu::offsets::StableTopology::default();
        let topo = topo.unwrap_or(&default_topo);
        let slot = win::read_u32(target, anchor.wrapping_add(topo.play_time_from_anchor))?;
        if slot == 0 {
            return Ok(None);
        }
        win::read_u32(target, slot).map(|value| Some(value as i32))
    }

    /// `retries`（`[baseAddr-0x33] + 0x8`，**一级间接**）。
    pub fn read_retries(target: &win::Target, base_addr: u32) -> Option<i32> {
        read_retries_with_topo(target, base_addr, None)
    }

    pub fn read_retries_with_topo(
        target: &win::Target,
        base_addr: u32,
        topo: Option<&crate::osu::offsets::StableTopology>,
    ) -> Option<i32> {
        let default_topo = crate::osu::offsets::StableTopology::default();
        let topo = topo.unwrap_or(&default_topo);
        let slot = win::read_u32(target, base_addr.wrapping_sub(topo.info_from_base)).ok()?;
        if slot == 0 {
            return None;
        }
        win::read_i32(target, slot.wrapping_add(topo.retries_offset)).ok()
    }

    /// `plays`（`[baseAddr-0x33] + 0xC`；间接层数同 `read_retries`）。
    pub fn read_plays(target: &win::Target, base_addr: u32) -> Option<i32> {
        read_plays_with_topo(target, base_addr, None)
    }

    pub fn read_plays_with_topo(
        target: &win::Target,
        base_addr: u32,
        topo: Option<&crate::osu::offsets::StableTopology>,
    ) -> Option<i32> {
        let default_topo = crate::osu::offsets::StableTopology::default();
        let topo = topo.unwrap_or(&default_topo);
        let slot = win::read_u32(target, base_addr.wrapping_sub(topo.info_from_base)).ok()?;
        if slot == 0 {
            return None;
        }
        win::read_i32(target, slot.wrapping_add(topo.plays_offset)).ok()
    }

    /// 规则集链：`read_u32(read_u32(rulesetsAddr - 0xB) + 0x4)`，外加"多一跳"的解释
    /// （只作证据；见 `ChainProbe`）。
    pub fn resolve_ruleset(
        target: &win::Target,
        anchor: u32,
    ) -> Result<(u32, ChainProbe), Reason> {
        resolve_ruleset_with_topo(target, anchor, None)
    }

    pub fn resolve_ruleset_with_topo(
        target: &win::Target,
        anchor: u32,
        topo: Option<&crate::osu::offsets::StableTopology>,
    ) -> Result<(u32, ChainProbe), Reason> {
        let default_topo = crate::osu::offsets::StableTopology::default();
        let topo = topo.unwrap_or(&default_topo);
        let mut probe = ChainProbe::default();
        let slot_addr = anchor.wrapping_sub(topo.ruleset_from_anchor);
        let slot_value = win::read_u32(target, slot_addr)?;
        probe.ruleset_slot = slot_value;
        if !crate::osu::patterns::is_aligned(slot_value) || slot_value < 0x1_0000 {
            return Err(Reason::InvariantFailed("ruleset.slot"));
        }
        let ruleset = win::read_u32(target, slot_value.wrapping_add(topo.ruleset_list_offset))?;
        if ruleset == 0 {
            return Err(Reason::InvariantFailed("ruleset.base"));
        }
        probe.ruleset = ruleset;
        // 证据：多一跳的解释（`[[[A-0xB]]+0x4]`）读出来是什么。
        if let Ok(second) = win::read_u32(target, slot_value) {
            probe.ruleset_slot_alt = second;
            if let Ok(alt) = win::read_u32(target, second.wrapping_add(topo.ruleset_list_offset)) {
                probe.ruleset_alt = alt;
            }
        }
        Ok((ruleset, probe))
    }

    /// 局内链：玩法基址 → 分数对象 → mod 掩码 + 8 个候选 hits 槽（+ `retries`/`plays`）。
    pub fn read_in_game(
        target: &win::Target,
        ruleset: u32,
        base_addr: u32,
    ) -> InGameRead {
        read_in_game_with_topo(target, ruleset, base_addr, None)
    }

    pub fn read_in_game_with_topo(
        target: &win::Target,
        ruleset: u32,
        base_addr: u32,
        topo: Option<&crate::osu::offsets::StableTopology>,
    ) -> InGameRead {
        let default_topo = crate::osu::offsets::StableTopology::default();
        let topo = topo.unwrap_or(&default_topo);
        let mut out = InGameRead {
            ruleset: Some(ruleset),
            retries: read_retries_with_topo(target, base_addr, Some(topo)),
            plays: read_plays_with_topo(target, base_addr, Some(topo)),
            ..Default::default()
        };
        let Ok(gameplay) = win::read_u32(target, ruleset.wrapping_add(topo.gameplay_from_ruleset)) else {
            return out;
        };
        out.probe.gameplay_raw = gameplay;
        if gameplay == 0 {
            return out;
        }
        out.gameplay_base = Some(gameplay);
        let Ok(score) = win::read_u32(target, gameplay.wrapping_add(topo.score_from_gameplay)) else {
            return out;
        };
        if score == 0 {
            return out;
        }
        out.score_base = Some(score);

        // mod 掩码：容器 `[score+0x1C]` → XOR 两项 → ScoreV2 补位。
        if let Ok(container) = win::read_u32(target, score.wrapping_add(topo.mods_container)) {
            out.probe.mods_container = container;
            let high = win::read_u32(target, container.wrapping_add(topo.mods_xor_high)).ok();
            let low = win::read_u32(target, container.wrapping_add(topo.mods_xor_low)).ok();
            let processor = win::read_u32(target, score.wrapping_add(topo.score_processor_from_score));
            if let (Some(high), Some(low)) = (high, low) {
                out.probe.mods_xor_high = high;
                out.probe.mods_xor_low = low;
                let processor = processor.unwrap_or(0);
                out.probe.score_processor = processor;
                out.play_mods_mask = Some(mods_mask_from_raw(high, low, processor));
            }
        }

        let (candidates, complete) = read_candidate_slots_with_topo(target, score, &topo.hits_candidate_offsets);
        out.hits_candidates = candidates;
        out.hits_candidates_complete = complete;
        out
    }

    /// 读一拍原始 ReplayFrame（`Score + score_frames` 的 `List<ReplayFrame>`）。
    ///
    /// 增量按 `_size` 增长；首拍/对象变化/`_size` 回退/`_version` 非纯 append ⇒ reset + 全量。
    /// 每拍最多 [`REPLAY_FRAMES_PER_TICK`] 帧；坏帧**不跳过**（帧流有状态，漏一帧会永久
    /// 污染按键掩码），连续失败 [`REPLAY_MAX_FAILURES`] 拍才整段清空重读。
    pub fn poll_replay_frames(
        target: &win::Target,
        topo: Option<&crate::osu::offsets::StableTopology>,
        score: u32,
        log: &crate::osu::lazer::ReplayFrameLog,
        state: &mut StableReplayPollState,
    ) -> Result<usize, &'static str> {
        let default_topo = crate::osu::offsets::StableTopology::default();
        let topo = topo.unwrap_or(&default_topo);
        let Some(section) = topo.replay.as_ref() else {
            return Ok(0);
        };
        if score == 0 {
            return Ok(0);
        }
        let list = win::read_u32(target, score.wrapping_add(section.score_frames))
            .map_err(|_| "replay-list-unreadable")?;
        if list == 0 {
            return Ok(0);
        }
        let items = win::read_u32(target, list.wrapping_add(section.list_items))
            .map_err(|_| "replay-items-unreadable")?;
        let size = win::read_i32(target, list.wrapping_add(section.list_size))
            .map_err(|_| "replay-size-unreadable")?;
        if size < 0 || size as usize > MAX_REPLAY_FRAMES || items == 0 {
            return Err("replay-size-invalid");
        }
        let size = size as usize;
        let version = match section.list_version {
            Some(offset) => win::read_i32(target, list.wrapping_add(offset)).unwrap_or(0),
            None => 0,
        };
        let mut reset = state.score != score
            || state.list != list
            || state.items != items
            || size < state.len;
        if !reset && state.len > 0 && section.list_version.is_some() {
            let size_delta = size as i64 - state.size as i64;
            let version_delta = version as i64 - state.version as i64;
            if size_delta != version_delta {
                reset = true;
            }
        }
        if reset {
            state.len = 0;
            state.last_time = i32::MIN;
        }
        let start = state.len;
        let end = (start + REPLAY_FRAMES_PER_TICK).min(size);
        let mut frames = Vec::new();
        let mut index = start;
        let mut failed = false;
        while index < end {
            let frame = match win::read_u32(
                target,
                items
                    .wrapping_add(section.array_data)
                    .wrapping_add((index as u32).wrapping_mul(4)),
            ) {
                Ok(frame) if frame != 0 => frame,
                _ => {
                    failed = true;
                    break;
                }
            };
            let time = match win::read_i32(target, frame.wrapping_add(section.frame_time)) {
                Ok(value) => value,
                Err(_) => {
                    failed = true;
                    break;
                }
            };
            let buttons = win::read_u32(target, frame.wrapping_add(section.frame_buttons)).unwrap_or(0);
            let y = f32::from_bits(win::read_u32(target, frame.wrapping_add(section.frame_y)).unwrap_or(0));
            // 加载期光标帧：y=-500、x=256；mania 帧 y 为列宽常量（>0）。
            if buttons != 0 || !(y > 0.0) {
                index += 1;
                continue;
            }
            let x = f32::from_bits(win::read_u32(target, frame.wrapping_add(section.frame_keys)).unwrap_or(0));
            if !x.is_finite()
                || x < 0.0
                || x > ((1u32 << 18) - 1) as f32
                || x != x.trunc()
            {
                failed = true;
                break;
            }
            if time < state.last_time {
                failed = true;
                break;
            }
            frames.push(crate::osu::lazer::RawReplayFrame {
                time: time as f64,
                keys: x as u32,
            });
            state.last_time = time;
            index += 1;
        }
        if reset || !frames.is_empty() {
            log.push(reset, &frames);
        }
        if failed {
            state.len = index;
            state.failures = state.failures.saturating_add(1);
            if state.failures >= REPLAY_MAX_FAILURES {
                log.clear();
                *state = StableReplayPollState::default();
            }
            return Err("replay-frame-invalid");
        }
        state.score = score;
        state.list = list;
        state.items = items;
        state.size = size;
        state.version = version;
        state.len = index;
        state.failures = 0;
        Ok(frames.len())
    }

    /// 读一拍 per-note 判定（`gameplay + note_list_from_gameplay` 的 `List<Note>`）。
    ///
    /// 全表扫描 + `emitted` 去重：note 表 6.6k/15k 级别，单拍 ~5–15ms；不依赖判定顺序，
    /// 能正确处理 LN 尾判晚于后续 tap 的情况。表缺 `notes` 段 ⇒ `Ok(0)`（不猜）。
    #[allow(clippy::too_many_arguments)]
    pub fn poll_note_judgements(
        target: &win::Target,
        topo: Option<&crate::osu::offsets::StableTopology>,
        gameplay: u32,
        od: f64,
        mods_mask: u32,
        log: &crate::osu::lazer::HitEventLog,
        state: &mut NoteJudgementPollState,
    ) -> Result<usize, &'static str> {
        let default_topo = crate::osu::offsets::StableTopology::default();
        let topo = topo.unwrap_or(&default_topo);
        let Some(section) = topo.notes.as_ref() else {
            return Ok(0);
        };
        let Some(column_offset) = section.note_column else {
            return Ok(0);
        };
        if gameplay == 0 {
            return Ok(0);
        }
        // V2 局：LN 头/尾各算一个判定；头档用 press-time 按 V2 窗口补算，尾档用 +0x5C。
        let v2 = (mods_mask & 0x2000_0000 != 0).then(|| v2_windows(od, mods_mask));
        let list = win::read_u32(target, gameplay.wrapping_add(section.note_list_from_gameplay))
            .map_err(|_| "note-list-unreadable")?;
        if list == 0 {
            return Ok(0);
        }
        let items = win::read_u32(target, list.wrapping_add(section.list_items))
            .map_err(|_| "note-items-unreadable")?;
        let size = win::read_i32(target, list.wrapping_add(section.list_size))
            .map_err(|_| "note-size-unreadable")?;
        if size < 0 || size as usize > MAX_NOTES || items == 0 {
            return Err("note-size-invalid");
        }
        let size = size as usize;
        let reset = state.gameplay != gameplay
            || state.list != list
            || state.items != items
            || state.size != size;
        if reset {
            state.emitted = vec![0u8; size];
            if let Some(windows) = v2.as_ref() {
                crate::server::log::log_at(
                    "info",
                    &format!(
                        "[stable] v2 od={od:.3} mods={mods_mask:#010x} windows={windows:?}"
                    ),
                );
            }
        }
        let mut events = Vec::new();
        let mut failed = false;
        // 全表扫描：长 LN 会把 cursor 卡住，窗口外的 note 会漏判（V2 双事件尤甚）。
        for index in 0..size {
            if state.emitted.get(index).copied().unwrap_or(0) != 0 {
                continue;
            }
            let note = match win::read_u32(
                target,
                items
                    .wrapping_add(section.array_data)
                    .wrapping_add((index as u32).wrapping_mul(4)),
            ) {
                Ok(note) if note != 0 => note,
                _ => {
                    failed = true;
                    break;
                }
            };
            let judgement = match win::read_i32(target, note.wrapping_add(section.note_judgement)) {
                Ok(value) => value,
                Err(_) => {
                    failed = true;
                    break;
                }
            };
            let time = match win::read_i32(target, note.wrapping_add(section.note_time)) {
                Ok(value) => value,
                Err(_) => {
                    failed = true;
                    break;
                }
            };
            let is_hold = section
                .note_is_hold
                .map(|offset| win::read_i32(target, note.wrapping_add(offset)).unwrap_or(0) != 0)
                .unwrap_or(false);
            let press = section
                .note_press
                .map(|offset| win::read_i32(target, note.wrapping_add(offset)).unwrap_or(0))
                .unwrap_or(0);
            let release = section
                .note_release
                .map(|offset| win::read_i32(target, note.wrapping_add(offset)).unwrap_or(0))
                .unwrap_or(0);
            let column = match win::read_i32(target, note.wrapping_add(column_offset)) {
                Ok(value) => value,
                Err(_) => {
                    failed = true;
                    break;
                }
            };
            if !(0..=17).contains(&column) {
                failed = true;
                break;
            }
            let mut emitted = state.emitted.get(index).copied().unwrap_or(0);
            if let Some(windows) = v2.as_ref() {
                if is_hold {
                    // V2 LN：头档 = press-time；尾档 = |release-end|/1.5（stable-re §3.2）。
                    // `+0x5C` 只用于 tap / miss / 50 上限，不再当 LN 尾档。
                    if (emitted & 1) == 0 {
                        // `+0x5C` 同源头档实测更差（MAX 4580 vs 4570，300 488 vs 497）；
                        // 头档只用 press−time 的 V2 窗口，`+0x5C` 只负责尾档。
                        let head_result = if press != 0 {
                            v2_head_result(press - time, windows)
                        } else if judgement != 0 || release != 0 {
                            1
                        } else {
                            continue;
                        };
                        events.push(crate::osu::lazer::RawHitEvent {
                            col: column,
                            result: head_result,
                            offset: 0.0,
                            note: Some(index as i32),
                        });
                        emitted |= 1;
                    }
                    if (emitted & 2) == 0 && judgement != 0 {
                        // `release` 在回放里多数未写（0），不能用它重算 V2 尾档；
                        // `+0x5C` 仍是当前唯一可用的最终尾档来源。
                        if let Some(result) = stable_judgement_result(judgement) {
                            events.push(crate::osu::lazer::RawHitEvent {
                                col: column,
                                result,
                                offset: 0.0,
                                note: Some(index as i32),
                            });
                            emitted |= 2;
                        }
                    }
                    state.emitted[index] = emitted;
                    continue;
                }
            }
            if (emitted & 1) == 0 {
                if let Some(result) = stable_judgement_result(judgement) {
                    events.push(crate::osu::lazer::RawHitEvent {
                        col: column,
                        result,
                        offset: 0.0,
                        note: Some(index as i32),
                    });
                    emitted |= 3;
                }
            }
            state.emitted[index] = emitted;
        }
        if failed {
            state.failures = state.failures.saturating_add(1);
            if state.failures >= NOTE_MAX_FAILURES {
                log.clear();
                *state = NoteJudgementPollState::default();
            }
            return Err("note-judgement-unreadable");
        }
        if reset || !events.is_empty() {
            log.push(reset, &events);
        }
        state.gameplay = gameplay;
        state.list = list;
        state.items = items;
        state.size = size;
        state.failures = 0;
        Ok(events.len())
    }

    /// 结算链：`result = [ruleset + 0x38]` → mod 掩码 + hits + 自证字段。
    pub fn read_result(target: &win::Target, ruleset: u32) -> ResultRead {
        read_result_with_topo(target, ruleset, None)
    }

    pub fn read_result_with_topo(
        target: &win::Target,
        ruleset: u32,
        topo: Option<&crate::osu::offsets::StableTopology>,
    ) -> ResultRead {
        let default_topo = crate::osu::offsets::StableTopology::default();
        let topo = topo.unwrap_or(&default_topo);
        let mut out = ResultRead {
            probe: ChainProbe {
                ruleset,
                ..Default::default()
            },
            ..Default::default()
        };
        let Ok(result) = win::read_u32(target, ruleset.wrapping_add(topo.result_from_ruleset)) else {
            return out;
        };
        if result == 0 {
            return out;
        }
        out.result_base = Some(result);
        out.probe.result_base = result;

        if let Ok(container) = win::read_u32(target, result.wrapping_add(topo.mods_container)) {
            out.probe.mods_container = container;
            let high = win::read_u32(target, container.wrapping_add(topo.mods_xor_high)).ok();
            let low = win::read_u32(target, container.wrapping_add(topo.mods_xor_low)).ok();
            if let (Some(high), Some(low)) = (high, low) {
                out.probe.mods_xor_high = high;
                out.probe.mods_xor_low = low;
                out.result_mods_mask = Some(mods_mask_from_raw(high, low, 0));
            }
        }

        let (candidates, complete) = read_candidate_slots_with_topo(target, result, &topo.hits_candidate_offsets);
        out.hits_candidates = candidates;
        out.hits_candidates_complete = complete;
        out.score = win::read_i32(target, result.wrapping_add(topo.result_score_offset)).ok();
        out.max_combo = win::read_u16(target, result.wrapping_add(topo.result_max_combo_offset)).ok();
        out.player_name = win::read_u32(target, result.wrapping_add(topo.result_player_name_offset))
            .ok()
            .and_then(|ptr| win::read_csharp_string(target, ptr).ok());
        out.online_id = read_i64(target, result.wrapping_add(topo.result_online_id_offset));
        out
    }

    fn read_i64(target: &win::Target, addr: u32) -> Option<i64> {
        let low = win::read_u32(target, addr).ok()? as u64;
        let high = win::read_u32(target, addr.wrapping_add(4)).ok()? as u64;
        Some(((high << 32) | low) as i64)
    }

    #[allow(dead_code)]
    fn read_candidate_slots(target: &win::Target, base: u32) -> (Vec<u16>, bool) {
        read_candidate_slots_with_topo(target, base, HITS_CANDIDATE_OFFSETS)
    }

    fn read_candidate_slots_with_topo(target: &win::Target, base: u32, offsets: &[u32]) -> (Vec<u16>, bool) {
        let mut values = Vec::with_capacity(offsets.len());
        let mut complete = true;
        for offset in offsets {
            match win::read_u16(target, base.wrapping_add(*offset)) {
                Ok(value) => values.push(value),
                Err(_) => {
                    complete = false;
                    values.push(0);
                }
            }
        }
        (values, complete)
    }

    /// `beatmap.time.mp3Length`：`round(f64(read_u32(read_u32(anchor)) + 0x4))`。
    pub fn read_mp3_length(target: &win::Target, anchor: u32) -> Result<i64, Reason> {
        read_mp3_length_with_topo(target, anchor, None)
    }

    pub fn read_mp3_length_with_topo(
        target: &win::Target,
        anchor: u32,
        topo: Option<&crate::osu::offsets::StableTopology>,
    ) -> Result<i64, Reason> {
        let default_topo = crate::osu::offsets::StableTopology::default();
        let topo = topo.unwrap_or(&default_topo);
        let slot = win::read_pointer(target, anchor.wrapping_add(topo.mp3_length_from_anchor))?;
        if slot == 0 {
            return Err(Reason::InvariantFailed("mp3Length.object"));
        }
        let seconds = win::read_f64(target, slot.wrapping_add(topo.mp3_length_field))?;
        if !mp3_length_is_sane(seconds) {
            return Err(Reason::InvariantFailed("mp3Length.range"));
        }
        Ok(seconds.round() as i64)
    }

    /// `folders.game` = `dirname(osu!.exe)`（只读路径字符串，不碰内存）。
    pub fn game_folder(image_path: &std::path::Path) -> Option<String> {
        image_path
            .parent()
            .filter(|dir| !dir.as_os_str().is_empty())
            .map(|dir| dir.to_string_lossy().to_string())
    }
}

#[cfg(windows)]
pub use win_io::*;

// ---- 非 Windows 桩（同 `mod.rs` 约定：同名同签名，行为为空）----

#[cfg(not(windows))]
pub fn read_play_time(_target: &win::Target, _anchor: u32) -> Result<Option<i32>, Reason> {
    Err(Reason::PlatformUnsupported)
}

#[cfg(not(windows))]
pub fn read_play_time_with_topo(_target: &win::Target, _anchor: u32, _topo: Option<&crate::osu::offsets::StableTopology>) -> Result<Option<i32>, Reason> {
    Err(Reason::PlatformUnsupported)
}

#[cfg(not(windows))]
pub fn read_retries(_target: &win::Target, _base_addr: u32) -> Option<i32> {
    None
}

#[cfg(not(windows))]
pub fn read_retries_with_topo(_target: &win::Target, _base_addr: u32, _topo: Option<&crate::osu::offsets::StableTopology>) -> Option<i32> {
    None
}

#[cfg(not(windows))]
pub fn read_plays(_target: &win::Target, _base_addr: u32) -> Option<i32> {
    None
}

#[cfg(not(windows))]
pub fn read_plays_with_topo(_target: &win::Target, _base_addr: u32, _topo: Option<&crate::osu::offsets::StableTopology>) -> Option<i32> {
    None
}

#[cfg(not(windows))]
pub fn resolve_ruleset(
    _target: &win::Target,
    _anchor: u32,
) -> Result<(u32, ChainProbe), Reason> {
    Err(Reason::PlatformUnsupported)
}

#[cfg(not(windows))]
pub fn resolve_ruleset_with_topo(
    _target: &win::Target,
    _anchor: u32,
    _topo: Option<&crate::osu::offsets::StableTopology>,
) -> Result<(u32, ChainProbe), Reason> {
    Err(Reason::PlatformUnsupported)
}

#[cfg(not(windows))]
pub fn read_in_game(
    _target: &win::Target,
    _ruleset: u32,
    _base_addr: u32,
) -> InGameRead {
    InGameRead::default()
}

#[cfg(not(windows))]
pub fn read_in_game_with_topo(
    _target: &win::Target,
    _ruleset: u32,
    _base_addr: u32,
    _topo: Option<&crate::osu::offsets::StableTopology>,
) -> InGameRead {
    InGameRead::default()
}

#[cfg(not(windows))]
pub fn poll_replay_frames(
    _target: &win::Target,
    _topo: Option<&crate::osu::offsets::StableTopology>,
    _score: u32,
    _log: &crate::osu::lazer::ReplayFrameLog,
    _state: &mut StableReplayPollState,
) -> Result<usize, &'static str> {
    Ok(0)
}

#[cfg(not(windows))]
pub fn poll_note_judgements(
    _target: &win::Target,
    _topo: Option<&crate::osu::offsets::StableTopology>,
    _gameplay: u32,
    _od: f64,
    _mods_mask: u32,
    _log: &crate::osu::lazer::HitEventLog,
    _state: &mut NoteJudgementPollState,
) -> Result<usize, &'static str> {
    Ok(0)
}

#[cfg(not(windows))]
pub fn read_result(_target: &win::Target, _ruleset: u32) -> ResultRead {
    ResultRead::default()
}

#[cfg(not(windows))]
pub fn read_result_with_topo(_target: &win::Target, _ruleset: u32, _topo: Option<&crate::osu::offsets::StableTopology>) -> ResultRead {
    ResultRead::default()
}

#[cfg(not(windows))]
pub fn read_mp3_length(_target: &win::Target, _anchor: u32) -> Result<i64, Reason> {
    Err(Reason::PlatformUnsupported)
}

#[cfg(not(windows))]
pub fn read_mp3_length_with_topo(_target: &win::Target, _anchor: u32, _topo: Option<&crate::osu::offsets::StableTopology>) -> Result<i64, Reason> {
    Err(Reason::PlatformUnsupported)
}

#[cfg(not(windows))]
pub fn game_folder(_image_path: &std::path::Path) -> Option<String> {
    None
}

#[cfg(test)]
#[path = "../../tests-local/osu_stable.rs"]
mod tests_stable;
