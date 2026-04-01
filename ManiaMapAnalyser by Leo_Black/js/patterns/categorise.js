import { PATTERNS_CONFIG } from "./config.js";

function isHybridChart(primary, secondary) {
    void primary;
    void secondary;
    return false;
}

export function categoriseChart(keys, orderedClusters, svAmount) {
    void keys;
    void svAmount;

    if (!orderedClusters.length) {
        return "Uncategorised";
    }

    const firstImportance = orderedClusters[0].Importance;
    const important = [];
    for (const cluster of orderedClusters) {
        if ((cluster.Importance / firstImportance) > PATTERNS_CONFIG.IMPORTANT_CLUSTER_RATIO) {
            important.push(cluster);
        } else {
            break;
        }
    }

    const cluster1 = important[0];
    const cluster2 = important.length > 1 ? important[1] : null;

    const hybrid = isHybridChart(cluster1, cluster2);
    const tech = cluster1.Mixed;

    let name;
    if (cluster1.SpecificTypes.length > 0 && cluster1.SpecificTypes[0][1] > 0.05) {
        name = cluster1.SpecificTypes[0][0];
    } else if (
        cluster1.SpecificTypes.length >= 2
        && cluster1.SpecificTypes[0][0] === "Jumpstream"
        && cluster1.SpecificTypes[1][0] === "Handstream"
    ) {
        const a1 = cluster1.SpecificTypes[0][1];
        const a2 = cluster1.SpecificTypes[1][1];
        name = (a2 / a1) > PATTERNS_CONFIG.CATEGORY_JS_HS_SECONDARY_RATIO
            ? "Jumpstream/Handstream"
            : cluster1.Pattern;
    } else {
        name = cluster1.Pattern;
    }

    return `${name}${hybrid ? " Hybrid" : ""}${tech ? " Tech" : ""}`;
}