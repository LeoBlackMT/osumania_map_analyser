// Browser-only per-message live PP updater (Task 14).
//
// Hooked into socketHandlers.js' setupSocketListener after updateSongTimeState
// so it runs on EVERY api_v2 message; the early-return guards keep per-message
// cost negligible (no throttling/RAF — CSS transitions smooth the bars).
//
// Judgement counts live module-level (never in state): `lastCounts` is retained
// only while the result screen shows (retainOnEmpty), so play-exit keeps the
// final counts on screen; a fresh play's all-zero counts replace it normally.
// `lastLive` tracks the last live/max flag and `lastMetricsRef` the last
// ppMetrics reference so the cheap guard can skip renders while neither the
// counts, the state mode, nor the map changed (pauses freeze naturally because
// the counts don't move; a new map forces one re-render via a fresh ref).
import { contentBarShows, state } from "./appContext.js";
import {
    isPlayStateName,
    isResultScreenStateName,
} from "./modeLogic.js";
import { renderReworkPpBars, showReworkPpValue } from "./display.js";
import { calculateReworkPp } from "../rework/reworkPerformance.js";
import { resolveCounts } from "./livePpCounts.js";

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

// tosu play.hits → formula counts lives in livePpCounts.js (DOM-free, Node-tested).

let lastCounts = null;
let lastLive = null;
// Last ppMetrics object the guard saw — a new map gives ppMetrics a fresh
// reference, forcing one re-render so max-mode capsule/bars refresh.
let lastMetricsRef = null;
// Last computed PP (max PP in idle, live PP in play) for the srText="ReworkPP"
// left capsule. Written by renderMax/renderLive, cleared by resetLivePp.
let latestPpValue = null;

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
    latestPpValue = ppRes.pp;
    renderReworkPpBars({ mode: "max", rows: buildRows(ppRes) }, { inPlaceOnly: true });
}

function renderLive(counts) {
    const ppRes = runPp(counts);
    if (!ppRes) return;
    latestPpValue = ppRes.pp;
    renderReworkPpBars({ mode: "live", rows: buildRows(ppRes) }, { inPlaceOnly: true });
}

export function updateLivePp(data) {
    // Early-exit guards first: per-message cost stays minimal.
    if ((!contentBarShows("ReworkPP") && state.srText !== "ReworkPP") || !state.ppMetrics) {
        return;
    }

    const live = resolveLiveMode(state.clientStateName);

    // Resolve next counts from this message's hits. Only resultScreen keeps the
    // last play's counts on empty/partial hits (retainOnEmpty) — a fresh play's
    // all-zero counts are legitimate and replace them. First frame
    // (lastCounts === null) falls back to all-zero counts → PP 0.000, no NaN.
    const hits = data && data.play && data.play.hits;
    const retainOnEmpty = live && isResultScreenStateName(state.clientStateName);
    const nextCounts = resolveCounts(hits, lastCounts, { retainOnEmpty });

    // Cheap guard: counts, live flag and ppMetrics reference all unchanged →
    // nothing to redraw (pause keeps counts frozen, so this naturally suppresses
    // re-renders). A new map gives ppMetrics a fresh reference, forcing one
    // re-render so the max-mode capsule/bars don't linger on the old map.
    if (lastLive === live && countsEqual(lastCounts, nextCounts) && state.ppMetrics === lastMetricsRef) {
        return;
    }
    lastCounts = nextCounts;
    lastLive = live;
    lastMetricsRef = state.ppMetrics;

    if (live) {
        renderLive(nextCounts);
    } else {
        renderMax();
    }

    // srText="ReworkPP": keep the left capsule in sync with the live/max PP.
    if (state.srText === "ReworkPP" && latestPpValue != null) {
        showReworkPpValue(latestPpValue);
    }
}

export function getLatestPpValue() {
    return latestPpValue;
}

export function resetLivePp() {
    lastCounts = null;
    lastLive = null;
    lastMetricsRef = null;
    latestPpValue = null;
    if (state.ppMetrics) {
        renderMax();
    }
}
