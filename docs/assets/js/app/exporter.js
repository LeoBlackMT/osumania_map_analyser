import { BAND_META, BAND_ORDER } from "../stats.js";
import { dom, state } from "./state.js";
import { sanitizeFileNameToken } from "./utils.js";
import { getActiveFilters } from "./model.js";

export function buildCurrentDataExportPayload() {
    return {
        formatVersion: 2,
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
        filters: getActiveFilters(dom),
        sorting: {
            key: state.sortKey,
            direction: state.sortDirection,
        },
        ui: {
            sourceHint: String(dom.sourceHint.textContent || ""),
            statusBadge: String(dom.statusBadge.textContent || ""),
            datasetInfo: String(dom.datasetInfo.textContent || ""),
            tableMeta: String(dom.tableMeta.textContent || ""),
            trendFitLabel: String(dom.trendFitValue?.textContent || ""),
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
        summaryFiltered: state.summary,
        summaryFullScope: state.fullSummary,
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

export function downloadCurrentDataSnapshot() {
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

export function exposeDashboardApi() {
    window.benchmarkDashboardApi = Object.freeze({
        getCurrentDataSnapshot: buildCurrentDataExportPayload,
        downloadCurrentDataSnapshot,
    });
}
