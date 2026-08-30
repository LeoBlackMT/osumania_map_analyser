// js/estimator/marathonCorrection.js
// 马拉松时长修正（Marathon Duration Correction）—— 共享 DOM-free 纯函数模块。
//
// 灵感来源：Dan-Overlay pipeline.py `_merge_primary_and_mina` 的马拉松时长修正
// （>300s + 技能均衡 → 时长修正、高难度 taper 渐减、只降不升）。
// 本模块将修正对象从 SR/DP 改为本项目的 `numericDifficulty`（段位数值，
// Roxy/Azusa 的语义输出），taper 从 SR 域改为 numeric 域，
// 修正式采用对数饱和（次线性）以保持相邻段位课程的相对顺序（reform 8th/9th 验收项）。
//
// 设计决策：
// - 修正对象：numericDifficulty。estDiff 由调用方同步重派生
//   （estDiff = numericToRcLabel(newNumeric)）；star 在本插件展示统一为 Sunny raw，
//   归一化会覆盖，无需在这里重算。
// - 时长惩罚对数饱和：corr = scale * ln(1 + excessMin)。线性随时长增长的惩罚
//   会翻转相邻段位课程的相对顺序（修正差 > base 差距）；对数在长端收敛修正差，
//   让排序由估算器 base 差支配。参数经 course 样本网格校准（见
//   docs/features/marathon-correction.md §8）。
// - taper：numeric <= 10 全量修正；10 ~ 16 线性降至 0；>= 16 不修正。
//   理由：低段位（Reform 1~10 马拉松课程包）时长虚高最严重，高段位校准稳定
//   （与 Dan-Overlay "SR>=7 不修"的机制意图一致，只是换到 numeric 域）。
// - 均衡条件：MSD skillsets 聚合出 4 个技能（jack/stream/stamina/tech），
//   max/total < 0.45 才触发——防止"真 marathon-jack 图"被时长误伤。
//   无 MSD 输入（ett 不可用）→ 返回 0，缺信号不动作。
// - 只降不升：应用后数值严格不高于原值。
//
// 两端（浏览器 worker / Node benchmark）同一实现，禁止 DOM/state 依赖。

import { numericToRcLabel } from "./rcDifficultyFormat.js";

// ── 参数 ────────────────────────────────────────────────────────────────

export const MARATHON_DURATION_THRESHOLD_S = 300;
export const MARATHON_CORRECTION_SCALE = 0.40;   // 对数域系数（v2.0.2 排序约束校准，见 docs/features/marathon-correction.md §8）
export const MARATHON_CORRECTION_CAP = 0.50;      // numeric 上限（排序约束校准）
export const MARATHON_BALANCE_RATIO = 0.45;       // max/total 均衡阈值
export const MARATHON_TAPER_LO = 10;
export const MARATHON_TAPER_HI = 16;

// ── 技能聚合（ett values 键名：首字母大写，见 js/ett/calc.js）─

export function aggregateSkillsets(ettValues) {
    if (!ettValues || typeof ettValues !== "object") {
        return null;
    }
    const jk = Math.max(
        Number(ettValues.JackSpeed) || 0,
        Number(ettValues.Chordjack) || 0,
    );
    const st = Math.max(
        Number(ettValues.Stream) || 0,
        Number(ettValues.Jumpstream) || 0,
    );
    const te = Number(ettValues.Technical) || 0;
    const en = 0.7 * (Number(ettValues.Stamina) || 0)
        + 0.3 * (Number(ettValues.Handstream) || 0);
    const total = jk + st + te + en;
    if (!(total > 1)) {
        return null;
    }
    return { jk, st, te, en, total };
}

// ── 核心计算 ─────────────────────────────────────────────────────────────

/**
 * 计算修正量（numeric 单位）。返回 0 表示不修正。
 *
 * @param {object} input
 * @param {number} input.durationS  谱面 drain 时长（秒，未按速率缩放）
 * @param {object|null} [input.ettValues]  ett values（Overall/Stream/... 大写键）
 * @param {number} [input.numeric]  修正前的 numericDifficulty（用于 taper）
 * @param {object} [params]  可选参数覆盖（调参/校准用；缺省 = 模块常量默认）
 * @returns {number} 修正量；0 = 不修正（时长不足/技能不均衡/无 MSD/数值无效/taper 归零）
 */
export function computeMarathonCorrection({ durationS, ettValues = null, numeric = null }, params = null) {
    const p = {
        thresholdS: MARATHON_DURATION_THRESHOLD_S,
        scale: MARATHON_CORRECTION_SCALE,
        cap: MARATHON_CORRECTION_CAP,
        balanceRatio: MARATHON_BALANCE_RATIO,
        taperLo: MARATHON_TAPER_LO,
        taperHi: MARATHON_TAPER_HI,
        ...(params || {}),
    };
    if (!(Number(durationS) > p.thresholdS)) {
        return 0;
    }
    // 严格数值类型判断：null/undefined/NaN 一律跳过
    // （Number(null)=0 会把"无效估算"误当 0 难度修正，Roxy scope 外结果为 null）
    if (typeof numeric !== "number" || !Number.isFinite(numeric)) {
        return 0;
    }
    const num = numeric;

    // 技能均衡检查（无 MSD → 缺信号不动作）
    const agg = aggregateSkillsets(ettValues);
    if (!agg || Math.max(agg.jk, agg.st, agg.te, agg.en) / agg.total >= p.balanceRatio) {
        return 0;
    }

    // 超出分钟数 → 对数饱和修正（次线性）：corr = scale * ln(1 + excessMin)。
    // 线性随时长增长会翻转相邻段位课程的相对顺序（修正差 > base 差距，reform 8th/9th 验收项），
    // 对数在长端收敛修正差，让排序由估算器 base 差支配。封顶 cap。
    const excessMin = (Number(durationS) - p.thresholdS) / 60;
    const raw = Math.min(p.cap, p.scale * Math.log(1 + excessMin));
    if (raw <= 0) {
        return 0;
    }

    // numeric taper：<= taperLo 全量；>= taperHi 归零；中间线性渐减
    let taper = 1;
    if (num >= p.taperHi) {
        taper = 0;
    } else if (num > p.taperLo) {
        taper = (p.taperHi - num) / (p.taperHi - p.taperLo);
    }
    if (!(taper > 0)) {
        return 0;
    }

    return raw * taper;
}

/**
 * 只降不升地应用修正。
 * @param {number} numeric  原 numericDifficulty
 * @param {number} corr     computeMarathonCorrection 的返回值
 * @returns {number} 修正后的 numericDifficulty
 */
export function applyMarathonCorrectionToNumeric(numeric, corr) {
    const n = Number(numeric);
    if (!Number.isFinite(n) || !(corr > 0)) {
        return n;
    }
    return n - corr; // corr>0 必定使结果降低；边界 clamp 由调用方维持
}

/**
 * 对 RC 估算结果应用修正并同步重派生 estDiff（numericToRcLabel）。
 * 只处理 numericDifficulty 与 estDiff 两个字段；star 不在此处理
 * （插件展示路径中 Azusa/Roxy/Mixed 的 star 会被归一化为 Sunny raw，
 * 见 runAnalysisPipeline 归一化段）。
 *
 * @param {object} result  估算结果（含 numericDifficulty / estDiff）
 * @param {object} input   computeMarathonCorrection 的输入（durationS/ettValues）
 * @returns {object} 新的结果对象（未修正时返回原引用）
 */
export function applyMarathonCorrectionToRcResult(result, input) {
    if (!result || typeof result !== "object") {
        return result;
    }
    const numeric = result.numericDifficulty;
    // 同 computeMarathonCorrection：严格类型判断，null（scope 外/无效）不修正
    if (typeof numeric !== "number" || !Number.isFinite(numeric)) {
        return result;
    }
    const corr = computeMarathonCorrection({
        durationS: input?.durationS,
        ettValues: input?.ettValues ?? null,
        numeric,
    });
    if (!(corr > 0)) {
        return result;
    }

    const corrected = applyMarathonCorrectionToNumeric(numeric, corr);
    return {
        ...result,
        numericDifficulty: Number(corrected.toFixed(2)),
        estDiff: numericToRcLabel(corrected),
    };
}