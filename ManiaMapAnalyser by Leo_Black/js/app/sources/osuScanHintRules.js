// 壳相位提示的**纯规则**（Step 9e；Step 9f 加"相位生命周期记忆"）：零依赖、零 DOM ——
// 好让 Node 直接 import 做表驱动测试（`osuScanHint.js` 才是浏览器侧的 DOM 写手）。
//
// 提示的判据（六种输入组合逐行钉在测试里：scanning / attaching / waiting / healthy /
// forced-tosu / no-game）：
// - **必须有壳且必须是壳页**：无壳（浏览器模式）与非壳页（24050 tosu 页）一律不提示；
// - **真载荷流动就不提示**：真数据帧到达即视为"扫描那份等待已经结束"；
// - **相位必须在会带提示的闭集里**：`healthy` / `unavailable` / 未知值（旧壳、未来相位）都不提示；
// - **句子必须由壳给**：壳抽掉了 `notice`（操作者强制 tosu / 24062 未绑定）就不提示 ——
//   页面绝不自己编一句，免得解释一条拿不到的数据路径。
// - **判据里没有状态行**：问卷面写着什么（`#status` 被分析流程改写）**不是**提示的判据。
//   Step 9e 把提示写进共享状态行、又拿"状态行是不是自己那句"当清理条件 ⇒ 一次外部改写就把提示
//   永久打死（Step 9f 的真机现象：只闪 ~300 ms）。Step 9f 起提示有自己的元素，这里也不接受
//   任何状态行入参。

/** 会带提示的相位（壳侧闭集字面量，见 `desktop/src/osu/mod.rs::Phase` 与 CONTRACT.md §8）。
 * 仅在 L0 内存扫描（scanning，约 10–20s）与重附着（attaching）等耗时等待阶段展示提示；
 * 游戏未启动态（waiting-for-game）由卡片自身 status 承担等待指示，不展示黄字以避免并排冲突。
 */
export const OSU_HINT_PHASES = Object.freeze(["attaching", "scanning"]);

/**
 * 提示的纯派生。
 *
 * @param {{phase?: string, notice?: string, shellOnline?: boolean, shellPage?: boolean,
 *          framesFlowing?: boolean}} input
 * @returns {{active: boolean, text: string}} `active` = 该显示提示；`text` = 要显示的句子
 *          （非活动时为空串）。调用方把它写进**提示自己的元素**（`#osu-scan-hint`）。
 */
export function deriveOsuScanHint(input) {
    if (!input || !input.shellOnline || !input.shellPage) {
        return { active: false, text: "" }; // 无壳 / 非壳页：绝不提示（浏览器行为逐字节不变）
    }
    // 非 osu 来源处于活跃状态（如用户正在使用 Malody / Etterna / Malody 4）时，绝不显示 osu 提示
    if (input.isNonOsu || (input.activeSource && input.activeSource !== "osu")) {
        return { active: false, text: "" };
    }
    if (input.framesFlowing) {
        return { active: false, text: "" }; // 真载荷已流动 ⇒ 提示必须消失
    }
    const phase = typeof input.phase === "string" ? input.phase : "";
    if (!OSU_HINT_PHASES.includes(phase)) {
        return { active: false, text: "" }; // healthy / unavailable / waiting-for-game / 未知相位
    }
    const notice = typeof input.notice === "string" ? input.notice : "";
    if (!notice) {
        return { active: false, text: "" }; // 壳没给句子（例如强制 tosu 时壳抽掉了提示）
    }
    return { active: true, text: notice };
}

/**
 * 相位生命周期记忆（Step 9f，纯数据）：**同一个相位值期间"真载荷已到"必须粘住**。
 *
 * 为什么要粘：提示现在写在**自己的元素**上，所以"同一相位值"的每次 state 帧（壳每 2 s 一发）
 * 都会重新断言提示 —— 若"载荷到过"只记在未被消费的瞬时输入里，载荷到达后下一次 state 帧就会
 * 把提示放回来（闪）。粘住之后，"真载荷 ⇒ 撤提示"对这个相位值**只生效一次**。
 *
 * 为什么以**相位值**为单位而不是"永久"：壳下发新相位值（healthy → 又一次 scanning）意味着
 * 真的又发生了一次附着/扫描，那时提示必须能重新出现。
 *
 * @returns {{phase: string|null, payloadSeen: boolean}} 记忆（`phase` = 上次见过的相位值字面量）
 */
export function createOsuScanLatch() {
    return { phase: null, payloadSeen: false };
}

/**
 * 观察一次相位值（`shellState.applyShellState` 的每次 state 帧调用）。
 *
 * 同一相位值 ⇒ 记忆原样返回（载荷粘住）；新相位值 ⇒ 载荷位归零（新一次扫描可以重新提示）。
 *
 * @param {{phase: string|null, payloadSeen: boolean}} latch
 * @param {string|null|undefined} phase
 * @returns {{phase: string|null, payloadSeen: boolean}}
 */
export function observeOsuScanPhase(latch, phase) {
    const next = typeof phase === "string" ? phase : "";
    if (latch && latch.phase === next) {
        return latch;
    }
    return { phase: next, payloadSeen: false };
}

/**
 * 记一次"真 osu 载荷到达"（`socketHandlers` 只在载荷**真的带谱面身份、且改变了卡片**时调用）。
 *
 * @param {{phase: string|null, payloadSeen: boolean}} latch
 * @returns {{phase: string|null, payloadSeen: boolean}}
 */
export function noteOsuScanPayload(latch) {
    return { phase: latch ? latch.phase : null, payloadSeen: true };
}