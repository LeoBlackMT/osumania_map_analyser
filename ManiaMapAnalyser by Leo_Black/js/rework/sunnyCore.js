// sunnyCore.js —— Sunny 与 SunnyWindow 算法共享的核心谱面与几何计算纯函数

import { OsuFileParser } from "../parser/osuFileParser.js";
import {
    bisectLeft,
    bisectRight,
    smoothOnCorners,
    mergeByHead,
} from "./reworkMathCore.js";

export const crossMatrix = {
    4: [0.1, 0.4, 0.4, 0.1, 0],
    5: [0.1, 0.35, 0.35, 0.1, 0, 0],
    6: [0.08, 0.28, 0.28, 0.28, 0.08, 0, 0],
    7: [0.05, 0.25, 0.25, 0.25, 0.25, 0.05, 0, 0],
    8: [0.04, 0.2, 0.2, 0.2, 0.2, 0.2, 0.04, 0, 0],
    9: [0.03, 0.15, 0.15, 0.15, 0.15, 0.15, 0.15, 0.03, 0, 0],
    10: [0.02, 0.12, 0.12, 0.12, 0.12, 0.12, 0.12, 0.12, 0.02, 0, 0],
};

export function interpSingle(newX, oldX, oldVals) {
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

export function findNextNoteInColumn(note, times, noteSeqByColumn) {
    const k = note[0];
    const h = note[1];
    const idx = bisectLeft(times, h);
    return idx + 1 < noteSeqByColumn[k].length ? noteSeqByColumn[k][idx + 1] : [0, 1e9, 1e9];
}

export function cloneOsuParser(src) {
    const p = new OsuFileParser("");
    p.od = src.od;
    p.columnCount = src.columnCount;
    p.columns = [...src.columns];
    p.noteStarts = [...src.noteStarts];
    p.noteEnds = [...src.noteEnds];
    p.noteTypes = [...src.noteTypes];
    p.gameMode = src.gameMode;
    p.status = src.status;
    p.lnRatio = src.lnRatio;
    p.metaData = { ...src.metaData };
    p.breaks = src.breaks.map((b) => [...b]);
    p.objectIntervals = src.objectIntervals.map((o) => [...o]);
    p.timingPoints = src.timingPoints.map((tp) => [...tp]);
    return p;
}

export function getCorners(T, noteSeq) {
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

export function getKeyUsage(K, T, noteSeq, baseCorners) {
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

export function computeAnchor(K, keyUsage400, baseCorners) {
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

export function lnBodiesCountSparseRepresentation(lnSeq, T) {
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

    return { points, values, cumsum };
}

export function lnSum(tStart, tEnd, sparseRep) {
    const { points, values, cumsum } = sparseRep;
    if (tEnd <= tStart) return 0;
    if (tEnd <= points[0] || tStart >= points[points.length - 1]) return 0;

    const s = Math.max(tStart, points[0]);
    const e = Math.min(tEnd, points[points.length - 1]);

    const i = Math.max(0, bisectRight(points, s) - 1);
    const j = Math.max(0, bisectRight(points, e) - 1);

    if (i === j) {
        return (e - s) * (values[i] || 0);
    }

    let total = (points[i + 1] - s) * (values[i] || 0);
    total += cumsum[j] - cumsum[i + 1];
    total += (e - points[j]) * (values[j] || 0);
    return total;
}

export function computeXbar(K, x, noteSeqByColumn, activeColumns, baseCorners) {
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

export function computeAbar(K, activeColumns, deltaKs, ACorners, baseCorners) {
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

export function computeRbar(K, x, noteSeqByColumn, tailSeq, baseCorners) {
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
