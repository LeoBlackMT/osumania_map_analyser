/**
 * Preset persistence — single authoritative store: the `presetStorage` tosu
 * setting (text, lives in settings/<folder>.values.json).
 *
 * It travels with the tosu instance, survives plugin updates and browser
 * cache clears, and reaches EVERY page (browser + in-game overlays, any
 * origin — localhost or 127.0.0.1) through the getSettings broadcast, so no
 * browser-side storage is needed at all.
 *
 * Store shape (version 2):
 *   { v: 2, lastWritten: [{presetName, snapshot, t}], presets: [...] }
 * Version 1 (a bare preset array) is accepted on read and migrated in memory.
 */

export const AUTO_SAVE_PRESET_NAME = "LastSavedPreset";
export const PRESET_STORAGE_SETTING = "presetStorage";
export const DEFAULT_SLOT_NAMES = ["Custom1", "Custom2", "Custom3"];

/**
 * System keys that must NEVER be captured into a preset snapshot:
 * - "presetStorage": the presets library itself — storing it inside a preset
 *   snapshot recursively embeds the store (which contains the preset, which
 *   contains the store...) and blows values.json up to hundreds of MB, making
 *   every getSettings parse take seconds.
 * - "preset": the active preset picker value — UI state, not a user setting.
 */
export const SYSTEM_SNAPSHOT_KEYS = new Set([PRESET_STORAGE_SETTING, "preset"]);

const STORE_VERSION = 2;
const LEGACY_AUTO_NAME = "Auto";
const WRITE_BACK_THROTTLE_MS = 1500;
const LAST_WRITTEN_DEPTH = 3;

// ---------------------------------------------------------------------------
// Store serialization / parsing
// ---------------------------------------------------------------------------

/**
 * Cleans and validates a parsed preset list.
 * - Keeps only structurally valid presets.
 * - Strips SYSTEM_SNAPSHOT_KEYS from every preset's settings: historical
 *   builds captured presetStorage/preset into snapshots, which recursively
 *   embedded the whole store inside itself and inflated values.json to
 *   hundreds of MB (each getSettings parse then took seconds). Cleaning on
 *   load shrinks the store back to its real content.
 */
function sanitizePresets(parsed) {
    if (!Array.isArray(parsed)) {
        return [];
    }
    const clean = [];
    for (const preset of parsed) {
        if (!preset
            || typeof preset.id !== "string"
            || typeof preset.name !== "string"
            || preset.name.trim().length === 0
            || !preset.settings || typeof preset.settings !== "object") {
            continue;
        }
        const settings = { ...preset.settings };
        for (const key of SYSTEM_SNAPSHOT_KEYS) {
            delete settings[key];
        }
        clean.push({ ...preset, settings });
    }
    return clean;
}

/** Serializes the full store (presets + lastWritten) for presetStorage. */
export function serializeStore(presets, lastWritten = []) {
    return JSON.stringify({
        v: STORE_VERSION,
        lastWritten: Array.isArray(lastWritten) ? lastWritten : [],
        presets,
    });
}

/**
 * Canonical fingerprint of the store CONTENT (presets + write-back queue
 * WITHOUT timestamps). Two stores that describe the same logical state produce
 * the same fingerprint, so the caller can skip a redundant POST when nothing
 * actually changed. Timestamps (`t`) are transient cross-page dedup state and
 * must not participate in the comparison.
 */
export function storeFingerprint(presets, lastWritten = []) {
    return JSON.stringify({
        v: STORE_VERSION,
        lastWritten: normalizeLastWritten(lastWritten),
        presets,
    });
}

/**
 * Canonical form of the write-back queue: presetName + snapshot only
 * (timestamps and system keys stripped). Snapshot objects may contain
 * historically embedded presetStorage/preset values that inflate values.json
 * to hundreds of MB — they are dropped here too.
 */
function normalizeLastWritten(lastWritten = []) {
    if (!Array.isArray(lastWritten)) {
        return [];
    }
    return lastWritten
        .filter((record) => record && typeof record === "object")
        .map((record) => {
            const snapshot = record.snapshot && typeof record.snapshot === "object"
                ? record.snapshot
                : {};
            const cleanSnapshot = { ...snapshot };
            for (const key of SYSTEM_SNAPSHOT_KEYS) {
                delete cleanSnapshot[key];
            }
            return {
                presetName: typeof record.presetName === "string" ? record.presetName : "",
                snapshot: cleanSnapshot,
            };
        });
}

/** Strips system keys from a raw lastWritten queue (parse-time cleanup). */
function cleanLastWritten(lastWritten) {
    if (!Array.isArray(lastWritten)) {
        return [];
    }
    return lastWritten
        .filter((record) => record && typeof record === "object")
        .map((record) => {
            const snapshot = record.snapshot && typeof record.snapshot === "object"
                ? { ...record.snapshot }
                : {};
            for (const key of SYSTEM_SNAPSHOT_KEYS) {
                delete snapshot[key];
            }
            return { ...record, snapshot };
        });
}

/**
 * Canonical fingerprint of the RAW (uncleaned) store as it lives in tosu.
 * Compares against storeFingerprint() of the cleaned in-memory store: when
 * they differ, historical pollution (system keys embedded in preset settings)
 * was stripped on load and the cleaned store should be written back once.
 */
export function rawStoreFingerprint(raw) {
    if (typeof raw !== "string" || raw.length === 0) {
        return null;
    }
    try {
        const parsed = JSON.parse(raw);
        if (Array.isArray(parsed)) {
            return JSON.stringify({ v: 1, presets: parsed });
        }
        if (parsed && typeof parsed === "object") {
            return JSON.stringify({
                v: parsed.v ?? 2,
                lastWritten: Array.isArray(parsed.lastWritten) ? parsed.lastWritten : [],
                presets: Array.isArray(parsed.presets) ? parsed.presets : [],
            });
        }
    } catch {
        // fall through
    }
    return null;
}

/**
 * Parses a store from a raw presetStorage value.
 * @returns {{presets: Array, lastWritten: Array}|null} null when unavailable/invalid.
 */
export function parseStore(raw) {
    if (typeof raw !== "string" || raw.length === 0) {
        return null;
    }
    try {
        const parsed = JSON.parse(raw);
        if (Array.isArray(parsed)) {
            // v1: bare preset array.
            return { presets: sanitizePresets(parsed), lastWritten: [] };
        }
        if (parsed && typeof parsed === "object") {
            return {
                presets: sanitizePresets(parsed.presets),
                lastWritten: cleanLastWritten(parsed.lastWritten),
            };
        }
    } catch {
        // fall through
    }
    return null;
}

/** Reads the store from a getSettings payload (presetStorage key). */
export function storeFromPayload(payload) {
    let raw = null;
    if (Array.isArray(payload)) {
        const entry = payload.find((item) => item?.uniqueID === PRESET_STORAGE_SETTING);
        raw = entry && typeof entry.value === "string" ? entry.value : null;
    } else if (payload && typeof payload === "object") {
        raw = typeof payload[PRESET_STORAGE_SETTING] === "string"
            ? payload[PRESET_STORAGE_SETTING]
            : null;
    }
    return parseStore(raw);
}

/**
 * Normalizes a parsed preset list: renames the legacy "Auto" container to
 * "LastSavedPreset". Returns the list itself.
 */
export function normalizeLibrary(presets) {
    const list = presets || [];
    if (!list.some((preset) => preset.name === AUTO_SAVE_PRESET_NAME)) {
        const legacy = list.find((preset) => preset.name === LEGACY_AUTO_NAME);
        if (legacy) {
            legacy.name = AUTO_SAVE_PRESET_NAME;
        }
    }
    return list;
}

// ---------------------------------------------------------------------------
// Write-back dedup (cross-page / cross-origin echo guard)
// ---------------------------------------------------------------------------

/**
 * True when ANY preset was written back very recently, per the lastWritten
 * queue of the given store (the authoritative, broadcast-shared copy — same
 * data on every origin).
 */
export function recentlyWritten(lastWritten = []) {
    const now = Date.now();
    return lastWritten.some((r) => r && typeof r.t === "number" && now - r.t < WRITE_BACK_THROTTLE_MS);
}

/** Prepends a write-back record to the queue (mutates the array). */
export function markWritten(lastWritten, snapshot, presetName) {
    lastWritten.unshift({ presetName, snapshot: { ...snapshot }, t: Date.now() });
    if (lastWritten.length > LAST_WRITTEN_DEPTH) {
        lastWritten.length = LAST_WRITTEN_DEPTH;
    }
}
