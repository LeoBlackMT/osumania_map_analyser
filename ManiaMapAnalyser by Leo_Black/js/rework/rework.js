import { SR_INTERVALS } from "./intervals.js";
import { calculate as calculateXxy } from "./xxyAlgorithm.js";
import { calculateDaniel } from "./danielAlgorithm.js";

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

        if (lnRatio < 0.1) return rcDiff;

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

        if (lnRatio > 0.9) return lnDiff;
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

        if (lnRatio < 0.1) return rcDiff;

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

        if (lnRatio > 0.9) return lnDiff;
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

        if (lnRatio < 0.1) return rcDiff;

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

        if (lnRatio > 0.9) return lnDiff;
        return `${rcDiff} || ${lnDiff}`;
    }

    return "Unknown difficulty";
}

export function runReworkFromText(osuText, options = {}) {
    const speedRate = options.speedRate ?? 1.0;
    const odFlag = options.odFlag ?? null;
    const cvtFlag = options.cvtFlag ?? null;
    const withGraph = options.withGraph === true;
    const useDanielAlgorithm = options.useDanielAlgorithm === true;

    let result;
    if (useDanielAlgorithm) {
        const danielResult = calculateDaniel(osuText, speedRate, odFlag, { withGraph });
        result = danielResult === -3
            ? calculateXxy(osuText, speedRate, odFlag, cvtFlag, { withGraph })
            : danielResult;
    } else {
        result = calculateXxy(osuText, speedRate, odFlag, cvtFlag, { withGraph });
    }

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

    return {
        star: sr,
        lnRatio,
        columnCount,
        estDiff: estDiff(sr, lnRatio, columnCount),
        graph,
    };
}
