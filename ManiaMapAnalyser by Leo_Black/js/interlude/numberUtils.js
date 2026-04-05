export function f32(value) {
    return Math.fround(value);
}

// Match .NET/F# round behavior (banker's rounding).
export function roundToEven(value) {
    if (!Number.isFinite(value)) {
        return value;
    }

    const sign = value < 0 ? -1 : 1;
    const absValue = Math.abs(value);
    const floor = Math.floor(absValue);
    const frac = absValue - floor;

    if (frac < 0.5) {
        return sign * floor;
    }
    if (frac > 0.5) {
        return sign * (floor + 1);
    }

    return sign * (floor % 2 === 0 ? floor : floor + 1);
}
