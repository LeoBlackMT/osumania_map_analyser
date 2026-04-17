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

const SCOPE_RC = "RC";
const SCOPE_LN = "LN";

const state = {
    catalog: new Map(),
    cache: new Map(),
    metaCache: new Map(),

    algorithms: [],
    currentAlgorithm: null,
    compareAlgorithm: "",

    baseMode: SCOPE_RC,
    compareMode: SCOPE_RC,

    baseRows: [],
    compareRows: [],
    scopedBaseRows: [],
    scopedCompareRows: [],

    displayRows: [],
    errorRows: [],

    summary: null,
    compareSummary: null,

    filteredRows: [],
    sortKey: "deltaAbs",
    sortDirection: "asc",
};

const dom = {
    algorithmSelect: document.getElementById("algorithmSelect"),
    compareAlgorithmSelect: document.getElementById("compareAlgorithmSelect"),

    baseCategoryField: document.getElementById("baseCategoryField"),
    baseCategorySelect: document.getElementById("baseCategorySelect"),
    compareCategoryField: document.getElementById("compareCategoryField"),
    compareCategorySelect: document.getElementById("compareCategorySelect"),

    reloadDataButton: document.getElementById("reloadDataButton"),
    openDataFolderButton: document.getElementById("openDataFolderButton"),
    downloadCurrentDataButton: document.getElementById("downloadCurrentDataButton"),
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

    errorStatusText: document.getElementById("errorStatusText"),
    errorInvalidCount: document.getElementById("errorInvalidCount"),
    errorFailedCount: document.getElementById("errorFailedCount"),
    errorMissingCount: document.getElementById("errorMissingCount"),
    errorTableBody: document.getElementById("errorTableBody"),
    errorEmptyState: document.getElementById("errorEmptyState"),

    underratedList: document.getElementById("underratedList"),
    overratedList: document.getElementById("overratedList"),

    searchInput: document.getElementById("searchInput"),
    patternFilter: document.getElementById("patternFilter"),
    subPatternFilter: document.getElementById("subPatternFilter"),
    bandFilter: document.getElementById("bandFilter"),
    clearFilterButton: document.getElementById("clearFilterButton"),

    tableMeta: document.getElementById("tableMeta"),
    resultTable: document.getElementById("resultTable"),
    resultTableBody: document.getElementById("resultTableBody"),
    emptyState: document.getElementById("emptyState"),
    comparePanel: document.getElementById("comparePanel"),
};

const charts = new BenchmarkCharts({
    accuracy: "accuracyBreakdownChart",
    scatter: "scatterChart",
    deltaDistribution: "deltaDistributionChart",
    trend: "trendChart",
    pattern: "patternChart",
    subPattern: "subPatternChart",
    headToHead: "headToHeadChart",
});

function normalizeLooseName(value) {
    return String(value ?? "").trim().toLowerCase();
}

function normalizePattern(value) {
    return String(value ?? "").trim().toLowerCase();
}

function normalizeSubPattern(value) {
    const text = String(value ?? "").trim();
    return text || "Unsigned";
}

function normalizeScope(value) {
    const text = String(value ?? "").trim().toUpperCase();
    return text === SCOPE_LN ? SCOPE_LN : SCOPE_RC;
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

const GOT_TIER_STEPS = Object.freeze([
    { delta: -0.4, label: "low" },
    { delta: -0.2, label: "mid/low" },
    { delta: 0.0, label: "mid" },
    { delta: 0.2, label: "mid/high" },
    { delta: 0.4, label: "high" },
]);

const GOT_BASE_LABELS = Object.freeze({
    11: "Alpha",
    12: "Beta",
    13: "Gamma",
    14: "Delta",
    15: "Epsilon",
    16: "Zeta",
    17: "Eta",
    18: "Theta",
    19: "iota",
    20: "kappa",
});

function pickNearestGotTier(decimalPart) {
    let best = GOT_TIER_STEPS[0];
    let distance = Number.POSITIVE_INFINITY;

    for (const tier of GOT_TIER_STEPS) {
        const nextDistance = Math.abs(decimalPart - tier.delta);
        if (nextDistance < distance) {
            distance = nextDistance;
            best = tier;
        }
    }

    return best;
}

function formatGotDifficultyFromNumeric(value, row) {
    if (!Number.isFinite(value)) {
        return "-";
    }

    const tier = pickNearestGotTier(value - Math.round(value));
    const base = Math.round(value - tier.delta);

    if (normalizePattern(row?.pattern) === "ln") {
        return `${base} ${tier.label}`;
    }

    if (base === -2) {
        return `Intro 1 ${tier.label}`;
    }
    if (base === -1) {
        return `Intro 2 ${tier.label}`;
    }
    if (base === 0) {
        return `Intro 3 ${tier.label}`;
    }
    if (base >= 1 && base <= 10) {
        return `Reform ${base} ${tier.label}`;
    }

    const baseLabel = GOT_BASE_LABELS[base] || String(base);
    return `${baseLabel} ${tier.label}`;
}

function setStatus(message, level) {
    dom.statusBadge.className = `badge ${level}`;
    dom.statusBadge.textContent = message;
}

function setDatasetInfo(text) {
    dom.datasetInfo.textContent = text;
}

function formatGeneratedAt(value) {
    if (!value) {
        return "Unknown";
    }

    try {
        const parsed = new Date(value);
        if (!Number.isNaN(parsed.getTime())) {
            return parsed.toLocaleString();
        }
    } catch {
        // keep raw value as fallback
    }

    return String(value);
}

function hasValidBid(row) {
    return Number.isInteger(row?.bid) && row.bid > 0;
}

function sanitizeFileNameToken(value, fallback = "dataset") {
    const text = String(value ?? "").trim();
    if (!text) {
        return fallback;
    }

    const normalized = text
        .toLowerCase()
        .replace(/[^a-z0-9]+/g, "-")
        .replace(/-+/g, "-")
        .replace(/^-|-$/g, "");

    return normalized || fallback;
}

function buildCurrentDataExportPayload() {
    return {
        formatVersion: 1,
        exportedAt: new Date().toISOString(),
        dashboard: {
            title: "Estimator Algorithm Benchmark",
            url: window.location.href,
        },
        selection: {
            algorithm: state.currentAlgorithm,
            baseScope: state.baseMode,
            compareAlgorithm: state.compareAlgorithm,
            compareScope: state.compareMode,
        },
        filters: {
            search: String(dom.searchInput.value || "").trim(),
            pattern: dom.patternFilter.value,
            subPattern: dom.subPatternFilter.value,
            band: dom.bandFilter.value,
        },
        sorting: {
            key: state.sortKey,
            direction: state.sortDirection,
        },
        ui: {
            sourceHint: String(dom.sourceHint.textContent || ""),
            statusBadge: String(dom.statusBadge.textContent || ""),
            datasetInfo: String(dom.datasetInfo.textContent || ""),
            tableMeta: String(dom.tableMeta.textContent || ""),
        },
        counters: {
            catalogCount: state.catalog.size,
            baseRows: state.baseRows.length,
            compareRows: state.compareRows.length,
            scopedBaseRows: state.scopedBaseRows.length,
            scopedCompareRows: state.scopedCompareRows.length,
            displayRows: state.displayRows.length,
            filteredRows: state.filteredRows.length,
            errorRows: state.errorRows.length,
        },
        bandOrder: BAND_ORDER,
        bandMeta: BAND_META,
        summary: state.summary,
        compareSummary: state.compareSummary,
        catalog: Array.from(state.catalog.entries()).map(([algorithm, descriptor]) => ({
            algorithm,
            ...descriptor,
        })),
        rows: {
            scopedBaseRows: state.scopedBaseRows,
            scopedCompareRows: state.scopedCompareRows,
            displayRows: state.displayRows,
            filteredRows: state.filteredRows,
            errorRows: state.errorRows,
        },
    };
}

function downloadCurrentDataSnapshot() {
    const payload = buildCurrentDataExportPayload();
    const baseToken = sanitizeFileNameToken(state.currentAlgorithm, "unknown");
    const compareToken = state.compareAlgorithm
        ? `-vs-${sanitizeFileNameToken(state.compareAlgorithm, "unknown")}`
        : "";
    const timestamp = new Date().toISOString().replace(/[:.]/g, "-");
    const fileName = `benchmark-current-data-${baseToken}${compareToken}-${timestamp}.json`;

    const content = JSON.stringify(payload, null, 2);
    const blob = new Blob([content], { type: "application/json;charset=utf-8" });
    const downloadUrl = URL.createObjectURL(blob);

    const anchor = document.createElement("a");
    anchor.href = downloadUrl;
    anchor.download = fileName;
    anchor.style.display = "none";
    document.body.appendChild(anchor);
    anchor.click();
    anchor.remove();

    URL.revokeObjectURL(downloadUrl);
    return fileName;
}

function sanitizeNameForSearch(name) {
    let text = String(name ?? "").trim();
    if (!text) {
        return "";
    }

    let previous = "";
    while (previous !== text) {
        previous = text;
        text = text.replace(/\([^()]*\)|\[[^\[\]]*\]/g, " ");
    }

    text = text.replace(/\bx\s*\d+(?:\.\d+)?\b/gi, " ");
    text = text.replace(/\b\d+(?:\.\d+)?\s*x\b/gi, " ");
    return text.replace(/\s+/g, " ").trim();
}

function getMapSearchUrl(name) {
    const keyword = sanitizeNameForSearch(name);
    if (!keyword) {
        return "https://osu.ppy.sh/beatmapsets?m=3&s=any";
    }

    return `https://osu.ppy.sh/beatmapsets?m=3&q=${encodeURIComponent(keyword)}&s=any`;
}

function getBeatmapDownloadUrl(bid) {
    return `https://osu.ppy.sh/osu/${encodeURIComponent(String(bid))}`;
}

function setCompareUiVisible(visible) {
    if (dom.comparePanel) {
        dom.comparePanel.classList.toggle("hidden", !visible);
    }

    const compareCells = document.querySelectorAll(".compare-col");
    compareCells.forEach((cell) => {
        cell.classList.toggle("hidden", !visible);
    });

    if (!visible && ["compareGot", "compareDeltaAbs", "better"].includes(state.sortKey)) {
        state.sortKey = "deltaAbs";
        state.sortDirection = "asc";
        updateSortVisual();
    }
}

function setFieldVisible(field, visible) {
    if (!field) {
        return;
    }
    field.classList.toggle("hidden", !visible);
}

function isRowValidForStats(row) {
    const expected = row.expected;
    const got = row.got;
    return Number.isFinite(expected) && Number.isFinite(got);
}

function buildAlgorithmMeta(rows) {
    const lnRows = rows.filter((row) => normalizePattern(row.pattern) === "ln");
    const lnValidRows = lnRows.filter((row) => isRowValidForStats(row));

    return {
        lnTotal: lnRows.length,
        lnValid: lnValidRows.length,
        hasUsableLn: lnRows.length > 0 && lnValidRows.length > 0,
    };
}

function normalizeLoadedRows(rows) {
    return rows.map((row) => ({
        ...row,
        pattern: String(row.pattern ?? "").trim(),
        subPattern: normalizeSubPattern(row.subPattern),
        gotRaw: String(row.gotRaw ?? "").trim(),
    }));
}

function cacheRowsForAlgorithm(algorithm, rows) {
    const normalizedRows = normalizeLoadedRows(rows);
    state.cache.set(algorithm, normalizedRows);
    state.metaCache.set(algorithm, buildAlgorithmMeta(normalizedRows));
    return normalizedRows;
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
        segments.push(`Remote ${remoteCount}`);
    }
    if (localCount > 0) {
        segments.push(`Local ${localCount}`);
    }
    if (!segments.length) {
        segments.push("No Datasets");
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

function clearRemoteCatalogEntries() {
    for (const [algorithm, descriptor] of state.catalog.entries()) {
        if (descriptor.source !== "local") {
            state.catalog.delete(algorithm);
            state.cache.delete(algorithm);
            state.metaCache.delete(algorithm);
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

function fillSubPatternFilter(rows) {
    const previous = dom.subPatternFilter.value;
    const subPatterns = [...new Set(rows.map((row) => normalizeSubPattern(row.subPattern)).filter(Boolean))]
        .sort((a, b) => a.localeCompare(b, undefined, { sensitivity: "base" }));

    dom.subPatternFilter.innerHTML = "";

    const allOption = document.createElement("option");
    allOption.value = "all";
    allOption.textContent = "All";
    dom.subPatternFilter.appendChild(allOption);

    for (const subPattern of subPatterns) {
        const option = document.createElement("option");
        option.value = subPattern;
        option.textContent = subPattern;
        dom.subPatternFilter.appendChild(option);
    }

    dom.subPatternFilter.value = subPatterns.includes(previous) ? previous : "all";
}

function getRowBand(row) {
    if (getRowErrorInfo(row)) {
        return "error";
    }

    return classifyBand(row.deltaAbs);
}

function parseErrorInfoFromRawGot(rawGot) {
    const raw = String(rawGot ?? "").trim();

    if (!raw) {
        return {
            type: "Failed",
            detail: "Got value is empty.",
            raw,
        };
    }

    const invalidMatch = raw.match(/^invalid\b\s*[:：-]?\s*(.*)$/i);
    if (invalidMatch) {
        return {
            type: "Invalid",
            detail: invalidMatch[1] ? invalidMatch[1].trim() : "Difficulty label contains bound symbol.",
            raw,
        };
    }

    const failedMatch = raw.match(/^failed\b\s*[:：-]?\s*(.*)$/i);
    if (failedMatch) {
        return {
            type: "Failed",
            detail: failedMatch[1] ? failedMatch[1].trim() : "Estimator execution or parsing failed.",
            raw,
        };
    }

    const missingMatch = raw.match(/^missing\b\s*[:：-]?\s*(.*)$/i);
    if (missingMatch) {
        return {
            type: "Missing",
            detail: missingMatch[1]
                ? missingMatch[1].trim()
                : "Local map file is missing.",
            raw,
        };
    }

    return {
        type: "Failed",
        detail: raw,
        raw,
    };
}

function getRowErrorInfo(row) {
    const got = row.got;
    if (Number.isFinite(got)) {
        return null;
    }

    return parseErrorInfoFromRawGot(row.gotRaw);
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

function applyScopeRows(rows, scope) {
    if (normalizeScope(scope) === SCOPE_LN) {
        return rows.filter((row) => normalizePattern(row.pattern) === "ln");
    }

    return rows.filter((row) => normalizePattern(row.pattern) !== "ln");
}

function syncBaseScopeVisibility(rows) {
    const meta = buildAlgorithmMeta(rows);
    state.metaCache.set(state.currentAlgorithm, meta);

    const showLnChoice = meta.hasUsableLn;
    setFieldVisible(dom.baseCategoryField, showLnChoice);

    if (!showLnChoice) {
        state.baseMode = SCOPE_RC;
    }

    dom.baseCategorySelect.value = state.baseMode;
}

function syncCompareScopeVisibility(rows) {
    if (!state.compareAlgorithm) {
        state.compareMode = SCOPE_RC;
        setFieldVisible(dom.compareCategoryField, false);
        dom.compareCategorySelect.value = state.compareMode;
        return;
    }

    const meta = buildAlgorithmMeta(rows);
    state.metaCache.set(state.compareAlgorithm, meta);

    const showLnChoice = meta.hasUsableLn;
    setFieldVisible(dom.compareCategoryField, showLnChoice);

    if (!showLnChoice) {
        state.compareMode = SCOPE_RC;
    }

    dom.compareCategorySelect.value = state.compareMode;
}

function mergeRowsForDisplay(baseRows, compareRows) {
    if (!state.compareAlgorithm || !Array.isArray(compareRows) || compareRows.length === 0) {
        return baseRows.map((row) => ({
            ...row,
            compareGot: null,
            compareGotRaw: "",
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
                compareGotRaw: "",
                compareDeltaAbs: null,
                better: "na",
            };
        }

        const baseDeltaAbs = Number.isFinite(row.deltaAbs) ? row.deltaAbs : null;
        const compareDeltaAbs = Number.isFinite(peer.deltaAbs) ? peer.deltaAbs : null;

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
            compareGot: Number.isFinite(peer.got) ? peer.got : null,
            compareGotRaw: String(peer.gotRaw || ""),
            compareDeltaAbs,
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

    dom.exactCountValue.textContent = `${summary.bandCounts.exact} maps`;
    dom.closeCountValue.textContent = `${summary.bandCounts.close} maps`;
    dom.moderateCountValue.textContent = `${summary.bandCounts.moderate} maps`;
    dom.missCountValue.textContent = `${summary.bandCounts.miss} maps`;
}

function renderInsightList(target, rows, direction) {
    if (!rows.length) {
        target.innerHTML = '<li class="insight-empty">No maps with valid values.</li>';
        return;
    }

    target.innerHTML = rows
        .map((row) => {
            const sign = direction === "positive" ? "+" : "";
            return [
                "<li>",
                `<strong>${escapeHtml(row.name)}</strong>`,
                `<span class="muted"> (${escapeHtml(row.pattern || "-")})</span>`,
                `<br><span class="muted">delta ${sign}${formatNumber(row.delta, 3)} | expected ${formatNumber(row.expected, 3)} | got ${formatNumber(row.got, 3)}</span>`,
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

    dom.compareStatusText.textContent = `${state.currentAlgorithm}[${state.baseMode}] vs ${state.compareAlgorithm}[${state.compareMode}]`;
    dom.compareMatchedValue.textContent = String(compareSummary.matchedRows);
    dom.compareBaseWinsValue.textContent = String(compareSummary.baseWins);
    dom.compareOtherWinsValue.textContent = String(compareSummary.compareWins);
    dom.compareTieValue.textContent = String(compareSummary.tieCount);
    dom.compareAgreementValue.textContent = formatPercent(compareSummary.agreementRate);
    dom.compareMaeGapValue.textContent = formatSigned(compareSummary.maeGap, 3);
}

function renderErrorPanel(rows) {
    const errors = [];

    for (const row of rows) {
        const info = getRowErrorInfo(row);
        if (!info) {
            continue;
        }

        errors.push({
            ...row,
            errorType: info.type,
            errorDetail: info.detail,
            errorRaw: info.raw,
        });
    }

    state.errorRows = errors;

    let invalidCount = 0;
    let failedCount = 0;
    let missingCount = 0;

    for (const row of errors) {
        if (row.errorType === "Invalid") {
            invalidCount += 1;
        } else if (row.errorType === "Missing") {
            missingCount += 1;
        } else {
            failedCount += 1;
        }
    }

    dom.errorInvalidCount.textContent = String(invalidCount);
    dom.errorFailedCount.textContent = String(failedCount);
    dom.errorMissingCount.textContent = String(missingCount);

    if (!errors.length) {
        dom.errorStatusText.textContent = "No error maps in current algorithm scope.";
        dom.errorTableBody.innerHTML = "";
        dom.errorEmptyState.hidden = false;
        return;
    }

    dom.errorStatusText.textContent = `${errors.length} error maps in current algorithm scope.`;
    dom.errorEmptyState.hidden = true;

    dom.errorTableBody.innerHTML = errors
        .map((row) => {
            const rowClass = String(row.errorType || "Failed").toLowerCase();
            const expectedText = String(row.expectedRaw || "").trim() || formatNumber(row.expected);
            const detailText = String(row.errorDetail || "").trim();
            const compactDetail = detailText.length > 120
                ? `${detailText.slice(0, 117)}...`
                : detailText;
            return [
                `<tr class="error-${rowClass}">`,
                `<td>${escapeHtml(row.name)}</td>`,
                `<td>${escapeHtml(row.pattern)}</td>`,
                `<td>${escapeHtml(normalizeSubPattern(row.subPattern))}</td>`,
                `<td>${escapeHtml(expectedText)}</td>`,
                `<td>${escapeHtml(row.errorRaw)}</td>`,
                `<td>${escapeHtml(row.errorType)}</td>`,
                `<td>${escapeHtml(compactDetail)}</td>`,
                "</tr>",
            ].join("");
        })
        .join("");
}

function compareValues(a, b, key) {
    if (key === "band") {
        const rank = {
            exact: 0,
            close: 1,
            moderate: 2,
            miss: 3,
            error: 4,
        };
        return (rank[getRowBand(a)] ?? 99) - (rank[getRowBand(b)] ?? 99);
    }

    if (key === "better") {
        const rank = { base: 0, compare: 1, tie: 2, na: 3 };
        return (rank[a.better] ?? 3) - (rank[b.better] ?? 3);
    }

    const numericKeys = new Set(["expected", "got", "delta", "deltaAbs", "compareGot", "compareDeltaAbs"]);
    if (numericKeys.has(key)) {
        const aVal = a[key];
        const bVal = b[key];
        const aFinite = Number.isFinite(aVal);
        const bFinite = Number.isFinite(bVal);

        if (!aFinite && !bFinite) {
            return String(a[key] ?? "").localeCompare(String(b[key] ?? ""));
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
        const aErrorPriority = getRowErrorInfo(a) ? 1 : 0;
        const bErrorPriority = getRowErrorInfo(b) ? 1 : 0;
        if (aErrorPriority !== bErrorPriority) {
            return aErrorPriority - bErrorPriority;
        }

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
            const bandLabel = bandKey === "error"
                ? "Error"
                : (BAND_META[bandKey]?.label || "Miss");
            const winnerLabel = getWinnerLabel(row.better);
            const winnerClass = row.better || "na";
            const hasBid = hasValidBid(row);
            const searchUrl = getMapSearchUrl(row.name);
            const downloadUrl = hasBid ? getBeatmapDownloadUrl(row.bid) : "";

            const gotValue = row.got;
            const gotText = Number.isFinite(gotValue)
                ? formatGotDifficultyFromNumeric(gotValue, row)
                : (String(row.gotRaw || "").trim() || "-");
            const gotNumericText = Number.isFinite(gotValue) ? formatNumber(gotValue) : "";

            const compareGotValue = row.compareGot;
            const compareGotText = Number.isFinite(compareGotValue)
                ? formatNumber(compareGotValue)
                : (String(row.compareGotRaw || "").trim() || "-");

            return [
                `<tr class="band-${bandKey}${bandKey === "error" ? " map-error" : ""}">`,
                `<td>${escapeHtml(row.name)}</td>`,
                `<td class="num">${formatNumber(row.expected)}</td>`,
                Number.isFinite(gotValue)
                    ? `<td class="got-cell has-hover-value"><span class="got-label">${escapeHtml(gotText)}</span><span class="got-number">${escapeHtml(gotNumericText)}</span></td>`
                    : `<td class="got-cell">${escapeHtml(gotText)}</td>`,
                `<td class="num">${formatSigned(row.delta)}</td>`,
                `<td class="num">${formatNumber(row.deltaAbs)}</td>`,
                `<td>${escapeHtml(row.pattern)}</td>`,
                `<td>${escapeHtml(normalizeSubPattern(row.subPattern))}</td>`,
                `<td class="band">${bandLabel}</td>`,
                `<td class="compare-col">${escapeHtml(compareGotText)}</td>`,
                `<td class="num compare-col">${formatNumber(row.compareDeltaAbs)}</td>`,
                `<td class="winner ${winnerClass} compare-col">${escapeHtml(winnerLabel)}</td>`,
                `<td class="actions-col"><div class="row-actions"><a class="icon-btn" href="${escapeHtml(searchUrl)}" target="_blank" rel="noopener noreferrer" title="Search beatmapsets">🔎</a>${hasBid
                    ? `<a class="icon-btn" href="${escapeHtml(downloadUrl)}" target="_blank" rel="noopener noreferrer" title="Download .osu">⬇</a>`
                    : '<span class="icon-btn disabled" title="No bid">⬇</span>'}</div></td>`,
                "</tr>",
            ].join("");
        })
        .join("");
}

function applyFilters() {
    const searchText = String(dom.searchInput.value || "").trim().toLowerCase();
    const selectedPattern = dom.patternFilter.value;
    const selectedSubPattern = dom.subPatternFilter.value;
    const selectedBand = dom.bandFilter.value;

    const filtered = state.displayRows.filter((row) => {
        if (selectedPattern && selectedPattern !== "all" && row.pattern !== selectedPattern) {
            return false;
        }

        if (selectedSubPattern && selectedSubPattern !== "all" && normalizeSubPattern(row.subPattern) !== selectedSubPattern) {
            return false;
        }

        if (selectedBand && selectedBand !== "all" && getRowBand(row) !== selectedBand) {
            return false;
        }

        if (!searchText) {
            return true;
        }

        const rowError = getRowErrorInfo(row);
        const haystack = [
            row.name,
            row.pattern,
            normalizeSubPattern(row.subPattern),
            row.gotRaw,
            rowError?.type,
            rowError?.detail,
        ]
            .map((part) => String(part || "").toLowerCase())
            .join(" ");

        return haystack.includes(searchText);
    });

    state.filteredRows = sortRows(filtered);
    renderTable(state.filteredRows);
    setCompareUiVisible(Boolean(state.compareAlgorithm));

    dom.tableMeta.textContent = `${state.filteredRows.length} / ${state.displayRows.length} Maps Shown | Errors=${state.errorRows.length}`;
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

    currentUrl.searchParams.set("scope", state.baseMode);

    if (state.compareAlgorithm) {
        currentUrl.searchParams.set("compareScope", state.compareMode);
    } else {
        currentUrl.searchParams.delete("compareScope");
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
            modifiedAt: entry.modifiedAt,
        });
    }

    rebuildAlgorithmList();
    updateSourceHint(sourceLabel ? `Discovered by ${sourceLabel}` : "");

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
    return cacheRowsForAlgorithm(algorithm, parsed.rows);
}

function buildFriendlyFetchHint(originalMessage) {
    const message = String(originalMessage || "");
    const isFetchFailure = /fetch|Failed to fetch|not allowed|scheme/i.test(message);

    if (window.location.protocol === "file:" && isFetchFailure) {
        return [
            "Direct file mode blocks fetch in some Edge contexts.",
            "Click Upload Data Folder and select docs/data to load CSV files locally.",
        ].join(" ");
    }

    return message;
}

function applyEmptyDashboard() {
    const emptySummary = computeSummary([]);
    state.baseRows = [];
    state.compareRows = [];
    state.scopedBaseRows = [];
    state.scopedCompareRows = [];
    state.displayRows = [];
    state.summary = emptySummary;
    state.compareSummary = null;

    updateSummary(emptySummary);
    updateInsightLists(emptySummary);
    updateCompareSummary(null);
    renderErrorPanel([]);
    setCompareUiVisible(Boolean(state.compareAlgorithm));
    fillPatternFilter([]);
    fillSubPatternFilter([]);
    renderTable([]);
    charts.render(emptySummary, null);
    dom.tableMeta.textContent = "No Data Loaded.";
}

async function loadCurrentView(options = {}) {
    const forceReload = Boolean(options.forceReload);

    if (!state.currentAlgorithm) {
        setStatus("Waiting", "warn");
        setDatasetInfo("No Algorithm Selected.");
        applyEmptyDashboard();
        return;
    }

    setStatus("Loading...", "warn");
    setDatasetInfo(`Loading ${state.currentAlgorithm}...`);

    try {
        state.baseRows = await ensureRowsLoaded(state.currentAlgorithm, forceReload);
        syncBaseScopeVisibility(state.baseRows);

        if (state.compareAlgorithm) {
            state.compareRows = await ensureRowsLoaded(state.compareAlgorithm, forceReload);
        } else {
            state.compareRows = [];
        }
        syncCompareScopeVisibility(state.compareRows);

        state.scopedBaseRows = applyScopeRows(state.baseRows, state.baseMode);
        state.scopedCompareRows = state.compareAlgorithm
            ? applyScopeRows(state.compareRows, state.compareMode)
            : [];

        state.summary = computeSummary(state.scopedBaseRows);
        state.compareSummary = state.compareAlgorithm
            ? computeHeadToHead(state.scopedBaseRows, state.scopedCompareRows)
            : null;

        state.displayRows = mergeRowsForDisplay(state.scopedBaseRows, state.scopedCompareRows);
        setCompareUiVisible(Boolean(state.compareAlgorithm));

        updateSummary(state.summary);
        updateInsightLists(state.summary);
        updateCompareSummary(state.compareSummary);
        renderErrorPanel(state.scopedBaseRows);

        fillPatternFilter(state.scopedBaseRows);
        fillSubPatternFilter(state.scopedBaseRows);

        applyFilters();
        charts.render(state.summary, state.compareSummary);

        const descriptor = state.catalog.get(state.currentAlgorithm);
        const generatedAtText = formatGeneratedAt(descriptor?.modifiedAt);
        const compareText = state.compareAlgorithm
            ? ` vs ${state.compareAlgorithm}[${state.compareMode}]`
            : "";

        setStatus("Ready", "ok");
        setDatasetInfo(
            `${state.currentAlgorithm}[${state.baseMode}]${compareText}`
            + ` | Maps=${state.scopedBaseRows.length} | Errors=${state.errorRows.length} | Generated At ${generatedAtText}`,
        );

        syncUrlParams();
    } catch (error) {
        applyEmptyDashboard();
        setStatus("Error", "error");
        setDatasetInfo(`${state.currentAlgorithm} | ${buildFriendlyFetchHint(toErrorMessage(error))}`);
    }
}

async function loadLocalDatasets(fileList) {
    const files = Array.from(fileList || []).filter((file) => isCsvFileName(file.name));
    if (!files.length) {
        setStatus("Waiting", "warn");
        setDatasetInfo("No CSV Files Found In Selected Folder.");
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
                modifiedAt: Number.isFinite(file.lastModified)
                    ? new Date(file.lastModified).toISOString()
                    : null,
            });

            cacheRowsForAlgorithm(algorithm, parsed.rows);
            imported += 1;
        } catch {
            // Continue importing remaining files.
        }
    }

    rebuildAlgorithmList();

    if (!state.algorithms.length) {
        setStatus("Error", "error");
        setDatasetInfo("Local Import Finished But No Valid CSV Was Parsed.");
        applyEmptyDashboard();
        return;
    }

    if (!state.currentAlgorithm || !state.algorithms.includes(state.currentAlgorithm)) {
        state.currentAlgorithm = state.algorithms[0];
        state.baseMode = SCOPE_RC;
    }

    if (state.compareAlgorithm && !state.algorithms.includes(state.compareAlgorithm)) {
        state.compareAlgorithm = "";
        state.compareMode = SCOPE_RC;
    }

    renderAlgorithmSelectors();
    updateSortVisual();
    updateSourceHint(`Local Import ${imported} File(s)`);

    await loadCurrentView();
}

function bindEvents() {
    dom.algorithmSelect.addEventListener("change", async () => {
        state.currentAlgorithm = dom.algorithmSelect.value;
        state.baseMode = SCOPE_RC;

        if (state.compareAlgorithm === state.currentAlgorithm) {
            state.compareAlgorithm = "";
        }

        renderCompareOptions();
        await loadCurrentView();
    });

    dom.baseCategorySelect.addEventListener("change", async () => {
        state.baseMode = normalizeScope(dom.baseCategorySelect.value);
        await loadCurrentView();
    });

    dom.compareAlgorithmSelect.addEventListener("change", async () => {
        state.compareAlgorithm = dom.compareAlgorithmSelect.value || "";
        state.compareMode = SCOPE_RC;
        await loadCurrentView();
    });

    dom.compareCategorySelect.addEventListener("change", async () => {
        state.compareMode = normalizeScope(dom.compareCategorySelect.value);
        await loadCurrentView();
    });

    dom.reloadDataButton.addEventListener("click", async () => {
        await refreshRemoteCatalog();

        if (state.currentAlgorithm && !state.algorithms.includes(state.currentAlgorithm)) {
            state.currentAlgorithm = state.algorithms[0] || null;
            state.baseMode = SCOPE_RC;
        }

        if (state.compareAlgorithm && !state.algorithms.includes(state.compareAlgorithm)) {
            state.compareAlgorithm = "";
            state.compareMode = SCOPE_RC;
        }

        renderAlgorithmSelectors();
        updateSortVisual();
        await loadCurrentView({ forceReload: true });
    });

    dom.openDataFolderButton.addEventListener("click", () => {
        dom.dataFileInput.value = "";
        dom.dataFileInput.click();
    });

    if (dom.downloadCurrentDataButton) {
        dom.downloadCurrentDataButton.addEventListener("click", () => {
            try {
                const fileName = downloadCurrentDataSnapshot();
                setStatus("Ready", "ok");
                setDatasetInfo(`Exported current dashboard data to ${fileName}`);
            } catch (error) {
                setStatus("Error", "error");
                setDatasetInfo(`Export failed: ${toErrorMessage(error)}`);
            }
        });
    }

    dom.dataFileInput.addEventListener("change", async (event) => {
        const files = event.target?.files;
        await loadLocalDatasets(files);
    });

    dom.searchInput.addEventListener("input", applyFilters);
    dom.patternFilter.addEventListener("change", applyFilters);
    dom.subPatternFilter.addEventListener("change", applyFilters);
    dom.bandFilter.addEventListener("change", applyFilters);

    dom.clearFilterButton.addEventListener("click", () => {
        dom.searchInput.value = "";
        dom.patternFilter.value = "all";
        dom.subPatternFilter.value = "all";
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
    setDatasetInfo("Discovering Datasets From docs/data...");
    updateSourceHint();

    const discoveredRemoteCount = await refreshRemoteCatalog();
    renderAlgorithmSelectors();
    updateSortVisual();

    if (!state.algorithms.length) {
        setStatus("Waiting", "warn");
        setDatasetInfo("No Dataset Discovered. Click Upload Data Folder And Select docs/data.");
        applyEmptyDashboard();
        return;
    }

    const search = new URLSearchParams(window.location.search);
    const requestedBase = findAlgorithmByLooseName(search.get("algorithm"));
    const requestedCompare = findAlgorithmByLooseName(search.get("compare"));

    state.baseMode = normalizeScope(search.get("scope"));
    state.compareMode = normalizeScope(search.get("compareScope"));

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
        setDatasetInfo("Local-file Mode Detected. Use Upload Data Folder To Import CSVs From docs/data.");
    }
}

window.benchmarkDashboardApi = Object.freeze({
    getCurrentDataSnapshot: buildCurrentDataExportPayload,
    downloadCurrentDataSnapshot,
});

init();
