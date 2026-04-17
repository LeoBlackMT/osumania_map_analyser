import { BAND_META } from "../stats.js";
import { dom, state } from "./state.js";
import {
    escapeHtml,
    formatGeneratedAt,
    formatGotDifficultyFromNumeric,
    formatNumber,
    formatPercent,
    formatSigned,
    getBeatmapDownloadUrl,
    getMapSearchUrl,
    hasValidBid,
    normalizeSubPattern,
} from "./utils.js";
import { getWinnerLabel } from "./model.js";

export function setStatus(message, level) {
    dom.statusBadge.className = `badge ${level}`;
    dom.statusBadge.textContent = message;
}

export function setDatasetInfo(text) {
    dom.datasetInfo.textContent = text;
}

export function setCompareUiVisible(visible) {
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

export function setFieldVisible(field, visible) {
    if (!field) {
        return;
    }
    field.classList.toggle("hidden", !visible);
}

export function updateSourceHint(extra = "") {
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

export function renderAlgorithmOptions() {
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

export function renderCompareOptions() {
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

export function renderAlgorithmSelectors() {
    renderAlgorithmOptions();
    renderCompareOptions();
}

export function fillPatternFilter(rows) {
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

export function fillSubPatternFilter(rows) {
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

export function updateSummary(summary) {
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

    if (dom.trendFitValue) {
        const value = summary.metrics.trendFitPercent;
        dom.trendFitValue.textContent = Number.isFinite(value)
            ? `Fit: ${Number(value).toFixed(1)}%`
            : "Fit: -";
    }
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

export function updateInsightLists(summary) {
    renderInsightList(dom.underratedList, summary.topUnderrated, "positive");
    renderInsightList(dom.overratedList, summary.topOverrated, "negative");
}

export function updateCompareSummary(compareSummary) {
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

export function renderErrorPanel(rows) {
    const errors = rows
        .filter((row) => Boolean(row._errorInfo))
        .map((row) => ({
            ...row,
            errorType: row._errorInfo.type,
            errorDetail: row._errorInfo.detail,
            errorRaw: row._errorInfo.raw,
        }));

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
                `<td>${escapeHtml(row._normalizedSubPattern || normalizeSubPattern(row.subPattern))}</td>`,
                `<td>${escapeHtml(expectedText)}</td>`,
                `<td>${escapeHtml(row.errorType)}</td>`,
                `<td>${escapeHtml(compactDetail)}</td>`,
                "</tr>",
            ].join("");
        })
        .join("");
}

export function renderTable(rows) {
    dom.emptyState.hidden = rows.length > 0;

    dom.resultTableBody.innerHTML = rows
        .map((row) => {
            const bandKey = row._band || "error";
            const bandLabel = bandKey === "error"
                ? "Error"
                : (BAND_META[bandKey]?.label || "Miss");
            const winnerLabel = getWinnerLabel(row.better, Boolean(state.compareAlgorithm));
            const winnerClass = row.better || "na";
            const hasBid = hasValidBid(row);
            const searchUrl = getMapSearchUrl(row.name);
            const downloadUrl = hasBid ? getBeatmapDownloadUrl(row.bid) : "";

            const gotValue = row.got;
            const gotText = Number.isFinite(gotValue)
                ? formatGotDifficultyFromNumeric(gotValue, row.pattern)
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
                `<td>${escapeHtml(row._normalizedSubPattern || normalizeSubPattern(row.subPattern))}</td>`,
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

export function updateSortVisual() {
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

export function setReadyDatasetInfo(summary, errorCount) {
    const descriptor = state.catalog.get(state.currentAlgorithm);
    const generatedAtText = formatGeneratedAt(descriptor?.modifiedAt);
    const compareText = state.compareAlgorithm
        ? ` vs ${state.compareAlgorithm}[${state.compareMode}]`
        : "";

    setDatasetInfo(
        `${state.currentAlgorithm}[${state.baseMode}]${compareText}`
        + ` | Maps=${summary.totalRows} | Errors=${errorCount} | Generated At ${generatedAtText}`,
    );
}
