// 图表选区交互（Graph Section Analysis）：双击进入待命态 → 拖拽画区间 → 按区间重算。
//
// 只在桌面壳里工作（isShellRuntime：24061 端口或壳追加的 #mma-shell）。浏览器 / OBS
// 模式下 initGraphSection() 直接返回：不挂监听、不改 DOM、行为与改动前完全一致。
//
// 三条时间轴（别混）：
//   显示轴 = 原始时间 / speedRate —— state.graphSeries / rework.graph.times / songTimeMs 都在这个域；
//   物理轴 = .osu 文件里的毫秒 —— 切片器与 TimingPoints 在这个域；
//   真实播放轴 = 显示轴（DT/HT 已在 songTimeMs 折算）。
// 选区同时存「相对绘图带的 u 比例」（画遮罩用）与「物理毫秒」（切片用）：
// 速率变化时全谱曲线等比缩放、像素形状不变 ⇒ u 不变；物理时间也与速率无关。
//
// 依赖约束：本模块只碰 DOM 与 state，不碰管线；重算通过 scheduleRecompute 派发。

import {
    GRAPH_PADDING_X,
    GRAPH_VIEWBOX_HEIGHT,
    GRAPH_VIEWBOX_WIDTH,
    forEachGraphView,
    state,
} from "../appContext.js";
import { clientXToViewBoxX } from "../graphMath.js";
import { isShellRuntime } from "../feedback/feedbackSpec.js";
import { scheduleRecompute } from "../scheduler.js";
import { paintSectionOverlay } from "./overlay.js";

const PLOT_WIDTH = GRAPH_VIEWBOX_WIDTH - 2 * GRAPH_PADDING_X;
const MIN_DRAG_VIEWBOX = 1.5;
const COMMIT_SUPPRESS_DBLCLICK_MS = 250;

let mode = "idle"; // "idle" | "armed"
let selection = null; // { u0, u1, startMs, endMs, rateAtSelection }
let drag = null; // { pointerId, anchorU, moved, prevSelection, view, axis }
let fullChart = { identity: "", text: "" };
let accentStar = Number.NaN;
let lastCommitAt = 0;

const clamp01 = (value) => Math.min(1, Math.max(0, value));

/** 读 SVG 的 preserveAspectRatio（两图不同：header=none，body=xMidYMid meet）。 */
function parsePreserveAspectRatio(svgEl) {
    const tokens = String(svgEl.getAttribute("preserveAspectRatio") || "xMidYMid meet")
        .trim()
        .split(/\s+/);
    return {
        align: tokens[0] || "xMidYMid",
        slice: tokens[1] === "slice" ? "slice" : "meet",
    };
}

/** 当前图表轴（显示轴 + 当前速率）。图未渲染 / 跨度为 0 时返回 null。 */
function getAxis() {
    const series = state.graphSeries;
    if (!series) {
        return null;
    }
    const minTime = Number(series.minTime);
    const maxTime = Number(series.maxTime);
    if (!Number.isFinite(minTime) || !Number.isFinite(maxTime) || !(maxTime - minTime > 1)) {
        return null;
    }
    const rate = Number.isFinite(state.speedRate) && state.speedRate > 0 ? Number(state.speedRate) : 1;
    return { minTime, maxTime, rate };
}

/** 客户端 X → 绘图带比例 u ∈ [0,1]；不可用时返回 null。 */
function pointerU(view, clientX) {
    if (!view?.svgEl || !view?.wrapEl) {
        return null;
    }
    const rect = view.svgEl.getBoundingClientRect();
    if (!rect || rect.width <= 0 || rect.height <= 0) {
        return null;
    }
    const par = parsePreserveAspectRatio(view.svgEl);
    const viewBoxX = clientXToViewBoxX({
        clientX,
        rect,
        viewBoxWidth: GRAPH_VIEWBOX_WIDTH,
        viewBoxHeight: GRAPH_VIEWBOX_HEIGHT,
        parAlign: par.align,
        parSlice: par.slice,
    });
    if (viewBoxX == null) {
        return null;
    }
    return clamp01((viewBoxX - GRAPH_PADDING_X) / PLOT_WIDTH);
}

function makeSelection(uA, uB, axis) {
    const u0 = Math.min(uA, uB);
    const u1 = Math.max(uA, uB);
    const toPhysical = (u) => (axis.minTime + u * (axis.maxTime - axis.minTime)) * axis.rate;
    return {
        u0,
        u1,
        startMs: toPhysical(u0),
        endMs: toPhysical(u1),
        rateAtSelection: axis.rate,
    };
}

function isViewVisible(view) {
    if (!view?.isEnabled() || !view?.wrapEl) {
        return false;
    }
    const rect = view.wrapEl.getBoundingClientRect();
    return Boolean(rect && rect.width > 0 && rect.height > 0);
}

function canArm() {
    if (!isShellRuntime() || state.isInPlayState) {
        return false;
    }
    // 外部源（Malody 桥 / Malody 4）的游玩位与 osu 的 isInPlayState 分开维护。
    if (state.malodyPlaying || state.malody4Playing) {
        return false;
    }
    if (String(state.clientStateName || "") === "resultscreen") {
        return false;
    }
    if (!getAxis()) {
        return false;
    }
    let visible = false;
    forEachGraphView((view) => {
        if (isViewVisible(view)) {
            visible = true;
        }
    });
    return visible;
}

function paint() {
    paintSectionOverlay({ mode, selection, accentStar });
}

function commitSelection() {
    lastCommitAt = Date.now();
    state.pendingChangeKind = "section";
    scheduleRecompute("graph section", true);
}

function onDblClick(event) {
    if (Date.now() - lastCommitAt < COMMIT_SUPPRESS_DBLCLICK_MS || drag) {
        return;
    }
    if (mode === "idle") {
        if (!canArm()) {
            return;
        }
        event.preventDefault();
        mode = "armed";
        paint();
        return;
    }
    event.preventDefault();
    const hadSelection = selection != null;
    clearSection("toggle", { recompute: hadSelection });
}

function onPointerDown(event, view) {
    if (mode !== "armed" || event.button !== 0) {
        return;
    }
    const axis = getAxis();
    const u = pointerU(view, event.clientX);
    if (!axis || u == null) {
        return;
    }
    drag = {
        pointerId: event.pointerId,
        anchorU: u,
        moved: false,
        prevSelection: selection,
        view,
        axis,
    };
    try {
        view.wrapEl.setPointerCapture(event.pointerId);
    } catch {
        // 指针捕获失败不阻断（release 时同样容错）。
    }
    event.preventDefault();
}

function onPointerMove(event) {
    if (!drag || event.pointerId !== drag.pointerId) {
        return;
    }
    const u = pointerU(drag.view, event.clientX);
    if (u == null) {
        return;
    }
    if (!drag.moved && Math.abs(u - drag.anchorU) * PLOT_WIDTH < MIN_DRAG_VIEWBOX) {
        return;
    }
    drag.moved = true;
    selection = makeSelection(drag.anchorU, u, drag.axis);
    paint();
}

function releaseCapture(view, pointerId) {
    try {
        if (view?.wrapEl?.hasPointerCapture?.(pointerId)) {
            view.wrapEl.releasePointerCapture(pointerId);
        }
    } catch {
        // 忽略：指针已释放 / 元素已移除。
    }
}

function onPointerUp(event) {
    if (!drag || event.pointerId !== drag.pointerId) {
        return;
    }
    const { moved, prevSelection, view, pointerId } = drag;
    drag = null;
    releaseCapture(view, pointerId);
    if (!moved) {
        // 纯点击：还原上一次选区，不跑分析（双击进入/退出由 dblclick 处理）。
        selection = prevSelection;
        paint();
        return;
    }
    commitSelection();
}

function onPointerCancel(event) {
    if (!drag || (event.pointerId != null && event.pointerId !== drag.pointerId)) {
        return;
    }
    const { view, pointerId, prevSelection } = drag;
    drag = null;
    releaseCapture(view, pointerId);
    selection = prevSelection;
    paint();
}

function bindView(view) {
    const wrap = view.wrapEl;
    if (!wrap || wrap.dataset.sectionBound === "1") {
        return;
    }
    wrap.dataset.sectionBound = "1";
    wrap.addEventListener("dblclick", onDblClick);
    wrap.addEventListener("pointerdown", (event) => onPointerDown(event, view));
    wrap.addEventListener("pointermove", onPointerMove);
    wrap.addEventListener("pointerup", onPointerUp);
    wrap.addEventListener("pointercancel", onPointerCancel);
    wrap.addEventListener("lostpointercapture", onPointerCancel);
}

/** 初始化（幂等）。非壳环境直接返回，不挂任何监听。 */
export function initGraphSection() {
    if (!isShellRuntime()) {
        return;
    }
    forEachGraphView(bindView);
}

/** 当前生效的切片区间（物理谱面毫秒）；未 armed / 无选区时返回 null。 */
export function getSectionRange() {
    if (mode !== "armed" || !selection) {
        return null;
    }
    const startMs = Number(selection.startMs);
    const endMs = Number(selection.endMs);
    if (!Number.isFinite(startMs) || !Number.isFinite(endMs)) {
        return null;
    }
    return { startMs: Math.min(startMs, endMs), endMs: Math.max(startMs, endMs) };
}

/** 全谱原文（供区间跑批切片）；identity 不匹配或为空时返回 null。 */
export function getFullChartText() {
    const identity = String(state.lastBeatmapIdentity || "");
    if (!identity || fullChart.identity !== identity || !fullChart.text) {
        return null;
    }
    return fullChart.text;
}

/** 全谱跑批成功后写入原文（只在非区间跑批时调用；浏览器模式不占用内存）。 */
export function rememberFullChartText(rawText) {
    if (!isShellRuntime()) {
        return;
    }
    const identity = String(state.lastBeatmapIdentity || "");
    const text = typeof rawText === "string" ? rawText : "";
    if (!identity || !text) {
        fullChart = { identity: "", text: "" };
        return;
    }
    fullChart = { identity, text };
}

/**
 * 复位选区。
 * @param {string} reason 诊断用原因
 * @param {{recompute?: boolean}} [options] recompute=true 时派发一次全谱重算
 * @returns {boolean} 是否真的复位了（未 armed 且无选区时为 no-op）
 */
export function clearSection(reason, options = {}) {
    const wasActive = mode === "armed" || selection != null || drag != null;
    if (!wasActive) {
        return false;
    }
    if (drag) {
        releaseCapture(drag.view, drag.pointerId);
    }
    mode = "idle";
    selection = null;
    drag = null;
    paint();
    if (options.recompute) {
        state.pendingChangeKind = "section";
        scheduleRecompute(`section cleared (${reason})`, true);
    }
    return true;
}

/** 分析完成后刷新遮罩主题色（跟随当前星级色阶）与胶囊时间。 */
export function notifySectionAnalysis(star) {
    if (!isShellRuntime()) {
        return;
    }
    const value = Number(star);
    if (Number.isFinite(value) && value > 0) {
        accentStar = value;
    }
    paint();
}
