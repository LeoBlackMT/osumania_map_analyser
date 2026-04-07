import { runDanielEstimatorFromText } from "./danielEstimator.js";
import { runSunnyEstimatorFromText } from "./sunnyEstimator.js";

const MIXED_SUPPORTED_KEYS = new Set([4, 6, 7]);

function modeTagFromLnRatio(lnRatio) {
    if (!Number.isFinite(lnRatio)) {
        return "Mix";
    }
    if (lnRatio <= 0.15) {
        return "RC";
    }
    if (lnRatio >= 0.9) {
        return "LN";
    }
    return "Mix";
}

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

function tryRunDanielFallback(osuText, options) {
    try {
        return runDanielEstimatorFromText(osuText, options);
    } catch {
        return null;
    }
}

export function runMixedEstimatorFromText(osuText, options = {}) {
    const sunnyBaseline = runSunnyEstimatorFromText(osuText, options);
    const columnCount = Number(sunnyBaseline.columnCount);
    if (!Number.isFinite(columnCount) || !MIXED_SUPPORTED_KEYS.has(columnCount)) {
        return {
            ...sunnyBaseline,
            mixedCompanellaPlan: null,
        };
    }

    const { inEnabled, hoEnabled } = parseCvtFlags(options.cvtFlag);
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
        if (!inEnabled) {
            const danielResult = tryRunDanielFallback(osuText, options);
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
                const danielResult = tryRunDanielFallback(osuText, options);
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
