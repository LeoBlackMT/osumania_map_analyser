// Shared threshold logic (js/patterns/config.js) re-exported for the
// browser side; analysis.js imports it from here.
export { modeTagFromLnRatio } from "../../patterns/config.js";

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

/** 暂停态不是“退出游玩”；LaneAcc 必须保留本局累加器，恢复后继续。 */
export function isPauseStateName(normalizedStateName) {
    return String(normalizedStateName || "").includes("pause");
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
