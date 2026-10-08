// `sources.osu`（契约 v6）的组装：读取器健康位 + 壳配置 + 24062 绑定结果 → 下发的端点描述。
//
// 决策是**纯函数**（`mode_for` / `build`），只接受普通值、不碰 `Shared`：
// `tests-local/server_mod.rs` 记过的那条约束——lib 单测一旦引用 `Shared`，tauri 窗口类型的
// 析构 glue 会被链进测试二进制，进程在**加载期**就以 `0xc0000139` 失败（一个用例都跑不起来）。
//
// 语义（计划 §6「双向线上语义」/ Step 9，抗抖动见 Step 9c）：
// - 读取器发布了载荷 + 24062 已绑定 + 壳配置没强制 tosu ⇒ `mode:"native"`（页面切到本机 origin）
// - 其余（未附着 / `unhealthy` / 端口被占 / 配置强制 tosu）⇒ `mode:"tosu"`（页面清覆盖，
//   回到设置里的 `wsEndpoint`）——**不制造差异**：无壳/无原生时页面行为逐字节不变。
// - **抗抖动**（Step 9c，窗口在 Step 9d 拉长到覆盖冷路径）：读取器**刚刚**掉线时
//   （< [`READER_GRACE_MS`]，且此前健康过）仍然 `"native"`——页面留在我们的端点、保留最后一张
//   好卡片；持续掉线才回落 `"tosu"`。为什么是 25 s：冷重附着要走一次全量锚点扫描（实测
//   12.6–18.6 s），窗口短于它就会在扫描中途把页面交给（可能已死的）tosu。
//   操作者的显式 `"osuTransport": "tosu"` 与 24062 未绑定**不受窗口影响**（立即 tosu）。
//
// `mode:"tosu"` 的 `host`/`port` 是壳知道的 tosu 端点（`tosu.env` 解析结果），只作诊断：
// 页面在该模式下一律读自己的 `wsEndpoint`。

use crate::frames::{
    OsuScanProgress, OsuSource, OsuTransport, OSU_COMPAT_FILES_PATH, OSU_COMPAT_PORT,
    OSU_COMPAT_WS_PATH,
};

/// 原生传输的主机名（24062 只监听 loopback）。
pub const NATIVE_HOST: &str = "127.0.0.1";

/// 壳配置 `mma-shell-config.json` 里 `osuTransport` 的逃生取值：强制走 tosu。
/// `auto`（缺省）/ `native` 一律"原生优先"（读不到健康读数就回落）。
pub const CONFIG_TOSU: &str = "tosu";

/// 读取器掉线多久才允许把 `mode` 回落 `"tosu"`（**抗抖动窗口**，Step 9c；Step 9d 拉长到 25 s）。
///
/// 为什么是"窗口"而不是"立刻回落"：**页面在我们的端点绑着的时候会保留最后一张好卡片**
/// （24062 不发帧 ⇒ 卡片不更新，但也不清空）；而回落到 `"tosu"` 会把页面的 WS / `.osu` / 封面
/// **全部**指向 `24050`——在"游戏重启、tosu 没开"这种现场那个端点是**死的**，页面于是直接丢掉
/// 来源（真机现场：`%TEMP%\mma-shell-step9\shell-err3.txt` 的 `read failed, detaching: read-error`
/// → 重附着 → 全量扫描 16.6 s）。所以跨一次**短瞬变**（读失败重连、冻结窗口重解析、同一实例的
/// 快速重附着）留在 `"native"` 严格优于指向一个可能不存在的 tosu。
///
/// **Step 9d 为什么必须覆盖冷路径**：游戏关掉 → 再打开时，重附着的目标是**新进程实例**
/// （`module_base` 变 ⇒ Step 9c 的锚点缓存按其键定义**必然丢弃**，见 `anchor_cache.rs`），
/// 于是恢复要走一次**全量锚点扫描**：本机实测 12.6–18.6 s（`step9c/step9d` 现场 13.5–16.6 s）。
/// 10 s 的窗口比它短 ⇒ 页面会在扫描中途被交给（可能已死的）tosu、扫描完再切回来。
/// 25 s 覆盖实测冷路径并留余量（扫描 16.6 s ⇒ 余量 8.4 s），**且不改变其余任何语义**。
///
/// 窗口只是"回落的下限"，不是"永不回落"：**从未见过健康读数**（游戏没开、读取器没启动）
/// 一律立即 `"tosu"`——那种情形下没有"最后一张好卡片"可保，回落与改动前逐字节相同。
pub const READER_GRACE_MS: u64 = 25_000;

/// 壳配置是否强制走 tosu（纯函数；缺省 / 未知值 = 不强制）。
pub fn forced_tosu(shell_config: &serde_json::Value) -> bool {
    shell_config
        .get("osuTransport")
        .and_then(|value| value.as_str())
        .map(|value| value.trim().eq_ignore_ascii_case(CONFIG_TOSU))
        .unwrap_or(false)
}

/// 读取器健康读数的**纯记忆**（抗抖动窗口的唯一状态；由 `Shared` 持有、监视器每 2 s 观测一次）。
///
/// 只记"上一次见到健康读数的时刻"：
/// - 从未见过健康读数（壳刚起来 / 游戏没开）⇒ `observe` 返回 `None`；
/// - 见过之后掉线 ⇒ 返回"已连续掉线多久"（毫秒），由 [`mode_for`] 与 [`READER_GRACE_MS`] 比；
/// - 重新健康 ⇒ 计时归零 ⇒ 下一次 `mode` **立即**回 `"native"`（≤2 s 由监视器拍保证）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReaderLiveness {
    last_alive_ms: Option<u64>,
}

impl ReaderLiveness {
    /// 记一次观测，返回"读取器已连续不健康多久"（毫秒）；从未见过健康读数 ⇒ `None`。
    pub fn observe(&mut self, reader_alive: bool, now_ms: u64) -> Option<u64> {
        if reader_alive {
            self.last_alive_ms = Some(now_ms);
            return Some(0);
        }
        self.last_alive_ms
            .map(|at| now_ms.saturating_sub(at))
    }
}

/// 传输模式决策（纯函数，表驱动单测）。
///
/// | 入参 | 结果 |
/// |---|---|
/// | `compat_bound == false` | `"tosu"`（**立即**：绝不下发一个没人听的端口） |
/// | `forced_tosu`（壳配置 `"osuTransport": "tosu"`） | `"tosu"`（**立即**：操作者的显式设置） |
/// | `reader_alive` | `"native"`（**立即**：健康读数一回来就切回） |
/// | 掉线 < [`READER_GRACE_MS`] | `"native"`（**抗抖动窗口**：页面留在我们的端点） |
/// | 掉线 ≥ [`READER_GRACE_MS`] | `"tosu"`（持续掉线才回落） |
/// | 从未健康过（`reader_down_ms == None`） | `"tosu"`（没有可保的卡片；与改动前逐字节相同） |
///
/// 抗抖动的理由写在 [`READER_GRACE_MS`] 上（页面在 native 下保留最后一张好卡片）。
pub fn mode_for(
    reader_alive: bool,
    forced_tosu: bool,
    compat_bound: bool,
    reader_down_ms: Option<u64>,
) -> &'static str {
    if !compat_bound || forced_tosu {
        return "tosu";
    }
    if reader_alive {
        return "native";
    }
    match reader_down_ms {
        Some(down_ms) if down_ms < READER_GRACE_MS => "native",
        _ => "tosu",
    }
}

/// `build` 的入参（从 `Shared` + 读取器取出的普通值）。
pub struct Inputs<'a> {
    /// `osu::healthy(&ReaderState)`：读取器是否发布了载荷。
    pub reader_alive: bool,
    /// 读取器已连续不健康多久（[`ReaderLiveness::observe`]；`None` = 从未见过健康读数）。
    pub reader_down_ms: Option<u64>,
    /// 读取器的门状态字面量（诊断字段 `gate`）。
    pub gate: &'a str,
    pub client: Option<String>,
    pub reason: Option<String>,
    pub degraded_fields: Vec<String>,
    pub forced_tosu: bool,
    pub compat_bound: bool,
    /// 壳知道的 tosu 端点（`mode:"tosu"` 时只作诊断）。
    pub tosu_host: String,
    pub tosu_port: u16,
    /// 读取器相位字面量（Step 9e；`osu::Phase::as_str`；`""` = 读取线程还没给出结论）。
    pub phase: &'a str,
    /// 相位对应的英文提示句（`osu::Phase::notice`；`""` = 不提示）。
    pub notice: &'a str,
    /// L0 扫描的实测进度（只在 `phase == "scanning"` 时有值）。
    pub progress: Option<crate::osu::ScanProgress>,
}

/// 组装 `sources.osu`（纯函数）。
///
/// Step 9e：相位/提示/进度三项诊断字段原样转写，**唯一**的加工是"原生传输结构性不可用时
/// 抽掉提示与进度"（见 [`native_reachable`]）——`phase` 永远如实上报（它是读取器的诊断）。
pub fn build(inputs: Inputs<'_>) -> OsuSource {
    let mode = mode_for(
        inputs.reader_alive,
        inputs.forced_tosu,
        inputs.compat_bound,
        inputs.reader_down_ms,
    );
    let native = mode == "native";
    let (host, port) = if native {
        (NATIVE_HOST.to_string(), OSU_COMPAT_PORT)
    } else {
        (inputs.tosu_host, inputs.tosu_port)
    };
    // 提示/进度只在"页面真的可能从原生传输拿到数据"时才下发：操作者强制 tosu 或 24062 没绑定
    // 时，原生读取器再健康也到不了页面 —— 那种情形下提示只会解释一条拿不到的数据路径。
    // ⚠️ 判据**不是** `mode`：冷启动（从没健康过）时 `mode` 是 `"tosu"`，而那一刻页面正需要
    // 这句提示（扫描 13–23 s 里它一个帧都收不到）。
    let hint_visible = native_reachable(inputs.forced_tosu, inputs.compat_bound);
    OsuSource {
        alive: native,
        transport: OsuTransport {
            mode: mode.to_string(),
            host,
            port,
            ws_path: OSU_COMPAT_WS_PATH.to_string(),
            files_path: OSU_COMPAT_FILES_PATH.to_string(),
        },
        gate: inputs.gate.to_string(),
        client: inputs.client,
        reason: inputs.reason.unwrap_or_default(),
        degraded_fields: inputs.degraded_fields,
        phase: inputs.phase.to_string(),
        notice: if hint_visible {
            inputs.notice.to_string()
        } else {
            String::new()
        },
        progress: if hint_visible {
            inputs.progress.map(|progress| OsuScanProgress {
                filter: progress.filter,
                regions: progress.regions,
                bytes: progress.bytes,
                elapsed_ms: progress.elapsed_ms,
            })
        } else {
            None
        },
    }
}

/// 页面是否**可能**从原生传输拿到数据（结构判据：24062 已绑定 + 操作者没强制 tosu）。
///
/// 与 [`mode_for`] 的两个"立即回落"条件逐字同源，但**不含**抗抖动窗口与"从未健康过"——
/// 那两条是**时间**判据（页面此刻在 tosu 端点，但原生早晚会到），这里问的是"能不能到"。
pub fn native_reachable(forced_tosu: bool, compat_bound: bool) -> bool {
    compat_bound && !forced_tosu
}

#[cfg(test)]
#[path = "../../tests-local/server_osu_source.rs"]
mod tests;