// 壳 state 帧 → 页面状态（M3/M5 最小集）。
//
// tosuOnline：壳模式在线数据面信号（M5 起驱动 osu 源启停与败方门控）。
// sources 存活位：供 Auto 决策（M5 sourceManager）与后续 UI（圆点源状态）。
//
// malody4 的字段各有唯一写入者（两个语义不得打架）：
//   - state 帧（applyShellState）→ malody4Playing / malody4Screen / malody4Reason / malody4Judge；
//   - selection 帧（applyMalody4Selection）→ malody4LastSeq / malody4LastSeenAt / malody4Alive。
// `state.malody4Alive` 由 selection 帧的新鲜度派生，**绝不被 30s 周期的 state 帧覆盖**。

import { state } from "../appContext.js";
import { notifySourceEvent, reEvaluate } from "./sourceManager.js";

/** 心跳 2s + 容忍丢一拍 → 6s 未见 selection 帧即视为本源离场。 */
const MALODY4_ALIVE_WINDOW_MS = 6000;

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
    const malody4 = sources.malody4 || {};
    state.malody4Playing = Boolean(malody4.playing);
    state.malody4Screen = malody4.screen || null;
    state.malody4Reason = malody4.reason || null;
    state.malody4Judge = malody4.judge || null;
    // 注意：这里绝不写 state.malody4Alive —— 它是 selection 帧新鲜度的派生值，
    // 30s 周期帧写它会把心跳之间的在线状态冲成假离线。
    reEvaluate();
}

/**
 * 应用壳 `malody4_selection` 帧（Malody 4 选曲记录）。
 *
 * `state.malody4Alive` 是**诊断位 + 圆点/日志**用的值：`decide()` 不读它，路由只由 L1
 * `malody4Playing` 与 L2 事件窗口决定（避免下一位维护者以为它是死字段）。
 * @param {object} payload selection 帧 payload
 */
export function applyMalody4Selection(payload) {
    state.malody4LastSeq = payload.sequence != null ? payload.sequence : null;
    state.malody4LastSeenAt = Date.now();
    state.malody4Alive = Date.now() - state.malody4LastSeenAt <= MALODY4_ALIVE_WINDOW_MS;
    // L2 续约：只有"真的选中了谱面"的帧才算新鲜事件。hidden 帧（path 为空）与心跳必须
    // 排除，否则壳只要在跑（hidden 心跳永不停止）就会把路由永久钉在 malody4，饿死 osu/Etterna。
    if (payload.path && payload.event !== "heartbeat") {
        notifySourceEvent("malody4");
    }
}
