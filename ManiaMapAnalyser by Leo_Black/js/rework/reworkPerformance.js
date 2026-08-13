// Rework mania PP calculation — ported verbatim from the C# osu-author-port
// rework branch (osu.Game.Rulesets.Mania/Difficulty/ManiaPerformanceCalculator.cs)
// via the genirx dart port (lib/logic/algorithm/rework/rework_performance.dart).
//
// Shared DOM-free module: no imports, no window/document, no state. Pure number
// formulas only — judgement counts come from the caller (tosu play.hits:
// geki→perfect(305), 300→great, katu→good(200), 100→ok, 50→meh, 0→miss).
// Do NOT "improve" any formula here — the C# output is authoritative.

/// Custom accuracy with 305-weighting (dart `_calculateCustomAccuracy`,
/// C# `calculateCustomAccuracy`):
/// `(perfect*305 + great*300 + good*200 + ok*100 + meh*50) / (totalHits*305)`,
/// totalHits includes miss. 0 when totalHits === 0; clamped to [0, 1].
function calculateCustomAccuracy({ perfect, great, good, ok, meh, miss }) {
    const totalHits = perfect + great + good + ok + meh + miss;
    if (totalHits === 0) return 0;
    const acc = (perfect * 305 + great * 300 + good * 200 + ok * 100 + meh * 50) /
        (totalHits * 305);
    return Math.min(1, Math.max(0, acc));
}

/// Performance proportion (dart `_calculatePerformanceProportion`,
/// C# `calculatePerformanceProportion`):
/// `acc > 0.8 ? 4.5*(acc - 0.8) / Math.pow(100*(1 - acc) + Math.pow(0.9, 20), 0.05) : 0`
function calculatePerformanceProportion(acc) {
    if (acc > 0.8) {
        return 4.5 * (acc - 0.8) /
            Math.pow(100 * (1 - acc) + Math.pow(0.9, 20), 0.05);
    }
    return 0;
}

/// Variety multiplier (dart `_varietyMultiplier`, C# `varietyMultiplier`):
/// `floor + (cap - floor) / (1 + Math.exp(-3 * (variety - 3.25)))`,
/// `floor = 0.945`, `cap = 1.055` (C# computes `L = cap - floor`, so the
/// multiplier width is `1.055 - 0.945`, NOT the literal `0.11`).
function varietyMultiplier(variety) {
    return 0.945 + (1.055 - 0.945) / (1 + Math.exp(-3 * (variety - 3.25)));
}

/// Accuracy multiplier (dart `_accMultiplier`, C# `accMultiplier`):
/// `sigmoidScaler * (2 * Math.pow(acc, 20) - 1) + 2 - 2 * Math.pow(acc, 20)`,
/// `sigmoidScaler = 0.87 + 0.26 / (1 + Math.exp(-20 * (accScalar - 1)))`.
function accMultiplier(acc, accScalar) {
    const sigmoidScaler = 0.87 + 0.26 / (1 + Math.exp(-20 * (accScalar - 1)));
    return sigmoidScaler * (2 * Math.pow(acc, 20) - 1) +
        2 - 2 * Math.pow(acc, 20);
}

/// Length multiplier (dart `_lengthMultiplier`, C# `lengthMultiplier`):
/// `1.1 / (1 + Math.sqrt(starRating / (2 * totalNotes)))`.
/// Returns null for invalid inputs (totalNotes <= 0 or non-finite starRating),
/// mirroring the dart invalid-input guard.
function lengthMultiplier(totalNotes, starRating) {
    if (totalNotes <= 0 || !Number.isFinite(starRating)) return null;
    return 1.1 / (1 + Math.sqrt(starRating / (2 * totalNotes)));
}

/// Input validity gate (dart `_inputsValid`): starRating <= 0 / totalNotes <= 0 /
/// any count < 0 / non-finite starRating·variety·accScalar → invalid.
function inputsValid({ starRating, variety, accScalar, totalNotes, perfect, great, good, ok, meh, miss }) {
    if (starRating <= 0 || totalNotes <= 0) return false;
    if (perfect < 0 || great < 0 || good < 0 || ok < 0 || meh < 0 || miss < 0) return false;
    if (!Number.isFinite(starRating) || !Number.isFinite(variety) || !Number.isFinite(accScalar)) return false;
    return true;
}

/// Rework mania PP (dart `calculateReworkPp`, C# `CreatePerformanceAttributes`):
/// `total = difficultyValue * modMultiplier * varietyMultiplier * accMultiplier * lengthMultiplier`,
/// `difficultyValue = 9.8 * Math.pow(Math.max(starRating - 0.15, 0.05), 2.2) * proportion`,
/// `modMultiplier = (noFail ? 0.75 : 1) * (easy ? 0.90 : 1)`.
/// Returns null on invalid input; otherwise `{pp, v2Acc, proportion, accMultiplier,
/// varietyMultiplier, lengthMultiplier}`.
function calculateReworkPp({ starRating, variety, accScalar, totalNotes, perfect, great, good, ok, meh, miss, noFail = false, easy = false }) {
    if (!inputsValid({ starRating, variety, accScalar, totalNotes, perfect, great, good, ok, meh, miss })) {
        return null;
    }

    const v2Acc = calculateCustomAccuracy({ perfect, great, good, ok, meh, miss });
    const proportion = calculatePerformanceProportion(v2Acc);

    const difficultyValue = 9.8 * Math.pow(Math.max(starRating - 0.15, 0.05), 2.2) * proportion;

    const modMultiplier = (noFail ? 0.75 : 1) * (easy ? 0.90 : 1);

    const varietyMult = varietyMultiplier(variety);
    const accMult = accMultiplier(v2Acc, accScalar);
    const lengthMult = lengthMultiplier(totalNotes, starRating);

    const pp = difficultyValue * modMultiplier * varietyMult * accMult * lengthMult;

    return {
        pp,
        v2Acc,
        proportion,
        accMultiplier: accMult,
        varietyMultiplier: varietyMult,
        lengthMultiplier: lengthMult,
    };
}

export {
    calculateCustomAccuracy,
    calculatePerformanceProportion,
    varietyMultiplier,
    accMultiplier,
    lengthMultiplier,
    calculateReworkPp,
};
