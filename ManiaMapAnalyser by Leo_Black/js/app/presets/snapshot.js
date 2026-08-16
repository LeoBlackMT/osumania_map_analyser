/**
 * Snapshot utilities for the preset system — pure functions with no dependency
 * on the preset module state (customPresets/lastWritten/lastValues). Split out
 * of core.js to keep module sizes manageable.
 */

import {
    SETTING_RECOMPUTE_KEYS,
    SETTING_CACHE_KEYS,
} from "../settings.js";
import { clearResultCache } from "../resultCache.js";
import { scheduleRecompute } from "../scheduler.js";
import {
    SYSTEM_SNAPSHOT_KEYS,
} from "./storage.js";
import {
    loadSettingsSchema,
    getterFor,
} from "./schema.js";

// ---------------------------------------------------------------------------
// Payload → settings map
// ---------------------------------------------------------------------------

/**
 * Converts a settings payload (tosu getSettings array or object) into a plain
 * settings map, stripping the system keys (presetStorage/preset) so they never
 * leak into preset snapshots or lastValues.
 */
export function snapshotOf(payload) {
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
export function stripSystemKeys(values) {
    const out = { ...values };
    for (const key of SYSTEM_SNAPSHOT_KEYS) {
        delete out[key];
    }
    return out;
}

/** True when `key` changed between two settings maps (present in next, differs from prev). */
export function hasKeyChanged(prev, next, key) {
    return Object.prototype.hasOwnProperty.call(next, key)
        && next[key] !== prev[key];
}

// ---------------------------------------------------------------------------
// Full snapshot capture / apply
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
