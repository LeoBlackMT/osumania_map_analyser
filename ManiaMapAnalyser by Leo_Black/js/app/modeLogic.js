export function modeTagFromLnRatio(lnRatio) {
    if (!Number.isFinite(lnRatio)) {
        return "Mix";
    }
    if (lnRatio <= 0.15) {
        return "RC";
    }
    if (lnRatio >= 0.9) {
        return "LN";
    }
    return "Mix";
}

export function normalizeClientStateName(value) {
    return String(value || "")
        .trim()
        .toLowerCase()
        .replace(/[^a-z]/g, "");
}

export function isPlayStateName(normalizedStateName) {
    return normalizedStateName === "play"
        || normalizedStateName === "gameplay"
        || normalizedStateName === "playing";
}

export function isResultScreenStateName(normalizedStateName) {
    return normalizedStateName === "resultscreen";
}

export function resolveAutoDisplayProfile(modeTag) {
    if (modeTag === "RC") {
        return {
            contentBar: "Etterna",
            srText: "MSD",
        };
    }

    return {
        contentBar: "Pattern",
        srText: "ReworkSR",
    };
}
