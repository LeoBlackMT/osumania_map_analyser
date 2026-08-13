// Browser-only per-message live PP updater (Task 14).
//
// Hooked into socketHandlers.js' setupSocketListener after updateSongTimeState
// so it runs on EVERY api_v2 message; the early-return guards keep per-message
// cost negligible (no throttling/RAF — CSS transitions smooth the bars).
//
// Judgement counts live module-level (never in state): `lastCounts` retains the
// last play's counts so resultScreen / play-exit can keep showing them;
// `lastLive` tracks the last live/max flag so the cheap guard can skip renders
// while neither the counts nor the state mode changed (pauses freeze naturally
// because the counts don't move).
import { contentBarShows, state } from "./appContext.js";
import {
    isPlayStateName,
    isResultScreenStateName,
} from "./modeLogic.js";
import { renderReworkPpBars } from "./display.js";
import { calculateReworkPp } from "../rework/reworkPerformance.js";

// Row constants shared verbatim with analysis.js buildReworkPpDisplay (Task 13):
// pp(0,1200,false), proportion(0,1,false), acc(0.87,1.13,centered),
// variety(0.945,1.055,centered), length(0.9,1.1,centered).
const ROW_SPECS = [
    { key: "pp", label: "Max PP", min: 0, max: 1200, centered: false },
    { key: "proportion", label: "Proportion", min: 0, max: 1, centered: false },
    { key: "acc", label: "Acc Multiplier", min: 0.87, max: 1.13, centered: true },
    { key: "variety", label: "Variety Multiplier", min: 0.945, max: 1.055, centered: true },
    { key: "length", label: "Length Multiplier", min: 0.9, max: 1.1, centered: true },
];

const ZERO_COUNTS = Object.freeze({
    perfect: 0, great: 0, good: 0, ok: 0, meh: 0, miss: 0,
});

let lastCounts = null;
let lastLive = null;

// Pure state-machine mapping (exported for Node smoke tests — modeLogic.js is
// DOM-free): play/gameplay/playing/resultscreen → live, everything else → max.
export function resolveLiveMode(clientStateName) {
    return isPlayStateName(clientStateName) || isResultScreenStateName(clientStateName);
}

// Pure count-equality check (exported for the guard smoke test).
export function countsEqual(a, b) {
    if (!a || !b) return false;
    return a.perfect === b.perfect
        && a.great === b.great
        && a.good === b.good
        && a.ok === b.ok
        && a.meh === b.meh
        && a.miss === b.miss;
}

// tosu play.hits → formula counts (geki→perfect/305, 300→great, katu→good/200,
// 100→ok, 50→meh, 0→miss). Missing fields default to 0.
function extractCounts(hits) {
    return {
        perfect: Number(hits.geki) || 0,
        great: Number(hits["300"]) || 0,
        good: Number(hits.katu) || 0,
        ok: Number(hits["100"]) || 0,
        meh: Number(hits["50"]) || 0,
        miss: Number(hits["0"]) || 0,
    };
}

// 5-row assembly — mirrors analysis.js buildReworkPpDisplay verbatim, including
// the Math.max(0, ...) negative-value guard on every row.
function rowValue(ppRes, key) {
    switch (key) {
        case "pp": return ppRes.pp;
        case "proportion": return ppRes.proportion;
        case "acc": return ppRes.accMultiplier;
        case "variety": return ppRes.varietyMultiplier;
        case "length": return ppRes.lengthMultiplier;
        default: return 0;
    }
}

function buildRows(ppRes) {
    return ROW_SPECS.map((spec) => ({
        key: spec.key,
        label: spec.label,
        value: Math.max(0, rowValue(ppRes, spec.key)),
        min: spec.min,
        max: spec.max,
        centered: spec.centered,
    }));
}

function runPp(counts) {
    return calculateReworkPp({
        starRating: state.ppMetrics.star,
        variety: state.ppMetrics.variety,
        accScalar: state.ppMetrics.accScalar,
        totalNotes: state.ppMetrics.totalNotes,
        ...counts,
        noFail: state.modCodes.includes("NF"),
        easy: state.modCodes.includes("EZ"),
    });
}

function renderMax() {
    const ppRes = runPp({
        perfect: state.ppMetrics.totalNotes,
        great: 0, good: 0, ok: 0, meh: 0, miss: 0,
    });
    if (!ppRes) return; // invalid input → skip this round (soft, no error UI)
    renderReworkPpBars({ mode: "max", rows: buildRows(ppRes) });
}

function renderLive(counts) {
    const ppRes = runPp(counts);
    if (!ppRes) return;
    renderReworkPpBars({ mode: "live", rows: buildRows(ppRes) });
}

export function updateLivePp(data) {
    // Early-exit guards first: per-message cost stays minimal.
    if (!contentBarShows("ReworkPP") || !state.ppMetrics) {
        return;
    }

    const live = resolveLiveMode(state.clientStateName);

    // Retain the last play's counts when this message has no hits payload
    // (play-end / resultScreen); fall back to zeros before any play exists
    // (play first-frame all-zero counts → v2Acc 0 → PP 0.000, no NaN).
    const hits = data && data.play && data.play.hits;
    const nextCounts = hits ? extractCounts(hits) : (lastCounts || ZERO_COUNTS);

    // Cheap guard: neither counts nor live flag changed → nothing to redraw
    // (pause keeps counts frozen, so this naturally suppresses re-renders).
    if (lastLive === live && countsEqual(lastCounts, nextCounts)) {
        return;
    }
    lastCounts = nextCounts;
    lastLive = live;

    if (live) {
        renderLive(nextCounts);
    } else {
        renderMax();
    }
}

export function resetLivePp() {
    lastCounts = null;
    lastLive = null;
    if (state.ppMetrics) {
        renderMax();
    }
}
