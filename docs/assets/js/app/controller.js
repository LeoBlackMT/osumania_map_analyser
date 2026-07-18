import { computeHeadToHead, computeSummary } from "../stats.js";
import { initI18n, onLanguageChange, t } from "../i18n.js";
import {
    charts,
    dom,
    SCOPE_ALL,
    SCOPE_LN,
    SCOPE_RC,
    state,
} from "./state.js";
import {
    normalizeScope,
    toErrorMessage,
} from "./utils.js";
import {
    applyScopeRows,
    buildAlgorithmMeta,
    filterDisplayRows,
    getActiveFilters,
    hasActiveFilters,
    mergeRowsForDisplay,
    pickFilteredCompareRows,
    sortRows,
} from "./model.js";
import {
    ensureRowsLoaded,
    findAlgorithmByLooseName,
    importLocalDatasets,
    refreshRemoteCatalog,
} from "./catalog.js";
import {
    fillPatternFilter,
    fillSubPatternFilter,
    renderAlgorithmSelectors,
    renderErrorPanel,
    renderTable,
    setCompareUiVisible,
    setDatasetInfo,
    setFieldVisible,
    setReadyDatasetInfo,
    setStatus,
    updateCompareSummary,
    updateInsightLists,
    updateSortVisual,
    updateSourceHint,
    updateSummary,
} from "./view.js";
import { downloadCurrentDataSnapshot, exposeDashboardApi } from "./exporter.js";

let viewRequestSeq = 0;
let filterRenderTimerId = 0;
const FILTER_RENDER_DEBOUNCE_MS = 120;

function syncBaseScopeVisibility(rows) {
    const meta = buildAlgorithmMeta(rows);
    state.metaCache.set(state.currentAlgorithm, meta);

    const showLnChoice = meta.hasUsableLn;
    setFieldVisible(dom.baseCategoryField, showLnChoice);

    if (!showLnChoice && state.baseMode === SCOPE_LN) {
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

    if (!showLnChoice && state.compareMode === SCOPE_LN) {
        state.compareMode = SCOPE_RC;
    }

    dom.compareCategorySelect.value = state.compareMode;
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

function buildFriendlyFetchHint(originalMessage) {
    const message = String(originalMessage || "");
    const isFetchFailure = /fetch|Failed to fetch|not allowed|scheme/i.test(message);

    if (window.location.protocol === "file:" && isFetchFailure) {
        return [
            t("hint.fileMode.fetchBlockedLine1", "Direct file mode blocks fetch in some Edge contexts."),
            t("hint.fileMode.fetchBlockedLine2", "Click Upload Data Folder and select docs/data to load CSV files locally."),
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
    state.allDisplayRows = [];
    state.displayRows = [];
    state.filteredRows = [];
    state.fullSummary = emptySummary;
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
    charts.render(emptySummary, null, {
        activeFilters: getActiveFilters(dom),
        fullSummary: emptySummary,
    });
    dom.tableMeta.textContent = t("index.table.noData", "No Data Loaded.");
}

function applyFiltersAndRender() {
    const filters = getActiveFilters(dom);
    const filteredAll = sortRows(
        filterDisplayRows(state.displayRows, filters),
        state.sortKey,
        state.sortDirection,
    );
    const filteredValid = filteredAll.filter((row) => !row._errorInfo);

    state.filteredRows = filteredValid;
    renderTable(filteredValid);
    setCompareUiVisible(Boolean(state.compareAlgorithm));

    const activeSummary = computeSummary(filteredAll);
    state.summary = activeSummary;

    const filteredCompareRows = state.compareAlgorithm
        ? pickFilteredCompareRows(filteredValid, state.scopedCompareRows)
        : [];

    state.compareSummary = state.compareAlgorithm
        ? computeHeadToHead(filteredValid, filteredCompareRows)
        : null;

    updateSummary(activeSummary);
    updateInsightLists(activeSummary);
    updateCompareSummary(state.compareSummary);

    const errorFilters = {
        ...filters,
        band: "all",
    };
    const errorScopeRows = filterDisplayRows(state.displayRows, errorFilters);
    renderErrorPanel(errorScopeRows);

    charts.render(activeSummary, state.compareSummary, {
        activeFilters: filters,
        fullSummary: state.fullSummary,
        dimUnselected: hasActiveFilters(filters),
    });

    dom.tableMeta.textContent = t(
        "index.table.meta",
        "{shownValid} / {shownTotal} Valid Maps Shown | Total In Scope={scopeTotal} | Errors={errorCount}",
        {
            shownValid: filteredValid.length,
            shownTotal: filteredAll.length,
            scopeTotal: state.displayRows.length,
            errorCount: state.errorRows.length,
        },
    );

    if (state.currentAlgorithm) {
        setReadyDatasetInfo(activeSummary, state.errorRows.length);
    }
}

function scheduleFilterRender() {
    if (filterRenderTimerId) {
        clearTimeout(filterRenderTimerId);
    }
    filterRenderTimerId = setTimeout(() => {
        filterRenderTimerId = 0;
        applyFiltersAndRender();
    }, FILTER_RENDER_DEBOUNCE_MS);
}

async function loadCurrentView(options = {}) {
    const requestSeq = ++viewRequestSeq;
    const isStaleRequest = () => requestSeq !== viewRequestSeq;
    const forceReload = Boolean(options.forceReload);
    const baseAlgorithm = state.currentAlgorithm;
    const compareAlgorithm = state.compareAlgorithm;

    if (!baseAlgorithm) {
        setStatus(t("status.waiting", "Waiting"), "warn");
        setDatasetInfo(t("status.noAlgorithmSelected", "No Algorithm Selected."));
        applyEmptyDashboard();
        return;
    }

    setStatus(t("status.loading", "Loading..."), "warn");
    setDatasetInfo(t("status.loadingAlgorithm", "Loading {algorithm}...", { algorithm: baseAlgorithm }));

    try {
        const baseRows = await ensureRowsLoaded(baseAlgorithm, forceReload);
        if (isStaleRequest()) return;

        let compareRows = [];
        if (compareAlgorithm) {
            compareRows = await ensureRowsLoaded(compareAlgorithm, forceReload);
            if (isStaleRequest()) return;
        }

        state.baseRows = baseRows;
        state.compareRows = compareRows;
        syncBaseScopeVisibility(state.baseRows);

        syncCompareScopeVisibility(state.compareRows);

        state.scopedBaseRows = applyScopeRows(state.baseRows, state.baseMode, SCOPE_LN, SCOPE_ALL);
        state.scopedCompareRows = state.compareAlgorithm
            ? applyScopeRows(state.compareRows, state.compareMode, SCOPE_LN, SCOPE_ALL)
            : [];

        state.allDisplayRows = mergeRowsForDisplay(
            state.scopedBaseRows,
            state.scopedCompareRows,
            Boolean(state.compareAlgorithm),
        );

        state.displayRows = state.allDisplayRows;

        state.fullSummary = computeSummary(state.scopedBaseRows);

        fillPatternFilter(state.scopedBaseRows);
        fillSubPatternFilter(state.scopedBaseRows);
        applyFiltersAndRender();

        setStatus(t("status.ready", "Ready"), "ok");
        syncUrlParams();
    } catch (error) {
        if (isStaleRequest()) return;
        applyEmptyDashboard();
        setStatus(t("status.error", "Error"), "error");
        setDatasetInfo(`${state.currentAlgorithm} | ${buildFriendlyFetchHint(toErrorMessage(error))}`);
    }
}

async function loadLocalDataAndRefresh(fileList) {
    const importedInfo = await importLocalDatasets(fileList);
    if (!importedInfo.hasCsv) {
        setStatus(t("status.waiting", "Waiting"), "warn");
        setDatasetInfo(t("status.noCsvFound", "No CSV Files Found In Selected Folder."));
        return;
    }

    if (!state.algorithms.length) {
        setStatus(t("status.error", "Error"), "error");
        setDatasetInfo(t("status.localImportNoValidCsv", "Local Import Finished But No Valid CSV Was Parsed."));
        applyEmptyDashboard();
        return;
    }

    if (state.currentAlgorithm && !state.algorithms.includes(state.currentAlgorithm)) {
        state.currentAlgorithm = state.algorithms[0];
        state.baseMode = SCOPE_RC;
    }

    if (state.compareAlgorithm && !state.algorithms.includes(state.compareAlgorithm)) {
        state.compareAlgorithm = "";
        state.compareMode = SCOPE_RC;
    }

    renderAlgorithmSelectors();
    updateSortVisual();
    updateSourceHint(t("index.meta.localImportFiles", "Local Import {count} File(s)", { count: importedInfo.imported }));
    await loadCurrentView();
}

function toggleSelectFilter(selectElement, value) {
    if (!value) {
        return;
    }

    const next = selectElement.value === value ? "all" : value;
    selectElement.value = next;
    applyFiltersAndRender();
}

function bindEvents() {
    charts.setInteractionHandlers({
        onBandSelect: (bandKey) => {
            toggleSelectFilter(dom.bandFilter, bandKey);
        },
        onPatternSelect: (patternName) => {
            toggleSelectFilter(dom.patternFilter, patternName);
        },
        onSubPatternSelect: (subPatternName) => {
            toggleSelectFilter(dom.subPatternFilter, subPatternName);
        },
    });

    dom.algorithmSelect.addEventListener("change", async () => {
        state.currentAlgorithm = dom.algorithmSelect.value;
        state.baseMode = SCOPE_RC;

        if (state.compareAlgorithm === state.currentAlgorithm) {
            state.compareAlgorithm = "";
        }

        renderAlgorithmSelectors();
        await loadCurrentView();
    });

    dom.baseCategorySelect.addEventListener("change", async () => {
        state.baseMode = normalizeScope(dom.baseCategorySelect.value, SCOPE_LN, SCOPE_ALL);
        await loadCurrentView();
    });

    dom.compareAlgorithmSelect.addEventListener("change", async () => {
        state.compareAlgorithm = dom.compareAlgorithmSelect.value || "";
        state.compareMode = SCOPE_RC;
        await loadCurrentView();
    });

    dom.compareCategorySelect.addEventListener("change", async () => {
        state.compareMode = normalizeScope(dom.compareCategorySelect.value, SCOPE_LN, SCOPE_ALL);
        await loadCurrentView();
    });

    dom.reloadDataButton.addEventListener("click", async () => {
        const refreshResult = await refreshRemoteCatalog();
        updateSourceHint(refreshResult.sourceLabel
            ? t("index.meta.discoveredBy", "Discovered by {source}", { source: refreshResult.sourceLabel })
            : "");

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
        await loadLocalDataAndRefresh(event.target?.files);
    });

    if (dom.downloadCurrentDataButton) {
        dom.downloadCurrentDataButton.addEventListener("click", () => {
            try {
                const fileName = downloadCurrentDataSnapshot();
                setStatus(t("status.ready", "Ready"), "ok");
                setDatasetInfo(t("status.exportedCurrentData", "Exported current dashboard data to {fileName}", { fileName }));
            } catch (error) {
                setStatus(t("status.error", "Error"), "error");
                setDatasetInfo(t("status.exportFailed", "Export failed: {detail}", { detail: toErrorMessage(error) }));
            }
        });
    }

    dom.searchInput.addEventListener("input", scheduleFilterRender);
    dom.patternFilter.addEventListener("change", applyFiltersAndRender);
    dom.subPatternFilter.addEventListener("change", applyFiltersAndRender);
    dom.bandFilter.addEventListener("change", applyFiltersAndRender);
    dom.expectedMinFilter.addEventListener("input", scheduleFilterRender);
    dom.expectedMaxFilter.addEventListener("input", scheduleFilterRender);
    dom.deltaMinFilter.addEventListener("input", scheduleFilterRender);
    dom.deltaMaxFilter.addEventListener("input", scheduleFilterRender);

    dom.clearFilterButton.addEventListener("click", () => {
        dom.searchInput.value = "";
        dom.patternFilter.value = "all";
        dom.subPatternFilter.value = "all";
        dom.bandFilter.value = "all";
        dom.expectedMinFilter.value = "";
        dom.expectedMaxFilter.value = "";
        dom.deltaMinFilter.value = "";
        dom.deltaMaxFilter.value = "";
        applyFiltersAndRender();
    });

    const sortableHeaders = dom.resultTable.querySelectorAll("thead th[data-sort]");
    sortableHeaders.forEach((header) => {
        const applySort = () => {
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
            applyFiltersAndRender();
        };
        header.tabIndex = 0;
        header.addEventListener("click", applySort);
        header.addEventListener("keydown", (event) => {
            if (event.key === "Enter" || event.key === " ") {
                event.preventDefault();
                applySort();
            }
        });
    });
}

export async function init() {
    await initI18n({ page: "index" });
    bindEvents();
    exposeDashboardApi();

    onLanguageChange(() => {
        renderAlgorithmSelectors();
        updateSortVisual();

        if (state.currentAlgorithm) {
            fillPatternFilter(state.scopedBaseRows);
            fillSubPatternFilter(state.scopedBaseRows);
            applyFiltersAndRender();
        } else {
            applyEmptyDashboard();
        }
    });

    setStatus(t("status.loading", "Loading..."), "warn");
    setDatasetInfo(t("status.discoveringDatasets", "Discovering Datasets From docs/data..."));
    updateSourceHint();

    const refreshResult = await refreshRemoteCatalog();
    updateSourceHint(refreshResult.sourceLabel
        ? t("index.meta.discoveredBy", "Discovered by {source}", { source: refreshResult.sourceLabel })
        : "");

    if (!state.algorithms.length) {
        setStatus(t("status.waiting", "Waiting"), "warn");
        setDatasetInfo(t("status.noDatasetDiscovered", "No Dataset Discovered. Click Upload Data Folder And Select docs/data."));
        applyEmptyDashboard();
        return;
    }

    const search = new URLSearchParams(window.location.search);
    const requestedBase = findAlgorithmByLooseName(search.get("algorithm"));
    const requestedCompare = findAlgorithmByLooseName(search.get("compare"));

    state.baseMode = normalizeScope(search.get("scope"), SCOPE_LN, SCOPE_ALL);
    state.compareMode = normalizeScope(search.get("compareScope"), SCOPE_LN, SCOPE_ALL);

    if (requestedBase) {
        state.currentAlgorithm = requestedBase;
    }

    if (requestedCompare && requestedCompare !== state.currentAlgorithm) {
        state.compareAlgorithm = requestedCompare;
    }

    renderAlgorithmSelectors();
    updateSortVisual();

    await loadCurrentView();

    if (window.location.protocol === "file:" && refreshResult.discoveredCount === 0) {
        setStatus(t("status.ready", "Ready"), "ok");
        setDatasetInfo(t("status.localFileModeHint", "Local-file Mode Detected. Use Upload Data Folder To Import CSVs From docs/data."));
    }
}
