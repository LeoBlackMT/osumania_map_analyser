/**
 * 壳窗口的移动把手、尺寸调整命中区与边缘提示。
 * 把手跟随卡片顶沿；四边/四角经桥调用系统缩放，浏览器模式不拦截输入。
 * 位置更新合并到一帧；提示短暂显示，收到离开事件或交给系统拖动时立即清除。
 */
import { sendControl } from "../sources/bridgeClient.js";

/** 拖动把手高度（与 styles/status.css 一致）。 */
export const DRAG_BAR_HEIGHT = 40;
/** 边缘点亮判定带宽度（px）。 */
export const EDGE_BAND = 16;
/** 一次鼠标输入后的提示时长，另有 CSS 的 150ms 淡出。 */
const HINT_DURATION_MS = 350;
/** 四个窗口边缘；同时也是 edgeFor() 的返回顺序。 */
const EDGES = ["top", "right", "bottom", "left"];

/**
 * 拖动把手的 CSS `top`：卡片顶沿正上方，再钳进窗口。
 *
 * @param {number} cardTop 卡片顶沿（窗口坐标，CSS px）；NaN/Infinity = 量不到卡片。
 * @param {number} barHeight 把手高度。
 * @param {number} windowHeight 窗口高度。
 * @returns {number} `clamp(cardTop - barHeight, 0, windowHeight - barHeight)`；
 *          卡片在窗口顶端（或量不到卡片/卡片高于窗口）时为 0，即改动前的行为。
 */
export function computeBarTop(cardTop, barHeight, windowHeight) {
    const height = Number.isFinite(barHeight) && barHeight > 0 ? barHeight : DRAG_BAR_HEIGHT;
    if (!Number.isFinite(cardTop)) {
        return 0;
    }
    const max = Math.max(0, (Number.isFinite(windowHeight) ? windowHeight : 0) - height);
    return Math.max(0, Math.min(cardTop - height, max));
}

/**
 * 鼠标位置命中的窗口边缘（鼠标坐标与 `window.innerWidth/innerHeight` 同一坐标系）。
 *
 * @param {number} x 鼠标 x（clientX）。
 * @param {number} y 鼠标 y（clientY）。
 * @param {number} width 窗口宽（innerWidth）。
 * @param {number} height 窗口高（innerHeight）。
 * @param {number} [band] 判定带宽度。
 * @returns {string[]} EDGES 的子集，按 top → right → bottom → left 排序；窗口中间返回空数组。
 */
export function edgeFor(x, y, width, height, band = EDGE_BAND) {
    if (!Number.isFinite(x) || !Number.isFinite(y)
        || !Number.isFinite(width) || !Number.isFinite(height)
        || x < 0 || y < 0 || x >= width || y >= height) {
        return [];
    }
    const reach = Number.isFinite(band) && band > 0 ? band : EDGE_BAND;
    const edges = [];
    if (y <= reach) {
        edges.push("top");
    }
    // 远端两条边要求窗口尺寸大于判定带。
    if (Number.isFinite(width) && width > reach && x >= width - reach) {
        edges.push("right");
    }
    if (Number.isFinite(height) && height > reach && y >= height - reach) {
        edges.push("bottom");
    }
    if (x <= reach) {
        edges.push("left");
    }
    return edges;
}

/** 建立边缘提示及八方向的尺寸调整命中区。 */
function buildHints(doc) {
    const root = doc.createElement("div");
    root.className = "mma-edge-hints";
    root.setAttribute("aria-hidden", "true");

    const outline = doc.createElement("div");
    outline.className = "mma-window-outline";
    root.appendChild(outline);

    for (const edge of EDGES) {
        const band = doc.createElement("span");
        band.className = `mma-edge mma-edge-${edge}`;
        root.appendChild(band);
    }

    for (const direction of ["North", "NorthEast", "East", "SouthEast", "South", "SouthWest", "West", "NorthWest"]) {
        const handle = doc.createElement("span");
        handle.className = `mma-resize-handle mma-resize-${direction.toLowerCase()}`;
        handle.dataset.direction = direction;
        root.appendChild(handle);
    }

    const parent = doc.body || doc.documentElement;
    if (parent) {
        parent.appendChild(root);
    }
    return root;
}

/**
 * 装配壳窗口提示。浏览器/tosu 模式同样调用（壳连接是异步的，html.shell-mode 之后才出现）——
 * 非壳模式下的每帧工作只是一次 classList 判断 + `top` 复位，不做任何布局读取。
 *
 * @param {Document} [doc] 页面文档（测试可注入）。
 * @param {Window} [win] 页面窗口（测试可注入）。
 * @returns {{refresh: () => void}|null} 无拖动把手/非浏览器环境时返回 null。
 */
export function initWindowAffordances(
    doc = typeof document !== "undefined" ? document : null,
    win = typeof window !== "undefined" ? window : null,
) {
    if (!doc || !win || typeof doc.querySelector !== "function"
        || typeof win.addEventListener !== "function") {
        return null;
    }
    const bar = doc.querySelector(".mma-drag-bar");
    if (!bar) {
        return null;
    }

    const card = doc.querySelector(".main-card");
    const hints = buildHints(doc);
    const bands = {};
    for (const edge of EDGES) {
        bands[edge] = hints.querySelector(`.mma-edge-${edge}`);
    }

    let pointer = null; // 最近一次鼠标位置（窗口坐标）
    let frame = 0;
    let barDirty = true;
    let edgeDirty = true;
    let hintDirty = false;
    let hintTimer = 0;

    function isShell() {
        return Boolean(doc.documentElement && doc.documentElement.classList
            && doc.documentElement.classList.contains("shell-mode"));
    }

    /** 非壳模式清掉位置和高度；壳内保持把手的窗口坐标高度。 */
    function applyBarTop() {
        if (!isShell()) {
            bar.style.top = "";
            bar.style.height = "";
            hints.style.zoom = "";
            clearPointer();
            return;
        }
        // 根 zoom 只缩放内容，把手保持 40px 高，避免被顶部 16px 缩放区挤占。
        const scale = Number.parseFloat(win.getComputedStyle(doc.documentElement).zoom) || 1;
        const barHeight = DRAG_BAR_HEIGHT / scale;
        bar.style.height = `${barHeight}px`;
        // 边缘命中区保持窗口坐标下的 16px，不随页面内容缩放。
        hints.style.zoom = String(1 / scale);
        const rect = card && typeof card.getBoundingClientRect === "function"
            ? card.getBoundingClientRect()
            : null;
        const cardTop = rect ? Number(rect.top) / scale : Number.NaN;
        const windowHeight = (Number(win.innerHeight) || 0) / scale;
        bar.style.top = `${Math.round(computeBarTop(cardTop, barHeight, windowHeight))}px`;
    }

    /** 四条边带的亮灭：按最近一次鼠标位置重算（离开窗口 = 全灭）。 */
    function applyEdges() {
        const active = new Set(isShell() && pointer
            ? edgeFor(pointer.x, pointer.y, Number(win.innerWidth) || 0, Number(win.innerHeight) || 0)
            : []);
        const overBar = Boolean(isShell() && pointer && pointer.overBar);
        bar.classList.toggle("drag-hover", overBar);
        hints.classList.toggle("drag-active", overBar);
        for (const edge of EDGES) {
            const band = bands[edge];
            if (band && band.classList) {
                band.classList.toggle("active", active.has(edge));
            }
        }
        // 只有新的鼠标输入延长提示；卡片布局刷新不延长高亮。
        if (hintDirty) {
            hintDirty = false;
            win.clearTimeout(hintTimer);
            hintTimer = active.size || overBar
                ? win.setTimeout(clearPointer, HINT_DURATION_MS)
                : 0;
        }
    }

    function schedule() {
        if (frame) {
            return;
        }
        frame = requestAnimationFrame(() => {
            frame = 0;
            if (barDirty) {
                barDirty = false;
                applyBarTop();
            }
            if (edgeDirty) {
                edgeDirty = false;
                applyEdges();
            }
        });
    }

    function scheduleBar() {
        barDirty = true;
        edgeDirty = true;
        schedule();
    }

    function onPointerMove(event) {
        if (!isShell()) {
            return;
        }
        pointer = {
            x: Number(event.clientX),
            y: Number(event.clientY),
            overBar: event.target?.closest?.(".mma-drag-bar") === bar,
        };
        hintDirty = true;
        edgeDirty = true;
        schedule();
    }

    function clearDragHover() {
        bar.classList.remove("drag-hover");
        hints.classList.remove("drag-active");
    }

    function clearPointer() {
        win.clearTimeout(hintTimer);
        hintTimer = 0;
        hintDirty = false;
        pointer = null;
        edgeDirty = false;
        clearDragHover();
        for (const band of Object.values(bands)) {
            band.classList.remove("active");
        }
    }

    function startGesture(event, action, direction = null) {
        if (event.button !== 0 || !isShell()) {
            return;
        }
        event.preventDefault();
        event.stopPropagation();
        clearPointer();
        sendControl(action, null, direction);
    }

    bar.addEventListener("mouseenter", onPointerMove);
    bar.addEventListener("mouseleave", clearPointer);
    bar.addEventListener("mousedown", (event) => startGesture(event, "dragStart"));
    hints.addEventListener("mousedown", (event) => {
        const handle = event.target.closest(".mma-resize-handle");
        if (handle) {
            startGesture(event, "resizeStart", handle.dataset.direction);
        }
    });
    win.addEventListener("mousemove", onPointerMove);
    doc.addEventListener("mouseleave", clearPointer);
    doc.documentElement.addEventListener("pointerleave", clearPointer);
    doc.addEventListener("mouseout", (event) => {
        if (!event.relatedTarget) {
            clearPointer();
        }
    });
    win.addEventListener("blur", clearPointer);
    win.addEventListener("pointercancel", clearPointer);
    doc.addEventListener("visibilitychange", () => {
        if (doc.hidden) {
            clearPointer();
        }
    });
    win.addEventListener("resize", () => {
        clearPointer();
        scheduleBar();
    });

    if (typeof ResizeObserver === "function" && card) {
        new ResizeObserver(scheduleBar).observe(card);
    }
    if (typeof MutationObserver === "function") {
        // 只看这三个节点的 class/style：extend-upward 在 .dashboard 上，
        // shell-mode 在 <html> 上，bars-* / card-hidden-by-play / 卡片 CSS 变量在卡片上。
        // 不订阅子树，避免图/进度条的逐帧属性写入把每帧都变成一次布局读取。
        const mutations = new MutationObserver(scheduleBar);
        for (const node of [doc.documentElement, card, doc.querySelector(".dashboard")]) {
            if (node) {
                mutations.observe(node, { attributes: true, attributeFilter: ["class", "style"] });
            }
        }
    }

    schedule();

    return { refresh: scheduleBar };
}
