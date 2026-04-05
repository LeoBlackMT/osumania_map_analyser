import { OsuFileParser } from "../parser/osuFileParser.js";

const BREAK_ZERO_THRESHOLD_MS = 400;
const GRAPH_RESAMPLE_INTERVAL_MS = 100;
const SMOOTH_SIGMA_MS = 800;

function bisectLeft(arr, target) {
    let lo = 0;
    let hi = arr.length;
    while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (arr[mid] < target) lo = mid + 1;
    else hi = mid;
    }
    return lo;
}

function bisectRight(arr, target) {
    let lo = 0;
    let hi = arr.length;
    while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (arr[mid] <= target) lo = mid + 1;
    else hi = mid;
    }
    return lo;
}

function cumulativeSum(x, f) {
    const F = new Float64Array(x.length);
    for (let i = 1; i < x.length; i += 1) {
    F[i] = F[i - 1] + f[i - 1] * (x[i] - x[i - 1]);
    }
    return F;
}

function queryCumsum(q, x, F, f) {
    if (q <= x[0]) return 0;
    if (q >= x[x.length - 1]) return F[F.length - 1];
    const i = bisectRight(x, q) - 1;
    return F[i] + f[i] * (q - x[i]);
}

function smoothOnCorners(x, f, window, scale = 1.0, mode = "sum") {
    const F = cumulativeSum(x, f);
    const g = new Float64Array(f.length);
    for (let i = 0; i < x.length; i += 1) {
    const s = x[i];
    const a = Math.max(s - window, x[0]);
    const b = Math.min(s + window, x[x.length - 1]);
    const val = queryCumsum(b, x, F, f) - queryCumsum(a, x, F, f);
    if (mode === "avg") {
            g[i] = b - a > 0 ? val / (b - a) : 0;
    } else {
            g[i] = scale * val;
    }
    }
    return g;
}

function interpSingle(newX, oldX, oldVals) {
    if (newX <= oldX[0]) return oldVals[0];
    if (newX >= oldX[oldX.length - 1]) return oldVals[oldVals.length - 1];
    const idx = bisectRight(oldX, newX) - 1;
    const x0 = oldX[idx];
    const x1 = oldX[idx + 1];
    const y0 = oldVals[idx];
    const y1 = oldVals[idx + 1];
    if (x1 === x0) return y0;
    const t = (newX - x0) / (x1 - x0);
    return y0 + t * (y1 - y0);
}

function interpValues(newX, oldX, oldVals) {
    const out = new Float64Array(newX.length);
    let idx = 0;

    for (let i = 0; i < newX.length; i += 1) {
    const x = newX[i];

    if (x <= oldX[0]) {
        out[i] = oldVals[0];
        continue;
    }
    if (x >= oldX[oldX.length - 1]) {
        out[i] = oldVals[oldVals.length - 1];
        continue;
    }

    while (idx + 1 < oldX.length && oldX[idx + 1] < x) {
        idx += 1;
    }

    const x0 = oldX[idx];
    const x1 = oldX[idx + 1];
    const y0 = oldVals[idx];
    const y1 = oldVals[idx + 1];

    if (x1 === x0) {
        out[i] = y0;
        continue;
    }

    const t = (x - x0) / (x1 - x0);
    out[i] = y0 + t * (y1 - y0);
    }

    return out;
}

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

function gaussianFilter1d(data, sigmaSamples) {
    if (!Number.isFinite(sigmaSamples) || sigmaSamples <= 0) {
    return Array.from(data);
    }

    const radius = Math.max(1, Math.trunc(4 * sigmaSamples + 0.5));
    const kernelSize = radius * 2 + 1;
    const kernel = new Float64Array(kernelSize);
    let kernelSum = 0;

    for (let i = -radius; i <= radius; i += 1) {
    const v = Math.exp(-0.5 * ((i / sigmaSamples) ** 2));
    kernel[i + radius] = v;
    kernelSum += v;
    }
    for (let i = 0; i < kernelSize; i += 1) {
    kernel[i] /= kernelSum;
    }

    const padded = new Float64Array(data.length + radius * 2);
    for (let i = 0; i < data.length; i += 1) {
    padded[i + radius] = data[i];
    }

    const out = new Float64Array(data.length);
    for (let i = 0; i < data.length; i += 1) {
    let acc = 0;
    for (let k = 0; k < kernelSize; k += 1) {
        acc += padded[i + k] * kernel[k];
    }
    out[i] = acc;
    }
    return Array.from(out);
}

function rescaleHigh(sr) {
    if (sr <= 9) return sr;
    return 9 + (sr - 9) * (1 / 1.2);
}

function mergeByHead(a, b) {
    const result = [];
    let i = 0;
    let j = 0;
    while (i < a.length && j < b.length) {
    if (a[i][1] <= b[j][1]) {
            result.push(a[i]);
            i += 1;
    } else {
            result.push(b[j]);
            j += 1;
    }
    }
    while (i < a.length) {
    result.push(a[i]);
    i += 1;
    }
    while (j < b.length) {
    result.push(b[j]);
    j += 1;
    }
    return result;
}

function findNextNoteInColumn(note, times, noteSeqByColumn) {
    const k = note[0];
    const h = note[1];
    const idx = bisectLeft(times, h);
    return idx + 1 < noteSeqByColumn[k].length ? noteSeqByColumn[k][idx + 1] : [0, 1e9, 1e9];
}

function preprocessFile(osuText, speedRate, odFlag, cvtFlag) {
    const pObj = new OsuFileParser(osuText);
    pObj.process();
    let p = pObj.getParsedData();
    let lnRatio = p.lnRatio;

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

    pObj.noteTimes = pObj.getNoteTimes();
    pObj.objectIntervals = pObj.getObjectIntervals();
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
    };
}

function getCorners(T, noteSeq) {
    const cornersBase = new Set();
    for (const [, h, t] of noteSeq) {
    cornersBase.add(h);
    if (t >= 0) cornersBase.add(t);
    }

    const copyBase = [...cornersBase];
    for (const s of copyBase) {
    cornersBase.add(s + 501);
    cornersBase.add(s - 499);
    cornersBase.add(s + 1);
    }
    cornersBase.add(0);
    cornersBase.add(T);

    const baseCorners = [...cornersBase]
    .filter((s) => s >= 0 && s <= T)
    .sort((a, b) => a - b);

    const cornersA = new Set();
    for (const [, h, t] of noteSeq) {
    cornersA.add(h);
    if (t >= 0) cornersA.add(t);
    }

    const copyA = [...cornersA];
    for (const s of copyA) {
    cornersA.add(s + 1000);
    cornersA.add(s - 1000);
    }
    cornersA.add(0);
    cornersA.add(T);

    const ACorners = [...cornersA]
    .filter((s) => s >= 0 && s <= T)
    .sort((a, b) => a - b);

    const allCorners = [...new Set([...baseCorners, ...ACorners])].sort((a, b) => a - b);

    return { allCorners, baseCorners, ACorners };
}

function getKeyUsage(K, T, noteSeq, baseCorners) {
    const keyUsage = {};
    for (let k = 0; k < K; k += 1) {
    keyUsage[k] = new Array(baseCorners.length).fill(false);
    }

    for (const [k, h, t] of noteSeq) {
    const startTime = Math.max(h - 150, 0);
    const endTime = t < 0 ? h + 150 : Math.min(t + 150, T - 1);
    const leftIdx = bisectLeft(baseCorners, startTime);
    const rightIdx = bisectLeft(baseCorners, endTime);
    for (let idx = leftIdx; idx < rightIdx; idx += 1) {
            keyUsage[k][idx] = true;
    }
    }

    return keyUsage;
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

function computeAnchor(K, keyUsage400, baseCorners) {
    const anchor = new Array(baseCorners.length).fill(0);

    for (let idx = 0; idx < baseCorners.length; idx += 1) {
    const counts = new Array(K).fill(0).map((_, k) => keyUsage400[k][idx]);
    counts.sort((a, b) => b - a);

    const nonZeroCounts = counts.filter((v) => v !== 0);
    if (nonZeroCounts.length > 1) {
            let walk = 0;
            let maxWalk = 0;
            for (let i = 0; i < nonZeroCounts.length - 1; i += 1) {
        walk += nonZeroCounts[i] * (1 - 4 * ((0.5 - (nonZeroCounts[i + 1] / nonZeroCounts[i])) ** 2));
        maxWalk += nonZeroCounts[i];
            }
            anchor[idx] = walk / maxWalk;
    } else {
            anchor[idx] = 0;
    }
    }

    for (let i = 0; i < anchor.length; i += 1) {
    anchor[i] = 1 + Math.min(anchor[i] - 0.18, 5 * ((anchor[i] - 0.22) ** 3));
    }
    return anchor;
}

function lnBodiesCountSparseRepresentation(lnSeq, T) {
    const diff = new Map();

    for (const [, h, t] of lnSeq) {
    const t0 = Math.min(h + 60, t);
    const t1 = Math.min(h + 120, t);

    diff.set(t0, (diff.get(t0) || 0) + 1.3);
    diff.set(t1, (diff.get(t1) || 0) + (-1.3 + 1));
    diff.set(t, (diff.get(t) || 0) - 1);
    }

    const pointsSet = new Set([0, T]);
    for (const k of diff.keys()) pointsSet.add(k);
    const points = [...pointsSet].sort((a, b) => a - b);

    const values = [];
    const cumsum = [0];
    let curr = 0;

    for (let i = 0; i < points.length - 1; i += 1) {
    const t = points[i];
    if (diff.has(t)) curr += diff.get(t);

    const v = Math.min(curr, 2.5 + 0.5 * curr);
    values.push(v);

    const segLength = points[i + 1] - points[i];
    cumsum.push(cumsum[cumsum.length - 1] + segLength * v);
    }

    return { points, cumsum, values };
}

function lnSum(a, b, lnRep) {
    const { points, cumsum, values } = lnRep;

    const i = bisectRight(points, a) - 1;
    const j = bisectRight(points, b) - 1;

    if (i === j) {
    return (b - a) * values[i];
    }

    let total = 0;
    total += (points[i + 1] - a) * values[i];
    total += cumsum[j] - cumsum[i + 1];
    total += (b - points[j]) * values[j];
    return total;
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

function computeXbar(K, x, noteSeqByColumn, activeColumns, baseCorners) {
    const crossMatrix = [
    [-1],
    [0.075, 0.075],
    [0.125, 0.05, 0.125],
    [0.125, 0.125, 0.125, 0.125],
    [0.175, 0.25, 0.05, 0.25, 0.175],
    [0.175, 0.25, 0.175, 0.175, 0.25, 0.175],
    [0.225, 0.35, 0.25, 0.05, 0.25, 0.35, 0.225],
    [0.225, 0.35, 0.25, 0.225, 0.225, 0.25, 0.35, 0.225],
    [0.275, 0.45, 0.35, 0.25, 0.05, 0.25, 0.35, 0.45, 0.275],
    [0.275, 0.45, 0.35, 0.25, 0.275, 0.275, 0.25, 0.35, 0.45, 0.275],
    [0.325, 0.55, 0.45, 0.35, 0.25, 0.05, 0.25, 0.35, 0.45, 0.55, 0.325],
    ];

    if (K < 1 || K > 10) {
    return new Array(baseCorners.length).fill(0);
    }

    const Xks = {};
    const fastCross = {};
    for (let k = 0; k <= K; k += 1) {
    Xks[k] = new Array(baseCorners.length).fill(0);
    fastCross[k] = new Array(baseCorners.length).fill(0);
    }

    const crossCoeff = crossMatrix[K];

    for (let k = 0; k <= K; k += 1) {
    let notesInPair = [];
    if (k === 0) {
            notesInPair = noteSeqByColumn[0] || [];
    } else if (k === K) {
            notesInPair = noteSeqByColumn[K - 1] || [];
    } else {
            notesInPair = mergeByHead(noteSeqByColumn[k - 1] || [], noteSeqByColumn[k] || []);
    }

    for (let i = 1; i < notesInPair.length; i += 1) {
            const start = notesInPair[i - 1][1];
            const end = notesInPair[i][1];
            const idxStart = bisectLeft(baseCorners, start);
            const idxEnd = bisectLeft(baseCorners, end);
            if (idxStart >= idxEnd) continue;

            const delta = 0.001 * (end - start);
            let val = 0.16 * (Math.max(x, delta) ** -2);

            const leftActive = activeColumns[Math.min(idxStart, activeColumns.length - 1)] || [];
            const rightActive = activeColumns[Math.min(idxEnd, activeColumns.length - 1)] || [];

            if (
        ((!leftActive.includes(k - 1)) && (!rightActive.includes(k - 1))) ||
        ((!leftActive.includes(k)) && (!rightActive.includes(k)))
            ) {
        val *= (1 - crossCoeff[k]);
            }

            const fast = Math.max(0, 0.4 * (Math.max(delta, 0.06, 0.75 * x) ** -2) - 80);
            for (let idx = idxStart; idx < idxEnd; idx += 1) {
        Xks[k][idx] = val;
        fastCross[k][idx] = fast;
            }
    }
    }

    const xBase = new Array(baseCorners.length).fill(0);
    for (let i = 0; i < baseCorners.length; i += 1) {
    let sum1 = 0;
    for (let k = 0; k <= K; k += 1) {
            sum1 += Xks[k][i] * crossCoeff[k];
    }

    let sum2 = 0;
    for (let k = 0; k < K; k += 1) {
            sum2 += Math.sqrt(
        fastCross[k][i] * crossCoeff[k] * fastCross[k + 1][i] * crossCoeff[k + 1]
            );
    }

    xBase[i] = sum1 + sum2;
    }

    return smoothOnCorners(baseCorners, xBase, 500, 0.001, "sum");
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

function computeAbar(K, activeColumns, deltaKs, ACorners, baseCorners) {
    const dks = {};
    for (let k = 0; k < K - 1; k += 1) {
    dks[k] = new Array(baseCorners.length).fill(0);
    }

    for (let i = 0; i < baseCorners.length; i += 1) {
    const cols = activeColumns[i] || [];
    for (let j = 0; j < cols.length - 1; j += 1) {
            const k0 = cols[j];
            const k1 = cols[j + 1];
            dks[k0][i] = Math.abs(deltaKs[k0][i] - deltaKs[k1][i]) + 0.4 * Math.max(0, Math.max(deltaKs[k0][i], deltaKs[k1][i]) - 0.11);
    }
    }

    const aStep = new Array(ACorners.length).fill(1);

    for (let i = 0; i < ACorners.length; i += 1) {
    const s = ACorners[i];
    let idx = bisectLeft(baseCorners, s);
    if (idx >= baseCorners.length) idx = baseCorners.length - 1;

    const cols = activeColumns[idx] || [];
    for (let j = 0; j < cols.length - 1; j += 1) {
            const k0 = cols[j];
            const k1 = cols[j + 1];
            const dVal = dks[k0][idx];

            if (dVal < 0.02) {
        aStep[i] *= Math.min(0.75 + 0.5 * Math.max(deltaKs[k0][idx], deltaKs[k1][idx]), 1);
            } else if (dVal < 0.07) {
        aStep[i] *= Math.min(0.65 + 5 * dVal + 0.5 * Math.max(deltaKs[k0][idx], deltaKs[k1][idx]), 1);
            }
    }
    }

    return smoothOnCorners(ACorners, aStep, 250, 1, "avg");
}

function computeRbar(K, x, noteSeqByColumn, tailSeq, baseCorners) {
    const RStep = new Array(baseCorners.length).fill(0);

    const timesByColumn = {};
    for (let i = 0; i < K; i += 1) {
    timesByColumn[i] = (noteSeqByColumn[i] || []).map((note) => note[1]);
    }

    const IList = [];
    for (let i = 0; i < tailSeq.length; i += 1) {
    const [k, hI, tI] = tailSeq[i];
    const [, hJ] = findNextNoteInColumn([k, hI, tI], timesByColumn[k], noteSeqByColumn);
    const I_h = 0.001 * Math.abs(tI - hI - 80) / x;
    const I_t = 0.001 * Math.abs(hJ - tI - 80) / x;
    IList.push(2 / (2 + Math.exp(-5 * (I_h - 0.75)) + Math.exp(-5 * (I_t - 0.75))));
    }

    for (let i = 0; i < tailSeq.length - 1; i += 1) {
    const tStart = tailSeq[i][2];
    const tEnd = tailSeq[i + 1][2];

    const leftIdx = bisectLeft(baseCorners, tStart);
    const rightIdx = bisectLeft(baseCorners, tEnd);
    if (leftIdx >= rightIdx) continue;

    const deltaR = 0.001 * (tEnd - tStart);
    const rValue = 0.08 * (deltaR ** -0.5) * (x ** -1) * (1 + 0.8 * (IList[i] + IList[i + 1]));

    for (let idx = leftIdx; idx < rightIdx; idx += 1) {
            RStep[idx] = rValue;
    }
    }

    return smoothOnCorners(baseCorners, RStep, 500, 0.001, "sum");
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

function applyProximityEnvelope(allCorners, DAll, noteSeq) {
    if (!noteSeq.length) {
    return Array.from(DAll);
    }

    const noteTimes = noteSeq
    .map((n) => Number(n[1]))
    .filter((v) => Number.isFinite(v))
    .sort((a, b) => a - b);

    if (!noteTimes.length) {
    return Array.from(DAll);
    }

    const proximityFadeMs = 500;
    const out = new Float64Array(allCorners.length);
    for (let i = 0; i < allCorners.length; i += 1) {
    const t = allCorners[i];
    const idx = bisectLeft(noteTimes, t);
    const after = idx < noteTimes.length ? Math.abs(noteTimes[idx] - t) : Number.POSITIVE_INFINITY;
    const before = idx > 0 ? Math.abs(noteTimes[idx - 1] - t) : Number.POSITIVE_INFINITY;
    const d = Math.min(after, before);
    const ratio = Math.max(0, Math.min(d / proximityFadeMs, 1));
    const envelope = 0.5 * (1 + Math.cos(Math.PI * ratio));
    out[i] = DAll[i] * envelope;
    }
    return Array.from(out);
}

function smoothDForGraph(allCorners, DAll, noteSeq) {
    if (!allCorners.length || !DAll.length) {
    return [];
    }

    const tStart = allCorners[0];
    const tEnd = allCorners[allCorners.length - 1];
    const uniformTimes = [];
    for (let t = tStart; t <= tEnd + GRAPH_RESAMPLE_INTERVAL_MS; t += GRAPH_RESAMPLE_INTERVAL_MS) {
    uniformTimes.push(t);
    }

    const noteTimes = noteSeq
    .map((n) => Number(n[1]))
    .filter((v) => Number.isFinite(v))
    .sort((a, b) => a - b);

    const uniformD = interpValues(uniformTimes, allCorners, DAll);

    if (noteTimes.length) {
    for (let i = 0; i < uniformTimes.length; i += 1) {
            const t = uniformTimes[i];
            const idx = bisectLeft(noteTimes, t);
            const after = idx < noteTimes.length ? Math.abs(noteTimes[idx] - t) : Number.POSITIVE_INFINITY;
            const before = idx > 0 ? Math.abs(noteTimes[idx - 1] - t) : Number.POSITIVE_INFINITY;
            const dist = Math.min(after, before);
            if (dist > BREAK_ZERO_THRESHOLD_MS) {
        uniformD[i] = 0;
            }
    }
    }

    const sigmaSamples = SMOOTH_SIGMA_MS / GRAPH_RESAMPLE_INTERVAL_MS;
    const smoothed = gaussianFilter1d(uniformD, sigmaSamples);

    if (noteTimes.length) {
    for (let i = 0; i < uniformTimes.length; i += 1) {
            const t = uniformTimes[i];
            const idx = bisectLeft(noteTimes, t);
            const after = idx < noteTimes.length ? Math.abs(noteTimes[idx] - t) : Number.POSITIVE_INFINITY;
            const before = idx > 0 ? Math.abs(noteTimes[idx - 1] - t) : Number.POSITIVE_INFINITY;
            const dist = Math.min(after, before);
            if (dist > BREAK_ZERO_THRESHOLD_MS) {
        smoothed[i] = 0;
            }
    }
    }

    return Array.from(interpValues(allCorners, uniformTimes, smoothed));
}

export function calculate(osuText, speedRate = 1.0, odFlag = null, cvtFlag = null, options = {}) {
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
    } = preprocessFile(osuText, speedRate, odFlag, cvtFlag);

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

    if (withGraph) {
    const DPre = applyProximityEnvelope(allCorners, DAll, noteSeq);
    const DGraph = smoothDForGraph(allCorners, DPre, noteSeq);
    return {
            star: sr,
            lnRatio,
            columnCount,
            graph: {
        times: Array.from(allCorners),
        values: DGraph,
            },
    };
    }

    return [sr, lnRatio, columnCount];
}
