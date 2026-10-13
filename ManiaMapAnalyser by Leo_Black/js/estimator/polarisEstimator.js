// js/estimator/polarisEstimator.js
// Polaris —— 表修正估计器（共享 DOM-free 纯函数模块；Node benchmark 与浏览器同一实现）。
//
// 定位：Sunny 的 star→段位表映射存在系统性偏差，Polaris 以 Sunny 为基线，对
// "star→level 连续映射"叠加行为数据拟合的平滑修正后输出标签：
//   level = starToLevel(表, star) + computePolarisCorrection({side, level, rate, lnRatio, keyCount})
// 修正模型细节与系数来源见 polarisCorrection.js；算法文档见 docs/polaris_algorithm.md。
//
// 作用域（两张表各修各自需要修的那一半）：
//   - **7K**：RC 半 + LN 半都修。7K 的 Mixed 恒为 Sunny 基线，两边都是表映射产物。
//   - **4K**：**只修 LN 半**。4K 的 RC 半最终答案来自 Mixed 的路由链（Roxy/Azusa/Daniel/
//     Companella），不是表映射，修它没有依据；而 LN 半恒由 Sunny 的 LN 表给出——
//     LN 侧谱面在 Mixed 里 98.7% 落到纯 Companella 的 L2 路径（Azusa 的 UnsupportedLN
//     门把 L1 融合分支压到 [0.15, 0.18] 这 0.03 宽的窗口里），所以显示的 LN 标签
//     此前从未被任何修正触碰过。
//   - 其他键数与 cvt（IN/HO）改图旗标：回退 Sunny（actualEstimatorAlgorithm = "Sunny"）。
//   - star：本就是 Sunny 原始 sr（管线归一化集合无需加入）；numeric：7K 取修正后的 RC level，
//     4K 保持 Sunny 的数值（RC 半未修）。
// 数据来源（拟合）：Mania Tracker 行为数据集，见文档；禁止修改为逐图/逐样本特判。

import { runSunnyEstimatorFromText } from "./sunnyEstimator.js";
import { DAN_INDEX } from "./intervals/index.js";
import { computePolarisCorrection } from "./polarisCorrection.js";

const TIER_OFFSETS = Object.freeze({
    "low": -0.4,
    "mid/low": -0.2,
    "mid": 0,
    "mid/high": 0.2,
    "high": 0.4,
});

// 区间表 → 连续数值映射（与 Mania Tracker 拟合所用的 star_to_numeric 同构）：
// 每个 band [lo, hi, label] 给出两个点 (lo, n−0.1)、(hi, n+0.1)；相邻 band 端点值相等（连续）。
// n 由 label 导出：层级 = 数字 token（Intro 减 3）或"新前缀 = 上一层 +1"，再加 tier 偏移。
// 特例：扩展表的 "Wild N" 前缀按序延伸（Stellium 14 → Wild 8 = 15 → Wild 9 = 16）。
function buildLevelMap(table) {
    const seen = new Map();
    const xs = [];
    const ys = [];
    const bandLevels = [];
    const bandLabels = [];
    let prevLevel = null;
    for (const [lo, hi, label] of table) {
        const parts = label.split(" ");
        const tier = parts[parts.length - 1];
        const base = parts.slice(0, -1).join(" ");
        if (!seen.has(base)) {
            const tok = base.split(" ").pop();
            let level;
            if (/^\d+$/.test(tok) && !base.startsWith("Wild")) {
                level = base.startsWith("Intro") ? Number(tok) - 3 : Number(tok);
            } else {
                level = prevLevel === null ? 0 : prevLevel + 1;
            }
            seen.set(base, level);
            prevLevel = level;
        }
        const value = seen.get(base) + TIER_OFFSETS[tier];
        const pushPoint = (x, y) => {
            if (xs.length === 0 || x > xs[xs.length - 1]) {
                xs.push(x);
                ys.push(y);
            }
        };
        pushPoint(lo, value - 0.1);
        pushPoint(hi, value + 0.1);
        bandLevels.push(value);
        bandLabels.push(label);
    }
    // 端点外推斜率（与拟合脚本同式：首/末 5 个结点的平均斜率）
    const last = xs.length - 1;
    const i0 = Math.min(10, last);
    const i1 = Math.max(0, last - 10);
    const s0 = xs[i0] > xs[0] ? (ys[i0] - ys[0]) / (xs[i0] - xs[0]) : 0;
    const s1 = xs[last] > xs[i1] ? (ys[last] - ys[i1]) / (xs[last] - xs[i1]) : 0;
    return { xs, ys, s0, s1, bandLevels, bandLabels };
}

const levelMapCache = new WeakMap();

function getLevelMap(table) {
    let map = levelMapCache.get(table);
    if (!map) {
        map = buildLevelMap(table);
        levelMapCache.set(table, map);
    }
    return map;
}

// star → 连续 level（分段落线性；表范围外按端点斜率线性延伸）
function starToLevel(map, star) {
    const { xs, ys, s0, s1 } = map;
    const x = Number(star);
    if (x <= xs[0]) {
        return ys[0] + (x - xs[0]) * s0;
    }
    const last = xs.length - 1;
    if (x >= xs[last]) {
        return ys[last] + (x - xs[last]) * s1;
    }
    let lo = 0;
    let hi = last;
    while (hi - lo > 1) {
        const mid = (lo + hi) >> 1;
        if (xs[mid] <= x) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    const t = (x - xs[lo]) / (xs[hi] - xs[lo]);
    return ys[lo] + (ys[hi] - ys[lo]) * t;
}

// 连续 level → 区间标签（最近 band 中心；表范围外沿用 `<` / `>` 边界语义，与 estDiff 一致）
function levelToLabel(map, level, star, table) {
    const first = table[0];
    const last = table[table.length - 1];
    if (star < first[0]) {
        return `< ${first[2]}`;
    }
    if (star > last[1]) {
        return `> ${last[2]}`;
    }
    let best = 0;
    let bestDistance = Infinity;
    for (let i = 0; i < map.bandLevels.length; i += 1) {
        const distance = Math.abs(level - map.bandLevels[i]);
        if (distance < bestDistance) {
            bestDistance = distance;
            best = i;
        }
    }
    return map.bandLabels[best];
}

// Sunny 的 estDiff 拆成两半：4K 下 RC 半原样沿用（Polaris 不修 4K 的 RC 半）
function splitSunnyHalves(estDiff) {
    const parts = String(estDiff ?? "").split("||").map((part) => part.trim()).filter(Boolean);
    if (parts.length >= 2) {
        return { rc: parts[0], ln: parts[parts.length - 1] };
    }
    return { rc: parts[0] ?? "", ln: null };
}

/**
 * 运行 Polaris 估计器。
 *
 * @param {string} osuText  .osu 文本
 * @param {object} options  与其他估算器同构（speedRate/odFlag/cvtFlag/withGraph/
 *   extendedEstimationRange/enableAlwaysShowLNDifficulty），可选 precomputedSunnyResult 复用 Sunny
 * @param {object|null} parsed  可选的已 process() OsuFileParser 实例
 * @returns {object} Sunny 结果 + 修正后的 estDiff（7K 另含修正后的 numericDifficulty）；
 *   `actualEstimatorAlgorithm` 为 "Polaris"（4K/7K 生效）或 "Sunny"（回退）
 */
export function runPolarisEstimatorFromText(osuText, options = {}, parsed = null) {
    const sunny = options.precomputedSunnyResult || runSunnyEstimatorFromText(osuText, options, parsed);

    const columnCount = Number(sunny.columnCount);
    const is7k = columnCount === 7;
    const is4k = columnCount === 4;
    const star = Number(sunny.star);
    const lnRatio = Number(sunny.lnRatio);
    const cvtText = String(options.cvtFlag ?? "").toUpperCase();
    const hasCvtFlag = cvtText.includes("IN") || cvtText.includes("HO");

    if ((!is7k && !is4k) || !Number.isFinite(star) || !Number.isFinite(lnRatio) || hasCvtFlag) {
        return { ...sunny, actualEstimatorAlgorithm: "Sunny" };
    }

    const keys = DAN_INDEX[columnCount];
    const useExtended = options.extendedEstimationRange === true;
    const lnTable = keys.LN?.default ?? null;

    const rate = Number(options.speedRate ?? 1.0) || 1.0;

    // ── RC 半：7K 由表校准；4K 原样沿用 Sunny（4K RC 的最终答案来自路由链，不是表）──
    let rcLabel = "";
    let rcBase = null;
    let rcLevel = null;
    if (is7k) {
        const rcTable = (useExtended ? keys.RC.extended : keys.RC.default) ?? keys.RC.default;
        const rcMap = getLevelMap(rcTable);
        rcBase = starToLevel(rcMap, star);
        rcLevel = rcBase + computePolarisCorrection({ side: "RC", level: rcBase, rate, keyCount: 7 });
        rcLabel = levelToLabel(rcMap, rcLevel, star, rcTable);
    } else {
        rcLabel = splitSunnyHalves(sunny.estDiff).rc;
    }

    // ── LN 半：4K / 7K 都校准（LN 标签在两边的来源都是 Sunny 的 LN 表）──
    let lnLabel = null;
    let lnBase = null;
    let lnLevel = null;
    if (lnTable) {
        const lnMap = getLevelMap(lnTable);
        lnBase = starToLevel(lnMap, star);
        lnLevel = lnBase + computePolarisCorrection({
            side: "LN",
            level: lnBase,
            rate,
            lnRatio,
            keyCount: columnCount,
        });
        lnLabel = levelToLabel(lnMap, lnLevel, star, lnTable);
    }

    const showLnHalf = lnLabel !== null
        && !(lnRatio < 0.15 && options.enableAlwaysShowLNDifficulty !== true);
    const estDiff = showLnHalf ? `${rcLabel} || ${lnLabel}` : rcLabel;

    // numeric：7K 取修正后的 RC level（且要求 star 在 RC 表范围内）；4K 保持 Sunny 的数值
    let numericDifficulty = sunny.numericDifficulty;
    if (is7k) {
        const rcTable = (useExtended ? keys.RC.extended : keys.RC.default) ?? keys.RC.default;
        const rcInRange = star >= rcTable[0][0] && star <= rcTable[rcTable.length - 1][1];
        numericDifficulty = rcInRange ? Number(rcLevel.toFixed(2)) : null;
    }

    return {
        ...sunny,
        estDiff,
        numericDifficulty,
        numericDifficultyHint: null,
        actualEstimatorAlgorithm: "Polaris",
        debug: {
            rate,
            keyCount: columnCount,
            rcBase: rcBase === null ? null : Number(rcBase.toFixed(4)),
            rcLevel: rcLevel === null ? null : Number(rcLevel.toFixed(4)),
            lnBase: lnBase === null ? null : Number(lnBase.toFixed(4)),
            lnLevel: lnLevel === null ? null : Number(lnLevel.toFixed(4)),
        },
    };
}
