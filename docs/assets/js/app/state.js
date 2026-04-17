import { BenchmarkCharts } from "../charts.js";

export const DATA_DIR = "data";
export const INDEX_URL = `${DATA_DIR}/index.json`;

export const SCOPE_RC = "RC";
export const SCOPE_LN = "LN";

export const state = {
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
    filteredRows: [],
    errorRows: [],

    fullSummary: null,
    summary: null,
    compareSummary: null,

    sortKey: "deltaAbs",
    sortDirection: "asc",
};

export const dom = {
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
    trendFitValue: document.getElementById("trendFitValue"),
};

export const charts = new BenchmarkCharts({
    accuracy: "accuracyBreakdownChart",
    scatter: "scatterChart",
    deltaDistribution: "deltaDistributionChart",
    trend: "trendChart",
    pattern: "patternChart",
    subPattern: "subPatternChart",
    headToHead: "headToHeadChart",
});
