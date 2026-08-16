/**
 * Preset persistence:
 *  - Primary store: the `presetStorage` tosu setting (text, lives in
 *    settings/<folder>.values.json). It travels with the tosu instance,
 *    survives plugin updates / browser cache clears, and reaches every page
 *    (including in-game overlays) through the getSettings broadcast.
 *  - Cache: localStorage mirrors the last known library for fast first paint.
 *  - Migration: legacy localStorage libraries (mma.presets.custom.v1) are
 *    moved into presetStorage on first load.
 */

export const CUSTOM_PRESETS_KEY = "mma.presets.custom.v1";
export const ACTIVE_PRESET_KEY = "mma.presets.active.v1";
const LAST_WRITTEN_KEY = "mma.presets.lastWritten.v1";
const LEGACY_AUTO_NAME = "Auto";
export const AUTO_SAVE_PRESET_NAME = "Last Saved Preset";
export const PRESET_STORAGE_SETTING = "presetStorage";
export const DEFAULT_SLOT_NAMES = ["Custom 1", "Custom 2", "Custom 3"];

// ---------------------------------------------------------------------------
// localStorage helpers
// ---------------------------------------------------------------------------

function readStorageValue(key) {
    try {
        return window.localStorage.getItem(key);
    } catch {
        return null;
    }
}

function writeStorageValue(key, value) {
    try {
        window.localStorage.setItem(key, value);
    } catch {
        // Storage failures must not break the runtime.
    }
}

// ---------------------------------------------------------------------------
// Library (custom presets)
// ---------------------------------------------------------------------------

function sanitizeLibrary(parsed) {
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

/** Serializes the library for presetStorage (json string) and localStorage. */
export function serializeLibrary(presets) {
    return JSON.stringify(presets);
}

/** Reads the library from a getSettings payload (presetStorage key). */
export function libraryFromPayload(payload) {
    let raw = null;
    if (Array.isArray(payload)) {
        const entry = payload.find((item) => item?.uniqueID === PRESET_STORAGE_SETTING);
        raw = entry && typeof entry.value === "string" ? entry.value : null;
    } else if (payload && typeof payload === "object") {
        raw = typeof payload[PRESET_STORAGE_SETTING] === "string"
            ? payload[PRESET_STORAGE_SETTING]
            : null;
    }
    if (!raw) {
        return null;
    }
    try {
        return sanitizeLibrary(JSON.parse(raw));
    } catch {
        return null;
    }
}

/** Reads the library from the localStorage cache. */
export function libraryFromCache() {
    try {
        const raw = readStorageValue(CUSTOM_PRESETS_KEY);
        return raw ? sanitizeLibrary(JSON.parse(raw)) : [];
    } catch {
        return [];
    }
}

/** Writes the library into the localStorage cache (best effort). */
export function cacheLibrary(presets) {
    writeStorageValue(CUSTOM_PRESETS_KEY, serializeLibrary(presets));
}

/**
 * Loads the library with migration:
 * 1. presetStorage payload (authoritative) if present;
 * 2. otherwise the localStorage cache;
 * 3. legacy "Auto" container is renamed to "Last Saved Preset".
 */
export function loadLibrary(payload) {
    let presets = libraryFromPayload(payload);
    if (presets === null) {
        presets = libraryFromCache();
    }
    presets = presets || [];

    if (!presets.some((preset) => preset.name === AUTO_SAVE_PRESET_NAME)) {
        const legacy = presets.find((preset) => preset.name === LEGACY_AUTO_NAME);
        if (legacy) {
            legacy.name = AUTO_SAVE_PRESET_NAME;
        }
    }
    return presets;
}

// ---------------------------------------------------------------------------
// Active preset name
// ---------------------------------------------------------------------------

export function loadActivePreset() {
    try {
        const raw = readStorageValue(ACTIVE_PRESET_KEY);
        if (typeof raw === "string" && raw.trim()) {
            const value = raw.trim();
            return value === LEGACY_AUTO_NAME ? AUTO_SAVE_PRESET_NAME : value;
        }
    } catch {
        // ignore
    }
    return "Default";
}

export function persistActivePreset(name) {
    writeStorageValue(ACTIVE_PRESET_KEY, name);
}

// ---------------------------------------------------------------------------
// Write-back dedup (cross-page echo guard)
// ---------------------------------------------------------------------------

const WRITE_BACK_THROTTLE_MS = 1500;
const LAST_WRITTEN_DEPTH = 3;

export function readLastWritten() {
    try {
        const raw = window.localStorage.getItem(LAST_WRITTEN_KEY);
        if (!raw) {
            return null;
        }
        const parsed = JSON.parse(raw);
        if (Array.isArray(parsed)) {
            return parsed;
        }
        // Legacy single-record format.
        return [parsed];
    } catch {
        return null;
    }
}

export function markWritten(snapshot, presetName) {
    const list = readLastWritten() || [];
    list.unshift({ presetName, snapshot: { ...snapshot }, t: Date.now() });
    if (list.length > LAST_WRITTEN_DEPTH) {
        list.length = LAST_WRITTEN_DEPTH;
    }
    try {
        window.localStorage.setItem(LAST_WRITTEN_KEY, JSON.stringify(list));
    } catch {
        // Storage failure only costs us cross-page dedup.
    }
}

/** True when ANY preset was written back very recently (by any page). */
export function recentlyWritten() {
    const list = readLastWritten();
    return Boolean(list && list.some((r) => Date.now() - r.t < WRITE_BACK_THROTTLE_MS));
}
