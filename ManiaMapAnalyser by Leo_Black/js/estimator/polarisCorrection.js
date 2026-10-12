// js/estimator/polarisCorrection.js
// Polaris 修正模型（7K）—— 共享 DOM-free 纯函数模块。
//
// 模型：corr(side, level, rate, lnRatio) = B_side(level) + R_side(level)·f(rate) + H(lnRatio)
//   - B：速率 1.0x 下的层级基线（表映射 star→level 的系统偏差）
//   - R：DT 增量曲线（同一层级下 1.5x 相对 1.0x 的差值；f(rate)=clamp((rate−1)/0.5, 0, 1)）
//   - H：LN 侧 lnRatio 项（全 LN / 反手释放群的额外低估）
//   - rate < 1.0 不修正（HT 侧两种统计口径方向不一致、幅度小，见文档）
//
// 系数来源：Mania Tracker 行为数据集（2026-10 快照；in-table 且 ≥8 玩家，n=6899 chart×rate），
// 经联合（backfitting）拟合 + 按谱面分组 70/30 CV 验证（见 docs/polaris_algorithm.md）。
// 曲线结点间距 0.5 level，连续线性插值、端外截断——调整系数时同步跑拟合脚本与 CV。

export const POLARIS_LEVEL_KNOTS = Object.freeze([
    -0.75, -0.25, 0.25, 0.75, 1.25, 1.75, 2.25, 2.75, 3.25, 3.75,
    4.25, 4.75, 5.25, 5.75, 6.25, 6.75, 7.25, 7.75, 8.25, 8.75,
    9.25, 9.75, 10.25, 10.75, 11.25, 11.75, 12.25, 12.75, 13.25,
]);

const B_RC = Object.freeze([
    -0.376, -0.376, -0.228, 0.003, 0.116, 0.164, 0.252, 0.397, 0.486, 0.36,
    0.143, 0.042, 0.11, 0.209, 0.229, 0.223, 0.238, 0.212, 0.09, -0.033,
    -0.05, 0.012, 0.041, 0.039, 0.032, 0.0, -0.014, -0.036, -0.087,
]);

const R_RC = Object.freeze([
    0.022, 0.022, 0.0, 0.0, 0.077, 0.0, 0.0, 0.0, 0.123, 0.441,
    0.567, 0.492, 0.678, 0.945, 0.914, 0.833, 0.926, 1.064, 1.133, 1.166,
    1.098, 0.957, 0.833, 0.729, 0.633, 0.511, 0.42, 0.324, 0.187,
]);

const B_LN = Object.freeze([
    -0.054, -0.054, -0.054, -0.054, -0.068, -0.066, -0.051, -0.023, 0.057, 0.044,
    -0.017, 0.053, 0.13, 0.174, 0.118, -0.113, -0.251, -0.228, -0.225, -0.296,
    -0.312, -0.182, -0.043, 0.01, -0.007, -0.084, -0.172, -0.191, -0.165,
]);

const R_LN = Object.freeze([
    0.144, 0.144, 0.144, 0.144, 0.144, 0.144, 0.144, 0.144, 0.144, 0.144,
    0.144, 0.144, 0.144, 0.144, 0.144, 0.144, 0.144, 0.144, 0.18, 0.253,
    0.289, 0.289, 0.303, 0.29, 0.277, 0.236, 0.155, 0.241, 0.381,
]);

// LN 侧 lnRatio 项：[lnRatio, 修正量]（首末结点外截断；lnRatio < 0.375 的 LN 半边修正趋近于 0）
const H_LN = Object.freeze([
    [0.4375, 0.06],
    [0.575, 0.264],
    [0.725, 0.306],
    [0.85, 0.136],
    [0.935, 0.429],
    [0.99, 1.001],
]);

// 与顶点一一对应的曲线插值（线性、端外截断）
function interpCurve(values, x) {
    const knots = POLARIS_LEVEL_KNOTS;
    if (x <= knots[0]) {
        return values[0];
    }
    if (x >= knots[knots.length - 1]) {
        return values[values.length - 1];
    }
    for (let i = 0; i < knots.length - 1; i += 1) {
        if (x >= knots[i] && x <= knots[i + 1]) {
            const t = (x - knots[i]) / (knots[i + 1] - knots[i]);
            return values[i] + (values[i + 1] - values[i]) * t;
        }
    }
    return values[values.length - 1];
}

// LN 项插值（[x, y] 结点对，端外截断）
function interpKnots(pairs, x) {
    if (x <= pairs[0][0]) {
        return pairs[0][1];
    }
    if (x >= pairs[pairs.length - 1][0]) {
        return pairs[pairs.length - 1][1];
    }
    for (let i = 0; i < pairs.length - 1; i += 1) {
        const [x0, y0] = pairs[i];
        const [x1, y1] = pairs[i + 1];
        if (x >= x0 && x <= x1) {
            const t = x1 > x0 ? (x - x0) / (x1 - x0) : 0;
            return y0 + (y1 - y0) * t;
        }
    }
    return pairs[pairs.length - 1][1];
}

/**
 * 计算 Polaris 修正量（numeric 单位）。
 *
 * @param {object} input
 * @param {"RC"|"LN"} input.side  谱面侧（与 Mixed/数据集的 0.375 lnRatio 切分同语义）
 * @param {number} input.level    表映射得到的连续 level（starToLevel 输出；对应数据集的 leo_continuous）
 * @param {number} input.rate     速率（speedRate；1.0 = NM，1.5 = DT）
 * @param {number} [input.lnRatio]  LN 侧使用；RC 侧忽略
 * @returns {number} 修正量（可为负）；rate < 1.0 时返回 0
 */
export function computePolarisCorrection({ side, level, rate, lnRatio = 0 }) {
    const lv = Number(level);
    const r = Number(rate);
    if (!Number.isFinite(lv) || !Number.isFinite(r) || r < 1.0) {
        return 0;
    }
    const isLn = side === "LN";
    let corr = interpCurve(isLn ? B_LN : B_RC, lv);
    if (r > 1.0) {
        corr += interpCurve(isLn ? R_LN : R_RC, lv) * Math.min(1.0, (r - 1.0) / 0.5);
    }
    if (isLn) {
        corr += interpKnots(H_LN, Number(lnRatio));
    }
    return corr;
}
