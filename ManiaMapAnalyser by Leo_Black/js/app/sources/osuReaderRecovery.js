// 读取器恢复后的补算规则（issue #132）。
//
// 壳的原生读取器在 `scanning` / `attaching` / `unavailable` 期间，24062 的
// `/files/beatmap/file` 会按设计返回 404。页面在同一张图的后续帧上会因
// `socketHandlers.applyBeatmapState` 的 `!hasStateMismatch` 早退，不会重新分析；
// 因此读取器相位回到 `healthy` 时必须主动补一次 recompute，否则失败的那张图
// 会一直停在 `Load failed`，直到用户再切一张图。
//
// 纯函数、零依赖：可在 Node 里直接 import 做表驱动测试（与 `osuScanHintRules.js` 同一先例）。

/**
 * 是否需要在读取器恢复时补算当前谱面。
 *
 * @param {string|null} previousPhase 上一帧 `sources.osu.phase`
 * @param {string|null} nextPhase 本帧 `sources.osu.phase`
 * @param {string|null} route `currentRoute()` 的结果
 * @returns {boolean}
 */
export function shouldRecomputeAfterOsuReaderRecovery(previousPhase, nextPhase, route) {
    return previousPhase !== null
        && previousPhase !== "healthy"
        && nextPhase === "healthy"
        && route === "osu";
}
