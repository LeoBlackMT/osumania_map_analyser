// Surface timing judgement curves, units, and PP composition.
//
// Ported from rosu-pp mania-surface-map-timing-difficulty @ 328e339
// (`src/mania/sunny_accuracy.rs` for curves/units, `src/mania/sunny.rs` for
// the two-factor + xxy PP combination). Pure, DOM-free module: runs unchanged
// in the tosu overlay (browser) and in Node (benchmark smoke). Imports the
// window bands from ./surfaceTimingWindows.js (S2) via explicit `.js`
// extension, which the esm-loader and both runtime environments require.

import {
    getBand,
    JUDGEMENT_NAMES,
} from "./surfaceTimingWindows.js";

/**
 * Frozen model constants for the surface timing pipeline.
 *
 * `ERROR_MODEL` mirrors `ErrorModel::default()` in sunny_accuracy.rs; the
 * sigma_ref / skill_exponent / difficulty_floor / sigma_floor fields are
 * `#[cfg(test)]` in upstream (test-only) and kept here as reserved constants.
 * The remaining scalars are the production defaults plus the two-factor and
 * xxy constants used by sunny.rs.
 */
export const SURFACE_TIMING_MODEL = Object.freeze({
    /** Upstream commit the whole surface timing port tracks: rosu-pp @ 328e339. */
    VERSION: "328e339",
    // ErrorModel production defaults (sunny_accuracy.rs DEFAULT_* / MEASURED_*;
    // sigma_ref & co. are `#[cfg(test)]` there and reserved here).
    ERROR_MODEL: Object.freeze({
        /** sunny_accuracy.rs `DEFAULT_SIGMA_REF` (test-only upstream). */
        sigmaRef: 18.0,
        /** sunny_accuracy.rs `DEFAULT_SKILL_EXPONENT` (test-only upstream). */
        skillExponent: 1.7,
        /** sunny_accuracy.rs `DEFAULT_DIFFICULTY_FLOOR` (test-only upstream). */
        difficultyFloor: 0.6,
        /** sunny_accuracy.rs `ErrorModel::default` sigma_floor (test-only upstream). */
        sigmaFloor: 0.0,
        /** sunny_accuracy.rs `DEFAULT_LAPSE_WEIGHT`. */
        lapseWeight: 0.0296,
        /** sunny_accuracy.rs `DEFAULT_LAPSE_RATIO`. */
        lapseRatio: 3.339,
        /** sunny_accuracy.rs `ErrorModel::default` release_sigma_ratio. */
        releaseSigmaRatio: 1.0,
        /** sunny_accuracy.rs `ErrorModel::default` short_hold_penalty. */
        shortHoldPenalty: 0.0,
        /** sunny_accuracy.rs `DEFAULT_SHORT_HOLD_SCALE`. */
        shortHoldScale: 120.0,
        /** sunny_accuracy.rs `ErrorModel::default` slip_rate. */
        slipRate: 0.0,
        /** sunny_accuracy.rs `DEFAULT_RELEASE_MEAN_OFFSET` (unfitted starting value). */
        releaseMeanOffset: 8.0,
        /** sunny_accuracy.rs `MEASURED_RECOVERY_OFFSET`. */
        recoveryOffset: 20.425,
        /** sunny_accuracy.rs `MEASURED_RECOVERY_TAU`. */
        recoveryTau: 116.68,
        /** sunny_accuracy.rs `MEASURED_ANTICIPATION_OFFSET`. */
        anticipationOffset: -2.517,
    }),
    /** sunny_accuracy.rs `TIMING_CORE_SIGMA` (replay-measured, reserved). */
    TIMING_CORE_SIGMA: 8.5,
    /** sunny_accuracy.rs `TIMING_BASELINE_SIGMA` — map-factor side (compute_timing_pp_with_units). */
    TIMING_BASELINE_SIGMA: 11.0,
    /** sunny.rs `compute_per_judgement_timing_adjustment` BASELINE_SIGMA local. */
    SCORE_ADJ_SIGMA: 12.0,
    /** sunny.rs `compute_map_timing_difficulty` BASELINE_ACC. */
    BASELINE_ACC: 0.994,
    /** sunny.rs `compute_map_timing_difficulty` ACC_SCALE. */
    ACC_SCALE: 15.0,
    /** sunny.rs `compute_map_timing_difficulty` ln_factor boost. */
    LN_FACTOR_BOOST: 1.03,
    /** sunny.rs `compute_map_timing_difficulty` LN ratio gate (0.3). */
    LN_RATIO_GATE: 0.3,
    /** sunny.rs `compute_map_timing_difficulty` LN boost ratio gate (0.5). */
    LN_RATIO_BOOST_GATE: 0.5,
    /** sunny.rs `compute_map_timing_difficulty` LN bucket-count gate (3). */
    LN_BUCKET_GATE: 3,
    /** sunny.rs `compute_map_timing_difficulty` combined clamp. */
    MAP_CLAMP: [0.85, 1.15],
    /** sunny.rs `compute_per_judgement_timing_adjustment` multiplier clamp. */
    SCORE_CLAMP: [0.85, 1.15],
    /** sunny.rs `compute_per_judgement_timing_adjustment` SCALE. */
    LOSS_SCALE: 5.0,
    /** sunny_accuracy.rs `LN_DURATION_BUCKETS`. */
    LN_DURATION_BUCKETS: 8,
    /** sunny.rs `LN_DURATION_EDGES`. */
    LN_DURATION_EDGES: [45, 70, 100, 145, 210, 320, 550],
    /** sunny.rs `LN_DURATION_REPRESENTATIVES`. */
    LN_DURATION_REPRESENTATIVES: [34, 56, 84, 120, 175, 259, 419, 900],
    /** sunny.rs `compute_per_judgement_timing_adjustment` ACC_WEIGHTS (305-based). */
    ACC_WEIGHTS: [1, 300 / 305, 200 / 305, 100 / 305, 50 / 305, 0],
    /** sunny.rs `compute_per_judgement_timing_adjustment` PENALTY_WEIGHTS (uniform). */
    PENALTY_WEIGHTS: [1, 1, 1, 1, 1, 1],
    /** sunny.rs `calculate_performance_inner` xxy pattern coefficient 9.8. */
    PATTERN_MULTIPLIER: 9.8,
    /** sunny.rs `calculate_performance_inner` pattern difficulty floor (0.2). */
    PATTERN_FLOOR: 0.2,
});

/**
 * Complementary error function, Numerical Recipes rational approximation
 * (sunny_accuracy.rs `erfc`, L846-862). Fractional error below 1.2e-7
 * everywhere, including deep in the tail.
 * @param {number} x
 * @returns {number}
 */
export function erfc(x) {
    const z = Math.abs(x);
    const t = 1 / (1 + 0.5 * z);
    const poly =
        -1.26551223 +
        t * (1.00002368 +
        t * (0.37409196 +
        t * (0.09678418 +
        t * (-0.18628806 +
        t * (0.27886807 +
        t * (-1.13520398 +
        t * (1.48851587 + t * (-0.82215223 + t * 0.17087277))))))));
    const value = t * Math.exp(-z * z + poly);
    return x >= 0 ? value : 2 - value;
}

/**
 * Probability that a zero-mean normal with sd `sigma` exceeds `x` (not in
 * absolute value); `P(Z > x)` (sunny_accuracy.rs `one_sided_tail`, L812-838).
 * @param {number} x
 * @param {number} sigma
 * @returns {number}
 */
export function oneSidedTail(x, sigma) {
    if (x === Infinity || x === -Infinity) {
        return x > 0 ? 0 : 1;
    }
    if (sigma === Infinity || sigma === -Infinity) {
        return 0.5;
    }
    if (Number.isNaN(sigma) || sigma <= 0) {
        return x > 0 ? 0 : (x < 0 ? 1 : 0.5);
    }
    return 0.5 * erfc(x / (sigma * Math.SQRT2));
}

/**
 * Probability that a zero-mean normal with sd `sigma` produces an absolute
 * error greater than `bound` (sunny_accuracy.rs `tail`, L771-791).
 * @param {number} bound
 * @param {number} sigma
 * @returns {number}
 */
export function tail(bound, sigma) {
    if (bound <= 0) {
        return 1;
    }
    if (bound === Infinity) {
        return 0;
    }
    if (sigma === Infinity || sigma === -Infinity) {
        return 1;
    }
    if (Number.isNaN(sigma) || sigma <= 0) {
        return 0;
    }
    return erfc(bound / (sigma * Math.SQRT2));
}

/**
 * Two-component mixture exceedance (sunny_accuracy.rs `ErrorModel::exceedance`,
 * L663-675). The lapse component is `lapse_ratio` times as wide as the core.
 * @param {number} bound
 * @param {number} sigma
 * @param {{lapseWeight: number, lapseRatio: number}} model
 * @returns {number}
 */
export function exceedance(bound, sigma, model) {
    const weight = Math.min(1, Math.max(0, model.lapseWeight));
    if (weight <= 0) {
        return tail(bound, sigma);
    }
    const ratio = Math.max(model.lapseRatio, 1);
    return (1 - weight) * tail(bound, sigma) + weight * tail(bound, sigma * ratio);
}

/**
 * Exceedance for a distribution shifted by mean `mu` (sunny_accuracy.rs
 * `ErrorModel::exceedance_with_offset`, L703-727). At `mu === 0` short-circuits
 * to {@link exceedance} bit-for-bit, exactly like upstream.
 * @param {number} bound
 * @param {number} sigma
 * @param {number} mu
 * @param {{lapseWeight: number, lapseRatio: number}} model
 * @returns {number}
 */
export function exceedanceWithOffset(bound, sigma, mu, model) {
    if (mu === 0) {
        return exceedance(bound, sigma, model);
    }
    if (bound <= 0) {
        return 1;
    }
    const twoSided = (s) => oneSidedTail(bound - mu, s) + oneSidedTail(bound + mu, s);
    const weight = Math.min(1, Math.max(0, model.lapseWeight));
    if (weight <= 0) {
        return twoSided(sigma);
    }
    const ratio = Math.max(model.lapseRatio, 1);
    return (1 - weight) * twoSided(sigma) + weight * twoSided(sigma * ratio);
}

/**
 * Judgement probabilities for an explicit timing spread (sunny_accuracy.rs
 * `judgement_probabilities_with_sigma`, L925-970). Bands come from the
 * windows module via {@link getBand}; entries sum to 1 (minus any slip rate).
 * @param {{perfect: number, great: number, good: number, ok: number, meh: number, miss: number}} windows finalized windows
 * @param {{lapseWeight: number, lapseRatio: number, slipRate: number}} model
 * @param {number} sigma timing spread in ms
 * @param {number} [mu=0] mean offset in ms
 * @returns {number[]} length-6 probabilities in JUDGEMENT_NAMES order
 */
export function judgementProbabilitiesWithSigma(windows, model, sigma, mu) {
    const probabilities = new Array(6).fill(0);
    let remaining = 1.0;
    JUDGEMENT_NAMES.forEach((name, i) => {
        const upper = getBand(windows, name)[1];
        const outside = Math.min(remaining, exceedanceWithOffset(upper, sigma, mu, model));
        probabilities[i] = remaining - outside;
        remaining = outside;
    });
    const slip = Math.min(1, Math.max(0, model.slipRate));
    if (slip > 0) {
        for (let i = 0; i < 6; i += 1) {
            probabilities[i] *= 1 - slip;
        }
        probabilities[5] += slip;
    }
    return probabilities;
}

/**
 * A judgement unit: `weight` judgements at local difficulty `difficulty`,
 * spread scaled by `sigmaScale`, error mean shifted by `meanOffset` (+
 * `fadingMeanOffset` at full spread). Mirrors sunny_accuracy.rs
 * `JudgementUnit`; returned as a plain object.
 * @param {{difficulty: number, weight: number, sigmaScale?: number, meanOffset?: number, fadingMeanOffset?: number}} fields
 * @returns {{difficulty: number, weight: number, sigmaScale: number, meanOffset: number, fadingMeanOffset: number}}
 */
export function makeUnit({ difficulty, weight, sigmaScale = 1, meanOffset = 0, fadingMeanOffset = 0 }) {
    return { difficulty, weight, sigmaScale, meanOffset, fadingMeanOffset };
}

/**
 * `count` judgements sharing one local difficulty (sunny_accuracy.rs
 * `JudgementUnit::repeated`, L1211).
 * @param {number} difficulty
 * @param {number} count
 * @returns {object}
 */
export function makeRepeatedUnit(difficulty, count) {
    return makeUnit({ difficulty, weight: count });
}

/**
 * Timing-spread multiplier for a ScoreV1 long note whose release is
 * `releaseRatio` times as wide as its press: `sqrt(1 + ratio^2)`
 * (sunny_accuracy.rs `ln_sigma_scale`, L1123-1131). At ratio 1 this is
 * `sqrt(2)`.
 * @param {number} releaseRatio
 * @returns {number}
 */
export function lnSigmaScale(releaseRatio) {
    if (!Number.isFinite(releaseRatio)) {
        return Math.SQRT2;
    }
    const ratio = Math.max(releaseRatio, 1);
    return Math.sqrt(1 + ratio * ratio);
}

/**
 * How much wider a release lands than a press for a hold of `durationMs`
 * (sunny_accuracy.rs `release_ratio_for_duration`, L1163-1187).
 * @param {{releaseSigmaRatio: number, shortHoldPenalty: number, shortHoldScale: number}} model
 * @param {number} durationMs
 * @returns {number}
 */
export function releaseRatioForDuration(model, durationMs) {
    const base = Math.max(model.releaseSigmaRatio, 1);
    if (!Number.isFinite(durationMs) || durationMs <= 0) {
        return base * (1 + Math.max(model.shortHoldPenalty, 0));
    }
    const penalty = Math.max(model.shortHoldPenalty, 0);
    if (penalty <= 0) {
        return base;
    }
    const scale = model.shortHoldScale;
    if (!Number.isFinite(scale) || scale <= 0) {
        return base;
    }
    return base * (1 + penalty * Math.exp(-durationMs / scale));
}

/**
 * `lnSigmaScale(releaseRatioForDuration(...))` (sunny_accuracy.rs
 * `ln_sigma_scale_for_duration`, L1194).
 * @param {{releaseSigmaRatio: number, shortHoldPenalty: number, shortHoldScale: number}} model
 * @param {number} durationMs
 * @returns {number}
 */
export function lnSigmaScaleForDuration(model, durationMs) {
    return lnSigmaScale(releaseRatioForDuration(model, durationMs));
}

/**
 * `count` ScoreV1 long-note judgements of local difficulty `difficulty`,
 * widened by the release-asymmetry spread and shifted by the release mean
 * offset (sunny_accuracy.rs `JudgementUnit::long_note`, L1229-1237).
 * @param {number} difficulty
 * @param {number} count
 * @param {object} model ErrorModel (uses releaseSigmaRatio / shortHoldPenalty / shortHoldScale / releaseMeanOffset)
 * @param {number} durationMs hold length in map time
 * @returns {object}
 */
export function makeLongNoteUnit(difficulty, count, model, durationMs) {
    return makeUnit({
        difficulty,
        weight: count,
        sigmaScale: lnSigmaScaleForDuration(model, durationMs),
        meanOffset: model.releaseMeanOffset,
        fadingMeanOffset: 0,
    });
}

/**
 * Expected judgement counts over a unit list at a supplied core timing spread
 * (sunny_accuracy.rs `expected_counts_at_core_sigma`, L1286-1313). `coreSigma`
 * must be finite and > 0, mirroring the upstream assert.
 * @param {object[]} units judgement units
 * @param {object} windows finalized windows
 * @param {object} model ErrorModel
 * @param {number} coreSigma core timing spread in ms
 * @returns {number[]} length-6 totals in JUDGEMENT_NAMES order
 */
export function expectedCountsAtCoreSigma(units, windows, model, coreSigma) {
    if (!Number.isFinite(coreSigma) || coreSigma <= 0) {
        throw new Error("core timing spread must be finite and positive");
    }
    const totals = new Array(6).fill(0);
    for (const unit of units) {
        const sigma = coreSigma * unit.sigmaScale;
        const probs = judgementProbabilitiesWithSigma(windows, model, sigma, unit.meanOffset + unit.fadingMeanOffset);
        for (let i = 0; i < 6; i += 1) {
            totals[i] += unit.weight * probs[i];
        }
    }
    return totals;
}

/**
 * 305-weighted accuracy implied by a counts vector (sunny_accuracy.rs
 * `ExpectedCounts::custom_accuracy`, L993-1010).
 * @param {number[]} countsOrExpected length-6 counts in JUDGEMENT_NAMES order
 * @returns {number} 0..1
 */
export function expectedCountsCustomAccuracy(countsOrExpected) {
    const total = countsOrExpected.reduce((sum, c) => sum + c, 0);
    if (total <= 0) {
        return 0;
    }
    const weights = [305, 300, 200, 100, 50, 0];
    let weighted = 0;
    for (let i = 0; i < 6; i += 1) {
        weighted += countsOrExpected[i] * weights[i];
    }
    return weighted / (total * 305);
}

/**
 * Bin long-note durations into the 8 LN_DURATION_EDGES buckets
 * (sunny.rs `ln_duration_bucket` + `ln_duration_histogram`, L74/L111).
 * Non-positive durations are skipped; durations past the last edge go to the
 * open top bucket (index 7).
 * @param {number[]} longNoteDurationsMs
 * @returns {number[]} length-8 bucket counts
 */
export function buildLnDurationBuckets(longNoteDurationsMs) {
    const buckets = new Array(SURFACE_TIMING_MODEL.LN_DURATION_BUCKETS).fill(0);
    for (const duration of longNoteDurationsMs) {
        if (duration <= 0) {
            continue;
        }
        const bin = SURFACE_TIMING_MODEL.LN_DURATION_EDGES.findIndex((edge) => duration < edge);
        buckets[bin === -1 ? SURFACE_TIMING_MODEL.LN_DURATION_BUCKETS - 1 : bin] += 1;
    }
    return buckets;
}

/**
 * Build the judgement-unit list for a map/score (sunny.rs `judgement_units`
 * L1298-1381 as finalised by the plan). `unitsTotal` is an explicit input —
 * callers pass `classic ? nObjects : nObjects + nLongNotes`. Under V2
 * (`classic: false`) or for pure-rice maps a single uniform repeated unit is
 * returned; under V1 with long notes the LN buckets each become a long-note
 * unit at their representative duration, and the remainder becomes a rice unit.
 * @param {{stars: number, unitsTotal: number, nObjects: number, nLongNotes: number, lnBuckets: number[], classic: boolean, model: object, options?: {unitsOverride?: object[]}}} args
 * @returns {object[]} judgement units
 */
export function buildJudgementUnits({ stars, unitsTotal, nObjects, nLongNotes, lnBuckets, classic, model, options = {} }) {
    if (options.unitsOverride) {
        return options.unitsOverride;
    }
    const uniform = [makeRepeatedUnit(stars, unitsTotal)];
    if (!classic || nLongNotes === 0 || nObjects === 0) {
        return uniform;
    }
    const perObject = unitsTotal / nObjects;
    const units = [];
    let lnTotal = 0;
    for (let bin = 0; bin < SURFACE_TIMING_MODEL.LN_DURATION_BUCKETS; bin += 1) {
        const count = lnBuckets[bin];
        if (count > 0) {
            const weight = count * perObject;
            lnTotal += weight;
            units.push(makeLongNoteUnit(stars, weight, model, SURFACE_TIMING_MODEL.LN_DURATION_REPRESENTATIVES[bin]));
        }
    }
    const rice = Math.max(unitsTotal - lnTotal, 0);
    if (rice > 0) {
        units.push(makeRepeatedUnit(stars, rice));
    }
    return units.length > 0 ? units : uniform;
}

/**
 * Map-based timing difficulty factor (sunny.rs `compute_map_timing_difficulty`,
 * L818-873). A PP multiplier in ~[0.85, 1.15] reflecting the map's inherent
 * accuracy difficulty: window tightness via expected accuracy at baseline
 * sigma, plus an LN boost for varied-duration high-LN maps.
 * @param {{expectedAcc: number, nLongNotes: number, nObjects: number, lnBuckets: number[]}} args
 * @returns {{factor: number, windowFactor: number, lnFactor: number, expectedAcc: number}}
 */
export function computeMapTimingFactor({ expectedAcc, nLongNotes, nObjects, lnBuckets }) {
    const M = SURFACE_TIMING_MODEL;
    const windowFactor = 1 + (M.BASELINE_ACC - expectedAcc) * M.ACC_SCALE;
    const lnRatio = nObjects > 0 ? nLongNotes / nObjects : 0;
    let lnFactor = 1.0;
    if (lnRatio > M.LN_RATIO_GATE && nLongNotes > 0) {
        const bucketsUsed = lnBuckets.filter((c) => c > 0).length;
        if (bucketsUsed >= M.LN_BUCKET_GATE && lnRatio > M.LN_RATIO_BOOST_GATE) {
            lnFactor = M.LN_FACTOR_BOOST;
        }
    }
    const factor = Math.min(M.MAP_CLAMP[1], Math.max(M.MAP_CLAMP[0], windowFactor * lnFactor));
    return { factor, windowFactor, lnFactor, expectedAcc };
}

/**
 * Score-based timing adjustment from per-judgement loss analysis (sunny.rs
 * `compute_per_judgement_timing_adjustment`, L896-978). Player distribution is
 * compared against expected counts at SCORE_ADJ_SIGMA; better than expected
 * rewards > 1.0, worse penalises < 1.0.
 * @param {{playerCounts: number[], units: object[], windows: object, hitTotal: number, model: object}} args
 * @returns {{multiplier: number, playerLoss: number, expectedLoss: number, lossDiff: number, expectedArr: number[]}}
 */
export function computeScoreAdjustment({ playerCounts, units, windows, hitTotal, model }) {
    const M = SURFACE_TIMING_MODEL;
    if (hitTotal <= 0) {
        return { multiplier: 1, playerLoss: 0, expectedLoss: 0, lossDiff: 0, expectedArr: [0, 0, 0, 0, 0, 0] };
    }
    const expectedArr = expectedCountsAtCoreSigma(units, windows, model, M.SCORE_ADJ_SIGMA);
    let totalPlayerLoss = 0;
    let totalExpectedLoss = 0;
    for (let i = 0; i < 6; i += 1) {
        const lossPerHit = 1 - M.ACC_WEIGHTS[i];
        const playerLoss = (playerCounts[i] / hitTotal) * lossPerHit;
        const expectedLoss = (expectedArr[i] / hitTotal) * lossPerHit;
        totalPlayerLoss += playerLoss * M.PENALTY_WEIGHTS[i];
        totalExpectedLoss += expectedLoss * M.PENALTY_WEIGHTS[i];
    }
    const lossDiff = totalPlayerLoss - totalExpectedLoss;
    const multiplier = Math.min(M.SCORE_CLAMP[1], Math.max(M.SCORE_CLAMP[0], 1 - lossDiff * M.LOSS_SCALE));
    return { multiplier, playerLoss: totalPlayerLoss, expectedLoss: totalExpectedLoss, lossDiff, expectedArr };
}

/**
 * Full surface PP composition (sunny.rs `calculate_performance_inner`
 * L992-1103 plus the xxy_* helpers). Pattern difficulty follows the xxy
 * formulation; timing difficulty multiplies it by `mapFactor * scoreAdj`;
 * `noFail` applies the 0.75 normalize_for_human_reference factor to the final
 * pp. All returned fields are numbers.
 * @param {{stars: number, variety: number, accScalar: number, nObjects: number, scoreAccuracy: number, mapFactor: number, scoreAdj: number, noFail?: boolean}} args
 * @returns {object} full pp breakdown
 */
export function computeSurfacePp({ stars, variety, accScalar, nObjects, scoreAccuracy, mapFactor, scoreAdj, noFail = false }) {
    const M = SURFACE_TIMING_MODEL;
    const proportion = scoreAccuracy > 0.8
        ? 4.5 * (scoreAccuracy - 0.8) / Math.pow(100 * (1 - scoreAccuracy) + Math.pow(0.9, 20), 0.05)
        : 0;
    const varietyMultiplier = 0.945 + (1.055 - 0.945) / (1 + Math.exp(-3 * (variety - 3.25)));
    const sigmoid = 0.87 + 0.26 / (1 + Math.exp(-20 * (accScalar - 1)));
    const accMultiplier = sigmoid * (2 * Math.pow(scoreAccuracy, 20) - 1) + 2 - 2 * Math.pow(scoreAccuracy, 20);
    const lengthMultiplier = 1.1 / (1 + Math.sqrt(stars / (2 * nObjects)));
    const patternDifficulty = Math.max(stars, M.PATTERN_FLOOR) - 0.15;
    const xxyPpPattern = M.PATTERN_MULTIPLIER * Math.pow(patternDifficulty, 2.2) * varietyMultiplier * lengthMultiplier * 1.0;
    const xxyPpAccuracy = xxyPpPattern * (proportion * accMultiplier - 1);
    const xxyPp = xxyPpPattern + xxyPpAccuracy;
    const timingMultiplier = mapFactor * scoreAdj;
    const ppWithTiming = xxyPp * timingMultiplier;
    const ppTiming = ppWithTiming - xxyPp;
    const pp = ppWithTiming * (noFail ? 0.75 : 1);
    return {
        pp,
        ppTiming,
        ppWithTiming,
        xxyPp,
        xxyPpPattern,
        xxyPpAccuracy,
        timingMultiplier,
        mapFactor,
        scoreAdj,
        proportion,
        accMultiplier,
        varietyMultiplier,
        lengthMultiplier,
        scoreAccuracy,
        patternDifficulty,
    };
}