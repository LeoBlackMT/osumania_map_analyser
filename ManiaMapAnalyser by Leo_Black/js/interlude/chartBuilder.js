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
    } else if (row[key] === NoteType.HOLDTAIL && noteType === NoteType.NORMAL) {
        row[key] = NoteType.HOLDTAIL_NORMAL;
    } else if (row[key] === NoteType.HOLDTAIL && noteType === NoteType.HOLDHEAD) {
        row[key] = NoteType.HOLDTAIL_HOLDHEAD;
    } else if (noteType === NoteType.HOLDTAIL && row[key] === NoteType.NORMAL) {
        row[key] = NoteType.HOLDTAIL_NORMAL;
    } else if (noteType === NoteType.HOLDTAIL && row[key] === NoteType.HOLDHEAD) {
        row[key] = NoteType.HOLDTAIL_HOLDHEAD;
    }
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
    const holdStartsAt = new Map();
    const holdEndsAt = new Map();

    const addHoldEvent = (events, time, key) => {
        if (!events.has(time)) events.set(time, []);
        events.get(time).push(key);
    };

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
                addHoldEvent(holdStartsAt, startTime, key);
                addHoldEvent(holdEndsAt, endTime, key);
            }
        } else {
            setNoteType(startRow, key, NoteType.NORMAL);
        }
    }

    const sortedTimes = Array.from(rowMap.keys()).sort((a, b) => a - b);

    const activeHolds = new Array(keyCount).fill(0);
    for (const time of sortedTimes) {
        for (const key of holdEndsAt.get(time) || []) {
            activeHolds[key] = Math.max(0, activeHolds[key] - 1);
        }

        const row = rowMap.get(time);
        for (let key = 0; key < keyCount; key += 1) {
            if (activeHolds[key] > 0 && row[key] === NoteType.NOTHING) {
                row[key] = NoteType.HOLDBODY;
            }
        }

        for (const key of holdStartsAt.get(time) || []) {
            activeHolds[key] += 1;
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
