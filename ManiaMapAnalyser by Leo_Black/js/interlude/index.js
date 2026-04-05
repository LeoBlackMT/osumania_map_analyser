import { buildInterludeRows } from "./chartBuilder.js";
import { calculateInterludeDifficulty } from "./difficulty.js";

function normalizeRate(rate) {
    const value = Number(rate);
    if (!Number.isFinite(value) || value <= 0) {
        return 1.0;
    }
    return value;
}

// Single public entry point for Interlude SR calculation.
// `source` accepts osu text, fetchable beatmap path/url, or OsuFileParser (with osuText).
export async function calculateInterludeStar(source, rate = 1.0, cvtFlag = null) {
    const resolvedRate = normalizeRate(rate);
    const { rows } = await buildInterludeRows(source, cvtFlag);
    const difficulty = calculateInterludeDifficulty(resolvedRate, rows);
    const overall = Number(difficulty?.overall);
    return Number.isFinite(overall) ? overall : 0.0;
}

export default calculateInterludeStar;
