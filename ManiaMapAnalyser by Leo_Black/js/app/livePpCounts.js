// Pure judgment-count resolution for livePp.js (DOM-free — Node smoke testable,
// same pattern as resultCache.js / modData.js).
export const ZERO_COUNTS = Object.freeze({
    perfect: 0, great: 0, good: 0, ok: 0, meh: 0, miss: 0,
});

// tosu play.hits → formula counts (geki→perfect/305, 300→great, katu→good/200,
// 100→ok, 50→meh, 0→miss). Missing/NaN fields default to 0.
export function extractCounts(hits) {
    return {
        perfect: Number(hits.geki) || 0,
        great: Number(hits["300"]) || 0,
        good: Number(hits.katu) || 0,
        ok: Number(hits["100"]) || 0,
        meh: Number(hits["50"]) || 0,
        miss: Number(hits["0"]) || 0,
    };
}

export function totalCount(counts) {
    return counts.perfect + counts.great + counts.good + counts.ok + counts.meh + counts.miss;
}

// Resolve the counts to render for one api_v2 message. Retention is
// resultScreen-only: its packets sometimes omit hits or carry empty counts
// (lazer behavior differs), so keep the last play's counts then
// (retainOnEmpty = true). Elsewhere an all-zero extraction is a legitimate
// fresh play / new map — return the zeros. First frame (lastCounts === null)
// falls back to ZERO_COUNTS — PP 0.000, no NaN.
export function resolveCounts(hits, lastCounts, { retainOnEmpty } = {}) {
    let nextCounts = hits ? extractCounts(hits) : (lastCounts || ZERO_COUNTS);
    if (totalCount(nextCounts) === 0 && retainOnEmpty && lastCounts) nextCounts = lastCounts;
    return nextCounts;
}
