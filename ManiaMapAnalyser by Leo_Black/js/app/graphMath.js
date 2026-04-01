export function buildLinePath(points) {
    if (!points.length) {
        return "";
    }

    let path = `M ${points[0][0].toFixed(2)} ${points[0][1].toFixed(2)}`;
    for (let i = 1; i < points.length; i += 1) {
        path += ` L ${points[i][0].toFixed(2)} ${points[i][1].toFixed(2)}`;
    }
    return path;
}

export function buildFillPath(points, baseY) {
    if (!points.length) {
        return "";
    }

    let path = `M ${points[0][0].toFixed(2)} ${baseY.toFixed(2)}`;
    path += ` L ${points[0][0].toFixed(2)} ${points[0][1].toFixed(2)}`;
    for (let i = 1; i < points.length; i += 1) {
        path += ` L ${points[i][0].toFixed(2)} ${points[i][1].toFixed(2)}`;
    }
    path += ` L ${points[points.length - 1][0].toFixed(2)} ${baseY.toFixed(2)} Z`;
    return path;
}

export function normalizeGraphSeries(graphData, resampleIntervalMs) {
    const rawTimes = Array.isArray(graphData?.times) ? graphData.times : [];
    const rawValues = Array.isArray(graphData?.values) ? graphData.values : [];

    const length = Math.max(rawTimes.length, rawValues.length);
    if (length < 2) {
        return null;
    }

    const times = [];
    const values = [];

    let lastTime = Number.NEGATIVE_INFINITY;
    let lastValue = 0;
    for (let i = 0; i < length; i += 1) {
        let time = rawTimes.length > 0 ? Number(rawTimes[i]) : i * resampleIntervalMs;
        let value = rawValues.length > 0 ? Number(rawValues[i]) : lastValue;

        if (!Number.isFinite(time)) {
            continue;
        }

        if (!Number.isFinite(value)) {
            value = values.length > 0 ? values[values.length - 1] : 0;
        }

        if (time <= lastTime) {
            time = lastTime + 1;
        }

        times.push(time);
        values.push(value);
        lastTime = time;
        lastValue = value;
    }

    if (times.length < 2) {
        return null;
    }

    return { times, values };
}

export function interpolateSeriesValue(times, values, targetTime) {
    if (!times.length || !values.length) {
        return 0;
    }

    if (targetTime <= times[0]) {
        return values[0];
    }
    if (targetTime >= times[times.length - 1]) {
        return values[values.length - 1];
    }

    let lo = 0;
    let hi = times.length - 1;
    while (lo + 1 < hi) {
        const mid = (lo + hi) >> 1;
        if (times[mid] <= targetTime) {
            lo = mid;
        } else {
            hi = mid;
        }
    }

    const x0 = times[lo];
    const x1 = times[hi];
    const y0 = values[lo];
    const y1 = values[hi];
    if (x1 === x0) {
        return y0;
    }

    const t = (targetTime - x0) / (x1 - x0);
    return y0 + t * (y1 - y0);
}
