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

function setStatus(message, level) {
    dom.statusBadge.className = `badge ${level}`;
    dom.statusBadge.textContent = message;
}

function setDatasetInfo(text) {
    dom.datasetInfo.textContent = text;
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
    return classifyBand(row.deltaAbs);
}

function parseErrorInfoFromRawGot(rawGot) {
    const raw = String(rawGot ?? "").trim();

    if (!raw) {
        return {
            type: "Failed",
            detail: "got 为空，未得到可解析结果",
            raw,
        };
    }

    const invalidMatch = raw.match(/^invalid\b\s*[:：-]?\s*(.*)$/i);
    if (invalidMatch) {
        return {
            type: "Invalid",
            detail: invalidMatch[1] ? invalidMatch[1].trim() : "估计难度字符串中包含 < 或 >",
            raw,
        };
    }

    const failedMatch = raw.match(/^failed\b\s*[:：-]?\s*(.*)$/i);
    if (failedMatch) {
        return {
            type: "Failed",
            detail: failedMatch[1] ? failedMatch[1].trim() : "估计器运行失败或难度解析失败",
            raw,
        };
    }

    const missingMatch = raw.match(/^missing\b\s*[:：-]?\s*(.*)$/i);
    if (missingMatch) {
        return {
            type: "Missing",
            detail: missingMatch[1] ? missingMatch[1].trim() : "找不到对应谱面文件",
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
        dom.errorStatusText.textContent = "No error rows in current algorithm scope.";
        dom.errorTableBody.innerHTML = "";
        dom.errorEmptyState.hidden = false;
        return;
    }

    dom.errorStatusText.textContent = `${errors.length} error rows in current algorithm scope.`;
    dom.errorEmptyState.hidden = true;

    dom.errorTableBody.innerHTML = errors
        .map((row) => {
            const rowClass = String(row.errorType || "Failed").toLowerCase();
            const expectedText = String(row.expectedRaw || "").trim() || formatNumber(row.expected);
            return [
                `<tr class="error-${rowClass}">`,
                `<td>${escapeHtml(row.name)}</td>`,
                `<td>${escapeHtml(row.pattern)}</td>`,
                `<td>${escapeHtml(normalizeSubPattern(row.subPattern))}</td>`,
                `<td class="num">${escapeHtml(expectedText)}</td>`,
                `<td>${escapeHtml(row.errorRaw)}</td>`,
                `<td>${escapeHtml(row.errorType)}</td>`,
                `<td>${escapeHtml(row.errorDetail)}</td>`,
                "</tr>",
            ].join("");
        })
        .join("");
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

            const gotValue = row.got;
            const gotText = Number.isFinite(gotValue)
                ? formatNumber(gotValue)
                : (String(row.gotRaw || "").trim() || "-");

            const compareGotValue = row.compareGot;
            const compareGotText = Number.isFinite(compareGotValue)
                ? formatNumber(compareGotValue)
                : (String(row.compareGotRaw || "").trim() || "-");

            return [
                `<tr class="band-${bandKey}">`,
                `<td>${escapeHtml(row.name)}</td>`,
                `<td class="num">${formatNumber(row.expected)}</td>`,
                `<td>${escapeHtml(gotText)}</td>`,
                `<td class="num">${formatSigned(row.delta)}</td>`,
                `<td class="num">${formatNumber(row.deltaAbs)}</td>`,
                `<td>${escapeHtml(row.pattern)}</td>`,
                `<td>${escapeHtml(normalizeSubPattern(row.subPattern))}</td>`,
                `<td class="band">${bandLabel}</td>`,
                `<td>${escapeHtml(compareGotText)}</td>`,
                `<td class="num">${formatNumber(row.compareDeltaAbs)}</td>`,
                `<td class="winner ${winnerClass}">${escapeHtml(winnerLabel)}</td>`,
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

    dom.tableMeta.textContent = `${state.filteredRows.length} / ${state.displayRows.length} rows shown | errors=${state.errorRows.length}`;
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
    fillPatternFilter([]);
    fillSubPatternFilter([]);
    renderTable([]);
    charts.render(emptySummary, null);
    dom.tableMeta.textContent = "No data loaded.";
}

async function loadCurrentView(options = {}) {
    const forceReload = Boolean(options.forceReload);

    if (!state.currentAlgorithm) {
        setStatus("Waiting", "warn");
        setDatasetInfo("No algorithm selected.");
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

        updateSummary(state.summary);
        updateInsightLists(state.summary);
        updateCompareSummary(state.compareSummary);
        renderErrorPanel(state.scopedBaseRows);

        fillPatternFilter(state.scopedBaseRows);
        fillSubPatternFilter(state.scopedBaseRows);

        applyFilters();
        charts.render(state.summary, state.compareSummary);

        const nowText = new Date().toLocaleString();
        const compareText = state.compareAlgorithm
            ? ` vs ${state.compareAlgorithm}[${state.compareMode}]`
            : "";

        setStatus("Ready", "ok");
        setDatasetInfo(
            `${state.currentAlgorithm}[${state.baseMode}]${compareText}`
            + ` | rows=${state.scopedBaseRows.length} | errors=${state.errorRows.length} | loaded ${nowText}`,
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

            cacheRowsForAlgorithm(algorithm, parsed.rows);
            imported += 1;
        } catch {
            // Continue importing remaining files.
        }
    }

    rebuildAlgorithmList();

    if (!state.algorithms.length) {
        setStatus("Error", "error");
        setDatasetInfo("Local import finished but no valid CSV was parsed.");
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
    updateSourceHint(`local import ${imported} file(s)`);

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
    setDatasetInfo("Discovering datasets from docs/data...");
    updateSourceHint();

    const discoveredRemoteCount = await refreshRemoteCatalog();
    renderAlgorithmSelectors();
    updateSortVisual();

    if (!state.algorithms.length) {
        setStatus("Waiting", "warn");
        setDatasetInfo("No dataset discovered. Click Upload Data Folder and select docs/data.");
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
        setDatasetInfo("Local-file mode detected. Use Upload Data Folder to import CSVs from docs/data.");
    }
}

init();
