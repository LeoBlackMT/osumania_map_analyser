// aleju03 结构特征提取（移植自 mania-hub `live-backend/src/dan/dan-estimator/features.ts`）
//
// 逐行移植 `extractDanFeatures`，数值与求值顺序未改动。适配点：
//  - 输入 map 为普通对象 `{ notes, keyCount, totalLength }`，其中
//    `notes = [{ column, time, endTime, isHold }]`（我们的 OsuFileParser 形态经适配器转换）。
//  - **未移植 `offGridRowShare` / `gapSnapDivisor` / `SNAP_DIVISORS`**：它们需要 timing 表，
//    而 LN 路径的距离函数、12 个结构 floor/compression 与回归兜底都不读该指标
//    （读取面：holdRatio/lnDensity/lnOverlapPressure/lnReleasePressure/lnChordPressure/
//    lnHoldDurationP90/chordRatio/peakNps5s/sustainedNps10s/noteCount/durationMs/
//    rowIntervalEntropy/patternVariety/fastRowRatio）。若将来移植 RC 家族评分，需要补回。
// 共享纯函数：禁止 window/document，禁止 import js/app/。

import {
    average,
    bucketEntropy,
    bucketValues,
    countInWindow,
    quantile,
    quantiles,
    raoQuadraticEntropyLog,
    strainSpikiness,
} from "./math.js";

function getRatedNotes(map, rate) {
    const notes = [];
    for (const note of map.notes) {
        if (note.column < 0 || note.column >= map.keyCount) continue;
        notes.push(rate === 1 ? note : {
            ...note,
            time: note.time / rate,
            endTime: note.endTime / rate,
        });
    }
    return notes;
}

function groupNotesByTime(notes) {
    const rows = new Map();
    for (const note of notes) {
        const row = rows.get(note.time);
        if (row) row.push(note);
        else rows.set(note.time, [note]);
    }
    return [...rows.entries()].sort((a, b) => a[0] - b[0]);
}

function bitCount(value) {
    let count = 0;
    let remaining = value;
    while (remaining > 0) {
        count += remaining & 1;
        remaining >>= 1;
    }
    return count;
}

function numericNgramRepeatRatio(values, size, base) {
    if (values.length < size + 1) return 0;

    const seen = new Set();
    let repeated = 0;
    let total = 0;
    for (let index = 0; index <= values.length - size; index++) {
        let key = 0;
        for (let offset = 0; offset < size; offset++) {
            key = key * base + values[index + offset];
        }
        if (seen.has(key)) repeated++;
        else seen.add(key);
        total++;
    }

    return total ? repeated / total : 0;
}

function adjacentNgramRepeatRatio(values, size) {
    if (values.length < size * 2) return 0;

    let repeated = 0;
    let total = 0;
    for (let index = size; index <= values.length - size; index++) {
        total++;
        let matches = true;
        for (let offset = 0; offset < size; offset++) {
            if (values[index + offset] !== values[index - size + offset]) {
                matches = false;
                break;
            }
        }
        if (matches) repeated++;
    }

    return total ? repeated / total : 0;
}

function sampledWindowNps(noteTimes, windowMs, durationMs) {
    if (noteTimes.length === 0 || durationMs <= 0) return [];

    const samples = [];
    let left = 0;
    let right = 0;
    for (let start = 0; start <= durationMs; start += 1000) {
        while (left < noteTimes.length && noteTimes[left] < start) left++;
        while (right < noteTimes.length && noteTimes[right] < start + windowMs) right++;
        samples.push((right - left) / (windowMs / 1000));
    }
    return samples;
}

export function extractDanFeatures(map, input, rate) {
    const notes = getRatedNotes(map, rate);
    const warnings = [];
    const noteTimes = [];
    const releaseTimes = [];
    const holdDurations = [];
    const holdEvents = [];
    let holdNoteCount = 0;
    let totalHoldMs = 0;
    let noteTimesSorted = true;
    let releaseTimesSorted = true;
    let previousNoteTime = -Infinity;
    let previousReleaseTime = -Infinity;

    for (const note of notes) {
        if (note.time < previousNoteTime) noteTimesSorted = false;
        previousNoteTime = note.time;
        noteTimes.push(note.time);

        if (note.isHold) {
            holdNoteCount++;
            if (note.endTime > note.time) {
                const holdDuration = note.endTime - note.time;
                holdDurations.push(holdDuration);
                totalHoldMs += holdDuration;
                if (note.endTime < previousReleaseTime) releaseTimesSorted = false;
                previousReleaseTime = note.endTime;
                releaseTimes.push(note.endTime);
                holdEvents.push({ time: note.time, delta: 1 }, { time: note.endTime, delta: -1 });
            }
        }
    }

    if (!noteTimesSorted) noteTimes.sort((a, b) => a - b);
    if (!releaseTimesSorted) releaseTimes.sort((a, b) => a - b);
    const durationMs = Math.max(input.totalLength ? (input.totalLength * 1000) / rate : 0, map.totalLength / rate, noteTimes.at(-1) ?? 0);

    if (notes.length < 50 || durationMs <= 0) {
        warnings.push("This map has very little note data, so the estimate is low confidence.");
    }

    const orderedRows = groupNotesByTime(notes);
    const holdRatio = notes.length ? holdNoteCount / notes.length : 0;

    const lastByColumn = Array.from({ length: Math.max(1, map.keyCount) }, () => -Infinity);
    const jackValues = [];
    const streamValues = [];
    const jumpstreamValues = [];
    const rowDensities = [];
    const rowDensityWeights = [];
    const rowIntervals = [];
    const rowRates = [];
    const columnIntervals = [];
    const columnRates = [];
    const tailIntervals = [];
    const tailRates = [];
    const columnCounts = Array.from({ length: Math.max(1, map.keyCount) }, () => 0);
    let chordRows = 0;
    let twoNoteChordRows = 0;
    let holdRows = 0;
    let lnChordRows = 0;
    let longGapCount = 0;
    let longGapMs = 0;
    let fastRowCount = 0;
    let directionChanges = 0;
    let previousColumn = null;
    let previousDirection = 0;
    let previousChordSize = 0;
    let chordSizeChanges = 0;
    let previousRowTime = null;
    let previousRowMask = null;
    let rowMaskTwoBack = null;
    let repeatedRowPatterns = 0;
    let alternatingRowPatterns = 0;
    let chordPairCount = 0;
    let chordPairOverlapCount = 0;
    let adjacentColumnRehitNotes = 0;
    let twoBackColumnRehitNotes = 0;
    let rowPatternChangeSum = 0;
    const rowMasks = [];
    const rowTimes = [];
    const rowSignatures = [];
    const rowMaskBase = Math.max(32, 2 ** Math.max(1, map.keyCount) + 1);
    const rowSignatureBase = rowMaskBase * 256;

    for (const [time, rowNotes] of orderedRows) {
        const columns = [];
        let rowMask = 0;
        let rowHasHold = false;
        for (const note of rowNotes) {
            columns.push(note.column);
            rowMask |= 1 << note.column;
            if (note.isHold) rowHasHold = true;
        }
        columns.sort((a, b) => a - b);

        if (columns.length >= 2) chordRows++;
        if (columns.length === 2) twoNoteChordRows++;
        if (rowHasHold) {
            holdRows++;
            if (columns.length >= 2) lnChordRows++;
        }

        // Chord-jack repetition: of adjacent chord rows (<1s apart, both >= 2
        // notes), how many re-hit a column. True chordjack repeats columns on
        // consecutive chords; dense bracket/jumpstream alternates hands and
        // barely overlaps at the same chord density. Must run before the
        // previous-row state updates below.
        if (
            previousRowMask != null && previousChordSize >= 2 && columns.length >= 2
            && previousRowTime != null && time - previousRowTime < 1000
        ) {
            chordPairCount++;
            if ((rowMask & previousRowMask) !== 0) chordPairOverlapCount++;
        }

        rowMasks.push(rowMask);
        rowTimes.push(time);
        if (previousRowMask != null) {
            if (previousRowTime != null && time - previousRowTime <= 500) {
                adjacentColumnRehitNotes += bitCount(rowMask & previousRowMask);
            }
            if (rowMask === previousRowMask) repeatedRowPatterns++;
            rowPatternChangeSum += bitCount(rowMask ^ previousRowMask) / Math.max(1, map.keyCount);
        }
        if (rowMaskTwoBack != null && rowTimes.length >= 3 && time - rowTimes[rowTimes.length - 3] <= 500) {
            twoBackColumnRehitNotes += bitCount(rowMask & rowMaskTwoBack);
        }
        if (rowMaskTwoBack != null && rowMask === rowMaskTwoBack) alternatingRowPatterns++;
        rowMaskTwoBack = previousRowMask;
        previousRowMask = rowMask;
        for (const column of columns) {
            columnCounts[column]++;
        }
        if (previousChordSize && previousChordSize !== columns.length) chordSizeChanges++;
        let intervalBucket = 0;
        if (previousRowTime != null) {
            const rowDelta = time - previousRowTime;
            if (rowDelta >= 2500) {
                longGapCount++;
                longGapMs += rowDelta;
            }
            if (rowDelta > 0 && rowDelta < 1200) {
                rowIntervals.push(rowDelta);
                rowRates.push(1000 / rowDelta);
                rowDensities.push((columns.length * 1000) / rowDelta);
                rowDensityWeights.push(Math.max(1, rowDelta));
                if (columns.length === 2) jumpstreamValues.push(Math.min(60, (columns.length * 1000) / rowDelta));
                if (rowDelta <= 80) fastRowCount++;
                intervalBucket = Math.round(rowDelta / 5);
            } else {
                intervalBucket = -1;
            }
        }
        rowSignatures.push(rowMask * 256 + intervalBucket + 1);
        previousChordSize = columns.length;
        previousRowTime = time;

        for (const column of columns) {
            const sameDelta = time - lastByColumn[column];
            if (sameDelta > 0 && sameDelta < 1000) {
                jackValues.push(Math.min(230, 15000 / sameDelta));
            }
            if (sameDelta > 0 && sameDelta < 1600) {
                columnIntervals.push(sameDelta);
                columnRates.push(1000 / sameDelta);
            }

            const leftNeighbor = column - 1;
            if (leftNeighbor >= 0) {
                const delta = time - lastByColumn[leftNeighbor];
                if (delta > 0 && delta < 260) {
                    streamValues.push((260 - delta) / 35);
                }
            }
            const rightNeighbor = column + 1;
            if (rightNeighbor < map.keyCount) {
                const delta = time - lastByColumn[rightNeighbor];
                if (delta > 0 && delta < 260) {
                    streamValues.push((260 - delta) / 35);
                }
            }

            if (previousColumn != null) {
                const direction = Math.sign(column - previousColumn);
                if (direction && previousDirection && direction !== previousDirection) directionChanges++;
                if (direction) previousDirection = direction;
            }
            previousColumn = column;
            lastByColumn[column] = time;
        }
    }

    for (let i = 1; i < releaseTimes.length; i++) {
        const tailDelta = releaseTimes[i] - releaseTimes[i - 1];
        if (tailDelta > 0 && tailDelta < 1600) {
            tailIntervals.push(tailDelta);
            tailRates.push(1000 / tailDelta);
        }
    }

    const chordRatio = orderedRows.length ? chordRows / orderedRows.length : 0;
    const twoNoteChordRatio = orderedRows.length ? twoNoteChordRows / orderedRows.length : 0;
    const peakNps1s = countInWindow(noteTimes, 1000);
    const peakNps5s = countInWindow(noteTimes, 5000) / 5;
    const nps5sSamples = sampledWindowNps(noteTimes, 5000, durationMs);
    const [nps5sP50, nps5sP90, nps5sP95] = quantiles(nps5sSamples, [0.5, 0.9, 0.95]);
    const sustainedNps10s = countInWindow(noteTimes, 10000) / 10;
    const sustainedNps30s = countInWindow(noteTimes, 30000) / 30;
    const sustainedNps60s = countInWindow(noteTimes, 60000) / 60;
    const noteSpanMs = noteTimes.length >= 2 ? Math.max(1, noteTimes[noteTimes.length - 1] - noteTimes[0]) : durationMs;
    const activeNps = notes.length / Math.max(1, noteSpanMs / 1000);
    const longGapRatio = durationMs > 0 ? longGapMs / durationMs : 0;
    const jackPressure = quantile(jackValues, 0.92);
    const streamPressure = quantile(streamValues, 0.9);
    const jumpstreamPressure = quantile(jumpstreamValues, 0.9);
    const burstDensity = quantile(rowDensities, 0.9);
    const rowBurstPressure = quantile(rowRates, 0.9);
    const fastRowRatio = rowIntervals.length
        ? fastRowCount / rowIntervals.length
        : 0;
    const rowIntervalEntropy = bucketEntropy(rowIntervals, 5);
    const rowPatternEntropy = bucketEntropy(rowMasks, 1);
    const rowPatternVariety = rowMasks.length
        ? new Set(rowMasks).size / Math.min(rowMasks.length, 2 ** Math.max(1, map.keyCount))
        : 0;
    const rowMotifRepeatRatio = numericNgramRepeatRatio(rowMasks, 4, rowMaskBase);
    const rhythmMotifRepeatRatio = numericNgramRepeatRatio(rowSignatures, 4, rowSignatureBase);
    const adjacentMotifRepeatRatio = adjacentNgramRepeatRatio(rowSignatures, 4);
    const repeatedRowPatternRatio = orderedRows.length > 1 ? repeatedRowPatterns / (orderedRows.length - 1) : 0;
    const alternatingRowPatternRatio = orderedRows.length > 2 ? alternatingRowPatterns / (orderedRows.length - 2) : 0;
    const rowPatternChangeRate = orderedRows.length > 1 ? rowPatternChangeSum / (orderedRows.length - 1) : 0;
    const patternVariety = 0.5 * raoQuadraticEntropyLog(bucketValues(rowIntervals, 5), 1)
        + 1.125 * raoQuadraticEntropyLog(bucketValues(columnIntervals, 5), 2)
        + 0.11 * raoQuadraticEntropyLog(bucketValues(tailIntervals, 5), 1);
    const spikiness = strainSpikiness(rowDensities, rowDensityWeights);
    const sustainedPressureRatio = sustainedNps10s / Math.max(1, peakNps1s, peakNps5s);
    const averageColumnCount = average(columnCounts);
    const columnImbalance = averageColumnCount > 0
        ? columnCounts.reduce((sum, count) => sum + Math.abs(count - averageColumnCount), 0) / (columnCounts.length * averageColumnCount)
        : 0;
    const anchorPressure = columnImbalance * (0.5 + Math.min(1.5, jackPressure / 150)) + Math.max(0, quantile(columnRates, 0.9) - 7) * 0.04;
    const lnReleasePressure = releaseTimes.length
        ? countInWindow(releaseTimes, 5000) / 5 + quantile(tailRates, 0.9) * 0.15
        : 0;
    const lnDensity = durationMs > 0 ? totalHoldMs / (durationMs * Math.max(1, map.keyCount)) : 0;
    const lnChordPressure = holdRows ? lnChordRows / holdRows : 0;
    const lnHoldDurationAvg = average(holdDurations);
    const lnHoldDurationP90 = quantile(holdDurations, 0.9);
    holdEvents.sort((left, right) => left.time - right.time || right.delta - left.delta);
    let activeHolds = 0;
    let activeHoldPeak = 0;
    let activeHoldArea = 0;
    let previousHoldEventTime = holdEvents[0]?.time ?? 0;
    for (const event of holdEvents) {
        activeHoldArea += Math.max(0, event.time - previousHoldEventTime) * activeHolds;
        activeHolds = Math.max(0, activeHolds + event.delta);
        activeHoldPeak = Math.max(activeHoldPeak, activeHolds);
        previousHoldEventTime = event.time;
    }
    const averageActiveHolds = durationMs > 0 ? activeHoldArea / durationMs : 0;
    const lnOverlapPressure = averageActiveHolds + activeHoldPeak * 0.35;
    const chordSizeChangeRate = orderedRows.length ? chordSizeChanges / orderedRows.length : 0;
    const directionChangeRate = notes.length ? directionChanges / notes.length : 0;
    const chordjackPressure = jackPressure * (0.28 + chordRatio * 1.35) + burstDensity * chordRatio * 0.6;
    const chordColumnOverlapRatio = chordPairCount ? chordPairOverlapCount / chordPairCount : 0;
    const adjacentColumnRehitShare = notes.length ? adjacentColumnRehitNotes / notes.length : 0;
    const twoBackColumnRehitShare = notes.length ? twoBackColumnRehitNotes / notes.length : 0;
    const twoBackColumnRehitExcess = twoBackColumnRehitShare - adjacentColumnRehitShare;
    const techPressure = orderedRows.length
        ? (directionChanges / orderedRows.length) * 4.4 + (chordSizeChanges / orderedRows.length) * 3.5 + chordRatio * 1.6 + average(rowDensities) * 0.018
        : 0;

    return {
        notes,
        noteTimes,
        durationMs,
        orderedRows,
        warnings,
        metrics: {
            keyCount: map.keyCount,
            noteCount: notes.length,
            durationMs,
            holdRatio,
            chordRatio,
            twoNoteChordRatio,
            peakNps1s,
            peakNps5s,
            nps5sP50,
            nps5sP90,
            nps5sP95,
            sustainedNps10s,
            sustainedNps30s,
            sustainedNps60s,
            activeNps,
            longGapRatio,
            longGapCount,
            jackPressure,
            streamPressure,
            jumpstreamPressure,
            chordjackPressure,
            chordColumnOverlapRatio,
            adjacentColumnRehitShare,
            twoBackColumnRehitShare,
            twoBackColumnRehitExcess,
            techPressure,
            rowBurstPressure,
            fastRowRatio,
            rowIntervalEntropy,
            patternVariety,
            rowPatternEntropy,
            rowPatternVariety,
            repeatedRowPatternRatio,
            alternatingRowPatternRatio,
            rowPatternChangeRate,
            rowMotifRepeatRatio,
            rhythmMotifRepeatRatio,
            adjacentMotifRepeatRatio,
            strainSpikiness: spikiness,
            sustainedPressureRatio,
            anchorPressure,
            lnReleasePressure,
            lnDensity,
            lnOverlapPressure,
            lnChordPressure,
            lnHoldDurationAvg,
            lnHoldDurationP90,
            chordSizeChangeRate,
            directionChangeRate,
            staminaPressure: sustainedNps10s,
        },
    };
}
