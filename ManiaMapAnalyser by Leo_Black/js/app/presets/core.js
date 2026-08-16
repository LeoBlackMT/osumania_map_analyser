/**
 * Preset system core: application logic, tosu settings-stream handling,
 * echo guard and write-back. UI-agnostic — the manager page renders through
 * the exported state/action API and onPresetsChanged notifications.
 */

import { socket, state } from "../appContext.js";
import { getCounterPathForCommand } from "../settings.js";
import {
    loadSettingsSchema,
    buildDefaultSnapshot,
} from "./schema.js";
import {
    AUTO_SAVE_PRESET_NAME,
    DEFAULT_SLOT_NAMES,
    PRESET_STORAGE_SETTING,
    SYSTEM_SNAPSHOT_KEYS,
    storeFromPayload,
    rawStoreFingerprint,
    serializeStore,
    storeFingerprint,
    normalizeLibrary,
    recentlyWritten,
    markWritten,
} from "./storage.js";
import {
    captureCurrentSettings,
    applySnapshot,
    snapshotOf,
    stripSystemKeys,
    hasKeyChanged,
} from "./snapshot.js";
import {
    getBuiltinPresets,
    getBuiltinSettings,
    findBuiltinPresetByName,
    loadBuiltinPresets,
} from "./builtin.js";

// Re-export split-out APIs so existing importers (manager.js etc.) keep working
// unchanged after the module split.
export { getBuiltinPresets, getBuiltinSettings } from "./builtin.js";
export { captureCurrentSettings, applySnapshot } from "./snapshot.js";

// ---------------------------------------------------------------------------
// Module state
// ---------------------------------------------------------------------------

let customPresets = [];
let lastWritten = []; // broadcast-shared write-back dedup queue
let currentPreset = "Default";
let lastValues = null;
let initialized = false;

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
            snapshot = getBuiltinSettings(builtin.id) || {};
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

/** Ensures the default "Custom1..N" anchor slots exist (single write-back). */
export async function ensureDefaultCustomSlots() {
    const snapshot = await captureCurrentSettings();
    let createdAny = false;
    for (const name of DEFAULT_SLOT_NAMES) {
        if (customPresets.some((preset) => preset.name === name)) {
            continue;
        }
        customPresets.push({
            id: uniquePresetId(name),
            name,
            description: "Fixed slot — Apply captures the current configuration.",
            version: 1,
            settings: { ...snapshot },
            createdAt: Date.now(),
        });
        createdAny = true;
    }
    if (createdAny) {
        // Batch creation: one persist instead of one POST per slot.
        persistLibrary();
        notifyChanged();
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
 * @returns {Promise<{store: {presets: Array, lastWritten: Array}, raw: string|null}|null>}
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
        const values = (data && data.values) || {};
        return {
            store: storeFromPayload(values),
            raw: typeof values[PRESET_STORAGE_SETTING] === "string"
                ? values[PRESET_STORAGE_SETTING]
                : null,
        };
    } catch {
        return null;
    }
}

/**
 * Applies a freshly loaded store into module state and self-heals historical
 * pollution: when the raw store (as it lives in tosu) differs from the cleaned
 * in-memory store, exactly one write-back is forced to shrink values.json.
 * Callers must guard on `lastValues === null` (never overwrite an already
 * loaded authoritative state). A re-entrancy flag prevents the HTTP fallback
 * and the settings broadcast from both racing through slot creation.
 */
let storeApplyInFlight = false;

function applyLoadedStore(store, raw) {
    if (!store || storeApplyInFlight || lastValues !== null) {
        return;
    }
    storeApplyInFlight = true;
    customPresets = normalizeLibrary(store.presets);
    lastWritten = store.lastWritten;
    ensureDefaultCustomSlots().then(() => {
        const rawFingerprint = raw ? rawStoreFingerprint(raw) : null;
        lastPersistedFingerprint = currentStoreFingerprint();
        if (rawFingerprint !== null && rawFingerprint !== lastPersistedFingerprint) {
            // Historical pollution stripped on load -> force the write-back
            // that shrinks values.json back to its real content.
            if (writeLibraryToTosu()) {
                lastPersistedFingerprint = currentStoreFingerprint();
            }
        } else {
            persistLibrary();
        }
        storeApplyInFlight = false;
        notifyChanged();
    });
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

async function handleSettingsPacket(packet) {
    const payload = extractSettingsPayload(packet);
    if (!payload) {
        return;
    }

    const presetValue = extractPresetValue(payload);

    if (lastValues === null) {
        // First batch: record baseline, load the store (library + lastWritten),
        // restore the active preset from the picker value.
        // If the HTTP fallback already applied the store, skip (re-entrancy).
        if (storeApplyInFlight) {
            return;
        }
        lastValues = snapshotOf(payload);
        const rawStore = rawStoreFromPayload(payload);
        const store = storeFromPayload(payload);
        customPresets = normalizeLibrary(store ? store.presets : []);
        lastWritten = store ? store.lastWritten : [];
        // Create missing anchor slots now that the authoritative library is
        // loaded (never persist an empty library over an existing one).
        await ensureDefaultCustomSlots();
        // Self-heal historical store pollution and flush local changes with a
        // SINGLE write-back. NOTE: the fingerprint guard is baselined BEFORE
        // the comparison, so persistLibrary() alone would be a no-op — the
        // self-heal must write directly when the raw store differs from the
        // cleaned in-memory store.
        const rawFingerprint = rawStore ? rawStoreFingerprint(rawStore) : null;
        lastPersistedFingerprint = currentStoreFingerprint();
        if (rawFingerprint !== null && rawFingerprint !== lastPersistedFingerprint) {
            // Historical pollution stripped on load -> force the write-back
            // that shrinks values.json back to its real content.
            if (writeLibraryToTosu()) {
                lastPersistedFingerprint = currentStoreFingerprint();
            }
        } else {
            // Nothing structurally different; only persist if anchor slots
            // (or other in-memory changes) actually modified the store.
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
    // 10+ preset JSON files there wastes bandwidth and memory on every load,
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
    fetchStoreFromTosu().then((result) => {
        if (result !== null && lastValues === null) {
            applyLoadedStore(result.store, result.raw);
        }
    });

    // Fallback: if neither the broadcast nor the HTTP pull delivered anything
    // (tosu offline), start with an empty library after a short grace period.
    // The persist guard keeps this read-only (no overwriting presetStorage).
    setTimeout(async () => {
        if (lastValues !== null || customPresets.length > 0) {
            return;
        }
        const result = await fetchStoreFromTosu();
        if (result !== null) {
            applyLoadedStore(result.store, result.raw);
        } else {
            ensureDefaultCustomSlots();
            notifyChanged();
        }
    }, 3000);
}
