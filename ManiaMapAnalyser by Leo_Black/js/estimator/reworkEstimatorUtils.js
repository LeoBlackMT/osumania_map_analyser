import { SR_INTERVALS } from "./intervals.js";

const DAN_MEANS = {
    Alpha: 6.562,
    Beta: 6.957,
    Gamma: 7.459,
    Delta: 7.939,
    Epsilon: 9.095,
    Zeta: 9.473,
    Eta: 10.162,
    Theta: 10.782,
};

const DAN_ORDER = Object.keys(DAN_MEANS);
const DAN_ORDER_START = 11;

function precomputeDanBoundaries() {
    const means = DAN_ORDER.map((name) => DAN_MEANS[name]);
    const boundaries = [];

    for (let i = 0; i < DAN_ORDER.length; i += 1) {
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
            label: `< ${DAN_ORDER[0]} Low`,
            numeric: null,
        };
    }

    if (sr >= DAN_BOUNDARIES[DAN_BOUNDARIES.length - 1][1]) {
        return {
            label: `> ${DAN_ORDER[DAN_ORDER.length - 1]} High`,
            numeric: null,
        };
    }

    for (let i = 0; i < DAN_ORDER.length; i += 1) {
        const [lower, upper] = DAN_BOUNDARIES[i];
        if (sr >= lower && sr < upper) {
            const tRaw = (sr - lower) / (upper - lower);
            const t = Math.max(0, Math.min(tRaw, 1));
            const numeric = Number((DAN_ORDER_START + i + t).toFixed(2));

            let label;
            if (t < 1 / 3) {
                label = `${DAN_ORDER[i]} Low`;
            } else if (t < 2 / 3) {
                label = `${DAN_ORDER[i]} Mid`;
            } else {
                label = `${DAN_ORDER[i]} High`;
            }
            
            switch (i){
                case 5:
                    label = `Emik ${label}`;
                    break;
                case 6:
                    label = `Thaumiel ${label}`;
                    break;
                case 7:
                    label = `CloverWisp ${label}`;
                    break;
                default:
                    break;
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

export function estDiff(sr, lnRatio, columnCount) {
    if (columnCount === 4) {
        let rcDiff = null;
        for (const [lower, upper, name] of SR_INTERVALS.RC_intervals_4K) {
            if (lower <= sr && sr <= upper) {
                rcDiff = name;
                break;
            }
        }
        if (rcDiff == null) {
            if (sr < 1.502) rcDiff = "< Intro 1 low";
            else if (sr > 11.129) rcDiff = "> Theta high";
            else rcDiff = "Unknown RC difficulty";
        }

        if (lnRatio < 0.15) return rcDiff;

        let lnDiff = null;
        for (const [lower, upper, name] of SR_INTERVALS.LN_intervals_4K) {
            if (lower <= sr && sr <= upper) {
                lnDiff = name;
                break;
            }
        }
        if (lnDiff == null) {
            if (sr < 4.832) lnDiff = "< LN 5 mid";
            else if (sr > 9.589) lnDiff = "> LN 17 high";
            else lnDiff = "Unknown LN difficulty";
        }

        return `${rcDiff} || ${lnDiff}`;
    }

    if (columnCount === 6) {
        let rcDiff = null;
        for (const [lower, upper, name] of SR_INTERVALS.RC_intervals_6K) {
            if (lower <= sr && sr <= upper) {
                rcDiff = name;
                break;
            }
        }
        if (rcDiff == null) {
            if (sr < 3.430) rcDiff = "< Regular 0 low";
            else if (sr > 7.965) rcDiff = "> Regular 9 high";
            else rcDiff = "Unknown RC difficulty";
        }

        if (lnRatio < 0.15) return rcDiff;

        let lnDiff = null;
        for (const [lower, upper, name] of SR_INTERVALS.LN_intervals_6K) {
            if (lower <= sr && sr <= upper) {
                lnDiff = name;
                break;
            }
        }
        if (lnDiff == null) {
            if (sr < 3.530) lnDiff = "< LN 0 low";
            else if (sr > 9.700) lnDiff = "> LN Finish high";
            else lnDiff = "Unknown LN difficulty";
        }

        return `${rcDiff} || ${lnDiff}`;
    }

    if (columnCount === 7) {
        let rcDiff = null;
        for (const [lower, upper, name] of SR_INTERVALS.RC_intervals_7K) {
            if (lower <= sr && sr <= upper) {
                rcDiff = name;
                break;
            }
        }
        if (rcDiff == null) {
            if (sr < 3.5085) rcDiff = "< Regular 0 low";
            else if (sr > 10.544) rcDiff = "> Regular Stellium high";
            else rcDiff = "Unknown RC difficulty";
        }

        if (lnRatio < 0.15) return rcDiff;

        let lnDiff = null;
        for (const [lower, upper, name] of SR_INTERVALS.LN_intervals_7K) {
            if (lower <= sr && sr <= upper) {
                lnDiff = name;
                break;
            }
        }
        if (lnDiff == null) {
            if (sr < 4.836) lnDiff = "< LN 3 low";
            else if (sr > 10.666) lnDiff = "> LN Stellium high";
            else lnDiff = "Unknown LN difficulty";
        }

        return `${rcDiff} || ${lnDiff}`;
    }

    return "Unknown difficulty";
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
        throw new Error("Invalid estimator output");
    }

    return {
        star: sr,
        lnRatio,
        columnCount,
        graph,
    };
}
