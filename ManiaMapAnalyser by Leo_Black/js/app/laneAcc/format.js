// format.js —— LaneAcc 的纯格式化（DOM-free）。

export function formatAcc(value) {
    const number = Number(value);
    if (!Number.isFinite(number)) {
        return "--";
    }
    return `${number.toFixed(2)}%`;
}

export function formatMiss(value) {
    const number = Number(value);
    return Number.isFinite(number) ? String(Math.max(0, Math.trunc(number))) : "0";
}
