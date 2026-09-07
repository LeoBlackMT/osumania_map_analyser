// Surface Timing Compare debug panel (browser-only, js/debug/).
//
// Renders the rosu-pp surface-map-timing pipeline (S2/S3 modules) against the
// live tosu api_v2 payload: mode dispatch (Max PP / Score PP / Live), hit
// judgement counts, map timing factor, score timing adjustment, and the
// combined xxy+timing PP against the Rework PP reference.
//
// Mirrors estimatorDebugPanel.js conventions: root.innerHTML full re-render,
// `.estimator-debug-*` + `.kv` classes from debug.html, performance.now()
// segment timing, runSeq invalidation for the async beatmap fetch.
//
// The pure pipeline is split into module-level DOM-free exports so the Node
// smoke (temp/surface-timing-smoke.mjs) can assert it without a browser:
//   - resolveModeAndCounts(data, lastCounts, unitsTotal)  mode/hits dispatch
//   - runSurfacePipeline({...})                            full calculation
// createSurfaceTimingPanel itself is browser-only (fetch / innerHTML).

import { APP_CONFIG } from "../../config.js";
import { getModData } from "../app/modData.js";
import {
    isPlayStateName,
    isResultScreenStateName,
    normalizeClientStateName,
} from "../app/modeLogic.js";
import { extractCounts } from "../app/livePpCounts.js";
import { runSunnyEstimatorFromText } from "../estimator/sunnyEstimator.js";
import { OsuFileParser } from "../parser/osuFileParser.js";
import { calculateReworkPp } from "../rework/reworkPerformance.js";
import {
    buildHitWindows,
    buildMapWindows,
    JUDGEMENT_NAMES,
} from "./surfaceTimingWindows.js";
import {
    buildJudgementUnits,
    buildLnDurationBuckets,
    computeMapTimingFactor,
    computeScoreAdjustment,
    computeSurfacePp,
    expectedCountsAtCoreSigma,
    expectedCountsCustomAccuracy,
    SURFACE_TIMING_MODEL,
} from "./surfaceTimingCurve.js";
import { calculateOfficialPp, calculateOfficialStar } from "./surfaceTimingOfficial.js";

const SORTED_KNOWN_MOD_CODES = [...APP_CONFIG.mods.knownCodes].sort((a, b) => b.length - a.length);
const MOD_BIT_FLAG_ENTRIES = Object.entries(APP_CONFIG.mods.bitFlags);

function finiteNumber(value) {
    const number = Number(value);
    return Number.isFinite(number) ? number : null;
}

function formatNumber(value, digits = 2) {
    const number = finiteNumber(value);
    return number == null ? "-" : number.toFixed(digits);
}

function normalizeText(value) {
    return String(value ?? "").trim();
}

function normalizePathText(value) {
    return normalizeText(value).replace(/\\/g, "/").replace(/\/+/g, "/").toLowerCase();
}

function getDebugModData(data) {
    return getModData(data, {
        sortedKnownModCodes: SORTED_KNOWN_MOD_CODES,
        modBitFlagEntries: MOD_BIT_FLAG_ENTRIES,
        fallbackClient: data?.client || "",
        preferPlayMods: false,
    });
}

// Map-only identity (no mods suffix) — drives the "beatmap changed → refetch"
// gate. Mods-only changes reuse the cached osu text and recompute SR instead.
function buildMapIdentity(data) {
    const beatmap = data?.beatmap || {};
    const id = finiteNumber(beatmap?.id);
    const setId = finiteNumber(beatmap?.set || beatmap?.setId || beatmap?.beatmapSetId);
    const hash = normalizeText(beatmap?.md5 || beatmap?.checksum).toLowerCase();
    const path = normalizePathText(data?.files?.beatmap || data?.directPath?.beatmapFile);
    const title = [
        beatmap?.artist,
        beatmap?.title,
        beatmap?.version,
        beatmap?.mapper,
    ].map(normalizeText).join("::").toLowerCase();

    const parts = [];
    if (id != null && id > 0) parts.push(`id:${Math.trunc(id)}`);
    if (hash) parts.push(`hash:${hash}`);
    if (path) parts.push(`path:${path}`);
    if (parts.length === 0 && title.replace(/[:]/g, "")) parts.push(`meta:${title}`);
    if (setId != null && setId > 0) parts.push(`set:${Math.trunc(setId)}`);
    return parts.join("|");
}

// Full identity incl. mods (same shape as estimatorDebugPanel.buildBeatmapIdentity).
function buildBeatmapIdentity(data, modSignature) {
    const mapKey = buildMapIdentity(data);
    return mapKey ? `${mapKey}|mods:${modSignature || "none"}` : "";
}

function countsToArray(counts) {
    return [
        Number(counts?.perfect) || 0,
        Number(counts?.great) || 0,
        Number(counts?.good) || 0,
        Number(counts?.ok) || 0,
        Number(counts?.meh) || 0,
        Number(counts?.miss) || 0,
    ];
}

function countsTotal(counts) {
    return counts.reduce((sum, c) => sum + c, 0);
}

function schemeLabel(classic, isConvert) {
    if (!classic) return "lazer";
    return isConvert ? "classic convert" : "classic non-convert";
}

// Key count from the parsed chart's column range (max column + 1), falling back
// to null when no parsed columns survive (should not happen in practice).
function columnCountFromParsed(parsed) {
    if (!parsed || !Array.isArray(parsed.columns) || parsed.columns.length === 0) return null;
    return Math.max(...parsed.columns) + 1;
}

/**
 * Resolve the display mode and judgement counts from an api_v2 payload.
 * Pure DOM-free helper (Node smoke tested).
 *
 * Mode dispatch: resultscreen → "Score PP", play/gameplay/playing → "Live",
 * anything else → "Max PP" (SS assumption: every unit judged perfect).
 * Counts come from resultsScreen.hits / play.hits (extractCounts semantics);
 * a resultscreen without hits retains the previous counts (retainOnEmpty).
 *
 * @param {object} data api_v2 payload
 * @param {number[]|null} [lastCounts] previous counts in JUDGEMENT_NAMES order
 * @param {number} [unitsTotal] judgement units for the SS override (0 → all-zero SS row)
 * @returns {{mode: string, source: string, isResult: boolean, isPlay: boolean,
 *            hits: object|null, counts: number[], hitTotal: number}}
 */
export function resolveModeAndCounts(data, lastCounts = null, unitsTotal = 0) {
    const stateName = normalizeClientStateName(data?.state?.name);
    const isResult = isResultScreenStateName(stateName);
    const isPlay = isPlayStateName(stateName);

    let mode = "Max PP";
    let source = "SS assumption";
    if (isResult) {
        mode = "Score PP";
        source = "resultsScreen.hits";
    } else if (isPlay) {
        mode = "Live";
        source = "play.hits";
    }

    const hits = isResult
        ? (data && data.resultsScreen && data.resultsScreen.hits)
        : isPlay ? (data && data.play && data.play.hits) : null;

    let counts;
    if (hits) {
        counts = countsToArray(extractCounts(hits));
        if (isResult && countsTotal(counts) === 0 && lastCounts) {
            counts = lastCounts.slice();
        }
    } else if (isResult && lastCounts) {
        counts = lastCounts.slice();
    } else {
        counts = [0, 0, 0, 0, 0, 0];
    }

    // Max PP: assume perfect on every judgement unit.
    if (!isPlay && !isResult) {
        counts = [unitsTotal, 0, 0, 0, 0, 0];
    }

    return { mode, source, isResult, isPlay, hits, counts, hitTotal: countsTotal(counts) };
}

/**
 * The full surface timing calculation for one snapshot. Pure DOM-free (Node
 * smoke tested). Mirrors handleSocketPayload's orchestration with
 * performance.now() segment timing; callers supply parsed/derived inputs
 * (no fetching, no parsing, no DOM).
 *
 * @param {{
 *   od: number, isConvert?: boolean, classic?: boolean,
 *   hr?: boolean, ez?: boolean, clockRate?: number,
 *   stars: number, variety: number, accScalar: number,
 *   nObjects: number, nLongNotes?: number, lnBuckets: number[],
 *   counts: number[], hitTotal: number, unitsTotal: number,
 *   totalNotes?: number, noFail?: boolean, easy?: boolean,
 *   model: object,
 * }} args
 * @returns {{
 *   windows: object, mapWindows: object, units: object[],
 *   expectedArr: number[], expectedAcc: number, scoreAccuracy: number,
 *   mapFactorRes: object, scoreAdjRes: object, ppRes: object,
 *   rework: object|null, deltaPct: number|null,
 *   timings: {windows: number, units: number, expected: number,
 *             mapFactor: number, scoreAdj: number, total: number},
 * }}
 */
export function runSurfacePipeline({
    od,
    isConvert = false,
    classic = true,
    hr = false,
    ez = false,
    clockRate = 1,
    stars,
    variety,
    accScalar,
    nObjects,
    nLongNotes = 0,
    lnBuckets,
    counts,
    hitTotal,
    unitsTotal,
    totalNotes = unitsTotal,
    noFail = false,
    easy = false,
    model,
}) {
    const t0 = performance.now();
    const windows = buildHitWindows({ od, isConvert, classic, hr, ez, clockRate });
    const mapWindows = buildMapWindows({ od, isConvert, classic, clockRate });
    const t1 = performance.now();

    const units = buildJudgementUnits({ stars, unitsTotal, nObjects, nLongNotes, lnBuckets, classic, model });
    const t2 = performance.now();

    const expectedArr = expectedCountsAtCoreSigma(units, windows, model, SURFACE_TIMING_MODEL.TIMING_BASELINE_SIGMA);
    const expectedAcc = expectedCountsCustomAccuracy(expectedArr);
    const t3 = performance.now();

    const mapFactorRes = computeMapTimingFactor({ expectedAcc, nLongNotes, nObjects, lnBuckets });
    const t4 = performance.now();

    const scoreAdjRes = computeScoreAdjustment({ playerCounts: counts, units, windows, hitTotal, model });
    const t5 = performance.now();

    const scoreAccuracy = expectedCountsCustomAccuracy(counts);
    const ppRes = computeSurfacePp({
        stars,
        variety,
        accScalar,
        nObjects,
        scoreAccuracy,
        mapFactor: mapFactorRes.factor,
        scoreAdj: scoreAdjRes.multiplier,
        noFail,
    });
    const now = performance.now();

    const rework = calculateReworkPp({
        starRating: stars,
        variety,
        accScalar,
        totalNotes,
        perfect: counts[0],
        great: counts[1],
        good: counts[2],
        ok: counts[3],
        meh: counts[4],
        miss: counts[5],
        noFail,
        easy,
    });
    const deltaPct = rework && Number.isFinite(ppRes.pp) && rework.pp > 0
        ? (ppRes.pp - rework.pp) / rework.pp * 100
        : null;

    return {
        windows,
        mapWindows,
        units,
        expectedArr,
        expectedAcc,
        scoreAccuracy,
        mapFactorRes,
        scoreAdjRes,
        ppRes,
        rework,
        deltaPct,
        timings: {
            windows: t1 - t0,
            units: t2 - t1,
            expected: t3 - t2,
            mapFactor: t4 - t3,
            scoreAdj: t5 - t4,
            total: now - t0,
        },
    };
}

function bandValuesText(windows) {
    return JUDGEMENT_NAMES.map((name) => `${name}: ${formatNumber(windows[name], 1)}ms`).join(" · ");
}

function buildPanelHtml(result) {
    const {
        mapInfo, mode, source, counts, unitsTotal, hitTotal, od, odAvailable,
        classic, isConvert, hr, ez, clockRate, windows, mapWindows,
        expectedAcc, mapFactorRes, scoreAdjRes, ppRes, rework, deltaPct, official, timings,
    } = result;

    const diffMultiplier = hr ? 1.4 : (ez ? 1 / 1.4 : 1);
    const unitLabel = classic ? "V1" : "V2";
    const modsText = [
        `difficulty ×${formatNumber(diffMultiplier, 2)}${hr ? " (HR)" : ""}${ez ? " (EZ)" : ""}`,
        `clock_rate ${formatNumber(clockRate, 3)}`,
    ].join(" · ");
    const officialStarText = official && official.star != null ? formatNumber(official.star, 2) : "-";

    const countNames = ["perfect", "great", "good", "ok", "meh", "miss"];
    const countText = counts.map((c, i) => `${countNames[i]} ${c}`).join(" · ");

    const ppCell = (label, value) => `
        <div class="surface-pp-cell">
            <div class="surface-pp-value ${value == null ? "surface-dim" : ""}">${formatNumber(value == null ? null : value, 2)}</div>
            <div class="surface-pp-label">${label}</div>
        </div>`;

    return `
        <section class="estimator-debug-panel">
            <div class="estimator-debug-header">
                <div>
                    <h2>Surface Timing Compare</h2>
                    <div class="estimator-debug-meta">
                        <span class="badge">${mode}</span>
                        <span class="badge">${source}</span>
                        <span class="badge">v${SURFACE_TIMING_MODEL.VERSION}</span>
                    </div>
                </div>
            </div>

            <div class="surface-map-info">
                <strong>${escapeHtml(mapInfo.artist)} - ${escapeHtml(mapInfo.title)} <span class="surface-dim">[${escapeHtml(mapInfo.version)}]</span></strong>
                <span class="estimator-debug-meta">by ${escapeHtml(mapInfo.mapper)}</span>
                <span class="badge">${mapInfo.keyCount != null ? `${mapInfo.keyCount}K` : "-K"}</span>
                <span class="badge">od ${formatNumber(od, 1)}${odAvailable ? "" : "?"}</span>
                <span class="badge">Sunny ${formatNumber(mapInfo.sunnyStar, 2)}★</span>
                <span class="badge">Official ${officialStarText}★</span>
                <span class="badge">${classic ? "classic" : "lazer"} · ${unitLabel}</span>
            </div>

            <div class="surface-hero">
                <div class="surface-pp-cell surface-hero-pp">
                    <div class="surface-pp-value">${formatNumber(ppRes.pp, 2)}</div>
                    <div class="surface-pp-label">${mode === "Live" ? "Live PP" : mode} · Surface</div>
                </div>
                ${ppCell("Official PP", official && official.pp)}
                ${ppCell("Rework PP", rework && rework.pp)}
                ${ppCell("Δ vs Rework", deltaPct == null ? null : deltaPct)}
            </div>
            ${deltaPct == null ? "" : `<div class="surface-hero-delta">${deltaPct >= 0 ? "+" : ""}${formatNumber(deltaPct, 2)}%</div>`}

            <div class="surface-group">
                <div class="surface-group-title">Map timing factor</div>
                <div class="kv">
                    <dt>windows (${schemeLabel(classic, isConvert)})</dt>
                    <dd>${bandValuesText(windows)}</dd>
                    <dt>map_windows</dt>
                    <dd>${bandValuesText(mapWindows)}</dd>
                    <dt>mods</dt>
                    <dd>${modsText}</dd>
                    <dt>expected_acc</dt>
                    <dd>${formatNumber(expectedAcc * 100, 3)}%</dd>
                    <dt>window_factor</dt>
                    <dd>${formatNumber(mapFactorRes.windowFactor, 4)}</dd>
                    <dt>ln_factor</dt>
                    <dd>${formatNumber(mapFactorRes.lnFactor, 4)}</dd>
                    <dt>map_factor</dt>
                    <dd><strong>${formatNumber(mapFactorRes.factor, 4)}</strong></dd>
                </div>
            </div>

            <div class="surface-group">
                <div class="surface-group-title">Score timing adjustment</div>
                <div class="kv">
                    <dt>counts (${source})</dt>
                    <dd>${countText} <span class="estimator-debug-meta">(hitTotal ${hitTotal})</span></dd>
                    <dt>units_total</dt>
                    <dd>${unitsTotal} (${unitLabel})</dd>
                    <dt>player_loss</dt>
                    <dd>${formatNumber(scoreAdjRes.playerLoss, 4)}</dd>
                    <dt>expected_loss</dt>
                    <dd>${formatNumber(scoreAdjRes.expectedLoss, 4)}</dd>
                    <dt>loss_diff</dt>
                    <dd>${formatNumber(scoreAdjRes.lossDiff, 4)}</dd>
                    <dt>score multiplier</dt>
                    <dd><strong>${formatNumber(scoreAdjRes.multiplier, 4)}</strong></dd>
                    <dt>timing_multiplier <span class="estimator-debug-meta">(map+score)</span></dt>
                    <dd><strong>${formatNumber(ppRes.timingMultiplier, 4)}</strong></dd>
                </div>
            </div>

            <div class="surface-group">
                <div class="surface-group-title">PP decomposition</div>
                <div class="kv">
                    <dt>xxy_pp_pattern</dt>
                    <dd>${formatNumber(ppRes.xxyPpPattern, 2)}</dd>
                    <dt>xxy_pp_accuracy</dt>
                    <dd>${formatNumber(ppRes.xxyPpAccuracy, 2)}</dd>
                    <dt>xxy_pp</dt>
                    <dd>${formatNumber(ppRes.xxyPp, 2)}</dd>
                    <dt>pp_with_timing</dt>
                    <dd>${formatNumber(ppRes.ppWithTiming, 2)}</dd>
                    <dt>pp_timing</dt>
                    <dd>${formatNumber(ppRes.ppTiming, 2)}</dd>
                </div>
            </div>

            <div class="estimator-debug-note surface-footnote">
                timings — sunny ${formatNumber(timings.sunny ?? 0, 1)}ms · windows ${formatNumber(timings.windows, 1)}ms · units ${formatNumber(timings.units, 1)}ms · expected ${formatNumber(timings.expected, 1)}ms · mapFactor ${formatNumber(timings.mapFactor, 1)}ms · scoreAdj ${formatNumber(timings.scoreAdj, 1)}ms · total ${formatNumber(timings.total, 1)}ms
            </div>
        </section>
    `;
}

// Minimal HTML escape for user-supplied beatmap metadata.
function escapeHtml(value) {
    return String(value ?? "")
        .replace(/&/g, "&amp;")
        .replace(/</g, "&lt;")
        .replace(/>/g, "&gt;")
        .replace(/"/g, "&quot;")
        .replace(/'/g, "&#39;");
}

/**
 * Surface Timing Compare debug panel.
 * @param {{root: HTMLElement, socketHost?: string}} options
 * @returns {{handleSocketPayload: (data: object) => Promise<void>, setCollapsed: (collapsed: boolean) => void}}
 */
export function createSurfaceTimingPanel({
    root,
    socketHost = "127.0.0.1:24050",
} = {}) {
    if (!root) {
        throw new Error("Surface timing panel root is required");
    }

    let lastIdentity = "";   // map-only key (refetch gate)
    let lastOsuText = "";    // cached osu text
    let lastModsKey = "";    // modSignature (SR recompute gate)
    let lastCounts = null;   // last judgement counts (resultscreen retain)
    let lastParse = null;    // cached OsuFileParser getParsedData (same map)
    let lastSunny = null;    // cached Sunny result (same map+mods)
    let runSeq = 0;          // async invalidation
    let collapsed = false;   // second gate (debug.html layer gates first)
    let lastRender = null;   // {error: string} or full result object
    let lastFetchFailed = false; // transient fetch failure (tosu not ready yet)
    let lastFetchFailAt = 0;  // ms timestamp — throttle retry storms while tosu warms up
    let lastCountsKey = null; // live/score hits fingerprint (recompute gate)
    const FETCH_RETRY_MS = 1000;

    async function fetchCurrentBeatmap() {
        const response = await fetch(`http://${socketHost}/files/beatmap/file`, {
            method: "GET",
            cache: "no-store",
        });
        if (!response.ok) {
            throw new Error(`beatmap fetch failed: HTTP ${response.status}`);
        }
        const text = await response.text();
        if (!text.trim()) {
            throw new Error("beatmap fetch returned empty content");
        }
        return text;
    }

    function renderError(message) {
        lastRender = { error: message };
        render();
    }

    function render() {
        const result = lastRender;
        if (result == null) {
            root.innerHTML = lastFetchFailed
                ? "<div class=\"estimator-debug-note\">Waiting for beatmap file… (tosu not ready, retrying on next update)</div>"
                : "<div class=\"estimator-debug-empty\">Waiting for map data.</div>";
            return;
        }
        if (result.error) {
            root.innerHTML = `<div class="estimator-debug-error">${result.error}</div>`;
            return;
        }
        root.innerHTML = buildPanelHtml(result);
    }

    async function handleSocketPayload(data) {
        if (collapsed) return;
        const tStart = performance.now();

        // Invalidate any in-flight async from an earlier message FIRST, then
        // capture the new sequence (estimatorDebugPanel runAll pattern). Every
        // async continuation checks `seq !== runSeq` and bails if a newer
        // message arrived meanwhile.
        runSeq += 1;
        const seq = runSeq;

        const modData = getDebugModData(data);
        const mapKey = buildMapIdentity(data);
        if (!mapKey) return; // keep "Waiting for map data." until a real beatmap

        const mapChanged = mapKey !== lastIdentity;
        const modsChanged = modData.modSignature !== lastModsKey;
        let needRecomputeSR = mapChanged || modsChanged;

        // --- Recompute gate: skip the whole pipeline when nothing that feeds
        // the display changed. ws messages arrive continuously (~10/s), and
        // without this every message re-runs the pipeline + full re-render
        // (all timings show ~0.0ms, panel churns for nothing). Two cases:
        //   1. map+mods unchanged and a successful result already rendered →
        //      nothing on screen can change (Max PP is a fixed SS snapshot).
        //   2. live/score: same map+mods but identical hits counts → also a
        //      no-op. Only a change in counts (or map/mods) triggers rework.
        const stateName = normalizeClientStateName(data?.state?.name);
        const isPlayState = isPlayStateName(stateName);
        const isResultState = isResultScreenStateName(stateName);
        const hitsNow = isResultState
            ? (data && data.resultsScreen && data.resultsScreen.hits)
            : isPlayState ? (data && data.play && data.play.hits) : null;
        const countsNow = hitsNow ? extractCounts(hitsNow) : null;
        const countsKey = countsNow
            ? `${countsNow.perfect},${countsNow.great},${countsNow.good},${countsNow.ok},${countsNow.meh},${countsNow.miss}`
            : null;
        const modeStateKey = isResultState ? "score" : isPlayState ? "live" : "max";

        if (!mapChanged && !modsChanged && lastParse) {
            // Same map+mods with a prior successful parse. For max mode the
            // display is a fixed SS snapshot — nothing can differ. For live/
            // score, only a hits-count change is a real update.
            if (modeStateKey === "max") {
                return;
            }
            if (countsKey !== null && countsKey === lastCountsKey) {
                return;
            }
            lastCountsKey = countsKey;
        }

        // Beatmap changed (or no cached text): fetch + parse. Mods-only changes
        // reuse the cached text/parse — parse results are mod-independent
        // (modIN/HO conversion happens inside sunnyAlgorithm on a clone).
        if (mapChanged || !lastOsuText) {
            // Throttle retries while tosu/osu are warming up: ws messages arrive
            // every ~100ms, so an immediate refetch on every message would spam
            // the server during the startup window. Fetch again at most once
            // per FETCH_RETRY_MS after a failure (or immediately after success).
            const now = Date.now();
            if (lastOsuText === "" && lastFetchFailed && now - lastFetchFailAt < FETCH_RETRY_MS) {
                return;
            }
            lastOsuText = "";
            lastParse = null;
            lastSunny = null;
            try {
                const osuText = await fetchCurrentBeatmap();
                if (seq !== runSeq) return;
                lastOsuText = osuText;
                const parser = new OsuFileParser(osuText);
                parser.process();
                const parsed = parser.getParsedData();
                if (seq !== runSeq) return;
                lastParse = parsed;
                lastIdentity = mapKey;
                if (lastFetchFailed) {
                    lastFetchFailed = false;
                    lastFetchFailAt = 0;
                    lastRender = null;
                    render();
                }
            } catch (error) {
                // Transient failure (e.g. tosu/osu just starting, menu beatmap
                // not ready yet → HTTP 404): do NOT brick the panel into a
                // permanent error. Mark the retry state and wait for the next
                // api_v2 message with a beatmap — the empty lastIdentity /
                // lastOsuText below makes any later payload retry the fetch
                // (throttled by FETCH_RETRY_MS).
                lastFetchFailed = true;
                lastFetchFailAt = now;
                lastRender = null;
                render();
                return;
            }
        }

        if (lastParse.status === "Fail" || lastParse.status === "NotMania") {
            renderError(lastParse.status === "NotMania"
                ? "beatmap mode is not mania"
                : "beatmap parse failed");
            return;
        }

        // Sunny SR — cached across identical map+mods; rerun when either changed.
        let sunny = lastSunny;
        let sunnyMs = 0;
        if (needRecomputeSR) {
            const tSunny = performance.now();
            try {
                sunny = runSunnyEstimatorFromText(lastOsuText, {
                    speedRate: modData.speedRate,
                    odFlag: modData.odFlag,
                    cvtFlag: modData.cvtFlag,
                    withPpMetrics: true,
                    classicMod: modData.classic,
                });
            } catch (error) {
                renderError(error?.message || "Sunny estimator failed");
                return;
            }
            sunnyMs = performance.now() - tSunny;
            if (seq !== runSeq) return;
            lastSunny = sunny;
        }

        const ppMetrics = sunny?.ppMetrics;
        const stars = finiteNumber(sunny?.star);
        if (stars == null || stars <= 0 || !ppMetrics
            || !Number.isFinite(finiteNumber(ppMetrics.variety))
            || !Number.isFinite(finiteNumber(ppMetrics.accScalar))
            || !(finiteNumber(ppMetrics.totalNotes) > 0)) {
            renderError("Sunny result unavailable (star/ppMetrics invalid)");
            return;
        }

        // LN histogram from the raw parsed chart (map-time ms, not ÷clockRate).
        const nObjects = lastParse.noteTypes.length;
        const longNoteDurations = [];
        for (let i = 0; i < nObjects; i += 1) {
            if ((lastParse.noteTypes[i] & 128) !== 0) {
                longNoteDurations.push(lastParse.noteEnds[i] - lastParse.noteStarts[i]);
            }
        }
        const nLongNotes = longNoteDurations.length;
        const lnBuckets = buildLnDurationBuckets(longNoteDurations);

        const classic = modData.classic;
        const unitsTotal = classic ? nObjects : nObjects + nLongNotes;

        const resolved = resolveModeAndCounts(data, lastCounts, unitsTotal);
        const { mode, source, counts, hitTotal, hits, isResult } = resolved;
        // Retain counts on the results screen; track live hits otherwise.
        // Fresh-play all-zero counts are legitimate — keep them.
        if (isResult || hits) lastCounts = counts;

        // Raw map OD from the parser (getParsedData exposes `.od`); the HR/EZ
        // difficulty multiplier is applied inside buildHitWindows, so no
        // pre-conversion here. Fall back to 8 when the map lacks OverallDifficulty.
        const rawOd = finiteNumber(lastParse.od);
        const odAvailable = rawOd != null && rawOd >= 0;
        const od = odAvailable ? rawOd : 8;

        const modCodes = modData.modCodes;
        const pipeline = runSurfacePipeline({
            od,
            isConvert: Boolean(modData.cvtFlag),
            classic,
            hr: modCodes.includes("HR"),
            ez: modCodes.includes("EZ"),
            clockRate: modData.speedRate,
            stars,
            variety: ppMetrics.variety,
            accScalar: ppMetrics.accScalar,
            nObjects,
            nLongNotes,
            lnBuckets,
            counts,
            hitTotal,
            unitsTotal,
            totalNotes: ppMetrics.totalNotes,
            noFail: modCodes.includes("NF"),
            easy: modCodes.includes("EZ"),
            model: SURFACE_TIMING_MODEL.ERROR_MODEL,
        });

        // Official osu!mania reference row (genirx official port of osu-master).
        const officialStar = calculateOfficialStar({
            columns: lastParse.columns ?? [],
            noteStarts: lastParse.noteStarts ?? [],
            noteEnds: lastParse.noteEnds ?? [],
        }, modData.speedRate);
        const officialPp = officialStar != null
            ? calculateOfficialPp({
                starRating: officialStar,
                perfect: counts[0], great: counts[1], good: counts[2],
                ok: counts[3], meh: counts[4], miss: counts[5],
                noFail: modCodes.includes("NF"),
                easy: modCodes.includes("EZ"),
            })
            : null;
        const official = officialPp != null ? { star: officialStar, pp: officialPp } : null;

        // Beatmap metadata row (from the payload + sunny result).
        const beatmap = data?.beatmap || {};
        const mapInfo = {
            artist: normalizeText(beatmap?.artist) || "-",
            title: normalizeText(beatmap?.title) || "-",
            version: normalizeText(beatmap?.version) || "-",
            mapper: normalizeText(beatmap?.mapper) || "-",
            keyCount: finiteNumber(ppMetrics?.totalNotes) != null
                ? columnCountFromParsed(lastParse, classic)
                : null,
            sunnyStar: Number.isFinite(stars) ? stars : null,
        };

        lastRender = {
            mapInfo,
            mode,
            source,
            counts,
            unitsTotal,
            hitTotal,
            od,
            odAvailable,
            classic,
            isConvert: Boolean(modData.cvtFlag),
            hr: modCodes.includes("HR"),
            ez: modCodes.includes("EZ"),
            clockRate: modData.speedRate,
            windows: pipeline.windows,
            mapWindows: pipeline.mapWindows,
            expectedAcc: pipeline.expectedAcc,
            mapFactorRes: pipeline.mapFactorRes,
            scoreAdjRes: pipeline.scoreAdjRes,
            ppRes: pipeline.ppRes,
            rework: pipeline.rework,
            deltaPct: pipeline.deltaPct,
            official,
            timings: {
                ...pipeline.timings,
                sunny: sunnyMs,
                total: performance.now() - tStart,
            },
        };
        lastModsKey = modData.modSignature;
        render();
    }

    function setCollapsed(nextCollapsed) {
        collapsed = Boolean(nextCollapsed);
    }

    render();

    return {
        handleSocketPayload,
        setCollapsed,
    };
}