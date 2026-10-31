import { OsuFileParser } from "../parser/osuFileParser.js";
import {
    crossMatrix,
    interpSingle,
    findNextNoteInColumn,
    cloneOsuParser,
    getCorners,
    getKeyUsage,
    computeAnchor,
    lnBodiesCountSparseRepresentation,
    lnSum,
    computeXbar,
    computeAbar,
    computeRbar,
} from "./sunnyCore.js";

import {
    bisectLeft,
    bisectRight,
    cumulativeSum,
    queryCumsum,
    smoothOnCorners,
    interpValues,
    gaussianFilter1d,
    rescaleHigh,
    mergeByHead,
    applyProximityEnvelope,
    smoothDForGraph,
    jackNerfer,
    targetPercentiles,
} from "./reworkMathCore.js";













function stepInterp(newX, oldX, oldVals) {
    const out = new Float64Array(newX.length);
    let idx = 0;
    for (let i = 0; i < newX.length; i += 1) {
    const x = newX[i];
    // D1: exact match takes the previous sample (cs StepInterp: BinarySearch + idx - 1).
    // Sunny-specific since 042ccee (cs osu-author-port sync); daniel keeps the old
    // semantics via reworkMathCore.stepInterp — do NOT move this back into the core.
    while (idx + 1 < oldX.length && oldX[idx + 1] < x) {
        idx += 1;
    }
    const clamped = Math.max(0, Math.min(idx, oldVals.length - 1));
    out[i] = oldVals[clamped];
    }
    return out;
}



// Field-copy clone of a processed OsuFileParser. modIN/modHO mutate the parser
// in place (columns/noteStarts/noteTypes/noteEnds/breaks), so a shared `parsed`
// instance must be isolated on a clone before converting — the shared chart
// stays pristine for later consumers (mutability audit, perf task 9).


function preprocessFile(osuText, speedRate, odFlag, cvtFlag, parsed = null) {
    // parsed: a shared OsuFileParser instance already processed by the caller.
    // Skips the parse; modIN/modHO conversion still runs on the parsed result.
    // Conversion mutates in place -> run it on a clone so the shared instance
    // (and any later consumer) sees un-converted data, matching the fresh-parse
    // behavior where each core parses its own copy.
    const needsConvert = Boolean(cvtFlag) && (String(cvtFlag).includes("IN") || String(cvtFlag).includes("HO"));
    const pObj = parsed
        ? (needsConvert ? cloneOsuParser(parsed) : parsed)
        : (() => { const p = new OsuFileParser(osuText); p.process(); return p; })();
    let p = pObj.getParsedData();
    let lnRatio = p.lnRatio;

    // PP metrics: raw HitObjects count (hold=1), captured BEFORE modIN/modHO
    // conversion (cs ManiaDifficultyCalculator TotalNotes = HitObjects.Count).
    const totalNotes = p.columns.length;

    if (cvtFlag) {
    if (String(cvtFlag).includes("IN")) {
            try {
        pObj.modIN();
        lnRatio = pObj.getLNRatio();
            } catch {
        // keep original on convert error
            }
    }
    if (String(cvtFlag).includes("HO")) {
            try {
        pObj.modHO();
        lnRatio = pObj.getLNRatio();
            } catch {
        // keep original on convert error
            }
    }
    }

    // On a shared parsed instance these were already computed by process()
    // (and by modIN/modHO when converting) — recomputing writes identical
    // values but mutates the shared object; skip for read-only sharing.
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
    // 防御：非法字符串（如 "none"）回落谱面 OD，避免 NaN 全链。
    if (!Number.isFinite(od)) {
    od = p.od;
    }
    }

    const timeScale = speedRate !== 0 ? 1 / speedRate : 1;

    const noteSeq = [];
    for (let i = 0; i < p.columns.length; i += 1) {
    const k = p.columns[i];
    let h = p.noteStarts[i];
    let t = (p.noteTypes[i] & 128) !== 0 ? p.noteEnds[i] : -1;

    h = Math.floor(h * timeScale);
    t = t >= 0 ? Math.floor(t * timeScale) : t;

    noteSeq.push([k, h, t]);
    }

    let x = 0.3 * Math.sqrt((64.5 - Math.ceil(od * 3)) / 500);
    x = Math.min(x, 0.6 * (x - 0.09) + 0.09);

    noteSeq.sort((a, b) => {
    if (a[1] !== b[1]) return a[1] - b[1];
    return a[0] - b[0];
    });
    // D3: drop the earliest note, aligning with cs `for (int i = 1; ...)` in ManiaDifficultyCalculator
    noteSeq.shift();

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
    totalNotes,
    };
}





function getKeyUsage400(K, T, noteSeq, baseCorners) {
    const keyUsage400 = {};
    for (let k = 0; k < K; k += 1) {
    keyUsage400[k] = new Array(baseCorners.length).fill(0);
    }

    // ponytail: loop-invariant across notes — cornerCoeff computed once instead of per (note, corner)
    const cornerCoeff = 3.75 / (400 ** 2);

    for (const [k, h, t] of noteSeq) {
    const startTime = Math.max(h, 0);
    const endTime = t < 0 ? h : Math.min(t, T - 1);

    const left400Idx = bisectLeft(baseCorners, startTime - 400);
    const leftIdx = bisectLeft(baseCorners, startTime);
    const rightIdx = bisectLeft(baseCorners, endTime);
    const right400Idx = bisectLeft(baseCorners, endTime + 400);

    const rampValue = 3.75 + Math.min(endTime - startTime, 1500) / 150;

    for (let idx = leftIdx; idx < rightIdx; idx += 1) {
            keyUsage400[k][idx] += rampValue;
    }

    for (let idx = left400Idx; idx < leftIdx; idx += 1) {
            keyUsage400[k][idx] += 3.75 - cornerCoeff * ((baseCorners[idx] - startTime) ** 2);
    }

    for (let idx = rightIdx; idx < right400Idx; idx += 1) {
            keyUsage400[k][idx] += 3.75 - cornerCoeff * (Math.abs(baseCorners[idx] - endTime) ** 2);
    }
    }

    return keyUsage400;
}







function computeJbar(K, x, noteSeqByColumn, baseCorners) {

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

    // ponytail: loop-invariant across corners — incMax computed once per note
    const incMax = Math.max(inc, inc * 2 - 10);

    for (let idx = leftIdx; idx < rightIdx; idx += 1) {
            pStep[idx] += Math.min(inc * anchor[idx], incMax);
    }
    }

    return smoothOnCorners(baseCorners, pStep, 500, 0.001, "sum");
}





function computeCAndKs(K, noteSeq, keyUsage, baseCorners) {
    const noteHitTimes = noteSeq.map((n) => n[1]).sort((a, b) => a - b);
    // D2: V2 = heads + LN tails (cs noteHitTimesV2), used for effectiveWeights
    const noteHitTimesV2 = noteSeq
    .flatMap((n) => (n[2] >= 0 ? [n[1], n[2]] : [n[1]]))
    .sort((a, b) => a - b);

    const CStep = new Float64Array(baseCorners.length);
    const CStepV2 = new Float64Array(baseCorners.length);
    let lo = 0;
    let hi = 0;
    let lo2 = 0;
    let hi2 = 0;
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

    while (lo2 < noteHitTimesV2.length && noteHitTimesV2[lo2] < low) {
            lo2 += 1;
    }
    while (hi2 < noteHitTimesV2.length && noteHitTimesV2[hi2] < high) {
            hi2 += 1;
    }

    CStepV2[i] = hi2 - lo2;
    }

    const KsStep = new Float64Array(baseCorners.length);
    for (let i = 0; i < baseCorners.length; i += 1) {
    let count = 0;
    for (let k = 0; k < K; k += 1) {
            if (keyUsage[k][i]) count += 1;
    }
    KsStep[i] = Math.max(count, 1);
    }

    return { CStep, CStepV2, KsStep };
}

// =======================================================================
// PP metrics (ported from genirx dart sunny_algorithm.dart :1595-1778)
// =======================================================================

// Rao quadratic entropy with logarithmic distance (dart _raoQuadraticEntropyLog):
// Q = Σᵢⱼ pᵢ·pⱼ·dist(i,j), dist = ln(1+|x−y|) applied `logIterations` times.
function raoQuadraticEntropyLog(values, logIterations) {
    if (values.length === 0) return 0;
    const counts = new Map();
    for (const v of values) counts.set(v, (counts.get(v) ?? 0) + 1);
    const unique = [...counts.keys()];
    const total = values.length;
    const p = unique.map((k) => counts.get(k) / total);
    const n = unique.length;
    let q = 0;
    for (let i = 0; i < n; i += 1) {
    for (let j = 0; j < n; j += 1) {
            let acc = Math.abs(unique[i] - unique[j]);
            for (let k = 0; k < logIterations; k += 1) {
        acc = Math.log(1 + acc);
            }
            q += p[i] * p[j] * acc;
    }
    }
    return q;
}

// Variety (dart _variety :1629-1669): 0.5·headVariety + 0.11·tailVariety + 0.45·colVariety.
// tail uses ALL notes sorted by tail (non-LN tail=-1 sorts first) — distinct from lnSeq tailSeq.
function computeVariety(noteSeq, noteSeqByColumn, keyCount) {
    const tailSorted = [...noteSeq].sort((a, b) => a[2] - b[2]);

    const headGaps = [];
    for (let i = 0; i < noteSeq.length - 1; i += 1) {
    headGaps.push(noteSeq[i + 1][1] - noteSeq[i][1]);
    }
    const tailGaps = [];
    for (let i = 0; i < tailSorted.length - 1; i += 1) {
    tailGaps.push(tailSorted[i + 1][2] - tailSorted[i][2]);
    }

    const headVariety = raoQuadraticEntropyLog(headGaps, 1);
    const tailVariety = raoQuadraticEntropyLog(tailGaps, 1);

    const headGapsNew = [];
    for (let k = 0; k < keyCount; k += 1) {
    const heads = noteSeqByColumn[k] || [];
    for (let i = 0; i < heads.length - 1; i += 1) {
            headGapsNew.push(heads[i + 1][1] - heads[i][1]);
    }
    }
    const colVariety = 2.5 * raoQuadraticEntropyLog(headGapsNew, 2);

    return 0.5 * headVariety + 0.11 * tailVariety + 0.45 * colVariety;
}

// Switches (dart _switches :1671-1778): head/tail gap signatures with ±50 window
// sliding average, Ks^0.25 and D weights. tail branch only when tails.last > tails.first.
function computeSwitches(noteSeq, tailSeq, allCorners, ksArr, dAll) {
    const heads = noteSeq.map((n) => n[1]);
    const idxList = heads.map((h) => bisectLeft(allCorners, h));
    const nHead = idxList.length > 1 ? idxList.length - 1 : 0;
    const ksArrAtNote = [];
    const weightsAtNote = [];
    for (let i = 0; i < nHead; i += 1) {
    ksArrAtNote.push(ksArr[idxList[i]]);
    weightsAtNote.push(dAll[idxList[i]]);
    }

    const headGaps = [];
    for (let i = 0; i < heads.length - 1; i += 1) {
    headGaps.push(heads[i + 1] - heads[i]);
    }
    const numHeadGaps = headGaps.length;

    const avgs = new Array(numHeadGaps);
    for (let i = 0; i < numHeadGaps; i += 1) {
    const start = Math.max(0, i - 50);
    const end = Math.min(i + 50, numHeadGaps - 1);
    let sum = 0;
    for (let j = start; j <= end; j += 1) sum += headGaps[j];
    avgs[i] = sum / (end - start + 1);
    }

    let signatureHead = 0;
    for (let i = 0; i < numHeadGaps; i += 1) {
    signatureHead += Math.sqrt((headGaps[i] / avgs[i] / numHeadGaps) * weightsAtNote[i])
        * Math.pow(ksArrAtNote[i], 0.25);
    }
    let sumRefHead = 0;
    for (let i = 0; i < numHeadGaps; i += 1) {
    sumRefHead += (headGaps[i] / avgs[i]) * weightsAtNote[i];
    }
    const refSignatureHead = Math.sqrt(sumRefHead);

    const tails = tailSeq.map((n) => n[2]);
    const idxListTails = tails.map((t) => bisectLeft(allCorners, t));
    const nTail = idxListTails.length > 1 ? idxListTails.length - 1 : 0;
    const ksArrAtTail = [];
    const weightsAtTail = [];
    for (let i = 0; i < nTail; i += 1) {
    ksArrAtTail.push(ksArr[idxListTails[i]]);
    weightsAtTail.push(dAll[idxListTails[i]]);
    }

    const tailGaps = [];
    for (let i = 0; i < tails.length - 1; i += 1) {
    tailGaps.push(tails[i + 1] - tails[i]);
    }

    let signatureTail = 0;
    let refSignatureTail = 0;
    if (tails.length > 0 && tails[tails.length - 1] > tails[0] && tailGaps.length > 0) {
    const numTailGaps = tailGaps.length;
    const avgsTail = new Array(numTailGaps);
    for (let i = 0; i < numTailGaps; i += 1) {
            const start = Math.max(0, i - 50);
            const end = Math.min(i + 50, numTailGaps - 1);
            let sum = 0;
            for (let j = start; j <= end; j += 1) sum += tailGaps[j];
            avgsTail[i] = sum / (end - start + 1);
    }
    for (let i = 0; i < numTailGaps; i += 1) {
            signatureTail += Math.sqrt((tailGaps[i] / avgsTail[i] / numTailGaps) * weightsAtTail[i])
        * Math.pow(ksArrAtTail[i], 0.25);
    }
    let sumRefTail = 0;
    for (let i = 0; i < numTailGaps; i += 1) {
            sumRefTail += (tailGaps[i] / avgsTail[i]) * weightsAtTail[i];
    }
    refSignatureTail = Math.sqrt(sumRefTail);
    }

    const numerator = signatureHead * numHeadGaps + signatureTail * tailGaps.length;
    const denominator = refSignatureHead * numHeadGaps + refSignatureTail * tailGaps.length;
    return numerator / denominator / 2 + 0.5;
}

export function calculate(osuText, speedRate = 1.0, odFlag = null, cvtFlag = null, options = {}, parsed = null) {
    const withGraph = options?.withGraph === true;

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
    totalNotes: rawTotalNotes,
    } = preprocessFile(osuText, speedRate, odFlag, cvtFlag, parsed);

    if (status === "Fail") return -1;
    if (status === "NotMania") return -2;
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

    const { CStep, CStepV2, KsStep } = computeCAndKs(K, noteSeq, keyUsage, baseCorners);
    const CArr = stepInterp(allCorners, baseCorners, CStep);
    const CArrV2 = stepInterp(allCorners, baseCorners, CStepV2);
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

    // D2: effectiveWeights uses C_arrV2 by default, C_arr under Classic —
    // cs MACalculator.cs:837-842 `ContainsCL ? C_arr : C_arrV2`, genirx dart :1487.
    const effectiveWeights = (options?.classicMod === true ? CArr : CArrV2).map((c, i) => c * gaps[i]);
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

    let lnLengthTerm = 0;
    for (const [, h, t] of lnSeq) {
    lnLengthTerm += Math.min((t - h), 1000) / 200;
    }
    const totalNotes = noteSeq.length + 0.5 * lnLengthTerm;

    sr *= totalNotes / (totalNotes + 60);
    sr = rescaleHigh(sr);
    sr *= 0.975;

    // PP metrics (dart :1550-1588) — gated on withPpMetrics; never affect star.
    let ppMetrics = null;
    if (options?.withPpMetrics === true) {
    // Spikiness: weightedVariance = (Σ((D⁸ − weightedMean⁸)²·w)/Σw)^(1/8); spikiness = sqrt(var)/mean
    let varianceSumTop = 0;
    for (let i = 0; i < DSorted.length; i += 1) {
            varianceSumTop += Math.pow(Math.pow(DSorted[i], 8) - Math.pow(weightedMean, 8), 2) * wSorted[i];
    }
    const weightedVariance = Math.pow(varianceSumTop / den, 1 / 8);
    const spikiness = den > 0 && weightedMean > 0 ? Math.sqrt(weightedVariance) / weightedMean : 0;

    const switches = computeSwitches(noteSeq, tailSeq, allCorners, KsArr, DAll);
    const variety = computeVariety(noteSeq, noteSeqByColumn, K);
    const accScalar = 0.5 * spikiness + 0.5 * switches;

    ppMetrics = { star: sr, variety, accScalar, totalNotes: rawTotalNotes, spikiness, switches };
    }

    if (withGraph) {
    const DPre = applyProximityEnvelope(allCorners, DAll, noteSeq);
    const DGraph = smoothDForGraph(allCorners, DPre, noteSeq);
    const result = {
            star: sr,
            lnRatio,
            columnCount,
            graph: {
        times: Array.from(allCorners),
        values: DGraph,
            },
    };
    if (ppMetrics) result.ppMetrics = ppMetrics;
    return result;
    }

    if (ppMetrics) {
    return { star: sr, lnRatio, columnCount, ppMetrics };
    }

    return [sr, lnRatio, columnCount];
}
