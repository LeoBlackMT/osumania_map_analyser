// aleju03 LN 参考邻域估计器（移植自 mania-hub `live-backend/src/dan/dan-estimator/ln.ts`）
//
// 算法逐字移植：压力距离 10 维与除数、近邻门槛（>2.6 放弃 / <0.08 直取 / 8 邻加权）、
// 高速与速率加成、12 个结构 floor 与 2 个 compression、课程分段（break/长间隔 → 75 分位）、
// 以及 `ln-pressure` 回归兜底。**数值与求值顺序未做任何改动**；参考数据表在
// `lnReferenceCharts.js`（逐字提取，127 行）。
//
// 来源说明：参考表末段若干 level 为小数的条目是上游为评测标注谱面折入的锚点
// （源码注释：every labeled non-course chart self-matches）。原实现的远邻门外回归
// 使用 star × rate^0.7，速率越大漂移越大——保留原样以便与我们自己的表对照。
// 共享纯函数：禁止 window/document，禁止 import js/app/。

import { LN_REFERENCE_CHARTS } from "./lnReferenceCharts.js";

/**
 * Last level on the LN dan ladder. The courses run 1-10 then the named stages
 * (11 Yoake, 12 Yuugure, 13 Yoru, 14 Yami, 15 Yume, 16 Yokaze, 17 Yeehee).
 * Anything past 17 is off the ladder, not an 18th dan.
 */
export const LN_LADDER_TOP = 17;

/** The hold share at which a chart's PRIMARY identity is the LN one. */
export const LN_PRIMARY_MIN_RATIO = 0.45;

/** 7K's own, lower line (its mapping culture ships hybrids read as LN). */
export const LN_PRIMARY_7K_MIN_RATIO = 0.375;

/** The LN identity line for a keymode: what "this chart is LN" means. */
export function lnPrimaryMinRatioFor(keyCount) {
    return keyCount === 7 ? LN_PRIMARY_7K_MIN_RATIO : LN_PRIMARY_MIN_RATIO;
}

// The LN dan ladder is numeric 1-17 with +/- variants; it never extends into
// the rice ladder's greek levels. The variant bands are parseDan's.
export function parseLnDan(rawDan) {
    const level = Math.max(1, Math.min(LN_LADDER_TOP, Math.round(rawDan)));
    const offset = rawDan - level;
    const variant = offset <= -0.45 ? "--" : offset <= -0.25 ? "-" : offset < 0.1 ? null : offset < 0.26 ? "+" : "++";
    return {
        label: String(level),
        variant,
        displayName: `LN ${level}${variant ?? ""}`,
    };
}

function parseRawLnDan(rawDan) {
    return {
        ...parseLnDan(rawDan),
        rawDan,
        estimatedSr: rawDan,
        confidence: 0.72,
        reason: "ln-pressure",
    };
}

export function pressureDistance(metrics, reference, durationSeconds) {
    return Math.abs(metrics.holdRatio - reference.h) / 0.13
        + (durationSeconds ? Math.abs(durationSeconds - reference.s) / 80 : 0)
        + (durationSeconds ? Math.abs(metrics.noteCount - reference.n) / 2200 : 0)
        + Math.abs(metrics.lnDensity - reference.d) / 0.11
        + Math.abs(metrics.lnOverlapPressure - reference.o) / 0.45
        + Math.abs(metrics.lnReleasePressure - reference.r) / 4
        + Math.abs(metrics.lnChordPressure - reference.c) / 0.16
        + Math.abs(metrics.peakNps5s - reference.p) / 4
        + Math.abs(metrics.sustainedNps10s - reference.u) / 4
        + Math.abs(metrics.chordRatio - reference.q) / 0.16;
}

export function getLnReferenceComparisonMetrics(metrics, rate) {
    if (rate <= 1) return metrics;
    return {
        ...metrics,
        peakNps1s: metrics.peakNps1s / rate,
        peakNps5s: metrics.peakNps5s / rate,
        sustainedNps10s: metrics.sustainedNps10s / rate,
        staminaPressure: metrics.staminaPressure / rate,
        lnDensity: metrics.lnDensity / rate,
        lnReleasePressure: metrics.lnReleasePressure / rate,
    };
}

export function getLnReferenceNeighbors(metrics, rate, limit = 8, durationSeconds) {
    const comparisonMetrics = getLnReferenceComparisonMetrics(metrics, rate);
    return LN_REFERENCE_CHARTS
        .map((reference) => ({
            level: reference.level,
            distance: pressureDistance(comparisonMetrics, reference, durationSeconds),
            metrics: reference,
        }))
        .sort((left, right) => left.distance - right.distance)
        .slice(0, Math.max(0, limit));
}

function officialReferenceNeighborTarget(metrics, rate, durationMs) {
    const [pressureBest] = getLnReferenceNeighbors(metrics, rate, 1);
    const nearest = getLnReferenceNeighbors(metrics, rate, 8, durationMs / 1000);
    const [best] = nearest;
    if (!best || !pressureBest || pressureBest.distance > 2.6) return null;

    if (best.distance < 0.08) {
        return {
            ...parseRawLnDan(best.level),
            confidence: 0.9,
            reason: "ln-reference-neighbor",
        };
    }

    const weighted = nearest.reduce((sum, item) => {
        const weight = 1 / Math.pow(item.distance + 0.35, 1.5);
        return {
            level: sum.level + item.level * weight,
            weight: sum.weight + weight,
        };
    }, { level: 0, weight: 0 });
    const neighborDan = weighted.weight > 0 ? weighted.level / weighted.weight : best.level;
    const highEndSpeedBonus = Math.min(
        0.85,
        Math.max(0, (metrics.sustainedNps10s - 28) / 4) * 0.8
        + Math.max(0, (metrics.lnReleasePressure - 30) / 5) * 0.6
        + Math.max(0, (metrics.peakNps5s - 29) / 4) * 0.35,
    );
    const ratePressureBonus = Math.min(0.45, Math.max(0, rate - 1) * 1.5);
    const rawDan = Math.max(1, neighborDan + highEndSpeedBonus + ratePressureBonus);

    return {
        ...parseRawLnDan(rawDan),
        confidence: Math.max(0.72, 0.9 - best.distance * 0.05),
        reason: "ln-reference-neighbor",
    };
}

function highSrLnPressureFloor(metrics, starRating) {
    if (starRating < 7
        || metrics.holdRatio < 0.65
        || metrics.lnDensity < 0.34
        || metrics.lnOverlapPressure < 2.75
        || metrics.lnReleasePressure < 20
        || metrics.chordRatio < 0.37
        || metrics.lnChordPressure < 0.43
        || metrics.peakNps5s > 24.5
        || metrics.sustainedNps10s > 23.5) {
        return 0;
    }

    return 12.8
        + Math.max(0, starRating - 7) * 1.22
        + Math.min(0.45, Math.max(0, metrics.lnReleasePressure - 24) * 0.07)
        + Math.min(0.35, Math.max(0, metrics.lnDensity - 0.4) * 1.2)
        + Math.min(0.35, Math.max(0, 24 - metrics.peakNps5s) * 0.08);
}

function applyHighSrLnPressureFloor(rawDan, metrics, starRating) {
    if (Math.abs(rawDan - Math.round(rawDan)) < 0.001) return rawDan;
    if (rawDan < 11.4) return rawDan;
    return Math.max(rawDan, highSrLnPressureFloor(metrics, starRating));
}

function lowRateDenseLnWallFloor(metrics, starRating) {
    if (starRating > 4.4
        || metrics.noteCount < 900
        || metrics.noteCount > 1100
        || metrics.holdRatio < 0.9
        || metrics.lnDensity < 0.6
        || metrics.lnOverlapPressure < 3.8
        || metrics.lnReleasePressure < 13
        || metrics.lnReleasePressure > 15
        || metrics.chordRatio < 0.58
        || metrics.chordRatio > 0.65
        || metrics.lnChordPressure < 0.58
        || metrics.peakNps5s < 12.5
        || metrics.peakNps5s > 14
        || metrics.rowIntervalEntropy > 0.7) {
        return 0;
    }

    return 8;
}

function beginnerLongHoldCourseFloor(metrics, starRating) {
    if (starRating > 3
        || metrics.noteCount < 700
        || metrics.noteCount > 800
        || metrics.holdRatio < 0.8
        || metrics.lnDensity < 0.38
        || metrics.lnReleasePressure > 11
        || metrics.peakNps5s > 9
        || metrics.sustainedNps10s > 8.8
        || metrics.lnHoldDurationP90 < 500
        || metrics.patternVariety < 3.3) {
        return 0;
    }

    return 6;
}

function slowCourseLnWallFloor(metrics, starRating) {
    if (starRating < 3.5
        || starRating > 4.2
        || metrics.noteCount < 1200
        || metrics.noteCount > 1400
        || metrics.holdRatio < 0.74
        || metrics.holdRatio > 0.8
        || metrics.lnDensity < 0.48
        || metrics.lnReleasePressure < 12
        || metrics.lnReleasePressure > 13
        || metrics.lnOverlapPressure < 3.3
        || metrics.chordRatio < 0.46
        || metrics.chordRatio > 0.5
        || metrics.peakNps5s > 12
        || metrics.rowIntervalEntropy > 0.7) {
        return 0;
    }

    return 8;
}

function shortHighEndReleaseWallFloor(metrics, starRating) {
    if (starRating < 8.5
        || metrics.noteCount > 2000
        || metrics.holdRatio < 0.8
        || metrics.lnDensity < 0.38
        || metrics.lnReleasePressure < 32
        || metrics.peakNps5s < 29
        || metrics.sustainedNps10s < 28
        || metrics.lnHoldDurationP90 > 180
        || metrics.rowIntervalEntropy > 1.6) {
        return 0;
    }

    return 16;
}

function compactTwelfthLnWallFloor(metrics) {
    if (metrics.noteCount >= 1800
        && metrics.noteCount <= 2200
        && metrics.holdRatio >= 0.9
        && metrics.lnDensity >= 0.45
        && metrics.lnDensity <= 0.5
        && metrics.lnReleasePressure >= 24
        && metrics.lnReleasePressure <= 26
        && metrics.peakNps5s >= 21
        && metrics.peakNps5s <= 23
        && metrics.rowIntervalEntropy >= 2) {
        return 12.46;
    }

    if (metrics.noteCount >= 4300
        && metrics.noteCount <= 4700
        && metrics.holdRatio >= 0.65
        && metrics.holdRatio <= 0.7
        && metrics.lnDensity >= 0.28
        && metrics.lnDensity <= 0.33
        && metrics.lnReleasePressure >= 26
        && metrics.lnReleasePressure <= 28
        && metrics.peakNps5s >= 24
        && metrics.peakNps5s <= 26
        && metrics.rowIntervalEntropy >= 1.4
        && metrics.rowIntervalEntropy <= 1.7) {
        return 12.46;
    }

    return 0;
}

function thirteenthLnWallFloor(metrics) {
    if (metrics.noteCount >= 1800
        && metrics.noteCount <= 2000
        && metrics.holdRatio >= 0.88
        && metrics.lnDensity >= 0.36
        && metrics.lnDensity <= 0.4
        && metrics.lnReleasePressure >= 26
        && metrics.lnReleasePressure <= 28
        && metrics.peakNps5s >= 24
        && metrics.peakNps5s <= 26
        && metrics.rowIntervalEntropy >= 1.9) {
        return 13;
    }

    if (metrics.noteCount >= 2700
        && metrics.noteCount <= 2900
        && metrics.holdRatio >= 0.93
        && metrics.lnDensity >= 0.5
        && metrics.lnReleasePressure >= 26
        && metrics.lnReleasePressure <= 28
        && metrics.peakNps5s >= 25
        && metrics.peakNps5s <= 26.5
        && metrics.rowIntervalEntropy <= 0.7) {
        return 13;
    }

    if (metrics.noteCount >= 3000
        && metrics.noteCount <= 3300
        && metrics.holdRatio >= 0.74
        && metrics.holdRatio <= 0.78
        && metrics.lnDensity >= 0.39
        && metrics.lnDensity <= 0.43
        && metrics.lnReleasePressure >= 27
        && metrics.lnReleasePressure <= 28
        && metrics.chordRatio >= 0.52
        && metrics.chordRatio <= 0.56
        && metrics.lnChordPressure >= 0.55) {
        return 13;
    }

    return 0;
}

function eleventhLnWallFloor(metrics) {
    if (metrics.noteCount >= 1500
        && metrics.noteCount <= 1700
        && metrics.holdRatio >= 0.9
        && metrics.lnDensity >= 0.48
        && metrics.lnDensity <= 0.5
        && metrics.lnReleasePressure >= 21
        && metrics.lnReleasePressure <= 22
        && metrics.peakNps5s >= 17.5
        && metrics.peakNps5s <= 18.5
        && metrics.lnHoldDurationP90 >= 260
        && metrics.rowIntervalEntropy <= 1.1) {
        return 11;
    }

    return 0;
}

function fifteenthLnWallFloor(metrics, starRating) {
    if (starRating >= 7.3
        && metrics.noteCount >= 3100
        && metrics.noteCount <= 3400
        && metrics.holdRatio >= 0.7
        && metrics.holdRatio <= 0.82
        && metrics.lnDensity >= 0.4
        && metrics.lnDensity <= 0.51
        && metrics.lnReleasePressure >= 26
        && metrics.lnReleasePressure <= 30
        && metrics.peakNps5s >= 26
        && metrics.peakNps5s <= 28
        && metrics.sustainedNps10s >= 25
        && metrics.chordRatio >= 0.33
        && metrics.chordRatio <= 0.5) {
        return 15;
    }

    if (starRating >= 7.8
        && metrics.noteCount >= 4200
        && metrics.noteCount <= 4600
        && metrics.holdRatio >= 0.86
        && metrics.holdRatio <= 0.9
        && metrics.lnDensity >= 0.4
        && metrics.lnDensity <= 0.43
        && metrics.lnReleasePressure >= 28
        && metrics.lnReleasePressure <= 30
        && metrics.peakNps5s >= 26
        && metrics.peakNps5s <= 27
        && metrics.rowIntervalEntropy >= 2.5) {
        return 15;
    }

    return 0;
}

function repetitiveFullLnWallFloor(metrics) {
    if (metrics.noteCount >= 2900
        && metrics.noteCount <= 3200
        && metrics.holdRatio >= 0.93
        && metrics.lnDensity >= 0.55
        && metrics.lnDensity <= 0.61
        && metrics.lnReleasePressure >= 20
        && metrics.lnReleasePressure <= 22
        && metrics.peakNps5s >= 18.5
        && metrics.peakNps5s <= 20
        && metrics.rowIntervalEntropy <= 1) {
        return 10;
    }

    return 0;
}

function chordHeavySlowLnWallFloor(metrics) {
    if (metrics.noteCount >= 2300
        && metrics.noteCount <= 2500
        && metrics.holdRatio >= 0.76
        && metrics.holdRatio <= 0.8
        && metrics.lnDensity >= 0.44
        && metrics.lnDensity <= 0.48
        && metrics.lnReleasePressure >= 17.5
        && metrics.lnReleasePressure <= 19
        && metrics.lnChordPressure >= 0.7) {
        return 8;
    }

    return 0;
}

function compactRepetitiveLnWallFloor(metrics) {
    if (metrics.noteCount >= 2500
        && metrics.noteCount <= 2800
        && metrics.holdRatio >= 0.89
        && metrics.holdRatio <= 0.94
        && metrics.lnDensity >= 0.4
        && metrics.lnDensity <= 0.45
        && metrics.lnReleasePressure >= 23
        && metrics.lnReleasePressure <= 24
        && metrics.peakNps5s >= 21
        && metrics.peakNps5s <= 22
        && metrics.rowIntervalEntropy >= 1
        && metrics.rowIntervalEntropy <= 1.4) {
        return 10;
    }

    return 0;
}

function beginnerLongHoldCourseCompression(metrics, starRating) {
    if (starRating > 2.8
        || metrics.noteCount < 780
        || metrics.noteCount > 850
        || metrics.holdRatio < 0.7
        || metrics.holdRatio > 0.8
        || metrics.lnDensity < 0.38
        || metrics.lnReleasePressure > 10
        || metrics.peakNps5s > 9
        || metrics.lnHoldDurationP90 < 600
        || metrics.rowIntervalEntropy > 1) {
        return null;
    }

    return 2;
}

function overweightedLnWallCompression(metrics) {
    if (metrics.noteCount >= 1700
        && metrics.noteCount <= 2000
        && metrics.holdRatio >= 0.86
        && metrics.holdRatio <= 0.91
        && metrics.lnDensity >= 0.55
        && metrics.lnDensity <= 0.63
        && metrics.lnReleasePressure >= 22
        && metrics.lnReleasePressure <= 24
        && metrics.peakNps5s >= 22
        && metrics.peakNps5s <= 24
        && metrics.lnHoldDurationP90 >= 330
        && metrics.rowIntervalEntropy >= 1.5) {
        return 10.46;
    }

    if (metrics.noteCount >= 3000
        && metrics.noteCount <= 3400
        && metrics.holdRatio >= 0.8
        && metrics.holdRatio <= 0.85
        && metrics.lnDensity >= 0.34
        && metrics.lnDensity <= 0.38
        && metrics.lnReleasePressure >= 27.5
        && metrics.lnReleasePressure <= 29.5
        && metrics.peakNps5s >= 24
        && metrics.peakNps5s <= 25
        && metrics.chordRatio >= 0.29
        && metrics.chordRatio <= 0.34) {
        return 12;
    }

    if (metrics.noteCount >= 1450
        && metrics.noteCount <= 1650
        && metrics.holdRatio >= 0.88
        && metrics.holdRatio <= 0.92
        && metrics.lnDensity >= 0.4
        && metrics.lnDensity <= 0.44
        && metrics.lnReleasePressure >= 20
        && metrics.lnReleasePressure <= 22
        && metrics.peakNps5s >= 18.5
        && metrics.peakNps5s <= 20
        && metrics.rowIntervalEntropy >= 1
        && metrics.rowIntervalEntropy <= 1.3) {
        return 9.46;
    }

    if (metrics.noteCount >= 3300
        && metrics.noteCount <= 3600
        && metrics.holdRatio >= 0.58
        && metrics.holdRatio <= 0.64
        && metrics.lnDensity >= 0.24
        && metrics.lnDensity <= 0.28
        && metrics.lnReleasePressure >= 20
        && metrics.lnReleasePressure <= 21.5
        && metrics.peakNps5s >= 18
        && metrics.peakNps5s <= 20
        && metrics.fastRowRatio >= 0.2) {
        return 7;
    }

    if (metrics.noteCount >= 2500
        && metrics.noteCount <= 4100
        && metrics.holdRatio >= 0.84
        && metrics.holdRatio <= 0.87
        && metrics.lnDensity >= 0.46
        && metrics.lnDensity <= 0.48
        && metrics.lnReleasePressure >= 25
        && metrics.lnReleasePressure <= 26.2
        && metrics.peakNps5s >= 23
        && metrics.peakNps5s <= 24
        && metrics.lnChordPressure >= 0.52) {
        return 11.46;
    }

    if (metrics.noteCount >= 3000
        && metrics.noteCount <= 3500
        && metrics.holdRatio >= 0.8
        && metrics.holdRatio <= 0.86
        && metrics.lnDensity >= 0.44
        && metrics.lnDensity <= 0.48
        && metrics.lnReleasePressure >= 22
        && metrics.lnReleasePressure <= 23.5
        && metrics.peakNps5s >= 20.5
        && metrics.peakNps5s <= 22
        && metrics.patternVariety >= 2.7
        && metrics.patternVariety <= 2.9) {
        return 8;
    }

    if (metrics.noteCount >= 3400
        && metrics.noteCount <= 3800
        && metrics.holdRatio >= 0.88
        && metrics.holdRatio <= 0.92
        && metrics.lnDensity >= 0.44
        && metrics.lnDensity <= 0.48
        && metrics.lnReleasePressure >= 27
        && metrics.lnReleasePressure <= 29
        && metrics.peakNps5s >= 24
        && metrics.peakNps5s <= 25.5
        && metrics.chordRatio >= 0.3
        && metrics.chordRatio <= 0.34) {
        return 12.46;
    }

    if (metrics.noteCount >= 5000
        && metrics.holdRatio >= 0.8
        && metrics.holdRatio <= 0.84
        && metrics.lnDensity >= 0.4
        && metrics.lnDensity <= 0.43
        && metrics.lnReleasePressure >= 27
        && metrics.lnReleasePressure <= 29
        && metrics.peakNps5s >= 25
        && metrics.peakNps5s <= 26.5
        && metrics.chordRatio >= 0.38
        && metrics.chordRatio <= 0.4) {
        return 12.46;
    }

    if (metrics.noteCount >= 2400
        && metrics.noteCount <= 2600
        && metrics.holdRatio >= 0.83
        && metrics.holdRatio <= 0.87
        && metrics.lnDensity >= 0.43
        && metrics.lnDensity <= 0.46
        && metrics.lnReleasePressure >= 24
        && metrics.lnReleasePressure <= 25.5
        && metrics.chordRatio >= 0.64
        && metrics.lnChordPressure >= 0.63) {
        return 13;
    }

    return null;
}

function applyLnStructuralCalibration(rawDan, metrics, starRating, rate) {
    const floored = Math.max(
        applyHighSrLnPressureFloor(rawDan, metrics, starRating),
        lowRateDenseLnWallFloor(metrics, starRating),
        beginnerLongHoldCourseFloor(metrics, starRating),
        slowCourseLnWallFloor(metrics, starRating),
        shortHighEndReleaseWallFloor(metrics, starRating),
        compactTwelfthLnWallFloor(metrics),
        thirteenthLnWallFloor(metrics),
        eleventhLnWallFloor(metrics),
        fifteenthLnWallFloor(metrics, starRating),
        repetitiveFullLnWallFloor(metrics),
        chordHeavySlowLnWallFloor(metrics),
        compactRepetitiveLnWallFloor(metrics),
    );
    const beginnerCompression = beginnerLongHoldCourseCompression(metrics, starRating);
    const overweightedCompression = overweightedLnWallCompression(metrics);
    const compressed = beginnerCompression === null ? floored : Math.min(floored, beginnerCompression);
    const structurallyCompressed = overweightedCompression === null ? compressed : Math.min(compressed, overweightedCompression);
    const shortMixedLnHybridCap = rate <= 1.05
        && starRating >= 8
        && starRating <= 9.5
        && metrics.noteCount >= 5000
        && metrics.holdRatio >= 0.28
        && metrics.holdRatio <= 0.45
        && metrics.lnDensity >= 0.18
        && metrics.lnDensity <= 0.3
        && metrics.lnReleasePressure >= 24
        && metrics.lnReleasePressure <= 30
        && metrics.peakNps5s >= 27
        && metrics.peakNps5s <= 32
        && metrics.sustainedNps10s >= 26
        && metrics.sustainedNps10s <= 31
        && metrics.lnHoldDurationP90 >= 220
        && metrics.lnHoldDurationP90 <= 290
        && metrics.chordRatio <= 0.42
        ? 13.46
        : null;
    return shortMixedLnHybridCap === null ? structurallyCompressed : Math.min(structurallyCompressed, shortMixedLnHybridCap);
}

function makeComponent(map, startTime, endTime) {
    const segmentNotes = map.notes.filter((note) => note.time >= startTime && note.time < endTime);
    if (endTime - startTime < 30000 || segmentNotes.length < 300) return null;

    return {
        ...map,
        notes: segmentNotes.map((segmentNote) => ({
            ...segmentNote,
            time: segmentNote.time - startTime,
            endTime: Math.max(segmentNote.endTime, segmentNote.time) - startTime,
        })),
        totalLength: endTime - startTime,
        breakPeriods: [],
    };
}

function splitComponentsByBreakPeriods(map) {
    if (map.notes.length === 0 || map.breakPeriods.length === 0) return [];

    const sortedBreaks = [...map.breakPeriods]
        .filter((period) => period.endTime > period.startTime)
        .sort((left, right) => left.startTime - right.startTime);
    const components = [];
    let segmentStart = map.notes[0].time;

    for (const period of sortedBreaks) {
        const component = makeComponent(map, segmentStart, period.startTime);
        if (component) components.push(component);
        segmentStart = period.endTime;
    }

    const component = makeComponent(map, segmentStart, Math.max(map.totalLength, map.notes.at(-1)?.endTime ?? segmentStart));
    if (component) components.push(component);

    return components;
}

function splitComponentsByRawGaps(map) {
    if (map.notes.length === 0) return [];

    const gaps = [];
    for (let index = 0; index < map.notes.length - 1; index++) {
        const current = map.notes[index];
        const next = map.notes[index + 1];
        const currentEnd = Math.max(current.time, current.endTime);
        if (next.time - currentEnd >= 4500) {
            gaps.push({ start: currentEnd, end: next.time });
        }
    }

    if (gaps.length !== 3) return [];

    const components = [];
    let segmentStart = map.notes[0].time;
    for (const gap of gaps) {
        const component = makeComponent(map, segmentStart, gap.start);
        if (component) components.push(component);
        segmentStart = gap.end;
    }

    const component = makeComponent(map, segmentStart, Math.max(map.totalLength, map.notes.at(-1)?.endTime ?? segmentStart));
    if (component) components.push(component);

    return components.length === 4 ? components : [];
}

function splitCourseComponents(map) {
    const explicitBreakComponents = splitComponentsByBreakPeriods(map);
    if (explicitBreakComponents.length >= 3) return explicitBreakComponents;

    return splitComponentsByRawGaps(map);
}

function estimateLnCourseFromComponents(map, input, starRating, rate, extractDanFeatures, forceLn) {
    const components = splitCourseComponents(map);
    if (components.length < 3) return null;

    const estimates = components
        .map((component) => {
            const features = extractDanFeatures(component, { ...input, totalLength: component.totalLength / 1000 }, rate);
            return estimateLnDanInternal(
                component,
                { ...input, totalLength: component.totalLength / 1000 },
                features.metrics,
                starRating,
                features.durationMs,
                rate,
                false,
                extractDanFeatures,
                forceLn,
            );
        })
        .filter((estimate) => estimate !== null);

    if (estimates.length < 3) return null;

    const rawDans = estimates.map((estimate) => estimate.rawDan).sort((left, right) => left - right);
    const rawDan = rawDans[Math.floor((rawDans.length - 1) * 0.75)];
    return {
        ...parseRawLnDan(rawDan),
        confidence: Math.min(0.9, 0.72 + estimates.length * 0.03),
        reason: "ln-course-components",
    };
}

/**
 * LN dan 估计主入口（源文件 `estimateLnDan`）。
 * `extractDanFeatures` 由调用方注入，避免本模块依赖 features.js 造成循环。
 * `forceLn` 为本仓库新增开关（源实现没有）：显式选定 aleju03 时跳过 LN 候选门，
 * 让任意 4K 谱面都能拿到 LN 判决（候选门不过则直接走回归兜底）；
 * Mixed 的低段接管保持 forceLn=false，维持源的候选门语义。
 */
export function estimateLnDan(map, input, metrics, starRating, durationMs, rate, allowCourseSegmentation = true, extractDanFeatures = null, forceLn = false) {
    return estimateLnDanInternal(map, input, metrics, starRating, durationMs, rate, allowCourseSegmentation, extractDanFeatures, forceLn);
}

function estimateLnDanInternal(map, input, metrics, starRating, durationMs, rate, allowCourseSegmentation, extractDanFeatures, forceLn = false) {
    const metadata = `${map.title ?? ""} ${map.version ?? ""} ${input.title ?? ""} ${input.version ?? ""}`
        .toLowerCase()
        .replace(/\s+/g, " ")
        .trim();
    const metadataHasLnHint = /\bln\b|long note|full ln|ln edit|ln hybrid|ln wall|ln jack|ln speed|ln jumpstream/.test(metadata);
    const metadataLnSignal = metadataHasLnHint && (
        metrics.holdRatio >= 0.12
        || metrics.lnDensity >= 0.08
        || metrics.lnReleasePressure >= 1.5
        || metrics.lnOverlapPressure >= 0.9
    );
    const chartLnSignal = (
        metrics.holdRatio >= 0.28
        && metrics.lnDensity >= 0.14
        && metrics.lnHoldDurationP90 >= 220
        && metrics.lnChordPressure >= 0.12
    ) || (
        metrics.holdRatio >= 0.28
        && metrics.lnDensity >= 0.16
        && metrics.lnReleasePressure >= 22
        && metrics.lnChordPressure >= 0.25
    ) || (
        metrics.holdRatio >= 0.34
        && metrics.lnDensity >= 0.1
        && metrics.lnOverlapPressure >= 0.75
        && metrics.lnHoldDurationP90 >= 160
    );
    const lnCandidate = metadataLnSignal || chartLnSignal;
    // forceLn：显式选择 aleju03 时不因候选门放弃（任意 4K 谱面都给 LN 判决）。
    if (!lnCandidate && !forceLn) return null;

    if (allowCourseSegmentation && extractDanFeatures) {
        const courseEstimate = estimateLnCourseFromComponents(map, input, starRating, rate, extractDanFeatures, forceLn);
        if (courseEstimate) return courseEstimate;
    }

    const referenceNeighbor = officialReferenceNeighborTarget(metrics, rate, durationMs);
    if (referenceNeighbor) {
        const rawDan = applyLnStructuralCalibration(referenceNeighbor.rawDan, metrics, starRating, rate);
        return {
            ...parseRawLnDan(rawDan),
            confidence: referenceNeighbor.confidence,
            reason: referenceNeighbor.reason,
        };
    }

    const sr = starRating > 0 ? starRating : Math.max(1, metrics.peakNps5s * 0.18 + metrics.lnReleasePressure * 0.55);
    const durationMinutes = Math.max(0.6, durationMs / 60000);
    const shortReleaseHybridCompression = metrics.holdRatio >= 0.28
        && metrics.holdRatio <= 0.45
        && metrics.lnDensity >= 0.14
        && metrics.lnDensity <= 0.25
        && metrics.lnReleasePressure >= 22
        && metrics.lnChordPressure >= 0.25
        && metrics.lnHoldDurationP90 < 220
        && metrics.peakNps5s < 27
        && metrics.sustainedNps10s < 27
        ? Math.min(
            1.1,
            0.78
            + Math.max(0, 220 - metrics.lnHoldDurationP90) * 0.004
            + Math.max(0, 0.5 - metrics.holdRatio) * 0.6
            + Math.max(0, 27 - metrics.peakNps5s) * 0.05,
        )
        : 0;
    const rawDan = -8.15
        + sr * 2.6502
        + Math.max(0, sr - 5) * -1.3038
        + Math.max(0, sr - 6.5) * 0.5527
        + (metrics.peakNps5s / 20) * 0.5802
        + (metrics.lnReleasePressure / 20) * 1.057
        + metrics.lnDensity * 0.3841
        + (metrics.lnOverlapPressure / 4) * 0.3841
        + metrics.lnChordPressure * 0.1443
        + Math.log2(durationMinutes) * 0.4391
        - shortReleaseHybridCompression;

    return parseRawLnDan(applyLnStructuralCalibration(rawDan, metrics, starRating, rate));
}
