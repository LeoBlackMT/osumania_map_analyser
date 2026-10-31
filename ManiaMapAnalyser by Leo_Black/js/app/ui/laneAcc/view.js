// view.js —— LaneAcc 卡片主体（DOM 渲染、Popover、占位态）。
//
// 纯逻辑在 js/laneAcc/；本文件只做 DOM 与 rAF 节流。所有 DOM 都是在
// `#laneacc-wrap` 内创建/复用，不触碰其它 contentBar 模块。

import { laneAccWrapEl, state } from "../../core/appContext.js";
import { isShellRuntime } from "../../feedback/feedbackSpec.js";
import { createAccumulator } from "../../../laneAcc/accumulator.js";
import { framesToSv1Events } from "../../../laneAcc/rawFrames.js";
import { formatAcc, formatMiss } from "../../../laneAcc/format.js";
import { createPopoverController } from "./popover.js";
import { renderHands } from "./hands.js";

const ERROR_TEXT = {
    shell: "LaneAcc needs the desktop shell.",
    contract: "Update the shell to use LaneAcc (contract v10).",
    native: "The shell's Lazer memory reader is unavailable.",
    lazer: "LaneAcc works in osu!lazer only.",
    mania: "LaneAcc supports native mania maps only.",
    keys: "LaneAcc supports 4K–18K.",
    offsets: "The Lazer offset table has no hit-event section.",
    chart: "Chart text unavailable; switch maps to reload.",
};

function dom() {
    if (!laneAccWrapEl) {
        return null;
    }
    return {
        wrap: laneAccWrapEl,
        summary: laneAccWrapEl.querySelector(".laneacc-summary"),
        sv1: laneAccWrapEl.querySelector("#laneacc-sv1-value"),
        hands: laneAccWrapEl.querySelector("#laneacc-hands"),
        stage: laneAccWrapEl.querySelector("#laneacc-stage"),
        popover: laneAccWrapEl.querySelector("#laneacc-popover"),
        error: laneAccWrapEl.querySelector("#laneacc-error"),
        od: laneAccWrapEl.querySelector("#laneacc-od"),
    };
}

export const laneAccView = (() => {
    let pendingChart = null;
    let activeChart = null;
    let accumulator = null;
    let rawAccumulator = null;
    let replayFrames = [];
    let hitEvents = [];
    let feedIndex = 0;
    let lastCutoff = -Infinity;
    let allEvents = false;
    let lastRawSolveAt = 0;
    let sessionActive = false;
    let renderHandle = 0;
    let laneButtons = [];
    let laneKey = "";
    let spaceModeSetting = "auto";
    let latestSnapshot = null;
    let laneBounds = null;
    let stageVisible = false;

    // Popover 交互控制器（分轨与手部弹窗）
    const popoverCtrl = createPopoverController({
        getDom: dom,
        getSnapshot: () => latestSnapshot || accumulator?.snapshot(),
        getActiveChart: () => activeChart,
    });

    const activeAnimations = new WeakMap();

    function animateNumber(element, target, formatter) {
        if (!element) {
            return;
        }
        if (target == null || !Number.isFinite(target)) {
            const current = activeAnimations.get(element);
            if (current) {
                cancelAnimationFrame(current.handle);
                activeAnimations.delete(element);
            }
            element.textContent = "--";
            delete element.dataset.displayValue;
            return;
        }
        const previousValue = element.dataset.displayValue != null
            ? Number(element.dataset.displayValue)
            : target;
        element.dataset.displayValue = String(target);
        if (Math.abs(previousValue - target) < 1e-4) {
            element.textContent = formatter(target);
            return;
        }
        const start = performance.now();
        const duration = 240;
        const from = previousValue;
        const delta = target - from;
        const current = activeAnimations.get(element);
        if (current) {
            cancelAnimationFrame(current.handle);
        }
        const anim = { handle: 0 };
        function tick(now) {
            const progress = Math.min(1, (now - start) / duration);
            const eased = 1 - (1 - progress) ** 3;
            const currentVal = from + delta * eased;
            element.textContent = formatter(currentVal);
            if (progress < 1) {
                anim.handle = requestAnimationFrame(tick);
            } else {
                element.textContent = formatter(target);
                activeAnimations.delete(element);
            }
        }
        anim.handle = requestAnimationFrame(tick);
        activeAnimations.set(element, anim);
    }

    function options() {
        return {
            spaceMode: spaceModeSetting,
            lazerScore: state.lazerScoreData ?? null,
            od: state.odFlag ?? null,
            speedRate: state.speedRate || 1.0,
            mods: state.modCodes || [],
        };
    }

    function scheduleRender() {
        if (renderHandle) {
            return;
        }
        if (typeof requestAnimationFrame === "function") {
            renderHandle = requestAnimationFrame(() => {
                renderHandle = 0;
                render();
            });
        } else {
            render();
        }
    }

    function showError(key) {
        const nodes = dom();
        if (!nodes) {
            return;
        }
        nodes.wrap.dataset.state = "error";
        nodes.summary.hidden = true;
        nodes.stage.hidden = true;
        nodes.error.hidden = false;
        nodes.error.textContent = ERROR_TEXT[key] || ERROR_TEXT.shell;
    }

    function showLoading(changeKind = "song") {
        const nodes = dom();
        if (!nodes) {
            return;
        }
        nodes.wrap.dataset.state = "loading";
        nodes.summary.hidden = false;
        nodes.stage.hidden = false;
        nodes.error.hidden = true;
        nodes.stage.classList.remove(
            "is-entering",
            "is-loading",
            "is-loading-song",
            "is-loading-difficulty",
        );
        nodes.stage.classList.add("is-loading", `is-loading-${changeKind}`);
    }

    function clearLoading() {
        const nodes = dom();
        if (!nodes) {
            return;
        }
        nodes.stage.classList.remove(
            "is-loading",
            "is-loading-song",
            "is-loading-difficulty",
        );
    }

    function clearError() {
        const nodes = dom();
        if (!nodes) {
            return;
        }
        nodes.wrap.dataset.state = "ready";
        nodes.summary.hidden = false;
        nodes.stage.hidden = false;
        nodes.error.hidden = true;
        nodes.error.textContent = "";
    }

    function ensureLanes(snapshot) {
        const nodes = dom();
        if (!nodes) {
            return;
        }
        const key = `${activeChart.columnCount}:${snapshot.spaceMode}`;
        if (key === laneKey) {
            return;
        }
        laneKey = key;
        laneButtons = [];
        nodes.stage.replaceChildren();
        nodes.stage.classList.toggle("is-dense", activeChart.columnCount > 7);
        nodes.stage.style.setProperty("--laneacc-lanes", String(activeChart.columnCount));
        for (let col = 0; col < activeChart.columnCount; col += 1) {
            const button = document.createElement("button");
            button.type = "button";
            button.className = "laneacc-lane";
            button.dataset.col = String(col);
            button.style.setProperty("--lane-index", String(col));
            const fill = document.createElement("span");
            fill.className = "laneacc-lane-fill";
            const number = document.createElement("span");
            number.className = "laneacc-lane-number";
            number.textContent = String(col + 1);
            const acc = document.createElement("span");
            acc.className = "laneacc-lane-acc";
            const miss = document.createElement("span");
            miss.className = "laneacc-lane-miss";
            miss.hidden = true;
            button.append(fill, number, acc, miss);
            const open = () => popoverCtrl.showPopover(col, latestSnapshot?.lanes[col] ?? accumulator?.snapshot()?.lanes[col]);
            button.addEventListener("pointerenter", open);
            button.addEventListener("pointerleave", popoverCtrl.hidePopover);
            button.addEventListener("focus", open);
            button.addEventListener("blur", popoverCtrl.hidePopover);
            button.addEventListener("click", (event) => {
                event.stopPropagation();
                if (popoverCtrl.isPopoverOpen(col)) {
                    popoverCtrl.hidePopover();
                } else {
                    open();
                }
            });
            nodes.stage.append(button);
            laneButtons.push(button);
        }
        nodes.stage.append(nodes.popover);
    }

    function accDisplayMode(columns) {
        const nodes = dom();
        const width = nodes?.stage ? nodes.stage.clientWidth / Math.max(1, columns) : 40;
        if (width >= 34) {
            return "full";
        }
        if (width >= 26) {
            return "short";
        }
        if (width >= 17) {
            return "int";
        }
        return "none";
    }

    function rawSnapshotWithLazerBoundary(raw) {
        const counts = Array.from(raw.sv1Counts);
        const rawGrades = rawAccumulator?.sv1Grades?.();
        const hitGrades = accumulator?.sv1Grades?.();
        if (rawGrades && hitGrades) {
            for (let note = 0; note < rawGrades.length; note += 1) {
                if (rawGrades[note] === 3 && hitGrades[note] === 4) {
                    counts[2] -= 1;
                    counts[3] += 1;
                }
            }
        }
        const total = counts.reduce((sum, value) => sum + value, 0);
        const points = counts[0] * 300 + counts[1] * 300 + counts[2] * 200 + counts[3] * 100 + counts[4] * 50;
        return {
            ...raw,
            sv1Counts: counts,
            sv1Total: total,
            sv1Points: points,
            sv1Acc: total > 0 ? points / (total * 300) * 100 : 100,
        };
    }

    function render() {
        const nodes = dom();
        if (!nodes || !accumulator || !activeChart) {
            return;
        }
        const currentOptions = options();
        accumulator.syncModifiers(currentOptions);
        const snapshot = accumulator.snapshot();
        if (rawAccumulator && replayFrames.length > 0) {
            const raw = rawSnapshotWithLazerBoundary(rawAccumulator.snapshot());
            snapshot.sv1Counts = raw.sv1Counts;
            snapshot.sv1Acc = raw.sv1Acc;
            snapshot.sv1Total = raw.sv1Total;
            snapshot.sv1Points = raw.sv1Points;
            snapshot.maxCombo = raw.maxCombo;
        }
        latestSnapshot = snapshot;
        clearError();
        animateNumber(nodes.sv1, snapshot.sv1Acc, (value) => formatAcc(value));
        const effectiveOd = snapshot.windows?.displayOd ?? snapshot.windows?.od;
        if (Number.isFinite(effectiveOd)) {
            const rounded = Math.round(Number(effectiveOd) * 100) / 100;
            const eff = Number.isInteger(rounded) ? rounded.toFixed(1) : String(rounded);
            nodes.od.hidden = false;
            nodes.od.textContent = `OD${eff}`;
        } else {
            nodes.od.hidden = true;
        }
        renderHands(nodes.hands, snapshot, animateNumber);
        ensureLanes(snapshot);
        const spaceIndex = snapshot.hands.space
            ? Math.floor(activeChart.columnCount / 2)
            : -1;
        const judgedAccs = snapshot.lanes
            .map((lane) => (lane.judged > 0 && Number.isFinite(lane.acc) ? lane.acc : null))
            .filter((value) => value != null);
        if (judgedAccs.length > 0) {
            const nextLow = Math.floor(Math.min(...judgedAccs) - 1);
            const nextHigh = Math.ceil(Math.max(...judgedAccs));
            if (!laneBounds) {
                if (Math.min(...judgedAccs) < 99.999) {
                    laneBounds = { low: nextLow, high: nextHigh };
                }
            } else {
                laneBounds.low = Math.min(laneBounds.low, nextLow);
                laneBounds.high = Math.max(laneBounds.high, nextHigh);
            }
            if (laneBounds && laneBounds.high <= laneBounds.low) {
                laneBounds.high = laneBounds.low + 1;
            }
        }
        const low = laneBounds?.low ?? 95;
        const high = laneBounds?.high ?? 100;
        const accMode = accDisplayMode(laneButtons.length);
        nodes.stage.dataset.accMode = accMode;
        const decimals = accMode === "full" ? 2 : accMode === "short" ? 1 : 0;
        for (let col = 0; col < laneButtons.length; col += 1) {
            const lane = snapshot.lanes[col];
            const judged = lane.judged || 0;
            const acc = judged > 0 && Number.isFinite(lane.acc) ? lane.acc : null;
            const button = laneButtons[col];
            button.classList.toggle("is-space", col === spaceIndex);
            button.classList.toggle("is-empty", acc == null);
            const fill = acc == null ? 0 : Math.max(2, Math.min(100, ((acc - low) / (high - low)) * 100));
            button.style.setProperty("--lane-scale", String(fill / 100));
            const accEl = button.querySelector(".laneacc-lane-acc");
            if (accEl) {
                animateNumber(accEl, acc, (value) => value.toFixed(decimals));
            }
            const missEl = button.querySelector(".laneacc-lane-miss");
            if (missEl) {
                missEl.hidden = !(lane.miss > 0);
                if (lane.miss > 0) {
                    animateNumber(missEl, lane.miss, (value) => String(Math.round(value)));
                } else {
                    missEl.textContent = "";
                    delete missEl.dataset.displayValue;
                }
            }
            button.setAttribute(
                "aria-label",
                `Lane ${col + 1}: ${formatAcc(lane.acc)}, Miss ${formatMiss(lane.miss)}`,
            );
        }
        if (latestSnapshot) {
            popoverCtrl.refreshStatPopover();
        }
        const visible = laneAccWrapEl.offsetParent !== null;
        if (visible && !stageVisible) {
            replayEntrance();
        }
        stageVisible = visible;
    }

    function replayEntrance() {
        const nodes = dom();
        if (!nodes || !activeChart) {
            return;
        }
        nodes.stage.classList.remove(
            "is-entering",
            "is-loading",
            "is-loading-song",
            "is-loading-difficulty",
        );
        void nodes.stage.offsetWidth;
        nodes.stage.classList.add("is-entering");
    }

    function sameChart(a, b) {
        if (!a || !b) {
            return false;
        }
        return a.columnCount === b.columnCount && a.notes.length === b.notes.length;
    }

    function evaluateError() {
        if (!isShellRuntime()) {
            return "shell";
        }
        const contract = Number(state.shellContract ?? 0);
        if (contract > 0 && contract < 10) {
            return "contract";
        }
        const client = String(state.gameClient || state.client || "").toLowerCase();
        if (client && !client.includes("lazer")) {
            return "lazer";
        }
        if (state.externalBridgeAvailable && state.shellOsuAlive === false) {
            return "native";
        }
        const degraded = Array.isArray(state.shellOsuDegradedFields)
            ? state.shellOsuDegradedFields
            : [];
        if (degraded.includes("hitEvents")) {
            return "offsets";
        }
        if (activeChart && (activeChart.columnCount < 4 || activeChart.columnCount > 18)) {
            return "keys";
        }
        return null;
    }

    function prepareChart(chart) {
        pendingChart = chart;
        const err = evaluateError();
        if (err) {
            showError(err);
            return;
        }
        if (!chart) {
            showError("chart");
            return;
        }
        if (chart.columnCount < 4 || chart.columnCount > 18) {
            showError("keys");
            return;
        }
        clearLoading();
        clearError();
        const reuseState = sessionActive && accumulator && sameChart(activeChart, chart);
        activeChart = chart;
        if (!reuseState) {
            accumulator = createAccumulator(chart, options());
            rawAccumulator = createAccumulator(chart, options());
            replayFrames = [];
            hitEvents = [];
            feedIndex = 0;
            lastCutoff = -Infinity;
            allEvents = false;
            lastRawSolveAt = 0;
            laneBounds = null;
        } else {
            accumulator.syncModifiers(options());
            rawAccumulator?.syncModifiers(options());
        }
        scheduleRender();
    }

    function startSession() {
        sessionActive = true;
        feedIndex = 0;
        lastCutoff = -Infinity;
        allEvents = false;
        lastRawSolveAt = 0;
        laneBounds = null;
        if (activeChart) {
            accumulator = createAccumulator(activeChart, options());
            rawAccumulator = createAccumulator(activeChart, options());
        }
        clearLoading();
        scheduleRender();
    }

    function replayFramesChanged(prev, next) {
        if (prev.length !== next.length) {
            return true;
        }
        if (prev.length === 0) {
            return false;
        }
        return prev[prev.length - 1].time !== next[next.length - 1].time;
    }

    function solveRawAccumulator(frames, chart) {
        const parsed = framesToSv1Events(frames, chart.columnCount);
        const sim = createAccumulator(chart, options());
        sim.feedEvents(parsed);
        sim.feedComplete();
        return sim;
    }

    function applyReplayFrames(frames) {
        if (!sessionActive || !Array.isArray(frames) || frames.length === 0 || !activeChart) {
            return;
        }
        const hasChange = replayFramesChanged(replayFrames, frames);
        replayFrames = frames;
        if (!hasChange) {
            return;
        }
        const now = typeof performance !== "undefined" ? performance.now() : Date.now();
        if (now - lastRawSolveAt >= 300) {
            lastRawSolveAt = now;
            rawAccumulator = solveRawAccumulator(replayFrames, activeChart);
            scheduleRender();
        }
    }

    function applyBatch(batch, isLast = false) {
        if (!sessionActive || !Array.isArray(batch) || !accumulator) {
            return;
        }
        if (batch.length === 0 && !isLast) {
            return;
        }
        if (batch.length > 0) {
            hitEvents = hitEvents.concat(batch);
        }
        if (isLast) {
            allEvents = true;
        }
        pump();
    }

    function pump() {
        if (!accumulator || !sessionActive) {
            return;
        }
        const timeMs = Number(state.songTimeMs);
        const cutoff = Number.isFinite(timeMs) ? timeMs : Infinity;
        let advanced = false;
        while (feedIndex < hitEvents.length) {
            const event = hitEvents[feedIndex];
            const eventTime = Number(event.time);
            if (Number.isFinite(cutoff) && Number.isFinite(eventTime) && eventTime > cutoff) {
                break;
            }
            accumulator.feedEvents([event]);
            feedIndex += 1;
            advanced = true;
        }
        if (allEvents && feedIndex >= hitEvents.length) {
            accumulator.feedComplete();
            if (rawAccumulator && replayFrames.length > 0) {
                rawAccumulator = solveRawAccumulator(replayFrames, activeChart);
            }
            advanced = true;
        }
        if (advanced || Math.abs(cutoff - lastCutoff) > 200) {
            lastCutoff = cutoff;
            scheduleRender();
        }
    }

    function finishSession() {
        sessionActive = false;
        if (accumulator) {
            if (feedIndex < hitEvents.length) {
                accumulator.feedEvents(hitEvents.slice(feedIndex));
                feedIndex = hitEvents.length;
            }
            accumulator.feedComplete();
            if (rawAccumulator && replayFrames.length > 0) {
                rawAccumulator = solveRawAccumulator(replayFrames, activeChart);
            }
            scheduleRender();
        }
    }

    function setSpaceMode(mode) {
        spaceModeSetting = mode;
        if (accumulator) {
            accumulator.syncModifiers(options());
            rawAccumulator?.syncModifiers(options());
            scheduleRender();
        }
    }

    function setUnavailable(reason) {
        if (reason) {
            showError(reason);
        } else {
            showError("shell");
        }
    }

    function reset() {
        sessionActive = false;
        pendingChart = null;
        activeChart = null;
        accumulator = null;
        rawAccumulator = null;
        replayFrames = [];
        hitEvents = [];
        feedIndex = 0;
        lastCutoff = -Infinity;
        allEvents = false;
        lastRawSolveAt = 0;
        laneBounds = null;
        popoverCtrl.hidePopover();
        popoverCtrl.hideStatPopover();
        clearLoading();
        clearError();
        const nodes = dom();
        if (nodes) {
            nodes.sv1.textContent = "--";
            delete nodes.sv1.dataset.displayValue;
            nodes.od.hidden = true;
            nodes.hands.replaceChildren();
            nodes.stage.replaceChildren();
            laneButtons = [];
            laneKey = "";
        }
    }

    function showLoadingFromState() {
        const nodes = dom();
        if (!nodes) {
            return;
        }
        const changeKind = state.pendingChangeKind === "song" ? "song" : "difficulty";
        showLoading(changeKind);
    }

    function init() {
        const nodes = dom();
        if (!nodes || nodes.wrap.dataset.bound === "1") {
            return;
        }
        nodes.wrap.dataset.bound = "1";
        setInterval(() => {
            if (sessionActive) {
                pump();
            }
        }, 200);
        const sv1Block = nodes.wrap.querySelector(".laneacc-sv1");
        sv1Block?.addEventListener("mouseenter", () => popoverCtrl.showStatPopover(sv1Block, "sv1"));
        sv1Block?.addEventListener("mouseleave", popoverCtrl.hideStatPopover);
        nodes.hands.addEventListener("mouseover", (event) => {
            const block = event.target.closest?.(".laneacc-hand");
            if (block) {
                popoverCtrl.showStatPopover(block, block.dataset.hand);
            }
        });
        nodes.hands.addEventListener("mouseleave", popoverCtrl.hideStatPopover);
        document.addEventListener("click", (event) => {
            if (nodes.popover.hidden) {
                popoverCtrl.hideStatPopover();
                return;
            }
            if (!nodes.popover.contains(event.target) && !event.target.closest?.(".laneacc-lane")) {
                popoverCtrl.hidePopover();
            }
        });
        document.addEventListener("keydown", (event) => {
            if (event.key === "Escape") {
                popoverCtrl.hidePopover();
                popoverCtrl.hideStatPopover();
            }
        });
    }

    return {
        init,
        prepareChart,
        startSession,
        applyBatch,
        applyReplayFrames,
        finishSession,
        setSpaceMode,
        setUnavailable,
        render,
        replayEntrance,
        reset,
        setPending: showLoading,
    };
})();
