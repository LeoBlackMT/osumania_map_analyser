export const NoteType = {
    NOTHING: 0,
    NORMAL: 1,
    HOLDHEAD: 2,
    HOLDBODY: 3,
    HOLDTAIL: 4,
};

export function createTimeItem(time, data) {
    return { Time: Number(time), Data: data };
}

export function createBPM(meter, msPerBeat) {
    return { Meter: meter, MsPerBeat: msPerBeat };
}

export function createChart(keys, notes, bpm, sv) {
    return {
    Keys: keys,
    Notes: notes,
    BPM: bpm,
    SV: sv,
    get FirstNote() {
            return this.Notes[0].Time;
    },
    get LastNote() {
            return this.Notes[this.Notes.length - 1].Time;
    },
    };
}
