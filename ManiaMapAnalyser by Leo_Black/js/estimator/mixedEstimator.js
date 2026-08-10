import { runDanielEstimatorFromText } from "./danielEstimator.js";
import { runSunnyEstimatorFromText } from "./sunnyEstimator.js";
import { runAzusaEstimatorFromText } from "./azusaEstimator.js";
import { runRoxyEstimatorFromText } from "./roxyEstimator.js";
import { modeTagFromLnRatio } from "../patterns/config.js";

const MIXED_SUPPORTED_KEYS = new Set([4, 6, 7]);
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

    return true;
}

function resultNumericValue(result) {
    const raw = result?.numericDifficulty;
    if (raw === null || raw === undefined) return null;
    const value = Number(raw);
    return Number.isFinite(value) ? value : null;
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

    const roxyNumeric = resultNumericValue(roxyResult);
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

    return balancedHandCandidate || anchorHeavyCandidate;
}

export function shouldPreferAzusaRcResult(roxyResult, azusaResult) {
    if (!canUseRcResult(roxyResult) || !canUseRcResult(azusaResult)) {
        return false;
    }

    const roxyNumeric = resultNumericValue(roxyResult);
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

    return balancedHandAzusaLift || anchorHeavyRoxyDamp;
}

export function runMixedEstimatorFromText(osuText, options = {}, parsed = null) {
    const sunnyBaseline = options.precomputedSunnyResult || runSunnyEstimatorFromText(osuText, options, parsed);
    const columnCount = Number(sunnyBaseline.columnCount);
    if (!Number.isFinite(columnCount) || !MIXED_SUPPORTED_KEYS.has(columnCount)) {
        return {
            ...sunnyBaseline,
            mixedCompanellaPlan: null,
        };
    }

    const { inEnabled, hoEnabled } = parseCvtFlags(options.cvtFlag);
    const hasExplicitOd = options.odFlag !== null && options.odFlag !== undefined;
    const mixedModeTag = hoEnabled ? "RC" : modeTagFromLnRatio(Number(sunnyBaseline.lnRatio));

    if (mixedModeTag === "RC" && columnCount !== 4) {
        return {
            ...sunnyBaseline,
            mixedCompanellaPlan: null,
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
                estDiff = azusaResult.estDiff;
                numericDifficulty = azusaResult.numericDifficulty;
                numericDifficultyHint = azusaResult.numericDifficultyHint;
            } else {
                const danielResult = tryRunDanielFallback(osuText, options, parsed);
                const canUseDaniel = danielResult
                    && Number(danielResult.columnCount) === 4
                    && !isDanielTooLowDifficulty(danielResult.estDiff);

                if (canUseDaniel) {
                    selectedRework = danielResult;
                    estDiff = danielResult.estDiff;
                    numericDifficulty = danielResult.numericDifficulty;
                    numericDifficultyHint = danielResult.numericDifficultyHint;
                }
            }
        }
    } else {
        const sunnyParts = splitDifficultyParts(sunnyBaseline.estDiff);
        const lnRatio = Number(sunnyBaseline.lnRatio);
        const lnDifficulty = sunnyParts.ln;

        let rcDifficulty = sunnyParts.rc;
        let rcNumericDifficulty = sunnyBaseline.numericDifficulty;
        let rcNumericDifficultyHint = sunnyBaseline.numericDifficultyHint;

        if (columnCount === 4) {
            if (Number(sunnyBaseline.star) < 9) {
                companellaPlan = {
                    lnRatio,
                    lnDifficulty,
                };
            } else {
                const danielResult = tryRunDanielFallback(osuText, options, parsed);
                const canUseDaniel = danielResult
                    && Number(danielResult.columnCount) === 4
                    && !isDanielTooLowDifficulty(danielResult.estDiff);

                if (canUseDaniel) {
                    rcDifficulty = danielResult.estDiff;
                    rcNumericDifficulty = danielResult.numericDifficulty;
                    rcNumericDifficultyHint = danielResult.numericDifficultyHint;
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
    };
}

export function applyCompanellaToMixedResult(mixedResult, companellaResult) {
    const plan = mixedResult?.mixedCompanellaPlan;
    if (!plan) {
        return mixedResult;
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
