// replayPauses.js —— 回放暂停的纯函数（无 DOM，可在 Node 下直接单测）。
//
// 数据源：24061 `replay_pauses` 帧的 payload。
// `pauses` 是**物理谱面时间**（`.osu` 毫秒轴）；页面显示轴 = 原始时间 / speedRate
// （与 `state.songStartMs` / `graphSeries.times` 同域，见 graph-section-analysis.md §3）。

/**
 * 归一化一帧 payload：任何字段坏了都不抛异常，返回安全默认值。
 * @param {{hash?: unknown, pauses?: unknown, available?: unknown, speedRate?: unknown}} payload
 */
export function normalizeReplayPauses(payload) {
    const hash = typeof payload?.hash === "string" ? payload.hash : "";
    const raw = Array.isArray(payload?.pauses) ? payload.pauses : [];
    const pauses = [];
    for (const value of raw) {
        if (Number.isFinite(value)) {
            pauses.push(value);
        }
    }
    return {
        hash,
        pauses,
        available: payload?.available === true,
        speedRate: Number.isFinite(payload?.speedRate) ? payload.speedRate : null,
    };
}

/**
 * 物理时间 → 显示轴，并按可游玩区间过滤 `[startMs, endMs - marginMs)`。
 * 边界缺失时只按已知的一侧过滤（负数/超界值仍由另一侧兜住）。
 * @param {number[]} raw
 * @param {number|null} rate
 * @param {number|null} startMs
 * @param {number|null} endMs
 * @param {number} marginMs
 * @returns {number[]}
 */
export function filterReplayPauseTimes(raw, rate, startMs, endMs, marginMs) {
    const divisor = Number.isFinite(rate) && rate > 0 ? rate : 1;
    const lower = Number.isFinite(startMs) ? startMs : -Infinity;
    const upper = Number.isFinite(endMs) ? endMs - marginMs : Infinity;
    const out = [];
    for (const value of raw) {
        if (!Number.isFinite(value)) {
            continue;
        }
        const time = value / divisor;
        if (time >= lower && time < upper) {
            out.push(time);
        }
    }
    return out;
}
