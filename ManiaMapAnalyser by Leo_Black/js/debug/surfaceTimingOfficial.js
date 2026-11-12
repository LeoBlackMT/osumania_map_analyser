// Official osu!mania star rating & pp — debug panel reference row.
//
// Ported from the genirx dart port of osu-master's official mania difficulty
// / performance calculators (lib/logic/algorithm/official/), which is itself a
// faithful translation of osu!lazer's osu.Game.Rulesets.Mania difficulty and
// performance code. DOM-free pure module: runs in the browser and in Node.
//
// GetColumn here uses the parser's derived column number directly (the parser
// already maps x → column); columnStrainTime and hold overlap use noteStarts /
// noteEnds (0 for circles). All times are divided by speedRate like the
// upstream CreateDifficultyHitObjects.

/**
 * Official mania star rating (osu-master ManiaDifficultyCalculator).
 * @param {{
 *   columns: number[],   // per-note column (0..keyCount-1)
 *   noteStarts: number[],// per-note start ms (map time)
 *   noteEnds: number[],  // per-note end ms (0 for circles)
 * }} parsed getParsedData() output
 * @param {number} [speedRate=1]
 * @returns {number|null} official star rating, or null on invalid input
 */
export function calculateOfficialStar({ columns, noteStarts, noteEnds }, speedRate = 1) {
    const n = columns ? columns.length : 0;
    if (!n) return null;
    if (!Number.isFinite(speedRate) || speedRate <= 0) return null;

    const keyCount = Math.max(...columns) + 1;
    if (keyCount < 1 || keyCount > 10) return null;

    // Sorted by start time (stable by original index like the upstream index
    // tie-break; circles have noteEnds[start] === 0).
    const order = [...Array(n).keys()].sort((a, b) => noteStarts[a] - noteStarts[b]);
    const times = order.map((i) => noteStarts[i] / speedRate);
    const ends = order.map((i) => noteEnds[i] / speedRate);
    const cols = order.map((i) => columns[i]);

    // Difficulty hit objects: skip the first note (i = 1..n-1 upstream.
    const dho = [];
    const perColumn = Array.from({ length: keyCount }, () => []);

    for (let i = 1; i < n; i += 1) {
        const cur = {
            column: cols[i],
            startTime: times[i],
            endTime: Math.max(ends[i], times[i]),
            deltaTime: times[i] - times[i - 1],
            columnStrainTime: 0,
            previous: null,
        };
        // previous = copy of prevDho.previous + prevDho at its column.
        const prevDho = dho.length ? dho[dho.length - 1] : null;
        const previous = Array(keyCount).fill(null);
        if (prevDho) {
            for (let c = 0; c < keyCount; c += 1) previous[c] = prevDho.previous ? prevDho.previous[c] : null;
            previous[prevDho.column] = prevDho;
        }
        cur.previous = previous;

        const colIndex = perColumn[cur.column].length;
        const prevInColumn = colIndex > 0 ? perColumn[cur.column][colIndex - 1] : null;
        cur.columnStrainTime = prevInColumn ? cur.startTime - prevInColumn.startTime : cur.startTime;

        dho.push(cur);
        perColumn[cur.column].push(cur);
    }

    // Strain skills.
    const individualStrains = Array(keyCount).fill(0);
    let highestIndividualStrain = 0;
    let overallStrain = 1;
    let currentStrain = 0;
    let currentSectionPeak = 0;
    let currentSectionEnd = 0;
    const strainPeaks = [];

    const sectionLength = 400;
    const decayWeight = 0.9;
    const individualDecayBase = 0.125;
    const overallDecayBase = 0.30;
    const releaseThreshold = 30.0;

    const applyDecay = (value, deltaTime, decayBase) => value * Math.pow(decayBase, deltaTime / 1000);
    const definitelyBigger = (a, b) => a - 1 > b;
    const logistic = (x, midpointOffset, multiplier) => 1 / (1 + Math.exp(multiplier * (midpointOffset - x)));

    const individualEvaluator = (o) => {
        let holdFactor = 1;
        for (const prev of o.previous) {
            if (!prev) continue;
            if (definitelyBigger(prev.endTime, o.endTime) && definitelyBigger(o.startTime, prev.startTime)) {
                holdFactor = 1.25;
                break;
            }
        }
        return 2.0 * holdFactor;
    };

    const overallEvaluator = (o) => {
        let isOverlapping = false;
        let closestEndTime = Math.abs(o.endTime - o.startTime);
        let holdFactor = 1;
        let holdAddition = 0;
        for (const prev of o.previous) {
            if (!prev) continue;
            isOverlapping = isOverlapping ||
                (definitelyBigger(prev.endTime, o.startTime)
                    && definitelyBigger(o.endTime, prev.endTime)
                    && definitelyBigger(o.startTime, prev.startTime));
            if (definitelyBigger(prev.endTime, o.endTime) && definitelyBigger(o.startTime, prev.startTime)) {
                holdFactor = 1.25;
            }
            closestEndTime = Math.min(closestEndTime, Math.abs(o.endTime - prev.endTime));
        }
        if (isOverlapping) holdAddition = logistic(closestEndTime, releaseThreshold, 0.27);
        return (1 + holdAddition) * holdFactor;
    };

    for (let i = 0; i < dho.length; i += 1) {
        const cur = dho[i];
        if (i === 0) {
            currentSectionEnd = Math.ceil(cur.startTime / sectionLength) * sectionLength;
        }
        while (cur.startTime > currentSectionEnd) {
            strainPeaks.push(currentSectionPeak);
            const prevStart = dho[i - 1].startTime;
            currentSectionPeak =
                applyDecay(highestIndividualStrain, currentSectionEnd - prevStart, individualDecayBase)
                + applyDecay(overallStrain, currentSectionEnd - prevStart, overallDecayBase);
            currentSectionEnd += sectionLength;
        }

        const col = cur.column;
        individualStrains[col] = applyDecay(individualStrains[col], cur.columnStrainTime, individualDecayBase);
        individualStrains[col] += individualEvaluator(cur);
        highestIndividualStrain = cur.deltaTime <= 1
            ? Math.max(highestIndividualStrain, individualStrains[col])
            : individualStrains[col];

        overallStrain = applyDecay(overallStrain, cur.deltaTime, overallDecayBase);
        overallStrain += overallEvaluator(cur);

        const strainValueOf = highestIndividualStrain + overallStrain - currentStrain;
        currentStrain += strainValueOf;
        currentSectionPeak = Math.max(currentStrain, currentSectionPeak);
    }

    // Difficulty value × 0.018.
    const peaks = [...strainPeaks, currentSectionPeak].filter((p) => p > 0).sort((a, b) => b - a);
    let difficulty = 0;
    let weight = 1;
    for (const peak of peaks) {
        difficulty += peak * weight;
        weight *= decayWeight;
    }
    const star = difficulty * 0.018;
    return Number.isFinite(star) ? star : null;
}

/**
 * Official osu!mania pp (osu-master ManiaPerformanceCalculator).
 * @param {{
 *   starRating: number,
 *   perfect: number, great: number, good: number, ok: number, meh: number, miss: number,
 *   noFail?: boolean, easy?: boolean,
 * }} args counts in geki/300/katu/100/50/0 order
 * @returns {number|null} official pp, or null on invalid input
 */
export function calculateOfficialPp({
    starRating, perfect, great, good, ok, meh, miss, noFail = false, easy = false,
}) {
    if (!(starRating > 0) || !Number.isFinite(starRating)) return null;
    if (perfect < 0 || great < 0 || good < 0 || ok < 0 || meh < 0 || miss < 0) return null;

    const totalHits = perfect + great + good + ok + meh + miss;
    const acc = totalHits === 0
        ? 0
        : (perfect * 320 + great * 300 + good * 200 + ok * 100 + meh * 50) / (totalHits * 320);
    const clampedAcc = Math.min(1, Math.max(0, acc));

    const difficultyValue = 8.0
        * Math.pow(Math.max(starRating - 0.15, 0.05), 2.2)
        * Math.max(0, 5 * clampedAcc - 4)
        * (1 + 0.1 * Math.min(1, totalHits / 1500));

    let multiplier = 1.0;
    if (noFail) multiplier *= 0.75;
    if (easy) multiplier *= 0.5;

    const pp = difficultyValue * multiplier;
    return Number.isFinite(pp) ? pp : null;
}