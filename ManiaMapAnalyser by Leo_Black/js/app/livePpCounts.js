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

// Resolve the counts to render for one api_v2 message. resultScreen / play-end
// packets sometimes omit hits or carry empty counts (lazer behavior differs) —
// judgment counts only grow during a play, so an all-zero extraction while a
// previous play's counts exist means data loss, not a real zero-judgement
// score: keep the last counts so the result screen renders correctly. First
// frame (lastCounts === null) keeps the zeros — PP 0.000 is correct there.
export function resolveCounts(hits, lastCounts) {
    let nextCounts = hits ? extractCounts(hits) : (lastCounts || ZERO_COUNTS);
    if (totalCount(nextCounts) === 0 && lastCounts) nextCounts = lastCounts;
    return nextCounts;
}
