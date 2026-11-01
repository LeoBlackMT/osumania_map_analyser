// 动态 osu! 三角层。
//
// 三角形只在初始化时生成一次为 DOM 节点；向上漂浮的动画完全交给 CSS 合成器
// （只动 transform / opacity），因此运行期没有逐帧 JS 开销，适合压在节奏游戏
// 之上的实时悬浮窗。
//
// 颜色统一取自 --ma-accent（由 coverTheme.js 在换图时设置），所以封面主题一变，
// 整片三角会自动跟着换色，无需在这里碰颜色。

const TRIANGLE_COUNT = 18;

function rand(min, max) {
    return min + Math.random() * (max - min);
}

/**
 * 在 .triangle-field 容器内填充若干漂浮三角形。
 * @param {HTMLElement} [container] 三角容器；缺省时自动在卡片内查找。
 */
export function initTriangleField(container) {
    const field = container
        || document.querySelector(".main-card .triangle-field")
        || document.querySelector(".triangle-field");
    if (!field) {
        return;
    }

    // 避免重复填充（例如初始化被调用多次）。
    if (field.dataset.populated === "1") {
        return;
    }
    field.dataset.populated = "1";

    const prefersReducedMotion = typeof window !== "undefined"
        && typeof window.matchMedia === "function"
        && window.matchMedia("(prefers-reduced-motion: reduce)").matches;

    const fragment = document.createDocumentFragment();

    for (let i = 0; i < TRIANGLE_COUNT; i += 1) {
        const triangle = document.createElement("span");
        triangle.className = "triangle";

        const size = rand(24, 92);          // 边长 px：大小混搭，远近层次
        const left = rand(-6, 100);         // 水平位置 %
        const opacity = rand(0.05, 0.20);   // 透明度：始终很淡，不抢内容
        const duration = rand(11, 26);      // 单程时长 s：慢飘
        const delay = -rand(0, duration);   // 负延迟：一开始就分散在不同高度
        const drift = rand(-26, 26);        // 上升过程中的横向漂移 px
        const spin = rand(-18, 18);         // 轻微旋转 deg

        triangle.style.setProperty("--tri-size", `${size.toFixed(1)}px`);
        triangle.style.setProperty("--tri-left", `${left.toFixed(2)}%`);
        triangle.style.setProperty("--tri-opacity", opacity.toFixed(3));
        triangle.style.setProperty("--tri-drift", `${drift.toFixed(1)}px`);
        triangle.style.setProperty("--tri-spin", `${spin.toFixed(1)}deg`);

        // 减弱动效时不设置 animation-duration/delay，CSS 里已把动画整体关掉，
        // 三角保持静止，仅作为静态纹理的补充。
        if (!prefersReducedMotion) {
            triangle.style.animationDuration = `${duration.toFixed(2)}s`;
            triangle.style.animationDelay = `${delay.toFixed(2)}s`;
        }

        fragment.appendChild(triangle);
    }

    field.appendChild(fragment);

    // 三角从卡片底沿之下（bottom:-130px）一路升到卡片顶沿之上才循环无缝。
    // 原版把升程写死 720px，正好够固定高度卡片；但 Full 模式卡片会变高，
    // 写死值盖不到顶部，于是上半截始终空着。改成按卡片实测高度动态设
    // --tri-rise（高度 + 起止缓冲），让任意卡片高度都能覆盖到顶。
    trackFieldRise(field);
}

const TRI_RISE_BUFFER_PX = 260; // 起点 -130px + 最大三角 92px + 旋转与留白缓冲，确保两端都在裁剪区外

function getElementLayoutHeight(el) {
    if (!el) {
        return 0;
    }
    // 优先取未受 zoom 缩放影响的 CSS 布局像素（offsetHeight）
    if (typeof el.offsetHeight === "number" && el.offsetHeight > 0) {
        return el.offsetHeight;
    }
    // 退路：从 getBoundingClientRect 折算，除以页面根 zoom
    const rect = el.getBoundingClientRect();
    if (rect && rect.height > 0) {
        const zoom = (typeof document !== "undefined" && Number.parseFloat(document.documentElement?.style?.zoom)) || 1;
        return rect.height / (zoom > 0 ? zoom : 1);
    }
    return 0;
}

function trackFieldRise(field) {
    const card = field.closest(".main-card") || field.parentElement;
    if (!card) {
        return;
    }

    const updateRise = () => {
        const height = Math.max(getElementLayoutHeight(card), getElementLayoutHeight(field));
        if (height <= 0) {
            return;
        }
        field.style.setProperty("--tri-rise", `${Math.round(height + TRI_RISE_BUFFER_PX)}px`);
    };

    updateRise();

    if (typeof ResizeObserver === "function") {
        const observer = new ResizeObserver(updateRise);
        observer.observe(card);
    } else if (typeof window !== "undefined") {
        // 退路：没有 ResizeObserver 时，至少跟随窗口尺寸变化。
        window.addEventListener("resize", updateRise);
    }
}
