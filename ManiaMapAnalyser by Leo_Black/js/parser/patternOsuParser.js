import { createBPM, createChart, createTimeItem, NoteType } from "../patterns/chart.js";

function parseSections(lines) {
    let sec = null;
    const out = {};

    for (const raw of lines) {
        const line = raw.trim();
        if (!line || line.startsWith("//")) continue;

        if (line.startsWith("[") && line.endsWith("]")) {
            sec = line.slice(1, -1);
            if (!out[sec]) out[sec] = [];
        } else if (sec != null) {
            out[sec].push(line);
        }
    }
    return out;
}

function parseKV(sectionLines) {
    const out = {};
    for (const line of sectionLines) {
        const idx = line.indexOf(":");
        if (idx < 0) continue;
        out[line.slice(0, idx).trim()] = line.slice(idx + 1).trim();
    }
    return out;
}

function xToColumn(x, keys) {
    let col = Math.trunc((x / 512.0) * keys);
    if (col < 0) col = 0;
    if (col > keys - 1) col = keys - 1;
    return col;
}

function findEarliestUpcomingRelease(holdingUntil) {
    let earliest = Infinity;
    for (const h of holdingUntil) {
        if (h != null && h < earliest) earliest = h;
    }
    return earliest;
}

function convertHitObjects(objects, keys) {
    const output = [];
    const holdingUntil = Array.from({ length: keys }, () => null);
    let lastRow = createTimeItem(-Infinity, []);

    function finishHolds(time) {
        let earliest = findEarliestUpcomingRelease(holdingUntil);

        while (earliest < time) {
            for (let k = 0; k < keys; k += 1) {
                if (holdingUntil[k] === earliest) {
                    if (earliest > lastRow.Time) {
                        lastRow = createTimeItem(earliest, Array.from({ length: keys }, () => NoteType.NOTHING));
                        output.push(lastRow);
                        for (let kk = 0; kk < keys; kk += 1) {
                            if (holdingUntil[kk] != null) {
                                lastRow.Data[kk] = NoteType.HOLDBODY;
                            }
                        }
                    }

                    const cur = lastRow.Data[k];
                    if (cur === NoteType.NOTHING || cur === NoteType.HOLDBODY) {
                        lastRow.Data[k] = NoteType.HOLDTAIL;
                        holdingUntil[k] = null;
                    } else {
                        throw new Error("impossible (HOLDTAIL overwrite conflict)");
                    }
                }
            }
            earliest = findEarliestUpcomingRelease(holdingUntil);
        }
    }

    function addNote(column, time) {
        finishHolds(time);

        if (time > lastRow.Time) {
            lastRow = createTimeItem(time, Array.from({ length: keys }, () => NoteType.NOTHING));
            output.push(lastRow);
            for (let k = 0; k < keys; k += 1) {
                if (holdingUntil[k] != null) lastRow.Data[k] = NoteType.HOLDBODY;
            }
        }

        const cur = lastRow.Data[column];
        if (cur === NoteType.NOTHING) {
            lastRow.Data[column] = NoteType.NORMAL;
        } else if (cur === NoteType.NORMAL || cur === NoteType.HOLDHEAD) {
            // keep stacked note behavior
        } else {
            throw new Error(`Stacked note at ${time}, column ${column + 1}, coincides with ${cur}`);
        }
    }

    function startHold(column, time, endTime) {
        finishHolds(time);

        if (time > lastRow.Time) {
            lastRow = createTimeItem(time, Array.from({ length: keys }, () => NoteType.NOTHING));
            output.push(lastRow);
            for (let k = 0; k < keys; k += 1) {
                if (holdingUntil[k] != null) lastRow.Data[k] = NoteType.HOLDBODY;
            }
        }

        const cur = lastRow.Data[column];
        if (cur === NoteType.NOTHING || cur === NoteType.NORMAL) {
            lastRow.Data[column] = NoteType.HOLDHEAD;
            holdingUntil[column] = endTime;
        } else {
            throw new Error(`Stacked LN at ${time}, column ${column + 1}, head coincides with ${cur}`);
        }
    }

    for (const obj of objects) {
        if (obj.kind === "HitCircle") {
            addNote(xToColumn(obj.X, keys), Number(obj.Time));
        } else if (obj.kind === "Hold") {
            if (obj.EndTime > obj.Time) {
                startHold(xToColumn(obj.X, keys), Number(obj.Time), Number(obj.EndTime));
            } else {
                addNote(xToColumn(obj.X, keys), Number(obj.Time));
            }
        }
    }

    finishHolds(Infinity);
    return output;
}

function findBpmDurations(points, endTime) {
    const uninherited = points.filter((p) => p.kind === "Uninherited");
    if (!uninherited.length) {
        throw new Error("Beatmap has no BPM points set");
    }

    const data = new Map();
    let current = Number(uninherited[0].MsPerBeat);
    let time = Number(uninherited[0].Time);

    for (const b of uninherited.slice(1)) {
        if (!data.has(current)) data.set(current, 0);
        data.set(current, data.get(current) + (Number(b.Time) - time));
        time = Number(b.Time);
        current = Number(b.MsPerBeat);
    }

    if (!data.has(current)) data.set(current, 0);
    data.set(current, data.get(current) + Math.max(endTime - time, 0));

    return data;
}

function convertTimingPoints(points, endTime) {
    const durations = findBpmDurations(points, endTime);
    const mostCommonMspb = [...durations.entries()].sort((a, b) => b[1] - a[1])[0][0];

    const sv = [];
    const bpm = [];
    let currentBpmMult = 1;

    for (const p of points) {
        if (p.kind === "Uninherited") {
            const mspb = Number(p.MsPerBeat);
            bpm.push(createTimeItem(Number(p.Time), createBPM(Number(p.Meter), mspb)));
            currentBpmMult = mspb !== 0 ? Number(mostCommonMspb) / mspb : 1;
            sv.push(createTimeItem(Number(p.Time), currentBpmMult));
        } else {
            sv.push(createTimeItem(Number(p.Time), currentBpmMult * Number(p.Multiplier)));
        }
    }

    return [bpm, sv];
}

function cleanedSv(sv) {
    if (!sv.length) return [];

    const rev = [...sv].reverse();
    const seen = new Set();
    const dedupRev = [];
    for (const item of rev) {
        if (seen.has(item.Time)) continue;
        seen.add(item.Time);
        dedupRev.push(item);
    }
    const dedup = dedupRev.reverse();

    const out = [];
    let previousValue = 1;
    for (const s of dedup) {
        if (Math.abs(Number(s.Data) - previousValue) > 0.005) {
            out.push(s);
            previousValue = Number(s.Data);
        }
    }
    return out;
}

function parseTimingPoints(lines) {
    const out = [];
    for (const line of lines) {
        const parts = line.split(",").map((p) => p.trim());
        if (parts.length < 2) continue;

        const t = Number.parseFloat(parts[0]);
        const beatLen = Number.parseFloat(parts[1]);
        const meter = parts.length > 2 && parts[2] ? Number.parseInt(parts[2], 10) : 4;
        const uninherited = parts.length > 6 && parts[6] ? Number.parseInt(parts[6], 10) : 1;

        if (uninherited === 1) {
            out.push({ kind: "Uninherited", Time: t, MsPerBeat: Math.max(0, beatLen), Meter: meter });
        } else if (beatLen !== 0) {
            out.push({ kind: "Inherited", Time: t, Multiplier: (-100.0 / beatLen) });
        }
    }
    return out;
}

function parseHitObjects(lines) {
    const out = [];
    for (const line of lines) {
        const parts = line.split(",").map((p) => p.trim());
        if (parts.length < 5) continue;

        const x = Number.parseFloat(parts[0]);
        const time = Math.trunc(Number.parseFloat(parts[2]));
        const typ = Number.parseInt(parts[3], 10);

        const isHold = (typ & 128) !== 0;
        if (isHold) {
            let endTime = time;
            if (parts.length >= 6 && parts[5]) {
                const endPart = parts[5].split(":")[0];
                const parsed = Number.parseFloat(endPart);
                if (!Number.isNaN(parsed)) endTime = Math.trunc(parsed);
            }
            out.push({ kind: "Hold", X: x, Time: time, EndTime: endTime });
        } else {
            out.push({ kind: "HitCircle", X: x, Time: time });
        }
    }
    return out;
}

export function parseOsuManiaFromText(osuText) {
    const lines = osuText.split(/\r?\n/);
    const sections = parseSections(lines);

    const diff = parseKV(sections.Difficulty || []);
    let keys = 4;
    try {
        keys = Math.trunc(Number.parseFloat(diff.CircleSize || "4"));
    } catch {
        keys = 4;
    }

    const timingPoints = parseTimingPoints(sections.TimingPoints || []);
    const hitObjects = parseHitObjects(sections.HitObjects || []);

    const snaps = convertHitObjects(hitObjects, keys);
    if (!snaps.length) {
        throw new Error("Beatmap has no hitobjects after conversion");
    }

    const endTime = snaps[snaps.length - 1].Time;
    let bpm = [];
    let sv = [];

    if (timingPoints.length) {
        [bpm, sv] = convertTimingPoints(timingPoints, endTime);
        sv = cleanedSv(sv);
    } else {
        bpm = [createTimeItem(0, createBPM(4, 500.0))];
        sv = [createTimeItem(0, 1.0)];
    }

    return createChart(keys, snaps, bpm, sv);
}
