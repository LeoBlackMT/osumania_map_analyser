/**
 * Preset export / import for sharing between players.
 * Formats:
 *  - single preset: { format: "mma-preset", version: 1, name, settings, exportedAt }
 *  - collection:    { format: "mma-preset-collection", version: 1, presets: [...] }
 */

import {
    getCustomPresets,
    createCustomPreset,
} from "./core.js";

const FORMAT_SINGLE = "mma-preset";
const FORMAT_COLLECTION = "mma-preset-collection";
const FORMAT_VERSION = 1;

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

/** Exports one custom preset as a downloadable json file. */
export function exportPresetToFile(preset) {
    const data = {
        format: FORMAT_SINGLE,
        version: FORMAT_VERSION,
        name: preset.name,
        settings: preset.settings,
        exportedAt: new Date().toISOString(),
    };
    download(JSON.stringify(data, null, 2), `${safeFileName(preset.name)}.json`);
}

/** Exports the whole custom library as a downloadable json file. */
export function exportLibraryToFile() {
    const data = {
        format: FORMAT_COLLECTION,
        version: FORMAT_VERSION,
        presets: getCustomPresets().map((preset) => ({
            name: preset.name,
            settings: preset.settings,
            exportedAt: preset.updatedAt || preset.createdAt,
        })),
        exportedAt: new Date().toISOString(),
    };
    download(JSON.stringify(data, null, 2), "mma-presets.json");
}

function importSingle(data) {
    const name = String(data.name || "").trim();
    if (!name || !data.settings || typeof data.settings !== "object") {
        throw new Error("Invalid preset file: missing name or settings.");
    }
    const created = createCustomPreset(name, data.settings);
    if (!created) {
        throw new Error(`Preset "${name}" could not be created (reserved or duplicate system name).`);
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
        if (parsed?.format === FORMAT_COLLECTION && Array.isArray(parsed.presets)) {
            if (parsed.presets.length === 0) {
                return { ok: false, message: "The collection is empty." };
            }
            const names = parsed.presets.map((item) => importSingle(item));
            return { ok: true, message: `Imported ${names.length} presets: ${names.join(", ")}.` };
        }
        if (parsed?.format === FORMAT_SINGLE) {
            const name = importSingle(parsed);
            return { ok: true, message: `Preset "${name}" imported.` };
        }
        return { ok: false, message: "Unrecognized preset file format." };
    } catch (error) {
        return { ok: false, message: error.message };
    }
}
