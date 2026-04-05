import {
    APP_CONFIG,
    estDiffCaptionEl,
    GRAPH_LOADING_BASELINE_Y,
    GRAPH_PADDING_BOTTOM,
    GRAPH_PADDING_TOP,
    GRAPH_PADDING_X,
    GRAPH_RESAMPLE_INTERVAL_MS,
    GRAPH_VIEWBOX_HEIGHT,
    GRAPH_VIEWBOX_WIDTH,
    hasAnyGraphModeEnabled,
    forEachEnabledGraphView,
    forEachGraphView,
    reworkDiffEl,
    reworkRightCapsuleEl,
    state,
} from "./appContext.js";
import {
    buildFillPath,
    buildLinePath,
    interpolateSeriesValue,
    normalizeGraphSeries,
} from "./graphMath.js";
import { updatePauseCountVisibility } from "./hud.js";

const GRAPH_SCAN_ENTER_DURATION_MS = 400;
const GRAPH_LOADING_TEXT_CLASS = "star-graph-loading-text";
const GRAPH_TIME_EPSILON_MS = 1e-3;

function getGraphLineVerticalBounds(view) {
    const yTop = GRAPH_PADDING_TOP;
    const yBottom = GRAPH_VIEWBOX_HEIGHT - GRAPH_PADDING_BOTTOM;
    if (!view || view.key !== "body") return { yTop, yBottom };
    return { yTop: Math.max(1, yTop - 5), yBottom: Math.min(GRAPH_VIEWBOX_HEIGHT - 1, yBottom + 5) };
}

function clearGraphScanEnter(view) {
    if (!view || !view.svgEl) {
        return;
    }
    if (view.scanEnterTimeoutId) {
        clearTimeout(view.scanEnterTimeoutId);
        view.scanEnterTimeoutId = 0;
    }
    view.svgEl.classList.remove("scan-enter");
}

function triggerGraphScanEnter(view) {
    if (!view || !view.svgEl) {
        return;
    }
    clearGraphScanEnter(view);
    view.svgEl.getBoundingClientRect();
    view.svgEl.classList.add("scan-enter");

    view.scanEnterTimeoutId = setTimeout(() => {
        if (!view.svgEl) {
            return;
        }
        view.svgEl.classList.remove("scan-enter");
        view.scanEnterTimeoutId = 0;
    }, GRAPH_SCAN_ENTER_DURATION_MS);
}

function getInterpolatedPlaybackTime() {
    if (state.pauseDetectionEnabled && state.isPaused && Number.isFinite(state.frozenInterpMs)) {
        return state.frozenInterpMs;
    }

    if (!Number.isFinite(state.songTimeMs) || state.songTimeReceiveTs <= 0) {
        return null;
    }

    const now = performance.now();
    const receiveDelta = state.songTimeReceiveTs - state.prevSongTimeReceiveTs;
    const songDelta = state.songTimeMs - state.prevSongTimeMs;

    let rate = 1;
    if (receiveDelta > 5 && songDelta >= 0 && Math.abs(songDelta) <= 4000) {
        rate = songDelta / receiveDelta;
    }
    rate = Math.max(0, Math.min(rate, 5));

    return state.songTimeMs + rate * (now - state.songTimeReceiveTs);
}

function buildGraphLoadingPaths() {
    const points = [];
    const xMin = GRAPH_PADDING_X;
    const xMax = GRAPH_VIEWBOX_WIDTH - GRAPH_PADDING_X;
    const span = xMax - xMin;
    const sampleCount = APP_CONFIG.graph.loadingSampleCount;

    for (let i = 0; i < sampleCount; i += 1) {
        const t = i / (sampleCount - 1);
        const x = xMin + span * t;
        const y = GRAPH_VIEWBOX_HEIGHT - GRAPH_PADDING_BOTTOM - APP_CONFIG.graph.loadingBaseOffset;
        points.push([x, y]);
    }

    return {
        linePath: buildLinePath(points),
        fillPath: buildFillPath(points, GRAPH_LOADING_BASELINE_Y),
    };
}

function ensureGraphLoadingTextEl(view) {
    if (!view || !view.wrapEl) {
        return null;
    }

    if (view.loadingTextEl && view.wrapEl.contains(view.loadingTextEl)) {
        return view.loadingTextEl;
    }

    const existing = view.wrapEl.querySelector(`.${GRAPH_LOADING_TEXT_CLASS}`);
    if (existing) {
        view.loadingTextEl = existing;
        return existing;
    }

    const loadingTextEl = document.createElement("div");
    loadingTextEl.className = GRAPH_LOADING_TEXT_CLASS;
    loadingTextEl.textContent = "Graph loading...";
    loadingTextEl.hidden = true;
    view.wrapEl.appendChild(loadingTextEl);
    view.loadingTextEl = loadingTextEl;
    return loadingTextEl;
}

function setGraphLoadingTextVisible(view, visible) {
    const loadingTextEl = ensureGraphLoadingTextEl(view);
    if (!loadingTextEl) {
        return;
    }
    loadingTextEl.hidden = !visible;
}

function trimSeriesStartToFirstObject(series) {
    if (!series || !Array.isArray(series.times) || !Array.isArray(series.values)) {
        return null;
    }

    const startTime = Number(state.songStartMs);
    if (!Number.isFinite(startTime)) {
        return series;
    }

    const { times, values } = series;
    if (times.length < 2 || values.length < 2) {
        return series;
    }

    const firstTime = times[0];
    const lastTime = times[times.length - 1];
    if (startTime <= firstTime + GRAPH_TIME_EPSILON_MS || startTime >= lastTime) {
        return series;
    }

    let startIndex = -1;
    for (let i = 0; i < times.length; i += 1) {
        if (times[i] >= startTime) {
            startIndex = i;
            break;
        }
    }

    if (startIndex <= 0) {
        return series;
    }

    const trimmedTimes = [startTime];
    const exactHit = Math.abs(times[startIndex] - startTime) <= GRAPH_TIME_EPSILON_MS;
    const startValue = exactHit
        ? values[startIndex]
        : interpolateSeriesValue(times, values, startTime);
    const trimmedValues = [startValue];
    const appendFrom = exactHit ? startIndex + 1 : startIndex;
    for (let i = appendFrom; i < times.length; i += 1) {
        trimmedTimes.push(times[i]);
        trimmedValues.push(values[i]);
    }

    if (trimmedTimes.length < 2) {
        return series;
    }

    return {
        times: trimmedTimes,
        values: trimmedValues,
    };
}

function formatEstimateDifficultyCaption() {
    const base = "Estimate Difficulty";
    if (!state.enableNumericDifficulty) {
        return base;
    }

    const formatRcCaptionValue = (rawValue) => {
        const text = String(rawValue ?? "").trim();
        if (!text) {
            return text;
        }
        if (state.currentModeTag === "RC") {
            return text;
        }
        if (/^RC\b/i.test(text)) {
            return text;
        }
        return `RC${text}`;
    };

    if (Number.isFinite(state.numericDifficulty)) {
        const valueText = formatRcCaptionValue(state.numericDifficulty.toFixed(2));
        return `${base}(${valueText})`;
    }

    if (typeof state.numericDifficultyHint === "string" && state.numericDifficultyHint.trim().length > 0) {
        const valueText = formatRcCaptionValue(state.numericDifficultyHint.trim());
        return `${base}(${valueText})`;
    }

    return base;
}

export function clearPauseMarkersDom(view = null) {
    if (view) {
        if (view.pauseMarkersEl) {
            view.pauseMarkersEl.innerHTML = "";
        }
        return;
    }

    forEachGraphView((entry) => {
        if (entry.pauseMarkersEl) {
            entry.pauseMarkersEl.innerHTML = "";
        }
    });
}

function drawPauseMarkersForView(view) {
    clearPauseMarkersDom(view);

    if (!state.pauseDetectionEnabled || !state.graphSeries || !view.pauseMarkersEl || !view.isEnabled()) {
        return;
    }

    const { minTime, maxTime } = state.graphSeries;
    const timeSpan = Math.max(1, maxTime - minTime);
    const xMin = GRAPH_PADDING_X;
    const xMax = GRAPH_VIEWBOX_WIDTH - GRAPH_PADDING_X;
    const { yTop, yBottom } = getGraphLineVerticalBounds(view);

    for (const markerTime of state.pauseMarkerTimes) {
        if (!Number.isFinite(markerTime)) {
            continue;
        }

        const clampedTime = Math.max(minTime, Math.min(markerTime, maxTime));
        const x = xMin + ((clampedTime - minTime) / timeSpan) * (xMax - xMin);
        const line = document.createElementNS("http://www.w3.org/2000/svg", "line");
        line.setAttribute("x1", x.toFixed(2));
        line.setAttribute("x2", x.toFixed(2));
        line.setAttribute("y1", yTop.toFixed(2));
        line.setAttribute("y2", yBottom.toFixed(2));
        line.setAttribute("class", "star-graph-pause-marker");
        line.setAttribute("stroke", APP_CONFIG.graph.pauseLineColor);
        line.setAttribute("stroke-width", String(APP_CONFIG.graph.pauseLineWidth));
        view.pauseMarkersEl.appendChild(line);
    }
}

export function redrawPauseMarkers() {
    forEachEnabledGraphView((view) => {
        drawPauseMarkersForView(view);
    });
}

export function clearAllPauseMarkers() {
    state.pauseMarkerTimes = [];
    state.pauseCount = 0;
    clearPauseMarkersDom();
    updatePauseCountVisibility();
}

export function addPauseMarker(songTimeMs) {
    if (!state.pauseDetectionEnabled || !Number.isFinite(songTimeMs)) {
        return;
    }

    state.pauseMarkerTimes.push(songTimeMs);
    state.pauseCount = state.pauseMarkerTimes.length;
    updatePauseCountVisibility();
    redrawPauseMarkers();
}

export function resetPauseRuntime(clearMarkers = false) {
    state.isPaused = false;
    state.pauseTimeMs = 0;
    state.frozenInterpMs = 0;
    state.hasSongTimeSample = false;
    if (clearMarkers) {
        clearAllPauseMarkers();
    }
}

export function clearDiffGraph() {
    state.graphSeries = null;

    forEachGraphView((view) => {
        if (view.svgEl) {
            view.svgEl.classList.remove("loading");
        }
        clearGraphScanEnter(view);

        if (view.fillEl) {
            view.fillEl.setAttribute("d", "");
        }
        if (view.lineEl) {
            view.lineEl.setAttribute("d", "");
        }

        if (view.cursorEl) {
            view.cursorEl.hidden = true;
        }
        if (view.cursorDotEl) {
            view.cursorDotEl.hidden = true;
        }

        if (view.errorEl) {
            view.errorEl.textContent = "Graph unavailable";
            view.errorEl.hidden = true;
        }

        setGraphLoadingTextVisible(view, false);

        clearPauseMarkersDom(view);
    });
}

export function setGraphCursorVisible(visible) {
    forEachGraphView((view) => {
        const enabled = view.isEnabled();
        if (view.cursorEl) {
            view.cursorEl.hidden = !visible || !enabled;
        }
        if (view.cursorDotEl) {
            view.cursorDotEl.hidden = !visible || !enabled;
        }
    });
}

export function setGraphLoading(isLoading) {
    if (!hasAnyGraphModeEnabled()) {
        return;
    }

    forEachEnabledGraphView((view) => {
        if (!view.svgEl || !view.fillEl || !view.lineEl) {
            return;
        }

        if (isLoading) {
            const { linePath, fillPath } = buildGraphLoadingPaths();
            view.svgEl.classList.add("loading");
            clearGraphScanEnter(view);
            view.lineEl.setAttribute("d", linePath);
            view.fillEl.setAttribute("d", fillPath);
            setGraphLoadingTextVisible(view, true);
            clearPauseMarkersDom(view);
            if (view.cursorEl) {
                view.cursorEl.hidden = true;
            }
            if (view.cursorDotEl) {
                view.cursorDotEl.hidden = true;
            }
            if (view.errorEl) {
                view.errorEl.hidden = true;
                view.errorEl.textContent = "Graph unavailable";
            }
            return;
        }

        view.svgEl.classList.remove("loading");
        setGraphLoadingTextVisible(view, false);
    });
}

export function showDiffGraphError(message) {
    if (!hasAnyGraphModeEnabled()) {
        return;
    }

    setGraphLoading(false);
    state.graphSeries = null;
    forEachEnabledGraphView((view) => {
        clearGraphScanEnter(view);
        if (view.cursorEl) {
            view.cursorEl.hidden = true;
        }
        if (view.cursorDotEl) {
            view.cursorDotEl.hidden = true;
        }

        if (view.fillEl) {
            view.fillEl.setAttribute("d", "");
        }
        if (view.lineEl) {
            view.lineEl.setAttribute("d", "");
        }
        clearPauseMarkersDom(view);

        if (view.errorEl) {
            view.errorEl.textContent = message || "Graph unavailable";
            view.errorEl.hidden = false;
        }

        setGraphLoadingTextVisible(view, false);
    });
}

export function updateGraphCursor(explicitTimeMs = null) {
    if (!hasAnyGraphModeEnabled()) {
        setGraphCursorVisible(false);
        return;
    }

    const series = state.graphSeries;
    if (!series) {
        setGraphCursorVisible(false);
        return;
    }

    const timeMs = Number.isFinite(explicitTimeMs) ? explicitTimeMs : getInterpolatedPlaybackTime();
    if (!Number.isFinite(timeMs)) {
        setGraphCursorVisible(false);
        return;
    }

    const { times, values, minTime, maxTime, minYValue, maxYValue } = series;
    const rangeStart = Number.isFinite(state.songStartMs) ? state.songStartMs : minTime;
    const rangeEnd = Number.isFinite(state.songEndMs) ? state.songEndMs : maxTime;
    const boundedTime = Math.max(rangeStart, Math.min(timeMs, rangeEnd));
    const clampedTime = Math.max(minTime, Math.min(boundedTime, maxTime));

    const xMin = GRAPH_PADDING_X;
    const xMax = GRAPH_VIEWBOX_WIDTH - GRAPH_PADDING_X;
    const yMin = GRAPH_PADDING_TOP;
    const yMax = GRAPH_VIEWBOX_HEIGHT - GRAPH_PADDING_BOTTOM;

    const timeSpan = Math.max(1, maxTime - minTime);
    const valueSpan = Math.max(0.001, maxYValue - minYValue);

    const x = xMin + ((clampedTime - minTime) / timeSpan) * (xMax - xMin);
    const value = interpolateSeriesValue(times, values, clampedTime);
    const normalized = valueSpan < 0.001 ? 0.5 : (value - minYValue) / valueSpan;
    const y = yMax - normalized * (yMax - yMin);

    forEachEnabledGraphView((view) => {
        const { yTop, yBottom } = getGraphLineVerticalBounds(view);
        if (view.cursorEl) {
            view.cursorEl.setAttribute("x1", x.toFixed(2));
            view.cursorEl.setAttribute("x2", x.toFixed(2));
            view.cursorEl.setAttribute("y1", yTop.toFixed(2));
            view.cursorEl.setAttribute("y2", yBottom.toFixed(2));
        }

        if (view.cursorDotEl) {
            view.cursorDotEl.setAttribute("cx", x.toFixed(2));
            view.cursorDotEl.setAttribute("cy", y.toFixed(2));
        }
    });

    setGraphCursorVisible(true);
}

export function startGraphAnimationLoop() {
    if (state.graphAnimationStarted) {
        return;
    }

    state.graphAnimationStarted = true;

    const tick = () => {
        if (hasAnyGraphModeEnabled()) {
            updateGraphCursor();
        }
        requestAnimationFrame(tick);
    };

    requestAnimationFrame(tick);
}

export function renderDiffGraph(graphData) {
    if (!hasAnyGraphModeEnabled()) {
        return false;
    }

    const normalizedSeries = normalizeGraphSeries(graphData, GRAPH_RESAMPLE_INTERVAL_MS);
    const series = trimSeriesStartToFirstObject(normalizedSeries);
    if (!series) {
        showDiffGraphError("Graph unavailable");
        return false;
    }

    const { times, values } = series;

    setGraphLoading(false);

    let minYValue = Number.POSITIVE_INFINITY;
    let maxYValue = Number.NEGATIVE_INFINITY;
    for (let i = 0; i < values.length; i += 1) {
        const value = values[i];
        if (value < minYValue) {
            minYValue = value;
        }
        if (value > maxYValue) {
            maxYValue = value;
        }
    }
    const valueSpan = Math.max(0.001, maxYValue - minYValue);

    const minTime = times[0];
    const maxTime = times[times.length - 1];
    const timeSpan = Math.max(1, maxTime - minTime);

    const xMin = GRAPH_PADDING_X;
    const xMax = GRAPH_VIEWBOX_WIDTH - GRAPH_PADDING_X;
    const yMin = GRAPH_PADDING_TOP;
    const yMax = GRAPH_VIEWBOX_HEIGHT - GRAPH_PADDING_BOTTOM;

    const points = new Array(values.length);
    for (let index = 0; index < values.length; index += 1) {
        const value = values[index];
        const x = xMin + ((times[index] - minTime) / timeSpan) * (xMax - xMin);
        const normalized = valueSpan < 0.001 ? 0.5 : (value - minYValue) / valueSpan;
        const y = yMax - normalized * (yMax - yMin);
        points[index] = [x, y];
    }

    const linePath = buildLinePath(points);
    const fillPath = buildFillPath(points, yMax);
    forEachEnabledGraphView((view) => {
        if (view.lineEl) {
            view.lineEl.setAttribute("d", linePath);
        }
        if (view.fillEl) {
            view.fillEl.setAttribute("d", fillPath);
        }
        if (view.errorEl) {
            view.errorEl.hidden = true;
            view.errorEl.textContent = "Graph unavailable";
        }
    });

    state.graphSeries = {
        times,
        values,
        minTime,
        maxTime,
        minYValue,
        maxYValue,
    };

    forEachEnabledGraphView((view) => {
        triggerGraphScanEnter(view);
    });
    redrawPauseMarkers();
    updateGraphCursor();

    return true;
}

export function updateDiffTextVisibility() {
    const mode = state.diffText;
    const showDiffText = mode === "Difficulty";
    const showHeaderGraph = mode === "Graph";
    const showRightCapsule = mode === "MSD"
        || mode === "Pattern"
        || mode === "ReworkSR"
        || mode === "InterludeSR";

    reworkDiffEl.hidden = !showDiffText;
    forEachGraphView((view) => {
        if (view.key === "header") {
            view.wrapEl.hidden = !showHeaderGraph;
        }
    });
    if (reworkRightCapsuleEl) {
        reworkRightCapsuleEl.hidden = !showRightCapsule;
    }

    estDiffCaptionEl.hidden = mode === "None";
    if (mode === "Graph") {
        estDiffCaptionEl.textContent = "Difficulty Graph";
    } else if (showRightCapsule) {
        switch (mode) {
            case "MSD":
                estDiffCaptionEl.textContent = "Mina Standard Difficulty";
                break;
            case "Pattern":
                estDiffCaptionEl.textContent = "Overall Pattern";
                break;
            case "ReworkSR":
                estDiffCaptionEl.textContent = "Sunny Rework SR";
                break;
            case "InterludeSR":
                estDiffCaptionEl.textContent = "Interlude Star Rating";
                break;
            default:
                estDiffCaptionEl.textContent = "";
        }
    } else {
        estDiffCaptionEl.textContent = formatEstimateDifficultyCaption();
    }

    if (!hasAnyGraphModeEnabled()) {
        clearDiffGraph();
    } else {
        setGraphCursorVisible(false);
    }

    if (!showRightCapsule && reworkRightCapsuleEl) {
        reworkRightCapsuleEl.textContent = "-";
        reworkRightCapsuleEl.classList.remove("category-mode", "numeric-mode", "high-contrast", "has-unit");
        reworkRightCapsuleEl.removeAttribute("data-unit");
        reworkRightCapsuleEl.style.backgroundColor = "rgba(38, 50, 84, 0.45)";
        reworkRightCapsuleEl.style.color = "#f6fbff";
        reworkRightCapsuleEl.style.textShadow = "none";
    }
}

export function setNumericDifficultyValue(value, hint = null) {
    if (value === null || value === undefined || value === "") {
        state.numericDifficulty = null;
    } else {
        const numericValue = Number(value);
        state.numericDifficulty = Number.isFinite(numericValue) ? numericValue : null;
    }

    state.numericDifficultyHint = Number.isFinite(state.numericDifficulty)
        ? null
        : (typeof hint === "string" && hint.trim().length > 0 ? hint.trim() : null);

    if (state.diffText === "Difficulty") {
        updateDiffTextVisibility();
    }
}
