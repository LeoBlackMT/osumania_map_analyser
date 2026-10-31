// 选区遮罩层：把 { mode, selection } 画到两张图上，并同步状态行时间胶囊。
// 纯渲染模块：不持有状态、不派发重算（状态机在 ./index.js）。
//
// 高亮颜色 = 当前卡片主题色（--ma-accent），再经 ensureContrastColor 保证在深色图表背景上
// 有足够对比度——高星/深色封面主题直接拿来用会看不清。

import {
    GRAPH_PADDING_X,
    GRAPH_VIEWBOX_WIDTH,
    forEachGraphView,
    mainCardEl,
    sectionTimeEl,
    state,
} from "../core/appContext.js";
import { getGraphLineVerticalBounds } from "./index.js";
import { ensureContrastColor } from "../ui/display.js";

const PLOT_WIDTH = GRAPH_VIEWBOX_WIDTH - 2 * GRAPH_PADDING_X;
/** 图表容器深色渐变起点（styles/graph.css `.star-graph-wrap`），用于计算对比度。 */
const GRAPH_BACKGROUND = "#0d1220";
const FALLBACK_ACCENT = "#a6e0ff";

/** 毫秒 → mm:ss（不设上限：75 分钟显示 75:03）。 */
export function formatClock(ms) {
    const totalSeconds = Math.max(0, Math.round(Number(ms) / 1000));
    const minutes = Math.floor(totalSeconds / 60);
    const seconds = totalSeconds % 60;
    return `${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`;
}

/** 当前主题色（coverTheme / 自定义背景写进 --ma-accent），并保证图表背景上的辨识度。 */
function resolveThemeAccent() {
    const root = typeof document !== "undefined" ? document.documentElement : null;
    const raw = root ? getComputedStyle(root).getPropertyValue("--ma-accent").trim() : "";
    return ensureContrastColor(raw || FALLBACK_ACCENT, GRAPH_BACKGROUND, 3);
}

function applyAccent() {
    if (!mainCardEl) {
        return;
    }
    mainCardEl.style.setProperty("--graph-section-accent", resolveThemeAccent());
}

function paintView(view, mode, selection) {
    const wrap = view.wrapEl;
    const svg = view.svgEl;
    if (!wrap || !svg) {
        return;
    }
    wrap.classList.toggle("graph-section-armed", mode === "armed");

    const overlay = svg.querySelector(".graph-section-overlay");
    if (!overlay) {
        return;
    }
    // 双保险：曲线被清空时不画遮罩（正常路径由 graph.js 的 cleared 回调复位选区）。
    const show = mode === "armed" && selection != null && Boolean(state.graphSeries) && view.isEnabled();
    overlay.classList.toggle("is-hidden", !show);
    if (!show) {
        return;
    }

    const { yTop, yBottom } = getGraphLineVerticalBounds(view);
    const x0 = GRAPH_PADDING_X + selection.u0 * PLOT_WIDTH;
    const x1 = GRAPH_PADDING_X + selection.u1 * PLOT_WIDTH;

    const rect = overlay.querySelector(".graph-section-mask");
    if (rect) {
        rect.setAttribute("x", Math.min(x0, x1).toFixed(2));
        rect.setAttribute("width", Math.abs(x1 - x0).toFixed(2));
        rect.setAttribute("y", yTop.toFixed(2));
        rect.setAttribute("height", (yBottom - yTop).toFixed(2));
    }
    for (const [side, x] of [["start", x0], ["end", x1]]) {
        const line = overlay.querySelector(`.graph-section-edge-${side}`);
        if (!line) {
            continue;
        }
        line.setAttribute("x1", x.toFixed(2));
        line.setAttribute("x2", x.toFixed(2));
        line.setAttribute("y1", yTop.toFixed(2));
        line.setAttribute("y2", yBottom.toFixed(2));
    }
}

function updateCapsule(mode, selection) {
    if (!sectionTimeEl) {
        return;
    }
    if (mode !== "armed" || !selection) {
        sectionTimeEl.hidden = true;
        return;
    }
    const rate = Number.isFinite(state.speedRate) && state.speedRate > 0 ? state.speedRate : 1;
    sectionTimeEl.textContent = `${formatClock(selection.startMs / rate)} - ${formatClock(selection.endMs / rate)}`;
    sectionTimeEl.hidden = false;
}

/**
 * 重画双图遮罩 + 胶囊 + 主题色。
 * @param {{mode:"idle"|"armed", selection:{u0:number,u1:number,startMs:number,endMs:number}|null}} snapshot
 */
export function paintSectionOverlay({ mode, selection }) {
    applyAccent();
    forEachGraphView((view) => paintView(view, mode, selection));
    updateCapsule(mode, selection);
}
