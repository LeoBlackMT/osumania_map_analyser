import { keysOnLeftHand } from "./layout.js";
import { isPlayableNoteType } from "./types.js";
import { f32 } from "./numberUtils.js";

const JACK_CURVE_CUTOFF = f32(230.0);
const STREAM_CURVE_CUTOFF = f32(10.0);
const STREAM_CURVE_CUTOFF_2 = f32(10.0);
const OHTNERF = f32(3.0);
const STREAM_SCALE = f32(6.0);
const STREAM_POW = f32(0.5);

function msToJackBpm(delta) {
    const value = f32(15000.0 / delta);
    return value < JACK_CURVE_CUTOFF ? value : JACK_CURVE_CUTOFF;
}

function msToStreamBpm(delta) {
    const x = f32(0.02 * delta);
    if (!Number.isFinite(x) || x <= 0) {
        return 0.0;
    }

    const value = f32(
        (300.0 / x)
        - (300.0 / Math.pow(x, STREAM_CURVE_CUTOFF) / STREAM_CURVE_CUTOFF_2),
    );
    return value > 0 ? value : 0.0;
}

function jackCompensation(jackDelta, streamDelta) {
    const ratio = jackDelta / streamDelta;
    if (!Number.isFinite(ratio) || ratio <= 0) {
        return 0.0;
    }

    const compensated = Math.sqrt(Math.max(0.0, Math.log2(ratio)));
    return Math.min(1.0, compensated);
}

export function noteDifficultyTotal(note) {
    return f32(
        Math.pow(
            Math.pow(STREAM_SCALE * Math.pow(note.SL, STREAM_POW), OHTNERF)
            + Math.pow(STREAM_SCALE * Math.pow(note.SR, STREAM_POW), OHTNERF)
            + Math.pow(note.J, OHTNERF),
            1.0 / OHTNERF,
        ),
    );
}

export function calculateNoteRatings(rate, noteRows) {
    if (!Array.isArray(noteRows) || noteRows.length === 0) {
        return [];
    }

    const rateValue = Number.isFinite(rate) && rate > 0 ? rate : 1.0;
    const keys = noteRows[0].data.length;
    const handSplit = keysOnLeftHand(keys);

    const data = Array.from({ length: noteRows.length }, () => (
        Array.from({ length: keys }, () => ({
            J: 0.0,
            SL: 0.0,
            SR: 0.0,
            Total: 0.0,
        }))
    ));

    const firstTime = Number(noteRows[0].time) || 0;
    const lastNoteInColumn = new Array(keys).fill(firstTime - 1000000.0);

    for (let i = 0; i < noteRows.length; i += 1) {
        const row = noteRows[i];
        const time = Number(row.time);

        for (let k = 0; k < keys; k += 1) {
            const noteType = row.data[k];
            if (!isPlayableNoteType(noteType)) {
                continue;
            }

            const jackDelta = (time - lastNoteInColumn[k]) / rateValue;
            const item = data[i][k];
            item.J = msToJackBpm(jackDelta);

            const handLo = k < handSplit ? 0 : handSplit;
            const handHi = k < handSplit ? (handSplit - 1) : (keys - 1);

            let sl = 0.0;
            let sr = 0.0;

            for (let handK = handLo; handK <= handHi; handK += 1) {
                if (handK === k) {
                    continue;
                }

                const trillDelta = (time - lastNoteInColumn[handK]) / rateValue;
                const trillValue = msToStreamBpm(trillDelta) * jackCompensation(jackDelta, trillDelta);
                if (handK < k) {
                    sl = Math.max(sl, trillValue);
                } else {
                    sr = Math.max(sr, trillValue);
                }
            }

            item.SL = f32(sl);
            item.SR = f32(sr);
            item.Total = noteDifficultyTotal(item);
        }

        for (let k = 0; k < keys; k += 1) {
            const noteType = row.data[k];
            if (isPlayableNoteType(noteType)) {
                lastNoteInColumn[k] = time;
            }
        }
    }

    return data;
}
