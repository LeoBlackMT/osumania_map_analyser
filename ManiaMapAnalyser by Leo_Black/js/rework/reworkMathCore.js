// Shared math core for the Sunny and Daniel rework algorithms.
//
// Every function/constant in this file is TEXTUALLY IDENTICAL (whitespace and
// comments ignored) in js/rework/sunnyAlgorithm.js and js/rework/danielAlgorithm.js
// — extracted verbatim, never "improved". Both algorithms depend on the exact
// computation; any numeric/order change here silently changes both estimators.
// See docs/guides/module-conventions.md. Do NOT import this file from
// sunnyWindowAlgorithm.js yet (out of scope for the extraction PR).
//
// EXCEPTION since 042ccee (PR #47 C# osu-author-port sync): stepInterp and
// computeCAndKs are now used ONLY by danielAlgorithm — sunnyAlgorithm.js keeps
// local D1/D2 versions (stepInterp `<=`→`<` exact-match sampling; computeCAndKs
// gains CStepV2/noteHitTimesV2 for effectiveWeights) that differ from daniel's
// old semantics. Do NOT move them back into this shared core: sunny and daniel
// intentionally diverge here.

// Graph-smoothing window constants — used only by smoothDForGraph below.
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

const jackNerfer = (delta) => 1 - 7e-5 * ((0.15 + Math.abs(delta - 0.08)) ** (-4));

const targetPercentiles = [0.945, 0.935, 0.925, 0.915, 0.845, 0.835, 0.825, 0.815];

export {
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
    targetPercentiles
};
