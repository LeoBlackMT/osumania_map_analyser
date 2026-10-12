// js/estimator/polarisEstimator.js
// Polaris —— 7K 表修正估计器（共享 DOM-free 纯函数模块；Node benchmark 与浏览器同一实现）。
//
// 定位：Sunny 的 star→段位表映射在 7K 上存在系统性偏差（变速响应、LN 全条群、层级形状）。
// Polaris 以 Sunny 为基线，对"star→level 连续映射"叠加行为数据拟合的平滑修正后输出标签：
//   level_side = starToLevel(表, star) + computePolarisCorrection({side, level, rate, lnRatio})
// 修正模型细节见 polarisCorrection.js；算法文档见 docs/polaris_algorithm.md。
//
// 作用域与回退：
//   - 仅 7K（columnCount === 7）；其他键数回退 Sunny（actualEstimatorAlgorithm = "Sunny"）。
//   - cvt（IN/HO）改图旗标下无拟合证据，回退 Sunny。
//   - star：本就是 Sunny 原始 sr（管线归一化集合无需加入）。
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

/**
 * 运行 Polaris 估计器。
 *
 * @param {string} osuText  .osu 文本
 * @param {object} options  与其他估算器同构（speedRate/odFlag/cvtFlag/withGraph/
 *   extendedEstimationRange/enableAlwaysShowLNDifficulty），可选 precomputedSunnyResult 复用 Sunny
 * @param {object|null} parsed  可选的已 process() OsuFileParser 实例
 * @returns {object} Sunny 结果 + 修正后的 estDiff/numericDifficulty；
 *   `actualEstimatorAlgorithm` 为 "Polaris"（7K 生效）或 "Sunny"（回退）
 */
export function runPolarisEstimatorFromText(osuText, options = {}, parsed = null) {
    const sunny = options.precomputedSunnyResult || runSunnyEstimatorFromText(osuText, options, parsed);

    const columnCount = Number(sunny.columnCount);
    const star = Number(sunny.star);
    const lnRatio = Number(sunny.lnRatio);
    const cvtText = String(options.cvtFlag ?? "").toUpperCase();
    const hasCvtFlag = cvtText.includes("IN") || cvtText.includes("HO");

    if (columnCount !== 7 || !Number.isFinite(star) || !Number.isFinite(lnRatio) || hasCvtFlag) {
        return { ...sunny, actualEstimatorAlgorithm: "Sunny" };
    }

    const keys = DAN_INDEX[7];
    const useExtended = options.extendedEstimationRange === true;
    const rcTable = (useExtended ? keys.RC.extended : keys.RC.default) ?? keys.RC.default;
    const lnTable = keys.LN?.default ?? null;

    const rate = Number(options.speedRate ?? 1.0) || 1.0;
    const rcMap = getLevelMap(rcTable);
    const rcBase = starToLevel(rcMap, star);
    const rcLevel = rcBase + computePolarisCorrection({ side: "RC", level: rcBase, rate });
    const rcLabel = levelToLabel(rcMap, rcLevel, star, rcTable);

    let lnLabel = null;
    let lnBase = null;
    let lnLevel = null;
    if (lnTable) {
        const lnMap = getLevelMap(lnTable);
        lnBase = starToLevel(lnMap, star);
        lnLevel = lnBase + computePolarisCorrection({ side: "LN", level: lnBase, rate, lnRatio });
        lnLabel = levelToLabel(lnMap, lnLevel, star, lnTable);
    }

    const showLnHalf = lnLabel !== null
        && !(lnRatio < 0.15 && options.enableAlwaysShowLNDifficulty !== true);
    const estDiff = showLnHalf ? `${rcLabel} || ${lnLabel}` : rcLabel;

    // numeric：仅当 star 位于 RC 表范围内时输出（表外沿用边界标签语义，不给外推数值）
    const rcInRange = star >= rcTable[0][0] && star <= rcTable[rcTable.length - 1][1];
    const numericDifficulty = rcInRange ? Number(rcLevel.toFixed(2)) : null;

    return {
        ...sunny,
        estDiff,
        numericDifficulty,
        numericDifficultyHint: null,
        actualEstimatorAlgorithm: "Polaris",
        debug: {
            rate,
            rcBase: Number(rcBase.toFixed(4)),
            rcLevel: Number(rcLevel.toFixed(4)),
            lnBase: lnBase === null ? null : Number(lnBase.toFixed(4)),
            lnLevel: lnLevel === null ? null : Number(lnLevel.toFixed(4)),
        },
    };
}
