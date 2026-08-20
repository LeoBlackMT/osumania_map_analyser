/**
 * Azusa variant algorithms for debug comparison.
 * Each variant runs the real Azusa pipeline then re-computes
 * the final output using alternative calibration/gate logic.
 *
 * These are NOT production algorithms — they exist solely
 * for side-by-side comparison in debug.html.
 */
import { runAzusaEstimatorFromText } from "../estimator/azusaEstimator.js";
import { numericToRcLabel } from "../estimator/rcDifficultyFormat.js";

function clamp(v, lo, hi) {
    return Math.max(lo, Math.min(hi, v));
}

function piecewiseLinear(x, knots, valueCol = 1) {
    if (x <= knots[0][0]) return knots[0][valueCol];
    if (x >= knots[knots.length - 1][0]) return knots[knots.length - 1][valueCol];
    for (let i = 0; i < knots.length - 1; i++) {
        if (x >= knots[i][0] && x <= knots[i + 1][0]) {
            const t = (x - knots[i][0]) / (knots[i + 1][0] - knots[i][0]);
            return knots[i][valueCol] + t * (knots[i + 1][valueCol] - knots[i][valueCol]);
        }
    }
    return knots[knots.length - 1][valueCol];
}

// ── Retrained isotonic table (osu+Malody combined, merged approach) ──
// New low-range knots from PAVA on combined data, old knots preserved for >=10
const AZUSA_ISOTONIC_RETRAINED = [
    [0.7502, 1.8350], [0.9044, 2.1960], [1.3432, 2.3305], [1.4453, 2.4133],
    [1.4766, 2.5078], [1.5487, 2.6467], [1.7817, 2.8803], [1.9434, 3.5733],
    [1.9719, 3.6639], [2.2005, 4.0000], [2.2020, 4.0313], [2.4196, 4.0823],
    [2.5660, 4.2375], [2.6995, 4.2600], [2.7439, 4.3455], [2.8667, 5.0753],
    [3.9393, 5.5492], [4.0073, 5.8408], [4.5136, 6.2707], [4.7164, 6.3880],
    [4.8315, 6.5854], [4.9761, 6.6360], [4.9814, 6.8173], [5.4007, 7.0000],
    [5.5105, 7.0144], [5.5544, 7.0583], [5.5857, 7.1450], [5.6575, 7.4174],
    [5.7832, 7.5167], [5.7840, 7.5882], [5.8309, 7.6976], [5.9958, 7.8509],
    [6.1477, 8.3036], [7.0374, 8.5010], [7.1875, 8.5770], [7.4539, 9.0977],
    [9.0318, 9.8810], [9.8115, 10.0000],
    // --- from here, old high-range knots ---
    [9.8344, 10.4000], [10.0013, 10.4000], [10.0778, 10.5000], [10.1054, 10.5000],
    [10.1435, 10.6000], [10.4782, 10.6462], [10.8866, 10.8000], [11.0934, 11.1727],
    [11.3266, 11.2867], [11.4970, 11.4000], [11.6024, 11.4750], [11.6947, 11.6000],
    [11.8932, 12.0636], [12.0076, 12.3000], [12.2947, 12.4150], [12.7583, 12.4500],
    [12.8756, 12.9000], [12.9268, 12.9000], [13.0042, 13.2000], [13.2387, 13.2694],
    [13.4620, 13.4400], [13.5467, 13.5000], [13.6016, 13.7375], [13.9609, 13.9500],
    [14.1414, 14.0250], [14.2226, 14.0762], [14.3178, 14.1273], [14.3786, 14.1643],
    [14.4421, 14.2182], [14.4825, 14.3000], [14.5063, 14.3750], [14.5452, 14.4778],
    [14.6359, 14.5850], [14.7301, 14.6389], [14.8846, 14.7906], [15.0424, 14.9263],
    [15.2159, 15.0944], [15.3942, 15.1875], [15.5380, 15.3300], [15.8096, 15.5320],
    [16.0262, 16.1000], [16.0702, 16.1000], [16.2738, 16.1267], [16.4723, 16.3579],
    [16.7156, 16.8000], [17.1446, 17.0600], [17.5478, 17.2000], [17.6403, 17.2000],
    [17.7603, 17.2000], [17.8264, 17.6000], [18.1258, 17.9750], [18.5000, 18.2000],
    [19.2000, 18.7000], [20.0000, 19.2000], [21.2000, 19.8000], [22.5000, 20.0000],
];

function makeVariantResult(baseResult, numeric, hint) {
    if (numeric == null || !Number.isFinite(numeric)) {
        return { ...baseResult, estDiff: "Invalid: variant produced non-finite value", numericDifficultyHint: hint };
    }
    return {
        ...baseResult,
        estDiff: numericToRcLabel(numeric),
        numericDifficulty: Number(numeric.toFixed(2)),
        numericDifficultyHint: hint,
        star: Number((3.4 + 0.38 * numeric).toFixed(4)),
        graph: null,
    };
}

/**
 * Simulate the blend formula with a custom gate source function.
 * Re-uses the intermediate values from the real Azusa debug output.
 */
function simulateBlendWithGate(primary, daniel, sunny, gateFn, hints) {
    const p = primary;
    const d = daniel;
    const s = sunny;

    const gateSource = gateFn(p, d, s);
    if (gateSource == null) return null;

    const lowGate = clamp((9.61 - gateSource) / 4.94, 0, 1);
    const highGate = 1 - lowGate;

    if (s == null) return null;

    let lowBase = -8.317 + 1.536 * s;
    if (p != null) lowBase += 0.011 * p;
    if (d != null) lowBase += 0.049 * d;

    if (lowGate > 0) {
        const sunnyPart = 0.442 * Math.max(0, s - 9.84);
        const primaryPart = 0.016 * Math.max(0, (p ?? 0) - 10.4);
        const sunnyConvex = 0.235 * Math.pow(Math.max(0, 7.935 - s), 2);
        lowBase += lowGate * (sunnyPart + primaryPart + sunnyConvex);
    }

    const dUse = d ?? s ?? p;
    if (dUse == null) return lowBase * lowGate;
    const primaryUse = p ?? dUse;
    const sunnyUse = s ?? dUse;

    let highBase = 0.809 * dUse + 0.057 * primaryUse + 0.165 * sunnyUse + 0.183;
    const highMask = clamp((gateSource - 14.83) / 2.667, 0, 1);
    if (highMask > 0) {
        highBase += highMask * (-0.154 * Math.max(0, primaryUse - dUse) + 0.081 * Math.max(0, sunnyUse - dUse));
    }

    const lowLift = Math.max(0, 9.889 - gateSource) * 0.257;
    const value = lowBase * lowGate + (highBase + lowLift) * highGate;

    // Now apply the same post-blend pipeline: block calibrate → residual → isotonic → refCorrect
    // But we need the original lowGate/highGate from the real Azusa for block calibrate blend
    // We'll use the simulated gate instead for consistency
    return { value: clamp(value, -2, 20), lowGate, highGate, lowBase, highBase };
}

// ── Block calibration (same as production) ──
const AZUSA_CALIBRATION_LOW_BLOCKS = [
    [1.9220, 1.9220, 1.0000], [2.3660, 2.7684, 1.6667], [2.8394, 2.8394, 2.0000],
    [2.8584, 3.7162, 2.3333], [3.7798, 3.7798, 3.0000], [3.8667, 3.8667, 3.0000],
    [4.2067, 5.2039, 4.3333], [5.2506, 5.7713, 5.0667], [5.8603, 6.1512, 5.3333],
    [6.3292, 6.8785, 6.0000], [7.1715, 7.3617, 6.2000], [7.4079, 7.8734, 7.2000],
    [8.0160, 8.4003, 8.2500], [8.4133, 8.4133, 9.0000], [8.9031, 9.4775, 9.5667],
    [9.6488, 9.6488, 10.0000], [9.8301, 9.8301, 10.3000],
];
const AZUSA_CALIBRATION_HIGH_BLOCKS = [
    [11.4336, 11.4336, 10.4000], [11.4436, 11.4436, 10.5000],
    [11.6012, 11.6665, 10.6500], [11.6696, 12.2317, 11.5000],
    [12.3295, 12.3919, 11.7500], [12.5238, 12.5238, 12.0000],
    [12.5318, 12.8329, 12.1400], [12.8605, 12.9781, 12.2800],
    [12.9868, 13.1170, 12.7800], [13.2003, 13.4418, 12.7857],
    [13.4660, 13.5829, 12.9250], [13.6044, 13.9924, 13.3667],
    [14.0583, 14.0583, 13.4000], [14.0795, 14.2266, 13.4600],
    [14.2346, 14.2346, 13.6000], [14.2414, 14.2414, 13.7000],
    [14.2903, 14.2903, 14.0000], [14.3258, 14.4760, 14.1200],
    [14.5365, 14.6006, 14.1333], [14.7269, 14.8716, 14.1333],
    [15.0048, 15.0048, 14.4000], [15.0521, 15.0521, 14.4000],
    [15.0521, 15.0521, 14.4000], [15.0950, 15.0950, 14.4000],
    [15.2335, 15.2335, 14.4000], [15.2388, 15.5821, 14.7385],
    [15.6977, 15.7002, 14.8500], [15.7535, 16.1593, 15.0667],
    [16.2009, 16.2958, 15.1000], [16.3172, 16.4748, 15.7600],
    [16.5620, 16.9083, 15.9833], [16.9485, 16.9485, 16.0000],
    [17.0216, 17.3799, 16.1000], [17.4616, 17.4616, 16.4000],
    [17.5167, 17.5167, 16.4000], [17.5306, 17.9077, 16.6400],
    [18.1973, 18.1973, 17.2000], [18.2026, 18.2026, 17.2000],
    [18.4562, 19.3477, 17.9500], [19.3477, 20.5000, 18.2000],
    [20.5000, 22.0000, 18.6000], [22.0000, 24.0000, 19.2000],
    [24.0000, 27.0000, 20.0000],
];

function piecewiseBlock(x, blocks) {
    for (const [xMin, xMax, y] of blocks) {
        if (x >= xMin && x <= xMax) return y;
    }
    if (x < blocks[0][0]) return blocks[0][2];
    return blocks[blocks.length - 1][2];
}

function calibrateBlock(value, lowGate, highGate) {
    const low = piecewiseBlock(value, AZUSA_CALIBRATION_LOW_BLOCKS);
    const high = piecewiseBlock(value, AZUSA_CALIBRATION_HIGH_BLOCKS);
    const lg = lowGate ?? 0;
    const hg = highGate ?? 0;
    const ws = lg + hg;
    if (ws <= 1e-6) return value < 11 ? low : high;
    return (lg * low + hg * high) / ws;
}

function computeResidual(calibrated, highGate, primary, sunny, daniel) {
    const x = calibrated;
    if (!Number.isFinite(x)) return 0;
    const hg = highGate ?? 0;
    const ds = (daniel ?? x) - (sunny ?? x);
    const sp = (sunny ?? x) - (primary ?? x);
    return clamp(
        4.335282 + (-0.170459 * x) + (-1.622303 * Math.max(0, 11 - x))
        + (1.328125 * Math.max(0, 12.5 - x)) + (-0.042829 * Math.max(0, 14 - x))
        + (-0.834997 * hg) + (3.060352 * hg * Math.max(0, 11 - x))
        + (-1.744638 * hg * Math.max(0, 12.5 - x)) + (0.409922 * ds)
        + (0.041072 * sp) + (-0.388231 * hg * ds) + (-0.170185 * hg * sp),
        -1.2, 1.2,
    );
}

function computeRefCorrection(output, daniel, sunny) {
    const x = output;
    if (!Number.isFinite(x) || x < 10.0 || x > 17.5) return 0;
    let gate, coeffD, coeffS;
    if (x < 11.5) { gate = clamp((x - 10.0) / 1.5, 0, 1); coeffD = 0.10; coeffS = 0.06; }
    else if (x < 12.5) { gate = 1.0; coeffD = 0.20; coeffS = 0.13; }
    else if (x < 16.0) { gate = 1.0; coeffD = 0.40; coeffS = 0.25; }
    else { gate = clamp((17.5 - x) / 1.5, 0, 1); coeffD = 0.28; coeffS = 0.17; }
    let correction = 0;
    if (daniel != null) correction += coeffD * (daniel - x);
    if (sunny != null) correction += coeffS * (sunny - x);
    return clamp(correction * gate, -1.2, 1.2);
}

function runFullPipeline(blendValue, lowGate, highGate, primary, sunny, daniel, isotonicTable) {
    const calibrated = calibrateBlock(blendValue, lowGate, highGate);
    const residual = computeResidual(calibrated, highGate, primary, sunny, daniel);
    const preOutput = clamp(calibrated + residual, -2, 20);
    const output = piecewiseLinear(preOutput, isotonicTable, 1);
    const refCorr = computeRefCorrection(output, daniel, sunny);
    const final = clamp(output + refCorr, -2, 20);
    return { calibrated, residual, preOutput, output, refCorr, final };
}

// ── Exported variant runners ──

/**
 * Azusa with primary gate source.
 * Changes lowGateSource from daniel to primaryNumeric.
 */
export async function runAzusaPrimaryGate(osuText, options) {
    const result = await runAzusaEstimatorFromText(osuText, {
        ...options,
        forceSunnyReferenceHo: false,
    });
    if (!result || result.errors?.length > 0 || result.debug?.primaryNumeric == null) {
        return result;
    }

    const d = result.debug;
    const primary = Number(d.primaryNumeric);
    const daniel = Number(d.danielNumeric);
    const sunny = Number(d.sunnyNumeric);

    // Re-simulate blend with primary as gate source
    const gateSource = primary;
    const lowGate = clamp((9.61 - gateSource) / 4.94, 0, 1);
    const highGate = 1 - lowGate;

    let lowBase = -8.317 + 1.536 * sunny;
    if (Number.isFinite(primary)) lowBase += 0.011 * primary;
    if (Number.isFinite(daniel)) lowBase += 0.049 * daniel;
    if (lowGate > 0) {
        const sunnyPart = 0.442 * Math.max(0, sunny - 9.84);
        const primaryPart = 0.016 * Math.max(0, primary - 10.4);
        const sunnyConvex = 0.235 * Math.pow(Math.max(0, 7.935 - sunny), 2);
        lowBase += lowGate * (sunnyPart + primaryPart + sunnyConvex);
    }

    const dUse = Number.isFinite(daniel) ? daniel : (Number.isFinite(sunny) ? sunny : primary);
    const primaryUse = Number.isFinite(primary) ? primary : dUse;
    const sunnyUse = Number.isFinite(sunny) ? sunny : dUse;
    let highBase = 0.809 * dUse + 0.057 * primaryUse + 0.165 * sunnyUse + 0.183;
    const highMask = clamp((gateSource - 14.83) / 2.667, 0, 1);
    if (highMask > 0) {
        highBase += highMask * (-0.154 * Math.max(0, primaryUse - dUse) + 0.081 * Math.max(0, sunnyUse - dUse));
    }
    const lowLift = Math.max(0, 9.889 - gateSource) * 0.257;
    const blendValue = lowBase * lowGate + (highBase + lowLift) * highGate;

    // Use the original calibration gate for block calibrate (from real Azusa)
    const origLowGate = Number(d.blend?.lowGate) ?? 0;
    const origHighGate = Number(d.blend?.highGate) ?? 0;

    // Re-run full pipeline with the new blend value and original isotonic
    const pipeline = runFullPipeline(
        blendValue, origLowGate, origHighGate,
        Number.isFinite(primary) ? primary : null,
        Number.isFinite(sunny) ? sunny : null,
        Number.isFinite(daniel) ? daniel : null,
        [[1.3868,1.0000],[1.4574,1.0000],[1.5361,1.0000],[1.6320,1.5000],
         [1.9833,2.5800],[2.2465,2.6000],[2.3344,2.8000],[2.5779,3.4500],
         [3.8277,3.6000],[4.2824,4.3429],[4.5665,4.6250],[4.8016,4.6750],
         [4.9529,5.1500],[5.1029,5.4000],[5.2475,5.4750],[5.5039,5.9000],
         [5.6951,6.0143],[5.9213,6.4000],[6.0093,6.9000],[6.1337,7.2000],
         [6.7092,7.4400],[7.2846,7.5000],[7.4233,7.8000],[7.9790,8.6000],
         [8.2927,8.6143],[9.0829,9.5000],[9.4639,9.6154],[9.8115,10.0000],
         [9.8344,10.4000],[10.0013,10.4000],[10.0778,10.5000],[10.1054,10.5000],
         [10.1435,10.6000],[10.4782,10.6462],[10.8866,10.8000],[11.0934,11.1727],
         [11.3266,11.2867],[11.4970,11.4000],[11.6024,11.4750],[11.6947,11.6000],
         [11.8932,12.0636],[12.0076,12.3000],[12.2947,12.4150],[12.7583,12.4500],
         [12.8756,12.9000],[12.9268,12.9000],[13.0042,13.2000],[13.2387,13.2694],
         [13.4620,13.4400],[13.5467,13.5000],[13.6016,13.7375],[13.9609,13.9500],
         [14.1414,14.0250],[14.2226,14.0762],[14.3178,14.1273],[14.3786,14.1643],
         [14.4421,14.2182],[14.4825,14.3000],[14.5063,14.3750],[14.5452,14.4778],
         [14.6359,14.5850],[14.7301,14.6389],[14.8846,14.7906],[15.0424,14.9263],
         [15.2159,15.0944],[15.3942,15.1875],[15.5380,15.3300],[15.8096,15.5320],
         [16.0262,16.1000],[16.0702,16.1000],[16.2738,16.1267],[16.4723,16.3579],
         [16.7156,16.8000],[17.1446,17.0600],[17.5478,17.2000],[17.6403,17.2000],
         [17.7603,17.2000],[17.8264,17.6000],[18.1258,17.9750],[18.5000,18.2000],
         [19.2000,18.7000],[20.0000,19.2000],[21.2000,19.8000],[22.5000,20.0000]],
    );

    const origBlend = Number(d.blendNumeric);
    const origFinal = Number(d.finalNumeric);
    const hint = `primaryGate: blend=${blendValue.toFixed(2)} (orig=${origBlend?.toFixed(2)}) → final=${pipeline.final.toFixed(2)} (orig=${origFinal?.toFixed(2)})`;
    return makeVariantResult(result, pipeline.final, hint);
}

/**
 * Azusa with retrained isotonic table.
 * Uses the osu+Malody combined isotonic for low range, original for high range.
 */
export async function runAzusaRetrainedIsotonic(osuText, options) {
    const result = await runAzusaEstimatorFromText(osuText, {
        ...options,
        forceSunnyReferenceHo: false,
    });
    if (!result || result.errors?.length > 0 || result.debug?.primaryNumeric == null) {
        return result;
    }

    const d = result.debug;
    const primary = Number(d.primaryNumeric);
    const daniel = Number(d.danielNumeric);
    const sunny = Number(d.sunnyNumeric);
    const lowGate = Number(d.blend?.lowGate) ?? 0;
    const highGate = Number(d.blend?.highGate) ?? 0;
    const calibrated = Number(d.calibratedNumeric);
    const residual = Number(d.curveGapResidual);
    const origFinal = Number(d.finalNumeric);

    if (!Number.isFinite(calibrated) || !Number.isFinite(residual)) {
        return result;
    }

    const preOutput = clamp(calibrated + residual, -2, 20);
    const newOutput = piecewiseLinear(preOutput, AZUSA_ISOTONIC_RETRAINED, 1);

    // Apply the same reference correction
    const refCorr = computeRefCorrection(newOutput, daniel, sunny);
    const newFinal = clamp(newOutput + refCorr, -2, 20);

    const hint = `retrainedIso: preOut=${preOutput.toFixed(2)} → new=${newFinal.toFixed(2)} (orig=${origFinal?.toFixed(2)})`;
    return makeVariantResult(result, newFinal, hint);
}
