import { OsuFileParser } from "../parser/osuFileParser.js";
import { calculateFromParsed as calculateSunnyRework } from "./sunny-rework/algorithm.js";
import { bisectLeft, interpValues } from "./sunny-rework/math-utils.js";

const GRAPH_BREAK_ZERO_THRESHOLD_MS = 400;
const GRAPH_RESAMPLE_INTERVAL_MS = 100;
const GRAPH_SMOOTH_SIGMA_MS = 800;

// Vendored from https://github.com/ZHAO20060708/sunny-rework-js (compatibility target
// 2025-04-15); keep this adapter aligned with that package's parsed-data API.
// Adapter for the standalone sunny-rework-js implementation. The estimator
// layer still calls the historical text-based API, so conversion and mod
// handling stay here at the project boundary.
function resolveOd(baseOd, odFlag) {
    if (odFlag == null) return baseOd;
    if (odFlag === "HR") return 6.462 + 0.715 * baseOd;
    if (odFlag === "EZ") return -20.761 + 2.566 * baseOd;
    const numeric = Number(odFlag);
    return Number.isFinite(numeric) ? numeric : baseOd;
}

function normalizeCvtFlag(cvtFlag) {
    return String(cvtFlag || "").trim().toUpperCase();
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
        const value = Math.exp(-0.5 * ((i / sigmaSamples) ** 2));
        kernel[i + radius] = value;
        kernelSum += value;
    }
    for (let i = 0; i < kernelSize; i += 1) {
        kernel[i] /= kernelSum;
    }

    const padded = new Float64Array(data.length + radius * 2);
    for (let i = 0; i < data.length; i += 1) {
        padded[i + radius] = data[i];
    }

    const result = new Float64Array(data.length);
    for (let i = 0; i < data.length; i += 1) {
        let sum = 0;
        for (let k = 0; k < kernelSize; k += 1) {
            sum += padded[i + k] * kernel[k];
        }
        result[i] = sum;
    }
    return Array.from(result);
}

function noteDistanceFromSortedTimes(noteTimes, time) {
    const index = bisectLeft(noteTimes, time);
    const after = index < noteTimes.length ? Math.abs(noteTimes[index] - time) : Number.POSITIVE_INFINITY;
    const before = index > 0 ? Math.abs(noteTimes[index - 1] - time) : Number.POSITIVE_INFINITY;
    return Math.min(after, before);
}

function applyGraphProximityEnvelope(times, values, noteTimes) {
    if (!noteTimes.length) {
        return Array.from(values);
    }

    const result = new Float64Array(times.length);
    for (let i = 0; i < times.length; i += 1) {
        const distance = noteDistanceFromSortedTimes(noteTimes, times[i]);
        const ratio = Math.max(0, Math.min(distance / 500, 1));
        const envelope = 0.5 * (1 + Math.cos(Math.PI * ratio));
        result[i] = values[i] * envelope;
    }
    return Array.from(result);
}

// Keep the graph presentation aligned with the legacy Sunny implementation:
// resample, blank breaks, apply a Gaussian pass, then restore the same gaps.
function smoothGraphSeries(graph, noteStarts) {
    if (!graph || !Array.isArray(graph.times) || !Array.isArray(graph.values) || graph.times.length < 2) {
        return graph;
    }

    const times = graph.times;
    const values = graph.values;
    const noteTimes = noteStarts
        .map((value) => Number(value))
        .filter((value) => Number.isFinite(value))
        .sort((a, b) => a - b);
    const start = times[0];
    const end = times[times.length - 1];
    const uniformTimes = [];
    for (let time = start; time <= end + GRAPH_RESAMPLE_INTERVAL_MS; time += GRAPH_RESAMPLE_INTERVAL_MS) {
        uniformTimes.push(time);
    }

    const enveloped = applyGraphProximityEnvelope(times, values, noteTimes);
    const uniformValues = interpValues(uniformTimes, times, enveloped);
    if (noteTimes.length) {
        for (let i = 0; i < uniformTimes.length; i += 1) {
            if (noteDistanceFromSortedTimes(noteTimes, uniformTimes[i]) > GRAPH_BREAK_ZERO_THRESHOLD_MS) {
                uniformValues[i] = 0;
            }
        }
    }

    const smoothed = gaussianFilter1d(
        uniformValues,
        GRAPH_SMOOTH_SIGMA_MS / GRAPH_RESAMPLE_INTERVAL_MS,
    );

    if (noteTimes.length) {
        for (let i = 0; i < uniformTimes.length; i += 1) {
            if (noteDistanceFromSortedTimes(noteTimes, uniformTimes[i]) > GRAPH_BREAK_ZERO_THRESHOLD_MS) {
                smoothed[i] = 0;
            }
        }
    }

    return {
        times: times.slice(),
        values: interpValues(times, uniformTimes, smoothed),
    };
}

function parseInput(osuText, speedRate, odFlag, cvtFlag) {
    const parser = new OsuFileParser(osuText);
    parser.process();

    if (parser.status === "Fail" || parser.status === "NotMania") {
        return { status: parser.status };
    }

    const cvt = normalizeCvtFlag(cvtFlag);
    if (cvt.includes("IN")) parser.modIN();
    if (cvt.includes("HO")) parser.modHO();

    const parsed = parser.getParsedData();
    const rate = Number(speedRate);
    const timeScale = Number.isFinite(rate) && rate > 0 ? 1 / rate : 1;
    const starts = parsed.noteStarts.map((value) => Math.floor(Number(value) * timeScale));
    const ends = parsed.noteEnds.map((value, index) => {
        const isLong = (parsed.noteTypes[index] & 128) !== 0;
        return isLong ? Math.floor(Number(value) * timeScale) : -1;
    });

    return {
        status: "OK",
        parsed: [
            parsed.columnCount,
            parsed.columns,
            starts,
            ends,
            parsed.noteTypes,
            resolveOd(parsed.od, odFlag),
            8,
        ],
        lnRatio: parsed.lnRatio,
        columnCount: parsed.columnCount,
        noteStarts: starts,
    };
}

export function calculate(osuText, speedRate = 1.0, odFlag = null, cvtFlag = null, options = {}) {
    const input = parseInput(osuText, speedRate, odFlag, cvtFlag);
    if (input.status === "Fail") return -1;
    if (input.status === "NotMania") return -2;

    const withGraph = options?.withGraph === true;
    const result = calculateSunnyRework(input.parsed, "NM", { withGraph });
    if (!result || !Number.isFinite(result.sr)) return -1;

    if (options?.withGraph === true) {
        return {
            star: result.sr,
            lnRatio: input.lnRatio,
            columnCount: input.columnCount,
            graph: smoothGraphSeries(result.graph, input.noteStarts),
        };
    }

    return [result.sr, input.lnRatio, input.columnCount];
}
