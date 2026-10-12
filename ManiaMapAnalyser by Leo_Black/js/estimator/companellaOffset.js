// js/estimator/companellaOffset.js
// Companella 系统性偏移校正（4K）—— 在 Companella（移植算法）输出之后应用，不改算法本体。
//
// 依据（详见 docs/features/companella-offset.md）：
//   - Mania Tracker 行为数据集（4.84M plays）中，Mixed 路由为 Companella 的 4K in-table 谱面
//     （≥8 玩家，n=8278 chart×rate）系统性低估：play-implied − Companella 数值 的均值
//     为 RC +0.443、LN +0.786；
//   - 即 Companella 本体整体偏"宽松"，同一难度层级下其输出平均低于玩家实际表现。
//     这不是某张图的问题，而是该移植算法与数据分布之间的整体尺度差，故按侧（RC/LN）
//     取常数偏移，不做层级依赖（finer binning 无额外 CV 收益，见文档）。
//
// 严格验证（按 beatmap_id 分组的 10 折 CV，训练集拟合常数、测试集应用）：
//   - 在"无任何其他修正"的 Companella-only 基线上：MAE 0.9708 → 0.8813（+9.21%）；
//   - 叠加现有 4K LN 修正后：MAE 0.9211 → 0.8765（+4.84%，两者互补不冲突）。
//
// 应用点：mixedEstimator.applyCompanellaToMixedResult，与 lnCorrection 同处 Companella 输出之后。
// 修正对象为 Companella 侧数值；estDiff 由修正后数值重派生（numeric 与标签同源）。

import { numericToRcLabel } from "./rcDifficultyFormat.js";

// 侧常数（numeric 单位）。正值为上调（Companella 原本低估）。
export const COMPANELLA_OFFSET_RC = 0.443;
export const COMPANELLA_OFFSET_LN = 0.786;

/**
 * 返回给定侧（RC/LN）的 Companella 常数偏移量。未知侧返回 0。
 * @param {"RC"|"LN"|string} side
 * @returns {number}
 */
export function computeCompanellaOffset(side) {
    if (side === "RC") {
        return COMPANELLA_OFFSET_RC;
    }
    if (side === "LN") {
        return COMPANELLA_OFFSET_LN;
    }
    return 0;
}

/**
 * 对 Companella 侧数值应用系统性偏移校正。
 *
 * @param {object} input
 * @param {number} input.numeric  当前 Companella numericDifficulty（或融合后数值）
 * @param {string} input.rcLabel  当前 RC 段标签（estDiff 的 RC 半边）
 * @param {string} input.side     谱面侧（"RC" | "LN"）
 * @returns {{ numeric: number, rcLabel: string, applied: boolean }}
 *   未修正时原样返回（applied=false，保证不受影响的行逐位不变）
 */
export function applyCompanellaOffset({ numeric, rcLabel, side }) {
    const offset = computeCompanellaOffset(side);
    const n = Number(numeric);
    // 仅在输入本身是有效有限数（非 null / 空串）且偏移为正时生效。
    if (numeric === null || numeric === undefined || numeric === "" || typeof numeric === "boolean") {
        return { numeric, rcLabel, applied: false };
    }
    if (!(offset > 0) || !Number.isFinite(n)) {
        return { numeric, rcLabel, applied: false };
    }
    const corrected = Number((n + offset).toFixed(2));
    return { numeric: corrected, rcLabel: numericToRcLabel(corrected), applied: true };
}
