import { OsuFileParser } from "../parser/osuFileParser.js";
import { NoteType, createEmptyRow, isRowEmpty } from "./types.js";

function normalizeCvtFlag(cvtFlag) {
    const normalized = String(cvtFlag || "").trim().toUpperCase();
    if (normalized === "IN" || normalized === "HO") {
        return normalized;
    }
    return null;
}

function setNoteType(row, key, noteType) {
    if (!Array.isArray(row) || key < 0 || key >= row.length) {
        return;
    }

    if (row[key] === NoteType.NOTHING) {
        row[key] = noteType;
    }
}

// First index in sortedAsc where value > target; sortedAsc.length if none.
function lowerBoundGreater(sortedAsc, target) {
    let lo = 0;
    let hi = sortedAsc.length;
    while (lo < hi) {
        const mid = (lo + hi) >> 1;
        if (sortedAsc[mid] <= target) {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    return lo;
}

function getOrCreateRow(rowMap, keyCount, time) {
    if (!rowMap.has(time)) {
        rowMap.set(time, createEmptyRow(keyCount));
    }
    return rowMap.get(time);
}

function applyConversionFlag(parser, cvtFlag) {
    const normalized = normalizeCvtFlag(cvtFlag);
    if (normalized === "IN") {
        parser.modIN();
    } else if (normalized === "HO") {
        parser.modHO();
    }
}

function isLikelyOsuText(value) {
    if (typeof value !== "string") {
        return false;
    }
    return value.includes("[HitObjects]") && (value.includes("\n") || value.includes("\r"));
}

function isProcessedOsuParser(source) {
    return Boolean(source)
        && typeof source === "object"
        && typeof source.process === "function"
        && typeof source.getParsedData === "function";
}

// Field-copy clone of a processed OsuFileParser. modIN/modHO mutate the parser
// in place, so a shared `parsed` instance must be isolated on a clone before
// converting — the shared chart stays pristine for later consumers (mutability
// audit, perf task 9; same pattern as sunnyAlgorithm.js cloneOsuParser).
function cloneOsuParser(src) {
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

// Build interlude rows from an ALREADY-processed OsuFileParser instance
// (shared-parse path used by runAnalysisPipeline). The shared instance stays
// pristine: cvtFlag IN/HO conversion runs on a clone.
function buildRowsFromSharedParser(parser, cvtFlag) {
    const parsed = parser.getParsedData();
    if (parsed.status === "NotMania") {
        throw new Error("Beatmap mode is not mania");
    }
    if (parsed.status === "Fail") {
        throw new Error("Beatmap parse failed");
    }
    const normalized = normalizeCvtFlag(cvtFlag);
    const rowsSource = (normalized === "IN" || normalized === "HO")
        ? (() => {
            const clone = cloneOsuParser(parser);
            applyConversionFlag(clone, cvtFlag);
            return clone.getParsedData();
        })()
        : parsed;
    return {
        keyCount: Number(rowsSource.columnCount) || 0,
        rows: buildRowsFromParsed(rowsSource),
    };
}

async function resolveOsuText(source) {
    if (typeof source === "string") {
        if (isLikelyOsuText(source)) {
            return source;
        }

        const response = await fetch(source, {
            method: "GET",
            cache: "no-store",
        });
        if (!response.ok) {
            throw new Error(`Interlude source request failed with status ${response.status}`);
        }
        return await response.text();
    }

    if (source && typeof source === "object" && typeof source.osuText === "string") {
        return source.osuText;
    }

    throw new Error("Unsupported Interlude source. Provide osu text, beatmap path, or OsuFileParser with osuText.");
}

function buildRowsFromParsed(parsed) {
    const keyCount = Number(parsed?.columnCount) || 0;
    if (keyCount < 3 || keyCount > 10) {
        return [];
    }

    const columns = Array.isArray(parsed?.columns) ? parsed.columns : [];
    const noteStarts = Array.isArray(parsed?.noteStarts) ? parsed.noteStarts : [];
    const noteEnds = Array.isArray(parsed?.noteEnds) ? parsed.noteEnds : [];
    const noteTypes = Array.isArray(parsed?.noteTypes) ? parsed.noteTypes : [];

    const rowMap = new Map();
    const holdSpans = [];

    for (let i = 0; i < columns.length; i += 1) {
        const key = Number(columns[i]);
        const startTime = Number(noteStarts[i]);
        const endTime = Number(noteEnds[i]);
        const rawType = Number(noteTypes[i]) || 0;
        const isLongNote = (rawType & 128) !== 0;

        if (!Number.isFinite(key) || key < 0 || key >= keyCount || !Number.isFinite(startTime)) {
            continue;
        }

        const startRow = getOrCreateRow(rowMap, keyCount, startTime);
        if (isLongNote) {
            setNoteType(startRow, key, NoteType.HOLDHEAD);

            if (Number.isFinite(endTime) && endTime > startTime) {
                const endRow = getOrCreateRow(rowMap, keyCount, endTime);
                setNoteType(endRow, key, NoteType.HOLDTAIL);
                holdSpans.push({ key, startTime, endTime });
            }
        } else {
            setNoteType(startRow, key, NoteType.NORMAL);
        }
    }

    const sortedTimes = Array.from(rowMap.keys()).sort((a, b) => a - b);

    // ponytail: sortedTimes is sorted above, so each hold span only needs its
    // first row time > startTime (binary search), then a linear scan until
    // endTime. O(rows per span) instead of O(rows per span + all rows).
    for (let i = 0; i < holdSpans.length; i += 1) {
        const { key, startTime, endTime } = holdSpans[i];
        let t = lowerBoundGreater(sortedTimes, startTime);
        for (; t < sortedTimes.length && sortedTimes[t] < endTime; t += 1) {
            const row = rowMap.get(sortedTimes[t]);
            if (row[key] === NoteType.NOTHING) {
                row[key] = NoteType.HOLDBODY;
            }
        }
    }

    return sortedTimes
        .map((time) => ({
            time,
            data: rowMap.get(time),
        }))
        .filter((row) => !isRowEmpty(row.data));
}

export async function buildInterludeRows(source, cvtFlag = null) {
    if (isProcessedOsuParser(source)) {
        return buildRowsFromSharedParser(source, cvtFlag);
    }
    const osuText = await resolveOsuText(source);
    const parser = new OsuFileParser(osuText);
    parser.process();

    applyConversionFlag(parser, cvtFlag);

    const parsed = parser.getParsedData();
    if (parsed.status === "NotMania") {
        throw new Error("Beatmap mode is not mania");
    }
    if (parsed.status === "Fail") {
        throw new Error("Beatmap parse failed");
    }

    return {
        keyCount: Number(parsed.columnCount) || 0,
        rows: buildRowsFromParsed(parsed),
    };
}
