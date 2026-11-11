// timeAxis.js —— 显示轴与物理轴之间的换算（纯函数，无 DOM）。
//
// 仓库内两套时间轴（见 docs/features/graph-section-analysis.md §3）：
// - 显示轴 = 物理 / speedRate：state.songTimeMs、graphSeries.times、选区 UI；
// - 物理轴 = .osu 毫秒：谱面解析、HitEvent.TimeOffset、ReplayFrame.Time。
// LaneAcc 的事件与原始帧都是物理轴，喂判定时必须把显示轴换算回物理轴。

/**
 * 显示轴 → 物理轴。
 * @param {number} displayMs
 * @param {number} speedRate
 * @returns {number} 物理毫秒；`displayMs` 非有限时返回 NaN
 */
export function toChartTime(displayMs, speedRate) {
    const value = Number(displayMs);
    if (!Number.isFinite(value)) {
        return NaN;
    }
    const rate = Number(speedRate);
    return value * (Number.isFinite(rate) && rate > 0 ? rate : 1);
}
