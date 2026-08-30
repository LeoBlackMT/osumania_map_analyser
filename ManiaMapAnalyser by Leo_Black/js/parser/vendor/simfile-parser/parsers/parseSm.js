import { Fraction } from "../fraction.js";
import { determineBeat, mergeSimilarBpmRanges, normalizedDifficultyMap, renameBackground, reportError, } from "../util.js";
// Ref: https://github.com/stepmania/stepmania/wiki/sm
// eslint-disable-next-line jsdoc/require-jsdoc
function isMetaTag(tag) {
    return [
        "title",
        "titletranslit",
        "subtitle",
        "subtitletranslit",
        "artist",
    ].includes(tag);
}
// eslint-disable-next-line jsdoc/require-jsdoc
function isImageTag(tag) {
    return ["banner", "background", "jacket"].includes(tag);
}
/**
 * does a given line count as the end of a notes tag/block?
 * @param line contents of the next line to check
 * @returns boolean
 */
function concludesANoteTag(line) {
    if (line === undefined) {
        return true;
    }
    return line.startsWith(";") || line.startsWith(",;");
}
// eslint-disable-next-line jsdoc/require-jsdoc
function getMeasureLength(lines, i) {
    let measureLength = 0;
    for (; i < lines.length && !concludesANoteTag(lines[i]) && lines[i][0] !== ","; ++i) {
        if (lines[i].trim() !== "") {
            measureLength += 1;
        }
    }
    return measureLength;
}
// eslint-disable-next-line jsdoc/require-jsdoc
function trimNoteLine(line, mode) {
    // [vendor patch] keep the full row width: Etterna 6K/7K charts declare
    // dance-single/double but carry wider rows; truncation would drop columns.
    return line;
}
// eslint-disable-next-line jsdoc/require-jsdoc
function isRest(line) {
    return line.split("").every((d) => d === "0");
}
/**
 * finds first non-empty measure in a chart
 * @param mode gameplay mode of this chart
 * @param lines all the lines in a current file
 * @param i starting line index
 * @returns line index for first step, and how many measures were passed in getting there
 */
export function findFirstNonEmptyMeasure(mode, lines, i) {
    let numMeasuresSkipped = 0;
    let measureIndex = i;
    for (; i < lines.length && !concludesANoteTag(lines[i]); ++i) {
        const line = lines[i];
        if (line.trim() === "") {
            continue;
        }
        if (line.startsWith(",")) {
            measureIndex = i + 1;
            numMeasuresSkipped += 1;
            continue;
        }
        if (!isRest(trimNoteLine(line, mode))) {
            return {
                found: true,
                firstNonEmptyMeasureIndex: measureIndex,
                numMeasuresSkipped,
            };
        }
    }
    return { found: false, numMeasuresSkipped };
}
/**
 * parse a SM file
 * @param sm entire contents of the file
 * @returns parsed simfile object
 */
export function parseSm(sm) {
    const lines = sm.split("\n").map((l) => l.trim());
    let i = 0;
    let bpmString = null;
    let stopsString = null;
    const sc = {
        charts: {},
        availableTypes: [],
        images: {
            banner: null,
            bg: null,
            jacket: null,
        },
    };
    /**
     * parse out bpm stops
     * @param stopsString input string
     * @param emptyOffsetInMeasures number of empty measures at the head of this chart
     * @returns list of stop locations and durations
     */
    function parseStops(stopsString, emptyOffsetInMeasures) {
        if (!stopsString) {
            return [];
        }
        const entries = stopsString.split(",");
        return entries.map((s) => {
            const [stopS, durationS] = s.split("=");
            return {
                offset: Number(stopS) * 0.25 - emptyOffsetInMeasures,
                duration: Number(durationS),
            };
        });
    }
    /**
     * parse a sequence of bpm changes
     * @param bpmString input string
     * @param emptyOffsetInMeasures number of empty measures at the head of this chart
     * @returns a sequence of bpm segments
     */
    function parseBpms(bpmString, emptyOffsetInMeasures) {
        const entries = bpmString.split(",");
        const bpms = entries.map((e, i, a) => {
            const [beatS, bpmS] = e.split("=");
            const nextBeatS = a[i + 1]?.split("=")[0] ?? null;
            return {
                startOffset: Number(beatS) * 0.25 - emptyOffsetInMeasures,
                endOffset: nextBeatS === null
                    ? null
                    : Number(nextBeatS) * 0.25 - emptyOffsetInMeasures,
                bpm: Number(bpmS),
            };
        });
        return mergeSimilarBpmRanges(bpms);
    }
    /**
     * @param lines all lines in this file
     * @param i current line
     * @param mode chart's gameplay mode, for reporting only
     * @param difficulty chart's difficulty category, for reporting only
     * @returns array of parsed freeze note locations
     */
    function parseFreezes(lines, i, mode, difficulty) {
        const freezes = [];
        const open = {};
        let curOffset = new Fraction(0);
        let curMeasureFraction = new Fraction(1, getMeasureLength(lines, i) || 1);
        for (; i < lines.length && !concludesANoteTag(lines[i]); ++i) {
            const line = lines[i];
            if (line.trim() === "") {
                continue;
            }
            if (line[0] === ",") {
                curMeasureFraction = new Fraction(1, getMeasureLength(lines, i + 1) || 1);
                continue;
            }
            if (line.indexOf("2") === -1 &&
                line.indexOf("3") === -1 &&
                line.indexOf("4") === -1) {
                curOffset = curOffset.add(curMeasureFraction);
                continue;
            }
            const cleanedLine = line.replace(/[^234]/g, "0");
            for (let d = 0; d < cleanedLine.length; ++d) {
                if (cleanedLine[d] === "2" || cleanedLine[d] === "4") {
                    if (open[d]) {
                        reportError(`${sc.title}, ${mode}, ${difficulty} -- error parsing freezes, found a new starting freeze before a previous one finished`);
                    }
                    else {
                        const startBeatFraction = curOffset;
                        open[d] = {
                            direction: d,
                            startOffset: startBeatFraction.n / startBeatFraction.d,
                        };
                    }
                }
                else if (cleanedLine[d] === "3") {
                    const thisFreeze = open[d];
                    if (!thisFreeze) {
                        reportError(`${sc.title}, ${mode}, ${difficulty} -- error parsing freezes, tried to close a freeze that never opened`);
                    }
                    else {
                        const endBeatFraction = curOffset.add(new Fraction(1, 4));
                        thisFreeze.endOffset = endBeatFraction.n / endBeatFraction.d;
                        freezes.push(thisFreeze);
                        open[d] = undefined;
                    }
                }
            }
            curOffset = curOffset.add(curMeasureFraction);
        }
        return freezes;
    }
    /**
     * Parse notes info for a chart and adds it to current chart's record of charts
     * @param lines all lines of current file
     * @param i current line
     * @param bpmString string of bpm changes for this chart
     * @returns number of lines parsed
     */
    function parseNotes(lines, i, bpmString) {
        // move past #NOTES into the note metadata
        i++;
        const mode = lines[i++].replace("dance-", "").replace(":", "");
        i++; // skip author for now
        const difficulty = normalizedDifficultyMap[lines[i++].replace(":", "").toLowerCase()];
        const feet = Number(lines[i++].replace(":", ""));
        i++; // skip groove meter data for now
        // skip couple, versus, etc for now
        if (mode !== "single" && mode !== "double") {
            // [vendor patch] keep every chart that carries rows: Etterna
            // keyboard charts may declare nonstandard stepstype strings.
            return i + 1;
        }
        // now i is pointing at the first measure
        const arrows = [];
        const firstMeasureSearchResult = findFirstNonEmptyMeasure(mode, lines, i);
        if (!firstMeasureSearchResult.found) {
            return i + firstMeasureSearchResult.numMeasuresSkipped;
        }
        const { firstNonEmptyMeasureIndex, numMeasuresSkipped } = firstMeasureSearchResult;
        i = firstNonEmptyMeasureIndex;
        const firstMeasureIndex = i;
        let curOffset = new Fraction(0);
        // in case the measure is size zero, fall back to dividing by one
        // this is just being defensive, this would mean the stepfile has no notes in it
        let curMeasureFraction = new Fraction(1, getMeasureLength(lines, i) || 1);
        for (; i < lines.length && !concludesANoteTag(lines[i]); ++i) {
            // for now, remove freeze ends as they are handled in parseFreezes
            // TODO: deal with freezes here, no need to have two functions doing basically the same thing
            const line = trimNoteLine(lines[i], mode).replace(/3/g, "0");
            if (line.trim() === "") {
                continue;
            }
            if (line.startsWith(",")) {
                curMeasureFraction = new Fraction(1, getMeasureLength(lines, i + 1) || 1);
                continue;
            }
            if (!isRest(line)) {
                arrows.push({
                    quantization: determineBeat(curOffset),
                    offset: curOffset.n / curOffset.d,
                    direction: line,
                });
            }
            curOffset = curOffset.add(curMeasureFraction);
        }
        const freezes = parseFreezes(lines, firstMeasureIndex, mode, difficulty);
        sc.charts[`${mode}-${difficulty}`] = {
            arrows,
            freezes,
            bpm: parseBpms(bpmString, numMeasuresSkipped),
            stops: parseStops(stopsString, numMeasuresSkipped),
        };
        sc.availableTypes.push({
            slug: `${mode}-${difficulty}`,
            mode,
            difficulty: difficulty,
            feet,
        });
        return i + 1;
    }
    /**
     * @param lines all lines of the file at hand
     * @param index current line number
     * @returns number of lines consumed
     */
    function parseTag(lines, index) {
        const line = lines[index];
        const r = /#([A-Za-z]+):([^;]*)/;
        const result = r.exec(line);
        if (result) {
            const tag = result[1].toLowerCase();
            const value = result[2];
            if (isMetaTag(tag)) {
                sc[tag] = value;
            }
            else if (isImageTag(tag)) {
                // @ts-expect-error: background is not an output value, will be cleaned up later
                sc.images[tag] = value;
            }
            else if (tag === "displaybpm") {
                sc.displayBpm = value.replace(":", "-");
            }
            else if (tag === "bpms") {
                bpmString = value;
            }
            else if (tag === "stops") {
                stopsString = value;
            }
            else if (tag === "notes") {
                if (!bpmString) {
                    throw new Error("parseSm: about to parse notes but never got bpm");
                }
                return parseNotes(lines, index, bpmString);
            }
        }
        return index + 1;
    }
    try {
        while (i < lines.length) {
            const line = lines[i];
            if (!line.length || line.startsWith("//")) {
                i += 1;
                continue;
            }
            if (line.startsWith("#")) {
                i = parseTag(lines, i);
            }
            else {
                i += 1;
            }
        }
        renameBackground(sc.images);
        return sc;
    }
    catch (e) {
        throw new Error(`error parsing ${sm.substring(0, 300)}`, { cause: e });
    }
}
//# sourceMappingURL=parseSm.js.map