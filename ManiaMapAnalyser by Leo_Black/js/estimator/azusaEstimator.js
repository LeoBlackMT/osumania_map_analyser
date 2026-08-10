import { OsuFileParser } from "../parser/osuFileParser.js";
import { runDanielEstimatorFromText } from "./danielEstimator.js";
import { runSunnyEstimatorFromText } from "./sunnyEstimator.js";
import { numericToRcLabel } from "./rcDifficultyFormat.js";

const AZUSA_CONFIG = Object.freeze({
    rcLnRatioLimit: 0.18,
    minNotes: 80,
    rowToleranceMs: 2,
    skillWeights: Object.freeze({
        speed: 0.36,
        stamina: 0.24,
        chord: 0.12,
        tech: 0.16,
        jack: 0.12,
    }),
    localPower: 2.15,
    decayWindowsMs: Object.freeze([140, 280, 560, 980]),
    decayWeights: Object.freeze([0.34, 0.30, 0.22, 0.14]),
    lengthRefNotes: 600,
    lengthExponent: 0.22,
    lengthCap: 3.5,
});

const AZUSA_CALIBRATION_LOW_BLOCKS = Object.freeze([
    [1.9220, 1.9220, 1.0000],
    [2.3660, 2.7684, 1.6667],
    [2.8394, 2.8394, 2.0000],
    [2.8584, 3.7162, 2.3333],
    [3.7798, 3.7798, 3.0000],
    [3.8667, 3.8667, 3.0000],
    [4.2067, 5.2039, 4.3333],
    [5.2506, 5.7713, 5.0667],
    [5.8603, 6.1512, 5.3333],
    [6.3292, 6.8785, 6.0000],
    [7.1715, 7.3617, 6.2000],
    [7.4079, 7.8734, 7.2000],
    [8.0160, 8.4003, 8.2500],
    [8.4133, 8.4133, 9.0000],
    [8.9031, 9.4775, 9.5667],
    [9.6488, 9.6488, 10.0000],
    [9.8301, 9.8301, 10.3000],
]);

const AZUSA_CALIBRATION_HIGH_BLOCKS = Object.freeze([
    [11.4336, 11.4336, 10.4000],
    [11.4436, 11.4436, 10.5000],
    [11.6012, 11.6665, 10.6500],
    [11.6696, 12.2317, 11.5000],
    [12.3295, 12.3919, 11.7500],
    [12.5238, 12.5238, 12.0000],
    [12.5318, 12.8329, 12.1400],
    [12.8605, 12.9781, 12.2800],
    [12.9868, 13.1170, 12.7800],
    [13.2003, 13.4418, 12.7857],
    [13.4660, 13.5829, 12.9250],
    [13.6044, 13.9924, 13.3667],
    [14.0583, 14.0583, 13.4000],
    [14.0795, 14.2266, 13.4600],
    [14.2346, 14.2346, 13.6000],
    [14.2414, 14.2414, 13.7000],
    [14.2903, 14.2903, 14.0000],
    [14.3258, 14.4760, 14.1200],
    [14.5365, 14.6006, 14.1333],
    [14.7269, 14.8716, 14.1333],
    [15.0048, 15.0048, 14.4000],
    [15.0521, 15.0521, 14.4000],
    [15.0521, 15.0521, 14.4000],
    [15.0950, 15.0950, 14.4000],
    [15.2335, 15.2335, 14.4000],
    [15.2388, 15.5821, 14.7385],
    [15.6977, 15.7002, 14.8500],
    [15.7535, 16.1593, 15.0667],
    [16.2009, 16.2958, 15.1000],
    [16.3172, 16.4748, 15.7600],
    [16.5620, 16.9083, 15.9833],
    [16.9485, 16.9485, 16.0000],
    [17.0216, 17.3799, 16.1000],
    [17.4616, 17.4616, 16.4000],
    [17.5167, 17.5167, 16.4000],
    [17.5306, 17.9077, 16.6400],
    [18.1973, 18.1973, 17.2000],
    [18.2026, 18.2026, 17.2000],
    [18.4562, 19.3477, 17.9500],
    [19.3477, 20.5000, 18.2000],
    [20.5000, 22.0000, 18.6000],
    [22.0000, 24.0000, 19.2000],
    [24.0000, 27.0000, 20.0000],
]);

const AZUSA_ISOTONIC_POINTS = Object.freeze([
    [1.3868, 1.0000],
    [1.4574, 1.0000],
    [1.5361, 1.0000],
    [1.6320, 1.5000],
    [1.9833, 2.5800],
    [2.2465, 2.6000],
    [2.3344, 2.8000],
    [2.5779, 3.4500],
    [3.8277, 3.6000],
    [4.2824, 4.3429],
    [4.5665, 4.6250],
    [4.8016, 4.6750],
    [4.9529, 5.1500],
    [5.1029, 5.4000],
    [5.2475, 5.4750],
    [5.5039, 5.9000],
    [5.6951, 6.0143],
    [5.9213, 6.4000],
    [6.0093, 6.9000],
    [6.1337, 7.2000],
    [6.7092, 7.4400],
    [7.2846, 7.5000],
    [7.4233, 7.8000],
    [7.9790, 8.6000],
    [8.2927, 8.6143],
    [9.0829, 9.5000],
    [9.4639, 9.6154],
    [9.8115, 10.0000],
    [9.8344, 10.4000],
    [10.0013, 10.4000],
    [10.0778, 10.5000],
    [10.1054, 10.5000],
    [10.1435, 10.6000],
    [10.4782, 10.6462],
    [10.8866, 10.8000],
    [11.0934, 11.1727],
    [11.3266, 11.2867],
    [11.4970, 11.4000],
    [11.6024, 11.4750],
    [11.6947, 11.6000],
    [11.8932, 12.0636],
    [12.0076, 12.3000],
    [12.2947, 12.4150],
    [12.7583, 12.4500],
    [12.8756, 12.9000],
    [12.9268, 12.9000],
    [13.0042, 13.2000],
    [13.2387, 13.2694],
    [13.4620, 13.4400],
    [13.5467, 13.5000],
    [13.6016, 13.7375],
    [13.9609, 13.9500],
    [14.1414, 14.0250],
    [14.2226, 14.0762],
    [14.3178, 14.1273],
    [14.3786, 14.1643],
    [14.4421, 14.2182],
    [14.4825, 14.3000],
    [14.5063, 14.3750],
    [14.5452, 14.4778],
    [14.6359, 14.5850],
    [14.7301, 14.6389],
    [14.8846, 14.7906],
    [15.0424, 14.9263],
    [15.2159, 15.0944],
    [15.3942, 15.1875],
    [15.5380, 15.3300],
    [15.8096, 15.5320],
    [16.0262, 16.1000],
    [16.0702, 16.1000],
    [16.2738, 16.1267],
    [16.4723, 16.3579],
    [16.7156, 16.8000],
    [17.1446, 17.0600],
    [17.5478, 17.2000],
    [17.6403, 17.2000],
    [17.7603, 17.2000],
    [17.8264, 17.6000],
    [18.1258, 17.9750],
    [18.5000, 18.2000],
    [19.2000, 18.7000],
    [20.0000, 19.2000],
    [21.2000, 19.8000],
    [22.5000, 20.0000],
]);

function buildErrorResult(code, message, extras = {}) {
    return {
        star: Number.NaN,
        lnRatio: Number.isFinite(extras.lnRatio) ? extras.lnRatio : 0,
        columnCount: Number.isFinite(extras.columnCount) ? extras.columnCount : 0,
        estDiff: `Invalid: ${message}`,
        numericDifficulty: null,
        numericDifficultyHint: code,
        graph: null,
        rawNumericDifficulty: null,
        debug: {
            code,
            message,
        },
    };
}

function clamp(value, min, max) {
    return Math.max(min, Math.min(max, value));
}

function safeDiv(a, b, fallback = 0) {
    if (!Number.isFinite(a) || !Number.isFinite(b) || Math.abs(b) < 1e-9) return fallback;
    return a / b;
}

function fmt4(value) {
    return Number.isFinite(value) ? Number(value.toFixed(4)) : null;
}

function piecewiseLinear(x, knots, valueCol = 1) {
    const v = Number(x);
    if (!Number.isFinite(v) || !Array.isArray(knots) || knots.length === 0) return v;
    if (v <= knots[0][0]) return knots[0][valueCol];
    const last = knots.length - 1;
    if (v >= knots[last][0]) return knots[last][valueCol];
    for (let i = 0; i < last; i += 1) {
        const x0 = knots[i][0], y0 = knots[i][valueCol];
        const x1 = knots[i + 1][0], y1 = knots[i + 1][valueCol];
        if (v >= x0 && v <= x1) return y0 + safeDiv((v - x0) * (y1 - y0), x1 - x0, 0);
    }
    return v;
}

function piecewiseBlock(x, blocks) {
    const v = Number(x);
    if (!Number.isFinite(v) || !Array.isArray(blocks) || blocks.length === 0) return v;
    if (v <= blocks[0][0]) return blocks[0][2];
    const last = blocks.length - 1;
    for (let i = 0; i < blocks.length; i += 1) {
        const [x0, x1, y] = blocks[i];
        if (v >= x0 && v <= x1) return y;
        if (i < last && v > x1 && v < blocks[i + 1][0]) {
            const t = safeDiv(v - x1, blocks[i + 1][0] - x1, 0);
            return y * (1 - t) + blocks[i + 1][2] * t;
        }
    }
    return blocks[last][2];
}

function estimateDanielNumeric(result) {
    const numericRaw = result?.numericDifficulty;
    if (typeof numericRaw === "number" && Number.isFinite(numericRaw)) {
        return numericRaw;
    }

    if (typeof numericRaw === "string" && numericRaw.trim().length > 0) {
        const parsed = Number(numericRaw);
        if (Number.isFinite(parsed)) {
            return parsed;
        }
    }
    
    const star = Number(result?.star);
    if (!Number.isFinite(star)) {
        return null;
    }

    // Piecewise map keeps Daniel high-end semantics while extending low-end below Alpha.
    if (star >= 6.56) {
        const normalized = clamp((star - 6.56) / 0.58, 0, 9.99);
        return Number((11 + normalized).toFixed(2));
    }

    const lowPart = -2 + 13 * Math.pow(clamp(star / 6.56, 0, 1), 1.72);
    return Number(lowPart.toFixed(2));
}

function hasDanielNativeNumeric(result) {
    const raw = result?.numericDifficulty;
    if (typeof raw === "number") {
        return Number.isFinite(raw);
    }

    if (typeof raw === "string" && raw.trim().length > 0) {
        return Number.isFinite(Number(raw));
    }

    return false;
}

function estimateSunnyNumeric(result) {
    const star = Number(result?.star);
    if (!Number.isFinite(star)) {
        return null;
    }

    const numeric = 2.85 + 1.33 * star;
    return Number(clamp(numeric, -2, 20).toFixed(2));
}

function quantileFromSorted(sortedValues, q) {
    if (!sortedValues.length) {
        return 0;
    }

    const t = clamp(Number(q), 0, 1) * (sortedValues.length - 1);
    const left = Math.floor(t);
    const right = Math.min(sortedValues.length - 1, left + 1);
    const w = t - left;
    return (sortedValues[left] * (1 - w)) + (sortedValues[right] * w);
}

function powerMean(values, p) {
    if (!values.length) {
        return 0;
    }

    let acc = 0;
    for (const value of values) {
        acc += Math.pow(Math.max(value, 0), p);
    }
    return Math.pow(acc / values.length, 1 / p);
}

function buildTapNotes(parsed) {
    const taps = [];
    const columns = parsed.columns || [];
    const starts = parsed.noteStarts || [];

    for (let i = 0; i < columns.length; i += 1) {
        const col = Number(columns[i]);
        const time = Number(starts[i]);
        if (!Number.isFinite(col) || !Number.isFinite(time)) {
            continue;
        }

        taps.push({
            t: time,
            c: col,
            hand: col < 2 ? 0 : 1,
            rowSize: 1,
        });
    }

    taps.sort((a, b) => {
        if (a.t !== b.t) return a.t - b.t;
        return a.c - b.c;
    });

    return taps;
}

function annotateRows(taps, toleranceMs) {
    if (!taps.length) {
        return;
    }

    let rowStart = 0;
    for (let i = 1; i <= taps.length; i += 1) {
        const shouldFlush = i === taps.length || Math.abs(taps[i].t - taps[rowStart].t) > toleranceMs;
        if (!shouldFlush) {
            continue;
        }

        const rowSize = i - rowStart;
        for (let j = rowStart; j < i; j += 1) {
            taps[j].rowSize = rowSize;
        }
        rowStart = i;
    }
}

function expDecayFactor(dtMs, tauMs) {
    if (!Number.isFinite(dtMs) || dtMs <= 0) {
        return 1;
    }
    return Math.exp(-dtMs / tauMs);
}

function skillFromStates(states) {
    let sum = 0;
    for (let i = 0; i < states.length; i += 1) {
        sum += states[i] * AZUSA_CONFIG.decayWeights[i];
    }
    return sum;
}

function buildDifficultyCurve(taps) {
    const states = {
        speed: Array.from({ length: AZUSA_CONFIG.decayWindowsMs.length }, () => 0),
        stamina: Array.from({ length: AZUSA_CONFIG.decayWindowsMs.length }, () => 0),
        chord: Array.from({ length: AZUSA_CONFIG.decayWindowsMs.length }, () => 0),
        tech: Array.from({ length: AZUSA_CONFIG.decayWindowsMs.length }, () => 0),
        jack: Array.from({ length: AZUSA_CONFIG.decayWindowsMs.length }, () => 0),
    };

    const lastByColumn = [-1e9, -1e9, -1e9, -1e9];
    const lastByHand = [-1e9, -1e9];

    const density250 = [];
    const density500 = [];
    const jackRawSeries = [];
    const columnCounts = [0, 0, 0, 0];
    let chordNoteCount = 0;
    let cursor250 = 0;
    let cursor500 = 0;

    const local = [];
    const speedSeries = [];
    const staminaSeries = [];
    const chordSeries = [];
    const techSeries = [];
    const jackSeries = [];
    const times = [];

    let prevTime = taps[0]?.t ?? 0;
    let prevAny1 = -1e9;
    let prevAny2 = -1e9;
    let prevCol = 0;

    for (let i = 0; i < taps.length; i += 1) {
        const note = taps[i];
        const t = note.t;
        const c = note.c;
        columnCounts[c] += 1;
        if (note.rowSize >= 2) {
            chordNoteCount += 1;
        }

        const dtGlobal = i === 0 ? 0 : Math.max(0, t - prevTime);
        const dtSame = Math.max(0, t - lastByColumn[c]);
        const dtHand = Math.max(0, t - lastByHand[note.hand]);
        const dtAny = Math.max(0, t - prevAny1);

        while (cursor250 < i && t - taps[cursor250].t > 250) cursor250 += 1;
        while (cursor500 < i && t - taps[cursor500].t > 500) cursor500 += 1;

        const d250 = (i - cursor250 + 1) / 0.25;
        const d500 = (i - cursor500 + 1) / 0.5;
        density250.push(d250);
        density500.push(d500);

        const jack = Math.pow(190 / (dtSame + 35), 1.16);
        jackRawSeries.push(jack);
        const stream = Math.pow(170 / (dtAny + 30), 1.07);
        const handStream = Math.pow(185 / (dtHand + 42), 1.08);

        const movement = Math.abs(c - prevCol) / 3;
        const rhythmRatio = safeDiv(Math.max(dtAny, 1), Math.max(t - prevAny2, 1), 1);
        const rhythmChaos = Math.abs(Math.log2(clamp(rhythmRatio, 0.2, 5)));

        const rowChord = Math.max(0, note.rowSize - 1);
        const chord = Math.pow(rowChord + 1, 1.22) - 1;

        const speedInput = 0.60 * stream + 0.30 * handStream + 0.10 * jack;
        const jackInput = jack * (1 + 0.15 * chord);
        const staminaInput = 0.48 * (d500 / 11) + 0.27 * (d250 / 15) + 0.25 * stream;
        const chordInput = chord * (1 + 0.10 * Math.min(1.5, stream));
        const techInput = 0.45 * rhythmChaos + 0.30 * movement + 0.25 * (rowChord > 0 ? 1 + 0.3 * rowChord : 0);

        for (let j = 0; j < AZUSA_CONFIG.decayWindowsMs.length; j += 1) {
            const tau = AZUSA_CONFIG.decayWindowsMs[j];
            const decay = expDecayFactor(dtGlobal, tau);
            states.speed[j] = states.speed[j] * decay + speedInput;
            states.stamina[j] = states.stamina[j] * decay + staminaInput;
            states.chord[j] = states.chord[j] * decay + chordInput;
            states.tech[j] = states.tech[j] * decay + techInput;
            states.jack[j] = states.jack[j] * decay + jackInput;
        }

        const speedSkill = skillFromStates(states.speed);
        const staminaSkill = skillFromStates(states.stamina);
        const chordSkill = skillFromStates(states.chord);
        const techSkill = skillFromStates(states.tech);
        const jackSkill = skillFromStates(states.jack);

        const p = AZUSA_CONFIG.localPower;
        const sw = AZUSA_CONFIG.skillWeights;
        const combined = Math.pow(
            (
                sw.speed * Math.pow(Math.max(speedSkill, 0), p)
                + sw.stamina * Math.pow(Math.max(staminaSkill, 0), p)
                + sw.chord * Math.pow(Math.max(chordSkill, 0), p)
                + sw.tech * Math.pow(Math.max(techSkill, 0), p)
                + sw.jack * Math.pow(Math.max(jackSkill, 0), p)
            )
            / (sw.speed + sw.stamina + sw.chord + sw.tech + sw.jack),
            1 / p,
        );

        local.push(combined);
        speedSeries.push(speedSkill);
        staminaSeries.push(staminaSkill);
        chordSeries.push(chordSkill);
        techSeries.push(techSkill);
        jackSeries.push(jackSkill);
        times.push(t);

        prevAny2 = prevAny1;
        prevAny1 = t;
        prevTime = t;
        prevCol = c;
        lastByColumn[c] = t;
        lastByHand[note.hand] = t;
    }

    return {
        local,
        speedSeries,
        staminaSeries,
        chordSeries,
        techSeries,
        jackSeries,
        times,
        density250,
        density500,
        jackRawSeries,
        columnCounts,
        chordNoteCount,
    };
}

function computeAzusaNumericFromCurve(curve, noteCount) {
    const local = curve.local;
    if (!local.length) {
        return 0;
    }

    const summarize = (values) => {
        const sorted = [...values].sort((a, b) => a - b);
        const q97 = quantileFromSorted(sorted, 0.97);
        const q94 = quantileFromSorted(sorted, 0.94);
        const q90 = quantileFromSorted(sorted, 0.90);
        const q75 = quantileFromSorted(sorted, 0.75);
        const q50 = quantileFromSorted(sorted, 0.50);
        const tailCount = Math.max(8, Math.floor(sorted.length * 0.04));
        const tailSlice = sorted.slice(sorted.length - tailCount);
        const tailMean = tailSlice.reduce((acc, value) => acc + value, 0) / tailSlice.length;
        const pm = powerMean(values, 2.6);
        return { q97, q94, q90, q75, q50, tailMean, pm };
    };

    const speed = summarize(curve.speedSeries);
    const stamina = summarize(curve.staminaSeries);
    const chord = summarize(curve.chordSeries);
    const tech = summarize(curve.techSeries);
    const jack = summarize(curve.jackSeries);

    const density250 = powerMean(curve.density250, 1.18);
    const density500 = powerMean(curve.density500, 1.12);
    const { lengthRefNotes, lengthExponent, lengthCap } = AZUSA_CONFIG;
    const lengthBoost = Math.min(lengthCap, Math.pow(Math.max(noteCount, 1) / lengthRefNotes, lengthExponent));

    const peakBlend =
        (0.26 * speed.q97)
        + (0.22 * stamina.q97)
        + (0.10 * chord.q97)
        + (0.10 * tech.q97)
        + (0.10 * jack.q97)
        + (0.06 * speed.q90)
        + (0.04 * stamina.q90)
        + (0.02 * chord.q90)
        + (0.02 * tech.q90)
        + (0.02 * jack.q90);

    const sustainBlend =
        (0.18 * speed.q75)
        + (0.16 * stamina.q75)
        + (0.08 * chord.q75)
        + (0.06 * tech.q75)
        + (0.08 * jack.q75)
        + (0.10 * speed.tailMean)
        + (0.08 * stamina.tailMean)
        + (0.04 * chord.tailMean)
        + (0.04 * tech.tailMean)
        + (0.04 * jack.tailMean);

    const densityBlend = (0.14 * Math.log1p(density250)) + (0.22 * Math.log1p(density500));
    const midBlend =
        (0.16 * speed.q50) + (0.13 * stamina.q50)
        + (0.06 * chord.q50) + (0.06 * tech.q50) + (0.06 * jack.q50);

    const raw =
        (0.52 * peakBlend) + (0.26 * sustainBlend)
        + (0.10 * densityBlend) + (0.08 * midBlend) + (0.04 * lengthBoost);
    const scaled = 0.82 + (0.43 * raw);

    const maxColumn = Math.max(...curve.columnCounts);
    const anchorImbalance = safeDiv((maxColumn / Math.max(noteCount, 1)) - 0.25, 0.75, 0);
    const chordRate = safeDiv(curve.chordNoteCount, Math.max(noteCount, 1), 0);
    const jackSorted = [...curve.jackRawSeries].sort((a, b) => a - b);
    const jackQ95 = quantileFromSorted(jackSorted, 0.95);

    // Chordjack interaction: chord density × jack density co-occurrence
    const chordjackBoost = clamp(
        2.5
        * clamp((chordRate - 0.40) * 3.5, 0, 1)
        * clamp((jackQ95 - 1.25) * 2.8, 0, 1)
        * clamp(1 - (anchorImbalance * 8), 0, 1),
        0,
        2.2,
    );

    const totalTimeSec = Math.max(1, (curve.times[curve.times.length - 1] - curve.times[0]) / 1000);
    const avgNPS = noteCount / totalTimeSec;
    const midSpeedBonus = clamp((avgNPS - 9) * 0.04, 0, 0.35) * clamp((19 - avgNPS) * 0.25, 0, 1);

    const corrected = scaled + chordjackBoost + midSpeedBonus;
    return clamp(corrected, -2, 20);
}

function resolveRcBlendComponents(primaryNumeric, danielNumeric, sunnyNumeric, curveHints = null) {
    const primary = Number.isFinite(primaryNumeric) ? primaryNumeric : null;
    const daniel = Number.isFinite(danielNumeric) ? danielNumeric : null;
    const sunny = Number.isFinite(sunnyNumeric) ? sunnyNumeric : null;

    if (daniel == null && primary == null && sunny == null) {
        return {
            value: null,
            lowGateSource: null,
            lowGate: null,
            highGate: null,
            lowBase: null,
            highBase: null,
        };
    }

    const lowGateSource = daniel != null ? daniel : (sunny ?? primary ?? 0);
    const lowGate = clamp((9.61 - lowGateSource) / 4.94, 0, 1);
    const highGate = 1 - lowGate;

    const lowBase = (() => {
        if (sunny == null) {
            return null;
        }

        let value = (-8.317) + (1.536 * sunny);
        if (primary != null) {
            value += 0.011 * primary;
        }
        if (daniel != null) {
            value += 0.049 * daniel;
        }

        if (lowGate > 0) {
            const primaryPart = primary != null ? Math.max(0, primary - 10.4) : 0;
            const sunnyPart = Math.max(0, sunny - 9.84);
            const lowSunnyConvex = Math.pow(Math.max(0, 7.935 - sunny), 2);
            value += lowGate * ((0.442 * sunnyPart) + (0.016 * primaryPart) + (0.235 * lowSunnyConvex));
        }

        return value;
    })();

    const highBase = (() => {
        const dUse = daniel != null ? daniel : (sunny ?? primary);
        if (dUse == null) {
            return null;
        }

        const primaryUse = primary ?? dUse;
        const sunnyUse = sunny ?? dUse;

        let value = (0.809 * dUse) + (0.057 * primaryUse) + (0.165 * sunnyUse) + 0.183;

        const highMask = clamp((lowGateSource - 14.83) / 2.667, 0, 1);
        if (highMask > 0) {
            value += highMask
            * ((-0.154 * Math.max(0, primaryUse - dUse)) + (0.081 * Math.max(0, sunnyUse - dUse)));
        }

        const anchorImbalance = Number.isFinite(curveHints?.anchorImbalance) ? curveHints.anchorImbalance : null;
        const chordRate = Number.isFinite(curveHints?.chordRate) ? curveHints.chordRate : null;
        const jackQ95 = Number.isFinite(curveHints?.jackQ95) ? curveHints.jackQ95 : null;
        if (anchorImbalance != null && chordRate != null && jackQ95 != null) {
            const anchorLift = clamp(
                0.20
                * Math.max(0, jackQ95 - 2.08)
                * Math.max(0, 0.24 - chordRate)
                * Math.max(0, anchorImbalance - 0.10),
                0,
                0.25,
            );
            value += anchorLift;
        }

        return value;
    })();

    const lowLift = Number.isFinite(lowGateSource)
        ? Math.max(0, 9.889 - lowGateSource) * 0.257
        : 0;

    if (lowBase == null && highBase == null) {
        return {
            value: null,
            lowGateSource,
            lowGate,
            highGate,
            lowBase,
            highBase,
        };
    }

    if (lowBase == null) {
        return {
            value: highBase,
            lowGateSource,
            lowGate,
            highGate,
            lowBase,
            highBase,
        };
    }

    if (highBase == null) {
        return {
            value: lowBase + lowLift,
            lowGateSource,
            lowGate,
            highGate,
            lowBase,
            highBase,
        };
    }

    return {
        value: (lowBase * lowGate) + ((highBase + lowLift) * highGate),
        lowGateSource,
        lowGate,
        highGate,
        lowBase,
        highBase,
    };
}

function calibrateAzusaNumeric(value, lowGate = null, highGate = null) {
    const v = Number(value);
    if (!Number.isFinite(v)) return v;
    const low = piecewiseBlock(v, AZUSA_CALIBRATION_LOW_BLOCKS);
    const high = piecewiseBlock(v, AZUSA_CALIBRATION_HIGH_BLOCKS);
    const lg = Number.isFinite(lowGate) ? clamp(Number(lowGate), 0, 1) : null;
    const hg = Number.isFinite(highGate) ? clamp(Number(highGate), 0, 1) : null;
    if (lg == null && hg == null) return v < 11 ? low : high;
    const lw = lg ?? Math.max(0, 1 - (hg ?? 0));
    const hw = hg ?? Math.max(0, 1 - lw);
    const ws = lw + hw;
    if (ws <= 1e-6) return v < 11 ? low : high;
    return (lw * low + hw * high) / ws;
}

function calibrateAzusaOutputNumeric(value) {
    return piecewiseLinear(Number(value), AZUSA_ISOTONIC_POINTS, 1);
}

function computeCurveGapResidualCorrection(baseNumeric, blendDetails, curveStats, primaryNumeric, sunnyNumeric, danielNumeric) {
    const x = Number(baseNumeric);
    if (!Number.isFinite(x)) {
        return 0;
    }

    const highGate = Number.isFinite(blendDetails?.highGate) ? clamp(blendDetails.highGate, 0, 1) : 0;
    const primary = Number.isFinite(primaryNumeric) ? primaryNumeric : x;
    const sunny = Number.isFinite(sunnyNumeric) ? sunnyNumeric : x;
    const daniel = Number.isFinite(danielNumeric) ? danielNumeric : x;
    const ds = daniel - sunny;
    const sp = sunny - primary;
    const anchorImbalance = Number.isFinite(curveStats?.anchorImbalance) ? curveStats.anchorImbalance : 0;
    const chordRate = Number.isFinite(curveStats?.chordRate) ? curveStats.chordRate : 0;
    const jackQ95 = Number.isFinite(curveStats?.jackQ95) ? curveStats.jackQ95 : 0;

    const residual = (
        4.335282
        + (-0.170459 * x)
        + (-1.622303 * Math.max(0, 11 - x))
        + (1.328125 * Math.max(0, 12.5 - x))
        + (-0.042829 * Math.max(0, 14 - x))
        + (-0.834997 * highGate)
        + (3.060352 * highGate * Math.max(0, 11 - x))
        + (-1.744638 * highGate * Math.max(0, 12.5 - x))
        + (0.409922 * ds)
        + (0.041072 * sp)
        + (-0.388231 * highGate * ds)
        + (-0.170185 * highGate * sp)
        + (3.466868 * anchorImbalance)
        + (-1.743778 * chordRate)
        + (-0.094758 * jackQ95)
        + (2.626366 * anchorImbalance * jackQ95)
        + (1.836357 * chordRate * jackQ95)
        + (-2.612648 * highGate * anchorImbalance)
        + (-2.493596 * highGate * chordRate)
    );

    return clamp(residual, -1.2, 1.2);
}

function computeReferenceCorrection(azusaEst, danielNumeric, sunnyNumeric) {
    const x = Number(azusaEst);
    if (!Number.isFinite(x)) return 0;

    if (x < 10.0 || x > 17.5) return 0;

    const daniel = Number.isFinite(danielNumeric) ? danielNumeric : null;
    const sunny = Number.isFinite(sunnyNumeric) ? sunnyNumeric : null;

    // Range-dependent gate and coefficients
    let gate, coeffD, coeffS;

    if (x < 11.5) {
        gate = clamp((x - 10.0) / 1.5, 0, 1);
        coeffD = 0.10;
        coeffS = 0.06;
    } else if (x < 12.5) {
        gate = 1.0;
        coeffD = 0.20;
        coeffS = 0.13;
    } else if (x < 16.0) {
        gate = 1.0;
        coeffD = 0.40;
        coeffS = 0.25;
    } else {
        gate = clamp((17.5 - x) / 1.5, 0, 1);
        coeffD = 0.28;
        coeffS = 0.17;
    }

    let correction = 0;
    if (daniel != null) correction += coeffD * (daniel - x);
    if (sunny != null) correction += coeffS * (sunny - x);

    return clamp(correction * gate, -1.2, 1.2);
}

export function runAzusaEstimatorFromText(osuText, options = {}, parsed = null) {
    const speedRate = Number.isFinite(options.speedRate) && options.speedRate > 0 ? Number(options.speedRate) : 1.0;
    const withGraph = options.withGraph === true;
    const forceSunnyReferenceHo = options.forceSunnyReferenceHo !== false;
    const precomputedDanielResult = options.precomputedDanielResult || null;
    const precomputedSunnyResult = options.precomputedSunnyResult || null;

    const parser = parsed != null ? parsed : new OsuFileParser(osuText);
    if (parsed == null) parser.process();
    const parsedData = parser.getParsedData();

    const lnRatio = Number(parsedData?.lnRatio) || 0;
    const columnCount = Number(parsedData?.columnCount) || 0;

    if (parsedData?.status === "Fail") {
        return buildErrorResult("ParseFailed", "Beatmap parse failed", { lnRatio, columnCount });
    }

    if (parsedData?.status === "NotMania") {
        return buildErrorResult("NotMania", "Beatmap mode is not mania", { lnRatio, columnCount });
    }

    if (columnCount !== 4) {
        return buildErrorResult("UnsupportedKeys", "Azusa only supports 4K", { lnRatio, columnCount });
    }

    const taps = buildTapNotes(parsedData);
    if (taps.length < AZUSA_CONFIG.minNotes) {
        return buildErrorResult(
            "TooShort",
            `Insufficient notes for stable estimate (${taps.length})`,
            { lnRatio, columnCount },
        );
    }

    const timeScale = speedRate !== 0 ? (1 / speedRate) : 1;
    const scaledTaps = timeScale === 1
        ? taps
        : taps.map((note) => ({
            ...note,
            t: note.t * timeScale,
        }));

    annotateRows(scaledTaps, AZUSA_CONFIG.rowToleranceMs * timeScale);

    const curve = buildDifficultyCurve(scaledTaps);
    const primaryNumeric = computeAzusaNumericFromCurve(curve, taps.length);

    const maxColumn = Math.max(...curve.columnCounts);
    const anchorImbalance = safeDiv((maxColumn / Math.max(taps.length, 1)) - 0.25, 0.75, 0);
    const chordRate = safeDiv(curve.chordNoteCount, Math.max(taps.length, 1), 0);
    const jackSorted = [...curve.jackRawSeries].sort((a, b) => a - b);
    const jackQ95 = quantileFromSorted(jackSorted, 0.95);

    let danielNumeric = null;
    let danielResult = precomputedDanielResult || null;
    let danielHasNativeNumeric = false;
    let sunnyNumeric = null;
    let sunnyResult = precomputedSunnyResult;

    if (precomputedDanielResult) {
        danielNumeric = estimateDanielNumeric(precomputedDanielResult);
        danielHasNativeNumeric = hasDanielNativeNumeric(precomputedDanielResult);
    } else {
        try {
            danielResult = runDanielEstimatorFromText(osuText, options, parsed);
            danielNumeric = estimateDanielNumeric(danielResult);
            danielHasNativeNumeric = hasDanielNativeNumeric(danielResult);
        } catch {
            danielNumeric = null;
            danielResult = null;
            danielHasNativeNumeric = false;
        }
    }

    if (sunnyResult) {
        sunnyNumeric = estimateSunnyNumeric(sunnyResult);
    } else {
        try {
            const sunnyOptions = forceSunnyReferenceHo
                ? { ...options, cvtFlag: "HO" }
                : options;
            sunnyResult = runSunnyEstimatorFromText(osuText, sunnyOptions, parsed);
            sunnyNumeric = estimateSunnyNumeric(sunnyResult);
        } catch {
            sunnyNumeric = null;
            sunnyResult = null;
        }
    }

    let danielNumericForBlend = danielNumeric;
    if (!danielHasNativeNumeric && Number.isFinite(danielNumeric)) {
        const highSignal = Math.max(
            Number.isFinite(primaryNumeric) ? primaryNumeric : -Infinity,
            Number.isFinite(sunnyNumeric) ? sunnyNumeric : -Infinity,
            danielNumeric,
        );

        if (highSignal < 14) {
            const speedDelta = speedRate - 1.0;
            const fallbackScale = speedDelta < 0
                ? clamp((-speedDelta) * 0.43, 0, 1)
                : clamp(speedDelta * 0.35, 0, 1);

            danielNumericForBlend = danielNumeric * fallbackScale;
        }
    }

    const blendDetails = resolveRcBlendComponents(primaryNumeric, danielNumericForBlend, sunnyNumeric, {
        anchorImbalance,
        chordRate,
        jackQ95,
    });
    const numericDifficulty = blendDetails.value;
    const calibratedNumeric = calibrateAzusaNumeric(numericDifficulty, blendDetails.lowGate, blendDetails.highGate);
    const curveGapResidual = computeCurveGapResidualCorrection(
        calibratedNumeric,
        blendDetails,
        { anchorImbalance, chordRate, jackQ95 },
        primaryNumeric,
        sunnyNumeric,
        danielNumericForBlend,
    );
    const preOutputNumeric = clamp(Number(calibratedNumeric) + curveGapResidual, -2, 20);
    const outputNumeric = calibrateAzusaOutputNumeric(preOutputNumeric);
    const refCorrection = computeReferenceCorrection(outputNumeric, danielNumericForBlend, sunnyNumeric);
    const finalNumeric = clamp(Number(outputNumeric) + refCorrection, -2, 20);
    const estDiff = numericToRcLabel(finalNumeric);

    const result = {
        star: Number((3.4 + 0.38 * finalNumeric).toFixed(4)),
        lnRatio,
        columnCount,
        estDiff,
        numericDifficulty: Number(finalNumeric.toFixed(2)),
        numericDifficultyHint: "azusa-rc-v1",
        graph: withGraph ? (sunnyResult?.graph || null) : null,
        rawNumericDifficulty: Number(primaryNumeric.toFixed(4)),
        debug: {
            primaryNumeric: fmt4(primaryNumeric),
            blendNumeric: fmt4(numericDifficulty),
            danielNumeric: fmt4(danielNumeric),
            danielNumericForBlend: fmt4(danielNumericForBlend),
            danielHasNativeNumeric,
            sunnyNumeric: fmt4(sunnyNumeric),
            notes: taps.length,
            calibratedNumeric: fmt4(calibratedNumeric),
            curveStats: {
                anchorImbalance: fmt4(anchorImbalance),
                chordRate: fmt4(chordRate),
                jackQ95: fmt4(jackQ95),
            },
            curveGapResidual: fmt4(curveGapResidual),
            outputNumeric: fmt4(outputNumeric),
            postCurveGapResidual: fmt4(refCorrection),
            finalNumeric: fmt4(finalNumeric),
            blend: {
                lowGateSource: blendDetails.lowGateSource?.toFixed(4) ?? null,
                lowGate: blendDetails.lowGate?.toFixed(4) ?? null,
                highGate: blendDetails.highGate?.toFixed(4) ?? null,
                lowBase: blendDetails.lowBase?.toFixed(4) ?? null,
                highBase: blendDetails.highBase?.toFixed(4) ?? null,
            },
        },
    };

    return result;
}
