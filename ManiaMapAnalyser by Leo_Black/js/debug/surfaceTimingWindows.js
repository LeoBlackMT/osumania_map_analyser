// Surface timing windows for osu!mania hit judgement.
//
// Ported from rosu-pp mania-surface-map-timing-difficulty @ 328e339
// `src/mania/sunny_windows.rs`. Zero-import, DOM-free pure module:
// runs unchanged in the tosu overlay (browser) and in Node (benchmark smoke).
//
// Window semantics match rosu-pp: raw ms windows are `finalize`d through
// `(floor(value / multiplier * clockRate) + 0.5) / clockRate` — no epsilon
// padding, exactly like the Rust `HitWindows::finalize`.

/** Lazer hit windows per OD anchor: { od0, od5, od10 } (from sunny_windows.rs `LAZER_*`). */
export const LAZER_PERFECT = { od0: 22.4, od5: 19.4, od10: 13.9 };
export const LAZER_GREAT = { od0: 64, od5: 49, od10: 34 };
export const LAZER_GOOD = { od0: 97, od5: 82, od10: 67 };
export const LAZER_OK = { od0: 127, od5: 112, od10: 97 };
export const LAZER_MEH = { od0: 151, od5: 136, od10: 121 };
export const LAZER_MISS = { od0: 188, od5: 173, od10: 158 };

/** Classic hit windows (convert maps, round(od) > 4) — sunny_windows.rs `CLASSIC_TIGHT`. */
export const CLASSIC_TIGHT = { perfect: 16, great: 34, good: 67, ok: 97, meh: 121, miss: 158 };
/** Classic hit windows (convert maps, round(od) <= 4) — sunny_windows.rs `CLASSIC_LOOSE`. */
export const CLASSIC_LOOSE = { perfect: 16, great: 47, good: 77, ok: 97, meh: 121, miss: 158 };

/** HR difficulty multiplier — sunny_windows.rs `DIFFICULTY_MULTIPLIER_HR`. */
export const DIFFICULTY_MULTIPLIER_HR = 1.4;
/** EZ difficulty multiplier — sunny_windows.rs `DIFFICULTY_MULTIPLIER_EZ`. */
export const DIFFICULTY_MULTIPLIER_EZ = 1 / 1.4;

/** Judgement names in ascending strictness order — sunny_windows.rs `JUDGEMENT_NAMES`. */
export const JUDGEMENT_NAMES = ["perfect", "great", "good", "ok", "meh", "miss"];

/**
 * Interpolate an OD-anchored window range at `od` (Rust `difficulty_range`).
 * @param {number} od overall difficulty (0..10)
 * @param {{od0: number, od5: number, od10: number}} range anchor windows
 * @returns {number} ms window
 */
export function difficultyRange(od, range) {
    return od > 5
        ? range.od5 + (range.od10 - range.od5) * (od - 5) / 5
        : range.od0 + (range.od5 - range.od0) * od / 5;
}

/**
 * Lazer hit windows at `od` (Rust `lazer_windows`).
 * @param {number} od overall difficulty (0..10)
 * @returns {{perfect: number, great: number, good: number, ok: number, meh: number, miss: number}} raw ms windows
 */
export function lazerWindows(od) {
    return {
        perfect: difficultyRange(od, LAZER_PERFECT),
        great: difficultyRange(od, LAZER_GREAT),
        good: difficultyRange(od, LAZER_GOOD),
        ok: difficultyRange(od, LAZER_OK),
        meh: difficultyRange(od, LAZER_MEH),
        miss: difficultyRange(od, LAZER_MISS),
    };
}

/**
 * Classic hit windows at `od` (Rust `classic_windows`). Convert maps pick the
 * tight/loose table by `Math.round(od)`; non-convert maps widen linearly with
 * `antiOd = clamp(10 - od, 0, 10)` (3 ms per band, perfect stays 16).
 * @param {number} od overall difficulty (0..10)
 * @param {boolean} [isConvert=false] true for convert (stable) maps
 * @returns {{perfect: number, great: number, good: number, ok: number, meh: number, miss: number}} raw ms windows
 */
export function classicWindows(od, isConvert) {
    if (isConvert) {
        return Math.round(od) > 4 ? CLASSIC_TIGHT : CLASSIC_LOOSE;
    }
    const antiOd = Math.min(10, Math.max(0, 10 - od));
    const wide = 3 * antiOd;
    return {
        perfect: 16,
        great: 34 + wide,
        good: 67 + wide,
        ok: 97 + wide,
        meh: 121 + wide,
        miss: 158 + wide,
    };
}

/**
 * Apply difficulty multiplier and clock rate to raw windows (Rust
 * `HitWindows::finalize`): `(floor(value / multiplier * clockRate) + 0.5) / clockRate`.
 * No epsilon padding — mirrors upstream exactly. Returns a new object.
 * @param {{perfect: number, great: number, good: number, ok: number, meh: number, miss: number}} windows raw ms windows
 * @param {number} multiplier difficulty multiplier (HR/EZ)
 * @param {number} [clockRate=1] playback rate
 * @returns {{perfect: number, great: number, good: number, ok: number, meh: number, miss: number}} final ms windows
 */
export function finalize(windows, multiplier, clockRate) {
    const apply = (value) => (Math.floor(value / multiplier * clockRate) + 0.5) / clockRate;
    return {
        perfect: apply(windows.perfect),
        great: apply(windows.great),
        good: apply(windows.good),
        ok: apply(windows.ok),
        meh: apply(windows.meh),
        miss: apply(windows.miss),
    };
}

/**
 * Difficulty multiplier from speed-rate-neutral mods (Rust
 * `difficulty_multiplier`, HR checked before EZ).
 * @param {{hr?: boolean, ez?: boolean}} [mods={}]
 * @returns {number} 1.4 for HR, 1/1.4 for EZ, else 1.0
 */
export function difficultyMultiplier({ hr = false, ez = false } = {}) {
    if (hr) {
        return DIFFICULTY_MULTIPLIER_HR;
    }
    if (ez) {
        return DIFFICULTY_MULTIPLIER_EZ;
    }
    return 1.0;
}

/**
 * Build finalized hit windows for a map (Rust `hit_windows`): classic by
 * default, lazer when `classic: false`.
 * @param {{od: number, isConvert?: boolean, classic?: boolean, hr?: boolean, ez?: boolean, clockRate?: number}} options
 * @returns {{perfect: number, great: number, good: number, ok: number, meh: number, miss: number}} final ms windows
 */
export function buildHitWindows({ od, isConvert = false, classic = true, hr = false, ez = false, clockRate = 1 }) {
    const raw = classic ? classicWindows(od, isConvert) : lazerWindows(od);
    return finalize(raw, difficultyMultiplier({ hr, ez }), clockRate);
}

/**
 * Build map windows with an empty mod set (Rust `map_windows`): no HR/EZ,
 * clock rate applied.
 * @param {{od: number, isConvert?: boolean, classic?: boolean, clockRate?: number}} options
 * @returns {{perfect: number, great: number, good: number, ok: number, meh: number, miss: number}} final ms windows
 */
export function buildMapWindows({ od, isConvert = false, classic = true, clockRate = 1 }) {
    return buildHitWindows({ od, isConvert, classic, clockRate });
}

/**
 * Map a hit error (ms) to a judgement (Rust `judge`). Negative errors use
 * absolute value; misses are everything beyond the meh window.
 * @param {{perfect: number, great: number, good: number, ok: number, meh: number, miss: number}} windows finalized windows
 * @param {number} errorMs hit error in ms
 * @returns {"perfect"|"great"|"good"|"ok"|"meh"|"miss"} judgement
 */
export function judgeError(windows, errorMs) {
    const error = Math.abs(errorMs);
    if (error <= windows.perfect) {
        return "perfect";
    }
    if (error <= windows.great) {
        return "great";
    }
    if (error <= windows.good) {
        return "good";
    }
    if (error <= windows.ok) {
        return "ok";
    }
    if (error <= windows.meh) {
        return "meh";
    }
    return "miss";
}

/**
 * Absolute window band [lower, upper] in ms for a judgement (Rust `band()`,
 * for the S3 curve module). Bands are contiguous: upper of one judgement is
 * the lower of the next.
 * @param {{perfect: number, great: number, good: number, ok: number, meh: number, miss: number}} windows finalized windows
 * @param {string} judgementName one of JUDGEMENT_NAMES
 * @returns {[number, number]} inclusive lower, exclusive upper (miss upper is Infinity)
 */
export function getBand(windows, judgementName) {
    switch (judgementName) {
        case "perfect":
            return [0, windows.perfect];
        case "great":
            return [windows.perfect, windows.great];
        case "good":
            return [windows.great, windows.good];
        case "ok":
            return [windows.good, windows.ok];
        case "meh":
            return [windows.ok, windows.meh];
        case "miss":
            return [windows.meh, Infinity];
        default:
            throw new Error(`unknown judgement: ${judgementName}`);
    }
}