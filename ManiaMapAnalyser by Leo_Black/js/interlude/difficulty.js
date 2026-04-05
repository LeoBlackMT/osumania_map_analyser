import { f32 } from "./numberUtils.js";
import { calculateNoteRatings } from "./noteDifficulty.js";
import { calculateVariety } from "./variety.js";
import { calculateFingerStrains, calculateHandStrains } from "./strain.js";

const CURVE_POWER = f32(0.6);
const CURVE_SCALE = f32(0.4056);
const MOST_IMPORTANT_NOTES = f32(2500.0);

function weightingCurve(x) {
    return f32(0.002 + Math.pow(x, 4.0));
}

export function weightedOverallDifficulty(data) {
    const values = Array.from(data || []).slice().sort((a, b) => a - b);
    if (values.length === 0) {
        return 0.0;
    }

    const length = f32(values.length);
    let weight = 0.0;
    let total = 0.0;

    for (let i = 0; i < values.length; i += 1) {
        const x = Math.max(0.0, (f32(i) + MOST_IMPORTANT_NOTES - length) / MOST_IMPORTANT_NOTES);
        const w = weightingCurve(x);
        weight += w;
        total += (Number(values[i]) || 0.0) * w;
    }

    if (!Number.isFinite(weight) || weight <= 0) {
        return 0.0;
    }

    const transformed = Math.pow(total / weight, CURVE_POWER) * CURVE_SCALE;
    return Number.isFinite(transformed) ? f32(transformed) : 0.0;
}

export function calculateInterludeDifficulty(rate, noteRows) {
    if (!Array.isArray(noteRows) || noteRows.length === 0) {
        return {
            noteDifficulty: [],
            strains: [],
            variety: [],
            hands: [],
            overall: 0.0,
        };
    }

    const noteDifficulty = calculateNoteRatings(rate, noteRows);
    const variety = calculateVariety(rate, noteRows, noteDifficulty);
    const strains = calculateFingerStrains(rate, noteRows, noteDifficulty);
    const hands = calculateHandStrains(rate, noteRows, noteDifficulty);

    const strainValues = [];
    for (let i = 0; i < strains.length; i += 1) {
        const row = strains[i].StrainV1Notes || [];
        for (let k = 0; k < row.length; k += 1) {
            const v = Number(row[k]) || 0.0;
            if (v > 0.0) {
                strainValues.push(v);
            }
        }
    }

    const overall = weightedOverallDifficulty(strainValues);

    return {
        noteDifficulty,
        strains,
        variety,
        hands,
        overall: Number.isFinite(overall) ? overall : 0.0,
    };
}
