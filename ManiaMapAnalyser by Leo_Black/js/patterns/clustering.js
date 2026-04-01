import { PATTERNS_CONFIG } from "./config.js";
import { resolveRatingMultiplier } from "./patternsDef.js";

function patternAmount(sortedStartsEnds) {
    let totalTime = 0;
    let [currentStart, currentEnd] = sortedStartsEnds[0];

    for (const [start, end] of sortedStartsEnds) {
    if (currentEnd < end) {
            totalTime += (currentEnd - currentStart);
            currentStart = start;
            currentEnd = end;
    } else {
            currentEnd = Math.max(currentEnd, end);
    }
    }

    totalTime += (currentEnd - currentStart);
    return totalTime;
}

function createClusterBuilder(value) {
    return {
    SumMs: value,
    OriginalMsPerBeat: value,
    Count: 1,
    BPM: null,
    add(v) {
            this.Count += 1;
            this.SumMs += v;
    },
    calculate() {
            const average = this.SumMs / this.Count;
            this.BPM = average <= 0 ? 0 : Math.round(60000.0 / average);
    },
    get Value() {
            return this.BPM;
    },
    };
}

function assignClusters(patterns) {
    const bpmsNonMixed = [];
    const bpmsMixed = new Map();

    function addToCluster(msPerBeat) {
    for (const c of bpmsNonMixed) {
            if (Math.abs(c.OriginalMsPerBeat - msPerBeat) < PATTERNS_CONFIG.BPM_CLUSTER_THRESHOLD) {
        c.add(msPerBeat);
        return c;
            }
    }
    const c = createClusterBuilder(msPerBeat);
    bpmsNonMixed.push(c);
    return c;
    }

    function addToMixedCluster(pattern, value) {
    if (bpmsMixed.has(pattern)) {
            const c = bpmsMixed.get(pattern);
            c.add(value);
            return c;
    }
    const c = createClusterBuilder(value);
    bpmsMixed.set(pattern, c);
    return c;
    }

    const patternsWithClusters = [];
    for (const p of patterns) {
    const c = p.Mixed ? addToMixedCluster(p.Pattern, p.MsPerBeat) : addToCluster(p.MsPerBeat);
    patternsWithClusters.push([p, c]);
    }

    for (const c of bpmsNonMixed) c.calculate();
    for (const c of bpmsMixed.values()) c.calculate();

    return patternsWithClusters;
}

function specificClusters(patternsWithClusters, options = {}) {
        const modeTag = options.modeTag || "Mix";
    const groups = new Map();

    for (const [p, c] of patternsWithClusters) {
    const key = `${p.Pattern}@@${p.Mixed ? 1 : 0}@@${c.Value}`;
    if (!groups.has(key)) {
            groups.set(key, { pattern: p.Pattern, mixed: p.Mixed, bpm: c.Value, data: [] });
    }
    groups.get(key).data.push([p, c]);
    }

    const out = [];
    for (const group of groups.values()) {
    const startsEnds = group.data.map(([m]) => [m.Start, m.End]).sort((a, b) => a[0] - b[0]);

    const dataCount = group.data.length;
    const counter = new Map();
    for (const [m] of group.data) {
            if (m.SpecificType != null) {
        counter.set(m.SpecificType, (counter.get(m.SpecificType) || 0) + 1);
            }
    }

    const specificTypes = [...counter.entries()]
            .map(([name, count]) => [name, count / dataCount])
            .sort((a, b) => b[1] - a[1]);

    const dominantSpecific = specificTypes.length ? specificTypes[0][0] : null;
    const amount = startsEnds.length ? patternAmount(startsEnds) : 0;

    out.push({
            Pattern: group.pattern,
            SpecificTypes: specificTypes,
            RatingMultiplier: resolveRatingMultiplier(group.pattern, dominantSpecific, modeTag),
            BPM: group.bpm,
            Mixed: group.mixed,
            Amount: amount,
            get Importance() {
        return this.Amount * this.RatingMultiplier * Number(this.BPM);
            },
            format(rate = 1.0) {
        const name = (this.SpecificTypes.length > 0 && this.SpecificTypes[0][1] >= PATTERNS_CONFIG.CLUSTER_SPECIFIC_NAME_MIN_RATIO)
                    ? this.SpecificTypes[0][0]
                    : this.Pattern;
        if (this.Mixed) {
                    return `~${Math.round(Number(this.BPM) * rate)}BPM Mixed ${name}`;
        }
        return `${Math.round(Number(this.BPM) * rate)}BPM ${name}`;
            },
    });
    }

        const hasDW = out.some((c) => c.Pattern === "Density" || c.Pattern === "Wildcard");
        if (hasDW && PATTERNS_CONFIG.RELEASE_WITH_DW_MULTIPLIER !== 1.0) {
        for (const c of out) {
                        if (c.SpecificTypes.some(([name, ratio]) => name === "Release" && ratio > 0)) {
                c.RatingMultiplier *= PATTERNS_CONFIG.RELEASE_WITH_DW_MULTIPLIER;
                        }
        }
        }

    return out;
}

export function calculateClusteredPatterns(patterns, options = {}) {
    const pwc = assignClusters(patterns);
        return specificClusters(pwc, options);
}
