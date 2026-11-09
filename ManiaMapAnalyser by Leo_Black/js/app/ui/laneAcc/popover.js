// popover.js —— LaneAcc 弹窗交互控制器（分轨详情 Tooltip + 左右手/SV1 判定详情 Popover）

import { formatAcc } from "../../../laneAcc/format.js";

/**
 * 创建并管理 LaneAcc 的 Popover 弹窗交互。
 * @param {object} options
 * @param {() => object|null} options.getDom
 * @param {() => object|null} options.getSnapshot
 * @param {() => object|null} options.getActiveChart
 * @returns {object}
 */
export function createPopoverController({ getDom, getSnapshot, getActiveChart }) {
    let statPopover = null;
    let statKind = null;
    let popoverCol = -1;
    let popoverTimer = 0;

    /**
     * MAX : 300 的比值。两端点单独处理，避免 0/0 的 NaN 与 x/0 的 Infinity：
     * - 0:0 显示 0.00；
     * - MAX>0 且 300=0 显示 ∞；
     * - 其余保留两位小数。
     */
    function maxTo300Ratio(maxCount, count300) {
        const max = Math.max(0, Number(maxCount) || 0);
        const n300 = Math.max(0, Number(count300) || 0);
        if (n300 === 0) {
            return max === 0 ? "0.00" : "∞";
        }
        return (max / n300).toFixed(2);
    }

    function appendComboAndRatio(parent, combo, maxCount, count300) {
        const comboLine = document.createElement("div");
        comboLine.className = "laneacc-popover-combo";
        comboLine.textContent = `Max Combo ${combo ?? 0}`;
        const ratioLine = document.createElement("div");
        ratioLine.className = "laneacc-popover-ratio";
        ratioLine.textContent = `Ratio ${maxTo300Ratio(maxCount, count300)}`;
        parent.append(comboLine, ratioLine);
    }

    function ensureStatPopover() {
        const nodes = getDom();
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

    function showStatPopover(anchor, kind) {
        const nodes = getDom();
        const snapshot = getSnapshot();
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
            ? [...snapshot.sv1Counts]
            : [...hand.counts].reverse();
        const combo = kind === "sv1" ? snapshot.maxCombo : hand.maxCombo;
        const title = document.createElement("div");
        title.className = "laneacc-popover-title";
        title.textContent = kind === "sv1"
            ? (snapshot.sv1Label || "ScoreV1")
            : kind === "left" ? "Left Hand" : "Right Hand";
        const grid = document.createElement("div");
        grid.className = "laneacc-popover-grid";
        for (let index = 0; index < labels.length; index += 1) {
            const cell = document.createElement("span");
            cell.className = "laneacc-popover-cell";
            cell.textContent = `${labels[index]} ${values[index] ?? 0}`;
            grid.append(cell);
        }
        popover.replaceChildren(title, grid);
        appendComboAndRatio(popover, combo, values[0], values[1]);
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

    function refreshStatPopover() {
        if (!statKind || !statPopover || statPopover.hidden) {
            return;
        }
        const nodes = getDom();
        if (!nodes) {
            return;
        }
        const anchor = statKind === "sv1"
            ? nodes.wrap.querySelector(".laneacc-sv1")
            : nodes.hands?.querySelector(`.laneacc-hand[data-hand="${statKind}"]`);
        if (anchor) {
            showStatPopover(anchor, statKind);
        }
    }

    function showPopover(col, lane) {
        const nodes = getDom();
        const activeChart = getActiveChart();
        if (!nodes?.popover || !lane || !activeChart) {
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
        nodes.popover.append(title, grid);
        appendComboAndRatio(nodes.popover, lane.maxCombo, values[0], values[1]);

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

    function refreshPopover() {
        const nodes = getDom();
        const popover = nodes?.popover;
        if (popoverCol < 0 || !popover || popover.hidden) {
            return;
        }
        const lane = getSnapshot()?.lanes?.[popoverCol];
        const anchor = nodes.stage?.querySelector(`.laneacc-lane[data-col="${popoverCol}"]`);
        if (anchor && lane) {
            showPopover(popoverCol, lane);
        }
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
        const nodes = getDom();
        popoverCol = -1;
        if (popoverTimer) {
            clearTimeout(popoverTimer);
            popoverTimer = 0;
        }
        if (nodes?.popover) {
            nodes.popover.hidden = true;
        }
    }

    function isPopoverOpen(col) {
        const nodes = getDom();
        return popoverCol === col && Boolean(nodes?.popover && !nodes.popover.hidden);
    }

    return {
        showStatPopover,
        hideStatPopover,
        refreshStatPopover,
        showPopover,
        refreshPopover,
        hidePopover,
        schedulePopoverHide,
        isPopoverOpen,
    };
}
