// view.js —— LaneAcc 卡片主体（DOM 渲染、Popover、占位态）。
//
// 纯逻辑在 js/laneAcc/；本文件只做 DOM 与 rAF 节流。所有 DOM 都是在
// `#laneacc-wrap` 内创建/复用，不触碰其它 contentBar 模块。

import { laneAccWrapEl, state } from "../../core/appContext.js";
import { isShellRuntime } from "../../feedback/feedbackSpec.js";
import { createAccumulator } from "../../../laneAcc/accumulator.js";
import { framesToSv1Events } from "../../../laneAcc/rawFrames.js";
import { framesToStableV2Events } from "../../../laneAcc/stableV2.js";
import { maniaRandomColumnMap } from "../../../laneAcc/random.js";
import { remapChartColumns } from "../../../laneAcc/chart.js";
import { formatAcc, formatMiss } from "../../../laneAcc/format.js";
import { createPopoverController } from "./popover.js";
import { renderHands } from "./hands.js";

// 分轨柱状图的自适应下限：少于这个判定数的轨道不参与范围计算，
// 避免开局几个垃圾判定把 lower bound 永久拉到很低。
const LANE_BOUNDS_MIN_JUDGED = 8;

const ERROR_TEXT = {
    shell: "LaneAcc needs the desktop shell.",
    contract: "Update the shell to use LaneAcc (contract v10).",
    native: "The shell's osu! memory reader is unavailable.",
    lazer: "LaneAcc works in osu!lazer only.",
    mania: "LaneAcc supports native mania maps only.",
    keys: "LaneAcc supports 4K–18K.",
    offsets: "The offset table has no LaneAcc section.",
    notes: "Stable per-note data unavailable; update the offset table.",
    chart: "Chart text unavailable; switch maps to reload.",
    random: "LaneAcc is unavailable while Random (RD) is active.",
    ppysb: "LaneAcc is not supported for ppy.sb injection.",
};

function dom() {
    if (!laneAccWrapEl) {
        return null;
    }
    const stage = laneAccWrapEl.querySelector("#laneacc-stage");
    let popover = laneAccWrapEl.querySelector("#laneacc-popover");
    // `stage.replaceChildren()`（ensureLanes / reset）会连带移除 popover；下一次
    // querySelector 查不到时若直接 `append(null)`，DOM 里会留下文本 "null"。
    // popover 是舞台的常驻子元素，丢了就补回来。
    if (!popover && stage) {
        popover = document.createElement("div");
        popover.id = "laneacc-popover";
        popover.className = "laneacc-popover";
        popover.hidden = true;
        stage.append(popover);
    }
    return {
        wrap: laneAccWrapEl,
        summary: laneAccWrapEl.querySelector(".laneacc-summary"),
        badge: laneAccWrapEl.querySelector(".laneacc-badge"),
        sv1: laneAccWrapEl.querySelector("#laneacc-sv1-value"),
        hands: laneAccWrapEl.querySelector("#laneacc-hands"),
        stage,
        popover,
        error: laneAccWrapEl.querySelector("#laneacc-error"),
        od: laneAccWrapEl.querySelector("#laneacc-od"),
    };
}

export const laneAccView = (() => {
    let pendingChart = null;
    let activeChart = null;
    let accumulator = null;
    let rawAccumulator = null;
    let v2TopAccumulator = null;
    let frozenTopSnapshot = null;
    let replayFrames = [];
    let hitEvents = [];
    let feedIndex = 0;
    let lastCutoff = -Infinity;
    let allEvents = false;
    let lastRawSolveAt = 0;
    let sessionActive = false;
    let hasResult = false;
    let sessionScoreMode = null;
    let renderHandle = 0;
    let laneButtons = [];
    let laneKey = "";
    let lastColumnCount = 4;
    let loadingChangeKind = "difficulty";
    let spaceModeSetting = "auto";
    let latestSnapshot = null;
    let laneBounds = null;
    let stageVisible = false;
    // 全量快照按 2048 事件分块到达；谱面还没准备好时必须排队（否则后面的块会覆盖前面的块）。
    let pendingBatches = [];

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
        const odFlag = state.odFlag ?? null;
        // 桥源（Malody/Etterna）的 odFlag 已是折算后的等效 OD：不再叠加 HR/EZ 倍率。
        const odResolved = state.activeSource !== "osu"
            && odFlag != null && odFlag !== ""
            && Number.isFinite(Number(odFlag));
        return {
            spaceMode: spaceModeSetting,
            speedRate: state.speedRate || 1.0,
            mods: Array.isArray(state.modCodes) ? state.modCodes : [],
            odFlag,
            odResolved,
        };
    }

    /** 分轨/左右手的数据源：Stable V1 用壳直读的 per-note；Stable V2 用原始帧自己算。 */
    function resolveScoreMode() {
        const client = String(state.client || "").toLowerCase();
        if (client.includes("stable")) {
            const mods = Array.isArray(state.modCodes) ? state.modCodes : [];
            const scoreV2 = mods.some((mod) => /^(SV2|V2)$/i.test(String(mod)));
            return scoreV2 ? "stable-sim" : "stable-native";
        }
        return "lazer";
    }

    /** 本局开始时的模式；结算屏 mods 抖动时保持本局方向，不能 SV2→SV1。 */
    function activeScoreMode() {
        return (sessionActive || hasResult) && sessionScoreMode ? sessionScoreMode : resolveScoreMode();
    }

    /** stable-sim 用 accumulator 的 native 计数模式来消费自己算出的每 note 事件。 */
    function accumulatorMode() {
        const mode = activeScoreMode();
        return mode === "stable-sim" ? "stable-native" : mode;
    }

    function accumulatorOptions(scoreMode, laneWeights = 305) {
        return { ...options(), scoreMode, laneWeights };
    }

    function laneWeightsForMode(mode) {
        return mode === "stable-native" ? 300 : 305;
    }

    function currentMods() {
        return Array.isArray(state.modCodes) ? state.modCodes : [];
    }

    function hasRandomMod() {
        return currentMods().some((mod) => /^(RD|RANDOM)$/i.test(String(mod)));
    }

    /** Lazer 有 seed 才支持 RD；Stable RD（无 seed）整模块禁用。 */
    function randomSeed() {
        const seed = Number(state.randomSeed);
        return Number.isInteger(seed) && seed >= -2147483648 && seed <= 2147483647 ? seed : null;
    }

    function randomModBlocked() {
        return hasRandomMod() && !(activeScoreMode() === "lazer" && randomSeed() != null);
    }

    /** Lazer RD：谱面列按 ManiaModRandom 的置换变换；native 事件/帧保持游戏列。 */
    function applyRandomColumns(chart) {
        if (!chart || activeScoreMode() !== "lazer" || !hasRandomMod()) {
            return;
        }
        const seed = randomSeed();
        if (seed == null || chart.__randomSeed === seed) {
            return;
        }
        const shuffled = maniaRandomColumnMap(chart.columnCount, seed);
        if (!shuffled) {
            return;
        }
        // 同一 chart 对象可能因 seed 变化再次进入；从原始列重新映射，避免二次置换。
        if (!chart.__rdOriginalColumns) {
            chart.__rdOriginalColumns = new Uint8Array(chart.columns);
        } else {
            chart.columns.set(chart.__rdOriginalColumns);
        }
        if (!remapChartColumns(chart, shuffled)) {
            return;
        }
        chart.__randomSeed = seed;
    }

    function hasMirrorMod() {
        return currentMods().some((mod) => /^MR$/i.test(String(mod)));
    }

    /** MR 下游戏事件列是镜像后的；转回谱面原始列，分轨/左右手才对得上。 */
    function maybeMirrorEvents(events) {
        if (!hasMirrorMod() || !Array.isArray(events)) {
            return events;
        }
        const chart = activeChart || pendingChart;
        if (!chart) {
            return events;
        }
        const last = chart.columnCount - 1;
        return events.map((event) => {
            const col = Number(event?.col);
            return Number.isInteger(col) ? { ...event, col: last - col } : event;
        });
    }

    function rawScoreMode() {
        return activeScoreMode() === "lazer" ? "lazer" : "stable-v1";
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
        if (nodes.summary) nodes.summary.hidden = true;
        if (nodes.stage) nodes.stage.hidden = true;
        if (nodes.error) {
            nodes.error.hidden = false;
            nodes.error.textContent = ERROR_TEXT[key] || ERROR_TEXT.shell;
        }
    }

    function showLoading(changeKind = "song") {
        // 结算后重算可能先走到 loading 占位：不能把已经冻结的终态盖掉。
        if (hasResult) {
            return;
        }
        const nodes = dom();
        if (!nodes) {
            return;
        }
        loadingChangeKind = changeKind === "song" ? "song" : "difficulty";
        nodes.wrap.dataset.state = "loading";
        nodes.wrap.dataset.changeKind = changeKind;
        if (nodes.summary) nodes.summary.hidden = false;
        if (nodes.popover) nodes.popover.hidden = true;
        if (nodes.stage) {
            nodes.stage.hidden = false;
            nodes.stage.classList.remove(
                "is-entering",
                "is-loading",
                "is-loading-song",
                "is-loading-difficulty",
            );
            nodes.stage.classList.add("is-loading", `is-loading-${changeKind}`);
            nodes.stage.dataset.changeKind = changeKind;
            // 还没有真实轨道时先铺占位轨道，保证“切图加载”本身有动画，
            // 而不是空白等分析完成后再突然弹出。
            if (!nodes.stage.querySelector(".laneacc-lane")) {
                for (let index = 0; index < lastColumnCount; index += 1) {
                    const placeholder = document.createElement("div");
                    placeholder.className = "laneacc-lane is-placeholder";
                    placeholder.dataset.col = String(index);
                    placeholder.style.setProperty("--lane-index", String(index));
                    nodes.stage.append(placeholder);
                }
            }
        }
        if (nodes.error) {
            nodes.error.hidden = true;
        }
    }

    function clearLoading() {
        const nodes = dom();
        if (!nodes) {
            return;
        }
        nodes.stage?.classList.remove(
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
        loadingChangeKind = "difficulty";
        if (nodes.summary) nodes.summary.hidden = false;
        if (nodes.stage) nodes.stage.hidden = false;
        if (nodes.error) {
            nodes.error.hidden = true;
            nodes.error.textContent = "";
        }
    }

    function ensureLanes(snapshot) {
        const nodes = dom();
        if (!nodes?.stage) {
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
        if (nodes.popover) {
            nodes.stage.append(nodes.popover);
        }
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
        if (state.laneAccReady?.ppysbActive === true) {
            showError("ppysb");
            return;
        }
        const nodes = dom();
        if (nodes && randomModBlocked()) {
            showError("random");
            return;
        }
        if (!nodes || !accumulator || !activeChart) {
            return;
        }
        const currentOptions = options();
        accumulator.syncModifiers(currentOptions);
        const scoreMode = activeScoreMode();
        // stable-native：分轨来自壳直读的游戏 per-note 判定（accumulator）；
        // stable-v1：旧路径（分轨来自 rawAccumulator 的 V1 重判，仅作 fallback）。
        const laneSource = scoreMode === "stable-v1" ? rawAccumulator : accumulator;
        if (!laneSource) {
            return;
        }
        const snapshot = laneSource.snapshot();
        // Stable 无 SV2：顶部展示 SV1→SV2 折算值，分轨仍是 V1。
        // Stable 有 SV2（同 Lazer）：顶部展示 SV2→SV1 折算值，分轨是 V2。
        const topIsV2 = scoreMode === "stable-native";
        if (nodes.badge) {
            nodes.badge.textContent = topIsV2 ? "ScoreV2" : "ScoreV1";
        }
        if (topIsV2) {
            const declaredHolds = Number(activeChart.holdCount);
            const noHolds = Number.isFinite(declaredHolds)
                ? declaredHolds === 0
                : !Array.from(activeChart.isHold || []).some(Boolean);
            // 0 LN 时 V1/V2 的 tap 判定相同，直接按 305 权重复算，不能再用
            // 原始帧重建（帧流尾段缺失会被误判成 miss）。
            if (noHolds && accumulator) {
                const v1 = accumulator.snapshot();
                const counts = v1.counts;
                const weights = [0, 50, 100, 200, 300, 305];
                let points = 0;
                for (let i = 0; i < 6; i += 1) points += (counts[i] || 0) * weights[i];
                const total = Number(v1.gameTotal) || 0;
                snapshot.sv1Counts = [...counts].reverse();
                snapshot.sv1Points = points;
                snapshot.sv1Total = total;
                snapshot.sv1Acc = total > 0 ? (points / (total * 305)) * 100 : 100;
                snapshot.maxCombo = v1.gameMaxCombo;
            } else {
                const v2Top = frozenTopSnapshot || v2TopAccumulator?.snapshot();
                if (v2Top) {
                    // v2Top.counts 是 miss..MAX；popover 统一按 MAX..Miss 显示。
                    snapshot.sv1Counts = [...v2Top.counts].reverse();
                    snapshot.sv1Acc = v2Top.gameAcc;
                    snapshot.sv1Total = v2Top.gameTotal;
                    snapshot.sv1Points = v2Top.gamePoints;
                    snapshot.maxCombo = v2Top.gameMaxCombo;
                }
            }
            snapshot.sv1Label = "ScoreV2";
        } else if (rawAccumulator && replayFrames.length > 0) {
            // Lazer / Stable SV2：顶部取 rawAccumulator 的 V1 口径；Lazer 才做边界混合。
            const raw = scoreMode === "lazer"
                ? rawSnapshotWithLazerBoundary(rawAccumulator.snapshot())
                : rawAccumulator.snapshot();
            snapshot.sv1Counts = raw.sv1Counts;
            snapshot.sv1Acc = raw.sv1Acc;
            snapshot.sv1Total = raw.sv1Total;
            snapshot.sv1Points = raw.sv1Points;
            snapshot.maxCombo = raw.maxCombo;
            snapshot.sv1Label = "ScoreV1";
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
            .filter((lane) => lane.judged >= LANE_BOUNDS_MIN_JUDGED && Number.isFinite(lane.acc))
            .map((lane) => lane.acc);
        if (judgedAccs.length > 0) {
            const nextLow = Math.max(0, Math.floor(Math.min(...judgedAccs) - 1));
            const nextHigh = Math.min(100, Math.ceil(Math.max(...judgedAccs)));
            if (!laneBounds) {
                if (Math.min(...judgedAccs) < 99.999) {
                    laneBounds = { low: nextLow, high: nextHigh };
                }
            } else {
                // 每次都用当前分段重新计算，而不是只让 lower bound 下降：
                // 开局低准度过后，后续高准度会把范围重新收紧，柱状图恢复区分度。
                laneBounds.low = nextLow;
                laneBounds.high = nextHigh;
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
        popoverCtrl.refreshPopover();
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
        const first = (chart) => (chart.noteCount > 0 ? chart.noteStarts[0] : -1);
        const last = (chart) => (chart.noteCount > 0 ? chart.noteEnds[chart.noteCount - 1] : -1);
        return a.columnCount === b.columnCount
            && a.noteCount === b.noteCount
            && a.od === b.od
            && first(a) === first(b)
            && last(a) === last(b)
            && a.identity === b.identity;
    }

    function evaluateError() {
        if (!isShellRuntime()) {
            return "shell";
        }
        const contract = Number(state.shellContractVersion ?? 0);
        if (contract > 0 && contract < 10) {
            return "contract";
        }
        const client = String(state.client || "").toLowerCase();
        if (state.laneAccReady?.ppysbActive === true || client.includes("ppysb")) {
            return "ppysb";
        }
        if (client.includes("lazer")) {
            if (state.laneAccReady?.lazer === false) {
                return "offsets";
            }
        } else if (client.includes("stable")) {
            // Stable 分轨/左右手必须用游戏自己的 per-note 判定；
            // 缺 notes 段时显示占位，绝不回退到近似的 V1 公式。
            const notesReady = state.laneAccReady?.stableNotes === true;
            if (!notesReady) {
                return "notes";
            }
        } else if (client) {
            return "lazer";
        }
        if (state.externalBridgeAvailable && state.shellOsuNativeAlive === false) {
            return "native";
        }
        if (activeChart && (activeChart.columnCount < 4 || activeChart.columnCount > 18)) {
            return "keys";
        }
        return null;
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
        nodes?.stage?.classList.remove(
            "is-entering",
            "is-loading",
            "is-loading-song",
            "is-loading-difficulty",
        );
        activeChart = chart;
        lastColumnCount = chart.columnCount;
        const mode = activeScoreMode();
        accumulator = createAccumulator(chart, accumulatorOptions(accumulatorMode(), laneWeightsForMode(mode)));
        rawAccumulator = createAccumulator(chart, accumulatorOptions(rawScoreMode(), 300));
        // 正常 Stable：顶部 ScoreV2 必须从原始帧按 V2 重新判定，不能拿 V1 计数换权重。
        v2TopAccumulator = mode === "stable-native"
            ? createAccumulator(chart, accumulatorOptions("stable-native", 305))
            : null;
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
            if (activeScoreMode() === "stable-native") {
                // native 事件已经是游戏判定结果，直接应用，不按 songTime 截断。
                accumulator.applyBatch({ reset: true, events: hitEvents });
                hitEvents = [];
            }
        }
        pump();
        laneKey = "";
        laneButtons = [];
        clearError();
        render();
        if (laneAccWrapEl && laneAccWrapEl.offsetParent !== null) {
            replayEntrance();
        }
    }

    /** 分析完成：只暂存；若当前没有战报，则显示 100.00% 预览。 */
    function prepareChart(chart) {
        // Stable RD 没有可读种子；Lazer RD 有 seed 时在下面按列置换还原。
        if (randomModBlocked()) {
            pendingChart = null;
            activeChart = null;
            accumulator = null;
            hasResult = false;
            sessionActive = false;
            showError("random");
            return;
        }
        applyRandomColumns(chart);
        // 结算后重算可能暂时拿不到谱面原文/解析失败：保留已经冻结的终态，
        // 不能把用户刚看到的转换成绩换成错误占位。真正换图会先离开 resultScreen。
        if (hasResult && (!chart || chart.error)) {
            clearLoading();
            scheduleRender();
            return;
        }
        if (!isShellRuntime()) {
            pendingChart = null;
            activeChart = null;
            accumulator = null;
            showError("shell");
            return;
        }
        const err = evaluateError();
        if (err) {
            pendingChart = null;
            activeChart = null;
            accumulator = null;
            showError(err);
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
        if (chart.columnCount < 4 || chart.columnCount > 18) {
            pendingChart = null;
            activeChart = null;
            accumulator = null;
            showError("keys");
            return;
        }
        if (activeChart && !sameChart(activeChart, chart)) {
            // 结算后重算可能带一张新谱面对象：先保留本局结算显示，
            // 等下一次 startSession 再真正切图；否则用户刚看到的转换成绩会被清掉。
            if (hasResult) {
                pendingChart = chart;
                clearLoading();
                clearError();
                scheduleRender();
                return;
            }
            // 切图/换难度：旧事件的 note 索引只对旧谱面有效，必须先清空并重建
            // 累加器；否则未匹配的旧头/尾会按新谱面落成 Miss。
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
        } else if (accumulator) {
            accumulator.syncModifiers(options());
            rawAccumulator?.syncModifiers(options());
            v2TopAccumulator?.syncModifiers(options());
        }
        clearLoading();
        clearError();
        scheduleRender();
    }

    /** 进入 play：采纳 pending 谱面并清空本局。 */
    function startSession() {
        if (randomModBlocked()) {
            showError("random");
            return false;
        }
        const chart = pendingChart || activeChart;
        if (!chart) {
            return false;
        }
        pendingChart = chart;
        sessionActive = true;
        hasResult = false;
        sessionScoreMode = resolveScoreMode();
        frozenTopSnapshot = null;
        allEvents = false;
        hitEvents = [];
        replayFrames = [];
        feedIndex = 0;
        lastCutoff = -Infinity;
        createFor(chart);
        clearLoading();
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

    /** 壳 native 事件里每个 note 的第一个事件是头判；返回头 miss 的 note 集合。 */
    function nativeHeadMissNotes() {
        if (!Array.isArray(hitEvents) || hitEvents.length === 0) {
            return null;
        }
        if (!hitEvents.some((event) => event?.note != null)) {
            return null;
        }
        const seen = new Set();
        const misses = new Set();
        for (const event of hitEvents) {
            const note = event?.note;
            if (note == null || seen.has(note)) {
                continue;
            }
            seen.add(note);
            if (Number(event.result) === 1) {
                misses.add(note);
            }
        }
        return misses;
    }

    /** 按时钟把 hitEvents 逐条喂给 accumulator；时钟回跳时重建。 */
    function pump() {
        if (randomModBlocked()) {
            return;
        }
        if (!accumulator || !activeChart) {
            return;
        }
        const cutoff = currentCutoff();
        if (cutoff + 1 < lastCutoff) {
            const mode = activeScoreMode();
            accumulator = createAccumulator(activeChart, accumulatorOptions(accumulatorMode(), laneWeightsForMode(mode)));
            rawAccumulator = createAccumulator(activeChart, accumulatorOptions(rawScoreMode(), 300));
            v2TopAccumulator = mode === "stable-native"
                ? createAccumulator(activeChart, accumulatorOptions("stable-native", 305))
                : null;
            feedIndex = 0;
            lastCutoff = -Infinity;
            lastRawSolveAt = 0;
        }
        if (activeScoreMode() === "stable-sim") {
            // Stable V2 分轨/左右手：从原始帧自己算每 note 头/尾事件，再交给 native 计数。
            if (replayFrames.length === 0) {
                accumulator.reset();
                lastCutoff = cutoff;
                scheduleRender();
                return;
            }
            const events = framesToStableV2Events(activeChart, replayFrames, {
                ...options(),
                cutoff,
                headMissNotes: nativeHeadMissNotes(),
            });
            accumulator.applyBatch({ reset: true, events });
            lastCutoff = cutoff;
            rebuildRawAccumulator(true, cutoff);
            scheduleRender();
            return;
        }
        while (feedIndex < hitEvents.length) {
            const time = accumulator.eventTime(hitEvents[feedIndex]);
            if (Number.isFinite(time) && time > cutoff) {
                break;
            }
            accumulator.applyEvent(hitEvents[feedIndex]);
            feedIndex += 1;
        }
        if (v2TopAccumulator) {
            if (replayFrames.length === 0) {
                v2TopAccumulator.reset();
            } else {
                // 顶部 V1→V2 折算按 Lazer 式段归属重判（分轨仍是 Stable 规则）。
                const events = framesToStableV2Events(activeChart, replayFrames, { ...options(), cutoff, judgementMode: "lazer" });
                v2TopAccumulator.applyBatch({ reset: true, events });
            }
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
        // rawAccumulator 永远是 V1 窗口；不能用 native/V2 accumulator 的窗口。
        const windows = rawAccumulator?.snapshot()?.windows;
        if (!windows) {
            return;
        }
        const final = cutoff === Infinity;
        const visible = final
            ? replayFrames
            : replayFrames.filter((frame) => frame.time <= cutoff + 250);
        if (visible.length === 0) {
            rawAccumulator = createAccumulator(activeChart, accumulatorOptions(rawScoreMode()));
            return;
        }
        const currentOptions = options();
        let events = framesToSv1Events(
            activeChart,
            visible,
            windows,
            currentOptions.mods,
            currentOptions.speedRate,
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
                currentOptions.mods,
                currentOptions.speedRate,
                accumulator?.noRegrabDropNotes?.() ?? null,
                cutoff,
            );
            rawAccumulator.applyBatch({ reset: true, events });
        }
    }

    function applyReplayFrames(payload) {
        // 结算后壳 detach 会发空 reset；保留终态，等下一局 startSession 再清。
        const emptyReset = payload?.reset && !(Array.isArray(payload.frames) && payload.frames.length > 0);
        if (emptyReset && (hasResult || sessionActive)) {
            return;
        }
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
        if (randomModBlocked()) {
            return;
        }
        const batch = Array.isArray(payload) ? { events: payload } : (payload || {});
        // 结算后壳 detach 会发空 reset；保留终态，等下一局 startSession 再清。
        const emptyReset = batch.reset && !(Array.isArray(batch.events) && batch.events.length > 0);
        if (emptyReset && (hasResult || sessionActive)) {
            return;
        }
        // Stable V2 的每 note 事件由原始帧自己算；但壳的 native 头判仍要保留，
        // 用来覆盖 V2 LN 预选把按下沿路由给前一条 LN 的头 miss（分轨才精确）。
        if (activeScoreMode() === "stable-sim") {
            if (batch.reset) {
                hitEvents = [];
            }
            if (Array.isArray(batch.events)) {
                hitEvents.push(...maybeMirrorEvents(batch.events));
            }
            if (accumulator) {
                pump();
            } else {
                scheduleRender();
            }
            return;
        }
        const native = activeScoreMode() === "stable-native";
        if (native && accumulator) {
            // Stable per-note 判定已是游戏最终结果：直接应用，不做时钟截断。
            accumulator.applyBatch({ ...batch, events: maybeMirrorEvents(batch.events) });
            scheduleRender();
            return;
        }
        if (batch.reset) {
            hitEvents = [];
            feedIndex = 0;
            lastCutoff = -Infinity;
            allEvents = false;
            if (accumulator) {
                accumulator.reset();
            }
        }
        if (Array.isArray(batch.events)) {
            hitEvents.push(...maybeMirrorEvents(batch.events));
        }
        if (!accumulator) {
            // 全量快照按 2048 事件分块到达；谱面还没准备好时必须全部排队，
            // 否则后一块会覆盖前一块，页面只统计到最后一块（真机少 20–30%）。
            if (batch.reset) {
                pendingBatches = [];
            }
            pendingBatches.push({ ...batch, events: maybeMirrorEvents(batch.events) });
            if (!startSession()) {
                return;
            }
            return;
        }
        accumulator.syncModifiers(options());
        pump();
    }

    /**
     * 结算：只冻结显示，分母仍是已判定计数。
     *
     * `complete=true`（真正进入结算屏）才把剩余原始帧/未判定 note 纳入
     * SV1 结算；中途退到选歌必须 `complete=false`，否则当前时钟之后的
     * 未游玩 note 会被 framesToSv1Events 当成“已到期”全部记 Miss。
     */
    function finishSession(complete = true) {
        if (!accumulator) {
            return;
        }
        // 结算补齐全谱面会把 live 未收到的尾段记成 miss；正常 Stable 的
        // ScoreV2 转换保留 live 终值，避免结算瞬间无故下降。
        if (complete && v2TopAccumulator) {
            frozenTopSnapshot = v2TopAccumulator.snapshot();
        }
        allEvents = complete;
        pump();
        rawAccumulator?.finish();
        v2TopAccumulator?.finish();
        accumulator.finish();
        sessionActive = false;
        hasResult = true;
        scheduleRender();
    }

    function setSpaceMode(mode) {
        spaceModeSetting = mode;
        if (accumulator) {
            accumulator.setSpaceMode(mode);
            rawAccumulator?.setSpaceMode(mode);
            v2TopAccumulator?.setSpaceMode(mode);
            laneKey = "";
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
        hasResult = false;
        sessionScoreMode = null;
        frozenTopSnapshot = null;
        pendingChart = null;
        activeChart = null;
        accumulator = null;
        rawAccumulator = null;
        v2TopAccumulator = null;
        replayFrames = [];
        hitEvents = [];
        feedIndex = 0;
        lastCutoff = -Infinity;
        allEvents = false;
        lastRawSolveAt = 0;
        laneBounds = null;
        latestSnapshot = null;
        pendingBatches = [];
        popoverCtrl.hidePopover();
        popoverCtrl.hideStatPopover();
        const nodes = dom();
        if (nodes) {
            // 切图只清内部数据，不清显示值：旧 ScoreV1、左右手和轨道保持可见，
            // 由 loading 动画负责过渡；否则会先变成空白再突然弹入。
            laneButtons = [];
            laneKey = "";
            showLoading(state.pendingChangeKind === "song" ? "song" : "difficulty");
        }
    }

    function showLoadingFromState() {
        const nodes = dom();
        if (!nodes) {
            return;
        }
        const pending = state.pendingChangeKind;
        const changeKind = pending === "song"
            ? "song"
            : (pending === "difficulty" || pending === "mod")
                ? "difficulty"
                : loadingChangeKind;
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
        isSessionActive: () => sessionActive || hasResult,
        setPending: showLoading,
    };
})();
