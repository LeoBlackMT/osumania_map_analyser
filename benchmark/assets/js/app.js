import { parseBenchmarkCsv } from "./csv.js";
import {
    BAND_META,
    BAND_ORDER,
    buildRowKey,
    classifyBand,
    computeHeadToHead,
    computeSummary,
} from "./stats.js";
import { BenchmarkCharts } from "./charts.js";

const DATA_DIR = "data";
const INDEX_URL = `${DATA_DIR}/index.json`;

const state = {
    catalog: new Map(),
    cache: new Map(),

    algorithms: [],
    currentAlgorithm: null,
    compareAlgorithm: "",

    baseRows: [],
    displayRows: [],
    summary: null,
    compareSummary: null,

    filteredRows: [],
    sortKey: "deltaAbs",
    sortDirection: "asc",
};

const dom = {
    algorithmSelect: document.getElementById("algorithmSelect"),
    compareAlgorithmSelect: document.getElementById("compareAlgorithmSelect"),
    reloadDataButton: document.getElementById("reloadDataButton"),
    openDataFolderButton: document.getElementById("openDataFolderButton"),
    dataFileInput: document.getElementById("dataFileInput"),
    sourceHint: document.getElementById("sourceHint"),

    statusBadge: document.getElementById("statusBadge"),
    datasetInfo: document.getElementById("datasetInfo"),

    totalMapsValue: document.getElementById("totalMapsValue"),
    validMapsValue: document.getElementById("validMapsValue"),
    maeValue: document.getElementById("maeValue"),
    rmseValue: document.getElementById("rmseValue"),
    biasValue: document.getElementById("biasValue"),
    medianValue: document.getElementById("medianValue"),
    coverageValue: document.getElementById("coverageValue"),
    p90Value: document.getElementById("p90Value"),
    maxUnderrateValue: document.getElementById("maxUnderrateValue"),
    maxOverrateValue: document.getElementById("maxOverrateValue"),

    exactRateValue: document.getElementById("exactRateValue"),
    closeRateValue: document.getElementById("closeRateValue"),
    moderateRateValue: document.getElementById("moderateRateValue"),
    missRateValue: document.getElementById("missRateValue"),

    exactCountValue: document.getElementById("exactCountValue"),
    closeCountValue: document.getElementById("closeCountValue"),
    moderateCountValue: document.getElementById("moderateCountValue"),
    missCountValue: document.getElementById("missCountValue"),

    compareStatusText: document.getElementById("compareStatusText"),
    compareMatchedValue: document.getElementById("compareMatchedValue"),
    compareBaseWinsValue: document.getElementById("compareBaseWinsValue"),
    compareOtherWinsValue: document.getElementById("compareOtherWinsValue"),
    compareTieValue: document.getElementById("compareTieValue"),
    compareAgreementValue: document.getElementById("compareAgreementValue"),
    compareMaeGapValue: document.getElementById("compareMaeGapValue"),

    underratedList: document.getElementById("underratedList"),
    overratedList: document.getElementById("overratedList"),

    searchInput: document.getElementById("searchInput"),
    patternFilter: document.getElementById("patternFilter"),
    bandFilter: document.getElementById("bandFilter"),
    clearFilterButton: document.getElementById("clearFilterButton"),

    tableMeta: document.getElementById("tableMeta"),
    resultTable: document.getElementById("resultTable"),
    resultTableBody: document.getElementById("resultTableBody"),
    emptyState: document.getElementById("emptyState"),
};

const charts = new BenchmarkCharts({
    accuracy: "accuracyBreakdownChart",
    scatter: "scatterChart",
    deltaDistribution: "deltaDistributionChart",
    trend: "trendChart",
    pattern: "patternChart",
    headToHead: "headToHeadChart",
});

function normalizeLooseName(value) {
    return String(value ?? "").trim().toLowerCase();
}

function isCsvFileName(fileName) {
    return /\.csv$/i.test(String(fileName ?? "").trim());
}

function stripCsvSuffix(fileName) {
    return String(fileName ?? "").replace(/\.csv$/i, "").trim();
}

function escapeHtml(value) {
    return String(value ?? "")
        .replace(/&/g, "&amp;")
        .replace(/</g, "&lt;")
        .replace(/>/g, "&gt;")
        .replace(/\"/g, "&quot;")
        .replace(/'/g, "&#39;");
}

function appendTimestamp(url) {
    const ts = Date.now();
    return url.includes("?") ? `${url}&ts=${ts}` : `${url}?ts=${ts}`;
}

function toErrorMessage(error) {
    if (error instanceof Error && error.message) {
        return error.message;
    }
    return String(error || "Unknown error");
}

function formatNumber(value, digits = 2) {
    return Number.isFinite(value) ? Number(value).toFixed(digits) : "-";
}

function formatPercent(value) {
    return `${Number(value || 0).toFixed(1)}%`;
}

function formatSigned(value, digits = 2) {
    if (!Number.isFinite(value)) {
        return "-";
    }

    const fixed = Number(value).toFixed(digits);
    return value > 0 ? `+${fixed}` : fixed;
}

function setStatus(message, level) {
    dom.statusBadge.className = `badge ${level}`;
    dom.statusBadge.textContent = message;
}

function setDatasetInfo(text) {
    dom.datasetInfo.textContent = text;
}

function updateSourceHint(extra = "") {
    let remoteCount = 0;
    let localCount = 0;

    for (const descriptor of state.catalog.values()) {
        if (descriptor.source === "local") {
            localCount += 1;
        } else {
            remoteCount += 1;
        }
    }

    const segments = [];
    if (remoteCount > 0) {
        segments.push(`remote ${remoteCount}`);
    }
    if (localCount > 0) {
        segments.push(`local ${localCount}`);
    }
    if (!segments.length) {
        segments.push("no datasets");
    }

    const suffix = extra ? ` | ${extra}` : "";
    dom.sourceHint.textContent = `Source: ${segments.join(" + ")}${suffix}`;
}

function addCatalogEntry(algorithm, descriptor) {
    const normalizedName = String(algorithm ?? "").trim();
    if (!normalizedName) {
        return;
    }

    const next = {
        algorithm: normalizedName,
        fileName: descriptor.fileName,
        url: descriptor.url || null,
        source: descriptor.source,
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

function clearRemoteCatalogEntries() {
    for (const [algorithm, descriptor] of state.catalog.entries()) {
        if (descriptor.source !== "local") {
            state.catalog.delete(algorithm);
            state.cache.delete(algorithm);
        }
    }
}

function rebuildAlgorithmList() {
    state.algorithms = [...state.catalog.keys()].sort((a, b) => a.localeCompare(b, undefined, { sensitivity: "base" }));
}

function renderAlgorithmOptions() {
    const previous = state.currentAlgorithm;

    dom.algorithmSelect.innerHTML = "";
    for (const algorithm of state.algorithms) {
        const option = document.createElement("option");
        option.value = algorithm;
        option.textContent = algorithm;
        dom.algorithmSelect.appendChild(option);
    }

    if (!state.algorithms.length) {
        state.currentAlgorithm = null;
        return;
    }

    if (previous && state.algorithms.includes(previous)) {
        state.currentAlgorithm = previous;
    } else {
        state.currentAlgorithm = state.algorithms[0];
    }

    dom.algorithmSelect.value = state.currentAlgorithm;
}

function renderCompareOptions() {
    const previous = state.compareAlgorithm;
    dom.compareAlgorithmSelect.innerHTML = "";

    const offOption = document.createElement("option");
    offOption.value = "";
    offOption.textContent = "Off";
    dom.compareAlgorithmSelect.appendChild(offOption);

    const candidates = state.algorithms.filter((algorithm) => algorithm !== state.currentAlgorithm);
    for (const algorithm of candidates) {
        const option = document.createElement("option");
        option.value = algorithm;
        option.textContent = algorithm;
        dom.compareAlgorithmSelect.appendChild(option);
    }

    if (previous && candidates.includes(previous)) {
        state.compareAlgorithm = previous;
    } else {
        state.compareAlgorithm = "";
    }

    dom.compareAlgorithmSelect.value = state.compareAlgorithm;
}

function renderAlgorithmSelectors() {
    renderAlgorithmOptions();
    renderCompareOptions();
}

function findAlgorithmByLooseName(value) {
    const needle = normalizeLooseName(value);
    if (!needle) {
        return null;
    }

    return state.algorithms.find((algorithm) => normalizeLooseName(algorithm) === needle) || null;
}

function fillPatternFilter(rows) {
    const previous = dom.patternFilter.value;
    const patterns = [...new Set(rows.map((row) => String(row.pattern || "").trim()).filter(Boolean))]
        .sort((a, b) => a.localeCompare(b, undefined, { sensitivity: "base" }));

    dom.patternFilter.innerHTML = "";

    const allOption = document.createElement("option");
    allOption.value = "all";
    allOption.textContent = "All";
    dom.patternFilter.appendChild(allOption);

    for (const pattern of patterns) {
        const option = document.createElement("option");
        option.value = pattern;
        option.textContent = pattern;
        dom.patternFilter.appendChild(option);
    }

    dom.patternFilter.value = patterns.includes(previous) ? previous : "all";
}

function getRowBand(row) {
    return classifyBand(Number(row.deltaAbs));
}

function getWinnerLabel(better) {
    if (!state.compareAlgorithm) {
        return "-";
    }

    if (better === "base") {
        return "Base";
    }
    if (better === "compare") {
        return "Compare";
    }
    if (better === "tie") {
        return "Tie";
    }
    return "-";
}

function mergeRowsForDisplay(baseRows, compareRows) {
    if (!state.compareAlgorithm || !Array.isArray(compareRows) || compareRows.length === 0) {
        return baseRows.map((row) => ({
            ...row,
            compareGot: null,
            compareDeltaAbs: null,
            better: "na",
        }));
    }

    const compareMap = new Map(compareRows.map((row) => [buildRowKey(row), row]));

    return baseRows.map((row) => {
        const peer = compareMap.get(buildRowKey(row));
        if (!peer) {
            return {
                ...row,
                compareGot: null,
                compareDeltaAbs: null,
                better: "na",
            };
        }

        const baseDeltaAbs = Number(row.deltaAbs);
        const compareDeltaAbs = Number(peer.deltaAbs);

        let better = "na";
        if (Number.isFinite(baseDeltaAbs) && Number.isFinite(compareDeltaAbs)) {
            if (baseDeltaAbs < compareDeltaAbs) {
                better = "base";
            } else if (baseDeltaAbs > compareDeltaAbs) {
                better = "compare";
            } else {
                better = "tie";
            }
        }

        return {
            ...row,
            compareGot: Number.isFinite(Number(peer.got)) ? Number(peer.got) : null,
            compareDeltaAbs: Number.isFinite(compareDeltaAbs) ? compareDeltaAbs : null,
            better,
        };
    });
}

function updateSummary(summary) {
    dom.totalMapsValue.textContent = String(summary.totalRows);
    dom.validMapsValue.textContent = String(summary.validRows);

    dom.maeValue.textContent = formatNumber(summary.metrics.mae, 3);
    dom.rmseValue.textContent = formatNumber(summary.metrics.rmse, 3);
    dom.biasValue.textContent = formatSigned(summary.metrics.bias, 3);
    dom.medianValue.textContent = formatNumber(summary.metrics.medianAbs, 3);
    dom.coverageValue.textContent = formatPercent(summary.metrics.coverage);
    dom.p90Value.textContent = formatNumber(summary.metrics.p90Abs, 3);
    dom.maxUnderrateValue.textContent = formatSigned(summary.metrics.maxUnderrate, 3);
    dom.maxOverrateValue.textContent = formatSigned(summary.metrics.maxOverrate, 3);

    dom.exactRateValue.textContent = formatPercent(summary.bandRates.exact);
    dom.closeRateValue.textContent = formatPercent(summary.bandRates.close);
    dom.moderateRateValue.textContent = formatPercent(summary.bandRates.moderate);
    dom.missRateValue.textContent = formatPercent(summary.bandRates.miss);

    dom.exactCountValue.textContent = `${summary.bandCounts.exact} rows`;
    dom.closeCountValue.textContent = `${summary.bandCounts.close} rows`;
    dom.moderateCountValue.textContent = `${summary.bandCounts.moderate} rows`;
    dom.missCountValue.textContent = `${summary.bandCounts.miss} rows`;
}

function renderInsightList(target, rows, direction) {
    if (!rows.length) {
        target.innerHTML = '<li class="insight-empty">No rows with valid values.</li>';
        return;
    }

    target.innerHTML = rows
        .map((row) => {
            const sign = direction === "positive" ? "+" : "";
            return [
                "<li>",
                `<strong>${escapeHtml(row.name)}</strong>`,
                `<span class="muted"> (${escapeHtml(row.pattern || "-")})</span>`,
                `<br><span class="muted">delta ${sign}${formatNumber(Number(row.delta), 3)} | expected ${formatNumber(Number(row.expected), 3)} | got ${formatNumber(Number(row.got), 3)}</span>`,
                "</li>",
            ].join("");
        })
        .join("");
}

function updateInsightLists(summary) {
    renderInsightList(dom.underratedList, summary.topUnderrated, "positive");
    renderInsightList(dom.overratedList, summary.topOverrated, "negative");
}

function updateCompareSummary(compareSummary) {
    if (!state.compareAlgorithm || !compareSummary) {
        dom.compareStatusText.textContent = "Comparison disabled.";
        dom.compareMatchedValue.textContent = "-";
        dom.compareBaseWinsValue.textContent = "-";
        dom.compareOtherWinsValue.textContent = "-";
        dom.compareTieValue.textContent = "-";
        dom.compareAgreementValue.textContent = "-";
        dom.compareMaeGapValue.textContent = "-";
        return;
    }

    dom.compareStatusText.textContent = `${state.currentAlgorithm} vs ${state.compareAlgorithm}`;
    dom.compareMatchedValue.textContent = String(compareSummary.matchedRows);
    dom.compareBaseWinsValue.textContent = String(compareSummary.baseWins);
    dom.compareOtherWinsValue.textContent = String(compareSummary.compareWins);
    dom.compareTieValue.textContent = String(compareSummary.tieCount);
    dom.compareAgreementValue.textContent = formatPercent(compareSummary.agreementRate);
    dom.compareMaeGapValue.textContent = formatSigned(compareSummary.maeGap, 3);
}

function compareValues(a, b, key) {
    if (key === "band") {
        const aIdx = BAND_ORDER.indexOf(getRowBand(a));
        const bIdx = BAND_ORDER.indexOf(getRowBand(b));
        return aIdx - bIdx;
    }

    if (key === "better") {
        const rank = { base: 0, compare: 1, tie: 2, na: 3 };
        return (rank[a.better] ?? 3) - (rank[b.better] ?? 3);
    }

    const numericKeys = new Set(["expected", "got", "delta", "deltaAbs", "compareGot", "compareDeltaAbs"]);
    if (numericKeys.has(key)) {
        const aVal = Number(a[key]);
        const bVal = Number(b[key]);
        const aFinite = Number.isFinite(aVal);
        const bFinite = Number.isFinite(bVal);

        if (!aFinite && !bFinite) {
            return 0;
        }
        if (!aFinite) {
            return 1;
        }
        if (!bFinite) {
            return -1;
        }
        return aVal - bVal;
    }

    return String(a[key] ?? "").localeCompare(String(b[key] ?? ""));
}

function sortRows(rows) {
    const sorted = [...rows];
    sorted.sort((a, b) => {
        const base = compareValues(a, b, state.sortKey);
        return state.sortDirection === "asc" ? base : -base;
    });
    return sorted;
}

function renderTable(rows) {
    dom.emptyState.hidden = rows.length > 0;

    dom.resultTableBody.innerHTML = rows
        .map((row) => {
            const bandKey = getRowBand(row);
            const bandLabel = BAND_META[bandKey].label;
            const winnerLabel = getWinnerLabel(row.better);
            const winnerClass = row.better || "na";

            return [
                `<tr class="band-${bandKey}">`,
                `<td>${escapeHtml(row.name)}</td>`,
                `<td class="num">${formatNumber(Number(row.expected))}</td>`,
                `<td class="num">${formatNumber(Number(row.got))}</td>`,
                `<td class="num">${formatSigned(Number(row.delta))}</td>`,
                `<td class="num">${formatNumber(Number(row.deltaAbs))}</td>`,
                `<td>${escapeHtml(row.pattern)}</td>`,
                `<td>${escapeHtml(row.subPattern)}</td>`,
                `<td class="band">${bandLabel}</td>`,
                `<td class="num">${formatNumber(Number(row.compareGot))}</td>`,
                `<td class="num">${formatNumber(Number(row.compareDeltaAbs))}</td>`,
                `<td class="winner ${winnerClass}">${escapeHtml(winnerLabel)}</td>`,
                "</tr>",
            ].join("");
        })
        .join("");
}

function applyFilters() {
    const searchText = String(dom.searchInput.value || "").trim().toLowerCase();
    const selectedPattern = dom.patternFilter.value;
    const selectedBand = dom.bandFilter.value;

    const filtered = state.displayRows.filter((row) => {
        if (selectedPattern && selectedPattern !== "all" && row.pattern !== selectedPattern) {
            return false;
        }

        if (selectedBand && selectedBand !== "all" && getRowBand(row) !== selectedBand) {
            return false;
        }

        if (!searchText) {
            return true;
        }

        const haystack = [row.name, row.pattern, row.subPattern]
            .map((part) => String(part || "").toLowerCase())
            .join(" ");

        return haystack.includes(searchText);
    });

    state.filteredRows = sortRows(filtered);
    renderTable(state.filteredRows);

    dom.tableMeta.textContent = `${state.filteredRows.length} / ${state.displayRows.length} rows shown`;
}

function updateSortVisual() {
    const headers = dom.resultTable.querySelectorAll("thead th[data-sort]");
    headers.forEach((head) => {
        const key = head.getAttribute("data-sort");
        const original = head.textContent.replace(/[\u2191\u2193]/g, "").trim();
        if (key === state.sortKey) {
            head.textContent = `${original} ${state.sortDirection === "asc" ? "\u2191" : "\u2193"}`;
        } else {
            head.textContent = original;
        }
    });
}

function syncUrlParams() {
    const currentUrl = new URL(window.location.href);

    if (state.currentAlgorithm) {
        currentUrl.searchParams.set("algorithm", state.currentAlgorithm);
    } else {
        currentUrl.searchParams.delete("algorithm");
    }

    if (state.compareAlgorithm) {
        currentUrl.searchParams.set("compare", state.compareAlgorithm);
    } else {
        currentUrl.searchParams.delete("compare");
    }

    history.replaceState(null, "", currentUrl.toString());
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
                });
                continue;
            }

            const fileName = String(item?.fileName ?? "").trim();
            if (!isCsvFileName(fileName)) {
                continue;
            }

            const algorithm = String(item?.algorithm ?? stripCsvSuffix(fileName)).trim();
            discovered.push({ algorithm, fileName });
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
        });
    }

    return discovered;
}

async function refreshRemoteCatalog() {
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
        });
    }

    rebuildAlgorithmList();
    updateSourceHint(sourceLabel ? `discovered by ${sourceLabel}` : "");

    return discovered.length;
}

async function ensureRowsLoaded(algorithm, forceReload = false) {
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
    state.cache.set(algorithm, parsed.rows);
    return parsed.rows;
}

function buildFriendlyFetchHint(originalMessage) {
    const message = String(originalMessage || "");
    const isFetchFailure = /fetch|Failed to fetch|not allowed|scheme/i.test(message);

    if (window.location.protocol === "file:" && isFetchFailure) {
        return [
            "Direct file mode blocks fetch in some Edge contexts.",
            "Click Open Data Folder and select benchmark/data to load CSV files locally.",
        ].join(" ");
    }

    return message;
}

async function loadCurrentView(options = {}) {
    const forceReload = Boolean(options.forceReload);

    if (!state.currentAlgorithm) {
        setStatus("Waiting", "warn");
        setDatasetInfo("No algorithm selected.");
        return;
    }

    setStatus("Loading...", "warn");
    setDatasetInfo(`Loading ${state.currentAlgorithm}...`);

    try {
        state.baseRows = await ensureRowsLoaded(state.currentAlgorithm, forceReload);

        let compareRows = [];
        if (state.compareAlgorithm) {
            compareRows = await ensureRowsLoaded(state.compareAlgorithm, forceReload);
        }

        state.summary = computeSummary(state.baseRows);
        state.compareSummary = state.compareAlgorithm
            ? computeHeadToHead(state.baseRows, compareRows)
            : null;

        state.displayRows = mergeRowsForDisplay(state.baseRows, compareRows);

        updateSummary(state.summary);
        updateInsightLists(state.summary);
        updateCompareSummary(state.compareSummary);

        fillPatternFilter(state.baseRows);
        applyFilters();
        charts.render(state.summary, state.compareSummary);

        const nowText = new Date().toLocaleString();
        const compareText = state.compareAlgorithm
            ? ` | compare=${state.compareAlgorithm} | matched=${state.compareSummary?.matchedRows ?? 0}`
            : "";

        setStatus("Ready", "ok");
        setDatasetInfo(`${state.currentAlgorithm}${compareText} | rows=${state.baseRows.length} | loaded ${nowText}`);
        syncUrlParams();
    } catch (error) {
        const emptySummary = computeSummary([]);
        state.baseRows = [];
        state.displayRows = [];
        state.summary = emptySummary;
        state.compareSummary = null;

        updateSummary(emptySummary);
        updateInsightLists(emptySummary);
        updateCompareSummary(null);
        renderTable([]);
        charts.render(emptySummary, null);
        dom.tableMeta.textContent = "No data loaded.";

        setStatus("Error", "error");
        setDatasetInfo(`${state.currentAlgorithm} | ${buildFriendlyFetchHint(toErrorMessage(error))}`);
    }
}

async function loadLocalDatasets(fileList) {
    const files = Array.from(fileList || []).filter((file) => isCsvFileName(file.name));
    if (!files.length) {
        setStatus("Waiting", "warn");
        setDatasetInfo("No CSV files found in selected folder.");
        return;
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
            });

            state.cache.set(algorithm, parsed.rows);
            imported += 1;
        } catch {
            // Continue importing remaining files.
        }
    }

    rebuildAlgorithmList();

    if (!state.algorithms.length) {
        setStatus("Error", "error");
        setDatasetInfo("Local import finished but no valid CSV was parsed.");
        return;
    }

    if (!state.currentAlgorithm || !state.algorithms.includes(state.currentAlgorithm)) {
        state.currentAlgorithm = state.algorithms[0];
    }

    if (state.compareAlgorithm && !state.algorithms.includes(state.compareAlgorithm)) {
        state.compareAlgorithm = "";
    }

    renderAlgorithmSelectors();
    updateSortVisual();
    updateSourceHint(`local import ${imported} file(s)`);

    await loadCurrentView();
}

function bindEvents() {
    dom.algorithmSelect.addEventListener("change", async () => {
        state.currentAlgorithm = dom.algorithmSelect.value;

        if (state.compareAlgorithm === state.currentAlgorithm) {
            state.compareAlgorithm = "";
        }

        renderCompareOptions();
        await loadCurrentView();
    });

    dom.compareAlgorithmSelect.addEventListener("change", async () => {
        state.compareAlgorithm = dom.compareAlgorithmSelect.value || "";
        await loadCurrentView();
    });

    dom.reloadDataButton.addEventListener("click", async () => {
        await refreshRemoteCatalog();

        if (state.currentAlgorithm && !state.algorithms.includes(state.currentAlgorithm)) {
            state.currentAlgorithm = state.algorithms[0] || null;
        }

        if (state.compareAlgorithm && !state.algorithms.includes(state.compareAlgorithm)) {
            state.compareAlgorithm = "";
        }

        renderAlgorithmSelectors();
        updateSortVisual();
        await loadCurrentView({ forceReload: true });
    });

    dom.openDataFolderButton.addEventListener("click", () => {
        dom.dataFileInput.value = "";
        dom.dataFileInput.click();
    });

    dom.dataFileInput.addEventListener("change", async (event) => {
        const files = event.target?.files;
        await loadLocalDatasets(files);
    });

    dom.searchInput.addEventListener("input", applyFilters);
    dom.patternFilter.addEventListener("change", applyFilters);
    dom.bandFilter.addEventListener("change", applyFilters);

    dom.clearFilterButton.addEventListener("click", () => {
        dom.searchInput.value = "";
        dom.patternFilter.value = "all";
        dom.bandFilter.value = "all";
        applyFilters();
    });

    const sortableHeaders = dom.resultTable.querySelectorAll("thead th[data-sort]");
    sortableHeaders.forEach((header) => {
        header.addEventListener("click", () => {
            const key = header.getAttribute("data-sort");
            if (!key) {
                return;
            }

            if (state.sortKey === key) {
                state.sortDirection = state.sortDirection === "asc" ? "desc" : "asc";
            } else {
                state.sortKey = key;
                state.sortDirection = "asc";
            }

            updateSortVisual();
            applyFilters();
        });
    });
}

async function init() {
    bindEvents();

    setStatus("Loading...", "warn");
    setDatasetInfo("Discovering datasets from benchmark/data...");
    updateSourceHint();

    const discoveredRemoteCount = await refreshRemoteCatalog();
    renderAlgorithmSelectors();
    updateSortVisual();

    if (!state.algorithms.length) {
        setStatus("Waiting", "warn");
        setDatasetInfo("No dataset discovered. Click Open Data Folder and select benchmark/data.");

        const emptySummary = computeSummary([]);
        updateSummary(emptySummary);
        updateInsightLists(emptySummary);
        updateCompareSummary(null);
        charts.render(emptySummary, null);
        dom.tableMeta.textContent = "No data loaded.";
        return;
    }

    const search = new URLSearchParams(window.location.search);
    const requestedBase = findAlgorithmByLooseName(search.get("algorithm"));
    const requestedCompare = findAlgorithmByLooseName(search.get("compare"));

    if (requestedBase) {
        state.currentAlgorithm = requestedBase;
    }

    if (requestedCompare && requestedCompare !== state.currentAlgorithm) {
        state.compareAlgorithm = requestedCompare;
    }

    renderAlgorithmSelectors();
    updateSortVisual();

    await loadCurrentView();

    if (window.location.protocol === "file:" && discoveredRemoteCount === 0) {
        setStatus("Ready", "ok");
        setDatasetInfo("Local-file mode detected. Use Open Data Folder to import CSVs directly.");
    }
}

init();
