import { NoteType } from "../patterns/chart.js";

// Re-exported single source (js/patterns/chart.js) — see module conventions.
export { NoteType } from "../patterns/chart.js";

export function createEmptyRow(keyCount) {
    return new Array(keyCount).fill(NoteType.NOTHING);
}

export function isPlayableNoteType(noteType) {
    return noteType === NoteType.NORMAL || noteType === NoteType.HOLDHEAD;
}

export function isRowEmpty(row) {
    for (let i = 0; i < row.length; i += 1) {
        const noteType = row[i];
        if (noteType !== NoteType.NOTHING && noteType !== NoteType.HOLDBODY) {
            return false;
        }
    }
    return true;
}
