/**
 * 壳窗口的可视提示（浏览器专属；只在 html.shell-mode 下生效，浏览器/tosu 模式行为不变）。
 *
 * 1. **拖动把手贴卡片顶沿**：把手是窗口级的（index.html 的 `.mma-drag-bar`，CSS 固定 `top:0`）。
 *    `reverseCardExtendDirection`（卡片贴底、向上生长）时卡片可能比窗口矮，把手会浮在卡片
 *    上方的空白里、看着和卡片脱节；这里改成「卡片顶沿之上一个把手高度」并钳进窗口。
 *    卡片顶到窗口顶端（常规情况）时结果仍是 `top:0`，与改动前完全一致；
 *    量不到卡片时同样回落到 `top:0`。
 * 2. **窗口边缘发光**：窗口透明无边框，用户看不到可拖拽的尺寸边界（Tauri 的 resize 边框在
 *    窗口四周 4~8px）。鼠标进入某条边 16px 内就点亮那条边（角落两条同时亮），移开即灭。
 * 3. **hover 把手时的一圈细描边**：提示"即将移动的是整扇窗口"。
 *
 * 全部纯视觉：提示容器 `position: fixed`、`pointer-events: none`，不参与布局、不拦输入。
 * 拖动把手仍是唯一可命中的元素（CSS `html.shell-mode .mma-drag-bar { pointer-events: auto }`），
 * 它跟着卡片走 ⇒ `data-tauri-drag-region` 的命中区与 Wayland 兜底的 `dragStart` control 帧
 * （页面 bootstrap index.js 的 mousedown）都落在用户看到的那条光带上。
 *
 * 位置计算在窗口坐标系：鼠标用 `clientX/clientY` 与 `window.innerWidth/innerHeight` 比较
 * （两者都按根 zoom 之前的坐标）；卡片用 `getBoundingClientRect()`（已被根 zoom 缩放），
 * 按把手自身的 rect/offsetHeight 比例折算回 CSS px。zoom=1（默认）时就是直接比较。
 *
 * 刷新时机：卡片 ResizeObserver + `window.resize` + `<html>`/卡片/`.dashboard` 的
 * class/style 变化（extend-upward、bars-* 模式切换、shell-mode 接入）；全部经
 * requestAnimationFrame 合并，每帧最多执行一次，不自转、不轮询。
 */

/** 拖动把手高度（styles/status.css 的 `.mma-drag-bar` height: 22px）。 */
export const DRAG_BAR_HEIGHT = 22;
/** 边缘点亮判定带宽度（px）。 */
export const EDGE_BAND = 16;
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
    if (!Number.isFinite(x) || !Number.isFinite(y)) {
        return [];
    }
    const reach = Number.isFinite(band) && band > 0 ? band : EDGE_BAND;
    const edges = [];
    if (y <= reach) {
        edges.push("top");
    }
    // 远端两条边要求窗口尺寸有效：尺寸量不到（0/NaN）时不能把 x/y >= -16 判成命中。
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

/**
 * 建提示容器与四条边带（模块自建：index.html 保持原样；fixed + pointer-events:none，
 * 不参与页面布局）。
 */
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

    function isShell() {
        return Boolean(doc.documentElement && doc.documentElement.classList
            && doc.documentElement.classList.contains("shell-mode"));
    }

    /** 把手的 top：非壳模式清掉 inline 值，回到 CSS 的 top:0（与改动前一致）。 */
    function applyBarTop() {
        if (!isShell()) {
            bar.style.top = "";
            return;
        }
        const barHeight = Number(bar.offsetHeight) || DRAG_BAR_HEIGHT;
        const barRect = typeof bar.getBoundingClientRect === "function"
            ? bar.getBoundingClientRect()
            : null;
        // 根 zoom（index.js 的页面缩放）会让 rect 比 offsetHeight 大同样的倍数；
        // 取不到就按 1 处理（zoom 不存在时即直接比较）。
        const scale = barRect && Number(barRect.height) > 0 ? Number(barRect.height) / barHeight : 1;
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
        for (const edge of EDGES) {
            const band = bands[edge];
            if (band && band.classList) {
                band.classList.toggle("active", active.has(edge));
            }
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
        schedule();
    }

    function onPointerMove(event) {
        pointer = { x: Number(event.clientX), y: Number(event.clientY) };
        edgeDirty = true;
        schedule();
    }

    function clearPointer() {
        pointer = null;
        edgeDirty = true;
        schedule();
    }

    if (typeof bar.addEventListener === "function") {
        bar.addEventListener("mouseenter", () => hints.classList.toggle("drag-active", true));
        bar.addEventListener("mouseleave", () => hints.classList.toggle("drag-active", false));
    }
    win.addEventListener("mousemove", onPointerMove);
    doc.addEventListener("mouseleave", clearPointer);
    win.addEventListener("resize", scheduleBar);

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
