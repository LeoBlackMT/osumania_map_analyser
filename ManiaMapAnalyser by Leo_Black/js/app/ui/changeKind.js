// 换图动画的 changeKind 判定（song/difficulty/mod）与 songKey 构造。
// 纯函数、无 DOM 依赖，可在 Node 中直接测试（见 temp/acceptance-8-changekind-smoke.mjs）。

// songKey 单一来源优先：set > dir > meta。
// tosu api_v2 不同状态（selectSong/play/resultScreen）提供的字段集合不同——
// set 存在时 key 恒为 `set:xxx`，dir 字段缺失/波动不影响，同 mapset 内切难度 key 稳定。
export function buildSongKey({ beatmapSetId, beatmapFolderPath, songMetaKey }) {
    const songKeyParts = [];
    if (beatmapSetId) songKeyParts.push(`set:${beatmapSetId}`);
    if (songKeyParts.length === 0 && beatmapFolderPath) songKeyParts.push(`dir:${beatmapFolderPath}`);
    if (songKeyParts.length === 0 && songMetaKey.replace(/[:]/g, "").length > 0) {
        songKeyParts.push(`meta:${songMetaKey}`);
    }
    return songKeyParts.join("|");
}

// 判定本次变化的类型，供渲染层选择对应的入场动画：
//   song       —— 换歌（mapset 变了，或首次加载）
//   difficulty —— 换难度（同一 mapset 内切换谱面）
//   mod        —— 仅 mod 改变，谱面与难度都没变
// nextSongKey 为空（partial 包，无 mapset 信息）时不判换歌——
// 首次加载由 analysis 的 reason fallback（"initial load" → song）兜底。
export function resolveChangeKind({ previousBeatmapIdentity, nextBeatmapIdentity, previousSongKey, nextSongKey }) {
    if (nextBeatmapIdentity === previousBeatmapIdentity) {
        return "mod";
    }
    const songChanged = nextSongKey === ""
        ? false
        : (!previousSongKey || nextSongKey !== previousSongKey);
    return songChanged ? "song" : "difficulty";
}
