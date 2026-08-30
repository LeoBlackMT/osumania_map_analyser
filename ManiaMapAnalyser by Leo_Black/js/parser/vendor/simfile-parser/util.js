import { Fraction } from "./fraction.js";
const beats = [
    new Fraction(1, 4),
    new Fraction(1, 6),
    new Fraction(1, 8),
    new Fraction(1, 12),
    new Fraction(1, 16),
    new Fraction(1, 32),
    new Fraction(1, 64),
];
/**
 * Picks a quantization color for a given note
 * @param offset fractional
 * @returns number indicating the quantization color
 */
export function determineBeat(offset) {
    const match = beats.find((b) => offset.mod(b).n === 0);
    if (!match) {
        // didn't find anything? then it's some kind of weirdo like a 5th note,
        // they get colored the same as 64ths
        return 64;
    }
    return match.d;
}
export const normalizedDifficultyMap = {
    beginner: "beginner",
    easy: "basic",
    basic: "basic",
    trick: "difficult",
    another: "difficult",
    medium: "difficult",
    difficult: "expert",
    expert: "expert",
    maniac: "expert",
    ssr: "expert",
    hard: "expert",
    challenge: "challenge",
    smaniac: "challenge",
    // TODO: filter edits out altogether
    edit: "edit",
};
/**
 * @param a first bpm
 * @param b second bpm
 * @returns true if difference between a and b is less than 1
 */
function similarBpm(a, b) {
    return Math.abs(a.bpm - b.bpm) < 1;
}
/**
 * Iterates across a list of bpm regions and collapses adjacent
 * ones with very similar bpm values into a single region
 * @param bpm array of bpms
 * @returns simplified version of input
 */
export function mergeSimilarBpmRanges(bpm) {
    return bpm.reduce((building, current, i, a) => {
        const prev = a[i - 1];
        const next = a[i + 1];
        if (prev && similarBpm(prev, current)) {
            // this bpm was merged on the last iteration, so skip it
            return building;
        }
        if (next && similarBpm(next, current)) {
            return building.concat({
                ...current,
                endOffset: next.endOffset,
            });
        }
        return building.concat(current);
    }, []);
}
/**
 * if we found a `background` tag, rename it to `bg`
 * @param images image data
 */
export function renameBackground(images) {
    if (typeof images.background !== "undefined") {
        if (images.background) {
            images.bg = images.background;
        }
        delete images.background;
    }
}
let errorTolerance = "warn";
/**
 * set the global error tolerance level for the parser
 * @param level a level to use
 */
export function setErrorTolerance(level) {
    errorTolerance = level;
}
/**
 * Depending on error tolerance level this may ignore, print, or bail with a given message
 * @param msg a message to maybe print or throw
 * @param cause an error with stack trace, if available
 */
export function reportError(msg, cause) {
    switch (errorTolerance) {
        case "ignore":
            break;
        case "warn":
            console.warn(msg, cause);
            break;
        case "bail":
        default:
            throw new Error(msg, { cause });
    }
}
//# sourceMappingURL=util.js.map