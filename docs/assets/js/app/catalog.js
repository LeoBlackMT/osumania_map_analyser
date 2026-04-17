import { parseBenchmarkCsv } from "../csv.js";
import { DATA_DIR, INDEX_URL, state } from "./state.js";
import {
    appendTimestamp,
    isCsvFileName,
    normalizeLooseName,
    stripCsvSuffix,
} from "./utils.js";
import { buildAlgorithmMeta, normalizeLoadedRows } from "./model.js";

export function cacheRowsForAlgorithm(algorithm, rows) {
    const normalizedRows = normalizeLoadedRows(rows);
    state.cache.set(algorithm, normalizedRows);
    state.metaCache.set(algorithm, buildAlgorithmMeta(normalizedRows));
    return normalizedRows;
}

export function addCatalogEntry(algorithm, descriptor) {
    const normalizedName = String(algorithm ?? "").trim();
    if (!normalizedName) {
        return;
    }

    const next = {
        algorithm: normalizedName,
        fileName: descriptor.fileName,
        url: descriptor.url || null,
        source: descriptor.source,
        modifiedAt: descriptor.modifiedAt || null,
    };

    const previous = state.catalog.get(normalizedName);
    if (!previous) {
        state.catalog.set(normalizedName, next);
        return;
    }

    if (previous.source === "local" && next.source !== "local") {
        return;
    }

    state.catalog.set(normalizedName, next);
}

export function clearRemoteCatalogEntries() {
    for (const [algorithm, descriptor] of state.catalog.entries()) {
        if (descriptor.source !== "local") {
            state.catalog.delete(algorithm);
            state.cache.delete(algorithm);
            state.metaCache.delete(algorithm);
        }
    }
}

export function rebuildAlgorithmList() {
    state.algorithms = [...state.catalog.keys()].sort((a, b) => a.localeCompare(b, undefined, { sensitivity: "base" }));
}

export function findAlgorithmByLooseName(value) {
    const needle = normalizeLooseName(value);
    if (!needle) {
        return null;
    }

    return state.algorithms.find((algorithm) => normalizeLooseName(algorithm) === needle) || null;
}

async function discoverCatalogFromIndexJson() {
    const response = await fetch(appendTimestamp(INDEX_URL), { cache: "no-store" });
    if (!response.ok) {
        throw new Error(`index.json request failed (${response.status})`);
    }

    const payload = await response.json();
    const discovered = [];

    if (Array.isArray(payload?.files)) {
        for (const item of payload.files) {
            if (typeof item === "string") {
                const fileName = item.trim();
                if (!isCsvFileName(fileName)) {
                    continue;
                }

                discovered.push({
                    algorithm: stripCsvSuffix(fileName),
                    fileName,
                    modifiedAt: null,
                });
                continue;
            }

            const fileName = String(item?.fileName ?? "").trim();
            if (!isCsvFileName(fileName)) {
                continue;
            }

            const algorithm = String(item?.algorithm ?? stripCsvSuffix(fileName)).trim();
            discovered.push({
                algorithm,
                fileName,
                modifiedAt: String(item?.modifiedAt ?? "").trim() || null,
            });
        }
    }

    if (!discovered.length && Array.isArray(payload?.algorithms)) {
        for (const algorithmRaw of payload.algorithms) {
            const algorithm = String(algorithmRaw ?? "").trim();
            if (!algorithm) {
                continue;
            }
            discovered.push({
                algorithm,
                fileName: `${algorithm}.csv`,
                modifiedAt: null,
            });
        }
    }

    return discovered.map((entry) => ({
        ...entry,
        url: `${DATA_DIR}/${encodeURIComponent(entry.fileName)}`,
    }));
}

async function discoverCatalogFromDirectoryListing() {
    const response = await fetch(appendTimestamp(`${DATA_DIR}/`), { cache: "no-store" });
    if (!response.ok) {
        throw new Error(`directory listing request failed (${response.status})`);
    }

    const text = await response.text();
    const pattern = /href\s*=\s*["']([^"']+?\.csv(?:\?[^"']*)?)["']/gi;

    const discovered = [];
    const seen = new Set();

    for (const match of text.matchAll(pattern)) {
        const href = String(match[1] || "").trim();
        if (!href) {
            continue;
        }

        const withoutHash = href.split("#")[0];
        const withoutQuery = withoutHash.split("?")[0];
        const decoded = decodeURIComponent(withoutQuery);
        const fileName = decoded.split("/").pop();

        if (!fileName || !isCsvFileName(fileName)) {
            continue;
        }

        const key = normalizeLooseName(fileName);
        if (seen.has(key)) {
            continue;
        }
        seen.add(key);

        discovered.push({
            algorithm: stripCsvSuffix(fileName),
            fileName,
            url: `${DATA_DIR}/${encodeURIComponent(fileName)}`,
            modifiedAt: null,
        });
    }

    return discovered;
}

export async function refreshRemoteCatalog() {
    clearRemoteCatalogEntries();

    let discovered = [];
    let sourceLabel = "";

    try {
        discovered = await discoverCatalogFromIndexJson();
        sourceLabel = "index.json";
    } catch {
        discovered = [];
    }

    if (!discovered.length) {
        try {
            discovered = await discoverCatalogFromDirectoryListing();
            sourceLabel = "directory listing";
        } catch {
            discovered = [];
        }
    }

    for (const entry of discovered) {
        addCatalogEntry(entry.algorithm, {
            fileName: entry.fileName,
            url: entry.url,
            source: "remote",
            modifiedAt: entry.modifiedAt,
        });
    }

    rebuildAlgorithmList();
    return {
        discoveredCount: discovered.length,
        sourceLabel,
    };
}

export async function ensureRowsLoaded(algorithm, forceReload = false) {
    if (!algorithm) {
        return [];
    }

    if (!forceReload && state.cache.has(algorithm)) {
        return state.cache.get(algorithm);
    }

    const descriptor = state.catalog.get(algorithm);
    if (!descriptor) {
        throw new Error(`Dataset not found for ${algorithm}`);
    }

    if (descriptor.source === "local") {
        return state.cache.get(algorithm) || [];
    }

    if (!descriptor.url) {
        throw new Error(`Dataset URL missing for ${algorithm}`);
    }

    const response = await fetch(appendTimestamp(descriptor.url), { cache: "no-store" });
    if (!response.ok) {
        throw new Error(`${descriptor.fileName} request failed (${response.status})`);
    }

    const csvText = await response.text();
    const parsed = parseBenchmarkCsv(csvText);
    return cacheRowsForAlgorithm(algorithm, parsed.rows);
}

export async function importLocalDatasets(fileList) {
    const files = Array.from(fileList || []).filter((file) => isCsvFileName(file.name));
    if (!files.length) {
        return {
            imported: 0,
            hasCsv: false,
        };
    }

    let imported = 0;
    for (const file of files) {
        const algorithm = stripCsvSuffix(file.name);
        if (!algorithm) {
            continue;
        }

        try {
            const text = await file.text();
            const parsed = parseBenchmarkCsv(text);

            addCatalogEntry(algorithm, {
                fileName: file.name,
                source: "local",
                url: null,
                modifiedAt: Number.isFinite(file.lastModified)
                    ? new Date(file.lastModified).toISOString()
                    : null,
            });

            cacheRowsForAlgorithm(algorithm, parsed.rows);
            imported += 1;
        } catch {
            // Keep importing remaining files.
        }
    }

    rebuildAlgorithmList();

    return {
        imported,
        hasCsv: true,
    };
}
