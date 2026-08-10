import { DAN_INDEX } from "./intervals/index.js";

const DAN_MEANS = [
    [6.562, "Alpha"],
    [6.957, "Beta"],
    [7.459, "Gamma"],
    [7.939, "Delta"],
    [9.095, "Epsilon"],
    [9.473, "Emik Zeta"],
    [10.162, "Thaumiel Eta"],
    [10.782, "CloverWisp Theta"],
];

const DAN_ORDER_START = 11;

function precomputeDanBoundaries() {
    const means = DAN_MEANS.map(([mean]) => mean);
    const boundaries = [];

    for (let i = 0; i < DAN_MEANS.length; i += 1) {
        const mean = means[i];
        const lower = i > 0
            ? (means[i - 1] + mean) / 2
            : mean - (((means[1] + mean) / 2) - mean);
        const upper = i < means.length - 1
            ? (mean + means[i + 1]) / 2
            : mean + ((mean - means[i - 1]) / 2);
        boundaries.push([lower, upper]);
    }

    return boundaries;
}

const DAN_BOUNDARIES = precomputeDanBoundaries();

export function estimateDanielDan(sr) {
    if (!Number.isFinite(sr)) {
        return {
            label: "Unknown",
            numeric: null,
        };
    }

    if (sr < DAN_BOUNDARIES[0][0]) {
        return {
            label: `< ${DAN_MEANS[0][1]} Low`,
            numeric: null,
        };
    }

    if (sr >= DAN_BOUNDARIES[DAN_BOUNDARIES.length - 1][1]) {
        return {
            label: `> ${DAN_MEANS[DAN_MEANS.length - 1][1]} High`,
            numeric: null,
        };
    }

    for (let i = 0; i < DAN_MEANS.length; i += 1) {
        const [lower, upper] = DAN_BOUNDARIES[i];
        if (sr >= lower && sr < upper) {
            const tRaw = (sr - lower) / (upper - lower);
            const t = Math.max(0, Math.min(tRaw, 1));
            const numeric = Number((DAN_ORDER_START + i + t).toFixed(2));

            let label;
            if (t < 1 / 3) {
                label = `${DAN_MEANS[i][1]} Low`;
            } else if (t < 2 / 3) {
                label = `${DAN_MEANS[i][1]} Mid`;
            } else {
                label = `${DAN_MEANS[i][1]} High`;
            }

            return {
                label,
                numeric,
            };
        }
    }

    return {
        label: "Unknown",
        numeric: null,
    };
}

function intervalLookup(sr, table, fallbackLabel) {
    for (const [lower, upper, name] of table) {
        if (lower <= sr && sr <= upper) return name;
    }
    if (sr < table[0][0]) return `< ${table[0][2]}`;
    if (sr > table[table.length - 1][1]) return `> ${table[table.length - 1][2]}`;
    return fallbackLabel;
}

// enableAlwaysShowLNDifficulty default false = config.js defaults.enableAlwaysShowLNDifficulty.
// Callers (analysis.js etc.) pass the resolved state value explicitly.
export function estDiff(sr, lnRatio, columnCount, useExtended = false, enableAlwaysShowLNDifficulty = false) {
    const keys = DAN_INDEX[columnCount];
    if (!keys) return "Unknown difficulty";

    const rcTable = keys.RC[useExtended ? "extended" : "default"] ?? keys.RC.default;
    const rcDiff = intervalLookup(sr, rcTable, "Unknown RC difficulty");
    if (lnRatio < 0.15 && !enableAlwaysShowLNDifficulty) return rcDiff;

    const lnTable = keys.LN[useExtended ? "extended" : "default"] ?? keys.LN.default;
    const lnDiff = intervalLookup(sr, lnTable, "Unknown LN difficulty");
    return `${rcDiff} || ${lnDiff}`;
}

export function estDiff2(sr, srLN, columnCount, useExtended = false) {
    const keys = DAN_INDEX[columnCount];
    if (!keys) return "Unknown difficulty";

    const rcTable = keys.RC[useExtended ? "extended" : "default"] ?? keys.RC.default;
    const rcDiff = intervalLookup(sr, rcTable, "Unknown RC difficulty");
    if (srLN <= 0) return rcDiff;

    const lnTable = keys.LN[useExtended ? "extended" : "default"] ?? keys.LN.default;
    const lnDiff = intervalLookup(srLN, lnTable, "Unknown LN difficulty");
    return `${rcDiff} || ${lnDiff}`;
}

export function normalizeReworkResult(result) {
    if (typeof result === "number") {
        if (result === -1) {
            throw new Error("Beatmap parse failed");
        }
        if (result === -2) {
            throw new Error("Beatmap mode is not mania");
        }
        throw new Error(`Unknown result code: ${result}`);
    }

    let sr;
    let lnRatio;
    let columnCount;
    let graph = null;

    if (Array.isArray(result)) {
        [sr, lnRatio, columnCount] = result;
    } else if (result && typeof result === "object") {
        sr = Number(result.star);
        lnRatio = Number(result.lnRatio);
        columnCount = Number(result.columnCount);
        graph = result.graph && typeof result.graph === "object" ? result.graph : null;
    } else {
        throw new Error("Unexpected calculation result format");
    }

    if (!Number.isFinite(sr) || !Number.isFinite(lnRatio) || !Number.isFinite(columnCount)) {
        throw new Error("Invalid estimator output" + sr + lnRatio + columnCount);
    }

    return {
        star: sr,
        lnRatio,
        columnCount,
        graph,
    };
}
