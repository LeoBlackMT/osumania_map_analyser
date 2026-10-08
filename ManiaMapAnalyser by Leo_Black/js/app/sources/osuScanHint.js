// 壳相位提示（Step 9e，Step 9f 改为**独立元素**；浏览器专属）：壳内 osu 读取器在 L0 附着/
// 锚点扫描时（实测 13–23 s，冷启动与**游戏重启**都会走一遍），卡片会十几秒毫无说明 ——
// 这里把壳下发的那句英文提示放进 `index.html` 的 `#osu-scan-hint`，仅此而已
// （不新造面板、不覆盖卡片、不动画、不新起一行）。
//
// **为什么必须是自己的元素**（Step 9f 的真机诊断）：Step 9e 把提示借写在共享状态行 `#status`
// 上，而 `#status` 的写手不止一个（分析流程的元信息行/"Waiting for a data source…"、Malody
// 提示、以及 Step 9e 自己"帧一到就撤"的清理）。`clearOsuScanHint`（旧 `osuScanHint.js:74-82`）
// 用"`state.statusText` 还是我那句"当清理条件，又被 `socketHandlers.setupSocketListener` 的
// **每一条** v2 帧调用（`旧 socketHandlers.js:337`，含没有 `beatmap` 的空帧）⇒ 提示一写进
// `#status`，下一条 tosu 帧就把 `setStatus("", "ok")` 打上去，而"同一个相位值不复活"的规则让
// 它再也回不来 —— 实测只闪 ~300 ms（tosu 的 `/websocket/v2` 本机只读实测 ≈9.6 帧/s，前 300 ms
// 就有 3 帧 ⇒ 提示活不过一格采样）。提示现在有自己的元素：别人的写入**碰不到它**。
//
// 提示是**壳下发句子的镜像**：`notice` 非空才渲染，壳不发（健康 / 强制 tosu / 24062 未绑定 /
// 旧壳没有这个字段）就什么都不显示。判据全在 `osuScanHintRules.js`（纯函数，Node 可测）；
// 本文件只做"读 state → 写自己的元素"。
//
// 浏览器模式（无壳）连 `syncOsuScanHint` 都不会被调用（它只由 `shellState.applyShellState` 调），
// `noteOsuScanPayloadArrival` 只动一个纯记忆 ⇒ 元素保持 `hidden`、行为与改动前逐字节相同、
// 无新 console 噪声。
//
// 三条纪律：
// - **只写自己的元素**：绝不碰 `#status`（那是分析流程的权威面），也就不存在"被别人清掉"。
// - **相位值内不复活**：壳**原生端点**的载荷到达 ⇒ 立刻撤；同一个相位值期间不再放回来
//   （等壳下发新相位值）。回落到 tosu 时的帧不算（见 `noteOsuScanPayloadArrival`）。
// - **只在壳页**（24061）：浏览器 tosu 页 / 无壳页一律不提示（壳的读取器与它无关）。

import { isRuntimeOsuOverrideActive, isShellPage, osuScanHintEl, state } from "../appContext.js";
import {
    createOsuScanLatch,
    deriveOsuScanHint,
    noteOsuScanPayload,
    observeOsuScanPhase,
} from "./osuScanHintRules.js";
import { currentRoute } from "./sourceManager.js";

export { OSU_HINT_PHASES, deriveOsuScanHint } from "./osuScanHintRules.js";

/** 当前由本模块写进提示元素的句子（`""` = 没写/已隐藏）。 */
let shownText = "";
/** 相位生命周期记忆（同一相位值期间"真载荷已到"粘住；见 `osuScanHintRules.js`）。 */
let latch = createOsuScanLatch();

/**
 * 应用壳 state 帧里的 `sources.osu` 诊断（`shellState.applyShellState` 调用）。
 *
 * 幂等：同一句已在元素里就只重断言语义、不重写文本（避免每 2 s 的 state 帧触发一次重排）。
 */
export function syncOsuScanHint() {
    const phase = typeof state.shellOsuPhase === "string" ? state.shellOsuPhase : "";
    latch = observeOsuScanPhase(latch, phase);
    let route = null;
    try {
        route = typeof currentRoute === "function" ? currentRoute() : null;
    } catch {
        route = null;
    }
    const isNonOsu = (route && route !== "osu")
        || (state.activeSource && state.activeSource !== "osu")
        || (state.cardOwner && state.cardOwner !== "osu")
        || Boolean(state.externalSourceActive);
    const hasDisplayedBeatmap = Boolean(state.lastBeatmapIdentity);
    const hint = deriveOsuScanHint({
        phase,
        notice: state.shellOsuNotice,
        shellOnline: state.externalBridgeAvailable === true,
        shellPage: isShellPage(),
        framesFlowing: latch.payloadSeen,
        activeSource: state.activeSource,
        isNonOsu,
        hasDisplayedBeatmap,
    });
    applyOsuScanHintText(hint.active ? hint.text : "");
}

/**
 * 真 osu 载荷到达（`socketHandlers.applyBeatmapState` 在载荷带谱面身份、且真的改动了卡片时
 * 调用一次）：撤掉提示。
 *
 * **只认壳原生端点的载荷**：提示说明的是**壳内读取器**在扫描；扫描期间页面按契约回落到
 * `wsEndpoint`（tosu 24050，`osu_source.rs::mode_for`：从未健康过 ⇒ `"tosu"`），而 tosu 的
 * `/websocket/v2` 是**持续推流**的（本机只读实测 ≈9.6 帧/s，前 300 ms 就有 3 帧）——Step 9e
 * 正是被这个流打掉的（钩子挂在**每条** v2 帧上，与相位无关）。所以这里加第二道门：只有页面
 * 真的在读壳的原生端点（`isRuntimeOsuOverrideActive()`）时，载荷才算"读取器的数据到了"。
 *
 * 记在**相位值**上而不是设一个永久标志：新相位值到达时归零 —— 于是"载荷流动过"只否定**它那
 * 一次**的扫描，不否定之后真的又发生的一次重附着。
 */
export function noteOsuScanPayloadArrival() {
    if (!isRuntimeOsuOverrideActive()) {
        return; // 回落到 tosu 时的帧不是壳读取器的数据（它们 ~10 Hz，绝不能撤提示）
    }
    latch = noteOsuScanPayload(latch);
    applyOsuScanHintText("");
}

/**
 * 把想要显示的句子落到提示元素上（`""` = 隐藏）。
 *
 * 文本只在变化时写（同一句话不重写 DOM）；`hidden` 每次都断言 —— 纯属性置位、无重排，
 * 于是"同一相位值期间反复断言"是幂等的，也不可能闪。
 */
function applyOsuScanHintText(desired) {
    const next = typeof desired === "string" ? desired : "";
    if (!osuScanHintEl) {
        shownText = next; // 该页面没有这个元素（例如 debug 页）：只维护内部状态
        return;
    }
    if (next !== shownText) {
        shownText = next;
        osuScanHintEl.textContent = next;
    }
    osuScanHintEl.hidden = next === "";
}