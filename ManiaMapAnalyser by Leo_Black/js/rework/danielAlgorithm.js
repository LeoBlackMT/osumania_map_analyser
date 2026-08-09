import { OsuFileParser } from "../parser/osuFileParser.js";
import {
    bisectLeft,
    bisectRight,
    cumulativeSum,
    queryCumsum,
    smoothOnCorners,
    interpValues,
    stepInterp,
    gaussianFilter1d,
    rescaleHigh,
    mergeByHead,
    computeCAndKs,
    applyProximityEnvelope,
    smoothDForGraph,
    jackNerfer,
    targetPercentiles,
} from "./reworkMathCore.js";












function preprocessDaniel(osuText, speedRate, _odFlag, parsed = null) {
    // parsed: a shared OsuFileParser instance already processed by the caller.
    // Daniel has no cvtFlag conversion and only reads the parsed chart, so the
    // shared instance is used directly (read-only) when provided.
    const parser = parsed ?? new OsuFileParser(osuText);
    if (!parsed) parser.process();
    const parsedData = parser.getParsedData();

    const lnRatio = Number(parsedData.lnRatio) || 0;
    const columnCount = Number(parsedData.columnCount) || 0;

    if (parsedData.status === "Fail") {
        return {
            status: "Fail",
            x: 0,
            K: 0,
            T: 0,
            noteSeq: [],
            noteSeqByColumn: [],
            lnRatio,
            columnCount,
        };
    }

    if (parsedData.status === "NotMania") {
        return {
            status: "NotMania",
            x: 0,
            K: 0,
            T: 0,
            noteSeq: [],
            noteSeqByColumn: [],
            lnRatio,
            columnCount,
        };
    }

    if (columnCount !== 4) {
        return {
            status: "UnsupportedKeys",
            x: 0,
            K: columnCount,
            T: 0,
            noteSeq: [],
            noteSeqByColumn: [],
            lnRatio,
            columnCount,
        };
    }

    // Keep Daniel port consistent with the original osu_file_parser.py used by Daniel-main.
    // That parser effectively keeps OD fixed to 9 for this algorithm branch.
    const od = 9;

    const timeScale = speedRate !== 0 ? 1 / speedRate : 1;

    const noteSeq = [];
    for (let i = 0; i < parsedData.columns.length; i += 1) {
        const k = parsedData.columns[i];
        let h = parsedData.noteStarts[i];
        h = Math.floor(h * timeScale);
        noteSeq.push([k, h]);
    }

    noteSeq.sort((a, b) => {
        if (a[1] !== b[1]) return a[1] - b[1];
        return a[0] - b[0];
    });

    const K = columnCount;
    const noteSeqByColumn = Array.from({ length: K }, () => []);
    for (const n of noteSeq) {
        const col = n[0];
        if (col >= 0 && col < K) noteSeqByColumn[col].push(n);
    }

    let x = 0.3 * Math.sqrt((64.5 - Math.ceil(od * 3)) / 500);
    x = Math.min(x, 0.6 * (x - 0.09) + 0.09);

    const T = noteSeq.length ? noteSeq[noteSeq.length - 1][1] + 1 : 0;

    return {
        status: "OK",
        x,
        K,
        T,
        noteSeq,
        noteSeqByColumn,
        lnRatio,
        columnCount,
    };
}

function getCorners(T, noteSeq) {
    const cornersBase = new Set();
    for (const [, h] of noteSeq) {
        cornersBase.add(h);
        cornersBase.add(h + 501);
        cornersBase.add(h - 499);
        cornersBase.add(h + 1);
    }
    cornersBase.add(0);
    cornersBase.add(T);

    const baseCorners = [...cornersBase]
        .filter((s) => s >= 0 && s <= T)
        .sort((a, b) => a - b);

    const cornersA = new Set();
    for (const [, h] of noteSeq) {
        cornersA.add(h);
        cornersA.add(h + 1000);
        cornersA.add(h - 1000);
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
        keyUsage[k] = new Uint8Array(baseCorners.length);
    }

    for (const [k, h] of noteSeq) {
        const startTime = Math.max(h - 150, 0);
        const endTime = Math.min(h + 150, T - 1);
        const leftIdx = bisectLeft(baseCorners, startTime);
        const rightIdx = bisectLeft(baseCorners, endTime);
        for (let idx = leftIdx; idx < rightIdx; idx += 1) {
            keyUsage[k][idx] = 1;
        }
    }

    return keyUsage;
}

function getKeyUsage400(K, noteSeq, baseCorners) {
    const keyUsage400 = {};
    for (let k = 0; k < K; k += 1) {
        keyUsage400[k] = new Float64Array(baseCorners.length);
    }

    // Loop-invariant coefficient — hoisted out of the per-note loops.
    const k400Scale = 3.75 / (400 ** 2);

    for (const [k, h] of noteSeq) {
        const left400Idx = bisectLeft(baseCorners, h - 400);
        const centerIdx = bisectLeft(baseCorners, h);
        const right400Idx = bisectLeft(baseCorners, h + 400);

        if (centerIdx >= 0 && centerIdx < baseCorners.length) {
            keyUsage400[k][centerIdx] += 3.75;
        }

        for (let idx = left400Idx; idx < centerIdx; idx += 1) {
            keyUsage400[k][idx] += 3.75 - k400Scale * ((baseCorners[idx] - h) ** 2);
        }

        for (let idx = centerIdx + 1; idx < right400Idx; idx += 1) {
            keyUsage400[k][idx] += 3.75 - k400Scale * ((baseCorners[idx] - h) ** 2);
        }
    }

    return keyUsage400;
}

function computeAnchor(K, keyUsage400, baseCorners) {
    const anchor = new Float64Array(baseCorners.length);

    for (let idx = 0; idx < baseCorners.length; idx += 1) {
        const counts = new Array(K).fill(0).map((_, k) => keyUsage400[k][idx]);
        counts.sort((a, b) => b - a);

        const nonZero = counts.filter((v) => v > 0);
        let raw = 0;
        if (nonZero.length > 1) {
            let walk = 0;
            let maxWalk = 0;
            for (let i = 0; i < nonZero.length - 1; i += 1) {
                const ratio = nonZero[i + 1] / nonZero[i];
                const weight = 1 - 4 * ((0.5 - ratio) ** 2);
                walk += nonZero[i] * weight;
                maxWalk += nonZero[i];
            }
            raw = maxWalk > 0 ? walk / maxWalk : 0;
        }

        anchor[idx] = 1 + Math.min(raw - 0.18, 5 * ((raw - 0.22) ** 3));
    }

    return anchor;
}

function computeJbar(K, x, noteSeqByColumn, baseCorners) {

    // Loop-invariant term (depends only on x) — hoisted out of the per-note loop.
    const xPowQuarter = 0.11 * (x ** 0.25);

    const Jks = {};
    const deltaKs = {};
    for (let k = 0; k < K; k += 1) {
        Jks[k] = new Float64Array(baseCorners.length);
        deltaKs[k] = new Float64Array(baseCorners.length).fill(1e9);
    }

    for (let k = 0; k < K; k += 1) {
        const notes = noteSeqByColumn[k] || [];
        for (let i = 0; i < notes.length - 1; i += 1) {
            const start = notes[i][1];
            const end = notes[i + 1][1];
            if (end <= start) continue;

            const leftIdx = bisectLeft(baseCorners, start);
            const rightIdx = bisectLeft(baseCorners, end);
            if (leftIdx >= rightIdx) continue;

            const delta = 0.001 * (end - start);
            const val = (delta ** -1) * ((delta + xPowQuarter) ** -1) * jackNerfer(delta);

            for (let idx = leftIdx; idx < rightIdx; idx += 1) {
                Jks[k][idx] = val;
                deltaKs[k][idx] = delta;
            }
        }
    }

    const JbarKs = {};
    for (let k = 0; k < K; k += 1) {
        JbarKs[k] = smoothOnCorners(baseCorners, Jks[k], 500, 0.001, "sum");
    }

    const Jbar = new Float64Array(baseCorners.length);
    for (let i = 0; i < baseCorners.length; i += 1) {
        let num = 0;
        let den = 0;
        for (let k = 0; k < K; k += 1) {
            const v = JbarKs[k][i];
            const w = 1 / Math.max(deltaKs[k][i], 1e-9);
            num += (Math.max(v, 0) ** 5) * w;
            den += w;
        }
        Jbar[i] = (num / Math.max(den, 1e-9)) ** 0.2;
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

    const crossCoeff = crossMatrix[K] || new Array(K + 1).fill(1 / (K + 1));
    const Xks = {};
    const fastCross = {};

    for (let k = 0; k < K + 1; k += 1) {
        Xks[k] = new Float64Array(baseCorners.length);
        fastCross[k] = new Float64Array(baseCorners.length);
    }

    for (let k = 0; k < K + 1; k += 1) {
        let notesInPair = [];
        if (k === 0) {
            notesInPair = noteSeqByColumn[0] || [];
        } else if (k === K) {
            notesInPair = noteSeqByColumn[K - 1] || [];
        } else {
            // mergeByHead merges the two time-sorted columns in O(n) with the same
            // tie-break as a stable sort (a[i][1] <= b[j][1] keeps the lower column
            // first, matching V8 Array#sort stability on the concatenation), so it is
            // element-for-element identical to concat+sort. Verified: 508 random +
            // boundary cases incl. equal timestamps -> identical (.omo/evidence/task-7-daniel.txt).
            notesInPair = mergeByHead(noteSeqByColumn[k - 1] || [], noteSeqByColumn[k] || []);
        }

        for (let i = 1; i < notesInPair.length; i += 1) {
            const start = notesInPair[i - 1][1];
            const end = notesInPair[i][1];
            if (end <= start) continue;

            const leftIdx = bisectLeft(baseCorners, start);
            const rightIdx = bisectLeft(baseCorners, end);
            if (rightIdx <= leftIdx) continue;

            const delta = 0.001 * (end - start);
            let val = 0.16 * (Math.max(x, delta) ** -2);

            const leftCols = activeColumns[leftIdx] || [];
            const rightCols = activeColumns[rightIdx] || [];

            const leftInactive = !leftCols.includes(k - 1)
                && !rightCols.includes(k - 1);
            const rightInactive = !leftCols.includes(k)
                && !rightCols.includes(k);

            if (leftInactive || rightInactive) {
                val *= 1 - (crossCoeff[k] ?? 0);
            }

            const fastVal = Math.max(0, 0.4 * (Math.max(delta, 0.06, 0.75 * x) ** -2) - 80);

            for (let idx = leftIdx; idx < rightIdx; idx += 1) {
                Xks[k][idx] = val;
                fastCross[k][idx] = fastVal;
            }
        }
    }

    const XBase = new Float64Array(baseCorners.length);
    for (let i = 0; i < baseCorners.length; i += 1) {
        let sum1 = 0;
        let sum2 = 0;
        for (let k = 0; k < K + 1; k += 1) {
            sum1 += Xks[k][i] * (crossCoeff[k] ?? 0);
        }
        for (let k = 0; k < K; k += 1) {
            const pair = fastCross[k][i] * (crossCoeff[k] ?? 0) * fastCross[k + 1][i] * (crossCoeff[k + 1] ?? 0);
            if (pair > 0) {
                sum2 += Math.sqrt(pair);
            }
        }
        XBase[i] = sum1 + sum2;
    }

    return smoothOnCorners(baseCorners, XBase, 500, 0.001, "sum");
}

function computePbar(x, noteSeq, anchor, baseCorners) {
    const streamBooster = (delta) => {
        const bpm = Math.max(0, Math.min(7.5 / Math.max(delta, 1e-9), 420));
        const primary = 0.10 / (1 + Math.exp(-0.06 * (bpm - 175)));
        const secondary = (bpm >= 200 && bpm <= 350)
            ? 0.30 * (1 - Math.exp(-0.02 * (bpm - 200)))
            : 0;
        return 1 + primary + secondary;
    };

    const PStep = new Float64Array(baseCorners.length);

    // Loop-invariant terms (depend only on x) — hoisted out of the per-note loop.
    const xInv = x ** -1;
    const xSixth = x / 6;
    const xHalf = x / 2;
    const twoThirdsX = (2 * x) / 3;
    const baseInc = (0.08 * xInv * (1 - 24 * xInv * (xSixth ** 2))) ** 0.25;
    const spike = 1000 * ((0.02 * (4 / x - 24)) ** 0.25);

    for (let i = 0; i < noteSeq.length - 1; i += 1) {
        const hL = noteSeq[i][1];
        const hR = noteSeq[i + 1][1];
        const deltaTime = hR - hL;

        if (deltaTime < 1e-9) {
            const leftIdx = bisectLeft(baseCorners, hL);
            const rightIdx = bisectRight(baseCorners, hL);
            for (let idx = leftIdx; idx < rightIdx; idx += 1) {
                PStep[idx] += spike;
            }
            continue;
        }

        const leftIdx = bisectLeft(baseCorners, hL);
        const rightIdx = bisectLeft(baseCorners, hR);
        if (rightIdx <= leftIdx) continue;

        const delta = 0.001 * deltaTime;
        const bVal = streamBooster(delta);

        let inc;
        if (delta < twoThirdsX) {
            inc = (delta ** -1)
                * ((0.08 * xInv * (1 - 24 * xInv * ((delta - xHalf) ** 2))) ** 0.25)
                * Math.max(bVal, 1);
        } else {
            inc = (delta ** -1) * baseInc * Math.max(bVal, 1);
        }

        for (let idx = leftIdx; idx < rightIdx; idx += 1) {
            const boosted = inc * anchor[idx];
            PStep[idx] += Math.min(boosted, Math.max(inc, inc * 2 - 10));
        }
    }

    return smoothOnCorners(baseCorners, PStep, 500, 0.001, "sum");
}

function computeAbar(K, activeColumns, deltaKs, ACorners, baseCorners) {
    const dks = {};
    for (let k = 0; k < K - 1; k += 1) {
        dks[k] = new Float64Array(baseCorners.length);
    }

    for (let i = 0; i < baseCorners.length; i += 1) {
        const cols = activeColumns[i] || [];
        for (let j = 0; j < cols.length - 1; j += 1) {
            const k0 = cols[j];
            const k1 = cols[j + 1];
            dks[k0][i] = Math.abs(deltaKs[k0][i] - deltaKs[k1][i])
                + 0.4 * Math.max(0, Math.max(deltaKs[k0][i], deltaKs[k1][i]) - 0.11);
        }
    }

    const AStep = new Float64Array(ACorners.length).fill(1);

    for (let i = 0; i < ACorners.length; i += 1) {
        let idx = bisectLeft(baseCorners, ACorners[i]);
        idx = Math.max(0, Math.min(idx, baseCorners.length - 1));

        const cols = activeColumns[idx] || [];
        for (let j = 0; j < cols.length - 1; j += 1) {
            const k0 = cols[j];
            const k1 = cols[j + 1];
            const dVal = dks[k0][idx];
            const dk0 = deltaKs[k0][idx];
            const dk1 = deltaKs[k1][idx];

            if (dVal < 0.02) {
                AStep[i] *= Math.min(0.75 + 0.5 * Math.max(dk0, dk1), 1);
            } else if (dVal < 0.07) {
                AStep[i] *= Math.min(0.65 + 5 * dVal + 0.5 * Math.max(dk0, dk1), 1);
            }
        }
    }

    return smoothOnCorners(ACorners, AStep, 250, 1.0, "avg");
}




export function calculateDaniel(osuText, speedRate = 1.0, odFlag = null, options = {}, parsed = null) {
    const withGraph = options?.withGraph === true;

    const {
        status,
        x,
        K,
        T,
        noteSeq,
        noteSeqByColumn,
        lnRatio,
        columnCount,
    } = preprocessDaniel(osuText, speedRate, odFlag, parsed);

    if (status === "Fail") return -1;
    if (status === "NotMania") return -2;
    if (status === "UnsupportedKeys") return -3;
    if (!noteSeq.length || K <= 0 || T <= 0) return -1;

    const { allCorners, baseCorners, ACorners } = getCorners(T, noteSeq);

    const keyUsage = getKeyUsage(K, T, noteSeq, baseCorners);
    const activeColumns = baseCorners.map((_, i) => {
        const active = [];
        for (let k = 0; k < K; k += 1) {
            if (keyUsage[k][i]) active.push(k);
        }
        return active;
    });

    const keyUsage400 = getKeyUsage400(K, noteSeq, baseCorners);
    const anchor = computeAnchor(K, keyUsage400, baseCorners);

    const { deltaKs, Jbar: JbarBase } = computeJbar(K, x, noteSeqByColumn, baseCorners);
    const Jbar = interpValues(allCorners, baseCorners, JbarBase);

    const XbarBase = computeXbar(K, x, noteSeqByColumn, activeColumns, baseCorners);
    const Xbar = interpValues(allCorners, baseCorners, XbarBase);

    const PbarBase = computePbar(x, noteSeq, anchor, baseCorners);
    const Pbar = interpValues(allCorners, baseCorners, PbarBase);

    const AbarBase = computeAbar(K, activeColumns, deltaKs, ACorners, baseCorners);
    const Abar = interpValues(allCorners, ACorners, AbarBase);

    const { CStep, KsStep } = computeCAndKs(K, noteSeq, keyUsage, baseCorners);
    const CArr = stepInterp(allCorners, baseCorners, CStep);
    const KsArr = stepInterp(allCorners, baseCorners, KsStep);

    const DAll = new Array(allCorners.length).fill(0);
    for (let i = 0; i < allCorners.length; i += 1) {
        const leftPart = 0.4 * ((Abar[i] ** (3 / KsArr[i]) * Math.min(Jbar[i], 8 + 0.85 * Jbar[i])) ** 1.5);
        const rightPart = 0.6 * ((Abar[i] ** (2 / 3) * (0.8 * Pbar[i])) ** 1.5);
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
    if (!Number.isFinite(totalWeight) || totalWeight <= 0) {
        return withGraph
            ? {
                star: 0,
                lnRatio,
                columnCount,
                graph: {
                    times: Array.from(allCorners),
                    values: new Array(allCorners.length).fill(0),
                },
            }
            : [0, lnRatio, columnCount];
    }

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
    const weightedMean = (num / Math.max(den, 1e-9)) ** 0.2;

    let sr = (0.88 * percentile93) * 0.25 + (0.94 * percentile83) * 0.2 + weightedMean * 0.55;
    sr *= noteSeq.length / (noteSeq.length + 60);
    sr = rescaleHigh(sr) * 0.975;

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
