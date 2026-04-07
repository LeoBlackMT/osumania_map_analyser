function stringToInt(value) {
    return Math.trunc(Number.parseFloat(value));
}

function bisectRight(arr, target) {
    let lo = 0;
    let hi = arr.length;
    while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (arr[mid] <= target) {
            lo = mid + 1;
    } else {
            hi = mid;
    }
    }
    return lo;
}

export class OsuFileParser {
    constructor(osuText) {
    this.osuText = osuText;
    this.od = -1;
    this.columnCount = -1;
    this.columns = [];
    this.noteStarts = [];
    this.noteEnds = [];
    this.noteTypes = [];
    this.gameMode = null;
    this.status = "init";
    this.lnRatio = 0;
    this.noteTimes = {};
    this.metaData = {};
    this.breaks = [];
    this.objectIntervals = [];
    this.timingPoints = [];
    }

    getParsedData() {
    return {
            columnCount: this.columnCount,
            columns: this.columns,
            noteStarts: this.noteStarts,
            noteEnds: this.noteEnds,
            noteTypes: this.noteTypes,
            od: this.od,
            gameMode: this.gameMode,
            status: this.status,
            lnRatio: this.lnRatio,
            metaData: this.metaData,
            breaks: this.breaks,
            objectIntervals: this.objectIntervals,
    };
    }

    process() {
    const lines = this.osuText.split(/\r?\n/);

    let inMetadataSection = false;
    let inEventsSection = false;
    let inTimingSection = false;

    for (let i = 0; i < lines.length; i += 1) {
            const line = lines[i].trim();
            if (!line) {
        continue;
            }

            if (line === "[Metadata]") {
        inMetadataSection = true;
        inEventsSection = false;
        inTimingSection = false;
        continue;
            }
            if (line === "[Events]") {
        inMetadataSection = false;
        inEventsSection = true;
        inTimingSection = false;
        continue;
            }
            if (line === "[TimingPoints]") {
        inMetadataSection = false;
        inEventsSection = false;
        inTimingSection = true;
        continue;
            }
            if (line.startsWith("[") && line.endsWith("]")) {
        inMetadataSection = false;
        inEventsSection = false;
        inTimingSection = false;
            }

            if (inMetadataSection && line.includes(":")) {
        const splitIdx = line.indexOf(":");
        const key = line.slice(0, splitIdx).trim();
        const value = line.slice(splitIdx + 1).trim();
        this.metaData[key] = value;
            }

            if (inEventsSection) {
        this.parseEventLine(line);
            }

            if (inTimingSection) {
        this.parseTimingPointLine(line);
            }

            if (line.includes("OverallDifficulty:")) {
        const odPart = line.split(":")[1];
        if (odPart != null) {
                    const parsed = Number.parseFloat(odPart.trim());
                    if (!Number.isNaN(parsed)) this.od = parsed;
        }
            }

            if (line.includes("CircleSize:")) {
        const csPart = line.split(":")[1];
        if (csPart != null) {
                    const cs = csPart.trim();
                    this.columnCount = cs === "0" ? 10 : stringToInt(cs);
        }
            }

            if (line.includes("Mode:")) {
        const modePart = line.split(":")[1];
        if (modePart != null) {
                    const mode = modePart.trim();
                    this.gameMode = mode;
                    if (mode !== "3") {
            this.status = "NotMania";
                    }
        }
            }

            if (line === "[HitObjects]") {
        for (let j = i + 1; j < lines.length; j += 1) {
                    const objLine = lines[j].trim();
                    if (!objLine) continue;
                    this.parseHitObject(objLine);
        }
        break;
            }
    }

    this.lnRatio = this.getLNRatio();
    this.noteTimes = this.getNoteTimes();
    this.objectIntervals = this.getObjectIntervals();

    if (!this.timingPoints.length) {
            this.timingPoints = [[0, 500.0]];
    }
    this.timingPoints.sort((a, b) => a[0] - b[0]);

    if (this.status !== "Fail" && this.status !== "NotMania") {
            this.status = "OK";
    }
    }

    parseEventLine(eventLine) {
    if (!eventLine || eventLine.startsWith("//")) return;

    const params = eventLine.split(",").map((part) => part.trim());
    if (params.length < 3) return;

    if (params[0] !== "2" && params[0] !== "Break") return;

    const breakStart = Number.parseInt(params[1], 10);
    const breakEnd = Number.parseInt(params[2], 10);
    if (Number.isNaN(breakStart) || Number.isNaN(breakEnd)) return;

    if (breakEnd > breakStart) {
            this.breaks.push([breakStart, breakEnd]);
    }
    }

    parseHitObject(objectLine) {
    const params = objectLine.split(",");
    if (params.length < 5) return;

    try {
            const x = stringToInt(params[0]);
            let column = 0;
            if (this.columnCount > 0) {
        // Keep lane mapping proportional to 512 width to avoid skew on keymodes
        // where 512 is not divisible by key count.
        column = Math.trunc((x * this.columnCount) / 512);
        column = Math.min(this.columnCount - 1, Math.max(0, column));
            }
            this.columns.push(column);

            const noteStart = Number.parseInt(params[2], 10);
            const noteType = Number.parseInt(params[3], 10);
            this.noteStarts.push(noteStart);
            this.noteTypes.push(noteType);

            let noteEnd = noteStart;
            if ((noteType & 128) !== 0 && params.length >= 6) {
        const lastParamChunk = params[5].split(":");
        noteEnd = Number.parseInt(lastParamChunk[0], 10);
            }
            this.noteEnds.push(noteEnd);
    } catch {
            this.status = "Fail";
    }
    }

    parseTimingPointLine(timingLine) {
    if (!timingLine || timingLine.startsWith("//")) return;

    const parts = timingLine.split(",").map((item) => item.trim());
    if (parts.length < 2) return;

    const t = Math.trunc(Number.parseFloat(parts[0]));
    const beatLength = Number.parseFloat(parts[1]);
    const uninherited = parts.length > 6 && parts[6] ? Number.parseInt(parts[6], 10) : 1;

    if (!Number.isNaN(t) && !Number.isNaN(beatLength) && uninherited === 1 && beatLength > 0) {
            this.timingPoints.push([t, beatLength]);
    }
    }

    getBeatLengthAt(timeMs) {
    if (!this.timingPoints.length) return 500.0;
    const times = this.timingPoints.map((tp) => tp[0]);
    const idx = bisectRight(times, Math.trunc(timeMs)) - 1;
    if (idx < 0) return this.timingPoints[0][1];
    return this.timingPoints[idx][1];
    }

    getLNRatio() {
    const totalNotes = this.noteTypes.length;
    if (!totalNotes) return 0;
    let lnCount = 0;
    for (const t of this.noteTypes) {
            if ((t & 128) !== 0) lnCount += 1;
    }
    return lnCount / totalNotes;
    }

    getColumnCount() {
    return this.columnCount;
    }

    getNoteTimes() {
    const noteTimes = {};
    for (let i = 0; i < this.columns.length; i += 1) {
            const col = this.columns[i];
            const time = this.noteStarts[i];
            if (!noteTimes[col]) noteTimes[col] = [];
            noteTimes[col].push(time);
    }

    for (const key of Object.keys(noteTimes)) {
            noteTimes[key].sort((a, b) => a - b);
    }

    return noteTimes;
    }

    getObjectIntervals() {
    if (!this.noteStarts.length) return [];

    const sortedStarts = [...this.noteStarts].sort((a, b) => a - b);
    const intervals = [];
    let prevStart = null;
    for (const startTime of sortedStarts) {
            const interval = prevStart == null ? 0 : startTime - prevStart;
            intervals.push([startTime, interval]);
            prevStart = startTime;
    }

    intervals.sort((a, b) => {
            if (b[1] !== a[1]) return b[1] - a[1];
            return a[0] - b[0];
    });

    return intervals;
    }

    modIN() {
    const startsByCol = {};
    for (let i = 0; i < this.columns.length; i += 1) {
            const col = this.columns[i];
            const start = this.noteStarts[i];
            if (!startsByCol[col]) startsByCol[col] = [];
            startsByCol[col].push(Number(start));
    }

    const newObjects = [];
    for (const colText of Object.keys(startsByCol)) {
            const col = Number.parseInt(colText, 10);
            const locations = startsByCol[col];

            locations.sort((a, b) => a - b);

            for (let i = 0; i < locations.length - 1; i += 1) {
        const startTime = locations[i];
        const nextTime = locations[i + 1];
        let duration = nextTime - startTime;
        const beatLength = this.getBeatLengthAt(nextTime);
        duration = Math.max(duration / 2, duration - beatLength / 4);
        const endTime = startTime + duration;

        newObjects.push([
                    Math.round(startTime),
                    col,
                    Math.round(endTime),
        ]);
            }
    }

    newObjects.sort((a, b) => {
            if (a[0] !== b[0]) return a[0] - b[0];
            return a[1] - b[1];
    });

    this.columns = newObjects.map((obj) => obj[1]);
    this.noteStarts = newObjects.map((obj) => obj[0]);
    this.noteTypes = newObjects.map(() => 128);
    this.noteEnds = newObjects.map((obj) => obj[2]);
    this.breaks = [];
    this.lnRatio = this.getLNRatio();
    this.noteTimes = this.getNoteTimes();
    this.objectIntervals = this.getObjectIntervals();
    }

    modHO() {
    for (let i = 0; i < this.noteTypes.length; i += 1) {
            if ((this.noteTypes[i] & 128) !== 0) {
        this.noteTypes[i] = 1;
        this.noteEnds[i] = 0;
            }
    }

    this.lnRatio = this.getLNRatio();
    this.noteTimes = this.getNoteTimes();
    this.objectIntervals = this.getObjectIntervals();
    }
}
