function pickNumber(obj, keys) {
    if (!obj || typeof obj !== "object") {
        return null;
    }

    for (const key of keys) {
        const value = Number(obj[key]);
        if (Number.isFinite(value)) {
            return value;
        }
    }

    return null;
}

export function detectVibro(values, threshold) {
    const overall = pickNumber(values, ["Overall", "overall"]);
    const jackSpeed = pickNumber(values, ["JackSpeed", "Jackspeed", "jackSpeed", "jackspeed"]);

    if (!Number.isFinite(overall) || overall <= 0 || !Number.isFinite(jackSpeed)) {
        return false;
    }

    return (jackSpeed / overall) >= threshold;
}
