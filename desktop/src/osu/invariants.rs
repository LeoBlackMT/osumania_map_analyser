// L2 不变量（I-01…I-10）+ 健康状态机（§3.4 的逐条实现）。
//
// 四层门的分工（计划 §3.4）：
// - **L0 锚点解析**：附着时/进程变化时/失败刷新时（在 `mod.rs` 的扫描路径里）
// - **L1 结构校验**：每次读（`win.rs` 的"整块读满" + `mod.rs::anchor_proves_out` 的候选自证
//   + 本文件的 `I-06` 指针合理性）
// - **L2 不变量**：**本文件**。无状态者每次轮询；时序性者（`Cadence::OnTransition`）只在
//   状态迁移那一帧评估
// - **L3 影子比对**：`shadow.rs`（tosu 在线时）
//
// 三种结局（**必须分清**，B2 的教训是把"没比"读成"比了且不等"）：
// - `Hard` 失败 ⇒ 累加 `INVARIANT_STRIKES`；到 3 次即 `unhealthy`（不出帧）；在此之前走
//   **字段级冻结**（只发 `state.name`，见 `Snapshot::to_frozen_packet`）
// - `Soft` 失败 ⇒ 照常出帧，但字段进 `degradedFields`（`degraded` ≠ `unhealthy`）
// - 通过 ⇒ 正常
//
// **hard 的准入条件（Step 8c 收紧）**：只有"结构上可证伪"的量配 `Hard`。时序性/派生量
// （I-04 `beatmap.time.live` 在本步由硬改软）一律 `Soft` —— 它们的"越界"在合法状态里
// 真实存在（重开图/重试/seek/结算钟摆），硬判会在日常态反复冻结—重扫，把整条载荷停掉。
// 逐状态的"绝不 unhealthy"回归表见 `tests-local/osu_invariants.rs::per_state_*`。
//
// 退出冻结的条件**不得是"值看起来又合理了"**（会永久闩死）：只能是
// ① 锚点重解析成功（`on_re_resolve`）且随后 `RECOVERY_CLEAN_FRAMES` 帧全清，
// ② 或有界窗口（`FREEZE_WINDOW`）到期后强制重解析（`should_re_resolve`）。
// 两条路径都必须记录 reason（`Reason` 闭集）。

use crate::osu::keys;
use crate::osu::model::{Hits, Reason, Snapshot};
use crate::osu::stable;
use std::time::Duration;

// ---- 门常数（§3.4 写死，实现不得再选）----

/// 连续不过的门次数（≈3×250 ms 的 tick ⇒ 亚秒级停帧）。
pub const INVARIANT_STRIKES: u32 = 3;
/// 恢复所需的连续清白帧数（≈3 s @150ms；本壳 250 ms tick ⇒ ≈5 s）。
pub const RECOVERY_CLEAN_FRAMES: u32 = 20;
/// 最小驻留：状态/传输的驻留下限（切换推迟到非 play 态时用）。
pub const MIN_DWELL: Duration = Duration::from_secs(10);
/// 冻结的**有界窗口**：到期即强制重解析锚点（不得靠"值看起来合理"退出）。
pub const FREEZE_WINDOW: Duration = Duration::from_secs(30);
/// L0 失败后的退避阶梯（秒），最后一档封顶。
pub const BACKOFF_SECONDS: &[u64] = &[2, 4, 8, 16, 30];
/// 状态整数的**合理区间**上界（观测集 0/1/2/4/5/7；越界 = 指针串了 ⇒ L1 失败）。
pub const STATE_INDEX_MAX: i32 = 15;
/// C# 字符串的码元上限（超过即不可能是路径/元信息，`win.rs` 的硬上限 4096 更宽）。
pub const STRING_MAX_UNITS: usize = 512;
/// id/set 的上界（osu! 现有 id 在 1e8 量级；1e9 留足两个数量级）。
pub const ID_MAX: i32 = 1_000_000_000;
/// 播放时间的合理域（±27 h，毫秒）。
pub const PLAY_TIME_MAX_MS: i32 = 86_400_000;
/// 时间窗（I-05）：`[firstObject - 5000, lastObject + 5000]`。
pub const TIME_WINDOW_MS: i32 = 5_000;
/// 允许的"每 N 秒一次 playTime 回跳"（I-04）：滚动窗口宽度（**不是**计数器）。
pub const BACKWARD_JUMP_WINDOW_MS: u64 = 10_000;
/// I-04 的**进图宽限**：`live` 在进图早期可以是**负数**（时钟从图表时间 0 之前起算），
/// 且真机会话里观测到 `-1746` 这种量级（play 72 帧负值、`<none>` 13 帧）。
/// 取 10 s 而不是 5 s：进图加载期内存里的 `live` 可能仍在上一次的读数上，容差必须覆盖
/// "lead-in + 采样相位差"，否则合法帧会被刷成降级。**不**复用 `TIME_WINDOW_MS`（I-05 的常数）
/// ——两者语义不同，共用一个常数会让"改窗"的副作用悄悄扩散。
pub const PLAY_TIME_LEAD_IN_MS: i32 = 10_000;
/// I-04 的**拖尾宽限**：结算/失败屏里播放时钟可以越过最后一个对象的结束时间。
pub const PLAY_TIME_TAIL_MS: i32 = 5_000;
/// 已定义 mod 位的并集（bit0..=bit30）；之外的位 = 未知位（I-10，软）。
pub const KNOWN_MOD_BITS: u32 = (1u32 << 31) - 1;

/// 一条不变量的台账（`hard`/`soft` 与 `cadence` 都写死，单测逐条钉住）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Invariant {
    /// `I-01`…`I-10`（日志/证据里的稳定编号）。
    pub id: &'static str,
    /// 出问题时报的字段名（进 `degradedFields` / `invariant-failed:<field>`）。
    pub field: &'static str,
    pub kind: Kind,
    pub cadence: Cadence,
    /// 为什么这样判（可独立复核）。
    pub why: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// 结构上可证伪 ⇒ 计 strike，累计到 `INVARIANT_STRIKES` 即 `unhealthy`。
    Hard,
    /// 启发式/派生量 ⇒ 只降级（出帧 + `degradedFields`）。
    Soft,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cadence {
    /// 无状态：每帧都判。
    EveryPoll,
    /// 时序性/跨对象：只在状态迁移那一帧判（避免"进图加载期"的合法越界被当成错误）。
    OnTransition,
}

/// 十项不变量的台账（顺序 = 评估顺序）。
pub const INVARIANTS: &[Invariant] = &[
    Invariant {
        id: "I-01",
        field: "state.name",
        kind: Kind::Hard,
        cadence: Cadence::EveryPoll,
        why: "名字表只能来自 P8 观测集：观测集内 → 正常；观测集外但整数落在 0..=15 → 发 \"\" + 降级（**不判 unhealthy**，否则恰在选歌等日常态冻结）；越界 → 指针串了，判 L1 失败",
    },
    Invariant {
        id: "I-02",
        field: "beatmap.md5",
        kind: Kind::Hard,
        cadence: Cadence::EveryPoll,
        why: "md5 是缓存键的一半（identity 的 hash: 段）：形状必须是 32 位小写十六进制，否则必然与 tosu 逐字节不等",
    },
    Invariant {
        id: "I-03",
        field: "files.beatmap",
        kind: Kind::Soft,
        cadence: Cadence::EveryPoll,
        why: "folder/filename/路径自洽：文件名必须以 .osu 结尾、不含路径分隔符（文件夹名同理）——不一致只降级（少数老图的名字确实很怪，硬判会把它们冻掉）",
    },
    Invariant {
        id: "I-04",
        field: "beatmap.time.live",
        kind: Kind::Soft,
        cadence: Cadence::EveryPoll,
        why: "**时序性软属性**（本步由硬改软，理由见 `History::check_play_time`）：`playTime` 单调，但**每 10 s 允许 1 次回跳**——回跳本身是合法的（重开图/重试把 `live` 归零、选歌 seek 预览、结算屏时钟越过末对象）。只有「10 s 内第二次回跳」或以已知谱面为界的明显越界才上报，且**只降级不停帧**（回跳与「读错字段」在 250 ms 采样下不可区分，故不配 hard）",
    },
    Invariant {
        id: "I-05",
        field: "beatmap.time.window",
        kind: Kind::Soft,
        cadence: Cadence::OnTransition,
        why: "时间窗 [firstObject-5000, lastObject+5000]（lastObject==0 跳过）：只在状态迁移那一帧判——进图加载/前奏的合法负值不该被当成错误",
    },
    Invariant {
        id: "I-06",
        field: "beatmap.object",
        kind: Kind::Hard,
        cadence: Cadence::EveryPoll,
        why: "指针合理性（**状态相关**）：身份路径（Beatmap 对象 + 规则集基址）在任何状态都必须存在且合理；局内链（gameplay/score）只在 play 期望存在、结算链（result）只在 resultScreen 期望存在。当前态**不期望**存在的指针一律不判——菜单/选歌里它们是残留/已释放的槽（真机实测选歌态 `[ruleset+0x64]` 会读到 `0x01000101` 这类非对齐垃圾），判硬失败会让门在日常态反复冻结",
    },
    Invariant {
        id: "I-07",
        field: "beatmap.id",
        kind: Kind::Soft,
        cadence: Cadence::EveryPoll,
        why: "id/set 范围：[-1, 1e9)（-1 = 未提交）；越界只降级（identity 会退化到 hash:/path: 段，仍可用）",
    },
    Invariant {
        id: "I-08",
        field: "string.length",
        kind: Kind::Soft,
        cadence: Cadence::EveryPoll,
        why: "字符串长度 ≤512 码元且合法 UTF-16（`String::from_utf16` 已保证合法性）；超长只降级（`win.rs` 的读取上限 4096 更宽）",
    },
    Invariant {
        id: "I-09",
        field: "beatmap.md5(disk)",
        kind: Kind::Soft,
        cadence: Cadence::EveryPoll,
        why: "内存 md5 vs 磁盘 `.osu` 的 md5（stable 侧**软**：文件可能正在被替换/改名，硬判会误冻；lazer 侧为硬，见 E 步）",
    },
    Invariant {
        id: "I-10",
        field: "mods.unknown-bits",
        kind: Kind::Soft,
        cadence: Cadence::EveryPoll,
        why: "未知 mod 位（bit31 与掩码之外的位）：只降级——页面只认已知位，未知位不影响签名",
    },
];

fn invariant(id: &str) -> &'static Invariant {
    INVARIANTS
        .iter()
        .find(|entry| entry.id == id)
        .expect("invariant id in ledger")
}

// ---- 单帧判定的结果 ----

/// 一帧的 L1+L2 判定（纯数据；`hard` 非空 ⇒ 本帧不可信）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FrameVerdict {
    /// 硬失败（每项计一次 strike；reason = `invariant-failed:<field>`）。
    pub hard: Vec<&'static str>,
    /// 软失败（出帧 + `degradedFields`）。
    pub soft: Vec<&'static str>,
    /// 字段级降级上报（含"观测集外的状态名"与"hits 键序未对拍"两类**不是**不变量失败的上报）。
    pub degraded_fields: Vec<String>,
}

impl FrameVerdict {
    pub fn is_clean(&self) -> bool {
        self.hard.is_empty()
    }

    /// 硬失败对应的 reason（第一个失败项；闭集里的 `invariant-failed:<field>`）。
    pub fn hard_reason(&self) -> Option<Reason> {
        self.hard.first().map(|field| Reason::InvariantFailed(field))
    }
}

/// 跨帧历史（I-04 与迁移检测用；只在门内部存活）。
#[derive(Clone, Debug, Default)]
pub struct History {
    pub last_play_time: Option<i32>,
    /// 上一次**回跳**（`playTime` 后退的那一帧）的时刻（毫秒）——「每 10 s 允许 1 次」的
    /// 配额锚点：只有落在 `BACKWARD_JUMP_WINDOW_MS` **之内**的回跳才有"超额"可言。
    pub last_backward_jump_ms: Option<u64>,
    /// 当前 10 s 滚动窗口内已经用掉的回跳次数（**≥2 才算模式可疑**）。
    ///
    /// 为什么要计数而不是"第二次回跳即失败"：真机序列里同一张图内也会出现两次合法回跳
    /// （`play → resultScreen → selectPlay` 的跨态钟摆，见 `evidence/C2-stable-full/i04-shape-play.txt`），
    /// 所以只有"窗口内反复后退"才值得上报；次数在窗口过期后按时间衰减。
    pub backward_jumps_in_window: u32,
    pub last_state_number: Option<i32>,
    pub last_checksum: Option<String>,
}

impl History {
    /// 新附着/重解析：历史作废（换进程/换图后旧的单调性前提不成立）。
    pub fn reset(&mut self) {
        *self = History::default();
    }

    /// 本帧是否发生了**状态迁移**（`state.number` 变化；首次拿到状态也算）。
    pub fn observe_state(&mut self, number: Option<i32>) -> bool {
        match number {
            Some(number) => {
                let transitioned = self.last_state_number != Some(number);
                self.last_state_number = Some(number);
                transitioned
            }
            None => false,
        }
    }

    /// I-04（**软**）：`playTime` 单调，**每 10 s 允许 1 次回跳**；滚动窗口内反复后退才上报。
    ///
    /// 为什么从 `Hard` 改成 `Soft`（Step 8c 的核心裁决；证据
    /// `temp/osu-native-memory/evidence/C2-stable-full/{i04-trace-play.txt,i04-shape-play.txt}`）：
    /// 上一轮真机会话里 `invariant-failed:beatmap.time.live` 硬失败 **209 帧**（`play-stderr.txt`
    /// 195 条 gate 迁移 + 12 条进入 `unhealthy` 停帧），逐帧追下来**全部**是合法状态：
    ///
    /// | 形态 | 会话实例 | 为什么合法 |
    /// |---|---|---|
    /// | 回跳到 0 | `48101 → 0`、`111632 → 0`（play 态，80 帧） | 重开图/重试：局内时钟从 0 重新起算 |
    /// | 小幅回退 | `-1583 → -1666`（-83 ms）、`0 → -58`（selectPlay） | 进图宽限（负值）+ 采样相位差 |
    /// | 大跨度回退到非零 | `49419 → 30146`、`51264 → 29883` | **seek**（选歌预览/重新进图）——`playTime` 是播放位置，不是进度条 |
    /// | 结算屏钟摆 | `52285 → 49419`（resultScreen） | 结算动画期间时钟本身会回退 |
    ///
    /// 关键事实：250 ms 采样下"合法 seek"与"读错字段"在**局部**不可区分（两者都是"时间倒退"），
    /// 而结构侧已有两道硬保护——L0 的 `playTimeAddr` 候选自证（`mod.rs::anchor_proves_out`
    /// 要求槽落区 + 值在 ±27 h 毫秒域）与读取链本身。因此**不存在可证明的 hard 情形**：
    /// 本不变量只降级、不停帧（软失败仍出帧，页面拿到的是真实读数）。
    ///
    /// 返回 `Some(field)` = 本帧上报降级（调用方写进 `soft`，**不**累加 strike）。
    pub fn check_play_time(&mut self, live: Option<i32>, now_ms: u64) -> Option<&'static str> {
        let current = match live {
            Some(value) => value,
            None => return None,
        };
        let previous = match self.last_play_time {
            Some(value) => value,
            None => {
                self.last_play_time = Some(current);
                return None;
            }
        };
        self.last_play_time = Some(current);
        if current > previous {
            // 前进：窗口按时间衰减（超过 10 s 的回跳自然出窗）。
            if let Some(at) = self.last_backward_jump_ms {
                if now_ms.saturating_sub(at) >= BACKWARD_JUMP_WINDOW_MS {
                    self.last_backward_jump_ms = None;
                    self.backward_jumps_in_window = 0;
                }
            }
            return None;
        }
        if current == previous {
            // 停滞不是回跳：暂停/卡顿语义由 `game.paused`（停滞推导）负责。
            return None;
        }
        // 回跳。窗口已过期 ⇒ 重新起算（这一次是"每 10 s 的那一次"，允许）。
        if let Some(at) = self.last_backward_jump_ms {
            if now_ms.saturating_sub(at) >= BACKWARD_JUMP_WINDOW_MS {
                self.last_backward_jump_ms = None;
                self.backward_jumps_in_window = 0;
            }
        }
        self.backward_jumps_in_window += 1;
        if self.last_backward_jump_ms.is_none() {
            self.last_backward_jump_ms = Some(now_ms);
        }
        if self.backward_jumps_in_window >= 2 {
            // 窗口内已用过配额 ⇒ 上报降级（软）。
            return Some(invariant("I-04").field);
        }
        None
    }

    /// I-04 的**范围**判据（软；与 [`History::check_play_time`] 合起来就是全部 I-04）：
    /// 已知谱面时 `live` 必须落在 `[firstObject - 5 s, lastObject + 5 s]`；未知谱面时只判
    /// 全局域（`±27 h`，与 L0 的候选自证同域）。菜单态（`live == 0`）**不**参与范围判据。
    pub fn play_time_in_range(&self, snapshot: &Snapshot, live: i32) -> bool {
        if live.unsigned_abs() > PLAY_TIME_MAX_MS as u32 {
            return false;
        }
        let (Some(first), Some(last)) = (snapshot.first_object(), snapshot.last_object()) else {
            return true;
        };
        if last <= 0 {
            return true;
        }
        live >= first.saturating_sub(PLAY_TIME_LEAD_IN_MS)
            && live <= last.saturating_add(PLAY_TIME_TAIL_MS)
    }
}

// ---- 逐条不变量（纯谓词；表驱动单测的入口）----

/// I-01：状态名（观测集内 → 正常；观测集外但整数合理 → 降级；越界 → 硬失败）。
pub fn i01_state_name(snapshot: &Snapshot) -> FrameVerdict {
    let mut verdict = FrameVerdict::default();
    let (Some(number), Some(name)) = (snapshot.state_number, snapshot.state_name.as_deref()) else {
        return verdict;
    };
    if !name.is_empty() {
        return verdict;
    }
    if (0..=STATE_INDEX_MAX).contains(&number) {
        verdict.soft.push(invariant("I-01").field);
        verdict.degraded_fields.push("state.name".to_string());
    } else {
        verdict.hard.push(invariant("I-01").field);
    }
    verdict
}

/// I-02：md5 形状 `^[0-9a-f]{32}$`（无图时跳过）。
pub fn i02_md5_shape(checksum: Option<&str>) -> FrameVerdict {
    let mut verdict = FrameVerdict::default();
    let Some(checksum) = checksum else {
        return verdict;
    };
    if checksum.len() != 32 || !checksum.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
        verdict.hard.push(invariant("I-02").field);
    }
    verdict
}

/// 单个文件名的形状（I-03 的积木）：非空、以 `.osu` 结尾、不含分隔符/冒号。
pub fn filename_is_plausible(filename: &str) -> bool {
    !filename.is_empty()
        && filename.to_lowercase().ends_with(".osu")
        && !filename.contains(['/', '\\', ':'])
}

/// 文件夹名的形状（I-03 的积木）：可为空（lazer 的空 folder），但不得含分隔符/冒号。
pub fn folder_is_plausible(folder: &str) -> bool {
    !folder.contains(['/', '\\', ':'])
}

/// I-03：folder / filename / 派生路径的自洽（软）。
pub fn i03_path_consistency(snapshot: &Snapshot) -> FrameVerdict {
    let mut verdict = FrameVerdict::default();
    let mut bad = false;
    if let Some(filename) = snapshot.filename.as_deref() {
        if !filename_is_plausible(filename) {
            bad = true;
        }
    }
    if let Some(folder) = snapshot.folder.as_deref() {
        if !folder_is_plausible(folder) {
            bad = true;
        }
    }
    // 派生路径必须与两个输入自洽（`folder\filename`；空 folder ⇒ 只剩 filename）。
    if let (Some(folder), Some(filename)) = (snapshot.folder.as_deref(), snapshot.filename.as_deref()) {
        let expected = if folder.is_empty() {
            filename.to_string()
        } else {
            format!("{folder}\\{filename}")
        };
        let payload = snapshot.to_packet();
        if let Some(actual) = payload.pointer("/directPath/beatmapFile").and_then(|v| v.as_str()) {
            if actual != expected {
                bad = true;
            }
        }
    }
    if bad {
        verdict.soft.push(invariant("I-03").field);
    }
    verdict
}

/// I-05：时间窗（软，迁移期才判）。`lastObject == 0` / 缺 `.osu` / 缺 live ⇒ 跳过。
///
/// **状态门（Step 8c 新增，与 I-04 的范围那一半同一理由）**：只在 `play`/`resultScreen`
/// 判。真机会话里 `menu → selectPlay` 那一帧的 `live` 往往是 0 或上一张图的读数（121 帧里
/// 2 帧为 0、其余跨了 5 张图），而解析出的 `.osu` 可能还是**上一次**选中/游玩的那张
/// ——拿 A 图的窗口去判 B 图的时间会稳定误报（软失败会把 `degradedFields` 刷成噪声）。
/// 这两个状态是图表时间的唯二消费态（`socketHandlers.js` 的 `isInPlayState`）。
pub fn i05_time_window(snapshot: &Snapshot) -> FrameVerdict {
    let mut verdict = FrameVerdict::default();
    if !(snapshot.is_play_state() || snapshot.is_result_state()) {
        return verdict;
    }
    let (Some(live), Some(first), Some(last)) = (
        snapshot.play_time,
        snapshot.first_object(),
        snapshot.last_object(),
    ) else {
        return verdict;
    };
    if last == 0 {
        return verdict;
    }
    let low = first.saturating_sub(TIME_WINDOW_MS);
    let high = last.saturating_add(TIME_WINDOW_MS);
    if live < low || live > high {
        verdict.soft.push(invariant("I-05").field);
    }
    verdict
}

/// 单个指针是否合理（32 位用户态最小判据）：非空、4 字节对齐、≥ 64 KiB。
pub fn pointer_is_plausible(pointer: u32) -> bool {
    pointer != 0 && pointer % 4 == 0 && pointer >= 0x1_0000
}

/// I-06：指针合理性（硬）的**状态相关期望集**：哪些指针在**当前状态**下必须存在且合理。
///
/// 为什么必须按状态分（本步的核心裁决）：`gameplay_base`/`score_base` 只在 play/resultScreen
/// 是活的——菜单/选歌里 `[ruleset+0x64]` 是**残留/已释放**的槽（C2 真机会话实测选歌态读到
/// `gameplay=0x2613CABC` → `score=0x01000101` 这种非对齐垃圾），而 tosu 的链 dump 在同一态给出
/// `gameplay_raw = 0x0`。把"当前态根本不期望存在"的指针判成硬失败，会让门在**日常态（选歌）**
/// 每 30 s 冻结—重扫一次，全部载荷停发（真机会话实测 1833+ 帧 `invariant-failed:beatmap.object`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExpectedPointers {
    pub beatmap_object: bool,
    pub ruleset_base: bool,
    pub gameplay_base: bool,
    pub score_base: bool,
    pub result_base: bool,
}

impl ExpectedPointers {
    /// 身份路径：任何状态都必须成立的四个字段（`beatmap.md5`/`files.beatmap`/… 的来源）。
    pub const IDENTITY: ExpectedPointers = ExpectedPointers {
        beatmap_object: true,
        ruleset_base: true,
        gameplay_base: false,
        score_base: false,
        result_base: false,
    };
}

impl Default for ExpectedPointers {
    fn default() -> Self {
        ExpectedPointers::IDENTITY
    }
}

/// 当前状态期望哪些指针（**唯一权威**，单测逐状态钉住）：
///
/// | 状态 | beatmap | ruleset | gameplay | score | result |
/// |---|---|---|---|---|---|
/// | `play` | ✓ | ✓ | ✓ | ✓ | – |
/// | `resultScreen` | ✓ | ✓ | – | – | ✓ |
/// | 其它（menu/selectPlay/edit/…、状态名未知） | ✓ | ✓ | – | – | – |
///
/// 结算态只期望**结算链**：真机会话 13 帧结算态里 `score_base` **11 帧为 0**、`gameplay_base`
/// 8 帧合理/4 帧非对齐垃圾（结算时玩法/分数对象正在被释放，活着的是 `[ruleset+0x38]`）。
/// 把这两枚列进期望集会让结算屏稳定硬失败 → 停帧（正是本步要修的那类回归）；
/// 而结算态真正发布的 `resultsScreen.{mods,hits}` 只依赖 `result_base` ⇒ 期望集就是它。
/// play 态则相反：`gameplay_base`/`score_base` 是 `play.mods`/`play.hits` 的唯一来源，
/// 缺失/垃圾必须硬失败（"签名读错"的真信号）。
///
/// 状态名未知（`state_name` 为 `""`/`None`）时按"其它态"处理：宁可少判一个签名错误，
/// 也不在状态读坏的那一帧把整个门打成不健康（读坏状态本身已由 I-01 上报）。
pub fn expected_pointers(snapshot: &Snapshot) -> ExpectedPointers {
    let mut expected = ExpectedPointers::IDENTITY;
    if snapshot.is_play_state() {
        expected.gameplay_base = true;
        expected.score_base = true;
    } else if snapshot.is_result_state() {
        expected.result_base = true;
    }
    expected
}

/// I-06：**状态相关**的指针合理性（硬）。规则三条：
///
/// ① 期望的指针必须存在且合理（`Some` + [`pointer_is_plausible`]）——这是"签名读错"的真信号
///    （play 态分数对象读到 `0x01000101` 这类非对齐值 ⇒ 停帧）；
/// ② **不期望**的指针一律不判：它可能合法缺失（`None`），也可能是残留/已释放的垃圾值；
/// ③ 不期望指针的**依赖字段**不因此在载荷里出现——发布面由 [`play_chain_valid`] /
///    [`result_chain_valid`] 与 [`mask_for_state`]/[`play_hits_publishable`] 负责
///    （载荷里既没有垃圾 mods，也没有垃圾 hits）。
pub fn i06_pointers(snapshot: &Snapshot) -> FrameVerdict {
    let mut verdict = FrameVerdict::default();
    let expected = expected_pointers(snapshot);
    let checks = [
        (expected.beatmap_object, snapshot.beatmap_object),
        (expected.ruleset_base, snapshot.ruleset_base),
        (expected.gameplay_base, snapshot.gameplay_base),
        (expected.score_base, snapshot.score_base),
        (expected.result_base, snapshot.result_base),
    ];
    if checks
        .into_iter()
        .any(|(expected, pointer)| expected && !pointer.map_or(false, pointer_is_plausible))
    {
        verdict.hard.push(invariant("I-06").field);
    }
    verdict
}

/// 局内链（`gameplay → score`）**结构可信**：分数对象指针非空/对齐/≥64 KiB。
///
/// 依赖字段 = `play.mods` 与 `play.hits`：链不可信时它们**不发布**（既不是"沿用上一帧"，
/// 也不是"发半截"）——菜单/选歌里这条槽是残留指针，读出来的东西与当前局无关。
pub fn play_chain_valid(snapshot: &Snapshot) -> bool {
    snapshot.score_base.map_or(false, pointer_is_plausible)
}

/// 结算链（`result`）**结构可信**：结算对象指针非空/对齐/≥64 KiB。
pub fn result_chain_valid(snapshot: &Snapshot) -> bool {
    snapshot.result_base.map_or(false, pointer_is_plausible)
}

/// `play.hits` 的发布门（**三道门全过**，纯函数）：
/// ① 状态门 —— 只有 `play`/`resultScreen` 才发（其余态内存里是上一局的残留，tosu 发全零）；
/// ② 链有效性门 —— 局内链可信 **且** 8 个候选槽整块读满（绝不发半截值）；
/// ③ 新鲜度门 —— `live >= firstObject - 100`（进图加载期的计数域还是上一局的残留）。
pub fn play_hits_publishable(snapshot: &Snapshot) -> bool {
    (snapshot.is_play_state() || snapshot.is_result_state())
        && play_chain_valid(snapshot)
        && snapshot.hits_candidates_complete
        && stable::hits_gate_passes(snapshot.play_time.unwrap_or(0), snapshot.first_object())
}

/// `resultsScreen.hits` 的发布门：状态 = `resultScreen` ∧ 结算链可信 ∧ 候选槽整块读满。
pub fn result_hits_publishable(snapshot: &Snapshot) -> bool {
    snapshot.is_result_state()
        && result_chain_valid(snapshot)
        && snapshot.result_hits_candidates_complete
}

/// I-07：id/set 范围（软）。`-1` 是"未提交"的合法值。
pub fn id_in_range(id: i32) -> bool {
    id >= -1 && id < ID_MAX
}

pub fn i07_id_range(snapshot: &Snapshot) -> FrameVerdict {
    let mut verdict = FrameVerdict::default();
    let mut bad = false;
    if let Some(id) = snapshot.map_id {
        if !id_in_range(id) {
            bad = true;
        }
    }
    if let Some(set) = snapshot.set_id {
        if !id_in_range(set) {
            bad = true;
        }
    }
    if bad {
        verdict.soft.push(invariant("I-07").field);
    }
    verdict
}

/// I-08：字符串长度 ≤512 码元（软）。UTF-16 合法性由 `String` 类型本身保证。
pub fn i08_strings(snapshot: &Snapshot) -> FrameVerdict {
    let mut verdict = FrameVerdict::default();
    let mut bad = false;
    for value in [
        snapshot.checksum.as_deref(),
        snapshot.filename.as_deref(),
        snapshot.folder.as_deref(),
        snapshot.version.as_deref(),
        snapshot.artist.as_deref(),
        snapshot.title.as_deref(),
        snapshot.mapper.as_deref(),
        snapshot.songs_folder.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        if value.encode_utf16().count() > STRING_MAX_UNITS {
            bad = true;
        }
    }
    if bad {
        verdict.soft.push(invariant("I-08").field);
    }
    verdict
}

/// I-09：内存 md5 vs 磁盘 md5（stable 侧软）。缺任一侧即跳过。
pub fn i09_md5_disk(snapshot: &Snapshot) -> FrameVerdict {
    let mut verdict = FrameVerdict::default();
    let (Some(memory), Some(disk)) = (
        snapshot.checksum.as_deref(),
        snapshot.beatmap_file_md5.as_deref(),
    ) else {
        return verdict;
    };
    if !memory.eq_ignore_ascii_case(disk) {
        verdict.soft.push(invariant("I-09").field);
    }
    verdict
}

/// I-10：未知 mod 位（软）。三个来源（菜单/局内/结算）里任一含未知位即降级。
pub fn i10_mod_bits(snapshot: &Snapshot) -> FrameVerdict {
    let mut verdict = FrameVerdict::default();
    let unknown = [
        snapshot.menu_mods_mask,
        snapshot.play_mods_mask,
        snapshot.result_mods_mask,
    ]
    .into_iter()
    .flatten()
    .fold(0u32, |acc, mask| acc | (mask & !KNOWN_MOD_BITS));
    if unknown != 0 {
        verdict.soft.push(invariant("I-10").field);
        verdict.degraded_fields.push(format!("mods.unknown-bits:0x{unknown:08X}"));
    }
    verdict
}

/// 一帧的完整 L1+L2 判定。
///
/// `transitioned` = 本帧发生了状态迁移（`Cadence::OnTransition` 的不变量只在这时为真）。
pub fn evaluate(
    snapshot: &Snapshot,
    history: &mut History,
    now_ms: u64,
    transitioned: bool,
) -> FrameVerdict {
    let mut verdict = FrameVerdict::default();
    merge(&mut verdict, i01_state_name(snapshot));
    merge(&mut verdict, i02_md5_shape(snapshot.checksum.as_deref()));
    merge(&mut verdict, i03_path_consistency(snapshot));
    // I-04（**软**）：时序那一半 + 范围那一半（后者只在 play/resultScreen 生效）。
    merge(&mut verdict, i04_play_time(snapshot, history, now_ms));
    if transitioned {
        merge(&mut verdict, i05_time_window(snapshot));
    }
    merge(&mut verdict, i06_pointers(snapshot));
    merge(&mut verdict, i07_id_range(snapshot));
    merge(&mut verdict, i08_strings(snapshot));
    merge(&mut verdict, i09_md5_disk(snapshot));
    merge(&mut verdict, i10_mod_bits(snapshot));
    // hits 键序未对拍（非不变量失败，但必须上报——读者要能一眼看出）
    if snapshot.play_hits.is_some() || snapshot.result_hits.is_some() {
        if !stable::HITS_MAPPING_VERIFIED {
            verdict
                .degraded_fields
                .push("play.hits.mapping".to_string());
        }
    }
    verdict
}

/// I-04 的整条判定（**软**）：时序半（滚动窗口，见 [`History::check_play_time`]）+
/// 范围半（[`History::play_time_in_range`]，只在 `play`/`resultScreen`）。
///
/// 两半可以同时命中（"窗口内第二次回跳"且回跳后的值又落在谱面窗外）⇒ 字段按**去重**上报，
/// `degraded_fields` 则分别列出两边观察到的值（读者要知道"是哪种越界"）。
pub fn i04_play_time(snapshot: &Snapshot, history: &mut History, now_ms: u64) -> FrameVerdict {
    let mut verdict = FrameVerdict::default();
    if let Some(field) = history.check_play_time(snapshot.play_time, now_ms) {
        verdict.soft.push(field);
    }
    if let Some(live) = snapshot.play_time {
        if (snapshot.is_play_state() || snapshot.is_result_state())
            && !history.play_time_in_range(snapshot, live)
        {
            verdict.soft.push(invariant("I-04").field);
            verdict
                .degraded_fields
                .push(format!("beatmap.time.live:out-of-window:{live}"));
        }
    }
    verdict
}

/// 合并两份判定：`hard` 逐条保留，`soft` 按字段**去重**（同一字段的两半只报一次），
/// `degraded_fields` 全部保留（不同的观察值都要能看到）。
fn merge(into: &mut FrameVerdict, from: FrameVerdict) {
    into.hard.extend(from.hard);
    for field in from.soft {
        if !into.soft.contains(&field) {
            into.soft.push(field);
        }
    }
    into.degraded_fields.extend(from.degraded_fields);
}

// ---- 健康状态机 ----

/// 四态（§3.4）：`idle`（没目标）/`healthy`/`degraded`（出帧 + 上报缺字段）/`unhealthy`（不出帧）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HealthState {
    Idle,
    Healthy,
    Degraded,
    Unhealthy,
}

impl HealthState {
    pub fn as_str(&self) -> &'static str {
        match self {
            HealthState::Idle => "idle",
            HealthState::Healthy => "healthy",
            HealthState::Degraded => "degraded",
            HealthState::Unhealthy => "unhealthy",
        }
    }
}

/// 本帧的动作（调用方照此处理载荷）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FrameAction {
    /// 全量出帧（可能带 `degradedFields`）。
    Publish,
    /// **字段级冻结**：只发 `state.name`（省略 `beatmap`）——整包停发会让页面 L1 的
    /// `isInPlayState` 失效、60 s 窗口过期把路由甩离 osu。
    FreezeStateOnly,
    /// `unhealthy`：**不出帧**（消费方拿不到载荷，只有 reason）。默认值（`FrameOutcome::default()`）。
    #[default]
    Stop,
}

/// 一帧的门输出。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FrameOutcome {
    pub action: FrameAction,
    pub state: Option<HealthState>,
    /// 本帧的 reason（`None` = 正常发布）。
    pub reason: Option<Reason>,
    /// 本帧的降级字段（出帧时随载荷上报）。
    pub degraded_fields: Vec<String>,
    /// 状态机**迁移**的 reason（`None` = 本帧没有迁移，或迁移到"正常"一侧）。
    ///
    /// 口径（如实记录）：闭集里的每个字面量都是"出了什么问题"；因此
    /// 迁移到 `healthy`（以及只因软失败进入 `degraded`）不带 reason——
    /// 那两种情况由 `degradedFields` 或"什么都没有"解释。硬失败冻结、
    /// L0 失败、迁移到 `unhealthy` 一律带闭集里的 reason。
    pub transition: Option<Reason>,
}

/// 健康状态机（**纯逻辑**：时间用 `now_ms` 传入，可单测）。
#[derive(Clone, Debug)]
pub struct Gate {
    state: HealthState,
    strikes: u32,
    clean_frames: u32,
    frozen: bool,
    frozen_since_ms: Option<u64>,
    freeze_reason: Option<Reason>,
    last_reason: Option<Reason>,
    degraded: Vec<String>,
    history: History,
}

impl Default for Gate {
    fn default() -> Self {
        Gate::new()
    }
}

impl Gate {
    pub fn new() -> Gate {
        Gate {
            state: HealthState::Idle,
            strikes: 0,
            clean_frames: 0,
            frozen: false,
            frozen_since_ms: None,
            freeze_reason: None,
            last_reason: None,
            degraded: Vec::new(),
            history: History::default(),
        }
    }

    pub fn state(&self) -> HealthState {
        self.state
    }

    pub fn frozen(&self) -> bool {
        self.frozen
    }

    pub fn strikes(&self) -> u32 {
        self.strikes
    }

    pub fn clean_frames(&self) -> u32 {
        self.clean_frames
    }

    pub fn reason(&self) -> Option<String> {
        self.last_reason.as_ref().map(Reason::as_str)
    }

    pub fn reason_value(&self) -> Option<Reason> {
        self.last_reason.clone()
    }

    pub fn degraded_fields(&self) -> Vec<String> {
        self.degraded.clone()
    }

    pub fn history(&self) -> &History {
        &self.history
    }

    pub fn history_mut(&mut self) -> &mut History {
        &mut self.history
    }

    /// **L0 解析成功**（新附着或重解析完成）：历史与冻结计数器归零；`frozen` 保持——
    /// 若刚从冻结里出来，仍要 `RECOVERY_CLEAN_FRAMES` 帧清白才对外全量出帧
    /// （"值看起来合理"从来不是退出条件，只有重解析 + 清白帧是）。
    ///
    /// 有界窗口的计时器在这里**清零**（窗口已被这次重解析消费掉）：若重解析之后值仍然是坏的，
    /// 第一次硬失败会重新起算窗口（见 `on_frame`），不会退化成"每 tick 都重解析"。
    pub fn on_anchor_resolved(&mut self, _now_ms: u64) {
        self.history.reset();
        self.strikes = 0;
        self.clean_frames = 0;
        self.frozen_since_ms = None;
    }

    /// **L0 解析失败**：`unhealthy` + 不出帧；reason 直接来自闭集
    /// （`signature-miss:<key>` / `access-denied` / …）。
    pub fn on_anchor_failure(&mut self, reason: Reason, _now_ms: u64) -> FrameOutcome {
        let transition = (self.state != HealthState::Unhealthy).then(|| reason.clone());
        self.state = HealthState::Unhealthy;
        self.strikes = INVARIANT_STRIKES;
        // 冻结标志保持：即使 L0 恢复了，也要先满足"重解析 + 清白帧"。
        if !self.frozen {
            self.frozen = true;
            self.frozen_since_ms = Some(_now_ms);
        }
        self.freeze_reason = Some(reason.clone());
        self.last_reason = Some(reason.clone());
        FrameOutcome {
            action: FrameAction::Stop,
            state: Some(self.state),
            reason: Some(reason),
            degraded_fields: self.degraded.clone(),
            transition,
        }
    }

    /// 一帧的 L1+L2 判定 + 状态迁移。
    pub fn on_frame(&mut self, snapshot: &Snapshot, now_ms: u64) -> FrameOutcome {
        let transitioned = self.history.observe_state(snapshot.state_number);
        let verdict = evaluate(snapshot, &mut self.history, now_ms, transitioned);

        if !verdict.is_clean() {
            self.strikes += 1;
            self.clean_frames = 0;
            self.frozen = true;
            if self.frozen_since_ms.is_none() {
                // 窗口起算点：第一次硬失败（或重解析之后的第一次硬失败）。
                self.frozen_since_ms = Some(now_ms);
            }
            let reason = verdict
                .hard_reason()
                .unwrap_or_else(|| Reason::InvariantFailed("unknown"));
            self.freeze_reason = Some(reason.clone());
            self.last_reason = Some(reason.clone());
            self.degraded = verdict.degraded_fields.clone();
            let previous = self.state;
            self.state = if self.strikes >= INVARIANT_STRIKES {
                HealthState::Unhealthy
            } else {
                HealthState::Degraded
            };
            let transition = (previous != self.state).then(|| reason.clone());
            let action = if self.state == HealthState::Unhealthy {
                FrameAction::Stop
            } else {
                FrameAction::FreezeStateOnly
            };
            return FrameOutcome {
                action,
                state: Some(self.state),
                reason: Some(reason),
                degraded_fields: verdict.degraded_fields,
                transition,
            };
        }

        // 清白帧：冻结未解除前只累计，不擅自解除（见 `should_re_resolve`）。
        self.clean_frames += 1;
        self.degraded = verdict.degraded_fields.clone();
        let previous = self.state;
        let (action, state) = if self.frozen {
            if self.clean_frames >= RECOVERY_CLEAN_FRAMES {
                self.frozen = false;
                self.frozen_since_ms = None;
                self.freeze_reason = None;
                self.strikes = 0;
                let state = if verdict.soft.is_empty() {
                    HealthState::Healthy
                } else {
                    HealthState::Degraded
                };
                self.last_reason = None;
                (FrameAction::Publish, state)
            } else {
                (FrameAction::FreezeStateOnly, HealthState::Degraded)
            }
        } else {
            let state = if verdict.soft.is_empty() {
                HealthState::Healthy
            } else {
                HealthState::Degraded
            };
            self.last_reason = None;
            (FrameAction::Publish, state)
        };
        self.state = state;
        // 迁移的 reason：只有**硬失败冻结**与 **L0 失败**带闭集 reason；软降级与
        // 恢复到 healthy 不带（口径见 `FrameOutcome::transition` 的注释）。
        let transition = match (previous, state) {
            (HealthState::Healthy | HealthState::Idle, HealthState::Degraded) => None,
            (_, HealthState::Degraded) => self.freeze_reason.clone(),
            (_, HealthState::Healthy) => None,
            (_, HealthState::Idle) => None,
            (_, HealthState::Unhealthy) => self.last_reason.clone(),
        };
        FrameOutcome {
            action,
            state: Some(state),
            reason: self.last_reason.clone(),
            degraded_fields: verdict.degraded_fields,
            transition,
        }
    }

    /// **有界窗口**：冻结/不健康持续超过 `FREEZE_WINDOW` ⇒ 调用方必须重解析锚点
    /// （这是"退出冻结"的两条合法路径之一；另一条是外部原因导致的重解析）。
    pub fn should_re_resolve(&self, now_ms: u64) -> bool {
        matches!(self.frozen_since_ms, Some(at) if now_ms.saturating_sub(at) >= FREEZE_WINDOW.as_millis() as u64)
    }

    /// 距离下一次该重解析还有多久（毫秒）；未冻结 ⇒ `None`。
    pub fn re_resolve_due_in_ms(&self, now_ms: u64) -> Option<u64> {
        let at = self.frozen_since_ms?;
        Some((FREEZE_WINDOW.as_millis() as u64).saturating_sub(now_ms.saturating_sub(at)))
    }
}

/// 退避阶梯（L0 连续失败时用）：第 `attempt` 次失败（0 起）→ 2/4/8/16/30 s。
pub fn backoff(attempt: usize) -> Duration {
    let seconds = BACKOFF_SECONDS[attempt.min(BACKOFF_SECONDS.len() - 1)];
    Duration::from_secs(seconds)
}

/// 载荷里的 `play.mods`/`resultsScreen.mods`/`menu.mods` 是否已经"当前态"可信。
///
/// 存在的意义：**mods 属缓存键，不得沿用上一个签名**（§3.4）——所以缺失就是缺失，
/// 由 `packet.rs` 发 `null`，绝不重发上一帧的掩码。
///
/// 两道门（本步加固）：
/// ① **状态门**：局内掩码只在 `play`/`resultScreen` 发、结算掩码只在 `resultScreen` 发；
/// ② **链有效性门**：局内/结算链不可信（指针缺失或非对齐垃圾）时同样发 `null`——
///    菜单/选歌里这两个槽是残留指针，读出来的掩码与当前局无关（真机实测选歌态
///    `score_base = 0x01000101`），绝不能进载荷。
pub fn mask_for_state(snapshot: &Snapshot) -> (Option<u32>, Option<u32>, Option<u32>) {
    let play = if (snapshot.is_play_state() || snapshot.is_result_state()) && play_chain_valid(snapshot)
    {
        snapshot.play_mods_mask
    } else {
        None
    };
    let result = if snapshot.is_result_state() && result_chain_valid(snapshot) {
        snapshot.result_mods_mask
    } else {
        None
    };
    (snapshot.menu_mods_mask, play, result)
}

/// 页面会看到的 mod 码集合（`keys.rs` 的同一套规则）——给对拍/日志用。
pub fn page_mod_codes(snapshot: &Snapshot) -> Vec<&'static str> {
    keys::mod_codes_from_payload(
        &snapshot.to_packet(),
        snapshot.client.unwrap_or(crate::osu::model::Client::Stable),
    )
}

/// 空 `Hits`（`livePp.js` 的首帧回退语义）——诊断与单测的常量。
pub fn zero_hits() -> Hits {
    Hits::default()
}

#[cfg(test)]
#[path = "../../tests-local/osu_invariants.rs"]
mod tests_invariants;
