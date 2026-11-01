// estimatorMath.js —— 估算器共享基础数学与插值工具纯函数

export function clamp(value, min, max) {
    return Math.max(min, Math.min(max, value));
}

export function safeDiv(a, b, fallback = 0) {
    if (!Number.isFinite(a) || !Number.isFinite(b) || Math.abs(b) < 1e-9) return fallback;
    return a / b;
}

export function fmt4(value) {
    return Number.isFinite(value) ? Number(value.toFixed(4)) : null;
}

export function piecewiseLinear(x, knots, valueCol = 1) {
    const v = Number(x);
    if (!Number.isFinite(v) || !Array.isArray(knots) || knots.length === 0) return v;
    if (v <= knots[0][0]) return knots[0][valueCol];
    const last = knots.length - 1;
    if (v >= knots[last][0]) return knots[last][valueCol];
    for (let i = 0; i < last; i += 1) {
        const x0 = knots[i][0], y0 = knots[i][valueCol];
        const x1 = knots[i + 1][0], y1 = knots[i + 1][valueCol];
        if (v >= x0 && v <= x1) return y0 + safeDiv((v - x0) * (y1 - y0), x1 - x0, 0);
    }
    return v;
}

export function quantileFromSorted(sortedValues, q) {
    if (!sortedValues.length) {
        return 0;
    }

    const t = clamp(Number(q), 0, 1) * (sortedValues.length - 1);
    const left = Math.floor(t);
    const right = Math.min(sortedValues.length - 1, left + 1);
    const w = t - left;
    return (sortedValues[left] * (1 - w)) + (sortedValues[right] * w);
}

export function powerMean(values, p) {
    if (!values.length) {
        return 0;
    }

    let acc = 0;
    for (const value of values) {
        acc += Math.pow(Math.max(value, 0), p);
    }
    return Math.pow(acc / values.length, 1 / p);
}

export function topTailMean(sortedValues, topFraction = 0.2) {
    if (!sortedValues.length) {
        return 0;
    }
    const frac = clamp(Number(topFraction), 0.01, 1);
    const count = Math.max(1, Math.round(sortedValues.length * frac));
    const start = Math.max(0, sortedValues.length - count);
    let sum = 0;
    for (let i = start; i < sortedValues.length; i += 1) {
        sum += sortedValues[i];
    }
    return sum / count;
}
