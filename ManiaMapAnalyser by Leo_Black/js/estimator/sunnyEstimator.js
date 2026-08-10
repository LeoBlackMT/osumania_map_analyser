import { calculate as calculateSunny } from "../rework/sunnyAlgorithm.js";
import { estDiff, normalizeReworkResult } from "./reworkEstimatorUtils.js";

export function runSunnyEstimatorFromText(osuText, options = {}, parsed = null) {
    const speedRate = options.speedRate ?? 1.0;
    const odFlag = options.odFlag ?? null;
    const cvtFlag = options.cvtFlag ?? null;
    const withGraph = options.withGraph === true;

    const rawResult = calculateSunny(osuText, speedRate, odFlag, cvtFlag, { withGraph }, parsed);
    const parsedResult = normalizeReworkResult(rawResult);

    return {
        ...parsedResult,
        estDiff: estDiff(parsedResult.star, parsedResult.lnRatio, parsedResult.columnCount, options.extendedEstimationRange === true, options.enableAlwaysShowLNDifficulty === true),
        numericDifficulty: null,
        numericDifficultyHint: null,
    };
}
