// Browser-only per-message live PP updater (Task 14).
//
// Hooked into socketHandlers.js' setupSocketListener after updateSongTimeState
// so it runs on EVERY api_v2 message; the early-return guards keep per-message
// cost negligible (no throttling/RAF — CSS transitions smooth the bars).
//
// Judgement counts live module-level (never in state): the source is
// state-aware — play states read live `play.hits`, resultScreen reads the
// authoritative `resultsScreen.hits` (tosu's `play.hits` zeroes out on the
// results screen because the Player object is replaced by SoloResultsScreen).
// `lastCounts` is retained only while the result screen shows (retainOnEmpty),
// so a missing/empty resultsScreen.hits keeps the final counts on screen; a
// fresh play's all-zero counts replace it normally.
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

// tosu hits → formula counts lives in livePpCounts.js (DOM-free, Node-tested);
// both play.hits and resultsScreen.hits share the same key structure.

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

// pp 行的 max 在 live 模式由当前图的 Max PP 决定（满格 = Max PP），否则回退
// 1200。liveMaxPp 无效（null / 非有限 / <=0）时回退 spec.max。导出供冒烟断言。
export function buildRows(ppRes, liveMaxPp) {
    return ROW_SPECS.map((spec) => {
        const max = spec.key === "pp"
            && liveMaxPp != null
            && Number.isFinite(liveMaxPp)
            && liveMaxPp > 0
            ? liveMaxPp
            : spec.max;
        return {
            key: spec.key,
            label: spec.label,
            value: Math.max(0, rowValue(ppRes, spec.key)),
            min: spec.min,
            max,
            centered: spec.centered,
        };
    });
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
    // Bar scale for the pp row is the current map's Max PP: full bar = max PP,
    // so Live PP visibly approaches full as the play nears perfect. Fall back
    // to the fixed 1200 spec max when the max run yields nothing.
    const maxRes = runPp({
        perfect: state.ppMetrics.totalNotes,
        great: 0, good: 0, ok: 0, meh: 0, miss: 0,
    });
    const ppRes = runPp(counts);
    if (!ppRes) return;
    latestPpValue = ppRes.pp;
    const liveMax = maxRes && Number.isFinite(maxRes.pp) ? maxRes.pp : ROW_SPECS[0].max;
    renderReworkPpBars({ mode: "live", rows: buildRows(ppRes, liveMax) }, { inPlaceOnly: true });
}

export function updateLivePp(data) {
    // Early-exit guards first: per-message cost stays minimal.
    if ((!contentBarShows("ReworkPP") && state.srText !== "ReworkPP") || !state.ppMetrics) {
        return;
    }

    const live = resolveLiveMode(state.clientStateName);

    // Resolve next counts from this message's hits, source is state-aware:
    // play states read live play.hits (a fresh play's all-zero counts are
    // legitimate), resultScreen reads the authoritative resultsScreen.hits —
    // tosu zeroes play.hits on the results screen (Player object replaced by
    // SoloResultsScreen), so play.hits there would wipe the final counts.
    // Only resultScreen keeps the last play's counts on empty/missing
    // resultsScreen.hits (retainOnEmpty). First frame (lastCounts === null)
    // falls back to all-zero counts → PP 0.000, no NaN.
    const isResult = isResultScreenStateName(state.clientStateName);
    const hits = isResult
        ? (data && data.resultsScreen && data.resultsScreen.hits)
        : (data && data.play && data.play.hits);
    const nextCounts = resolveCounts(hits, lastCounts, { retainOnEmpty: isResult });

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
