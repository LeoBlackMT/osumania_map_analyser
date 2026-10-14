// js/estimator/companellaOffset.js
// Companella 系统性偏移校正（4K，RC 侧）—— 在 Companella（移植算法）输出之后应用，不改算法本体。
//
// 依据（详见 docs/features/companella-offset.md）：
//   - Mania Tracker 行为数据集（4.84M plays）中，Mixed 路由为 Companella 的 4K 谱面
//     （≥8 玩家，n=5621 chart×rate）系统性低估：`play-implied − Companella 数值`
//     整体均值 +0.443。这不是某张图的问题，而是该移植算法与数据分布之间的整体尺度差。
//   - **该尺度差按层级并不恒定**：level < 2.5 段只有 +0.19，level ≥ 3.5 段升到 +0.80，
//     差一个量级。常数版严格分组 CV MAE 改善 −5.22%；两段 + 1.0 宽线性过渡做到 −10.20%。
//   - **rate 无系统依赖**：1.0x / 1.5x / 0.75x 分别为 +0.441 / +0.404 / +0.951
//     （1.0x 与 1.5x 几乎一致，0.75x 样本少且 HT 侧本身不稳定），故只做层级。
//
// 为什么只作用于 RC 侧（LN 侧偏移 = 0）：
//   行为数据集对 LN 侧谱面的判定量是 **LN 半段**（`verdict_used = "LN half"`，
//   即 Sunny 星数过 LN 表的值），而 Companella 的数值是 **RC 半段**。二者不同量纲，
//   因此 `play-implied − companella_numeric`（LN 侧为 +0.786）是跨量纲之差，
//   不能当作偏移使用。LN 侧唯一可靠量纲上的证据只有 benchmark 的 102 行 ln 样本，
//   按项目"不为小样本单源段做针对性修复"的判据不足以支撑一条常数，
//   故 LN 侧不施加偏移（LN 侧现有修正由 lnCorrection.js 承担）。
//
// 为什么按"Companella 权重"缩放：
//   本偏移刻画的是 **Companella 分量**的尺度差，因此只应作用于输出值中属于
//   Companella 的那一部分。低难段的 0.5/0.5 Azusa⊕Companella 融合值只有一半来自
//   Companella，全额施加会把修正量放大到实际所需的约两倍，故按权重缩放
//   （融合值 weight = 0.5，纯 Companella 值 weight = 1）。
//
// 应用点：mixedEstimator.applyCompanellaToMixedResult，与 lnCorrection 同处 Companella 输出之后。
// 修正对象为 Companella 侧数值；estDiff 由修正后数值重派生（numeric 与标签同源）。

import { numericToRcLabel } from "./rcDifficultyFormat.js";

// RC 侧偏移（numeric 单位，作用在"纯 Companella 分量"上；正值为上调，Companella 原本低估）。
// 两段值 + 1.0 宽的线性过渡：硬阶梯（同一对分段值、无过渡）的 CV 与之等价（0.8166 vs 0.8178），
// 但会在过渡点造出约 0.6 的输出跳变——同一张图换个小数位就跳半级，用户可见；斜坡把它抹平。
export const COMPANELLA_OFFSET_RC_LOW = 0.19;
export const COMPANELLA_OFFSET_RC_HIGH = 0.804;
export const COMPANELLA_OFFSET_RC_RAMP_START = 2.5;
export const COMPANELLA_OFFSET_RC_RAMP_WIDTH = 1.0;

/**
 * 返回给定侧 / 层级上的 Companella 偏移量（未按权重缩放）。只有 RC 侧有值
 * （LN 侧见文件头说明），未知侧、LN 侧与非有限 level 一律返回 0。
 * @param {"RC"|"LN"|string} side
 * @param {number} level 层级查找键：**Companella 自身输出的数值**（分段值就是在纯 Companella 行上按该值拟合的）
 * @returns {number}
 */
export function computeCompanellaOffset(side, level) {
    if (side !== "RC") {
        return 0;
    }
    const lv = Number(level);
    if (!Number.isFinite(lv)) {
        return 0;
    }
    const t = Math.max(0, Math.min(1, (lv - COMPANELLA_OFFSET_RC_RAMP_START) / COMPANELLA_OFFSET_RC_RAMP_WIDTH));
    return COMPANELLA_OFFSET_RC_LOW + (COMPANELLA_OFFSET_RC_HIGH - COMPANELLA_OFFSET_RC_LOW) * t;
}

/**
 * 对 Companella 侧数值应用系统性偏移校正。
 *
 * @param {object} input
 * @param {number} input.numeric          当前数值（纯 Companella 或融合值）
 * @param {string} input.rcLabel          当前 RC 段标签（estDiff 的 RC 半边）
 * @param {string} input.side             谱面侧（"RC" | "LN"）
 * @param {number} input.level            层级查找键：Companella 自身输出的数值
 * @param {number} [input.companellaWeight] 该数值中 Companella 的权重（0~1，默认 1）。
 *                                        低难 0.5/0.5 融合路径传 0.5。
 * @returns {{ numeric: number, rcLabel: string, applied: boolean, offset: number }}
 *   未修正时原样返回（applied=false，保证不受影响的行逐位不变）
 */
export function applyCompanellaOffset({ numeric, rcLabel, side, level, companellaWeight = 1 }) {
    const weight = Number(companellaWeight);
    const scaled = computeCompanellaOffset(side, level) * (Number.isFinite(weight) ? weight : 0);
    const n = Number(numeric);
    // 仅在输入本身是有效有限数（非 null / 空串）且缩放后偏移为正时生效。
    if (numeric === null || numeric === undefined || numeric === "" || typeof numeric === "boolean") {
        return { numeric, rcLabel, applied: false, offset: 0 };
    }
    if (!(scaled > 0) || !Number.isFinite(n)) {
        return { numeric, rcLabel, applied: false, offset: 0 };
    }
    const corrected = Number((n + scaled).toFixed(2));
    return { numeric: corrected, rcLabel: numericToRcLabel(corrected), applied: true, offset: scaled };
}
