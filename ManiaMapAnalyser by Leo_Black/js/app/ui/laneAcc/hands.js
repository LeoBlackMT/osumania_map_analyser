// hands.js —— LaneAcc 左右手/空格键按键块的构建与渲染

import { formatAcc, formatMiss } from "../../../laneAcc/format.js";

/**
 * 创建单手/空格显示块
 * @param {string} label "L" | "M" | "R"
 * @returns {HTMLDivElement}
 */
export function createHandBlock(label) {
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

/**
 * 渲染手部数据到容器中
 * @param {HTMLElement} handsContainer
 * @param {object} snapshot
 * @param {(el: HTMLElement, target: number, formatter: (n: number) => string) => void} animateNumber
 */
export function renderHands(handsContainer, snapshot, animateNumber) {
    if (!handsContainer || !snapshot?.hands) {
        return;
    }
    const specs = [["L", snapshot.hands.left], ["R", snapshot.hands.right]];
    if (snapshot.hands.space) {
        specs.splice(1, 0, ["M", snapshot.hands.space]);
    }
    handsContainer.dataset.count = String(specs.length);
    const wanted = new Set(specs.map(([label]) => label));
    for (const block of [...handsContainer.querySelectorAll(".laneacc-hand")]) {
        if (!wanted.has(block.dataset.handLabel)) {
            block.remove();
        }
    }
    for (const [label, hand] of specs) {
        let block = handsContainer.querySelector(`.laneacc-hand[data-hand-label="${label}"]`);
        if (!block) {
            block = createHandBlock(label);
            handsContainer.append(block);
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
