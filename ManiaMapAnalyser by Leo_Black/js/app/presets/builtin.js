/**
 * Built-in preset loading (presets/*.json) — self-contained state, split out
 * of core.js to keep module sizes manageable.
 */

let builtinPresets = []; // [{ id, name, description, file }]
let builtinCache = new Map(); // id -> settings object
let builtinPromise = null;

/** Returns the list of built-in preset metadata entries. */
export function getBuiltinPresets() {
    return builtinPresets;
}

/** Returns the settings of a built-in preset by id (null when unavailable). */
export function getBuiltinSettings(id) {
    return builtinCache.get(id) || null;
}

/** Finds a built-in preset by display name (null when not found). */
export function findBuiltinPresetByName(name) {
    return builtinPresets.find((preset) => preset.name === name) || null;
}

/**
 * Loads built-in presets (index.json + per-preset files). Concurrent callers
 * share one promise so nobody observes a half-loaded list.
 */
export function loadBuiltinPresets() {
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
