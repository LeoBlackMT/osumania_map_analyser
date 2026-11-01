// chart.js —— 谱面 → LaneAcc 的 typed-array 模型（DOM-free，浏览器/Node 共用）。
//
// 每个 tap 产生 1 个 slot；每个 LN 产生 2 个 slot（head=0 / tail=1）。
// 同列 slot 按时间升序；事件按列内 FIFO 消费（同列音符在时间轴上不重叠）。
// 10 万音符 + 2 万 LN 约 120k slot / 1.6MB typed arrays，不用对象数组。

export const MIN_KEYS = 4;
export const MAX_KEYS = 18;

function asIntArray(values, length) {
    const out = new Int32Array(length);
    for (let i = 0; i < length; i += 1) {
        const value = Number(values?.[i]);
        out[i] = Number.isFinite(value) ? value : 0;
    }
    return out;
}

/**
 * @param {object} parsed `OsuFileParser.getParsedData()` 的返回值
 * @param {string} identity 谱面身份（仅用于展示/调试）
 * @returns {object} chart 或 `{ error: "keys" | "mode" | "empty" }`
 */
export function buildChart(parsed, identity = "") {
    const columnCount = Number(parsed?.columnCount);
    if (!Number.isInteger(columnCount) || columnCount < MIN_KEYS || columnCount > MAX_KEYS) {
        return { error: "keys" };
    }
    if (String(parsed?.gameMode) !== "3") {
        return { error: "mode" };
    }
    const noteCount = Math.min(
        parsed?.noteStarts?.length || 0,
        parsed?.columns?.length || 0,
        parsed?.noteEnds?.length || 0,
    );
    if (noteCount === 0) {
        return { error: "empty" };
    }

    const starts = asIntArray(parsed.noteStarts, noteCount);
    const ends = asIntArray(parsed.noteEnds, noteCount);
    const sourceColumns = parsed.columns;
    const sourceTypes = parsed.noteTypes;
    const columns = new Uint8Array(noteCount);
    const isHold = new Uint8Array(noteCount);
    const od = Number.isFinite(Number(parsed.od)) ? Number(parsed.od) : 8;

    const slotsByColumn = Array.from({ length: columnCount }, () => []);
    for (let note = 0; note < noteCount; note += 1) {
        const col = Number(sourceColumns?.[note]);
        if (!Number.isInteger(col) || col < 0 || col >= columnCount) {
            return { error: "empty" };
        }
        columns[note] = col;
        const type = Number(sourceTypes?.[note]) || 0;
        const hold = (type & 128) !== 0;
        isHold[note] = hold ? 1 : 0;
        slotsByColumn[col].push({ note, part: 0, time: starts[note] });
        if (hold) {
            slotsByColumn[col].push({ note, part: 1, time: ends[note] });
        }
    }

    let totalSlots = 0;
    for (const slots of slotsByColumn) {
        slots.sort((a, b) => a.time - b.time);
        totalSlots += slots.length;
    }
    const slotCol = new Uint8Array(totalSlots);
    const slotNote = new Int32Array(totalSlots);
    const slotPart = new Uint8Array(totalSlots);
    const slotTime = new Float64Array(totalSlots);
    const slotStart = new Int32Array(columnCount + 1);
    const slotCount = new Int32Array(columnCount);

    let cursor = 0;
    for (let col = 0; col < columnCount; col += 1) {
        slotStart[col] = cursor;
        const slots = slotsByColumn[col];
        slotCount[col] = slots.length;
        for (const slot of slots) {
            slotCol[cursor] = col;
            slotNote[cursor] = slot.note;
            slotPart[cursor] = slot.part;
            slotTime[cursor] = slot.time;
            cursor += 1;
        }
    }
    slotStart[columnCount] = cursor;

    return {
        identity: String(identity || ""),
        columnCount,
        od,
        noteCount,
        noteStarts: starts,
        noteEnds: ends,
        columns,
        isHold,
        slotCol,
        slotNote,
        slotPart,
        slotTime,
        slotStart,
        slotCount,
        totalSlots,
    };
}

/** 4K–18K 左右手分组；奇数键的中列由 `spaceMode` 决定归属（independent 时中列不计入左右手）。 */
export function handGroups(columnCount, spaceMode = "independent") {
    const mid = Math.floor(columnCount / 2);
    const odd = columnCount % 2 === 1;
    if (odd && spaceMode === "independent") {
        // independent：中列不参与任何手，不显示 M
        return { left: [0, mid], space: null, right: [mid + 1, columnCount] };
    }
    if (odd && spaceMode === "left") {
        return { left: [0, mid + 1], space: null, right: [mid + 1, columnCount] };
    }
    if (odd && spaceMode === "right") {
        return { left: [0, mid], space: null, right: [mid, columnCount] };
    }
    return { left: [0, mid], space: null, right: [mid, columnCount] };
}
