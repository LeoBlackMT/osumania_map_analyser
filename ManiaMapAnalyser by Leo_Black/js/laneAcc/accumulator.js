// accumulator.js —— LaneAcc 的纯逻辑核心（DOM-free）。
//
// 双口径：
//   game = Lazer 305 权重（分轨/左右手；LN 头尾各算一个可判定事件）
//   sv1  = Stable Classic 窗口 + LN 头尾合并（顶部 ScoreV1 转换 Acc）
//
// SV1 判定以 osu!stable 的 mania V1 规则为准（stable-re notes/input-routing.md）：
//   - tap：按下当帧按 |Δ| 定档；晚于 100 窗由到期闸先行判 miss。
//   - LN：头按下不计数；尾释放时按 head/tail 合并公式给出唯一判定；
//         尾早于 tail−50 窗 = 掉落（不计桶、断连），逾 tail+100 窗仍在按 = 以 now 结算。
//   - tick：按住期间每 100 ms 一个判定 7，只加连击、不进判定桶。
//
// 失败局分母：`gameTotal` / `sv1Total` 恒为**已消费/已判定**的计数；`finish()` 只冻结显示，
// 不把分母改成谱面总音符数（完整打完时二者自然相等）。

import { handGroups } from "./chart.js";

export const SPACE_MODES = ["independent", "left", "right"];

const MISS = 1;
const IGNORE_MISS = 13;
const IGNORE_HIT = 14;
const COMBO_BREAK = 15;
const FORCE_DROP = 50;
const LAZER_WEIGHTS = [0, 50, 100, 200, 300, 305];
const SV1_WEIGHTS = [300, 300, 200, 100, 50, 0];

export function normalizeSpaceMode(value) {
    return SPACE_MODES.includes(value) ? value : "independent";
}

function clamp(value, min, max) {
    return Math.min(max, Math.max(min, value));
}

function adjustedOd(baseOd, odFlag) {
    // stable mania 的 OD 只参与窗口计算；HR/EZ 不改 OD，只改窗口倍率。
    const base = clamp(Number(baseOd) || 0, -15, 15);
    if (odFlag !== null && odFlag !== undefined && odFlag !== "" && Number.isFinite(Number(odFlag))) {
        return clamp(Number(odFlag), -15, 15);
    }
    return base;
}

function difficultyRange(value, min, mid, max) {
    if (value <= 5) {
        return min + (mid - min) * (value / 5);
    }
    return mid + (max - mid) * ((value - 5) / 5);
}

function inverseDifficultyRange(value, min, mid, max) {
    if (Math.sign(value - mid) === Math.sign(max - mid)) {
        return (value - mid) / (max - mid) * 5 + 5;
    }
    return (value - mid) / (mid - min) * 5 + 5;
}

/**
 * Lazer Mania song-select equivalent OD (ManiaRuleset.GetAdjustedDisplayDifficulty).
 * 只影响卡片上的 OD 胶囊显示，不参与 SV1 判定。
 */
function displayEquivalentOd(od, mods, odResolved) {
    if (odResolved) {
        return od;
    }
    let perfect = difficultyRange(od, 22.4, 19.4, 13.9);
    if (mods.includes("HR")) {
        perfect /= 1.4;
    } else if (mods.includes("EZ")) {
        perfect /= 1 / 1.4;
    }
    return inverseDifficultyRange(perfect, 22.4, 19.4, 13.9);
}

/**
 * osu!stable mania V1 判定窗（`--zy_Ow…:317-325` + `iDWAt1kTWArb`）。
 * 注意：窗口经 HR(/1.4) / EZ(×1.4) / DT(×1.5) / HT(×0.75) 后 **整数截断**，
 * 不是 Lazer Classic 的 floor+0.5。
 */
export function computeWindows(baseOd, speedRate = 1, mods = [], odFlag = null, odResolved = false) {
    const od = adjustedOd(baseOd, odFlag);
    const rate = Number.isFinite(Number(speedRate)) && Number(speedRate) > 0 ? Number(speedRate) : 1;
    // odResolved（桥源已把 HR/EZ 判定折算进 OD）：不再额外乘 HR/EZ 的窗口倍率。
    const difficultyMultiplier = odResolved
        ? 1
        : mods.includes("HR")
            ? 1.4
            : mods.includes("EZ")
                ? 1 / 1.4
                : 1;
    const totalMultiplier = rate / difficultyMultiplier;
    const inv = 10 - clamp(od, 0, 10);
    const scale = (value) => Math.trunc(value * totalMultiplier);
    return {
        od,
        displayOd: displayEquivalentOd(od, mods, odResolved),
        totalMultiplier,
        perfect: scale(16),
        great: scale(34 + 3 * inv),
        good: scale(67 + 3 * inv),
        ok: scale(97 + 3 * inv),
        meh: scale(121 + 3 * inv),
        miss: scale(158 + 3 * inv),
    };
}

function judgeStableTap(offset, windows) {
    const raw = Number(offset) || 0;
    // 晚于 100 窗的按下：stable 的到期闸先于按键执行，直接 Miss（50 档只在早按侧存在）。
    if (raw >= windows.ok) {
        return 6;
    }
    const error = Math.abs(Math.trunc(raw));
    if (error <= windows.perfect) return 1;
    if (error <= windows.great) return 2;
    if (error <= windows.good) return 3;
    if (error <= windows.ok) return 4;
    if (error <= windows.meh) return 5;
    return 6;
}

/**
 * osu!stable mania V1 LN 合并判定（`--zj_xBP…:262-301`）。
 * @param {number} headOffset 按下时刻相对 LN 头的毫秒偏移
 * @param {number} releaseOffset 释放时刻相对 LN 尾的毫秒偏移；NaN = 从未释放
 * @param {boolean} dropped 是否发生过掉落（早于 tail−50 窗松手 / body break）
 */
function judgeStableHold(note, headOffset, releaseOffset, dropped, windows, chart) {
    const start = Number(chart.noteStarts[note]);
    const end = Number(chart.noteEnds[note]);
    const pressTime = start + Math.trunc(headOffset);
    let num2;
    if (pressTime < start - windows.meh) {
        // 头按得太深早：头参照钉到「尾−1」
        num2 = end - 1;
    } else if (pressTime >= start) {
        num2 = pressTime;
    } else {
        // 浅早按：镜像到晚侧，|Δ| 不变
        num2 = start + (start - pressTime);
    }

    let num3;
    if (!Number.isFinite(releaseOffset)) {
        // 没有释放事件（一直按住）：Judge 在 now < tail + 50 窗 时返回 0，
        // 真正的结算点是 tail + 50 窗，而不是 100 窗。
        num3 = end + windows.meh;
    } else {
        const releaseTime = end + Math.trunc(releaseOffset);
        // 超过 tail + 50 窗仍未松手：到期帧已经以 now 结算，用 50 窗代替实际松手时间
        num3 = releaseTime > end + windows.meh ? end + windows.meh : releaseTime;
    }

    const num4 = Math.abs(num2 - start);
    const num5 = Math.abs(num3 - end);
    const num6 = num4 + num5;
    // stable 的到期结算：超过 miss 窗才算 Miss；50 窗与 miss 窗之间按掉落 50 处理。
    if (num3 < end - windows.miss) {
        return 6;
    }
    if (num3 < end - windows.meh) {
        return 5;
    }
    let judgement;
    if (num4 <= windows.perfect * 1.2 && num6 <= windows.perfect * 2.4) {
        judgement = 1;
    } else if (num4 <= windows.great * 1.1 && num6 <= windows.great * 2.2) {
        judgement = 2;
    } else if (num4 <= windows.good - 2 && num6 <= windows.good * 2) {
        judgement = 3;
    } else if (num4 <= windows.ok && num6 <= windows.ok * 2) {
        judgement = 4;
    } else {
        judgement = 5;
    }
    // 赎回线：掉落只把 300g/300 压到 200，不把 200 继续往下压。
    if (dropped && judgement < 3) {
        judgement = 3;
    }
    return judgement;
}

function sumPoints(counts, weights, offset = 0) {
    let points = 0;
    for (let index = 0; index < 6; index += 1) {
        points += (counts[offset + index] || 0) * weights[index];
    }
    return points;
}

function accuracy(points, judged, denominatorWeight) {
    return judged > 0 ? (points / (denominatorWeight * judged)) * 100 : null;
}

function modifierSignature(options = {}) {
    const rate = Number(options.speedRate);
    const mods = Array.isArray(options.mods) ? options.mods.join(",") : "";
    const od = options.odFlag == null || options.odFlag === "" ? "" : options.odFlag;
    return `${Number.isFinite(rate) && rate > 0 ? rate : 1}|${mods}|${od}|${options.odResolved ? 1 : 0}`;
}

/**
 * @param {object} chart `buildChart()` 的返回值
 * @param {{speedRate?: number, mods?: string[], odFlag?: string|number|null, spaceMode?: string}} options
 */
export function createAccumulator(chart, options = {}) {
    if (!chart || chart.error) {
        throw new Error("laneacc: chart is not ready");
    }
    const columnCount = chart.columnCount;
    const noteCount = chart.noteCount;
    const gameCounts = new Int32Array(columnCount * 6);
    const sv1Counts = new Int32Array(6);
    const final = new Uint8Array(noteCount);
    const broken = new Uint8Array(noteCount);
    const headOffset = new Float64Array(noteCount);
    const tailOffset = new Float64Array(noteCount);
    const dropOffset = new Float64Array(noteCount);
    const forceGrade = new Uint8Array(noteCount);
    const regrabbed = new Uint8Array(noteCount);
    const sv1Grade = new Int8Array(noteCount);
    const nextPress = new Float64Array(columnCount);
    const headResult = new Uint8Array(noteCount);
    const gameTail = new Uint8Array(noteCount);
    const pending = new Int32Array(columnCount);
    const cursor = new Int32Array(columnCount);
    const laneCombo = new Int32Array(columnCount);
    const laneMaxCombo = new Int32Array(columnCount);
    const handCombo = new Int32Array(2);
    const handMaxCombo = new Int32Array(2);
    const colHand = new Int8Array(columnCount);
    const lastTickTime = new Float64Array(columnCount);
    let combo = 0;
    let maxCombo = 0;
    let sv1Combo = 0;
    let sv1MaxCombo = 0;
    let gameTotal = 0;
    let sv1Total = 0;
    let unmatched = 0;
    let finished = false;
    let windows = computeWindows(chart.od, options.speedRate, options.mods, options.odFlag, options.odResolved);
    let modifierKey = modifierSignature(options);
    let spaceMode = normalizeSpaceMode(options.spaceMode);
    rebuildHandMap();

    function rebuildHandMap() {
        const groups = handGroups(columnCount, spaceMode);
        colHand.fill(-1);
        for (let col = groups.left[0]; col < groups.left[1]; col += 1) colHand[col] = 0;
        for (let col = groups.right[0]; col < groups.right[1]; col += 1) colHand[col] = 1;
    }

    function registerJudgement(col, success) {
        if (!success) {
            combo = 0;
            laneCombo[col] = 0;
            const hand = colHand[col];
            if (hand >= 0) handCombo[hand] = 0;
            return;
        }
        combo += 1;
        laneCombo[col] += 1;
        if (combo > maxCombo) maxCombo = combo;
        if (laneCombo[col] > laneMaxCombo[col]) laneMaxCombo[col] = laneCombo[col];
        const hand = colHand[col];
        if (hand >= 0) {
            handCombo[hand] += 1;
            if (handCombo[hand] > handMaxCombo[hand]) handMaxCombo[hand] = handCombo[hand];
        }
    }

    function registerSv1Combo(success) {
        if (!success) {
            sv1Combo = 0;
            return;
        }
        sv1Combo += 1;
        if (sv1Combo > sv1MaxCombo) sv1MaxCombo = sv1Combo;
    }

    function clearNotes() {
        final.fill(0);
        broken.fill(0);
        headOffset.fill(NaN);
        tailOffset.fill(NaN);
        dropOffset.fill(NaN);
        forceGrade.fill(0);
        regrabbed.fill(0);
        sv1Grade.fill(0);
        nextPress.fill(NaN);
        headResult.fill(0);
        gameTail.fill(0);
        pending.fill(-1);
        cursor.fill(0);
        laneCombo.fill(0);
        laneMaxCombo.fill(0);
        handCombo.fill(0);
        handMaxCombo.fill(0);
        lastTickTime.fill(-1);
        combo = 0;
        maxCombo = 0;
        sv1Combo = 0;
        sv1MaxCombo = 0;
    }

    function reset() {
        gameCounts.fill(0);
        sv1Counts.fill(0);
        gameTotal = 0;
        sv1Total = 0;
        unmatched = 0;
        finished = false;
        clearNotes();
    }

    function nextSlot(col) {
        const start = chart.slotStart[col];
        const count = chart.slotCount[col];
        const index = cursor[col];
        if (index >= count) {
            return -1;
        }
        cursor[col] = index + 1;
        return start + index;
    }

    /** 按住 LN 时按 stable 的 100 ms 锚点补 tick（只加连击，不进判定桶）。 */
    function advanceTicks(currentTime) {
        for (let col = 0; col < columnCount; col += 1) {
            const note = pending[col];
            if (note >= 0 && broken[note] === 0) {
                const end = chart.noteEnds[note];
                const targetTime = Math.min(end, currentTime);
                let lt = lastTickTime[col];
                if (lt >= 0) {
                    while (lt + 100 <= targetTime) {
                        lt += 100;
                        registerSv1Combo(true);
                    }
                    lastTickTime[col] = lt;
                }
            }
        }
    }

    function finalize(note) {
        if (final[note]) {
            return;
        }
        final[note] = 1;
        let judgement = 6;
        if (forceGrade[note] > 0) {
            judgement = forceGrade[note];
        } else if (Number.isFinite(headOffset[note])) {
            judgement = judgeStableHold(note, headOffset[note], tailOffset[note], broken[note] === 1, windows, chart);
        }
        sv1Counts[judgement - 1] += 1;
        sv1Total += 1;
        sv1Grade[note] = judgement;
        registerSv1Combo(judgement !== 6);
    }

    /** 结算该列当前仍挂着、但已收到尾事件的 LN（缺 13/14 或新音符头到来时兜底）。 */
    function flushPending(col, ticksAt = null) {
        const note = pending[col];
        if (note < 0) {
            return;
        }
        if (ticksAt !== null && Number.isFinite(ticksAt)) {
            advanceTicks(ticksAt);
        } else {
            advanceTicks(chart.noteEnds[note]);
        }
        if (!final[note]) {
            finalize(note);
        }
        pending[col] = -1;
        lastTickTime[col] = -1;
    }

    /** 该事件对应的谱面时间（ms）；无法判断时返回 -Infinity（立即应用）。 */
    function eventTime(event) {
        const col = Number(event?.col);
        const result = Number(event?.result);
        const offset = Number(event?.offset);
        if (!Number.isInteger(col) || col < 0 || col >= columnCount || !Number.isFinite(offset)) {
            return -Infinity;
        }
        if (result >= 1 && result <= 6) {
            const index = cursor[col];
            if (index < 0 || index >= chart.slotCount[col]) {
                return -Infinity;
            }
            const slot = chart.slotStart[col] + index;
            const note = chart.slotNote[slot];
            const part = chart.slotPart[slot];
            return part === 1
                ? chart.noteEnds[note] + Math.trunc(offset)
                : chart.noteStarts[note] + Math.trunc(offset);
        }
        if (result === IGNORE_MISS || result === IGNORE_HIT || result === COMBO_BREAK || result === FORCE_DROP) {
            const note = pending[col];
            if (note < 0) {
                return -Infinity;
            }
            return chart.noteEnds[note] + Math.trunc(offset);
        }
        return -Infinity;
    }

    function applyEvent(event) {
        const col = Number(event?.col);
        const result = Number(event?.result);
        const offset = Number(event?.offset);
        if (!Number.isInteger(col) || col < 0 || col >= columnCount) {
            unmatched += 1;
            return;
        }
        if (!Number.isFinite(offset)) {
            unmatched += 1;
            return;
        }
        if (result >= 1 && result <= 6) {
            // 同一列出现新的音符头时，先把上一条只差 13/14 的 LN 结算掉。
            if (pending[col] >= 0) {
                const index = cursor[col];
                const sameTail = index < chart.slotCount[col]
                    && chart.slotNote[chart.slotStart[col] + index] === pending[col]
                    && chart.slotPart[chart.slotStart[col] + index] === 1;
                if (!sameTail) {
                    flushPending(col);
                }
            }
            const slot = nextSlot(col);
            if (slot < 0) {
                unmatched += 1;
                return;
            }
            const note = chart.slotNote[slot];
            const part = chart.slotPart[slot];
            let eventOffset = offset;
            if (part === 0 && Number.isFinite(nextPress[col])) {
                // 上一条 LN 的 13 offset 是下一次真实按键时刻；修正 Lazer 因 regrab 产生的 head 偏移。
                eventOffset = nextPress[col] - chart.noteStarts[note];
                nextPress[col] = NaN;
            }
            gameCounts[col * 6 + result - 1] += 1;
            gameTotal += 1;
            registerJudgement(col, result !== MISS);
            if (part === 1) {
                // 尾事件：只记录释放偏移；LN 的唯一判定等到 13/14（或下一事件）再发。
                gameTail[note] = 1;
                // Lazer 在从未松手时会发 result=1、offset=0；这不是 release，按 held 到期处理。
                tailOffset[note] = result === MISS && Math.abs(eventOffset) < 1e-9 ? NaN : eventOffset;
                const releaseTime = chart.noteEnds[note] + Math.trunc(eventOffset);
                advanceTicks(releaseTime);
                if (eventOffset < -windows.meh) {
                    broken[note] = 1;
                    registerSv1Combo(false);
                }
                return;
            }
            // Lazer 的 result=1, offset=0 表示 LN 头从未被按下；stable 会判整条 Miss（不伪造按下）。
            headOffset[note] = result === MISS && Math.abs(eventOffset) < 1e-9 ? NaN : eventOffset;
            headResult[note] = result;
            if (!chart.isHold[note]) {
                const judgement = result === MISS && Math.abs(offset) < 1e-9
                    ? 6
                    : judgeStableTap(eventOffset, windows);
                sv1Counts[judgement - 1] += 1;
                sv1Total += 1;
                sv1Grade[note] = judgement;
                registerSv1Combo(judgement !== 6);
                return;
            }
            // LN 头：不计数，只记录按下时刻与 tick 锚点。
            const noPress = result === MISS && Math.abs(offset) < 1e-9;
            pending[col] = note;
            lastTickTime[col] = noPress ? -1 : chart.noteStarts[note] + Math.trunc(eventOffset);
            if (lastTickTime[col] >= 0) {
                advanceTicks(chart.noteStarts[note] + Math.trunc(eventOffset));
            }
            return;
        }
        if (result === FORCE_DROP) {
            // rawFrames 合成事件：Stable 把「按下过、但早于尾窗松手且没有 regrab」的 LN
            // 记 50；头偏移明显坏掉（>30ms）的记 100。只用于原始帧 SV1，不进 gameCounts。
            if (pending[col] >= 0 && !final[pending[col]]) {
                const note = pending[col];
                tailOffset[note] = offset;
                forceGrade[note] = Math.abs(headOffset[note]) > 30 ? 4 : 5;
                finalize(note);
                pending[col] = -1;
                lastTickTime[col] = -1;
            }
            return;
        }
        if (result === COMBO_BREAK) {
            // 断持伪判定：不计桶，只断连；记在正在处理的 LN 上。
            registerJudgement(col, false);
            if (pending[col] >= 0) {
                const note = pending[col];
                const breakTime = chart.noteEnds[note] + Math.trunc(offset);
                advanceTicks(breakTime);
                dropOffset[note] = offset;
                broken[note] = 1;
                registerSv1Combo(false);
            }
            return;
        }
        if (result === IGNORE_MISS || result === IGNORE_HIT) {
            if (result === IGNORE_MISS) {
                registerJudgement(col, false);
            }
            if (pending[col] >= 0 && !final[pending[col]]) {
                const note = pending[col];
                const drop = dropOffset[note];
                if (result === IGNORE_HIT) {
                    // 14：掉落后的重抓/最终释放时间；没有 15 时只补缺失的尾偏移。
                    regrabbed[note] = 1;
                    if (Number.isFinite(drop) && offset >= drop) {
                        tailOffset[note] = offset;
                        broken[note] = 1;
                    } else if (gameTail[note] === 0 && !Number.isFinite(tailOffset[note])) {
                        tailOffset[note] = offset;
                    }
                } else if (Number.isFinite(drop)) {
                    // 13：掉落结算。13 的 offset 晚于 15 时，它是下一次真实按键时刻（regrab/下一音符），
                    // 当前 LN 的释放仍是 15 的掉落时刻。
                    tailOffset[note] = drop;
                    if (offset > drop + 1) {
                        nextPress[col] = chart.noteEnds[note] + offset;
                    }
                } else if (gameTail[note] === 0 && Number.isFinite(offset)) {
                    tailOffset[note] = offset;
                }
                const releaseTime = chart.noteEnds[note]
                    + Math.trunc(Number.isFinite(tailOffset[note]) ? tailOffset[note] : offset);
                advanceTicks(releaseTime);
                finalize(note);
                pending[col] = -1;
                lastTickTime[col] = -1;
            }
            return;
        }
    }

    function applyBatch(batch) {
        if (batch?.reset) {
            reset();
        }
        const events = Array.isArray(batch?.events) ? batch.events : [];
        for (const event of events) {
            applyEvent(event);
        }
        return events.length;
    }

    /** 只冻结显示；分母保持已判定计数。 */
    function finish() {
        for (let col = 0; col < columnCount; col += 1) {
            flushPending(col);
        }
        finished = true;
    }

    function sv1Grades() {
        return sv1Grade;
    }

    /** hitEvents 口径：有 15 掉落、没有 14 regrab 的 LN note 列表。 */
    function noRegrabDropNotes() {
        const notes = [];
        for (let note = 0; note < noteCount; note += 1) {
            if (Number.isFinite(dropOffset[note]) && !regrabbed[note] && Number.isFinite(headOffset[note]) && sv1Grade[note] === 6) {
                notes.push(note);
            }
        }
        return notes;
    }

    function laneSnapshot(col) {
        const counts = Array.from(gameCounts.slice(col * 6, col * 6 + 6));
        const judged = counts.reduce((sum, value) => sum + value, 0);
        const points = sumPoints(counts, LAZER_WEIGHTS);
        return {
            col,
            counts,
            judged,
            miss: counts[0],
            points,
            acc: accuracy(points, judged, 305),
            maxCombo: laneMaxCombo[col],
        };
    }

    function handSnapshot(range, handIndex = -1, laneCache = null) {
        if (!range) {
            return null;
        }
        let judged = 0;
        let points = 0;
        let miss = 0;
        const counts = [0, 0, 0, 0, 0, 0];
        for (let col = range[0]; col < range[1]; col += 1) {
            const lane = laneCache ? laneCache[col] : laneSnapshot(col);
            judged += lane.judged;
            points += lane.points;
            miss += lane.miss;
            for (let index = 0; index < 6; index += 1) counts[index] += lane.counts[index];
        }
        return {
            judged,
            points,
            miss,
            acc: accuracy(points, judged, 305),
            counts,
            maxCombo: handIndex >= 0 ? handMaxCombo[handIndex] : 0,
        };
    }

    function snapshot() {
        const groups = handGroups(columnCount, spaceMode);
        const lanes = [];
        for (let col = 0; col < columnCount; col += 1) {
            lanes.push(laneSnapshot(col));
        }
        let gamePoints = 0;
        for (let col = 0; col < columnCount; col += 1) {
            gamePoints += sumPoints(gameCounts, LAZER_WEIGHTS, col * 6);
        }
        const sv1Points = sumPoints(sv1Counts, SV1_WEIGHTS);
        const totalCounts = [0, 0, 0, 0, 0, 0];
        for (let col = 0; col < columnCount; col += 1) {
            for (let index = 0; index < 6; index += 1) {
                totalCounts[index] += gameCounts[col * 6 + index];
            }
        }
        return {
            finished,
            spaceMode,
            columnCount,
            lanes,
            hands: {
                left: handSnapshot(groups.left, 0, lanes),
                right: handSnapshot(groups.right, 1, lanes),
                space: groups.space == null ? null : handSnapshot([groups.space, groups.space + 1], -1, lanes),
            },
            gameTotal,
            gamePoints,
            gameAcc: accuracy(gamePoints, gameTotal, 305) ?? 100,
            gameMiss: lanes.reduce((sum, lane) => sum + lane.miss, 0),
            counts: totalCounts,
            maxCombo: sv1MaxCombo,
            sv1Total,
            sv1Points,
            sv1Acc: accuracy(sv1Points, sv1Total, 300) ?? 100,
            sv1Counts: Array.from(sv1Counts),
            unmatched,
            windows,
        };
    }

    function setModifiers(next = {}) {
        windows = computeWindows(chart.od, next.speedRate, next.mods, next.odFlag, next.odResolved);
        modifierKey = modifierSignature(next);
        reset();
    }

    /**
     * 局内同步倍速/OD（自定义 DT 速率、DA OD、HR/EZ）。
     * 只在**尚未判定任何音符**时改窗口：一局开始的头几帧补齐 Mod 状态；
     * 一旦有判定就不再改，避免同一局前后用不同窗口（历史事件无法重判）。
     */
    function syncModifiers(next = {}) {
        const key = modifierSignature(next);
        if (key === modifierKey || gameTotal > 0 || sv1Total > 0) {
            return false;
        }
        modifierKey = key;
        windows = computeWindows(chart.od, next.speedRate, next.mods, next.odFlag, next.odResolved);
        reset();
        return true;
    }

    function setSpaceMode(value) {
        spaceMode = normalizeSpaceMode(value);
        rebuildHandMap();
    }

    reset();

    return { reset, applyEvent, applyBatch, eventTime, finish, snapshot, sv1Grades, noRegrabDropNotes, setModifiers, syncModifiers, setSpaceMode };
}
