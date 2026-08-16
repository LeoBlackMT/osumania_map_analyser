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

const STORE_VERSION = 2;
const LEGACY_AUTO_NAME = "Auto";
const WRITE_BACK_THROTTLE_MS = 1500;
const LAST_WRITTEN_DEPTH = 3;

// ---------------------------------------------------------------------------
// Store serialization / parsing
// ---------------------------------------------------------------------------

function sanitizePresets(parsed) {
    if (!Array.isArray(parsed)) {
        return [];
    }
    return parsed.filter(
        (preset) => preset
            && typeof preset.id === "string"
            && typeof preset.name === "string"
            && preset.name.trim().length > 0
            && preset.settings && typeof preset.settings === "object",
    );
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
                lastWritten: Array.isArray(parsed.lastWritten) ? parsed.lastWritten : [],
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
