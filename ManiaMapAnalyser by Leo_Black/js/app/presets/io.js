/**
 * Preset export / import for sharing between players.
 * Formats:
 *  - single preset v2: { format: "mma-preset", version: 2, preset: {id, name, description, version, settings}, exportedAt }
 *  - collection v2:    { format: "mma-preset-collection", version: 2, presets: [...] }
 *  - v1 (name + settings at top level) is accepted on import.
 */

import {
    getCustomPresets,
    createCustomPreset,
} from "./core.js";

const FORMAT_SINGLE = "mma-preset";
const FORMAT_COLLECTION = "mma-preset-collection";

function safeFileName(name) {
    return String(name || "preset")
        .replace(/[\\/:*?"<>|]/g, "_")
        .slice(0, 60);
}

function download(text, fileName) {
    const blob = new Blob([text], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const link = document.createElement("a");
    link.href = url;
    link.download = fileName;
    document.body.appendChild(link);
    link.click();
    link.remove();
    URL.revokeObjectURL(url);
}

function singlePayload(name, description, version, settings) {
    return {
        format: FORMAT_SINGLE,
        version: 2,
        preset: {
            name,
            description: description || "",
            version: Number.isInteger(version) && version > 0 ? version : 1,
            settings: settings || {},
        },
        exportedAt: new Date().toISOString(),
    };
}

/** Exports one custom preset as a downloadable json file. */
export function exportPresetToFile(preset) {
    download(
        JSON.stringify(singlePayload(preset.name, preset.description, preset.version, preset.settings), null, 2),
        `${safeFileName(preset.name)}.json`,
    );
}

/** Exports the currently edited (unsaved) preset as a downloadable json file. */
export function exportCurrentToFile(name, description, version, settings) {
    download(
        JSON.stringify(singlePayload(name, description, version, settings), null, 2),
        `${safeFileName(name || "preset")}.json`,
    );
}

/** Exports the whole custom library as a downloadable json file. */
export function exportLibraryToFile() {
    const data = {
        format: FORMAT_COLLECTION,
        version: 2,
        presets: getCustomPresets()
            .filter((preset) => preset.name !== "Last Saved Preset")
            .map((preset) => ({
                name: preset.name,
                description: preset.description || "",
                version: preset.version || 1,
                settings: preset.settings,
                exportedAt: preset.updatedAt || preset.createdAt,
            })),
        exportedAt: new Date().toISOString(),
    };
    download(JSON.stringify(data, null, 2), "mma-presets.json");
}

function importSingle(name, description, version, settings) {
    const cleanName = String(name || "").trim();
    if (!cleanName || !settings || typeof settings !== "object") {
        throw new Error("Invalid preset: missing name or settings.");
    }
    const created = createCustomPreset(cleanName, settings, { description, version });
    if (!created) {
        throw new Error(`Preset "${cleanName}" could not be created (reserved, invalid name, or duplicate system name).`);
    }
    return created.name;
}

/**
 * Imports a preset (or collection) file into the library.
 * @returns {Promise<{ok: boolean, message: string}>}
 */
export async function importPresetFromFile(file) {
    let parsed;
    try {
        parsed = JSON.parse(await file.text());
    } catch {
        return { ok: false, message: "Not a valid JSON file." };
    }

    try {
        // Collection (v1/v2 share the same shape).
        if (parsed?.format === FORMAT_COLLECTION && Array.isArray(parsed.presets)) {
            if (parsed.presets.length === 0) {
                return { ok: false, message: "The collection is empty." };
            }
            const names = parsed.presets.map((item) => {
                const p = item?.preset || item;
                return importSingle(p.name, p.description, p.version, p.settings);
            });
            return { ok: true, message: `Imported ${names.length} presets: ${names.join(", ")}.` };
        }

        // Single preset (v2 wraps in .preset; v1 has fields at top level).
        if (parsed?.format === FORMAT_SINGLE) {
            const p = parsed.preset || parsed;
            const name = importSingle(p.name, p.description, p.version, p.settings);
            return { ok: true, message: `Preset "${name}" imported.` };
        }

        return { ok: false, message: "Unrecognized preset file format." };
    } catch (error) {
        return { ok: false, message: error.message };
    }
}
