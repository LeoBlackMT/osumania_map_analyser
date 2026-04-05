export const NoteType = Object.freeze({
    NOTHING: 0,
    NORMAL: 1,
    HOLDHEAD: 2,
    HOLDBODY: 3,
    HOLDTAIL: 4,
});

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
