// view.js —— LaneAcc 卡片主体（DOM 渲染、Popover、占位态）。
//
// 纯逻辑在 accumulator.js；本文件只做 DOM 与 rAF 节流。所有 DOM 都是在
// `#laneacc-wrap` 内创建/复用，不触碰其它 contentBar 模块。

import { laneAccWrapEl, state } from "../appContext.js";
import { isShellRuntime } from "../feedback/feedbackSpec.js";
import { createAccumulator } from "./accumulator.js";
import { framesToSv1Events } from "./rawFrames.js";
import { formatAcc, formatMiss } from "./format.js";

const ERROR_TEXT = {
    shell: "LaneAcc needs the desktop shell.",
    contract: "Update the shell to use LaneAcc (contract v9).",
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
    let hasResult = false;
    let renderHandle = 0;
    let laneButtons = [];
    let laneKey = "";
    let laneBounds = null;
    let pendingBatches = [];
    let popoverCol = -1;
    let popoverTimer = 0;
    let statPopover = null;
    let statKind = null;
    let latestSnapshot = null;
    let stageVisible = false;
    let lastColumnCount = 4;
    const numericTokens = new WeakMap();

    function animateNumber(element, target, format, duration = 220) {
        if (!element) {
            return;
        }
        const numericTarget = Number(target);
        if (target == null || !Number.isFinite(numericTarget)) {
            numericTokens.delete(element);
            element.textContent = "--";
            delete element.dataset.displayValue;
            return;
        }
        const previous = Number(element.dataset.displayValue);
        const from = Number.isFinite(previous) ? previous : numericTarget;
        element.dataset.displayValue = String(numericTarget);
        if (Math.abs(numericTarget - from) < 0.00001) {
            element.textContent = format(numericTarget);
            return;
        }
        const token = Symbol("laneacc-number");
        numericTokens.set(element, token);
        const start = performance.now();
        const tick = (now) => {
            if (numericTokens.get(element) !== token) {
                return;
            }
            const progress = Math.min(1, (now - start) / duration);
            const eased = 1 - ((1 - progress) ** 3);
            const value = from + (numericTarget - from) * eased;
            element.textContent = format(value);
            if (progress < 1) {
                requestAnimationFrame(tick);
            }
        };
        requestAnimationFrame(tick);
    }

    function options() {
        const odFlag = state.odFlag;
        // 桥源（Malody/Etterna）的 odFlag 已是折算后的等效 OD：不再叠加 HR/EZ 倍率。
        const odResolved = state.activeSource !== "osu"
            && odFlag != null && odFlag !== "" && Number.isFinite(Number(odFlag));
        return {
            speedRate: state.speedRate,
            mods: Array.isArray(state.modCodes) ? state.modCodes : [],
            odFlag,
            spaceMode: state.laneAccSpaceMode,
            odResolved,
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

    function showError(reason) {
        const nodes = dom();
        if (!nodes) {
            return;
        }
        nodes.wrap.dataset.state = "error";
        nodes.summary.hidden = true;
        nodes.stage.hidden = true;
        nodes.popover.hidden = true;
        nodes.error.hidden = false;
        nodes.error.textContent = ERROR_TEXT[reason] || ERROR_TEXT.chart;
        hideStatPopover();
    }

    function showLoading(kind = "difficulty") {
        const nodes = dom();
        if (!nodes) {
            return;
        }
        nodes.wrap.dataset.state = "loading";
        nodes.summary.hidden = false;
        nodes.stage.hidden = false;
        nodes.popover.hidden = true;
        nodes.error.hidden = true;
        nodes.error.textContent = "";
        nodes.stage.classList.remove("is-entering");
        nodes.stage.classList.add("is-loading", kind === "song" ? "is-loading-song" : "is-loading-difficulty");
        nodes.stage.dataset.changeKind = kind;
        if (!nodes.stage.querySelector(".laneacc-lane")) {
            for (let index = 0; index < lastColumnCount; index += 1) {
                const placeholder = document.createElement("div");
                placeholder.className = "laneacc-lane is-placeholder";
                placeholder.dataset.col = String(index);
                placeholder.style.setProperty("--lane-index", String(index));
                nodes.stage.append(placeholder);
            }
        }
        hideStatPopover();
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

    function createHandBlock(label) {
        const block = document.createElement("div");
        block.className = "laneacc-hand";
        block.dataset.hand = label === "L" ? "left" : label === "M" ? "space" : "right";
        block.dataset.handLabel = label;
        const name = document.createElement("span");
        name.className = "laneacc-hand-label";
        name.textContent = label;
        const acc = document.createElement("span");
        acc.className = "laneacc-hand-acc";
        acc.textContent = "--";
        const miss = document.createElement("span");
        miss.className = "laneacc-hand-miss";
        miss.hidden = true;
        block.append(name, acc, miss);
        return block;
    }

    function renderHands(snapshot) {
        const nodes = dom();
        if (!nodes) {
            return;
        }
        const specs = [["L", snapshot.hands.left], ["R", snapshot.hands.right]];
        if (snapshot.hands.space) {
            specs.splice(1, 0, ["M", snapshot.hands.space]);
        }
        nodes.hands.dataset.count = String(specs.length);
        const wanted = new Set(specs.map(([label]) => label));
        for (const block of [...nodes.hands.querySelectorAll(".laneacc-hand")]) {
            if (!wanted.has(block.dataset.handLabel)) {
                block.remove();
            }
        }
        for (const [label, hand] of specs) {
            let block = nodes.hands.querySelector(`.laneacc-hand[data-hand-label="${label}"]`);
            if (!block) {
                block = createHandBlock(label);
                nodes.hands.append(block);
            }
            const acc = block.querySelector(".laneacc-hand-acc");
            animateNumber(acc, hand?.acc, (value) => formatAcc(value));
            const miss = block.querySelector(".laneacc-hand-miss");
            const missCount = Number(hand?.miss) || 0;
            miss.hidden = missCount <= 0;
            if (missCount > 0) {
                animateNumber(miss, missCount, (value) => `Miss ${formatMiss(value)}`);
            } else {
                miss.textContent = "";
                delete miss.dataset.displayValue;
            }
        }
    }

    function ensureStatPopover() {
        const nodes = dom();
        if (!nodes) {
            return null;
        }
        if (!statPopover) {
            statPopover = document.createElement("div");
            statPopover.className = "laneacc-stat-popover";
            statPopover.hidden = true;
            nodes.wrap.append(statPopover);
        }
        return statPopover;
    }

    function hideStatPopover() {
        statKind = null;
        if (statPopover) {
            statPopover.hidden = true;
        }
    }

    function refreshStatPopover() {
        if (!statKind || !statPopover || statPopover.hidden) {
            return;
        }
        const nodes = dom();
        if (!nodes) {
            return;
        }
        const anchor = statKind === "sv1"
            ? nodes.wrap.querySelector(".laneacc-sv1")
            : nodes.hands.querySelector(`.laneacc-hand[data-hand="${statKind}"]`);
        if (anchor) {
            showStatPopover(anchor, statKind);
        }
    }

    function showStatPopover(anchor, kind) {
        const nodes = dom();
        const snapshot = latestSnapshot || accumulator?.snapshot();
        if (snapshot && rawAccumulator && replayFrames.length > 0) {
            const raw = rawSnapshotWithLazerBoundary(rawAccumulator.snapshot());
            snapshot.sv1Counts = raw.sv1Counts;
            snapshot.sv1Acc = raw.sv1Acc;
            snapshot.sv1Total = raw.sv1Total;
            snapshot.sv1Points = raw.sv1Points;
            snapshot.maxCombo = raw.maxCombo;
        }
        if (!nodes || !snapshot || !anchor) {
            return;
        }
        const hand = kind === "left" ? snapshot.hands.left : kind === "right" ? snapshot.hands.right : null;
        if (kind !== "sv1" && !hand) {
            return;
        }
        statKind = kind;
        const popover = ensureStatPopover();
        if (!popover) {
            return;
        }
        const labels = ["MAX", "300", "200", "100", "50", "Miss"];
        const values = kind === "sv1"
            ? snapshot.sv1Counts
            : [...hand.counts].reverse();
        const combo = kind === "sv1" ? snapshot.maxCombo : hand.maxCombo;
        const title = document.createElement("div");
        title.className = "laneacc-popover-title";
        title.textContent = kind === "sv1" ? "ScoreV1" : kind === "left" ? "Left Hand" : "Right Hand";
        const grid = document.createElement("div");
        grid.className = "laneacc-popover-grid";
        for (let index = 0; index < labels.length; index += 1) {
            const cell = document.createElement("span");
            cell.className = "laneacc-popover-cell";
            cell.textContent = `${labels[index]} ${values[index] ?? 0}`;
            grid.append(cell);
        }
        const comboLine = document.createElement("div");
        comboLine.className = "laneacc-popover-combo";
        comboLine.textContent = `Max Combo ${combo ?? 0}`;
        popover.replaceChildren(title, grid, comboLine);
        popover.hidden = false;
        const wrapRect = nodes.wrap.getBoundingClientRect();
        const anchorRect = anchor.getBoundingClientRect();
        const popRect = popover.getBoundingClientRect();
        let left = anchorRect.left - wrapRect.left + anchorRect.width / 2 - popRect.width / 2;
        left = Math.max(4, Math.min(left, wrapRect.width - popRect.width - 4));
        let top = anchorRect.top - wrapRect.top - popRect.height - 6;
        if (top < 4) {
            top = anchorRect.bottom - wrapRect.top + 6;
        }
        popover.style.left = `${left}px`;
        popover.style.top = `${top}px`;
    }

    function showPopover(col, lane) {
        const nodes = dom();
        if (!nodes || !lane) {
            return;
        }
        popoverCol = col;
        nodes.popover.hidden = false;
        nodes.popover.dataset.col = String(col);
        const columns = Math.max(1, activeChart.columnCount);
        const counts = lane.counts;
        nodes.popover.replaceChildren();
        const title = document.createElement("div");
        title.className = "laneacc-popover-title";
        title.textContent = `Lane ${col + 1} · ${formatAcc(lane.acc)}`;
        const grid = document.createElement("div");
        grid.className = "laneacc-popover-grid";
        const labels = ["MAX", "300", "200", "100", "50", "Miss"];
        const values = [counts[5], counts[4], counts[3], counts[2], counts[1], counts[0]];
        for (let index = 0; index < labels.length; index += 1) {
            const cell = document.createElement("span");
            cell.className = "laneacc-popover-cell";
            cell.textContent = `${labels[index]} ${values[index]}`;
            grid.append(cell);
        }
        const comboLine = document.createElement("div");
        comboLine.className = "laneacc-popover-combo";
        comboLine.textContent = `Max Combo ${lane.maxCombo ?? 0}`;
        nodes.popover.append(title, grid, comboLine);
        // 用像素夹紧：百分比夹紧在窄卡片/边缘轨仍会把 tooltip 截断。
        const stageWidth = nodes.stage?.clientWidth || 0;
        const popWidth = nodes.popover.offsetWidth || 0;
        if (stageWidth > 0 && popWidth > 0) {
            const half = popWidth / 2 + 4;
            const center = ((col + 0.5) / columns) * stageWidth;
            const clamped = Math.min(Math.max(center, half), Math.max(half, stageWidth - half));
            nodes.popover.style.left = `${clamped}px`;
        }
        schedulePopoverHide();
    }

    function schedulePopoverHide() {
        if (popoverTimer) {
            clearTimeout(popoverTimer);
        }
        popoverTimer = setTimeout(() => {
            popoverTimer = 0;
            hidePopover();
        }, 4000);
    }

    function hidePopover() {
        const nodes = dom();
        popoverCol = -1;
        if (popoverTimer) {
            clearTimeout(popoverTimer);
            popoverTimer = 0;
        }
        if (nodes) {
            nodes.popover.hidden = true;
        }
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
            const open = () => showPopover(col, latestSnapshot?.lanes[col] ?? accumulator?.snapshot()?.lanes[col]);
            button.addEventListener("pointerenter", open);
            button.addEventListener("pointerleave", hidePopover);
            button.addEventListener("focus", open);
            button.addEventListener("blur", hidePopover);
            button.addEventListener("click", (event) => {
                event.stopPropagation();
                if (popoverCol === col && !nodes.popover.hidden) {
                    hidePopover();
                } else {
                    open();
                }
            });
            nodes.stage.append(button);
            laneButtons.push(button);
        }
        // popover 是 stage 的绝对定位子元素：replaceChildren 后补回，保证贴在条上方。
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
        // OD 胶囊只给 Lazer 等效 OD；速度不属于判定窗口语义，不再单列。
        const effectiveOd = snapshot.windows?.displayOd ?? snapshot.windows?.od;
        if (Number.isFinite(effectiveOd)) {
            const rounded = Math.round(Number(effectiveOd) * 100) / 100;
            const eff = Number.isInteger(rounded) ? rounded.toFixed(1) : String(rounded);
            nodes.od.hidden = false;
            // 只显示一个等效 OD，避免 `OD8.5 + HR` 在窄卡片里被截断。
            nodes.od.textContent = `OD${eff}`;
        } else {
            nodes.od.hidden = true;
        }
        renderHands(snapshot);
        ensureLanes(snapshot);
        const spaceIndex = snapshot.hands.space
            ? Math.floor(activeChart.columnCount / 2)
            : -1;
        // 动态上下限：上限取本局最好轨道（不再固定 100），下限取最差轨道再留 1 点，
        // 这样 99.5–100 这种窄区间也能看出差异。
        // 边界只向外扩；且不从“全 100 的预览”建立边界，避免开局后 92 分被 100 的上限压扁。
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
        if (popoverCol >= 0 && !nodes.popover.hidden) {
            showPopover(popoverCol, snapshot.lanes[popoverCol]);
        }
        refreshStatPopover();
        // 从别的页签切回 LaneAcc 时重放入场动画（元素已存在，CSS 动画不会自己重跑）。
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
        if (a === b) {
            return true;
        }
        const first = (chart) => chart.noteCount > 0 ? chart.noteStarts[0] : -1;
        const last = (chart) => chart.noteCount > 0 ? chart.noteEnds[chart.noteCount - 1] : -1;
        return a.columnCount === b.columnCount
            && a.noteCount === b.noteCount
            && a.od === b.od
            && first(a) === first(b)
            && last(a) === last(b)
            && (!a.identity || !b.identity || a.identity === b.identity);
    }

    function clearSessionEvents() {
        hitEvents = [];
        replayFrames = [];
        feedIndex = 0;
        lastCutoff = -Infinity;
        allEvents = false;
        lastRawSolveAt = 0;
        pendingBatches = [];
        latestSnapshot = null;
    }

    function createFor(chart) {
        const nodes = dom();
        nodes?.stage.classList.remove(
            "is-entering",
            "is-loading",
            "is-loading-song",
            "is-loading-difficulty",
        );
        activeChart = chart;
        lastColumnCount = chart.columnCount;
        accumulator = createAccumulator(chart, options());
        rawAccumulator = createAccumulator(chart, options());
        feedIndex = 0;
        lastCutoff = -Infinity;
        lastRawSolveAt = 0;
        laneBounds = null;
        if (pendingBatches.length > 0) {
            const batches = pendingBatches;
            pendingBatches = [];
            for (const batch of batches) {
                if (batch?.reset) {
                    hitEvents = [];
                }
                if (Array.isArray(batch?.events)) {
                    hitEvents.push(...batch.events);
                }
            }
        }
        pump();
        laneKey = "";
        laneButtons = [];
        clearError();
        render();
        // 新谱面/重试即使页签本来就可见也要重放入场动画。
        if (laneAccWrapEl.offsetParent !== null) {
            replayEntrance();
        }
    }

    /** 分析完成：只暂存；若当前没有战报，则显示 100.00% 预览。 */
    function prepareChart(chart) {
        if (!isShellRuntime()) {
            pendingChart = null;
            activeChart = null;
            accumulator = null;
            showError("shell");
            return;
        }
        if (state.shellContractVersion > 0 && state.shellContractVersion < 9) {
            pendingChart = null;
            activeChart = null;
            accumulator = null;
            showError("contract");
            return;
        }
        if (state.laneAccScoreReady === false) {
            pendingChart = null;
            activeChart = null;
            accumulator = null;
            showError("offsets");
            return;
        }
        if (state.client && state.client !== "lazer") {
            pendingChart = null;
            activeChart = null;
            accumulator = null;
            showError("lazer");
            return;
        }
        if (!chart || chart.error) {
            const reason = chart?.error === "keys" ? "keys" : chart?.error === "mode" ? "mania" : "chart";
            pendingChart = null;
            activeChart = null;
            accumulator = null;
            hasResult = false;
            sessionActive = false;
            showError(reason);
            return;
        }
        if (activeChart && !sameChart(activeChart, chart)) {
            // 切图/换难度：旧事件的 note 索引只对旧谱面有效，必须先清空并重建
            // 累加器；否则未匹配的旧头/尾会按新谱面落成 Miss，Acc 会瞬间掉下去。
            clearSessionEvents();
            pendingChart = chart;
            sessionActive = false;
            hasResult = false;
            createFor(chart);
            return;
        }
        pendingChart = chart;
        if (!sessionActive && !hasResult) {
            createFor(chart);
        } else if (sessionActive && !accumulator) {
            createFor(chart);
        }
    }

    /** 进入 play：采纳 pending 谱面并清空本局。 */
    function startSession() {
        const chart = pendingChart || activeChart;
        if (!chart) {
            return false;
        }
        pendingChart = chart;
        sessionActive = true;
        hasResult = false;
        allEvents = false;
        hitEvents = [];
        feedIndex = 0;
        lastCutoff = -Infinity;
        createFor(chart);
        return true;
    }

    /** 当前播放位置（chart ms）；尚无时钟时返回 -Infinity（不喂任何判定）。 */
    function currentCutoff() {
        if (allEvents || hasResult) {
            return Infinity;
        }
        const live = Number(state.songTimeMs);
        return Number.isFinite(live) ? live : -Infinity;
    }

    /** 按时钟把 hitEvents 逐条喂给 accumulator；时钟回跳时重建。 */
    function pump() {
        if (!accumulator || !activeChart) {
            return;
        }
        const cutoff = currentCutoff();
        if (cutoff + 1 < lastCutoff) {
            accumulator = createAccumulator(activeChart, options());
            rawAccumulator = createAccumulator(activeChart, options());
            feedIndex = 0;
            lastCutoff = -Infinity;
            lastRawSolveAt = 0;
        }
        while (feedIndex < hitEvents.length) {
            const time = accumulator.eventTime(hitEvents[feedIndex]);
            if (Number.isFinite(time) && time > cutoff) {
                break;
            }
            accumulator.applyEvent(hitEvents[feedIndex]);
            feedIndex += 1;
        }
        lastCutoff = cutoff;
        rebuildRawAccumulator(true, cutoff);
        scheduleRender();
    }

    /** 原始 ReplayFrame：只用于 SV1，不改变 gameCounts。 */
    function rebuildRawAccumulator(force = false, cutoff = currentCutoff()) {
        if (!rawAccumulator || !activeChart) {
            return;
        }
        const now = Date.now();
        if (!force && now - lastRawSolveAt < 500) {
            return;
        }
        lastRawSolveAt = now;
        const windows = accumulator?.snapshot()?.windows;
        if (!windows) {
            return;
        }
        const final = cutoff === Infinity;
        const visible = final
            ? replayFrames
            : replayFrames.filter((frame) => frame.time <= cutoff + 250);
        if (visible.length === 0) {
            rawAccumulator = createAccumulator(activeChart, options());
            return;
        }
        let events = framesToSv1Events(
            activeChart,
            visible,
            windows,
            options().mods,
            options().speedRate,
            null,
            cutoff,
        );
        rawAccumulator.applyBatch({ reset: true, events });
        // 原始帧的 first-release 在「掉落 + regrab」上会多算 Miss；若比 Lazer 判定
        // 多出明显数量，才用 hitEvents 的 15/13 掉落集合重建成 Stable 的 50/100。
        const rawMiss = rawAccumulator.snapshot().sv1Counts[5];
        const hitMiss = accumulator?.snapshot()?.sv1Counts?.[5] ?? 0;
        if (rawMiss > hitMiss + 5) {
            events = framesToSv1Events(
                activeChart,
                visible,
                windows,
                options().mods,
                options().speedRate,
                accumulator?.noRegrabDropNotes?.() ?? null,
                cutoff,
            );
            rawAccumulator.applyBatch({ reset: true, events });
        }
    }

    function applyReplayFrames(payload) {
        if (payload?.reset) {
            replayFrames = [];
        }
        if (Array.isArray(payload?.frames)) {
            for (const frame of payload.frames) {
                if (Number.isFinite(frame?.time) && Number.isFinite(frame?.keys)) {
                    replayFrames.push({ time: frame.time, keys: frame.keys >>> 0 });
                }
            }
        }
        if (accumulator) {
            pump();
        } else {
            scheduleRender();
        }
    }

    function applyBatch(payload) {
        if (payload?.reset) {
            hitEvents = [];
            feedIndex = 0;
            lastCutoff = -Infinity;
            allEvents = false;
        }
        if (Array.isArray(payload?.events)) {
            hitEvents.push(...payload.events);
        }
        if (!accumulator) {
            // 全量快照按 2048 事件分块到达；谱面还没准备好时必须全部排队，
            // 否则后一块会覆盖前一块，页面只统计到最后一块（真机少 20–30%）。
            if (payload?.reset) {
                pendingBatches = [];
            }
            pendingBatches.push(payload);
            if (!startSession()) {
                return;
            }
            return;
        }
        accumulator.syncModifiers(options());
        pump();
    }

    /** 结算：只冻结显示，分母仍是已判定计数。 */
    function finishSession() {
        if (!accumulator) {
            return;
        }
        allEvents = true;
        pump();
        rawAccumulator?.finish();
        accumulator.finish();
        sessionActive = false;
        hasResult = true;
        scheduleRender();
    }

    function setSpaceMode(mode) {
        if (accumulator) {
            accumulator.setSpaceMode(mode);
        }
        laneKey = "";
        scheduleRender();
    }

    function setUnavailable(reason) {
        showError(reason);
    }

    /** 谱面身份变化：保留卡片结构和旧内容，叠加对应切图方式的懒加载态。 */
    function reset() {
        pendingChart = null;
        activeChart = null;
        accumulator = null;
        sessionActive = false;
        hasResult = false;
        laneBounds = null;
        laneButtons = [];
        laneKey = "";
        pendingBatches = [];
        hitEvents = [];
        replayFrames = [];
        rawAccumulator = null;
        feedIndex = 0;
        lastCutoff = -Infinity;
        allEvents = false;
        lastRawSolveAt = 0;
        latestSnapshot = null;
        hidePopover();
        hideStatPopover();
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
        sv1Block?.addEventListener("mouseenter", () => showStatPopover(sv1Block, "sv1"));
        sv1Block?.addEventListener("mouseleave", hideStatPopover);
        nodes.hands.addEventListener("mouseover", (event) => {
            const block = event.target.closest?.(".laneacc-hand");
            if (block) {
                showStatPopover(block, block.dataset.hand);
            }
        });
        nodes.hands.addEventListener("mouseleave", hideStatPopover);
        document.addEventListener("click", (event) => {
            if (nodes.popover.hidden) {
                hideStatPopover();
                return;
            }
            if (!nodes.popover.contains(event.target) && !event.target.closest?.(".laneacc-lane")) {
                hidePopover();
            }
        });
        document.addEventListener("keydown", (event) => {
            if (event.key === "Escape") {
                hidePopover();
                hideStatPopover();
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
