import { APP_CONFIG } from "../../config.js";
import { runAzusaEstimatorFromText } from "../estimator/azusaEstimator.js";
import { classifyCompanellaDifficulty } from "../estimator/companellaEstimator.js";
import { runDanielEstimatorFromText } from "../estimator/danielEstimator.js";
import {
    applyCompanellaToMixedResult,
    runMixedEstimatorFromText,
} from "../estimator/mixedEstimator.js";
import { numericToRcLabel } from "../estimator/rcDifficultyFormat.js";
import { runRoxyEstimatorFromText } from "../estimator/roxyEstimator.js";
import { runSunnyEstimatorFromText } from "../estimator/sunnyEstimator.js";
import {
    analyzeEtternaFromText,
    DEFAULT_SCORE_GOAL as ETT_DEFAULT_SCORE_GOAL,
} from "../ett/index.js";
import { calculateInterludeStar } from "../interlude/index.js";
import { getModData } from "../app/modData.js";
import { state } from "../app/appContext.js";
import {
    evaluateRoxyOverfitVariant,
    ROXY_OVERFIT_REPORT,
} from "./roxyOverfitModels.generated.js";

const STORAGE_KEY = "mma-debug-estimator-panel-v1";
const DEFAULT_ALGORITHMS = ["Roxy", "Azusa", "Mixed"];
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

function buildBeatmapIdentity(data, modSignature) {
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

    return `${parts.join("|")}|mods:${modSignature || "none"}`;
}

function getDebugModData(data) {
    return getModData(data, {
        sortedKnownModCodes: SORTED_KNOWN_MOD_CODES,
        modBitFlagEntries: MOD_BIT_FLAG_ENTRIES,
        fallbackClient: data?.client || "",
        preferPlayMods: false,
    });
}

function summarizeBeatmap(data) {
    const beatmap = data?.beatmap || {};
    const artist = normalizeText(beatmap.artist);
    const title = normalizeText(beatmap.title);
    const version = normalizeText(beatmap.version);
    const mapper = normalizeText(beatmap.mapper);
    const id = normalizeText(beatmap.id);
    const main = [artist, title].filter(Boolean).join(" - ") || "Unknown beatmap";
    const tail = [version, mapper ? `mapped by ${mapper}` : "", id ? `bid ${id}` : ""]
        .filter(Boolean)
        .join(" | ");
    return tail ? `${main} [${tail}]` : main;
}

function resultIsUsable(result) {
    const text = normalizeText(result?.estDiff);
    return Boolean(result) && text && !/^Invalid\b/i.test(text);
}

function makeInvalidResult(message) {
    return {
        estDiff: `Unavailable: ${message}`,
        numericDifficulty: null,
        numericDifficultyHint: "debug-unavailable",
        star: Number.NaN,
        lnRatio: 0,
        columnCount: 0,
    };
}

function makeNumericResult(baseResult, numericDifficulty, hint) {
    const numeric = finiteNumber(numericDifficulty);
    if (numeric == null) {
        return makeInvalidResult("source value is not finite");
    }
    return {
        ...baseResult,
        estDiff: numericToRcLabel(numeric),
        numericDifficulty: Number(numeric.toFixed(2)),
        numericDifficultyHint: hint,
        graph: null,
    };
}

async function runCompanellaFromText(osuText, options, context) {
    const sunny = await getCached(context, "Sunny", () => runSunnyEstimatorFromText(osuText, options));
    if (Number(sunny?.columnCount) !== 4) {
        return makeInvalidResult("Companella supports 4K only");
    }

    const [ettResult, interludeStar] = await Promise.all([
        analyzeEtternaFromText(osuText, {
            musicRate: options.speedRate,
            scoreGoal: ETT_DEFAULT_SCORE_GOAL,
            keyOverride: null,
            cvtFlag: options.cvtFlag,
            etternaVersion: APP_CONFIG.defaults.companellaEtternaVersion || APP_CONFIG.defaults.etternaVersion,
        }),
        calculateInterludeStar(osuText, options.speedRate, options.cvtFlag),
    ]);

    const companella = await classifyCompanellaDifficulty({
        msdValues: ettResult?.values,
        interludeStar,
        sunnyStar: Number(sunny.star),
    });

    return {
        ...sunny,
        ...companella,
        graph: null,
    };
}

async function runMixedActualFromText(osuText, options, context) {
    const mixed = runMixedEstimatorFromText(osuText, options);
    if (!mixed?.mixedCompanellaPlan) {
        return mixed;
    }
    const companella = await getCached(context, "Companella", () => runCompanellaFromText(osuText, options, context));
    return applyCompanellaToMixedResult(mixed, companella);
}

async function getCached(context, key, factory) {
    if (!context.cache.has(key)) {
        context.cache.set(key, Promise.resolve().then(factory));
    }
    return context.cache.get(key);
}

async function runRoxyBase(osuText, options, context) {
    return getCached(context, "Roxy", () => runRoxyEstimatorFromText(osuText, options));
}

function overfitReportText(variantId) {
    const report = ROXY_OVERFIT_REPORT?.models?.[variantId];
    if (!report) {
        return "Debug-only aggressive variant fitted from local benchmark probes.";
    }
    return `Debug-only overfit model, local full benchmark: Exact ${formatNumber(report.exactRate)}%, Close+ ${formatNumber(report.closePlusRate)}%, Miss ${formatNumber(report.missRate)}%.`;
}

function runRoxyOverfitVariant(variantId, hint) {
    return async (osuText, options, context) => {
        const roxy = await runRoxyBase(osuText, options, context);
        const numeric = evaluateRoxyOverfitVariant(variantId, roxy);
        return makeNumericResult(roxy, numeric, hint);
    };
}

const ALGORITHM_REGISTRY = Object.freeze([
    {
        id: "Mixed",
        label: "Mixed",
        group: "Settings algorithms",
        note: "Actual Mixed path, including Companella post-pass when required.",
        run: runMixedActualFromText,
    },
    {
        id: "Azusa",
        label: "Azusa",
        group: "Settings algorithms",
        note: "Standalone Azusa RC estimator.",
        run: (osuText, options) => runAzusaEstimatorFromText(osuText, {
            ...options,
            forceSunnyReferenceHo: false,
        }),
    },
    {
        id: "Roxy",
        label: "Roxy",
        group: "Settings algorithms",
        note: "Current production Roxy.",
        run: runRoxyBase,
    },
    {
        id: "Sunny",
        label: "Sunny",
        group: "Settings algorithms",
        note: "Sunny rework estimator.",
        run: (osuText, options) => runSunnyEstimatorFromText(osuText, options),
    },
    {
        id: "Daniel",
        label: "Daniel",
        group: "Settings algorithms",
        note: "Daniel RC estimator.",
        run: (osuText, options) => runDanielEstimatorFromText(osuText, options),
    },
    {
        id: "Companella",
        label: "Companella",
        group: "Settings algorithms",
        note: "Async ONNX path; requires Etterna + Interlude inputs.",
        run: runCompanellaFromText,
    },
    {
        id: "RoxyStructural",
        label: "Roxy structural only",
        group: "Roxy diagnostics",
        note: "Roxy before meta model and reference stacking.",
        run: async (osuText, options, context) => {
            const roxy = await runRoxyBase(osuText, options, context);
            return makeNumericResult(roxy, roxy?.debug?.structuralNumeric, "debug-roxy-structural");
        },
    },
    {
        id: "RoxyMetaRaw",
        label: "Roxy meta raw",
        group: "Roxy diagnostics",
        note: "Roxy meta model output before later guards/corrections.",
        run: async (osuText, options, context) => {
            const roxy = await runRoxyBase(osuText, options, context);
            return makeNumericResult(roxy, roxy?.debug?.metaNumeric, "debug-roxy-meta-raw");
        },
    },
    {
        id: "RoxyNoAzusaLift",
        label: "Roxy without Azusa lift",
        group: "Roxy diagnostics",
        note: "Current Roxy minus the small Azusa high-gap lift.",
        run: async (osuText, options, context) => {
            const roxy = await runRoxyBase(osuText, options, context);
            const numeric = finiteNumber(roxy?.numericDifficulty);
            const lift = finiteNumber(roxy?.debug?.azusaHighGapLift) || 0;
            return makeNumericResult(roxy, numeric == null ? null : numeric - lift, "debug-roxy-no-azusa-lift");
        },
    },
    {
        id: "RoxyGbdtFull500",
        label: "Roxy GBDT full 500",
        group: "Archived overfit variants",
        overfit: true,
        note: overfitReportText("RoxyGbdtFull500"),
        run: runRoxyOverfitVariant("RoxyGbdtFull500", "debug-roxy-gbdt-full-500"),
    },
    {
        id: "RoxyPolyResidual",
        label: "Roxy polynomial residual",
        group: "Archived overfit variants",
        overfit: true,
        note: overfitReportText("RoxyPolyResidual"),
        run: runRoxyOverfitVariant("RoxyPolyResidual", "debug-roxy-poly-residual"),
    },
    {
        id: "RoxyIsotonicFull",
        label: "Roxy isotonic full",
        group: "Archived overfit variants",
        overfit: true,
        note: overfitReportText("RoxyIsotonicFull"),
        run: runRoxyOverfitVariant("RoxyIsotonicFull", "debug-roxy-isotonic-full"),
    },
    {
        id: "RoxySegmentedMeta",
        label: "Roxy segmented meta",
        group: "Archived overfit variants",
        overfit: true,
        note: overfitReportText("RoxySegmentedMeta"),
        run: runRoxyOverfitVariant("RoxySegmentedMeta", "debug-roxy-segmented-meta"),
    },
    {
        id: "RoxyBucket05Linear",
        label: "Roxy bucket 0.5 linear",
        group: "Archived overfit variants",
        overfit: true,
        note: overfitReportText("RoxyBucket05Linear"),
        run: runRoxyOverfitVariant("RoxyBucket05Linear", "debug-roxy-bucket-05-linear"),
    },
    {
        id: "RoxyBucketOffsets",
        label: "Roxy bucket offsets",
        group: "Archived overfit variants",
        overfit: true,
        note: overfitReportText("RoxyBucketOffsets"),
        run: runRoxyOverfitVariant("RoxyBucketOffsets", "debug-roxy-bucket-offsets"),
    },
    {
        id: "RoxyHandSplitTech",
        label: "Roxy hand-split tech",
        group: "Archived overfit variants",
        overfit: true,
        note: "Debug-only proxy for the removed hand-split/tech idea; it uses current Roxy hand/tech stream summaries and is not a recovered historical model.",
        run: runRoxyOverfitVariant("RoxyHandSplitTech", "debug-roxy-hand-split-tech-proxy"),
    },
]);

const ALGORITHM_BY_ID = new Map(ALGORITHM_REGISTRY.map((entry) => [entry.id, entry]));

function createDefaultState() {
    return {
        slots: DEFAULT_ALGORITHMS.map((algorithmId) => ({
            id: createSlotId(),
            algorithmId,
            counter: 0,
        })),
        runCount: 0,
    };
}

function createSlotId() {
    if (globalThis.crypto && typeof globalThis.crypto.randomUUID === "function") {
        return globalThis.crypto.randomUUID();
    }
    return `${Date.now()}-${Math.random()}`;
}

function loadState() {
    try {
        const parsed = JSON.parse(localStorage.getItem(STORAGE_KEY) || "null");
        if (!parsed || !Array.isArray(parsed.slots) || parsed.slots.length === 0) {
            return createDefaultState();
        }
        return {
            slots: parsed.slots.map((slot) => ({
                id: normalizeText(slot.id) || `${Date.now()}-${Math.random()}`,
                algorithmId: ALGORITHM_BY_ID.has(slot.algorithmId) ? slot.algorithmId : "Roxy",
                counter: Math.max(0, Number(slot.counter) || 0),
            })),
            runCount: Math.max(0, Number(parsed.runCount) || 0),
        };
    } catch {
        return createDefaultState();
    }
}

function saveState(state) {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({
        slots: state.slots,
        runCount: state.runCount,
    }));
}

function buildAlgorithmOptions(selectedId) {
    const groups = [];
    for (const entry of ALGORITHM_REGISTRY) {
        let group = groups.find((item) => item.label === entry.group);
        if (!group) {
            group = { label: entry.group, entries: [] };
            groups.push(group);
        }
        group.entries.push(entry);
    }

    return groups.map((group) => `
        <optgroup label="${group.label}">
            ${group.entries.map((entry) => `
                <option value="${entry.id}" ${entry.id === selectedId ? "selected" : ""}>
                    ${entry.label}${entry.overfit ? " [overfit]" : ""}${entry.archived ? " [archived]" : ""}
                </option>
            `).join("")}
        </optgroup>
    `).join("");
}

function renderResult(resultState) {
    if (!resultState) {
        return "<div class=\"estimator-debug-empty\">Waiting for map data.</div>";
    }
    if (resultState.status === "running") {
        return "<div class=\"estimator-debug-running\">Running...</div>";
    }
    if (resultState.status === "error") {
        return `<div class="estimator-debug-error">${resultState.message}</div>`;
    }

    const result = resultState.result || {};
    return `
        <div class="estimator-debug-result-line">
            <strong>${normalizeText(result.estDiff) || "-"}</strong>
            <span>numeric ${formatNumber(result.numericDifficulty)}</span>
            <span>star ${formatNumber(result.star, 3)}</span>
            <span>LN ${formatNumber((finiteNumber(result.lnRatio) || 0) * 100, 1)}%</span>
            <span>${finiteNumber(result.columnCount) || "-"}K</span>
            <span>${formatNumber(resultState.elapsedMs, 1)} ms</span>
        </div>
        <div class="estimator-debug-note">${normalizeText(result.numericDifficultyHint) || "no hint"}</div>
    `;
}

function runOptionsFromPayload(data) {
    const modData = getDebugModData(data);
    return {
        options: {
            speedRate: modData.speedRate,
            odFlag: modData.odFlag,
            cvtFlag: modData.cvtFlag,
            withGraph: false,
            enableAlwaysShowLNDifficulty: state.enableAlwaysShowLNDifficulty,
        },
        modData,
    };
}

export function createEstimatorDebugPanel({
    root,
    socketHost = "127.0.0.1:24050",
} = {}) {
    if (!root) {
        throw new Error("Estimator debug panel root is required");
    }

    const state = loadState();
    const results = new Map();
    let lastPayload = null;
    let lastIdentity = "";
    let lastOsuText = "";
    let lastOptions = null;
    let runSeq = 0;

    function setResult(slotId, next) {
        results.set(slotId, next);
        render();
    }

    function addSlot(algorithmId = "Roxy") {
        state.slots.push({
            id: createSlotId(),
            algorithmId,
            counter: 0,
        });
        saveState(state);
        render();
        runAll("slot added");
    }

    function removeSlot(slotId) {
        if (state.slots.length <= 1) return;
        state.slots = state.slots.filter((slot) => slot.id !== slotId);
        results.delete(slotId);
        saveState(state);
        render();
    }

    function resetCounters() {
        for (const slot of state.slots) {
            slot.counter = 0;
        }
        state.runCount = 0;
        saveState(state);
        render();
    }

    function updateCounter(slotId, delta) {
        const slot = state.slots.find((item) => item.id === slotId);
        if (!slot) return;
        slot.counter = Math.max(0, (Number(slot.counter) || 0) + delta);
        saveState(state);
        render();
    }

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

    async function runSlot(slot, osuText, options, seq, context) {
        const entry = ALGORITHM_BY_ID.get(slot.algorithmId);
        if (!entry) {
            setResult(slot.id, { status: "error", message: "Unknown algorithm" });
            return;
        }
        if (entry.archived || typeof entry.run !== "function") {
            setResult(slot.id, {
                status: "ok",
                elapsedMs: 0,
                result: makeInvalidResult(`Archived overfit variant. ${entry.note || ""}`),
            });
            return;
        }

        setResult(slot.id, { status: "running" });
        const startedAt = performance.now();
        try {
            const result = await entry.run(osuText, options, context);
            if (seq !== runSeq) return;
            setResult(slot.id, {
                status: resultIsUsable(result) ? "ok" : "error",
                elapsedMs: performance.now() - startedAt,
                result,
                message: resultIsUsable(result) ? "" : normalizeText(result?.estDiff) || "Estimator returned invalid result",
            });
        } catch (error) {
            if (seq !== runSeq) return;
            setResult(slot.id, {
                status: "error",
                elapsedMs: performance.now() - startedAt,
                message: error?.message || "Estimator failed",
            });
        }
    }

    async function runAll(reason = "manual") {
        if (!lastPayload) {
            render();
            return;
        }

        const seq = runSeq + 1;
        runSeq = seq;
        try {
            const { options } = runOptionsFromPayload(lastPayload);
            lastOptions = options;
            const osuText = lastOsuText || await fetchCurrentBeatmap();
            if (seq !== runSeq) return;
            lastOsuText = osuText;
            state.runCount += 1;
            saveState(state);
            render();
            const context = { cache: new Map() };
            await Promise.all(state.slots.map((slot) => runSlot(slot, osuText, options, seq, context)));
        } catch (error) {
            for (const slot of state.slots) {
                setResult(slot.id, {
                    status: "error",
                    message: error?.message || `Failed to run estimators (${reason})`,
                });
            }
        }
    }

    function handleSocketPayload(data) {
        lastPayload = data;
        const { modData } = runOptionsFromPayload(data);
        const identity = buildBeatmapIdentity(data, modData.modSignature);
        if (!identity || identity === lastIdentity) {
            return;
        }
        lastIdentity = identity;
        lastOsuText = "";
        runAll("beatmap changed");
    }

    function render() {
        const modData = lastPayload ? runOptionsFromPayload(lastPayload).modData : null;
        const metadata = lastPayload ? summarizeBeatmap(lastPayload) : "Waiting for websocket data...";
        const modText = modData
            ? `rate ${formatNumber(modData.speedRate, 3)} | OD ${modData.odFlag ?? "base"} | CVT ${modData.cvtFlag ?? "none"}`
            : "mods unknown";

        root.innerHTML = `
            <section class="estimator-debug-panel">
                <div class="estimator-debug-header">
                    <div>
                        <h2>Estimator Compare</h2>
                        <div class="estimator-debug-meta">${metadata}</div>
                        <div class="estimator-debug-note">${modText}</div>
                    </div>
                    <div class="estimator-debug-actions">
                        <span class="badge">Runs: ${state.runCount}</span>
                        <button type="button" data-debug-action="run">Run now</button>
                        <button type="button" data-debug-action="add">Add estimator</button>
                        <button type="button" data-debug-action="reset">Reset counters</button>
                    </div>
                </div>
                <div class="estimator-debug-slots">
                    ${state.slots.map((slot) => {
                        const entry = ALGORITHM_BY_ID.get(slot.algorithmId) || ALGORITHM_BY_ID.get("Roxy");
                        return `
                            <div class="estimator-debug-slot" data-slot-id="${slot.id}">
                                <div class="estimator-debug-slot-top">
                                    <select data-debug-action="select">
                                        ${buildAlgorithmOptions(slot.algorithmId)}
                                    </select>
                                    <div class="estimator-debug-counter">
                                        <button type="button" data-debug-action="dec">-</button>
                                        <span>Count: ${slot.counter}</span>
                                        <button type="button" data-debug-action="inc">+</button>
                                    </div>
                                    <button type="button" data-debug-action="remove">Remove</button>
                                </div>
                                <div class="estimator-debug-note">${entry?.note || ""}</div>
                                ${renderResult(results.get(slot.id))}
                            </div>
                        `;
                    }).join("")}
                </div>
            </section>
        `;
    }

    root.addEventListener("click", (event) => {
        const target = event.target;
        if (!(target instanceof HTMLElement)) return;
        const action = target.getAttribute("data-debug-action");
        if (!action) return;
        const slotEl = target.closest("[data-slot-id]");
        const slotId = slotEl?.getAttribute("data-slot-id") || "";

        if (action === "run") runAll("manual");
        if (action === "add") addSlot();
        if (action === "reset") resetCounters();
        if (action === "remove") removeSlot(slotId);
        if (action === "inc") updateCounter(slotId, 1);
        if (action === "dec") updateCounter(slotId, -1);
    });

    root.addEventListener("change", (event) => {
        const target = event.target;
        if (!(target instanceof HTMLSelectElement)) return;
        if (target.getAttribute("data-debug-action") !== "select") return;
        const slotId = target.closest("[data-slot-id]")?.getAttribute("data-slot-id") || "";
        const slot = state.slots.find((item) => item.id === slotId);
        if (!slot) return;
        slot.algorithmId = target.value;
        saveState(state);
        render();
        runAll("algorithm changed");
    });

    render();

    return {
        handleSocketPayload,
        runAll,
    };
}
