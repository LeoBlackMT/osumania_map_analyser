// 壳 state 帧 → 页面状态（M3/M5 最小集）。
//
// tosuOnline：壳模式在线数据面信号（M5 起驱动 osu 源启停与败方门控）。
// sources 存活位：供 Auto 决策（M5 sourceManager）与后续 UI（圆点源状态）。

import { state } from "../appContext.js";
import { reEvaluate } from "./sourceManager.js";

/**
 * 应用壳 state 帧（shell → 页）。
 * @param {object} payload state 帧 payload
 */
export function applyShellState(payload) {
    state.shellTosuOnline = Boolean(payload.tosuOnline);
    state.shellErrors = Array.isArray(payload.errors) ? payload.errors : [];
    const sources = payload.sources || {};
    state.etternaAlive = Boolean(sources.etterna && sources.etterna.alive);
    state.etternaPlaying = Boolean(sources.etterna && sources.etterna.playing);
    state.etternaPlayingExpireAt = sources.etterna ? sources.etterna.playingExpireAt : null;
    state.malodyAlive = Boolean(sources.malody && sources.malody.alive);
    reEvaluate();
}