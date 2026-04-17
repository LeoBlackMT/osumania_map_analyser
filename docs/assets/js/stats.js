export const BAND_ORDER = ["exact", "close", "moderate", "miss"];

export const BAND_META = Object.freeze({
    exact: {
        label: "Exact",
        maxAbsDelta: 0.2,
    },
    close: {
        label: "Close",
        maxAbsDelta: 0.5,
    },
    moderate: {
        label: "Moderate",
        maxAbsDelta: 1.0,
    },
    miss: {
        label: "Miss",
        maxAbsDelta: Number.POSITIVE_INFINITY,
    },
});

function round4(value) {
    return Number(Number(value).toFixed(4));
}

function average(values) {
    if (!values.length) {
        return null;
    }
    return values.reduce((sum, value) => sum + value, 0) / values.length;
}

function median(values) {
    if (!values.length) {
        return null;
    }

    const sorted = [...values].sort((a, b) => a - b);
    const middle = Math.floor(sorted.length / 2);

    if (sorted.length % 2 === 1) {
        return sorted[middle];
    }

    return (sorted[middle - 1] + sorted[middle]) / 2;
}

function percentile(values, p) {
    if (!values.length) {
        return null;
    }

    const sorted = [...values].sort((a, b) => a - b);
    const position = (sorted.length - 1) * p;
    const lower = Math.floor(position);
    const upper = Math.ceil(position);

    if (lower === upper) {
        return sorted[lower];
    }

    const ratio = position - lower;
    return sorted[lower] * (1 - ratio) + sorted[upper] * ratio;
}

function buildHistogram(values, step = 0.2) {
    if (!values.length) {
        return {
            labels: [],
            counts: [],
        };
    }

    const min = Math.min(...values);
    const max = Math.max(...values);
    let leftEdge = Math.floor(min / step) * step;
    let rightEdge = Math.ceil(max / step) * step;

    if (leftEdge === rightEdge) {
        rightEdge += step;
    }

    const labels = [];
    const counts = [];
    for (let edge = leftEdge; edge < rightEdge; edge = Number((edge + step).toFixed(6))) {
        const next = Number((edge + step).toFixed(6));
        labels.push(`${edge.toFixed(1)} to ${next.toFixed(1)}`);
        counts.push(0);
    }

    for (const value of values) {
        const idx = Math.min(
            counts.length - 1,
            Math.max(0, Math.floor((value - leftEdge) / step)),
        );
        counts[idx] += 1;
    }

    return {
        labels,
        counts,
    };
}

function computeTrendFitPercent(rows) {
    if (!rows.length) {
        return null;
    }

    const expectedValues = rows.map((row) => row.expected);
    const gotValues = rows.map((row) => row.got);
    const expectedMean = average(expectedValues);

    if (!Number.isFinite(expectedMean)) {
        return null;
    }

    let ssTot = 0;
    let ssRes = 0;
    for (let i = 0; i < rows.length; i += 1) {
        const expected = expectedValues[i];
        const got = gotValues[i];

        const diffMean = expected - expectedMean;
        ssTot += diffMean * diffMean;

        const diffFit = expected - got;
        ssRes += diffFit * diffFit;
    }

    if (ssTot === 0) {
        return ssRes === 0 ? 100 : 0;
    }

    const r2 = 1 - (ssRes / ssTot);
    const clamped = Math.max(0, Math.min(r2, 1));
    return clamped * 100;
}

export function classifyBand(absDelta) {
    if (!Number.isFinite(absDelta)) {
        return "miss";
    }

    if (absDelta <= BAND_META.exact.maxAbsDelta) {
        return "exact";
    }

    if (absDelta <= BAND_META.close.maxAbsDelta) {
        return "close";
    }

    if (absDelta <= BAND_META.moderate.maxAbsDelta) {
        return "moderate";
    }

    return "miss";
}

export function buildRowKey(row) {
    return [
        String(row?.name ?? "").trim(),
        String(row?.pattern ?? "").trim(),
        String(row?.subPattern ?? "").trim(),
    ].join("\u0001");
}

export function computeSummary(rows) {
    const validRows = [];
    const bandCounts = {
        exact: 0,
        close: 0,
        moderate: 0,
        miss: 0,
    };

    const scatterByBand = {
        exact: [],
        close: [],
        moderate: [],
        miss: [],
    };

    const patternAcc = new Map();
    const subPatternAcc = new Map();

    for (const row of rows) {
        const expected = row.expected;
        const got = row.got;
        if (!Number.isFinite(expected) || !Number.isFinite(got)) {
            continue;
        }

        const delta = Number.isFinite(row.delta)
            ? row.delta
            : expected - got;
        const deltaAbs = Number.isFinite(row.deltaAbs)
            ? row.deltaAbs
            : Math.abs(delta);

        const band = classifyBand(deltaAbs);
        bandCounts[band] += 1;

        const normalized = {
            ...row,
            expected,
            got,
            delta,
            deltaAbs,
            band,
        };

        validRows.push(normalized);
        scatterByBand[band].push(normalized);

        const patternKey = String(row.pattern || "unknown").trim() || "unknown";
        if (!patternAcc.has(patternKey)) {
            patternAcc.set(patternKey, {
                pattern: patternKey,
                count: 0,
                sumAbs: 0,
                sumDelta: 0,
            });
        }

        const slot = patternAcc.get(patternKey);
        slot.count += 1;
        slot.sumAbs += deltaAbs;
        slot.sumDelta += delta;

        const subPatternKey = String(row.subPattern || "Unsigned").trim() || "Unsigned";
        if (!subPatternAcc.has(subPatternKey)) {
            subPatternAcc.set(subPatternKey, {
                subPattern: subPatternKey,
                count: 0,
                sumAbs: 0,
                sumDelta: 0,
            });
        }

        const subSlot = subPatternAcc.get(subPatternKey);
        subSlot.count += 1;
        subSlot.sumAbs += deltaAbs;
        subSlot.sumDelta += delta;
    }

    const deltas = validRows.map((row) => row.delta);
    const absDeltas = validRows.map((row) => row.deltaAbs);

    const mae = average(absDeltas);
    const rmse = absDeltas.length
        ? Math.sqrt(average(deltas.map((delta) => delta * delta)))
        : null;
    const bias = average(deltas);
    const medianAbs = median(absDeltas);
    const p90Abs = percentile(absDeltas, 0.9);

    const maxUnderrate = deltas.length ? Math.max(...deltas) : null;
    const maxOverrate = deltas.length ? Math.min(...deltas) : null;

    const patternRows = [...patternAcc.values()]
        .map((item) => ({
            pattern: item.pattern,
            count: item.count,
            mae: item.count ? item.sumAbs / item.count : 0,
            bias: item.count ? item.sumDelta / item.count : 0,
        }))
        .sort((a, b) => b.mae - a.mae);

    const subPatternRows = [...subPatternAcc.values()]
        .map((item) => ({
            subPattern: item.subPattern,
            count: item.count,
            mae: item.count ? item.sumAbs / item.count : 0,
            bias: item.count ? item.sumDelta / item.count : 0,
        }))
        .sort((a, b) => b.mae - a.mae);

    const trendRows = [...validRows]
        .sort((a, b) => (a.expected - b.expected) || a.name.localeCompare(b.name));

    const topUnderrated = [...validRows]
        .filter((row) => Number.isFinite(row.delta) && row.delta > 0)
        .sort((a, b) => b.delta - a.delta)
        .slice(0, 8);

    const topOverrated = [...validRows]
        .filter((row) => Number.isFinite(row.delta) && row.delta < 0)
        .sort((a, b) => a.delta - b.delta)
        .slice(0, 8);

    const trendFitPercent = computeTrendFitPercent(trendRows);

    const totalValid = validRows.length;
    const bandRates = BAND_ORDER.reduce((acc, key) => {
        acc[key] = totalValid > 0 ? (bandCounts[key] / totalValid) * 100 : 0;
        return acc;
    }, {});

    const coverage = rows.length > 0 ? (totalValid / rows.length) * 100 : 0;

    return {
        totalRows: rows.length,
        validRows: totalValid,
        bandCounts,
        bandRates,
        metrics: {
            mae: Number.isFinite(mae) ? round4(mae) : null,
            rmse: Number.isFinite(rmse) ? round4(rmse) : null,
            bias: Number.isFinite(bias) ? round4(bias) : null,
            medianAbs: Number.isFinite(medianAbs) ? round4(medianAbs) : null,
            p90Abs: Number.isFinite(p90Abs) ? round4(p90Abs) : null,
            maxUnderrate: Number.isFinite(maxUnderrate) ? round4(maxUnderrate) : null,
            maxOverrate: Number.isFinite(maxOverrate) ? round4(maxOverrate) : null,
            coverage: round4(coverage),
            trendFitPercent: Number.isFinite(trendFitPercent) ? round4(trendFitPercent) : null,
        },
        validDataRows: validRows,
        scatterByBand,
        trendRows,
        patternRows,
        subPatternRows,
        deltaHistogram: buildHistogram(deltas),
        topUnderrated,
        topOverrated,
    };
}

export function computeHeadToHead(baseRows, compareRows) {
    const compareMap = new Map(compareRows.map((row) => [buildRowKey(row), row]));

    let matchedRows = 0;
    let baseWins = 0;
    let compareWins = 0;
    let tieCount = 0;
    let agreementCount = 0;

    const baseAbsList = [];
    const compareAbsList = [];
    const points = [];

    for (const baseRow of baseRows) {
        const key = buildRowKey(baseRow);
        const compareRow = compareMap.get(key);
        if (!compareRow) {
            continue;
        }

        const baseAbs = baseRow.deltaAbs;
        const compareAbs = compareRow.deltaAbs;
        if (!Number.isFinite(baseAbs) || !Number.isFinite(compareAbs)) {
            continue;
        }

        matchedRows += 1;
        baseAbsList.push(baseAbs);
        compareAbsList.push(compareAbs);

        if (baseAbs < compareAbs) {
            baseWins += 1;
        } else if (baseAbs > compareAbs) {
            compareWins += 1;
        } else {
            tieCount += 1;
        }

        if (classifyBand(baseAbs) === classifyBand(compareAbs)) {
            agreementCount += 1;
        }

        points.push({
            x: baseAbs,
            y: compareAbs,
            name: baseRow.name,
            baseDeltaAbs: baseAbs,
            compareDeltaAbs: compareAbs,
            pattern: baseRow.pattern,
        });
    }

    const baseMae = average(baseAbsList);
    const compareMae = average(compareAbsList);
    const maeGap = (Number.isFinite(baseMae) && Number.isFinite(compareMae))
        ? baseMae - compareMae
        : null;

    const agreementRate = matchedRows > 0
        ? (agreementCount / matchedRows) * 100
        : 0;

    return {
        matchedRows,
        baseWins,
        compareWins,
        tieCount,
        agreementRate: round4(agreementRate),
        baseMae: Number.isFinite(baseMae) ? round4(baseMae) : null,
        compareMae: Number.isFinite(compareMae) ? round4(compareMae) : null,
        maeGap: Number.isFinite(maeGap) ? round4(maeGap) : null,
        points,
    };
}
