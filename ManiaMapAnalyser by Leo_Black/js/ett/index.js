import {
    analyzeEtternaFromText as analyzeEtternaWasm,
    DEFAULT_SCORE_GOAL,
    DISPLAY_SKILLSET_ORDER,
} from "./calc.js";
import { SUPPORTED_KEYS } from "./constants.js";

function normalizeKeyOverride(value) {
    const parsed = Number(value);
    if (!Number.isFinite(parsed)) {
        return null;
    }
    return SUPPORTED_KEYS.has(parsed) ? parsed : null;
}

function sanitizeSkillValues(values) {
    const input = values && typeof values === "object" ? values : {};
    const normalized = {};
    for (const name of DISPLAY_SKILLSET_ORDER) {
        const value = Number(input[name]);
        normalized[name] = Number.isFinite(value) ? value : 0;
    }
    return normalized;
}

async function requestWasmCalc(osuText, options) {
    const wasmResult = await analyzeEtternaWasm(osuText, options);
    return {
        ...wasmResult,
        values: sanitizeSkillValues(wasmResult?.values),
        engine: "wasm",
    };
}

export async function analyzeEtternaFromText(osuText, {
    musicRate = 1.0,
    scoreGoal = DEFAULT_SCORE_GOAL,
    keyOverride = null,
    cvtFlag = null,
    etternaVersion = null,
} = {}) {
    const normalizedOptions = {
        musicRate: Number.isFinite(Number(musicRate)) ? Number(musicRate) : 1.0,
        scoreGoal: Number.isFinite(Number(scoreGoal)) ? Number(scoreGoal) : DEFAULT_SCORE_GOAL,
        keyOverride: normalizeKeyOverride(keyOverride),
        cvtFlag,
        etternaVersion,
    };

    return requestWasmCalc(osuText, normalizedOptions);
}

export {
    DEFAULT_SCORE_GOAL,
    DISPLAY_SKILLSET_ORDER,
};
