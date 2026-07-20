import { calculate as calculateSunny } from "../rework/sunnyAlgorithm.js";
import { calculateLN } from "../rework/sunnyWindowAlgorithm.js";
import { estDiff, estDiff2, normalizeReworkResult, estimateDanielDan } from "./reworkEstimatorUtils.js";

export function runSunnyEstimatorFromText(osuText, options = {}) {
    const speedRate = options.speedRate ?? 1.0;
    const odFlag = options.odFlag ?? null;
    const cvtFlag = options.cvtFlag ?? null;
    const withGraph = options.withGraph === true;
    const enableLNRework = options.enableLNRework === true;

    const rawResult = calculateSunny(osuText, speedRate, odFlag, cvtFlag, { withGraph });
    const parsed = normalizeReworkResult(rawResult);

    // LN Rework: 计算 LN 专属难度
    let numericDifficulty = null;
    let numericDifficultyHint = null;
    if (enableLNRework) {
        const lnResult = calculateLN(osuText, speedRate, odFlag, cvtFlag);
        let lnStar = 0;
        if (lnResult !== -3 && Array.isArray(lnResult)) {
            lnStar = lnResult[0];
        }
        numericDifficulty = estimateDanielDan(parsed.star).numeric;
        numericDifficultyHint = "sunny-rc-dan";
        return {
            ...parsed,
            estDiff: estDiff2(parsed.star, lnStar, parsed.columnCount),
            numericDifficulty,
            numericDifficultyHint,
        };
    }

    return {
        ...parsed,
        estDiff: estDiff(parsed.star, parsed.lnRatio, parsed.columnCount),
        numericDifficulty: null,
        numericDifficultyHint: null,
    };
}
