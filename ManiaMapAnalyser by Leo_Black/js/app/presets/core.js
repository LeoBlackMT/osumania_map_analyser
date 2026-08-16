/**
 * Preset system core: application logic, tosu settings-stream handling,
 * echo guard and write-back. UI-agnostic — the manager page renders through
 * the exported state/action API and onPresetsChanged notifications.
 */

import { socket, state } from "../appContext.js";
import {
    SETTING_RECOMPUTE_KEYS,
    SETTING_CACHE_KEYS,
} from "../settings.js";
import { clearResultCache } from "../resultCache.js";
import { scheduleRecompute } from "../scheduler.js";
import {
    loadSettingsSchema,
    getterFor,
    buildDefaultSnapshot,
} from "./schema.js";
import {
    AUTO_SAVE_PRESET_NAME,
    CUSTOM_PRESETS_KEY,
    ACTIVE_PRESET_KEY,
    DEFAULT_SLOT_NAMES,
    PRESET_STORAGE_SETTING,
    loadLibrary,
    cacheLibrary,
    serializeLibrary,
    libraryFromPayload,
    loadActivePreset,
    persistActivePreset,
    readLastWritten,
    markWritten,
    recentlyWritten,
} from "./storage.js";

// ---------------------------------------------------------------------------
// Module state
// ---------------------------------------------------------------------------

let customPresets = [];
let currentPreset = "Default";
let lastValues = null;
let initialized = false;
let builtinLoaded = false;
let builtinPresets = []; // [{ id, name, description, file }]
let builtinCache = new Map(); // id -> settings object

// Keys never counted as a "manual settings change": wsEndpoint is a connection
// parameter, presetStorage is the presets library itself.
const IGNORED_DIFF_KEYS = new Set([PRESET_STORAGE_SETTING, "wsEndpoint"]);

const listeners = new Set();

function notifyChanged() {
    for (const cb of listeners) {
        try {
            cb();
        } catch {
            // A broken listener must not break preset logic.
        }
    }
}

// ---------------------------------------------------------------------------
// Public state / events
// ---------------------------------------------------------------------------

export function getCustomPresets() {
    return customPresets;
}

export function getBuiltinPresets() {
    return builtinPresets;
}

/** Returns the settings of a built-in preset by id (null when unavailable). */
export function getBuiltinSettings(id) {
    return builtinCache.get(id) || null;
}

export function getCurrentPreset() {
    return currentPreset;
}

/** Registers a UI listener, returns an unsubscribe function. */
export function onPresetsChanged(callback) {
    listeners.add(callback);
    return () => listeners.delete(callback);
}

// ---------------------------------------------------------------------------
// Built-in presets (presets/*.json)
// ---------------------------------------------------------------------------

async function loadBuiltinPresets() {
    if (builtinLoaded) {
        return;
    }
    builtinLoaded = true;
    try {
        const response = await fetch("./presets/index.json", { cache: "no-store" });
        const index = await response.json();
        builtinPresets = Array.isArray(index.presets) ? index.presets : [];
        await Promise.all(builtinPresets.map(async (preset) => {
            try {
                const fileResponse = await fetch(`./presets/${preset.file}`, { cache: "no-store" });
                const data = await fileResponse.json();
                builtinCache.set(preset.id, (data && data.settings) || {});
                // Merge metadata from the preset file (version etc.) into the list entry.
                preset.version = (data && typeof data.version === "number") ? data.version : 1;
            } catch {
                // Keep the entry out of builtinCache; it is simply not applicable.
            }
        }));
    } catch {
        builtinPresets = [];
    }
}

// ---------------------------------------------------------------------------
// Snapshots
// ---------------------------------------------------------------------------

/** Captures the currently applied user settings as a full snapshot. */
export async function captureCurrentSettings() {
    const { keys } = await loadSettingsSchema();
    const snapshot = {};
    for (const key of keys) {
        snapshot[key] = getterFor(key)();
    }
    return snapshot;
}

/**
 * Applies a (possibly partial) snapshot: only keys present in the snapshot
 * are applied; everything else keeps its current value.
 */
export async function applySnapshot(snapshot) {
    const { appliers } = await loadSettingsSchema();
    let recomputeNeeded = false;
    let cacheNeeded = false;

    for (const [key, value] of Object.entries(snapshot)) {
        const applier = appliers.get(key);
        if (!applier) {
            continue;
        }
        if (applier(value)) {
            if (SETTING_RECOMPUTE_KEYS.has(key)) {
                recomputeNeeded = true;
            }
            if (SETTING_CACHE_KEYS.has(key)) {
                cacheNeeded = true;
            }
        }
    }

    if (cacheNeeded) {
        clearResultCache();
    }
    if (recomputeNeeded) {
        scheduleRecompute("preset applied", true);
    }
}

// ---------------------------------------------------------------------------
// Custom snapshot apply (used by the manager page "Apply" action)
// ---------------------------------------------------------------------------

/**
 * Applies an arbitrary (partial) snapshot and syncs it back to tosu.
 * Optionally anchors the picker to a preset name.
 */
export async function applyCustomSnapshot(snapshot, presetName = null) {
    await applySnapshot(snapshot);
    const anchor = presetName || currentPreset;
    if (presetName) {
        currentPreset = presetName;
        persistActivePreset(presetName);
    }
    if (shouldWriteBack(snapshot, anchor)) {
        writeBackToTosu(anchor, snapshot);
        markWritten(snapshot, anchor);
    }
    lastValues = { ...lastValues, ...snapshot, preset: anchor };
    notifyChanged();
}

// ---------------------------------------------------------------------------
// Preset lookup / application
// ---------------------------------------------------------------------------

function findBuiltinPresetByName(name) {
    return builtinPresets.find((preset) => preset.name === name) || null;
}

function findPresetByName(name) {
    return findBuiltinPresetByName(name)
        || customPresets.find((preset) => preset.name === name)
        || null;
}

/**
 * Applies a preset by name and syncs the result back to tosu.
 * "Default" resets to the factory snapshot (generated from settings.json values).
 * Unknown names are lazily materialized ONLY for the "Custom N" slots.
 */
export async function applyPresetByName(name) {
    await loadSettingsSchema();
    await loadBuiltinPresets();

    let snapshot;
    if (name === "Default") {
        snapshot = await buildDefaultSnapshot();
    } else {
        let preset = findPresetByName(name);
        if (!preset && DEFAULT_SLOT_NAMES.includes(name)) {
            createCustomPreset(name, await captureCurrentSettings());
            preset = findPresetByName(name);
        }
        if (!preset) {
            return false;
        }
        snapshot = preset.settings || {};
    }

    await applySnapshot(snapshot);
    currentPreset = name;
    persistActivePreset(name);
    if (shouldWriteBack(snapshot, name)) {
        writeBackToTosu(name, snapshot);
        markWritten(snapshot, name);
    }
    // Mirror the write-back into lastValues so the echo broadcast of the same
    // values is not mistaken for a manual settings change.
    lastValues = { ...lastValues, ...snapshot, preset: name };
    notifyChanged();
    return true;
}

// ---------------------------------------------------------------------------
// Custom preset CRUD
// ---------------------------------------------------------------------------

// User preset names: English letters, digits, underscore, hyphen, 1-40 chars.
// Fixed anchor slots ("Custom 1" etc.) are system-created and exempt.
const PRESET_NAME_RE = /^[A-Za-z0-9_-]{1,40}$/;

/** Converts a preset name into a stable slug id (lowercase, - separators). */
export function slugify(name) {
    return String(name || "")
        .toLowerCase()
        .replace(/[^a-z0-9_-]+/g, "-")
        .replace(/^-+|-+$/g, "")
        .slice(0, 48);
}

/**
 * Creates or updates (same-name overwrite) a user preset.
 * @param {string} name preset name (English letters/digits/_/-)
 * @param {object} snapshot partial settings snapshot
 * @param {{description?: string, version?: number}} [meta]
 */
export function createCustomPreset(name, snapshot, meta = {}) {
    const cleanName = String(name || "").trim();
    const isSystemSlot = DEFAULT_SLOT_NAMES.includes(cleanName);
    if (!cleanName || cleanName === "Custom" || cleanName === AUTO_SAVE_PRESET_NAME) {
        return null;
    }
    if (!isSystemSlot && !PRESET_NAME_RE.test(cleanName)) {
        return null;
    }
    if (findBuiltinPresetByName(cleanName)) {
        return null;
    }

    const version = normalizeVersion(meta.version);
    const existing = customPresets.find((preset) => preset.name === cleanName);
    if (existing) {
        existing.settings = snapshot || {};
        existing.description = String(meta.description ?? existing.description ?? "");
        existing.version = version;
        existing.updatedAt = Date.now();
    } else {
        const preset = {
            id: slugify(cleanName),
            name: cleanName,
            description: String(meta.description ?? ""),
            version,
            settings: snapshot || {},
            createdAt: Date.now(),
        };
        customPresets.push(preset);
    }

    persistLibrary();
    notifyChanged();
    return existing || customPresets[customPresets.length - 1];
}

/** Normalizes a version value to a positive integer (default 1). */
function normalizeVersion(value) {
    const num = Number(value);
    return Number.isInteger(num) && num > 0 ? num : 1;
}

/** Updates preset metadata (name/description/version) by id. Returns true on success. */
export function updatePresetMetadata(id, meta = {}) {
    const preset = customPresets.find((item) => item.id === id);
    if (!preset) {
        return false;
    }

    if (meta.name !== undefined) {
        const cleanName = String(meta.name).trim();
        if (!cleanName || cleanName === "Custom" || cleanName === AUTO_SAVE_PRESET_NAME) {
            return false;
        }
        if (!PRESET_NAME_RE.test(cleanName)) {
            return false;
        }
        if (findBuiltinPresetByName(cleanName)) {
            return false;
        }
        if (customPresets.some((item) => item.id !== id && item.name === cleanName)) {
            return false;
        }
        preset.name = cleanName;
        preset.id = slugify(cleanName);
    }
    if (meta.description !== undefined) {
        preset.description = String(meta.description ?? "");
    }
    if (meta.version !== undefined) {
        preset.version = normalizeVersion(meta.version);
    }

    persistLibrary();
    notifyChanged();
    return true;
}

/** Renames a user preset by id. Returns true on success. */
export function renameCustomPreset(id, newName) {
    const preset = customPresets.find((item) => item.id === id);
    if (!preset) {
        return false;
    }
    return updatePresetMetadata(id, { name: newName });
}

/** Deletes a user preset by id. Fixed anchor slots cannot be deleted. */
export function deleteCustomPreset(id) {
    const index = customPresets.findIndex((item) => item.id === id);
    if (index === -1) {
        return false;
    }
    if (DEFAULT_SLOT_NAMES.includes(customPresets[index].name)) {
        return false;
    }
    customPresets.splice(index, 1);
    persistLibrary();
    notifyChanged();
    return true;
}

/** Ensures the default "Custom 1..N" anchor slots exist. */
export async function ensureDefaultCustomSlots() {
    const snapshot = await captureCurrentSettings();
    for (const name of DEFAULT_SLOT_NAMES) {
        const existing = customPresets.some((preset) => preset.name === name);
        if (!existing && createCustomPreset(name, snapshot) === null) {
            break;
        }
    }
}

// ---------------------------------------------------------------------------
// Auto-save / follow mode
// ---------------------------------------------------------------------------

/**
 * Auto-save the current configuration after a dashboard settings change:
 * anchored custom preset -> overwrite it; otherwise -> Last Saved Preset.
 */
export async function autoSaveCurrentPreset() {
    // The broadcast payload (lastValues) is the ONLY source: every page of
    // this origin receives the same values, so snapshots built from it are
    // identical across pages (no divergent write-back loops).
    const snapshot = { ...lastValues };

    const anchored = customPresets.find((preset) => preset.name === currentPreset);
    if (anchored) {
        anchored.settings = snapshot;
        anchored.updatedAt = Date.now();
        persistLibrary();
        notifyChanged();
        if (!recentlyWritten() && shouldWriteBack(snapshot, anchored.name)) {
            writeBackToTosu(anchored.name, snapshot);
            markWritten(snapshot, anchored.name);
        }
        lastValues = { ...lastValues, ...snapshot, preset: anchored.name };
        return;
    }

    await saveToLastSavedPreset();
}

/** Overwrites ONLY the "Last Saved Preset" container and moves the picker there. */
export async function saveToLastSavedPreset() {
    const snapshot = { ...lastValues };
    updateAutoContainer(snapshot);
    persistLibrary();
    currentPreset = AUTO_SAVE_PRESET_NAME;
    persistActivePreset();
    notifyChanged();
    if (!recentlyWritten() && shouldWriteBack(snapshot, AUTO_SAVE_PRESET_NAME)) {
        writeBackToTosu(AUTO_SAVE_PRESET_NAME, snapshot);
        markWritten(snapshot, AUTO_SAVE_PRESET_NAME);
    }
    lastValues = { ...lastValues, ...snapshot, preset: AUTO_SAVE_PRESET_NAME };
}

/** Creates or updates the fixed "Last Saved Preset" container in memory. */
function updateAutoContainer(snapshot) {
    const auto = customPresets.find((preset) => preset.name === AUTO_SAVE_PRESET_NAME);
    if (auto) {
        auto.settings = snapshot;
        auto.updatedAt = Date.now();
        return auto;
    }
    const created = {
        id: `auto-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`,
        name: AUTO_SAVE_PRESET_NAME,
        settings: snapshot,
        createdAt: Date.now(),
    };
    customPresets.push(created);
    return created;
}

// ---------------------------------------------------------------------------
// Persistence (presetStorage + localStorage cache)
// ---------------------------------------------------------------------------

function persistLibrary() {
    cacheLibrary(customPresets);
    writeLibraryToTosu();
}

/** Writes the library into the presetStorage tosu setting (values.json). */
function writeLibraryToTosu() {
    // Only the browser page (127.0.0.1) writes back; in-game overlays load from
    // localhost (different origin) and stay read-only.
    if (window.location.hostname !== "127.0.0.1") {
        return;
    }
    const folderName = typeof window.COUNTER_PATH === "string"
        ? window.COUNTER_PATH.trim()
        : "";
    if (!folderName) {
        return;
    }
    fetch(`/api/counters/settings/${encodeURIComponent(folderName)}`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify([{
            uniqueID: PRESET_STORAGE_SETTING,
            value: serializeLibrary(customPresets),
        }]),
    }).catch(() => {
        // Best-effort sync; the library is still cached locally.
    });
}

// ---------------------------------------------------------------------------
// tosu write-back (preset apply echo)
// ---------------------------------------------------------------------------

function writeBackToTosu(presetName, snapshot) {
    if (window.location.hostname !== "127.0.0.1") {
        return;
    }
    const folderName = typeof window.COUNTER_PATH === "string"
        ? window.COUNTER_PATH.trim()
        : "";
    if (!folderName) {
        return;
    }
    const values = Object.keys(snapshot).map((key) => ({
        uniqueID: key,
        value: snapshot[key],
    }));
    values.push({ uniqueID: "preset", value: presetName });

    fetch(`/api/counters/settings/${encodeURIComponent(folderName)}`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(values),
    }).catch(() => {
        // Write-back is a best-effort sync; preset application still worked.
    });
}

function shouldWriteBack(snapshot, presetName) {
    const list = readLastWritten();
    const last = list && list[0];
    if (!last || last.presetName !== presetName) {
        return true;
    }
    for (const key of Object.keys(snapshot)) {
        if (snapshot[key] !== last.snapshot[key]) {
            return true;
        }
    }
    return false;
}

// ---------------------------------------------------------------------------
// tosu settings stream (own /websocket/commands connection)
// ---------------------------------------------------------------------------

function extractSettingsPayload(packet) {
    if (Array.isArray(packet)) {
        return packet;
    }
    if (packet && typeof packet === "object" && packet.command === "getSettings") {
        return packet.message;
    }
    return null;
}

function extractPresetValue(payload) {
    if (Array.isArray(payload)) {
        const item = payload.find((entry) => entry?.uniqueID === "preset");
        return typeof item?.value === "string" && item.value.trim() ? item.value.trim() : null;
    }
    if (payload && typeof payload === "object") {
        const value = payload.preset;
        return typeof value === "string" && value.trim() ? value.trim() : null;
    }
    return null;
}

function snapshotOf(payload) {
    if (Array.isArray(payload)) {
        const out = {};
        for (const entry of payload) {
            if (entry && typeof entry.uniqueID === "string") {
                out[entry.uniqueID] = entry.value;
            }
        }
        return out;
    }
    return { ...(payload || {}) };
}

function hasKeyChanged(prev, next, key) {
    return Object.prototype.hasOwnProperty.call(next, key)
        && next[key] !== prev[key];
}

async function handleSettingsPacket(packet) {
    const payload = extractSettingsPayload(packet);
    if (!payload) {
        return;
    }

    const presetValue = extractPresetValue(payload);

    if (lastValues === null) {
        // First batch: record baseline, load the library, restore active preset.
        lastValues = snapshotOf(payload);
        customPresets = loadLibrary(payload);
        cacheLibrary(customPresets);

        if (presetValue && presetValue !== currentPreset) {
            if (presetValue === "Default" || !(await applyPresetByName(presetValue))) {
                currentPreset = "Default";
                persistActivePreset();
                notifyChanged();
            }
        }
        return;
    }

    const prev = lastValues;
    lastValues = snapshotOf(payload);

    // Sync the library when another page changed presetStorage.
    const library = libraryFromPayload(payload);
    if (library !== null && serializeLibrary(customPresets) !== serializeLibrary(library)) {
        customPresets = library;
        cacheLibrary(customPresets);
        notifyChanged();
    }

    // True when the user actually changed settings in the dashboard.
    const { keys } = await loadSettingsSchema();
    const hasManualChange = keys
        .filter((key) => !IGNORED_DIFF_KEYS.has(key))
        .some((key) => hasKeyChanged(prev, lastValues, key));

    // Echo broadcast: the payload matches a recent write-back of the same
    // preset. Delayed echoes must not be treated as picker switches.
    const lastWrittenNow = readLastWritten();
    const isWriteBackEcho = Boolean(lastWrittenNow) && lastWrittenNow.some((record) =>
        record.presetName === presetValue
        && Object.keys(record.snapshot).every((key) =>
            !(key in record.snapshot) || record.snapshot[key] === lastValues[key]));

    if (presetValue && presetValue !== currentPreset && !isWriteBackEcho) {
        if (presetValue === AUTO_SAVE_PRESET_NAME) {
            if (hasManualChange) {
                await saveToLastSavedPreset();
            } else {
                currentPreset = AUTO_SAVE_PRESET_NAME;
                persistActivePreset();
                notifyChanged();
            }
            return;
        }

        const isCustom = customPresets.some((preset) => preset.name === presetValue);
        if (isCustom) {
            if (hasManualChange) {
                await overwriteCustomPreset(presetValue);
            } else if (!(await applyPresetByName(presetValue))) {
                currentPreset = "Default";
                persistActivePreset();
                notifyChanged();
            }
            return;
        }

        // Built-in (read-only) preset, including "Default".
        if (hasManualChange) {
            // Edits with a built-in preset selected become the new Auto preset.
            await saveToLastSavedPreset();
        } else if (!(await applyPresetByName(presetValue))) {
            currentPreset = "Default";
            persistActivePreset();
            notifyChanged();
        }
        return;
    }

    // The picker stayed on the same preset: any change is an edit of whatever
    // is selected -> auto-save. Write-back echoes never count as edits.
    if (hasManualChange && !isWriteBackEcho) {
        await autoSaveCurrentPreset();
    }
}

/** User edited settings with a custom preset selected: overwrite that preset. */
async function overwriteCustomPreset(presetValue) {
    const snapshot = { ...lastValues };
    const target = customPresets.find((preset) => preset.name === presetValue);
    if (target) {
        target.settings = snapshot;
        target.updatedAt = Date.now();
    }
    updateAutoContainer(snapshot);
    persistLibrary();
    currentPreset = presetValue;
    persistActivePreset();
    notifyChanged();
    if (!recentlyWritten() && shouldWriteBack(snapshot, presetValue)) {
        writeBackToTosu(presetValue, snapshot);
        markWritten(snapshot, presetValue);
    }
    lastValues = { ...lastValues, ...snapshot, preset: presetValue };
}

// ---------------------------------------------------------------------------
// Init (side-effect import in main.js / manager page)
// ---------------------------------------------------------------------------

export function initPresets() {
    if (initialized) {
        return;
    }
    initialized = true;

    currentPreset = loadActivePreset();
    ensureDefaultCustomSlots();

    // Observe the tosu settings stream on our own commands connection.
    socket.commands((packet) => {
        handleSettingsPacket(packet);
    });

    // Cross-page sync via localStorage events (presetStorage writes arrive
    // through the settings stream instead; this covers cache/active changes).
    window.addEventListener("storage", (event) => {
        if (event.key === CUSTOM_PRESETS_KEY || event.key === ACTIVE_PRESET_KEY) {
            customPresets = loadLibrary(lastValues || {});
            if (event.key === ACTIVE_PRESET_KEY) {
                currentPreset = loadActivePreset();
            }
            notifyChanged();
        }
    });
}
