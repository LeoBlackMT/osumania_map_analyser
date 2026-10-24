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
    slugify,
} from "./core.js";
import { AUTO_SAVE_PRESET_NAME } from "./storage.js";

const FORMAT_SINGLE = "mma-preset";
const FORMAT_COLLECTION = "mma-preset-collection";
const PRESET_NAME_RE = /^[A-Za-z0-9_-]{1,40}$/;

function safeFileName(name) {
    return String(name || "preset")
        .replace(/[\\/:*?"<>|]/g, "_")
        .slice(0, 60);
}

/**
 * 保存 JSON 到用户选定的位置。
 * 优先用 File System Access API（WebView2 / Chromium 都支持）弹出「另存为」，
 * 用户不选就不落盘；不支持或调用失败时回落到普通下载（浏览器/CEF 的下载目录）。
 * @returns {Promise<boolean>} true = 已保存，false = 用户取消
 */
async function download(text, fileName) {
    const blob = new Blob([text], { type: "application/json" });
    if (typeof window !== "undefined" && typeof window.showSaveFilePicker === "function") {
        try {
            const handle = await window.showSaveFilePicker({
                suggestedName: fileName,
                types: [{ description: "JSON", accept: { "application/json": [".json"] } }],
            });
            const writable = await handle.createWritable();
            await writable.write(blob);
            await writable.close();
            return true;
        } catch (error) {
            if (error && error.name === "AbortError") {
                return false; // 用户在对话框里点了取消
            }
            // 其他失败（权限、不支持的类型等）：继续走下面的普通下载
        }
    }
    const url = URL.createObjectURL(blob);
    const link = document.createElement("a");
    link.href = url;
    link.download = fileName;
    document.body.appendChild(link);
    link.click();
    link.remove();
    URL.revokeObjectURL(url);
    return true;
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
    return download(
        JSON.stringify(singlePayload(preset.name, preset.description, preset.version, preset.settings), null, 2),
        `${safeFileName(preset.name)}.json`,
    );
}

/** Exports the currently edited (unsaved) preset as a downloadable json file. */
export function exportCurrentToFile(name, description, version, settings) {
    return download(
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
            .filter((preset) => preset.name !== AUTO_SAVE_PRESET_NAME)
            .map((preset) => ({
                name: preset.name,
                description: preset.description || "",
                version: preset.version || 1,
                settings: preset.settings,
                exportedAt: preset.updatedAt || preset.createdAt,
            })),
        exportedAt: new Date().toISOString(),
    };
    return download(JSON.stringify(data, null, 2), "mma-presets.json");
}

function importSingle(name, description, version, settings) {
    const cleanName = String(name || "").trim();
    if (!cleanName || !settings || typeof settings !== "object") {
        throw new Error("Invalid preset: missing name or settings.");
    }
    // v1 files may carry names with spaces/symbols — slugify them on import so
    // the preset stays usable instead of being rejected.
    const finalName = PRESET_NAME_RE.test(cleanName) ? cleanName : slugify(cleanName);
    if (!finalName) {
        throw new Error(`Preset "${cleanName}" has an unusable name.`);
    }
    const created = createCustomPreset(finalName, settings, { description, version });
    if (!created) {
        throw new Error(`Preset "${finalName}" could not be created (reserved or duplicate system name).`);
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
