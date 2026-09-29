// 目标发现（进程选择）的**纯逻辑**层：候选集 → 决策（Step 9b；Step 10B 加入位数分派）。
//
// 为什么独立成层：进程发现此前是 `win.rs` 里"一次扫描定生死"的内联逻辑，而兜底 PID 扫描
// 在受限会话（自动化沙箱/受限令牌）里会为**不存在的 PID** 开出指向**同一个真进程**的句柄
// ——B2 实测：几乎每个进程都有 n+1..n+3 三个"幻觉坐标"
// （`temp/osu-native-memory/evidence/B2-stable-minimal/diag-phantom.txt`）。这些坐标的指纹
// （映像路径 + 区域数 + 私有提交）在扫描过程中会**漂移**，于是"坐标去重"有时合并、有时不
// 合并：真机日志里同一份候选先 `stable=2` 后 `stable=1`。旧逻辑对**单次**扫描直接判
// `multiple-instances`，把一个健康目标挡在门外（Step 9b 缺陷 ②：卡在 `multiple-instances`）。
//
// 判据（写死，单测逐条钉住；`tests-local/osu_discovery.rs`）：
// 1. **Toolhelp32 枚举是权威的**：它枚举的是**进程**（不是"试开坐标"），不会出现幻觉坐标
//    ⇒ ≥2 个同类候选 = 真的有两个实例，按既有的规模规则判（分不出 ⇒ `multiple-instances`）。
// 2. **兜底 PID 扫描只信"确认过"的候选**：该 PID 在 `PROCESSENTRY32W` 里（`threads > 0`）
//    ⇒ 是真进程；幻觉坐标不在进程表里（`threads == 0`，B2 实测真 91 / 幻觉 0）。有确认候选
//    时**只看它们**——这条在"进程表被截断但按 PID 仍能查到条目"的上下文里一步定音。
// 3. **进程表完全读不到时**（连真进程也不在表里，B2 的"35 个进程"上下文）：要求**同一候选
//    集连续两次扫描**才升级 `multiple-instances`；首次歧义**不升级**，而是
//    ① 采纳"跨两次扫描都在"的那一个候选（幻觉坐标每次都在变），② 跨次还不重合的返
//    `Retry`（调用方很快重扫）——这就是"绝不从一次抖动的扫描升级"的实现面。
//
// **位数分派（Step 10B / DEC-19）**：stable = 32 位、lazer = 64 位，**同进程名 `osu!.exe`**。
// 分派表（`dispatch`，表驱动单测逐行钉住）：
//
// | 32 位候选 | 64 位候选 | 决策 |
// |---|---|---|
// | 1 | 任意 | `Attach{ Stable }`（唯一化的 32 位实例优先——stable 路径是已验证路径） |
// | ≥2 | 任意 | 既有规模规则（分不出 ⇒ `MultipleInstances`） |
// | 0 | 1 | **`Attach{ Lazer }`**（Step 10B 的修复点：旧规则在这里报 `client-ambiguous`） |
// | 0 | ≥2 | 既有规模规则（分不出 ⇒ `MultipleInstances`） |
// | 0 | 0 | `Toolhelp` ⇒ `ClientAmbiguous`；兜底扫描 ⇒ `Retry` |
//
// 与 `win.rs` 的分工：本文件**不碰任何句柄/系统调用**（纯数据进、纯数据出，可离线单测）；
// `win.rs` 只负责"枚举 → 组装本层的输入 → 按本层的决策开句柄"。

use crate::osu::model::{Client, Reason};

/// 候选来自哪条发现路径（决定"候选数的可信度"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// `Toolhelp32` 枚举：权威（枚举的是进程，不会出现幻觉坐标）。
    Toolhelp,
    /// 兜底 PID 扫描（`Toolhelp32` 一个 `osu!.exe` 都没看到）：可能出现同一进程的多个坐标。
    PidSweep,
}

/// 一个 `osu!.exe` 候选（纯数据；不含句柄）。
///
/// 位数不在这里：候选按位数分列（`Discovery::stable` / `Discovery::lazer`），分派是纯计数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub pid: u32,
    /// 私有提交字节数（`GetProcessMemoryInfo`）。
    pub private_bytes: u64,
    /// `PROCESSENTRY32W.cntThreads`（0 = 该 PID **不在**进程表里 = 幻觉坐标）。
    pub threads: u32,
}

/// 一次发现的候选集。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Discovery {
    /// 32 位候选（PE machine `0x014C`）⇒ stable 读取路径。
    pub stable: Vec<Candidate>,
    /// 64 位候选（PE machine `0x8664`）⇒ lazer 读取路径（Step 10B）。
    pub lazer: Vec<Candidate>,
    pub source: Source,
}

impl Discovery {
    /// 两个位数桶里的全部候选（升序 PID；集合比较必须与枚举顺序无关）。
    pub fn all_pids(&self) -> Vec<u32> {
        let mut pids: Vec<u32> = self
            .stable
            .iter()
            .chain(self.lazer.iter())
            .map(|candidate| candidate.pid)
            .collect();
        pids.sort_unstable();
        pids
    }
}

/// 决策（调用方照此开句柄 / 报 reason）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    /// 唯一目标：附着它（`client` 决定随后走哪条读取路径）。
    Attach { pid: u32, client: Client },
    /// **不猜、也不升级**：候选集还在抖、或进程表读不到 ⇒ 调用方很快重扫一次。
    Retry,
    /// ≥2 个真目标：绝不猜跟哪一个。
    MultipleInstances,
    /// 看到过 `osu!.exe` 但一个候选都分不出（`Toolhelp32` 权威枚举下的空集）。
    ClientAmbiguous,
}

/// "像游戏本体"的私有提交下限（既有判据，原样保留）：真机实测 stable 主进程约 600 MiB，
/// 同映像的辅助进程在 10 MiB 量级。
pub const GAME_MIN_PRIVATE_BYTES: u64 = 128 * 1024 * 1024;

/// 决策 → 壳侧 reason（**沿用既有闭集**，不新增字面量）：`Attach` ⇒ `None`（要开句柄，
/// 开句柄失败的 reason 由 `win.rs` 报）。单测逐条钉住映射，防止"悄悄新增一个字面量"。
pub fn reason_for(decision: Decision) -> Option<Reason> {
    match decision {
        Decision::Attach { .. } => None,
        Decision::Retry => Some(Reason::ProcessNotFound),
        Decision::MultipleInstances => Some(Reason::MultipleInstances),
        Decision::ClientAmbiguous => Some(Reason::ClientAmbiguous),
    }
}

/// 跨次扫描的候选集记忆（**进程选择的唯一状态**；调用方按线程持有）。
///
/// 只保存"上一次的候选 PID 集合"与"同一集合连续出现了几次"——不含时间，故可离线单测。
#[derive(Clone, Debug, Default)]
pub struct Stability {
    previous: Option<Vec<u32>>,
    repeats: u32,
}

impl Stability {
    /// `const`：`win.rs` 把它放进 `thread_local!` 的 `const` 初始化（无运行期构造）。
    pub const fn new() -> Stability {
        Stability {
            previous: None,
            repeats: 0,
        }
    }

    /// 附着成功：忘掉上一次集合（下一次发现重新确认，不继承上一个进程的候选）。
    pub fn reset(&mut self) {
        *self = Stability::default();
    }

    /// 同一候选 PID 集合连续出现的次数（诊断/日志用；首次 = 1）。
    pub fn repeats(&self) -> u32 {
        self.repeats
    }

    pub fn previous(&self) -> Option<&[u32]> {
        self.previous.as_deref()
    }

    /// 一次发现 → 一次决策（**纯函数**：只依赖入参与上次的集合）。
    pub fn decide(&mut self, discovery: &Discovery) -> Decision {
        let current = discovery.all_pids();
        let same_as_previous = self.previous.as_deref() == Some(current.as_slice());
        let previous = self.previous.take();
        self.previous = Some(current);
        self.repeats = if same_as_previous { self.repeats + 1 } else { 1 };

        match discovery.source {
            // 权威枚举：不存在幻觉坐标 ⇒ 直接按分派表（不引入任何"跨次"条件）。
            Source::Toolhelp => dispatch(&discovery.stable, &discovery.lazer, true),
            Source::PidSweep => {
                // 真进程在进程表里 ⇒ 幻觉坐标已被这一步筛掉（两个位数桶各自过一遍）。
                let confirmed_stable = confirmed(&discovery.stable);
                let confirmed_lazer = confirmed(&discovery.lazer);
                if !confirmed_stable.is_empty() || !confirmed_lazer.is_empty() {
                    return dispatch(&confirmed_stable, &confirmed_lazer, false);
                }
                // 进程表里连真进程都查不到：只能靠"跨次稳定"。
                if same_as_previous {
                    // 同一集合连续两次 ⇒ 按既有规模规则（两个都像游戏 ⇒ `multiple-instances`）。
                    return dispatch(&discovery.stable, &discovery.lazer, false);
                }
                match unique_common(&previous, discovery) {
                    // 只有一个候选跨次都在 ⇒ 就是它（幻觉坐标每次都在变）；位数决定读哪条路。
                    Some((pid, client)) => Decision::Attach { pid, client },
                    // 集合还在变、也没有唯一的"常驻者" ⇒ 不猜、不升级。
                    None => Decision::Retry,
                }
            }
        }
    }
}

/// 位数分派表（**唯一权威**，单测逐行钉住；见文件头）：
///
/// - 有 32 位候选 ⇒ 一律先按 stable 的既有规模规则（唯一实例优先；多个 ⇒ 分不出就 `multiple-instances`）
/// - 没有 32 位候选、恰有 1 个 64 位候选 ⇒ `Attach{ Lazer }`（Step 10B 的修复点）
/// - 没有 32 位候选、≥2 个 64 位候选 ⇒ 同一套规模规则
/// - 两个桶都空 ⇒ `toolhelp` 为真时报 `client-ambiguous`（权威枚举看到过 `osu!.exe`
///   但分不出位数），为假（兜底扫描）时报 `Retry`（"没有目标"）
fn dispatch(stable: &[Candidate], lazer: &[Candidate], toolhelp: bool) -> Decision {
    if !stable.is_empty() {
        // 32 位 stable 是**已验证的读取路径**：唯一实例优先，多实例走既有规模规则。
        return match rank(stable) {
            Ranked::One(pid) => Decision::Attach {
                pid,
                client: Client::Stable,
            },
            Ranked::Ambiguous => Decision::MultipleInstances,
        };
    }
    if !lazer.is_empty() {
        return match rank(lazer) {
            Ranked::One(pid) => Decision::Attach {
                pid,
                client: Client::Lazer,
            },
            Ranked::Ambiguous => Decision::MultipleInstances,
        };
    }
    if toolhelp {
        Decision::ClientAmbiguous
    } else {
        Decision::Retry
    }
}

/// 确认过的候选（`cntThreads > 0` = 真进程；幻觉坐标不在进程表里）。
fn confirmed(candidates: &[Candidate]) -> Vec<Candidate> {
    candidates
        .iter()
        .copied()
        .filter(|candidate| candidate.threads > 0)
        .collect()
}

/// `rank` 的结论（把"要不要开句柄"与"跟谁"分开，避免调用方误读一个裸 PID）。
enum Ranked {
    One(u32),
    Ambiguous,
}

/// 既有规模规则（唯一权威，原样保留）：唯一候选 ⇒ 直接用它；多候选 ⇒ 只有"最像游戏本体
/// 且与第二名拉开一倍"的那个才敢用，否则 `multiple-instances`（绝不猜跟哪一个）。
fn rank(candidates: &[Candidate]) -> Ranked {
    if candidates.len() == 1 {
        return Ranked::One(candidates[0].pid);
    }
    let mut ranked: Vec<&Candidate> = candidates.iter().collect();
    ranked.sort_by(|a, b| {
        b.private_bytes
            .cmp(&a.private_bytes)
            .then(a.pid.cmp(&b.pid))
    });
    let top = ranked[0];
    let second = ranked[1];
    let top_looks_like_game = top.private_bytes >= GAME_MIN_PRIVATE_BYTES;
    let separated = top.private_bytes >= second.private_bytes.saturating_mul(2);
    if top_looks_like_game && separated {
        Ranked::One(top.pid)
    } else {
        Ranked::Ambiguous
    }
}

/// 上一次集合与本次候选的**唯一**交集元素（0 个或多个 ⇒ `None`，不猜）；位数由它所在
/// 的候选桶决定。
fn unique_common(previous: &Option<Vec<u32>>, discovery: &Discovery) -> Option<(u32, Client)> {
    let previous = previous.as_ref()?;
    let mut common = discovery
        .stable
        .iter()
        .map(|candidate| (candidate.pid, Client::Stable))
        .chain(
            discovery
                .lazer
                .iter()
                .map(|candidate| (candidate.pid, Client::Lazer)),
        )
        .filter(|(pid, _)| previous.contains(pid));
    let first = common.next()?;
    if common.next().is_some() {
        return None;
    }
    Some(first)
}

#[cfg(test)]
#[path = "../../tests-local/osu_discovery.rs"]
mod tests_discovery;
