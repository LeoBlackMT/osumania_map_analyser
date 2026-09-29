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

/// mod 掩码的**纯解码**：`[container+0xC] ^ [container+0x8]`，再按 ScoreV2 处理器补位。
pub fn mods_mask_from_raw(container_high: u32, container_low: u32, score_processor: u32) -> u32 {
    let mut mask = container_high ^ container_low;
    if score_processor != 0 {
        mask |= SCOREV2_BIT;
    }
    mask
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

#[cfg(windows)]
mod win_io {
    use super::*;

    /// 播放位置（毫秒）：`read_u32(read_u32(playTimeAddr + 0x5))`。
    ///
    /// 槽地址为 0（`exit` 态：下一个函数的全局还没装载）⇒ `Ok(None)` = "本帧没有时间"，
    /// 与"读失败"（`Err`）分开——前者是**合法状态**，后者是结构问题。
    pub fn read_play_time(target: &win::Target, anchor: u32) -> Result<Option<i32>, Reason> {
        let slot = win::read_u32(target, anchor.wrapping_add(PLAY_TIME_FROM_ANCHOR))?;
        if slot == 0 {
            return Ok(None);
        }
        win::read_u32(target, slot).map(|value| Some(value as i32))
    }

    /// `retries`（`[baseAddr-0x33] + 0x8`，**一级间接**）。
    ///
    /// ⚠️ 这条链的间接层数与其他链不同：`[baseAddr-0x33]` 本身就是那个信息对象（`+0x8`/`+0xC`
    /// 是它的两个字段），所以只有**一次**读——写两次会一路读到对象的第一个字段里去
    /// （C2 首轮真机实测：两次读的结果恒为 `None`）。
    pub fn read_retries(target: &win::Target, base_addr: u32) -> Option<i32> {
        let slot = win::read_u32(target, base_addr.wrapping_sub(INFO_FROM_BASE)).ok()?;
        if slot == 0 {
            return None;
        }
        win::read_i32(target, slot.wrapping_add(RETRIES_OFFSET)).ok()
    }

    /// `plays`（`[baseAddr-0x33] + 0xC`；间接层数同 `read_retries`）。
    pub fn read_plays(target: &win::Target, base_addr: u32) -> Option<i32> {
        let slot = win::read_u32(target, base_addr.wrapping_sub(INFO_FROM_BASE)).ok()?;
        if slot == 0 {
            return None;
        }
        win::read_i32(target, slot.wrapping_add(PLAYS_OFFSET)).ok()
    }

    /// 规则集链：`read_u32(read_u32(rulesetsAddr - 0xB) + 0x4)`，外加"多一跳"的解释
    /// （只作证据；见 `ChainProbe`）。
    ///
    /// 三个原始值全部落进 `probe`：真机证据里要能看到"哪一跳对数、多一跳读到什么"，
    /// 否则下次偏移漂移时无法判断是签名错位还是链跳数错。
    pub fn resolve_ruleset(
        target: &win::Target,
        anchor: u32,
    ) -> Result<(u32, ChainProbe), Reason> {
        let mut probe = ChainProbe::default();
        let slot_addr = anchor.wrapping_sub(RULESET_FROM_ANCHOR);
        let slot_value = win::read_u32(target, slot_addr)?;
        probe.ruleset_slot = slot_value;
        if !crate::osu::patterns::is_aligned(slot_value) || slot_value < 0x1_0000 {
            return Err(Reason::InvariantFailed("ruleset.slot"));
        }
        let ruleset = win::read_u32(target, slot_value.wrapping_add(RULESET_LIST_OFFSET))?;
        if ruleset == 0 {
            return Err(Reason::InvariantFailed("ruleset.base"));
        }
        probe.ruleset = ruleset;
        // 证据：多一跳的解释（`[[[A-0xB]]+0x4]`）读出来是什么。
        if let Ok(second) = win::read_u32(target, slot_value) {
            probe.ruleset_slot_alt = second;
            if let Ok(alt) = win::read_u32(target, second.wrapping_add(RULESET_LIST_OFFSET)) {
                probe.ruleset_alt = alt;
            }
        }
        Ok((ruleset, probe))
    }

    /// 局内链：玩法基址 → 分数对象 → mod 掩码 + 8 个候选 hits 槽（+ `retries`/`plays`）。
    ///
    /// 语义：`gameplay = [ruleset + 0x64]`（一次读）；`score = [gameplay + 0x38]`（一次读）。
    /// 任一为 0 ⇒ 该状态没有局内对象（菜单态属正常）⇒ 对应字段 `None`，**不判失败**。
    pub fn read_in_game(
        target: &win::Target,
        ruleset: u32,
        base_addr: u32,
    ) -> InGameRead {
        let mut out = InGameRead {
            ruleset: Some(ruleset),
            retries: read_retries(target, base_addr),
            plays: read_plays(target, base_addr),
            ..Default::default()
        };
        let Ok(gameplay) = win::read_u32(target, ruleset.wrapping_add(GAMEPLAY_FROM_RULESET)) else {
            return out;
        };
        out.probe.gameplay_raw = gameplay;
        if gameplay == 0 {
            return out;
        }
        out.gameplay_base = Some(gameplay);
        let Ok(score) = win::read_u32(target, gameplay.wrapping_add(SCORE_FROM_GAMEPLAY)) else {
            return out;
        };
        if score == 0 {
            return out;
        }
        out.score_base = Some(score);

        // mod 掩码：容器 `[score+0x1C]` → XOR 两项 → ScoreV2 补位。
        if let Ok(container) = win::read_u32(target, score.wrapping_add(MODS_CONTAINER)) {
            out.probe.mods_container = container;
            let high = win::read_u32(target, container.wrapping_add(MODS_XOR_HIGH)).ok();
            let low = win::read_u32(target, container.wrapping_add(MODS_XOR_LOW)).ok();
            let processor = win::read_u32(target, score.wrapping_add(SCORE_PROCESSOR_FROM_SCORE));
            if let (Some(high), Some(low)) = (high, low) {
                out.probe.mods_xor_high = high;
                out.probe.mods_xor_low = low;
                let processor = processor.unwrap_or(0);
                out.probe.score_processor = processor;
                out.play_mods_mask = Some(mods_mask_from_raw(high, low, processor));
            }
        }

        // 8 个候选槽**先读齐**（诊断）；是否发布由调用方的门决定
        // （状态门 + 链有效性门 + 时间门，见 `invariants::play_hits_publishable`）。
        let (candidates, complete) = read_candidate_slots(target, score);
        out.hits_candidates = candidates;
        out.hits_candidates_complete = complete;
        out
    }

    /// 结算链：`result = [ruleset + 0x38]` → mod 掩码 + hits + 自证字段。
    pub fn read_result(target: &win::Target, ruleset: u32) -> ResultRead {
        let mut out = ResultRead {
            probe: ChainProbe {
                ruleset,
                ..Default::default()
            },
            ..Default::default()
        };
        let Ok(result) = win::read_u32(target, ruleset.wrapping_add(RESULT_FROM_RULESET)) else {
            return out;
        };
        if result == 0 {
            return out;
        }
        out.result_base = Some(result);
        out.probe.result_base = result;

        if let Ok(container) = win::read_u32(target, result.wrapping_add(MODS_CONTAINER)) {
            out.probe.mods_container = container;
            let high = win::read_u32(target, container.wrapping_add(MODS_XOR_HIGH)).ok();
            let low = win::read_u32(target, container.wrapping_add(MODS_XOR_LOW)).ok();
            if let (Some(high), Some(low)) = (high, low) {
                out.probe.mods_xor_high = high;
                out.probe.mods_xor_low = low;
                // 结算链**不**补 ScoreV2 位（该链的两个 XOR 项里已含 mod 位；
                // 结算屏没有 "score processor" 语义）。
                out.result_mods_mask = Some(mods_mask_from_raw(high, low, 0));
            }
        }

        let (candidates, complete) = read_candidate_slots(target, result);
        out.hits_candidates = candidates;
        out.hits_candidates_complete = complete;
        out.score = win::read_i32(target, result.wrapping_add(RESULT_SCORE_OFFSET)).ok();
        out.max_combo = win::read_u16(target, result.wrapping_add(RESULT_MAX_COMBO_OFFSET)).ok();
        out.player_name = win::read_u32(target, result.wrapping_add(RESULT_PLAYER_NAME_OFFSET))
            .ok()
            .and_then(|ptr| win::read_csharp_string(target, ptr).ok());
        out.online_id = read_i64(target, result.wrapping_add(RESULT_ONLINE_ID_OFFSET));
        out
    }

    fn read_i64(target: &win::Target, addr: u32) -> Option<i64> {
        let low = win::read_u32(target, addr).ok()? as u64;
        let high = win::read_u32(target, addr.wrapping_add(4)).ok()? as u64;
        Some(((high << 32) | low) as i64)
    }

    /// 8 个候选 u16 槽 + **整块读满**标志。
    ///
    /// 读不出来的位置仍然记 `0`（诊断要看"读到什么"），但返回的第二个值为 `false`——
    /// 调用方的发布门据此**拒绝发布半截值**（"0 个 100"与"没读到 100"在页面上无法区分）。
    fn read_candidate_slots(target: &win::Target, base: u32) -> (Vec<u16>, bool) {
        let mut values = Vec::with_capacity(HITS_CANDIDATE_OFFSETS.len());
        let mut complete = true;
        for offset in HITS_CANDIDATE_OFFSETS {
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
        let slot = win::read_pointer(target, anchor.wrapping_add(MP3_LENGTH_FROM_ANCHOR))?;
        if slot == 0 {
            return Err(Reason::InvariantFailed("mp3Length.object"));
        }
        let seconds = win::read_f64(target, slot.wrapping_add(MP3_LENGTH_FIELD))?;
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
pub fn read_retries(_target: &win::Target, _base_addr: u32) -> Option<i32> {
    None
}

#[cfg(not(windows))]
pub fn read_plays(_target: &win::Target, _base_addr: u32) -> Option<i32> {
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
pub fn read_in_game(
    _target: &win::Target,
    _ruleset: u32,
    _base_addr: u32,
) -> InGameRead {
    InGameRead::default()
}

#[cfg(not(windows))]
pub fn read_result(_target: &win::Target, _ruleset: u32) -> ResultRead {
    ResultRead::default()
}

#[cfg(not(windows))]
pub fn read_mp3_length(_target: &win::Target, _anchor: u32) -> Result<i64, Reason> {
    Err(Reason::PlatformUnsupported)
}

#[cfg(not(windows))]
pub fn game_folder(_image_path: &std::path::Path) -> Option<String> {
    None
}

#[cfg(test)]
#[path = "../../tests-local/osu_stable.rs"]
mod tests_stable;
