// js/estimator/polarisEstimator.js
// Polaris —— 表修正估计器（共享 DOM-free 纯函数模块；Node benchmark 与浏览器同一实现）。
//
// 定位：Sunny 的 star→段位表映射存在系统性偏差，Polaris 以 Sunny 为基线，对
// "star→level 连续映射"叠加行为数据拟合的平滑修正后输出标签：
//   level = starToLevel(表, star) + computePolarisCorrection({side, level, rate, lnRatio, keyCount})
// 修正模型细节与系数来源见 polarisCorrection.js；算法文档见 docs/polaris_algorithm.md。
//
// 作用域：**7K 专用**——RC 半 + LN 半都修（7K 的 Mixed 在接入 Polaris 前恒为 Sunny
// 基线，两边都是表映射产物）。
//   - 其他键数（4K/6K/10K/…）与 cvt（IN/HO）改图旗标：回退 Sunny
//     （actualEstimatorAlgorithm = "Sunny"）。4K 曾有 LN 半曲线，实机测试后随 4K 侧
//     全部回退。
//   - star：本就是 Sunny 原始 sr（管线归一化集合无需加入）；numeric：7K 取修正后的 RC level。
// 数据来源（拟合）：Mania Tracker 行为数据集，见文档；禁止修改为逐图/逐样本特判。

import { runSunnyEstimatorFromText } from "./sunnyEstimator.js";
import { DAN_INDEX } from "./intervals/index.js";
import { computePolarisCorrection, hasPolarisCurve } from "./polarisCorrection.js";

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

// Sunny 的 estDiff 拆成两半（无曲线的键数下作回退用）
function splitSunnyHalves(estDiff) {
    const parts = String(estDiff ?? "").split("||").map((part) => part.trim()).filter(Boolean);
    if (parts.length >= 2) {
        return { rc: parts[0], ln: parts[parts.length - 1] };
    }
    return { rc: parts[0] ?? "", ln: null };
}

// 单个半段的表校准：star → level →（加 Polaris 修正）→ 标签。
// 无表、或该 (键数, 侧) 没有曲线时返回 null —— 没有曲线的"校准结果"与表值逐位相同，
// 返回非 null 会让消费方误以为"这一半被校准过"（Mixed 的 6K 曾因此被标成 Polaris）。
// inRange 与数值输出口径一致：star 落在表范围内才给外推数值（表外沿用 `<` / `>` 边界标签语义）。
function calibrateHalf(table, { star, rate, lnRatio, side, keyCount }) {
    if (!table || !hasPolarisCurve(keyCount, side)) {
        return null;
    }
    const map = getLevelMap(table);
    const base = starToLevel(map, star);
    const level = base + computePolarisCorrection({ side, level: base, rate, lnRatio, keyCount });
    return {
        label: levelToLabel(map, level, star, table),
        base,
        level,
        inRange: star >= table[0][0] && star <= table[table.length - 1][1],
    };
}

/**
 * 某一个键数下**表映射两半**的 Polaris 校准结果（Polaris 与 Mixed 共用一份实现）。
 *
 * 两半在混合路由里都只是"表映射产物"，因此这是它们唯一的校准入口；共用同一份实现意味着
 * 选 Polaris 与选 Mixed 不可能对同一张图给出不同的标签（冒烟测试直接断言这一点）。
 * 哪一半返回非 null 由**曲线集**决定（`hasPolarisCurve`）：7K 两半都有，其余键数都没有——它们有区间表但没有拟合曲线，因此两半都返回 null，消费方据此保持原表映射（Mixed 的 4K/6K 即走 Sunny）。
 *
 * @param {object} input
 * @param {number} input.columnCount 键数
 * @param {number} input.star        Sunny 原始 sr
 * @param {number} input.rate        速率
 * @param {number} input.lnRatio     Sunny lnRatio（LN 项 H 的查找键）
 * @param {boolean} [input.extendedEstimationRange] RC 侧是否用扩展表（与 Sunny/Polaris 同开关）
 * @returns {{ rc: object|null, ln: object|null } | null} 无该键数的表或输入非有限时返回 null
 */
export function calibrateTableHalves({ columnCount, star, rate, lnRatio, extendedEstimationRange = false }) {
    const kc = Number(columnCount);
    const keys = DAN_INDEX[kc];
    const s = Number(star);
    const r = Number(rate);
    const lr = Number(lnRatio);
    if (!keys || !Number.isFinite(s) || !Number.isFinite(r) || !Number.isFinite(lr)) {
        return null;
    }
    const pick = (which) => (extendedEstimationRange ? keys[which]?.extended : keys[which]?.default)
        ?? keys[which]?.default ?? null;
    return {
        rc: calibrateHalf(pick("RC"), { star: s, rate: r, lnRatio: lr, side: "RC", keyCount: kc }),
        ln: calibrateHalf(pick("LN"), { star: s, rate: r, lnRatio: lr, side: "LN", keyCount: kc }),
    };
}

/** LN 半段的校准结果（calibrateTableHalves 的 LN 半边）。 */
export function calibrateLnLevel(input) {
    const halves = calibrateTableHalves(input);
    return halves ? halves.ln : null;
}

// 供 Mixed 直接把"Sunny 的 LN 半标签"换成校准后的标签（拿不到表或输入异常时保留原值）
export function calibrateLnHalfLabel(input, fallback = null) {
    const out = calibrateLnLevel(input);
    return out ? out.label : fallback;
}

/**
 * 运行 Polaris 估计器。
 *
 * @param {string} osuText  .osu 文本
 * @param {object} options  与其他估算器同构（speedRate/odFlag/cvtFlag/withGraph/
 *   extendedEstimationRange/enableAlwaysShowLNDifficulty），可选 precomputedSunnyResult 复用 Sunny
 * @param {object|null} parsed  可选的已 process() OsuFileParser 实例
 * @returns {object} Sunny 结果 + 修正后的 estDiff（7K 另含修正后的 numericDifficulty）；
 *   `actualEstimatorAlgorithm` 为 "Polaris"（7K 生效）或 "Sunny"（回退）
 */
export function runPolarisEstimatorFromText(osuText, options = {}, parsed = null) {
    const sunny = options.precomputedSunnyResult || runSunnyEstimatorFromText(osuText, options, parsed);

    const columnCount = Number(sunny.columnCount);
    const is7k = columnCount === 7;
    const star = Number(sunny.star);
    const lnRatio = Number(sunny.lnRatio);
    const cvtText = String(options.cvtFlag ?? "").toUpperCase();
    const hasCvtFlag = cvtText.includes("IN") || cvtText.includes("HO");

    if (!is7k || !Number.isFinite(star) || !Number.isFinite(lnRatio) || hasCvtFlag) {
        return { ...sunny, actualEstimatorAlgorithm: "Sunny" };
    }

    const rate = Number(options.speedRate ?? 1.0) || 1.0;

    // ── 两半都取自 calibrateTableHalves（与 Mixed 共用同一份实现，两边不可能给出不同答案）──
    const halves = calibrateTableHalves({
        columnCount,
        star,
        rate,
        lnRatio,
        extendedEstimationRange: options.extendedEstimationRange === true,
    });

    // RC 半：表校准（本估计器只对 7K 生效；其他键数在入口已回退 Sunny）
    const rcLabel = halves?.rc?.label ?? splitSunnyHalves(sunny.estDiff).rc;
    const rcBase = halves?.rc?.base ?? null;
    const rcLevel = halves?.rc?.level ?? null;

    // LN 半：表校准（与 Mixed 共用同一份实现，两边不可能给出不同答案）
    const lnLabel = halves?.ln?.label ?? null;
    const lnBase = halves?.ln?.base ?? null;
    const lnLevel = halves?.ln?.level ?? null;

    const showLnHalf = lnLabel !== null
        && !(lnRatio < 0.15 && options.enableAlwaysShowLNDifficulty !== true);
    const estDiff = showLnHalf ? `${rcLabel} || ${lnLabel}` : rcLabel;

    // numeric：取修正后的 RC level（且要求 star 在 RC 表范围内）
    let numericDifficulty = sunny.numericDifficulty;
    if (is7k && rcLevel !== null) {
        numericDifficulty = halves.rc.inRange ? Number(rcLevel.toFixed(2)) : null;
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
