// 从当前谱面背景图提取主题主色，设到 --ma-accent / --ma-cover 上，
// 给 osu! 三角主题（theme.css）和封面垫图用。
//
// 这是 astrbot 渲染桥里 cover_theme.py 的浏览器版：思路一致，整套都是
// 「尽力而为」—— 任何一步失败都退回默认 osu! 粉主题、不垫封面，绝不影响出图。
//
// tosu 的 HTTP 静态服务把当前谱面背景挂在 /files/beatmap/background，与
// analysis.js 取 .osu 文件用的 /files/beatmap/file 同源，所以 canvas 读像素
// 不会被跨域污染；即便被污染（异源打开），也只是取色失败退默认，封面仍可显示。

import { getSocketHost, state } from "./appContext.js";

const DEFAULT_ACCENT = "#232f5d";

// 取色用的缩略尺寸：越小越快，64 足够挑主色。
const SAMPLE_SIZE = 64;
// 颜色直方图量化位数：每通道保留高 4 位 -> 16 级 -> 最多 4096 个桶。
const QUANT_BITS = 4;

let themeRequestSeq = 0;
let lastThemedIdentity = "";

/** 把当前主题恢复成默认（暗色玻璃）或保持自定义颜色、清掉封面。 */
export function resetCoverTheme() {
    const root = document.documentElement;
    // If customBackgroundColor is active, keep it. Otherwise remove --ma-accent
    // so the :root default or osu theme takes over, effectively falling back
    // to dark glass style when cover art is off.
    if (state.customBackgroundColor && state.customBackgroundColor !== "#000000") {
        // Keep custom color — just remove cover image
        root.style.removeProperty("--ma-cover");
    } else {
        // No custom color: remove accent to fall back to dark glass / osu default
        root.style.removeProperty("--ma-accent");
        root.style.removeProperty("--ma-cover");
    }
    root.classList.remove("ma-has-cover");
    lastThemedIdentity = "";
}

/**
 * 给某张谱面套用封面主题。identity 用作缓存键与防抖序号，避免同图重复取色、
 * 以及旧图的异步结果覆盖新图。
 * @param {string} identity 谱面标识（socketHandlers 里算好的 beatmap identity）
 */
export async function applyCoverThemeForBeatmap(identity) {
    if (!identity) {
        return;
    }
    if (identity === lastThemedIdentity) {
        return;
    }

    const seq = (themeRequestSeq += 1);
    const host = getSocketHost();
    // identity 当 cache-buster：换图必然换 URL，强制 tosu 回当前图。
    const url = `http://${host}/files/beatmap/background?ts=${encodeURIComponent(identity)}`;

    let image;
    try {
        image = await loadImage(url);
    } catch {
        // 没有背景图（或加载失败）：退回默认主题。
        if (seq === themeRequestSeq) {
            resetCoverTheme();
        }
        return;
    }

    if (seq !== themeRequestSeq) {
        return; // 已有更新的换图请求，丢弃本次结果。
    }

    let accent = null;
    try {
        accent = extractAccentFromImage(image);
    } catch {
        accent = null; // 读像素被污染等：accent 退默认，封面仍照垫。
    }

    if (seq !== themeRequestSeq) {
        return;
    }

    // If customBackgroundColor is active, use it instead of the extracted accent
    if (state.customBackgroundColor && state.customBackgroundColor !== "#000000") {
        accent = state.customBackgroundColor;
    }

    const root = document.documentElement;
    root.style.setProperty("--ma-accent", accent || DEFAULT_ACCENT);
    // 封面直接复用同源 URL，无需再编码成 dataURI（实时窗不是截图）。
    root.style.setProperty("--ma-cover", `url("${url}")`);
    root.classList.add("ma-has-cover");
    lastThemedIdentity = identity;
}

function loadImage(url) {
    return new Promise((resolve, reject) => {
        const image = new Image();
        image.crossOrigin = "anonymous";

        const cleanup = () => {
            image.onload = null;
            image.onerror = null;
        };

        image.onload = () => {
            cleanup();
            if (image.naturalWidth > 0 && image.naturalHeight > 0) {
                resolve(image);
            } else {
                reject(new Error("Empty image"));
            }
        };
        image.onerror = () => {
            cleanup();
            reject(new Error("Image load failed"));
        };
        image.src = url;
    });
}

/** 挑一个鲜艳、有代表性的主色，返回 #rrggbb；失败抛错。 */
function extractAccentFromImage(image) {
    const canvas = document.createElement("canvas");
    canvas.width = SAMPLE_SIZE;
    canvas.height = SAMPLE_SIZE;
    const ctx = canvas.getContext("2d", { willReadFrequently: true });
    if (!ctx) {
        throw new Error("No 2D context");
    }

    ctx.drawImage(image, 0, 0, SAMPLE_SIZE, SAMPLE_SIZE);
    const { data } = ctx.getImageData(0, 0, SAMPLE_SIZE, SAMPLE_SIZE);

    // 量化直方图：桶 -> { count, r, g, b 累加 }
    const shift = 8 - QUANT_BITS;
    const buckets = new Map();
    let total = 0;

    for (let i = 0; i < data.length; i += 4) {
        const alpha = data[i + 3];
        if (alpha < 125) {
            continue; // 跳过近透明像素
        }
        const r = data[i];
        const g = data[i + 1];
        const b = data[i + 2];
        const key = ((r >> shift) << (QUANT_BITS * 2))
            | ((g >> shift) << QUANT_BITS)
            | (b >> shift);

        let bucket = buckets.get(key);
        if (!bucket) {
            bucket = { count: 0, r: 0, g: 0, b: 0 };
            buckets.set(key, bucket);
        }
        bucket.count += 1;
        bucket.r += r;
        bucket.g += g;
        bucket.b += b;
        total += 1;
    }

    if (total === 0) {
        throw new Error("No opaque pixels");
    }

    // 评分：够鲜艳 + 亮度适中（避开纯黑纯白）+ 占比别太小。与 cover_theme.py 一致。
    let bestScore = -1;
    let bestRgb = null;

    for (const bucket of buckets.values()) {
        const r = bucket.r / bucket.count;
        const g = bucket.g / bucket.count;
        const b = bucket.b / bucket.count;
        const [, l, s] = rgbToHls(r, g, b);
        const freq = bucket.count / total;
        let brightnessFit = 1.0 - Math.abs(l - 0.55) * 1.4;
        if (brightnessFit < 0.05) {
            brightnessFit = 0.05;
        }
        const score = Math.sqrt(freq) * (0.25 + s * 1.35) * brightnessFit;
        if (score > bestScore) {
            bestScore = score;
            bestRgb = [r, g, b];
        }
    }

    if (!bestRgb) {
        throw new Error("No accent candidate");
    }

    return normalizeAccent(bestRgb[0], bestRgb[1], bestRgb[2]);
}

/** 把主色压进一个「好看且可读」的区间，避免太暗/太灰/太刺眼。 */
function normalizeAccent(r, g, b) {
    let [h, l, s] = rgbToHls(r, g, b);

    if (s < 0.15) {
        // 近乎灰度：给一点饱和，贴着原有色相，呈现「带色调的石板蓝/灰」而非死灰。
        s = Math.max(s, 0.18);
        l = Math.min(Math.max(l, 0.43), 0.56);
    } else {
        s = Math.min(Math.max(s, 0.5), 0.9);
        l = Math.min(Math.max(l, 0.46), 0.62);
    }

    const [nr, ng, nb] = hlsToRgb(h, l, s);
    return `#${toHex(nr)}${toHex(ng)}${toHex(nb)}`;
}

function toHex(value) {
    const clamped = Math.max(0, Math.min(255, Math.round(value)));
    return clamped.toString(16).padStart(2, "0");
}

// --- colorsys 端口（输入 0..255，HLS 各分量 0..1，与 Python 顺序一致：h, l, s） ---

function rgbToHls(r, g, b) {
    const rn = r / 255;
    const gn = g / 255;
    const bn = b / 255;
    const max = Math.max(rn, gn, bn);
    const min = Math.min(rn, gn, bn);
    const l = (max + min) / 2;

    if (max === min) {
        return [0, l, 0];
    }

    const delta = max - min;
    const s = l > 0.5 ? delta / (2 - max - min) : delta / (max + min);

    let h;
    if (max === rn) {
        h = (gn - bn) / delta + (gn < bn ? 6 : 0);
    } else if (max === gn) {
        h = (bn - rn) / delta + 2;
    } else {
        h = (rn - gn) / delta + 4;
    }
    h /= 6;

    return [h, l, s];
}

function hlsToRgb(h, l, s) {
    if (s === 0) {
        const v = l * 255;
        return [v, v, v];
    }

    const q = l < 0.5 ? l * (1 + s) : l + s - l * s;
    const p = 2 * l - q;

    return [
        hueToChannel(p, q, h + 1 / 3) * 255,
        hueToChannel(p, q, h) * 255,
        hueToChannel(p, q, h - 1 / 3) * 255,
    ];
}

function hueToChannel(p, q, t) {
    let tt = t;
    if (tt < 0) tt += 1;
    if (tt > 1) tt -= 1;
    if (tt < 1 / 6) return p + (q - p) * 6 * tt;
    if (tt < 1 / 2) return q;
    if (tt < 2 / 3) return p + (q - p) * (2 / 3 - tt) * 6;
    return p;
}
