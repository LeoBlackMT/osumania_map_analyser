import { isPlayableNoteType } from "./types.js";
import { roundToEven } from "./numberUtils.js";

const VARIETY_WINDOW = 750.0;

export function calculateVariety(rate, noteRows, noteDifficulties) {
    if (!Array.isArray(noteRows) || noteRows.length === 0) {
        return [];
    }

    const rateValue = Number.isFinite(rate) && rate > 0 ? rate : 1.0;
    const keys = noteRows[0].data.length;

    const buckets = new Map();
    let front = 0;
    let back = 0;

    const output = [];

    for (let i = 0; i < noteRows.length; i += 1) {
        const now = noteRows[i].time;

        while (front < noteRows.length && noteRows[front].time < now + VARIETY_WINDOW * rateValue) {
            const frontRow = noteRows[front].data;
            for (let k = 0; k < keys; k += 1) {
                if (!isPlayableNoteType(frontRow[k])) {
                    continue;
                }

                const strainBucket = roundToEven((Number(noteDifficulties[front][k].Total) || 0) / 5.0);
                buckets.set(strainBucket, (buckets.get(strainBucket) || 0) + 1);
            }
            front += 1;
        }

        while (back < i && noteRows[back].time < now - VARIETY_WINDOW * rateValue) {
            const backRow = noteRows[back].data;
            for (let k = 0; k < keys; k += 1) {
                if (!isPlayableNoteType(backRow[k])) {
                    continue;
                }

                const strainBucket = roundToEven((Number(noteDifficulties[back][k].Total) || 0) / 5.0);
                const next = (buckets.get(strainBucket) || 0) - 1;
                if (next <= 0) {
                    buckets.delete(strainBucket);
                } else {
                    buckets.set(strainBucket, next);
                }
            }
            back += 1;
        }

        output.push(buckets.size);
    }

    return output;
}
