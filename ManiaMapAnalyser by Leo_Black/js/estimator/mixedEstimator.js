import { runDanielEstimatorFromText } from "./danielEstimator.js";
import { runSunnyEstimatorFromText } from "./sunnyEstimator.js";
import { runAzusaEstimatorFromText } from "./azusaEstimator.js";
import { runRoxyEstimatorFromText } from "./roxyEstimator.js";
import { modeTagFromLnRatio } from "../patterns/config.js";
import { numericToRcLabel } from "./rcDifficultyFormat.js";

const MIXED_SUPPORTED_KEYS = new Set([4, 6, 7]);
// 低难段（Roxy scope-out 或 Sunny star < 9）RC 部分的 Azusa⊕Companella 融合权重。
// 两个近似独立的低难估计器取平均以降低方差（与 Roxy 内部 Azusa 融合同一机制族）；
// 离线探针显示 w∈[0.4,0.7] 结果平坦，取对称 0.5，避免拟合基准分布。
const RC_AZUSA_COMPANELLA_FUSION_WEIGHT = 0.5;
// Companella 仅覆盖低难段：Sunny 星数达到该值以上不再参与 RC 融合。
const LOW_BAND_COMPANELLA_STAR_MAX = 9;
// 融合作用域：仅当 Azusa 的 RC 数值自身低于 Alpha 边界（低难主张成立）时融合。
// 一致性门控（|Azusa−Companella| ≤ 1.0）经真实运行验证会误杀大量有益融合
// （净收益 -4.89 → -2.87 MAE 点），已移除：分歧大小无法区分方向对错，
// 净效应由权重平坦性保证。
const RC_FUSION_LOW_BAND_MAX = 11;
const AZUSA_RC_PREFERENCE = Object.freeze({
    balancedHandScreenMaxBias: 0.006,
    balancedHandMaxBias: 0.003,
    azusaHigherScreenMinDelta: 0.25,
    azusaHigherMinDelta: 0.4,
    anchorHeavyScreenMinRate: 0.72,
    anchorHeavyMinRate: 0.78,
    azusaLowerScreenMaxDelta: -0.55,
    azusaLowerMaxDelta: -0.7,
});

function parseCvtFlags(value) {
    const normalized = String(value ?? "").toUpperCase();
    return {
        inEnabled: normalized.includes("IN"),
        hoEnabled: normalized.includes("HO"),
    };
}

function splitDifficultyParts(value) {
    const text = String(value ?? "").trim();
    if (!text) {
        return { rc: "-", ln: "-" };
    }

    const parts = text
        .split("||")
        .map((part) => part.trim())
        .filter((part) => part.length > 0);

    if (parts.length >= 2) {
        return {
            rc: parts[0],
            ln: parts[1],
        };
    }

    return {
        rc: parts[0] || text,
        ln: parts[0] || text,
    };
}

export function composeDifficultyFromRcLn(rcLabel, lnLabel, lnRatio) {
    const rc = String(rcLabel ?? "").trim();
    const ln = String(lnLabel ?? "").trim();
    const ratio = Number(lnRatio);

    if (!Number.isFinite(ratio) || ratio < 0.15) {
        return rc || ln || "-";
    }

    if (!rc) {
        return ln || "-";
    }
    if (!ln) {
        return rc;
    }
    return `${rc} || ${ln}`;
}

export function isDanielTooLowDifficulty(value) {
    const text = String(value ?? "").trim();
    return /^<\s*alpha\b/i.test(text);
}

function tryRunDanielFallback(osuText, options, parsed) {
    try {
        return runDanielEstimatorFromText(osuText, options, parsed);
    } catch {
        return null;
    }
}

function tryRunAzusaFallback(osuText, options, parsed) {
    try {
        return runAzusaEstimatorFromText(osuText, options, parsed);
    } catch {
        return null;
    }
}

function tryRunRoxyFallback(osuText, options, parsed) {
    try {
        return runRoxyEstimatorFromText(osuText, options, parsed);
    } catch {
        return null;
    }
}

function canUseRcResult(result) {
    if (!result || Number(result.columnCount) !== 4) {
        return false;
    }

    const estDiff = String(result.estDiff ?? "").trim();
    if (!estDiff || /^Invalid\b/i.test(estDiff)) {
        return false;
    }

    // Roxy 高难聚焦的 scope 边界（"< Alpha Low" / "> Emik Zeta high"）返回
    // numericDifficulty null，视为不可用，路由到 Azusa（低难）。
    const numeric = result.numericDifficulty;
    if (numeric === null || numeric === undefined || numeric === "") {
        return false;
    }

    return true;
}

function resultNumericValue(result) {
    const raw = result?.numericDifficulty;
    if (raw === null || raw === undefined) return null;
    const value = Number(raw);
    return Number.isFinite(value) ? value : null;
}

// 低难段融合计划：plan 携带 Azusa 的 RC 基准值（fuseRc），
// applyCompanellaToMixedResult 在 Companella 结果到达后做 0.5/0.5 融合。
// onDisagree 指定门控未通过时保留哪一侧（该分支改动前的原赢家），
// 保证融合只在两参考一致且都主张低难时生效，其余行为与改动前一致。
function buildLowBandCompanellaPlan(rcResult, lnRatio, lnDifficulty, onDisagree) {
    return {
        lnRatio,
        lnDifficulty,
        fuseRc: true,
        onDisagree,
        rcEstDiff: rcResult.estDiff,
        rcNumeric: resultNumericValue(rcResult),
        rcNumericHint: rcResult.numericDifficultyHint ?? null,
    };
}

// Roxy 的 debug.finalNumeric 是全部后处理（OD 校正、结构下限、参考间隙、
// Azusa 融合）之后的连续值，比 numericDifficulty（保留 2 位小数）更精确，
// 换路判定基于它可避免舍入导致的 delta 抖动。
function roxyUnquantizedNumeric(result) {
    const raw = result?.debug?.finalNumeric;
    if (raw !== null && raw !== undefined && raw !== "") {
        const value = Number(raw);
        if (Number.isFinite(value)) return value;
    }
    return resultNumericValue(result);
}

function debugStatValue(result, name) {
    const raw = result?.debug?.stats?.[name];
    if (raw === null || raw === undefined) return null;
    const value = Number(raw);
    return Number.isFinite(value) ? value : null;
}

function debugReferenceValue(result, name) {
    const raw = result?.debug?.meta?.references?.[name];
    if (raw === null || raw === undefined) return null;
    const value = Number(raw);
    return Number.isFinite(value) ? value : null;
}

function shouldEvaluateAzusaRcPreference(roxyResult) {
    if (!canUseRcResult(roxyResult)) {
        return false;
    }

    const roxyNumeric = roxyUnquantizedNumeric(roxyResult);
    const azusaReference = debugReferenceValue(roxyResult, "Azusa");
    const handBias = debugStatValue(roxyResult, "handBias");
    const anchorRate = debugStatValue(roxyResult, "anchorRate");
    if (roxyNumeric == null || azusaReference == null) {
        return false;
    }

    const delta = azusaReference - roxyNumeric;
    const balancedHandCandidate = handBias != null
        && handBias <= AZUSA_RC_PREFERENCE.balancedHandScreenMaxBias
        && delta >= AZUSA_RC_PREFERENCE.azusaHigherScreenMinDelta;
    const anchorHeavyCandidate = anchorRate != null
        && anchorRate >= AZUSA_RC_PREFERENCE.anchorHeavyScreenMinRate
        && delta <= AZUSA_RC_PREFERENCE.azusaLowerScreenMaxDelta;

    // 跨界规则：Roxy 输出已到 11+ 而 Azusa 参考低于 11（见 shouldPreferAzusaRcResult）。
    const crossingCandidate = roxyNumeric >= 11 && azusaReference < 11;

    return balancedHandCandidate || anchorHeavyCandidate || crossingCandidate;
}

export function shouldPreferAzusaRcResult(roxyResult, azusaResult) {
    if (!canUseRcResult(roxyResult) || !canUseRcResult(azusaResult)) {
        return false;
    }

    const roxyNumeric = roxyUnquantizedNumeric(roxyResult);
    const azusaNumeric = resultNumericValue(azusaResult);
    const handBias = debugStatValue(roxyResult, "handBias");
    const anchorRate = debugStatValue(roxyResult, "anchorRate");
    if (roxyNumeric == null || azusaNumeric == null) {
        return false;
    }

    const delta = azusaNumeric - roxyNumeric;
    const balancedHandAzusaLift = handBias != null
        && handBias <= AZUSA_RC_PREFERENCE.balancedHandMaxBias
        && delta >= AZUSA_RC_PREFERENCE.azusaHigherMinDelta;
    const anchorHeavyRoxyDamp = anchorRate != null
        && anchorRate >= AZUSA_RC_PREFERENCE.anchorHeavyMinRate
        && delta <= AZUSA_RC_PREFERENCE.azusaLowerMaxDelta;

    // 跨界规则：Roxy 输出已到 11+（Alpha 上界）而 Azusa 仍低于 11——这些图
    // 的 expected 段位接近 Alpha 边界，Azusa 的收敛输出（<11）通常更贴近真实
    // 段位（Roxy 的结构模型对"结构难但段位低"的图系统性高估）。仅当两个条件
    // 同时满足时触发，11~17 段正常图（Azusa 参考也在 11+）不受影响。
    const crossingLift = roxyNumeric >= 11 && azusaNumeric < 11;

    return balancedHandAzusaLift || anchorHeavyRoxyDamp || crossingLift;
}

export function runMixedEstimatorFromText(osuText, options = {}, parsed = null) {
    const sunnyBaseline = options.precomputedSunnyResult || runSunnyEstimatorFromText(osuText, options, parsed);
    // Track which sub-algorithm actually won the routing chain below so callers
    // (analysis pipeline → telemetry) can report the real algorithm, not "Mixed".
    let actualAlgorithm = "Sunny";
    const columnCount = Number(sunnyBaseline.columnCount);
    if (!Number.isFinite(columnCount) || !MIXED_SUPPORTED_KEYS.has(columnCount)) {
        return {
            ...sunnyBaseline,
            mixedCompanellaPlan: null,
            actualEstimatorAlgorithm: actualAlgorithm,
        };
    }

    const { inEnabled, hoEnabled } = parseCvtFlags(options.cvtFlag);
    const hasExplicitOd = options.odFlag !== null && options.odFlag !== undefined;
    const mixedModeTag = hoEnabled ? "RC" : modeTagFromLnRatio(Number(sunnyBaseline.lnRatio));
    const sunnyParts = splitDifficultyParts(sunnyBaseline.estDiff);
    const lnRatio = Number(sunnyBaseline.lnRatio);
    const lnDifficulty = sunnyParts.ln;

    if (mixedModeTag === "RC" && columnCount !== 4) {
        return {
            ...sunnyBaseline,
            mixedCompanellaPlan: null,
            actualEstimatorAlgorithm: actualAlgorithm,
        };
    }

    let selectedRework = sunnyBaseline;
    let estDiff = sunnyBaseline.estDiff;
    let numericDifficulty = sunnyBaseline.numericDifficulty;
    let numericDifficultyHint = sunnyBaseline.numericDifficultyHint;
    let companellaPlan = null;

    if (mixedModeTag === "RC") {
        const roxyResult = tryRunRoxyFallback(osuText, {
            ...options,
            precomputedSunnyResult: sunnyBaseline,
        }, parsed);
        if (canUseRcResult(roxyResult)) {
            selectedRework = roxyResult;
            actualAlgorithm = "Roxy";
            estDiff = roxyResult.estDiff;
            numericDifficulty = roxyResult.numericDifficulty;
            numericDifficultyHint = roxyResult.numericDifficultyHint;
            if (!inEnabled && !hasExplicitOd && shouldEvaluateAzusaRcPreference(roxyResult)) {
                const azusaResult = tryRunAzusaFallback(osuText, {
                    ...options,
                    forceSunnyReferenceHo: false,
                    precomputedSunnyResult: sunnyBaseline,
                }, parsed);
                if (shouldPreferAzusaRcResult(roxyResult, azusaResult)) {
                    selectedRework = azusaResult;
                    actualAlgorithm = "Azusa";
                    estDiff = azusaResult.estDiff;
                    numericDifficulty = azusaResult.numericDifficulty;
                    numericDifficultyHint = azusaResult.numericDifficultyHint;
                }
            }
        } else if (!inEnabled) {
            const azusaResult = tryRunAzusaFallback(osuText, {
                ...options,
                forceSunnyReferenceHo: false,
                precomputedSunnyResult: sunnyBaseline,
            }, parsed);
            if (canUseRcResult(azusaResult)) {
                selectedRework = azusaResult;
                actualAlgorithm = "Azusa";
                estDiff = azusaResult.estDiff;
                numericDifficulty = azusaResult.numericDifficulty;
                numericDifficultyHint = azusaResult.numericDifficultyHint;
                // 低难段（Roxy scope-out 且 Sunny star < 9）：RC 数值升级为
                // Azusa⊕Companella 融合；门控未通过或 Companella 失败时，
                // 本结果（纯 Azusa）兜底——与改动前行为一致。
                if (Number(sunnyBaseline.star) < LOW_BAND_COMPANELLA_STAR_MAX) {
                    companellaPlan = buildLowBandCompanellaPlan(azusaResult, lnRatio, lnDifficulty, "azusa");
                }
            } else {
                const danielResult = tryRunDanielFallback(osuText, options, parsed);
                const canUseDaniel = danielResult
                    && Number(danielResult.columnCount) === 4
                    && !isDanielTooLowDifficulty(danielResult.estDiff);

                if (canUseDaniel) {
                    selectedRework = danielResult;
                    actualAlgorithm = "Daniel";
                    estDiff = danielResult.estDiff;
                    numericDifficulty = danielResult.numericDifficulty;
                    numericDifficultyHint = danielResult.numericDifficultyHint;
                }
            }
        }
    } else {
        let rcDifficulty = sunnyParts.rc;
        let rcNumericDifficulty = sunnyBaseline.numericDifficulty;
        let rcNumericDifficultyHint = sunnyBaseline.numericDifficultyHint;

        if (columnCount === 4) {
            if (Number(sunnyBaseline.star) < LOW_BAND_COMPANELLA_STAR_MAX) {
                // 低难 4K：RC 部分以 Azusa 为基准与 Companella 融合（0.5/0.5）。
                // 旧实现把 RC 段完全交给 Companella，对以 RC 为主的 Mix 图
                // （如低段位 RC course）偏差更大；Azusa 无效时保留纯 Companella 行为。
                const azusaResult = tryRunAzusaFallback(osuText, {
                    ...options,
                    forceSunnyReferenceHo: false,
                    precomputedSunnyResult: sunnyBaseline,
                }, parsed);
                if (canUseRcResult(azusaResult)) {
                    rcDifficulty = azusaResult.estDiff;
                    rcNumericDifficulty = azusaResult.numericDifficulty;
                    rcNumericDifficultyHint = azusaResult.numericDifficultyHint;
                    actualAlgorithm = "Azusa";
                    companellaPlan = buildLowBandCompanellaPlan(azusaResult, lnRatio, lnDifficulty, "companella");
                } else {
                    companellaPlan = {
                        lnRatio,
                        lnDifficulty,
                    };
                    actualAlgorithm = "Companella";
                }
            } else {
                const danielResult = tryRunDanielFallback(osuText, options, parsed);
                const canUseDaniel = danielResult
                    && Number(danielResult.columnCount) === 4
                    && !isDanielTooLowDifficulty(danielResult.estDiff);

                if (canUseDaniel) {
                    rcDifficulty = danielResult.estDiff;
                    rcNumericDifficulty = danielResult.numericDifficulty;
                    rcNumericDifficultyHint = danielResult.numericDifficultyHint;
                    actualAlgorithm = "Daniel";
                }
            }
        }

        estDiff = composeDifficultyFromRcLn(rcDifficulty, lnDifficulty, lnRatio);
        numericDifficulty = rcNumericDifficulty;
        numericDifficultyHint = rcNumericDifficultyHint;
    }

    const normalizedLnRatio = Number(selectedRework.lnRatio);
    const forcedLnRatio = hoEnabled ? 0 : normalizedLnRatio;

    return {
        ...selectedRework,
        lnRatio: Number.isFinite(forcedLnRatio) ? forcedLnRatio : 0,
        estDiff,
        numericDifficulty,
        numericDifficultyHint,
        mixedCompanellaPlan: companellaPlan,
        actualEstimatorAlgorithm: actualAlgorithm,
    };
}

export function applyCompanellaToMixedResult(mixedResult, companellaResult) {
    const plan = mixedResult?.mixedCompanellaPlan;
    if (!plan) {
        return mixedResult;
    }

    // 低难融合路径：Companella 与计划携带的 Azusa RC 基准做固定权重平均，
    // estDiff 由融合数值重新派生（numeric 与 estDiff 保持同源）。
    // 门控：仅当 Azusa 数值低于 Alpha（低难主张成立）时融合；未通过时回落
    // onDisagree 指定的原赢家（RC 分支为 Azusa，Mix 分支为 Companella），
    // 行为与改动前一致。
    if (plan.fuseRc) {
        const rcNumeric = Number(plan.rcNumeric);
        const companellaNumeric = Number(companellaResult?.numericDifficulty);
        if (!Number.isFinite(rcNumeric) || !Number.isFinite(companellaNumeric)) {
            return mixedResult;
        }
        if (rcNumeric >= RC_FUSION_LOW_BAND_MAX) {
            if (plan.onDisagree === "companella") {
                return {
                    ...mixedResult,
                    estDiff: composeDifficultyFromRcLn(
                        companellaResult.estDiff,
                        plan.lnDifficulty,
                        plan.lnRatio,
                    ),
                    numericDifficulty: companellaResult.numericDifficulty,
                    numericDifficultyHint: companellaResult.numericDifficultyHint,
                    mixedCompanellaPlan: null,
                };
            }
            return mixedResult;
        }
        const weight = RC_AZUSA_COMPANELLA_FUSION_WEIGHT;
        const fused = Number((rcNumeric * weight + companellaNumeric * (1 - weight)).toFixed(2));
        return {
            ...mixedResult,
            estDiff: composeDifficultyFromRcLn(
                numericToRcLabel(fused),
                plan.lnDifficulty,
                plan.lnRatio,
            ),
            numericDifficulty: fused,
            numericDifficultyHint: null,
            mixedCompanellaPlan: null,
        };
    }

    return {
        ...mixedResult,
        estDiff: composeDifficultyFromRcLn(
            companellaResult.estDiff,
            plan.lnDifficulty,
            plan.lnRatio,
        ),
        numericDifficulty: companellaResult.numericDifficulty,
        numericDifficultyHint: companellaResult.numericDifficultyHint,
        mixedCompanellaPlan: null,
    };
}
