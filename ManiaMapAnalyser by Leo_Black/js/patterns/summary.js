import { PATTERNS_CONFIG } from "./config.js";
import { find } from "./findPatterns.js";
import { calculateClusteredPatterns } from "./clustering.js";
import { CORE_PATTERN_LIST } from "./patternsDef.js";
import { lnPercent, svTime } from "./primitives.js";
import { NoteType } from "./chart.js";
import { categoriseChart } from "./categorise.js";

const LN_CORE_PATTERNS = new Set(["Coordination", "Density", "Wildcard"]);

function hbRowRatio(chart) {
    const rows = chart?.Notes || [];
    if (!rows.length) return 0;

    let hbRows = 0;
    for (const row of rows) {
        const data = Array.isArray(row?.Data) ? row.Data : [];
        const hasHead = data.some((n) => n === NoteType.HOLDHEAD);
        const hasNormal = data.some((n) => n === NoteType.NORMAL);
        if (hasHead && hasNormal) {
            hbRows += 1;
        }
    }

    return hbRows / rows.length;
}

function resolveModeTag(lnRatio, hbRatio) {
    if (lnRatio <= PATTERNS_CONFIG.LN_MODE_LOW_THRESHOLD) return "RC";
    if (lnRatio >= PATTERNS_CONFIG.LN_MODE_HIGH_THRESHOLD) return "LN";
    if (hbRatio >= PATTERNS_CONFIG.HB_ROW_RATIO_THRESHOLD) return "HB";
    return "Mix";
}

export function fromChart(chart) {
    const lnRatio = lnPercent(chart);
    const hbRatio = hbRowRatio(chart);
    const modeTag = resolveModeTag(lnRatio, hbRatio);

    let patterns = find(chart);
    if (modeTag === "RC") {
        patterns = patterns.filter((p) => !LN_CORE_PATTERNS.has(p.Pattern));
    }

    const clusters = calculateClusteredPatterns(patterns, { modeTag })
    .filter((c) => c.BPM > 25 || c.BPM === 0)
    .sort((a, b) => b.Amount - a.Amount);

    function canBePruned(cluster) {
    for (const other of clusters) {
            if (other.Pattern === cluster.Pattern && other.Amount * 0.5 > cluster.Amount && other.BPM > cluster.BPM) {
        return true;
            }
    }
    return false;
    }

    const filtered = clusters.filter((c) => !canBePruned(c));

    const prunedClusters = [];
    for (const pattern of CORE_PATTERN_LIST) {
    prunedClusters.push(...filtered.filter((c) => c.Pattern === pattern).slice(0, 3));
    }
    prunedClusters.sort((a, b) => b.Importance - a.Importance);

    const svAmount = svTime(chart);
    const category = categoriseChart(chart.Keys, prunedClusters, svAmount);

    return {
    Clusters: prunedClusters,
    Category: category,
    LNPercent: lnRatio,
    HBRowRatio: hbRatio,
    ModeTag: modeTag,
    SVAmount: svAmount,
    Duration: chart.LastNote - chart.FirstNote,
    get ImportantClusters() {
            if (!this.Clusters.length) return [];
            const importance = this.Clusters[0].Importance;
            const out = [];
            for (const c of this.Clusters) {
        if (c.Importance / importance > PATTERNS_CONFIG.IMPORTANT_CLUSTER_RATIO) out.push(c);
        else break;
            }
            return out;
    },
    };
}
