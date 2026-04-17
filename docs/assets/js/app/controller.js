import { computeHeadToHead, computeSummary } from "../stats.js";
import { charts, dom, SCOPE_LN, SCOPE_RC, state } from "./state.js";
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
    dom.tableMeta.textContent = "No Data Loaded.";
}

function applyFiltersAndRender() {
    const filters = getActiveFilters(dom);
    const filtered = sortRows(
        filterDisplayRows(state.displayRows, filters),
        state.sortKey,
        state.sortDirection,
    );

    state.filteredRows = filtered;
    renderTable(filtered);
    setCompareUiVisible(Boolean(state.compareAlgorithm));

    const activeSummary = computeSummary(filtered);
    state.summary = activeSummary;

    const filteredCompareRows = state.compareAlgorithm
        ? pickFilteredCompareRows(filtered, state.scopedCompareRows)
        : [];

    state.compareSummary = state.compareAlgorithm
        ? computeHeadToHead(filtered, filteredCompareRows)
        : null;

    updateSummary(activeSummary);
    updateInsightLists(activeSummary);
    updateCompareSummary(state.compareSummary);
    renderErrorPanel(filtered);

    charts.render(activeSummary, state.compareSummary, {
        activeFilters: filters,
        fullSummary: state.fullSummary,
        dimUnselected: hasActiveFilters(filters),
    });

    dom.tableMeta.textContent = `${state.filteredRows.length} / ${state.displayRows.length} Maps Shown | Errors=${state.errorRows.length}`;

    if (state.currentAlgorithm) {
        setReadyDatasetInfo(activeSummary, state.errorRows.length);
    }
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

        state.scopedBaseRows = applyScopeRows(state.baseRows, state.baseMode, SCOPE_LN);
        state.scopedCompareRows = state.compareAlgorithm
            ? applyScopeRows(state.compareRows, state.compareMode, SCOPE_LN)
            : [];

        state.displayRows = mergeRowsForDisplay(
            state.scopedBaseRows,
            state.scopedCompareRows,
            Boolean(state.compareAlgorithm),
        );

        state.fullSummary = computeSummary(state.scopedBaseRows);

        fillPatternFilter(state.scopedBaseRows);
        fillSubPatternFilter(state.scopedBaseRows);
        applyFiltersAndRender();

        setStatus("Ready", "ok");
        syncUrlParams();
    } catch (error) {
        applyEmptyDashboard();
        setStatus("Error", "error");
        setDatasetInfo(`${state.currentAlgorithm} | ${buildFriendlyFetchHint(toErrorMessage(error))}`);
    }
}

async function loadLocalDataAndRefresh(fileList) {
    const importedInfo = await importLocalDatasets(fileList);
    if (!importedInfo.hasCsv) {
        setStatus("Waiting", "warn");
        setDatasetInfo("No CSV Files Found In Selected Folder.");
        return;
    }

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
    updateSourceHint(`Local Import ${importedInfo.imported} File(s)`);
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
        state.baseMode = normalizeScope(dom.baseCategorySelect.value, SCOPE_LN);
        await loadCurrentView();
    });

    dom.compareAlgorithmSelect.addEventListener("change", async () => {
        state.compareAlgorithm = dom.compareAlgorithmSelect.value || "";
        state.compareMode = SCOPE_RC;
        await loadCurrentView();
    });

    dom.compareCategorySelect.addEventListener("change", async () => {
        state.compareMode = normalizeScope(dom.compareCategorySelect.value, SCOPE_LN);
        await loadCurrentView();
    });

    dom.reloadDataButton.addEventListener("click", async () => {
        const refreshResult = await refreshRemoteCatalog();
        updateSourceHint(refreshResult.sourceLabel ? `Discovered by ${refreshResult.sourceLabel}` : "");

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
                setStatus("Ready", "ok");
                setDatasetInfo(`Exported current dashboard data to ${fileName}`);
            } catch (error) {
                setStatus("Error", "error");
                setDatasetInfo(`Export failed: ${toErrorMessage(error)}`);
            }
        });
    }

    dom.searchInput.addEventListener("input", applyFiltersAndRender);
    dom.patternFilter.addEventListener("change", applyFiltersAndRender);
    dom.subPatternFilter.addEventListener("change", applyFiltersAndRender);
    dom.bandFilter.addEventListener("change", applyFiltersAndRender);

    dom.clearFilterButton.addEventListener("click", () => {
        dom.searchInput.value = "";
        dom.patternFilter.value = "all";
        dom.subPatternFilter.value = "all";
        dom.bandFilter.value = "all";
        applyFiltersAndRender();
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
            applyFiltersAndRender();
        });
    });
}

export async function init() {
    bindEvents();
    exposeDashboardApi();

    setStatus("Loading...", "warn");
    setDatasetInfo("Discovering Datasets From docs/data...");
    updateSourceHint();

    const refreshResult = await refreshRemoteCatalog();
    updateSourceHint(refreshResult.sourceLabel ? `Discovered by ${refreshResult.sourceLabel}` : "");

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

    state.baseMode = normalizeScope(search.get("scope"), SCOPE_LN);
    state.compareMode = normalizeScope(search.get("compareScope"), SCOPE_LN);

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
        setStatus("Ready", "ok");
        setDatasetInfo("Local-file Mode Detected. Use Upload Data Folder To Import CSVs From docs/data.");
    }
}
