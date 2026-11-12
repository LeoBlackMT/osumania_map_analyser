import { calculate as calculateSunny } from "../rework/sunnyAlgorithm.js";
import { calculateLN } from "../rework/sunnyWindowAlgorithm.js"
import { estDiff2, normalizeReworkResult } from "./reworkEstimatorUtils.js";

function normalizeSunnyWindowResult(result) {
    if (result?.NoLN) return {star: 0, typePercentageData: result.typePercentageData};
    const ret = normalizeReworkResult(result);
    ret.typePercentageData = result.typePercentageData;
    ret.lnPartsRatio = result.lnPartsRatio;
    return ret;
}

export function runSunnyWindowEstimatorFromText(osuText, options = {}, parsed = null) {
    const speedRate = options.speedRate ?? 1.0;
    const odFlag = options.odFlag ?? null;
    const cvtFlag = options.cvtFlag ?? null;
    const withGraph = options.withGraph === true;

    // Perf: the pipeline may already have a Sunny result for the same
    // text/options/parser (normalization / Mixed baseline). Reuse it and only
    // run calculateLN here. Callers must not pass a classicMod=true result:
    // calculateSunny below does not receive classicMod, so that path is not
    // equivalent and must fall back to a fresh calculateSunny.
    const precomputedSunnyResult = options.classicMod === true
        ? null
        : (options.precomputedSunnyResult
            && typeof options.precomputedSunnyResult === "object"
            ? options.precomputedSunnyResult
            : null);
    const canReuseSunny = precomputedSunnyResult != null
        && (!withGraph || precomputedSunnyResult.graph != null);
    const parsedResult = canReuseSunny
        ? normalizeReworkResult({
            star: precomputedSunnyResult.star,
            lnRatio: precomputedSunnyResult.lnRatio,
            columnCount: precomputedSunnyResult.columnCount,
            graph: withGraph ? precomputedSunnyResult.graph : null,
        })
        : normalizeReworkResult(calculateSunny(osuText, speedRate, odFlag, cvtFlag, { withGraph }, parsed));

    const rawResultLN = calculateLN(osuText, speedRate, odFlag, cvtFlag, { withGraph, enableAnalyzeLN: options.enableAnalyzeLN === true }, parsed);
    const parsedLN = normalizeSunnyWindowResult(rawResultLN);

    const shouldShowLN = options.enableAlwaysShowLNDifficulty === true || parsedResult.lnRatio > 0.15 || (parsedLN.star > 1.5 && parsedLN.star > parsedResult.star * 0.7) || parsedResult.lnPartsRatio > 0.3
    const LNStar = shouldShowLN && parsedLN.star ? parsedLN.star : 0;

    return {
        ...parsedResult,
        estDiff: estDiff2(parsedResult.star, LNStar, parsedResult.columnCount, options.extendedEstimationRange === true),
        numericDifficulty: null,
        numericDifficultyHint: null,
        typePercentageData: parsedLN.typePercentageData,
        lnStar: LNStar,
    };
}
