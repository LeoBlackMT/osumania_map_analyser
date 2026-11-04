// 数据源“正在游玩”判定。
//
// 壳只保证 `playing` 是该帧的读数；游戏退出、桥停止或页面与壳断连后，
// 旧的 `playing`/`alive` 位会留在页面状态里。卡片显隐与 Graph 进度都直接
// 依赖这个判定，必须在这里统一叠加存活与过期条件，避免一场 Etterna 留下的
// `playing=1` 让之后所有源永久被当成“游玩中”。

import { state } from "../core/appContext.js";

const MALODY4_ALIVE_WINDOW_MS = 6000;

/** 壳桥存活：断连后不再信任任何壳侧来源位。 */
function shellSourcesAvailable() {
    return state.externalBridgeAvailable === true;
}

function isEtternaPlayingNow() {
    if (!shellSourcesAvailable() || !state.etternaAlive || !state.etternaPlaying) {
        return false;
    }
    // 壳按谱面时长给出 playing 过期时间；到点后即使桥不再写文件也判为结束。
    const expireAt = Number(state.etternaPlayingExpireAt);
    return !Number.isFinite(expireAt) || Date.now() < expireAt;
}

function isMalodyPlayingNow() {
    return Boolean(shellSourcesAvailable() && state.malodyAlive && state.malodyPlaying);
}

function isMalody4PlayingNow() {
    if (!shellSourcesAvailable() || !state.malody4Alive) {
        return false;
    }
    // selection 心跳是 Malody 4 唯一的新鲜度来源；心跳断了就不再相信 playing。
    const seenAt = Number(state.malody4LastSeenAt);
    if (!Number.isFinite(seenAt) || Date.now() - seenAt > MALODY4_ALIVE_WINDOW_MS) {
        return false;
    }
    return Boolean(
        state.malody4Playing
        || state.malody4Screen === "playing"
        || state.malody4Screen === "play"
    );
}

export function isSourcePlaying(source) {
    switch (source) {
        case "osu":
            return Boolean(state.isInPlayState);
        case "etterna":
            return isEtternaPlayingNow();
        case "malody":
            return isMalodyPlayingNow();
        case "malody4":
            return isMalody4PlayingNow();
        default:
            return false;
    }
}

/** 卡片归属源统一成路由名（Malody V 桥的归属名是 `malody-bridge`）。 */
export function normalizeCardOwnerSource(owner) {
    if (owner === "malody-bridge") {
        return "malody";
    }
    return owner === "osu" || owner === "etterna" || owner === "malody" || owner === "malody4"
        ? owner
        : null;
}
