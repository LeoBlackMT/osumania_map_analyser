// Copied from sunnyAlgorithm.js
import { OsuFileParser } from "../parser/osuFileParser.js";

const BREAK_ZERO_THRESHOLD_MS = 400;
const GRAPH_RESAMPLE_INTERVAL_MS = 100;
const SMOOTH_SIGMA_MS = 800;















function stepInterp(newX, oldX, oldVals) {
    const out = new Float64Array(newX.length);
    let idx = 0;
    for (let i = 0; i < newX.length; i += 1) {
    const x = newX[i];
    while (idx + 1 < oldX.length && oldX[idx + 1] <= x) {
        idx += 1;
    }
    const clamped = Math.max(0, Math.min(idx, oldVals.length - 1));
    out[i] = oldVals[clamped];
    }
    return out;
}









// Field-copy clone of a processed OsuFileParser (see sunnyAlgorithm.js clone).
// modIN/modHO mutate in place; a shared `parsed` instance must be isolated on a
// clone before converting so later consumers read un-converted data.


function preprocessFile(osuText, speedRate, odFlag, cvtFlag, enableAnalyzeLN = false, parsed = null) {
    // parsed: a shared OsuFileParser instance already processed by the caller.
    // Conversion (modIN/modHO) mutates in place -> run it on a clone so the
    // shared instance stays pristine for later consumers.
    const needsConvert = Boolean(cvtFlag) && (String(cvtFlag).includes("IN") || String(cvtFlag).includes("HO"));
    const pObj = parsed
        ? (needsConvert ? cloneOsuParser(parsed) : parsed)
        : (() => { const p = new OsuFileParser(osuText); p.process(); return p; })();
    let p = pObj.getParsedData();
    let lnRatio;

    if (cvtFlag) {
    if (String(cvtFlag).includes("IN")) {
            try {
        pObj.modIN();
            } catch {
        // keep original on convert error
            }
    }
    if (String(cvtFlag).includes("HO")) {
            try {
        pObj.modHO();
            } catch {
        // keep original on convert error
            }
    }
    }

    // Already computed by process()/modIN/modHO; only recompute on owned
    // (non-shared) parsers so the shared instance stays read-only.
    if (!parsed) {
        pObj.noteTimes = pObj.getNoteTimes();
        pObj.objectIntervals = pObj.getObjectIntervals();
    }
    p = pObj.getParsedData();
    lnRatio = pObj.getLNRatio();

    const columnCount = pObj.getColumnCount();

    if (p.status === "Fail") {
    return {
            status: "Fail",
            x: 0,
            K: 0,
            T: 0,
            noteSeq: [],
            noteSeqByColumn: [],
            lnSeq: [],
            tailSeq: [],
            lnSeqByColumn: [],
            lnRatio,
            columnCount,
            typePercentageData: null,
            lnPartsRatio: 0,
    };
    }
    if (p.status === "NotMania") {
    return {
            status: "NotMania",
            x: 0,
            K: 0,
            T: 0,
            noteSeq: [],
            noteSeqByColumn: [],
            lnSeq: [],
            tailSeq: [],
            lnSeqByColumn: [],
            lnRatio,
            columnCount,
            typePercentageData: null,
            lnPartsRatio: 0,
    };
    }

    if (lnRatio <= 0) {
        return {
            status: "NoLN",
            x: 0,
            K: 0,
            T: 0,
            noteSeq: [],
            noteSeqByColumn: [],
            lnSeq: [],
            tailSeq: [],
            lnSeqByColumn: [],
            lnRatio,
            columnCount,
            typePercentageData: null,
            lnPartsRatio: 0,
        };
    }

    let od = 0;
    if (odFlag == null) {
    od = p.od;
    } else if (odFlag === "HR") {
    od = 6.462 + 0.715 * p.od;
    } else if (odFlag === "EZ") {
    od = -20.761 + 2.566 * p.od;
    } else {
    od = Number.parseFloat(odFlag);
    }

    const timeScale = speedRate !== 0 ? 1 / speedRate : 1;

    function calcTypeMixLength (p) {
        const noteSeq = [];
        for (let i = 0; i < p.columns.length; i += 1) {
            const k = p.columns[i];
            let h = p.noteStarts[i];
            let t = (p.noteTypes[i] & 128) !== 0 ? p.noteEnds[i] : -1;
            h = Math.floor(h * timeScale);
            t = t >= 0 ? Math.floor(t * timeScale) : t;
            noteSeq.push([k, h, t]);
        }
        return getCuttedNoteSeq(noteSeq).length;
    }

    function getTypePercentageData(p, HB_NoteSeq, osuText, speedRate, odFlag, cvtFlag, parsed = null) {
        const Mix_Length = calcTypeMixLength (p);
        const HB_Length = HB_NoteSeq.length;
        const LNParts = getLNParts(true, osuText, speedRate, odFlag, cvtFlag, parsed);
        const LN_NoteSeq = []
        for (let LNPartsIndex = 0; LNPartsIndex < LNParts.length; LNPartsIndex++){
        for (let i = LNParts[LNPartsIndex][0]; i <= LNParts[LNPartsIndex][1]; i += 1) {
            const k = p.columns[i];
            let h = p.noteStarts[i];
            let t = (p.noteTypes[i] & 128) !== 0 ? p.noteEnds[i] : -1;
            h = Math.floor(h * timeScale);
            t = t >= 0 ? Math.floor(t * timeScale) : t;
            LN_NoteSeq.push([k, h, t]);
        }
        }
        const LN_Length = getCuttedNoteSeq(LN_NoteSeq).length

        return [
            ["All", p.columns.length],
            ["RC", p.columns.length - Mix_Length],
            ["Mix", Mix_Length - HB_Length],
            ["HB", HB_Length - LN_Length],
            ["LN", LN_Length],
        ]
    }

    const LNParts = getLNParts(false, osuText, speedRate, odFlag, cvtFlag, parsed);

    // enableAnalyzeLN default false = config.js defaults.enableAnalyzeLN;
    // caller (calculateLN via its options) passes the resolved state value.
    const shouldCalcData = enableAnalyzeLN;
    if (LNParts.length <= 0) {
        return {
            status: "NoLN",
            x: 0,
            K: 0,
            T: 0,
            noteSeq: [],
            noteSeqByColumn: [],
            lnSeq: [],
            tailSeq: [],
            lnSeqByColumn: [],
            lnRatio,
            columnCount,
            typePercentageData : shouldCalcData ? getTypePercentageData(p, [], osuText, speedRate, odFlag, cvtFlag, parsed) : null,
            lnPartsRatio: 0,
        };
    }

    // 第一步：存储正常剪完的note
    const noteSeq_Temp = [];
    for (let LNPartsIndex = 0; LNPartsIndex < LNParts.length; LNPartsIndex++){
    for (let i = LNParts[LNPartsIndex][0]; i <= LNParts[LNPartsIndex][1]; i += 1) {
        const k = p.columns[i];
        let h = p.noteStarts[i];
        let t = (p.noteTypes[i] & 128) !== 0 ? p.noteEnds[i] : -1;

        h = Math.floor(h * timeScale);
        t = t >= 0 ? Math.floor(t * timeScale) : t;

        noteSeq_Temp.push([k, h, t]);
    }
    }

    const noteSeq = getCuttedNoteSeq(noteSeq_Temp);

    const typePercentageData = shouldCalcData ? getTypePercentageData(p, noteSeq, osuText, speedRate, odFlag, cvtFlag, parsed) : null;
    if (noteSeq.length <= 0) {
        return {
            status: "NoLN",
            x: 0,
            K: 0,
            T: 0,
            noteSeq: [],
            noteSeqByColumn: [],
            lnSeq: [],
            tailSeq: [],
            lnSeqByColumn: [],
            lnRatio,
            columnCount,
            typePercentageData,
            lnPartsRatio: 0,
        };
    }

    let x = 0.3 * Math.sqrt((64.5 - Math.ceil(od * 3)) / 500);
    x = Math.min(x, 0.6 * (x - 0.09) + 0.09);

    noteSeq.sort((a, b) => {
    if (a[1] !== b[1]) return a[1] - b[1];
    return a[0] - b[0];
    });

    const K = p.columnCount;
    const noteSeqByColumn = Array.from({ length: K }, () => []);
    for (const n of noteSeq) {
    const col = n[0];
    if (col >= 0 && col < K) noteSeqByColumn[col].push(n);
    }

    const lnSeq = noteSeq.filter((n) => n[2] >= 0);
    const tailSeq = [...lnSeq].sort((a, b) => a[2] - b[2]);

    const lnSeqByColumn = Array.from({ length: K }, () => []);
    for (const n of lnSeq) {
    const col = n[0];
    if (col >= 0 && col < K) lnSeqByColumn[col].push(n);
    }

    const maxHead = noteSeq.length ? Math.max(...noteSeq.map((n) => n[1])) : 0;
    const maxTail = noteSeq.length ? Math.max(...noteSeq.map((n) => n[2])) : 0;
    const T = Math.max(maxHead, maxTail) + 1;

    return {
    status: "OK",
    x,
    K,
    T,
    noteSeq,
    noteSeqByColumn,
    lnSeq,
    tailSeq,
    lnSeqByColumn,
    lnRatio,
    columnCount,
    typePercentageData,
    lnPartsRatio: noteSeq.length / p.columns.length,
    };
}





function getKeyUsage400(K, T, noteSeq, baseCorners) {
    const keyUsage400 = {};
    for (let k = 0; k < K; k += 1) {
    keyUsage400[k] = new Array(baseCorners.length).fill(0);
    }

    for (const [k, h, t] of noteSeq) {
    const startTime = Math.max(h, 0);
    const endTime = t < 0 ? h : Math.min(t, T - 1);

    const left400Idx = bisectLeft(baseCorners, startTime - 400);
    const leftIdx = bisectLeft(baseCorners, startTime);
    const rightIdx = bisectLeft(baseCorners, endTime);
    const right400Idx = bisectLeft(baseCorners, endTime + 400);

    for (let idx = leftIdx; idx < rightIdx; idx += 1) {
            keyUsage400[k][idx] += 3.75 + Math.min(endTime - startTime, 1500) / 150;
    }

    for (let idx = left400Idx; idx < leftIdx; idx += 1) {
            keyUsage400[k][idx] += 3.75 - (3.75 / (400 ** 2)) * ((baseCorners[idx] - startTime) ** 2);
    }

    for (let idx = rightIdx; idx < right400Idx; idx += 1) {
            keyUsage400[k][idx] += 3.75 - (3.75 / (400 ** 2)) * (Math.abs(baseCorners[idx] - endTime) ** 2);
    }
    }

    return keyUsage400;
}







function computeJbar(K, x, noteSeqByColumn, baseCorners) {
    const jackNerfer = (delta) => 1 - 7e-5 * ((0.15 + Math.abs(delta - 0.08)) ** (-4));

    const Jks = {};
    const deltaKs = {};
    for (let k = 0; k < K; k += 1) {
    Jks[k] = new Array(baseCorners.length).fill(0);
    deltaKs[k] = new Array(baseCorners.length).fill(1e9);
    }

    for (let k = 0; k < K; k += 1) {
    const notes = noteSeqByColumn[k] || [];
    for (let i = 0; i < notes.length - 1; i += 1) {
            const start = notes[i][1];
            const end = notes[i + 1][1];

            const leftIdx = bisectLeft(baseCorners, start);
            const rightIdx = bisectLeft(baseCorners, end);
            if (leftIdx >= rightIdx) continue;

            const delta = 0.001 * (end - start);
            const val = (delta ** -1) * ((delta + 0.11 * (x ** 0.25)) ** -1);
            const jVal = val * jackNerfer(delta);

            for (let idx = leftIdx; idx < rightIdx; idx += 1) {
        Jks[k][idx] = jVal;
        deltaKs[k][idx] = delta;
            }
    }
    }

    const JbarKs = {};
    for (let k = 0; k < K; k += 1) {
    JbarKs[k] = smoothOnCorners(baseCorners, Jks[k], 500, 0.001, "sum");
    }

    const Jbar = new Array(baseCorners.length).fill(0);
    for (let i = 0; i < baseCorners.length; i += 1) {
    let num = 0;
    let den = 0;
    for (let k = 0; k < K; k += 1) {
            const v = JbarKs[k][i];
            const w = 1 / deltaKs[k][i];
            num += (Math.max(v, 0) ** 5) * w;
            den += w;
    }
    const raw = num / Math.max(1e-9, den);
    Jbar[i] = raw ** (1 / 5);
    }

    return { deltaKs, Jbar };
}



function computePbar(x, noteSeq, lnRep, anchor, baseCorners) {
    const streamBooster = (delta) => {
    const expr = (7.5 / delta);
    if (160 < expr && expr < 360) {
            return 1 + 1.7e-7 * (expr - 160) * ((expr - 360) ** 2);
    }
    return 1;
    };

    const pStep = new Array(baseCorners.length).fill(0);

    for (let i = 0; i < noteSeq.length - 1; i += 1) {
    const hL = noteSeq[i][1];
    const hR = noteSeq[i + 1][1];
    const deltaTime = hR - hL;

    if (deltaTime < 1e-9) {
            const spike = 1000 * ((0.02 * (4 / x - 24)) ** 0.25);
            const leftIdx = bisectLeft(baseCorners, hL);
            const rightIdx = bisectRight(baseCorners, hL);
            for (let idx = leftIdx; idx < rightIdx; idx += 1) {
        pStep[idx] += spike;
            }
            continue;
    }

    const leftIdx = bisectLeft(baseCorners, hL);
    const rightIdx = bisectLeft(baseCorners, hR);
    if (leftIdx >= rightIdx) continue;

    const delta = 0.001 * deltaTime;
    const v = 1 + 6 * 0.001 * lnSum(hL, hR, lnRep);
    const bVal = streamBooster(delta);

    let inc = 0;
    if (delta < (2 * x) / 3) {
            inc = (delta ** -1) * ((0.08 * (x ** -1) * (1 - 24 * (x ** -1) * ((delta - x / 2) ** 2))) ** 0.25) * Math.max(bVal, v);
    } else {
            inc = (delta ** -1) * ((0.08 * (x ** -1) * (1 - 24 * (x ** -1) * ((x / 6) ** 2))) ** 0.25) * Math.max(bVal, v);
    }

    for (let idx = leftIdx; idx < rightIdx; idx += 1) {
            pStep[idx] += Math.min(inc * anchor[idx], Math.max(inc, inc * 2 - 10));
    }
    }

    return smoothOnCorners(baseCorners, pStep, 500, 0.001, "sum");
}





function computeCAndKs(K, noteSeq, keyUsage, baseCorners) {
    const noteHitTimes = noteSeq.map((n) => n[1]).sort((a, b) => a - b);

    const CStep = new Float64Array(baseCorners.length);
    let lo = 0;
    let hi = 0;
    for (let i = 0; i < baseCorners.length; i += 1) {
    const s = baseCorners[i];
    const low = s - 500;
    const high = s + 500;

    while (lo < noteHitTimes.length && noteHitTimes[lo] < low) {
            lo += 1;
    }
    while (hi < noteHitTimes.length && noteHitTimes[hi] < high) {
            hi += 1;
    }

    CStep[i] = hi - lo;
    }

    const KsStep = new Float64Array(baseCorners.length);
    for (let i = 0; i < baseCorners.length; i += 1) {
    let count = 0;
    for (let k = 0; k < K; k += 1) {
            if (keyUsage[k][i]) count += 1;
    }
    KsStep[i] = Math.max(count, 1);
    }

    return { CStep, KsStep };
}






const MIN_LN_PERCENTAGE = 20; // 20%
const MIN_LN_DENSITY = MIN_LN_PERCENTAGE * 0.01;
const TYPELN_MIN_LN_DENSITY = 100 * 0.01; // 90%

/**
 * 解析 .osu 文本中的 timing points。
 * 返回 {timings, isSV}：
 *   timings = 非继承 timing point 数组 {offset, beatLength, signature}
 *   isSV = 如果谱面存在复杂 SV（非1.0x速度持续超过2秒），则为 true
 */
function parseTimingsAndDetectSV(osuText) {
    const lines = osuText.split('\n');
    let inTiming = false;
    const uninherited = [];
    let svTotalMs = 0;
    let svIntervals = 0;
    let inSV = false;
    let svStartTime = 0;
    const SV_EPS = 0.05;

    for (const line of lines) {
        const t = line.trim();
        if (t === '[TimingPoints]') { inTiming = true; continue; }
        if (inTiming && t.startsWith('[')) break;
        if (!inTiming || t.startsWith('//') || t === '') continue;
        const parts = t.split(',');
        const offset = parseFloat(parts[0]);
        const beatLength = parseFloat(parts[1]);
        const signature = parseInt(parts[2]) || 4;

        if (beatLength > 0) {
            uninherited.push({ offset, beatLength, signature });
        } else {
            // 继承 timing point：beatLength < 0，速度 = -100 / beatLength
            const vel = -100 / beatLength;
            const isNonOne = Math.abs(vel - 1) > SV_EPS;
            if (isNonOne && !inSV) {
                svIntervals++;
                inSV = true;
                svStartTime = offset;
            } else if (!isNonOne && inSV) {
                svTotalMs += offset - svStartTime;
                inSV = false;
            }
        }
    }
    // 处理末尾仍在 SV 中的情况
    if (inSV) svTotalMs += (uninherited.length > 0 ? uninherited[uninherited.length - 1].offset : 0) - svStartTime;

    const timings = uninherited.length > 0 ? uninherited : [{ offset: 0, beatLength: 500, signature: 4 }];
    const isSV = svIntervals > 1 && svTotalMs > 2000;
    return { timings, isSV };
}

/**
 * 计算给定时间戳处的单位长度（4小节）的毫秒数。
 */
function getUnitLength(timings, timeMs) {
    let active = timings[0];
    for (const tp of timings) {
        if (tp.offset <= timeMs) active = tp;
        else break;
    }
    return active.signature * active.beatLength * 4;
}

/**
 * 计算 [timeStart, timeEnd) 范围内的 LN 时长覆盖。
 * 返回 LN 占据的总毫秒数（合并重叠区间，不重复计算）。
 */
function computeLNDuration(p, timeStart, timeEnd, noteStartIdx, noteEndIdx) {
    let merged = 0;
    let curStart = -1e300, curEnd = -1e300;
    for (let k = noteStartIdx; k <= noteEndIdx; k++) {
        if (p.noteStarts[k] >= timeEnd) break;
        if ((p.noteTypes[k] & 128) !== 0) {
            if (curStart !== -1e300 && p.noteStarts[k] > curEnd) {
                merged += (curEnd - curStart);
                curStart = -1e300;
                curEnd = -1e300;
            }
            if (curStart === -1e300) {
                curStart = p.noteStarts[k];
                curEnd = p.noteEnds[k];
            }
            else {
                curEnd = Math.max(curEnd, p.noteEnds[k]);
            }
        }
    }
    if (curStart !== -1e300) merged += (curEnd - curStart);
    return merged;
}

function getLNParts(pureLN, osuText, _speedRate, odFlag, cvtFlag, parsed = null) {
    const MinLNDensity = pureLN ? TYPELN_MIN_LN_DENSITY : MIN_LN_DENSITY;
    // parsed: shared OsuFileParser instance for the note data; SV/timing
    // detection below still parses [TimingPoints] from text (parseTimingsAndDetectSV),
    // which is not part of the parsed chart — keep that text path.
    const needsConvert = Boolean(cvtFlag) && (String(cvtFlag).includes("IN") || String(cvtFlag).includes("HO"));
    const pObj = parsed
        ? (needsConvert ? cloneOsuParser(parsed) : parsed)
        : (() => { const p = new OsuFileParser(osuText); p.process(); return p; })();
    let p = pObj.getParsedData();
    let lnRatio;

    if (cvtFlag) {
    if (String(cvtFlag).includes("IN")) {
            try {
        pObj.modIN();
            } catch {
        // keep original on convert error
            }
    }
    if (String(cvtFlag).includes("HO")) {
            try {
        pObj.modHO();
            } catch {
        // keep original on convert error
            }
    }
    }

    if (!parsed) {
        pObj.noteTimes = pObj.getNoteTimes();
        pObj.objectIntervals = pObj.getObjectIntervals();
    }
    p = pObj.getParsedData();
    lnRatio = pObj.getLNRatio();

    const columnCount = pObj.getColumnCount();

    if (p.status === "Fail" || p.status === "NotMania" || lnRatio <= 0) return new Array();

    const speedRate = _speedRate !== 0 ? _speedRate : 1;

    // 解析 timing points 并检测 SV
    const { timings: tpTimings, isSV } = parseTimingsAndDetectSV(osuText);

    // SV 回退：复杂 SV → 固定5秒区块；否则 → 4小节区块
    const SV_FIXED_BLOCK = 5000;
    function getBlockLength(timeMs) {
        if (isSV) return SV_FIXED_BLOCK * speedRate;
        return getUnitLength(tpTimings, timeMs) * speedRate;
    }

    // 判断 LN 占比是否达到阈值（基于音符数量）
    function isLNPercentageValid(riceCount, LNCount, MinLnDensity) {
        if (riceCount + LNCount === 0) return false;
        return (LNCount / (riceCount + LNCount)) >= MinLnDensity;
    }

    /**
     * 判断区块内 LN 时长覆盖是否超过阈值。
     * 使用合并的 LN 区间避免重复计算。
     */
    function isLNDurationValid(noteStartIdx, noteEndIdx, blockStart, blockEnd, threshold) {
        if (noteStartIdx === -1) return false;
        const duration = computeLNDuration(p, blockStart, blockEnd, noteStartIdx, noteEndIdx);
        return duration / (blockEnd - blockStart) > threshold;
    }

    /**
     * 统计 [timeStart, timeEnd) 范围内的音符数量（从 startIndex 开始扫描）。
     * 返回 {riceCount, lnCount, noteStartIdx, noteEndIdx, nextIndex}
     */
    function countNotesInRange(timeStart, timeEnd, startIndex) {
        let rc = 0, ln = 0;
        let noteStartIdx = -1, noteEndIdx = -1;
        let idx = startIndex;
        while (idx < p.columns.length && p.noteStarts[idx] < timeEnd) {
            if (p.noteStarts[idx] >= timeStart) {
                if ((p.noteTypes[idx] & 128) === 0) rc++; else ln++;
                if (noteStartIdx === -1) noteStartIdx = idx;
                noteEndIdx = idx;
            }
            idx++;
        }
        return { riceCount: rc, lnCount: ln, noteStartIdx, noteEndIdx, nextIndex: idx };
    }

    // 第一步：按单位长度步进，框出所有区间
    const firstNoteTime = p.noteStarts[0];
    const lastNoteTime = p.noteStarts[p.columns.length - 1];
    let blockStart = firstNoteTime;
    let WindowList1 = new Array();

    let scanIndex = 0;

    while (blockStart <= lastNoteTime) {
        const blockLen = getBlockLength(blockStart);
        const blockEnd = blockStart + blockLen;

        const block = countNotesInRange(blockStart, blockEnd, scanIndex);
        scanIndex = block.nextIndex;

        if (block.noteStartIdx === -1) {
            blockStart = blockEnd;
            continue;
        }

        if (isLNPercentageValid(block.riceCount, block.lnCount, MinLNDensity)) {
            // 强区块：向后扩展，条件是下一个区块 >25% LN 时长 且 合并后 >20% LN 占比
            let extendEnd = blockEnd;
            let extendNoteEnd = block.noteEndIdx;
            let accumRC = block.riceCount;
            let accumLN = block.lnCount;

            while (true) {
                const nextStart = extendEnd;
                const nextEnd = nextStart + getBlockLength(nextStart);
                const peek = countNotesInRange(nextStart, nextEnd, scanIndex);

                if (peek.noteStartIdx === -1) break;
                // 基于时长判断：下一个区块 LN 时长覆盖 >25%？
                if (!isLNDurationValid(peek.noteStartIdx, peek.noteEndIdx, nextStart, nextEnd, 0.25)) break;
                // 合并后 LN 占比 >20%？
                if (!isLNPercentageValid(accumRC + peek.riceCount, accumLN + peek.lnCount, MinLNDensity)) break;

                extendEnd = nextEnd;
                extendNoteEnd = peek.noteEndIdx;
                accumRC += peek.riceCount;
                accumLN += peek.lnCount;
                scanIndex = peek.nextIndex;
            }

            WindowList1.push([block.noteStartIdx, extendNoteEnd]);
            blockStart = extendEnd;
        } else {
            // 弱区块：窥视下一个区块，如果 LN 时长覆盖 >25% → 扩展1个单位；这一机制在纯LN检测时被禁用
            const nextStart = blockEnd;
            const nextEnd = nextStart + getBlockLength(nextStart);
            const peek = countNotesInRange(nextStart, nextEnd, scanIndex);

            if (peek.noteStartIdx !== -1 && !pureLN && isLNDurationValid(peek.noteStartIdx, peek.noteEndIdx, nextStart, nextEnd, 0.25)) {
                WindowList1.push([block.noteStartIdx, peek.noteEndIdx]);
                scanIndex = peek.nextIndex;
                blockStart = nextEnd;
            } else {
                scanIndex = peek.nextIndex;
                blockStart = nextEnd;
            }
        }
    }

    if (WindowList1.length === 0) return WindowList1;

    // 第二步：合并区间
    let WindowList2 = new Array();
    let windowStartIndex = -1;
    let windowEndIndex = -1;
    for (let i = 0; i < WindowList1.length; i++) {
        if (windowStartIndex !== -1) {
            if (WindowList1[i][0] <= windowEndIndex + 1) {
                windowEndIndex = WindowList1[i][1];
            } else {
                WindowList2.push([windowStartIndex, windowEndIndex]);
                windowStartIndex = -1;
            }
        }
        if (windowStartIndex === -1) {
            windowStartIndex = WindowList1[i][0];
            windowEndIndex = WindowList1[i][1];
        }
        if (i + 1 >= WindowList1.length && windowStartIndex !== -1) {
            WindowList2.push([windowStartIndex, windowEndIndex]);
        }
    }
    WindowList1 = null;
    // getCuttedNoteSeq做过一次删米了
    return WindowList2;
}
    
function getCuttedNoteSeq(noteSeq_Temp) {
    // 截取有LN覆盖的时间（当前版本不考虑单手情况，改成只有单手也简单） 如果改成单手记得改
    const LNTimes = [];
    
    // 第一步：框出所有区间
    let currentTime = undefined;
    let LNStartTime = -1e300;
    let LNEndTime = -1e300;
    let LNRangeList = new Array();

    for (const [, start, end] of noteSeq_Temp) {
        if (start !== currentTime) {
            if (LNStartTime !== -1e300 && LNEndTime < start) {
                LNRangeList.push([LNStartTime, LNEndTime]);
                LNStartTime = -1e300;
                LNEndTime = -1e300;
            }
            currentTime = start;
        }
        if (end !== -1) { //是LN
            if (LNStartTime === -1e300) LNStartTime = start;
            LNEndTime = Math.max(LNEndTime, end);
        }
    }
    if (LNStartTime !== -1e300) LNRangeList.push([LNStartTime, LNEndTime]); // 防止LN结尾
    if (LNRangeList.length <= 0) return [];

    // 第二步：返回
    let i = 0;
    const noteSeq = [];
    for (const [k, start, end] of noteSeq_Temp) {
        if (start > LNRangeList[i][1]) i++; // 显而易见的一个note不会连续跳过两个窗口
        if (i >= LNRangeList.length) break;
        if (start >= LNRangeList[i][0]) noteSeq.push([k, start, end]);
    }
    return noteSeq;
}

export function calculateLN(osuText, speedRate = 1.0, odFlag = null, cvtFlag = null, options = {}, parsed = null) {
    const withGraph = options?.withGraph === true;
    const enableAnalyzeLN = options?.enableAnalyzeLN === true;

    const {
    status,
    x,
    K,
    T,
    noteSeq,
    noteSeqByColumn,
    lnSeq,
    tailSeq,
    lnRatio,
    columnCount,
    typePercentageData,
    lnPartsRatio,
    } = preprocessFile(osuText, speedRate, odFlag, cvtFlag, enableAnalyzeLN, parsed);

    if (status === "Fail") return -1;
    if (status === "NotMania") return -2;
    if (status === "NoLN") return {NoLN: true, typePercentageData: typePercentageData}
    if (!noteSeq.length || K <= 0) return -1;

    const { allCorners, baseCorners, ACorners } = getCorners(T, noteSeq);

    const keyUsage = getKeyUsage(K, T, noteSeq, baseCorners);
    const activeColumns = baseCorners.map((_, i) => {
    const active = [];
    for (let k = 0; k < K; k += 1) {
            if (keyUsage[k][i]) active.push(k);
    }
    return active;
    });

    const keyUsage400 = getKeyUsage400(K, T, noteSeq, baseCorners);
    const anchor = computeAnchor(K, keyUsage400, baseCorners);

    const { deltaKs, Jbar: JbarBase } = computeJbar(K, x, noteSeqByColumn, baseCorners);
    const Jbar = interpValues(allCorners, baseCorners, JbarBase);

    const XbarBase = computeXbar(K, x, noteSeqByColumn, activeColumns, baseCorners);
    const Xbar = interpValues(allCorners, baseCorners, XbarBase);

    const lnRep = lnBodiesCountSparseRepresentation(lnSeq, T);
    const PbarBase = computePbar(x, noteSeq, lnRep, anchor, baseCorners);
    const Pbar = interpValues(allCorners, baseCorners, PbarBase);

    const AbarBase = computeAbar(K, activeColumns, deltaKs, ACorners, baseCorners);
    const Abar = interpValues(allCorners, ACorners, AbarBase);

    const RbarBase = computeRbar(K, x, noteSeqByColumn, tailSeq, baseCorners);
    const Rbar = interpValues(allCorners, baseCorners, RbarBase);

    const { CStep, KsStep } = computeCAndKs(K, noteSeq, keyUsage, baseCorners);
    const CArr = stepInterp(allCorners, baseCorners, CStep);
    const KsArr = stepInterp(allCorners, baseCorners, KsStep);

    const DAll = new Array(allCorners.length).fill(0);
    for (let i = 0; i < allCorners.length; i += 1) {
    const leftPart = 0.4 * ((Abar[i] ** (3 / KsArr[i]) * Math.min(Jbar[i], 8 + 0.85 * Jbar[i])) ** 1.5);
    const rightPart = 0.6 * ((Abar[i] ** (2 / 3) * (0.8 * Pbar[i] + Rbar[i] * 35 / (CArr[i] + 8))) ** 1.5);
    const SAll = (leftPart + rightPart) ** (2 / 3);
    const TAll = (Abar[i] ** (3 / KsArr[i]) * Xbar[i]) / (Xbar[i] + SAll + 1);
    DAll[i] = 2.7 * (SAll ** 0.5) * (TAll ** 1.5) + SAll * 0.27;
    }

    const gaps = new Array(allCorners.length).fill(0);
    gaps[0] = (allCorners[1] - allCorners[0]) / 2;
    gaps[gaps.length - 1] = (allCorners[allCorners.length - 1] - allCorners[allCorners.length - 2]) / 2;
    for (let i = 1; i < allCorners.length - 1; i += 1) {
    gaps[i] = (allCorners[i + 1] - allCorners[i - 1]) / 2;
    }

    const effectiveWeights = CArr.map((c, i) => c * gaps[i]);
    const sortedIndices = DAll.map((_, i) => i).sort((a, b) => DAll[a] - DAll[b]);
    const DSorted = sortedIndices.map((i) => DAll[i]);
    const wSorted = sortedIndices.map((i) => effectiveWeights[i]);

    const cumWeights = new Array(wSorted.length).fill(0);
    let running = 0;
    for (let i = 0; i < wSorted.length; i += 1) {
    running += wSorted[i];
    cumWeights[i] = running;
    }

    const totalWeight = cumWeights[cumWeights.length - 1];
    const normCumWeights = cumWeights.map((w) => w / totalWeight);

    const targetPercentiles = [0.945, 0.935, 0.925, 0.915, 0.845, 0.835, 0.825, 0.815];
    const percentileIndices = targetPercentiles.map((p) => bisectLeft(normCumWeights, p));

    const firstGroup = percentileIndices.slice(0, 4).map((idx) => DSorted[Math.min(idx, DSorted.length - 1)]);
    const secondGroup = percentileIndices.slice(4, 8).map((idx) => DSorted[Math.min(idx, DSorted.length - 1)]);

    const percentile93 = firstGroup.reduce((acc, v) => acc + v, 0) / firstGroup.length;
    const percentile83 = secondGroup.reduce((acc, v) => acc + v, 0) / secondGroup.length;

    let num = 0;
    let den = 0;
    for (let i = 0; i < DSorted.length; i += 1) {
    num += (DSorted[i] ** 5) * wSorted[i];
    den += wSorted[i];
    }
    const weightedMean = (num / den) ** (1 / 5);

    let sr = (0.88 * percentile93) * 0.25 + (0.94 * percentile83) * 0.2 + weightedMean * 0.55;
    sr = (sr ** 1) / (8 ** 1) * 8;

    let lnLengthTerm = 0;
    for (const [, h, t] of lnSeq) {
    lnLengthTerm += Math.min((t - h), 1000) / 200;
    }
    const totalNotes = noteSeq.length + 0.5 * lnLengthTerm;

    sr *= totalNotes / (totalNotes + 60);
    sr = rescaleHigh(sr);
    sr *= 0.975;

    const ret = {
        star: sr,
        lnRatio,
        columnCount,
        typePercentageData,
        lnPartsRatio
    }
    if (withGraph) {
        const DPre = applyProximityEnvelope(allCorners, DAll, noteSeq);
        const DGraph = smoothDForGraph(allCorners, DPre, noteSeq);
        ret.graph = {
            times: Array.from(allCorners),
            values: DGraph,
        };
    }

    return ret;
}
