/**
 * Self-extending settings schema for the preset system.
 *
 * Fetches the plugin's own settings.json and dynamically registers an applier
 * for every setting whose settings.js exports an `apply{Key}Setting` function
 * (naming convention, e.g. "contentBar" -> applyContentBarSetting,
 * "VibroDetection" -> applyVibroDetectionSetting). Keys without an applier
 * (headers, buttons) are skipped automatically, so adding a new setting to
 * settings.json + settings.js needs NO change here.
 */

import * as settingsModule from "../settings.js";
import { state } from "../appContext.js";

// Keys whose apply function name deviates from the convention.
const APPLIER_NAME_OVERRIDES = {
    // uniqueID "enablePauseDetection" -> settings.js exports applyPauseDetectionSetting.
    enablePauseDetection: "applyPauseDetectionSetting",
};

// Keys whose state field name deviates from the uniqueID (read side).
const GETTER_OVERRIDES = {
    // user* fields hold the raw user preference (resolved values live in the base field).
    contentBar: (s) => s.userContentBar,
    srText: (s) => s.userSrText,
    diffText: (s) => s.userDiffText,
    enablePauseDetection: (s) => s.pauseDetectionEnabled,
    VibroDetection: (s) => s.vibroDetection,
};

let built = null;

/**
 * Loads and caches the settings schema.
 * @returns {Promise<{entries: Array, appliers: Map<string, Function>, defaults: Object, keys: string[]}>}
 */
export async function loadSettingsSchema() {
    if (built) {
        return built;
    }

    const response = await fetch("./settings.json", { cache: "no-store" });
    const entries = await response.json();

    const appliers = new Map();
    const defaults = {};
    for (const entry of entries) {
        const key = entry?.uniqueID;
        if (!key || entry.type === "header" || entry.type === "button") {
            continue;
        }
        const applyFn = applierFor(key);
        if (!applyFn) {
            continue;
        }
        appliers.set(key, applyFn);
        defaults[key] = entry.value;
    }

    built = {
        entries,
        appliers,
        defaults,
        keys: [...appliers.keys()],
    };
    return built;
}

/** Resolves the apply function for a setting key (convention + overrides). */
function applierFor(key) {
    const name = APPLIER_NAME_OVERRIDES[key]
        || `apply${key.charAt(0).toUpperCase()}${key.slice(1)}Setting`;
    return settingsModule[name] || null;
}

/** Returns a getter that reads the CURRENT user value of a setting key from state. */
export function getterFor(key) {
    const override = GETTER_OVERRIDES[key];
    return override ? () => override(state) : () => state[key];
}

/**
 * Builds the factory-default snapshot from settings.json `value` fields.
 * Used by the "Default" preset (resets everything) and as the merge base
 * for built-in presets that only carry overrides.
 */
export async function buildDefaultSnapshot() {
    const { defaults } = await loadSettingsSchema();
    return { ...defaults };
}
