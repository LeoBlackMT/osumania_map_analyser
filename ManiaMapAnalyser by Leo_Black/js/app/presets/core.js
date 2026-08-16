/**
 * Preset system core: application logic, tosu settings-stream handling,
 * echo guard and write-back. UI-agnostic — the manager page renders through
 * the exported state/action API and onPresetsChanged notifications.
 */

import { socket, state } from "../appContext.js";
import {
    SETTING_RECOMPUTE_KEYS,
    SETTING_CACHE_KEYS,
    getCounterPathForCommand,
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
    DEFAULT_SLOT_NAMES,
    PRESET_STORAGE_SETTING,
    SYSTEM_SNAPSHOT_KEYS,
    parseStore,
    storeFromPayload,
    rawStoreFingerprint,
    serializeStore,
    storeFingerprint,
    normalizeLibrary,
    recentlyWritten,
    markWritten,
} from "./storage.js";

// ---------------------------------------------------------------------------
// Module state
// ---------------------------------------------------------------------------

let customPresets = [];
let lastWritten = []; // broadcast-shared write-back dedup queue
let currentPreset = "Default";
let lastValues = null;
let initialized = false;
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

/** True once the settings stream delivered the authoritative library. */
export function isLibraryLoaded() {
    return lastValues !== null;
}

/** Registers a UI listener, returns an unsubscribe function. */
export function onPresetsChanged(callback) {
    listeners.add(callback);
    return () => listeners.delete(callback);
}

// ---------------------------------------------------------------------------
// Built-in presets (presets/*.json)
// ---------------------------------------------------------------------------

let builtinPromise = null;

/**
 * Loads built-in presets (index.json + per-preset files). Concurrent callers
 * share one promise so nobody observes a half-loaded list.
 */
function loadBuiltinPresets() {
    if (!builtinPromise) {
        builtinPromise = doLoadBuiltinPresets().catch((error) => {
            builtinPromise = null;
            throw error;
        });
    }
    return builtinPromise;
}

async function doLoadBuiltinPresets() {
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
 *
 * Each key is applied defensively: apply functions may touch overlay DOM
 * elements that do not exist on the manager page (presets.html) — a failure
 * must never abort the rest of the snapshot. State changes made before the
 * failure still count (the write-back + broadcast re-render on the overlay).
 */
export async function applySnapshot(snapshot) {
    const { appliers } = await loadSettingsSchema();
    let recomputeNeeded = false;
    let cacheNeeded = false;

    for (const [key, value] of Object.entries(snapshot)) {
        // wsEndpoint is a connection parameter — applying a preset must never
        // drop or change the socket connection.
        if (key === "wsEndpoint") {
            continue;
        }
        const applier = appliers.get(key);
        if (!applier) {
            continue;
        }
        let changed = false;
        try {
            changed = applier(value) === true;
        } catch (error) {
            console.error(`[presets] apply "${key}" failed (DOM may be missing on this page):`, error);
            // The state may already have been updated — treat as changed so
            // recompute/cache invalidation still fire when needed.
            changed = true;
        }
        if (changed) {
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
    }
    if (shouldWriteBack(snapshot, anchor)) {
        markWritten(lastWritten, snapshot, anchor);
        writeBackToTosu(anchor, snapshot);
    }
    lastValues = { ...lastValues, ...stripSystemKeys(snapshot), preset: anchor };
    notifyChanged();
}

// ---------------------------------------------------------------------------
// Preset lookup / application
// ---------------------------------------------------------------------------

function findBuiltinPresetByName(name) {
    return builtinPresets.find((preset) => preset.name === name) || null;
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
        const builtin = findBuiltinPresetByName(name);
        if (builtin) {
            // Built-in entries only carry metadata; settings live in builtinCache.
            snapshot = builtinCache.get(builtin.id) || {};
        } else {
            let preset = customPresets.find((p) => p.name === name);
            if (!preset && DEFAULT_SLOT_NAMES.includes(name)) {
                createCustomPreset(name, await captureCurrentSettings());
                preset = customPresets.find((p) => p.name === name);
            }
            if (!preset) {
                return false;
            }
            snapshot = preset.settings || {};
        }
    }

    await applySnapshot(snapshot);
    currentPreset = name;
    if (shouldWriteBack(snapshot, name)) {
        markWritten(lastWritten, snapshot, name);
        writeBackToTosu(name, snapshot);
    }
    // Mirror the write-back into lastValues so the echo broadcast of the same
    // values is not mistaken for a manual settings change.
    lastValues = { ...lastValues, ...stripSystemKeys(snapshot), preset: name };
    notifyChanged();
    return true;
}

// ---------------------------------------------------------------------------
// Custom preset CRUD
// ---------------------------------------------------------------------------

// User preset names: English letters, digits, underscore, hyphen, 1-40 chars.
// Fixed anchor slots ("Custom1" etc.) are system-created and exempt.
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
            id: uniquePresetId(cleanName),
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

/** Generates a slug id that is unique within the custom library. */
function uniquePresetId(name, excludeId = null) {
    const taken = (id) => id !== excludeId && customPresets.some((preset) => preset.id === id);
    const base = slugify(name);
    if (!taken(base)) {
        return base;
    }
    let suffix = 2;
    while (taken(`${base}-${suffix}`)) {
        suffix += 1;
    }
    return `${base}-${suffix}`;
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
        preset.id = uniquePresetId(cleanName, id);
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

/** Ensures the default "Custom1..N" anchor slots exist. */
export async function ensureDefaultCustomSlots() {
    const snapshot = await captureCurrentSettings();
    for (const name of DEFAULT_SLOT_NAMES) {
        const existing = customPresets.some((preset) => preset.name === name);
        if (!existing && createCustomPreset(
            name,
            snapshot,
            { description: "Fixed slot — Apply captures the current configuration." },
        ) === null) {
            break;
        }
    }
}

// ---------------------------------------------------------------------------
// Auto-save / follow mode
// ---------------------------------------------------------------------------

/**
 * Auto-save the current configuration after a dashboard settings change:
 * anchored custom preset -> overwrite it; otherwise -> LastSavedPreset.
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
        notifyChanged();
        if (!recentlyWritten(lastWritten) && shouldWriteBack(snapshot, anchored.name)) {
            // Mark FIRST so the write-back POST ships the fresh lastWritten
            // queue; that single POST also persists the updated library
            // (store). One POST per change — no extra broadcasts.
            markWritten(lastWritten, snapshot, anchored.name);
            writeBackToTosu(anchored.name, snapshot);
        } else {
            persistLibrary();
        }
        lastValues = { ...lastValues, ...stripSystemKeys(snapshot), preset: anchored.name };
        return;
    }

    await saveToLastSavedPreset();
}

/** Overwrites ONLY the "LastSavedPreset" container and moves the picker there. */
export async function saveToLastSavedPreset() {
    const snapshot = { ...lastValues };
    updateAutoContainer(snapshot);
    currentPreset = AUTO_SAVE_PRESET_NAME;
    notifyChanged();
    if (!recentlyWritten(lastWritten) && shouldWriteBack(snapshot, AUTO_SAVE_PRESET_NAME)) {
        markWritten(lastWritten, snapshot, AUTO_SAVE_PRESET_NAME);
        writeBackToTosu(AUTO_SAVE_PRESET_NAME, snapshot);
    } else {
        persistLibrary();
    }
    lastValues = { ...lastValues, ...stripSystemKeys(snapshot), preset: AUTO_SAVE_PRESET_NAME };
}

/** Creates or updates the fixed "LastSavedPreset" container in memory. */
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
// Persistence (single authoritative store: presetStorage tosu setting)
// ---------------------------------------------------------------------------

// Fingerprint of the last store payload actually written to tosu. When the
// current store content matches it, persistLibrary() is a no-op — this is the
// guard that breaks the broadcast->POST->broadcast loop (every page receives
// the echo of its own write and must NOT write again).
let lastPersistedFingerprint = null;

function currentStoreFingerprint() {
    return storeFingerprint(customPresets, lastWritten);
}

function persistLibrary() {
    // Never write before the authoritative store arrived from the settings
    // stream: persisting the not-yet-loaded (empty) library would overwrite an
    // existing one in values.json.
    if (lastValues === null) {
        return;
    }
    const fingerprint = currentStoreFingerprint();
    if (fingerprint === lastPersistedFingerprint) {
        // Content unchanged — nothing to persist. Avoids the write-back echo
        // loop where each page re-POSTs what it just received.
        return;
    }
    if (writeLibraryToTosu()) {
        lastPersistedFingerprint = fingerprint;
    }
}

/**
 * Writes the library (+ lastWritten queue) into the presetStorage tosu setting.
 * Returns true when the POST was actually issued.
 */
function writeLibraryToTosu() {
    // Write-back happens only from a browser page (the manager page or the
    // overlay in a browser tab): localhost and 127.0.0.1 are both fine.
    // The in-game CEF overlay never opens presets.html, so it stays read-only.
    if (!isBrowserOrigin()) {
        return false;
    }
    const folderName = typeof window.COUNTER_PATH === "string"
        ? window.COUNTER_PATH.trim()
        : "";
    if (!folderName) {
        return false;
    }
    fetch(`/api/counters/settings/${encodeURIComponent(folderName)}`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify([{
            uniqueID: PRESET_STORAGE_SETTING,
            value: serializeStore(customPresets, lastWritten),
        }]),
    }).catch(() => {
        // Best-effort sync; the library stays in memory and re-syncs on next
        // successful write.
    });
    return true;
}

/** True when the page runs in a regular browser (localhost / 127.0.0.1). */
function isBrowserOrigin() {
    const host = window.location.hostname;
    return host === "127.0.0.1" || host === "localhost";
}

/**
 * Pulls the preset store straight from tosu's values file
 * (GET /api/counters/settings/<folder>). Origin-independent: localhost and
 * 127.0.0.1 read the same data here.
 * @returns {Promise<{presets: Array, lastWritten: Array}|null>}
 */
async function fetchStoreFromTosu() {
    if (!isBrowserOrigin()) {
        return null;
    }
    const folderName = typeof window.COUNTER_PATH === "string"
        ? window.COUNTER_PATH.trim()
        : "";
    if (!folderName) {
        return null;
    }
    try {
        const response = await fetch(
            `/api/counters/settings/${encodeURIComponent(folderName)}`,
            { cache: "no-store" },
        );
        if (!response.ok) {
            return null;
        }
        const data = await response.json();
        return storeFromPayload((data && data.values) || {});
    } catch {
        return null;
    }
}

// ---------------------------------------------------------------------------
// tosu write-back (preset apply echo)
// ---------------------------------------------------------------------------

function writeBackToTosu(presetName, snapshot) {
    if (!isBrowserOrigin()) {
        return;
    }
    const folderName = typeof window.COUNTER_PATH === "string"
        ? window.COUNTER_PATH.trim()
        : "";
    if (!folderName) {
        return;
    }
    const values = Object.keys(snapshot)
        .filter((key) => key !== "wsEndpoint" && !SYSTEM_SNAPSHOT_KEYS.has(key))
        .map((key) => ({
            uniqueID: key,
            value: snapshot[key],
        }));
    // Ship the store (library + lastWritten) in the same POST so every origin
    // sees the echo guard state through the broadcast.
    values.push({ uniqueID: "preset", value: presetName });
    values.push({
        uniqueID: PRESET_STORAGE_SETTING,
        value: serializeStore(customPresets, lastWritten),
    });

    fetch(`/api/counters/settings/${encodeURIComponent(folderName)}`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(values),
    }).catch(() => {
        // Write-back is a best-effort sync; preset application still worked.
    });
}

function shouldWriteBack(snapshot, presetName) {
    const last = lastWritten[0];
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

/**
 * Returns the RAW presetStorage string from a payload, WITHOUT sanitization.
 * Used to detect historical store pollution (system keys embedded in preset
 * settings) so the cleaned store can be written back exactly once.
 */
function rawStoreFromPayload(payload) {
    if (Array.isArray(payload)) {
        const entry = payload.find((item) => item?.uniqueID === PRESET_STORAGE_SETTING);
        return entry && typeof entry.value === "string" ? entry.value : null;
    }
    if (payload && typeof payload === "object") {
        return typeof payload[PRESET_STORAGE_SETTING] === "string"
            ? payload[PRESET_STORAGE_SETTING]
            : null;
    }
    return null;
}

function snapshotOf(payload) {
    if (Array.isArray(payload)) {
        const out = {};
        for (const entry of payload) {
            if (entry && typeof entry.uniqueID === "string"
                && !SYSTEM_SNAPSHOT_KEYS.has(entry.uniqueID)) {
                out[entry.uniqueID] = entry.value;
            }
        }
        return out;
    }
    const out = { ...payload };
    for (const key of SYSTEM_SNAPSHOT_KEYS) {
        delete out[key];
    }
    return out;
}

/**
 * Returns a copy of `values` with the system keys (presetStorage/preset)
 * stripped. Used wherever a settings snapshot is built for storage — those
 * keys must never end up inside a preset, or the store recursively embeds
 * itself and values.json explodes (observed: 264MB).
 */
function stripSystemKeys(values) {
    const out = { ...values };
    for (const key of SYSTEM_SNAPSHOT_KEYS) {
        delete out[key];
    }
    return out;
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
        // First batch: record baseline, load the store (library + lastWritten),
        // restore the active preset from the picker value.
        lastValues = snapshotOf(payload);
        const rawStore = rawStoreFromPayload(payload);
        const store = storeFromPayload(payload);
        customPresets = normalizeLibrary(store ? store.presets : []);
        lastWritten = store ? store.lastWritten : [];
        // Baseline the persist guard against the freshly loaded store so the
        // first persistLibrary() below only fires when something actually
        // changed (e.g. anchor slots were missing).
        lastPersistedFingerprint = currentStoreFingerprint();
        // Create missing anchor slots now that the authoritative library is
        // loaded (never persist an empty library over an existing one).
        await ensureDefaultCustomSlots();
        // Self-heal historical store pollution: if the store as it lives in
        // tosu contained system keys embedded in preset settings (which
        // recursively embedded the store and inflated values.json to hundreds
        // of MB), the cleaned in-memory store differs -> force ONE write-back
        // to shrink it back to its real content.
        const rawFingerprint = rawStore ? rawStoreFingerprint(rawStore) : null;
        if (rawFingerprint !== null && rawFingerprint !== lastPersistedFingerprint) {
            persistLibrary();
            lastPersistedFingerprint = currentStoreFingerprint();
        } else {
            // Flush any in-memory changes made before the first batch arrived
            // (no-op when nothing changed — see the fingerprint guard).
            persistLibrary();
        }
        // Always notify: the UI may have rendered before this first batch.
        notifyChanged();

        if (presetValue && presetValue !== currentPreset) {
            if (presetValue === "Default" || !(await applyPresetByName(presetValue))) {
                currentPreset = "Default";
                notifyChanged();
            }
        }
        return;
    }

    const prev = lastValues;
    lastValues = snapshotOf(payload);

    // Sync the store (library + lastWritten) when another page changed it —
    // this is the single cross-origin source of truth via the broadcast.
    const store = storeFromPayload(payload);
    if (store !== null) {
        customPresets = normalizeLibrary(store.presets);
        lastWritten = store.lastWritten;
        // The broadcast content IS the persisted state (it came from tosu) —
        // align the persist guard so this page does not immediately re-POST
        // the echo it just received (breaks the broadcast->POST->broadcast loop).
        lastPersistedFingerprint = currentStoreFingerprint();
        notifyChanged();
    }

    // True when the user actually changed settings in the dashboard.
    const { keys } = await loadSettingsSchema();
    const hasManualChange = keys
        .filter((key) => !IGNORED_DIFF_KEYS.has(key))
        .some((key) => hasKeyChanged(prev, lastValues, key));

    // Echo broadcast: the payload matches a recent write-back (shared
    // lastWritten queue). Matches on SETTINGS CONTENT, not on the preset field
    // — a dashboard save broadcasts without a "preset" entry, and treating that
    // as a fresh picker switch/auto-save on EVERY page would re-POST forever.
    const isWriteBackEcho = lastWritten.some((record) => {
        const snapshot = record && record.snapshot ? record.snapshot : null;
        if (!snapshot || Object.keys(snapshot).length === 0) {
            return false;
        }
        return Object.keys(snapshot).every((key) =>
            lastValues[key] === snapshot[key]);
    });

    if (presetValue && presetValue !== currentPreset && !isWriteBackEcho) {
        if (presetValue === AUTO_SAVE_PRESET_NAME) {
            if (hasManualChange) {
                await saveToLastSavedPreset();
            } else {
                currentPreset = AUTO_SAVE_PRESET_NAME;
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
    currentPreset = presetValue;
    notifyChanged();
    if (!recentlyWritten(lastWritten) && shouldWriteBack(snapshot, presetValue)) {
        markWritten(lastWritten, snapshot, presetValue);
        writeBackToTosu(presetValue, snapshot);
    } else {
        persistLibrary();
    }
    lastValues = { ...lastValues, ...stripSystemKeys(snapshot), preset: presetValue };
}

// ---------------------------------------------------------------------------
// Init (side-effect import in main.js / manager page)
// ---------------------------------------------------------------------------

export function initPresets() {
    if (initialized) {
        return;
    }
    initialized = true;

    // The active preset name comes from the tosu picker value (broadcast) —
    // no browser-side state needed.
    // Anchor slots are created AFTER the first settings broadcast loads the
    // library (see handleSettingsPacket) — creating them here would persist an
    // empty library over an existing one.

    // Load built-in presets eagerly ONLY on the manager page, where the list
    // is rendered. The game overlay (index.html, also loaded inside the tosu
    // in-game CEF iframe) never shows the manager list — eagerly fetching the
    // 12+ preset JSON files there wastes bandwidth and memory on every load,
    // which amplifies the crash-reload loop seen in production. applyPresetByName
    // still loads them lazily when a built-in preset is actually applied.
    if (typeof window !== "undefined" && /presets\.html/i.test(window.location.pathname || "")) {
        loadBuiltinPresets().then(() => {
            notifyChanged();
        });
    }

    // Observe the tosu settings stream on our own commands connection.
    socket.commands((packet) => {
        handleSettingsPacket(packet).catch((error) => {
            console.error("[presets] settings stream handler failed:", error);
        });
    });

    // The manager page does not go through loadSettings(), so request the
    // settings stream explicitly (idempotent — duplicates are harmless).
    if (typeof socket.sendCommand === "function") {
        socket.sendCommand("getSettings", getCounterPathForCommand());
    }

    // Eagerly pull the authoritative store straight from tosu. This is
    // origin-independent: localhost and 127.0.0.1 are DIFFERENT origins, and
    // tosu's values.json is the single cross-origin source of truth. The
    // settings broadcast (when it arrives) remains authoritative and wins.
    fetchStoreFromTosu().then((store) => {
        if (store !== null && lastValues === null) {
            customPresets = normalizeLibrary(store.presets);
            lastWritten = store.lastWritten;
            ensureDefaultCustomSlots();
            notifyChanged();
        }
    });

    // Fallback: if neither the broadcast nor the HTTP pull delivered anything
    // (tosu offline), start with an empty library after a short grace period.
    // The persist guard keeps this read-only (no overwriting presetStorage).
    setTimeout(async () => {
        if (lastValues !== null || customPresets.length > 0) {
            return;
        }
        const store = await fetchStoreFromTosu();
        if (store !== null) {
            customPresets = normalizeLibrary(store.presets);
            lastWritten = store.lastWritten;
        }
        ensureDefaultCustomSlots();
        notifyChanged();
    }, 3000);
}
