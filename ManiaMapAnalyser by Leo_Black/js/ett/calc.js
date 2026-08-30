import { OsuFileParser } from "../parser/osuFileParser.js";
import { ETTERNA_VERSION_KEYS, SUPPORTED_KEYS, WASM_ASSET_VERSION } from "./constants.js";
import {
    DEFAULT_ETTERNA_VERSION,
    resolveEtternaVersionLoaderForKeycount,
} from "./versions/index.js";

const DEFAULT_SCORE_GOAL = 0.93;
const OFFICIAL_OUTPUT_ORDER = [
    "Overall",
    "Stream",
    "Jumpstream",
    "Handstream",
    "Stamina",
    "JackSpeed",
    "Chordjack",
    "Technical",
];

const DISPLAY_SKILLSET_ORDER = [
    "Stream",
    "Jumpstream",
    "Handstream",
    "Stamina",
    "JackSpeed",
    "Chordjack",
    "Technical",
    "Overall",
];

const wasmModulePromiseByVersion = new Map();
const fallbackWarningShownByRequestedVersion = new Set();

const IS_NODE = typeof process !== "undefined" && !!process.versions?.node;

// Browser resolves the wasm through the URL (fetch); Node's fs cannot read
// file:// URL strings, so convert to a filesystem path there.
function toWasmPath(fileUrl) {
    const parsed = new URL(fileUrl);
    let p = decodeURIComponent(parsed.pathname);
    if (/^\/[A-Za-z]:[\\/]/.test(p)) {
        p = p.slice(1);
    }
    return p;
}

// Version -> wasm filename mapping. Keys are guaranteed by ETTERNA_VERSION_KEYS
// (versions missing a filename entry fall back to the loader's own fetch path).
const WASM_FILE_NAME_BY_VERSION = Object.freeze({
    "0.68.0-Unofficial": "minaclac-68.0-unofficial.wasm",
    "0.70.0": "minaclac-70.0.wasm",
    "0.72.0": "minaclac-72.0.wasm",
    "0.72.3": "minaclac-72.3.wasm",
    "0.74.0": "minaclac-74.0.wasm",
    "0.75.0": "minaclac-75.0.wasm",
});

const WASM_FILE_BY_VERSION = Object.freeze(Object.fromEntries(
    ETTERNA_VERSION_KEYS.map((version) => [version, WASM_FILE_NAME_BY_VERSION[version]]),
));

async function loadEtternaModule(version, loader) {
    const locateFile = (path) => {
        const url = new URL(`./versions/${path}`, import.meta.url);
        if (!IS_NODE) {
            // Browser fetch: bust HTTP cache so updated .wasm bytes (e.g. the
            // MSD cap patch) are actually re-downloaded. Node preloads via
            // wasmBinary and resolves a filesystem path from pathname only,
            // so the query never reaches the fs path.
            url.searchParams.set("v", WASM_ASSET_VERSION);
        }
        return IS_NODE ? toWasmPath(url) : url.toString();
    };

    if (!IS_NODE) {
        return loader({ locateFile });
    }

    // Under Node, preload the wasm via fs and hand it to the glue as
    // `wasmBinary`; otherwise Emscripten's streaming-fetch path would try to
    // fetch the filesystem path and fail (fetch has no fallback there).
    const wasmName = WASM_FILE_BY_VERSION[version];
    if (!wasmName) {
        return loader({ locateFile });
    }
    const { readFile } = await import("node:fs/promises");
    const wasmPath = toWasmPath(new URL(`./versions/${wasmName}`, import.meta.url).toString());
    return loader({ locateFile, wasmBinary: new Uint8Array(await readFile(wasmPath)) });
}

function resolveKeycount(parsedCount, override) {
    if (Number.isFinite(override) && SUPPORTED_KEYS.has(override)) {
        return override;
    }

    if (SUPPORTED_KEYS.has(parsedCount)) {
        return parsedCount;
    }

    throw new Error(`Unsupported keycount: ${parsedCount}`);
}

function applyMod(chart, cvtFlag) {
    const normalized = String(cvtFlag || "").toUpperCase();
    if (!normalized) {
        return;
    }

    if (normalized.includes("IN")) {
        chart.modIN();
    }
    if (normalized.includes("HO")) {
        chart.modHO();
    }
}

function buildRows(chart) {
    const byTime = new Map();
    const columns = Array.isArray(chart.columns) ? chart.columns : [];
    const starts = Array.isArray(chart.noteStarts) ? chart.noteStarts : [];
    const len = Math.min(columns.length, starts.length);

    for (let i = 0; i < len; i += 1) {
        const col = Number(columns[i]);
        const start = Math.trunc(Number(starts[i]));
        if (!Number.isFinite(col) || !Number.isFinite(start) || col < 0 || col > 31) {
            continue;
        }

        const prev = byTime.get(start) || 0;
        byTime.set(start, prev | (1 << col));
    }

    const times = [...byTime.keys()].sort((a, b) => a - b);
    const masks = new Uint32Array(times.length);
    const seconds = new Float32Array(times.length);

    for (let i = 0; i < times.length; i += 1) {
        const t = times[i];
        masks[i] = byTime.get(t) >>> 0;
        seconds[i] = t / 1000;
    }

    return { masks, seconds };
}

function makeZeroValues() {
    const out = {};
    for (const name of DISPLAY_SKILLSET_ORDER) {
        out[name] = 0;
    }
    return out;
}

async function getWasmModule(requestedVersion = DEFAULT_ETTERNA_VERSION, keycount = null) {
    const {
        requestedVersion: normalizedRequestedVersion,
        version,
        loader,
        fallbackReason,
    } = resolveEtternaVersionLoaderForKeycount(requestedVersion, keycount);

    if (normalizedRequestedVersion !== version
        && fallbackReason
        && !fallbackWarningShownByRequestedVersion.has(normalizedRequestedVersion)) {
        fallbackWarningShownByRequestedVersion.add(normalizedRequestedVersion);
        console.warn(`Etterna version ${normalizedRequestedVersion} is unavailable; falling back to ${version}. Reason: ${fallbackReason}`);
    }

    if (!wasmModulePromiseByVersion.has(version)) {
        wasmModulePromiseByVersion.set(version, loadEtternaModule(version, loader));
    }
    return {
        requestedVersion: normalizedRequestedVersion,
        version,
        fallbackReason,
        module: await wasmModulePromiseByVersion.get(version),
    };
}

function mapOutputValues(rawEight) {
    const out = {};
    for (let i = 0; i < OFFICIAL_OUTPUT_ORDER.length; i += 1) {
        out[OFFICIAL_OUTPUT_ORDER[i]] = Number(rawEight[i]) || 0;
    }
    return out;
}

function runOfficialWasm(module, {
    keycount,
    musicRate,
    scoreGoal,
    rowMasks,
    rowTimes,
}) {
    const masksBytes = rowMasks.length * Uint32Array.BYTES_PER_ELEMENT;
    const timesBytes = rowTimes.length * Float32Array.BYTES_PER_ELEMENT;
    const outCount = OFFICIAL_OUTPUT_ORDER.length;
    const outBytes = outCount * Float32Array.BYTES_PER_ELEMENT;

    const ptrMasks = module._malloc(masksBytes);
    const ptrTimes = module._malloc(timesBytes);
    const ptrOut = module._malloc(outBytes);

    try {
        module.HEAPU32.set(rowMasks, ptrMasks >>> 2);
        module.HEAPF32.set(rowTimes, ptrTimes >>> 2);

        const ok = module._minacalc_compute(
            keycount,
            Number(musicRate),
            Number(scoreGoal),
            ptrMasks,
            ptrTimes,
            rowMasks.length,
            ptrOut,
        );

        if (!ok) {
            throw new Error("minacalc_compute returned failure");
        }

        const rawOut = module.HEAPF32.slice((ptrOut >>> 2), (ptrOut >>> 2) + outCount);
        return mapOutputValues(rawOut);
    } finally {
        module._free(ptrMasks);
        module._free(ptrTimes);
        module._free(ptrOut);
    }
}

export async function analyzeEtternaFromText(osuText, {
    musicRate = 1.0,
    scoreGoal = DEFAULT_SCORE_GOAL,
    keyOverride = null,
    cvtFlag = null,
    etternaVersion = DEFAULT_ETTERNA_VERSION,
} = {}) {
    const chart = new OsuFileParser(osuText);
    chart.process();

    if (chart.status !== "OK") {
        throw new Error(`Beatmap parse status: ${chart.status}`);
    }

    const keycount = resolveKeycount(chart.columnCount, keyOverride);
    applyMod(chart, cvtFlag);

    const { masks, seconds } = buildRows(chart);
    if (masks.length <= 1) {
        return {
            keycount,
            lnRatio: chart.lnRatio,
            metadata: chart.metaData,
            values: makeZeroValues(),
        };
    }

    const moduleInfo = await getWasmModule(etternaVersion, keycount);
    const values = runOfficialWasm(moduleInfo.module, {
        keycount,
        musicRate,
        scoreGoal,
        rowMasks: masks,
        rowTimes: seconds,
    });

    return {
        keycount,
        lnRatio: chart.lnRatio,
        metadata: chart.metaData,
        requestedEtternaVersion: moduleInfo.requestedVersion,
        etternaVersion: moduleInfo.version,
        etternaVersionFallbackReason: moduleInfo.fallbackReason,
        values,
    };
}

export {
    DEFAULT_SCORE_GOAL,
    DISPLAY_SKILLSET_ORDER,
};
