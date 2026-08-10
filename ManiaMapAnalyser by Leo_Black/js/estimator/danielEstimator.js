import { calculateDaniel } from "../rework/danielAlgorithm.js";
import { runSunnyEstimatorFromText } from "./sunnyEstimator.js";
import {
    estDiff,
    estimateDanielDan,
    normalizeReworkResult,
} from "./reworkEstimatorUtils.js";

export function runDanielEstimatorFromText(osuText, options = {}, parsed = null) {
    const speedRate = options.speedRate ?? 1.0;
    const odFlag = options.odFlag ?? null;
    const cvtFlag = options.cvtFlag ?? null;
    const withGraph = options.withGraph === true;

    const danielResult = calculateDaniel(osuText, speedRate, odFlag, { withGraph }, parsed);

    // Keep previous behavior: Daniel only supports 4K and falls back to Sunny.
    if (danielResult === -3) {
        return runSunnyEstimatorFromText(osuText, {
            speedRate,
            odFlag,
            cvtFlag,
            withGraph,
            extendedEstimationRange: options.extendedEstimationRange,
            enableAlwaysShowLNDifficulty: options.enableAlwaysShowLNDifficulty,
        }, parsed);
    }

    const parsedResult = normalizeReworkResult(danielResult);
    const useDanielDifficulty = parsedResult.columnCount === 4;
    const danielDifficulty = useDanielDifficulty ? estimateDanielDan(parsedResult.star) : null;
    const numericDifficulty = useDanielDifficulty ? danielDifficulty.numeric : null;

    return {
        ...parsedResult,
        estDiff: useDanielDifficulty
            ? danielDifficulty.label
            : estDiff(parsedResult.star, parsedResult.lnRatio, parsedResult.columnCount, options.extendedEstimationRange === true, options.enableAlwaysShowLNDifficulty === true),
        numericDifficulty,
        numericDifficultyHint: useDanielDifficulty && !Number.isFinite(numericDifficulty)
            ? "N/A"
            : null,
    };
}
