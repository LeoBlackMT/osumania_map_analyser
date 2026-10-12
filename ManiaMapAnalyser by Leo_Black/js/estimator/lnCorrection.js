// js/estimator/lnCorrection.js
// 4K LN 主导谱面的数值后修正 —— 在 Companella（移植算法）输出之后应用，不改算法本体。
//
// 依据（详见 docs/features/ln-correction.md）：
//   - benchmark ln 行（102 条）：Mixed 数值 bias +1.02（低估）；
//   - Mania Tracker 4K LN 侧（行为数据）：+0.2~0.5 低估。
// 取保守幅度 0.5（纯 benchmark 最优值 ≥0.8 未采纳，避免小样本/单源过拟合），
// 由 lnRatio 渐进门控：≤0.375 不触发、0.55 起全量。
//
// 应用点：mixedEstimator.applyCompanellaToMixedResult（Companella 结果合并处，4K 计划必经此路）。
// 修正对象为 RC 字段数值；estDiff 由修正后数值重派生（numeric 与标签同源，遵循既有硬约束）。

import { numericToRcLabel } from "./rcDifficultyFormat.js";

export const LN_CORRECTION_MAGNITUDE = 0.5;
export const LN_CORRECTION_RAMP_LO = 0.375;
export const LN_CORRECTION_RAMP_HI = 0.55;

/**
 * 计算 lnRatio 门控下的 4K LN 修正量（numeric 单位）。返回 0 表示不修正。
 * 上界 1.5 排除 forceSunnyWindow 的 4e65 哨兵（该路径不触发修正）。
 */
export function computeLnRatioCorrection(lnRatio) {
    const r = Number(lnRatio);
    if (!(r > LN_CORRECTION_RAMP_LO && r <= 1.5)) {
        return 0;
    }
    const t = Math.min(1, (r - LN_CORRECTION_RAMP_LO) / (LN_CORRECTION_RAMP_HI - LN_CORRECTION_RAMP_LO));
    return LN_CORRECTION_MAGNITUDE * t;
}

/**
 * 对 RC 字段应用 4K LN 后修正。
 *
 * @param {object} input
 * @param {number} input.numeric  当前 numericDifficulty
 * @param {string} input.rcLabel  当前 RC 段标签（estDiff 的 RC 半边）
 * @param {number} input.lnRatio  谱面 lnRatio
 * @returns {{ numeric: number, rcLabel: string, applied: boolean }}
 *   未修正时原样返回（applied=false，保证非 LN 行逐位不变）
 */
export function applyLnCorrectionToRcFields({ numeric, rcLabel, lnRatio }) {
    const corr = computeLnRatioCorrection(lnRatio);
    const n = Number(numeric);
    if (!(corr > 0) || !Number.isFinite(n)) {
        return { numeric, rcLabel, applied: false };
    }
    const corrected = Number((n + corr).toFixed(2));
    return { numeric: corrected, rcLabel: numericToRcLabel(corrected), applied: true };
}
