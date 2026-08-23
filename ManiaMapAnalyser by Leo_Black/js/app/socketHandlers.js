import {
    hasAnyGraphModeEnabled,
    MOD_BIT_FLAG_ENTRIES,
    NOTE_END_MARGIN_MS,
    PAUSE_DETECT_EPSILON_MS,
    SONG_TIME_JUMP_THRESHOLD_MS,
    SORTED_KNOWN_MOD_CODES,
    socket,
    state,
} from "./appContext.js";
import {
    extractCurrentSongTimeMs as extractCurrentSongTimeMsFromPayload,
    getModData as getModDataFromPayload,
} from "./modData.js";
import {
    isPlayStateName,
    isResultScreenStateName,
    normalizeClientStateName,
} from "./modeLogic.js";
import {
    addPauseMarker,
    resetPauseRuntime,
    updateGraphCursor,
} from "./graph.js";
import { updateCardPlayVisibility } from "./hud.js";
import { scheduleRecompute } from "./scheduler.js";
import { getCounterPathForCommand } from "./settings.js";
import { applyCoverThemeForBeatmap } from "./coverTheme.js";
import { updateLivePp } from "./livePp.js";
import { buildSongKey, resolveChangeKind } from "./changeKind.js";
import { noteTelemetryActivity } from "./telemetry.js";


function getModData(data) {
    return getModDataFromPayload(data, {
        sortedKnownModCodes: SORTED_KNOWN_MOD_CODES,
        modBitFlagEntries: MOD_BIT_FLAG_ENTRIES,
        fallbackClient: state.client,
        preferPlayMods: state.isInPlayState,
    });
}

function extractCurrentSongTimeMs(data) {
    return extractCurrentSongTimeMsFromPayload(data);
}

function updateSongTimeState(data) {
    const beatmapTime = data?.beatmap?.time;
    const liveTimeMs = extractCurrentSongTimeMs(data);
    if (!Number.isFinite(liveTimeMs)) {
        return;
    }

    const speedRate = Number.isFinite(state.speedRate) && state.speedRate > 0 ? state.speedRate : 1;
    const scaledLiveTimeMs = liveTimeMs / speedRate;

    const firstObjectMs = Number(beatmapTime?.firstObject);
    const lastObjectMs = Number(beatmapTime?.lastObject);
    state.songStartMs = Number.isFinite(firstObjectMs) ? firstObjectMs / speedRate : null;
    state.songEndMs = Number.isFinite(lastObjectMs) ? lastObjectMs / speedRate : null;

    if (state.pauseDetectionEnabled && state.isInPlayState && state.pauseMarkerTimes.length > 0) {
        let earliestPauseTimeMs = Number.POSITIVE_INFINITY;
        for (const markerTime of state.pauseMarkerTimes) {
            if (Number.isFinite(markerTime) && markerTime < earliestPauseTimeMs) {
                earliestPauseTimeMs = markerTime;
            }
        }
        if (Number.isFinite(earliestPauseTimeMs) && (scaledLiveTimeMs + PAUSE_DETECT_EPSILON_MS) < earliestPauseTimeMs) {
            resetPauseRuntime(true);
        }
    }

    const now = performance.now();
    const previousTime = state.songTimeMs;

    if (!state.hasSongTimeSample) {
        state.hasSongTimeSample = true;
        state.prevSongTimeMs = scaledLiveTimeMs;
        state.prevSongTimeReceiveTs = now;
        state.songTimeMs = scaledLiveTimeMs;
        state.songTimeReceiveTs = now;
        state.frozenInterpMs = scaledLiveTimeMs;

        if (hasAnyGraphModeEnabled()) {
            updateGraphCursor(state.songTimeMs);
        }
        return;
    }

    if (state.pauseDetectionEnabled && state.isInPlayState) {
        // 直接采用 tosu api_v2 的 game.paused 原生暂停标志，不再通过谱面时间
        // 冻结来推断暂停——游戏卡顿导致的 time 停滞不再产生误判。旧版 tosu
        // 无此字段时（undefined）一律视为未暂停，仅暂停检测不可用，不影响其余功能。
        const gamePaused = data?.game?.paused === true;
        if (gamePaused && !state.isPaused) {
            // 仅在谱面时间线内记录暂停标记（开头之前 / 末尾缓冲带内不记）。
            const hasTimelineEnd = Number.isFinite(state.songEndMs);
            const hasTimelineStart = Number.isFinite(state.songStartMs);
            const atTimelineEnd = hasTimelineEnd && scaledLiveTimeMs >= (state.songEndMs - NOTE_END_MARGIN_MS);
            const beforeTimelineStart = hasTimelineStart && scaledLiveTimeMs < state.songStartMs;
            if (!atTimelineEnd && !beforeTimelineStart) {
                addPauseMarker(scaledLiveTimeMs);
                state.pauseTimeMs = scaledLiveTimeMs;
                state.frozenInterpMs = scaledLiveTimeMs;
            }
            state.isPaused = true;
        } else if (!gamePaused && state.isPaused) {
            state.isPaused = false;
            state.pauseTimeMs = 0;
        }
    } else {
        state.isPaused = false;
        state.pauseTimeMs = 0;
        state.frozenInterpMs = state.songTimeMs;
    }

    state.prevSongTimeMs = previousTime;
    state.prevSongTimeReceiveTs = state.songTimeReceiveTs;
    state.songTimeMs = scaledLiveTimeMs;
    state.songTimeReceiveTs = now;

    if (Math.abs(state.songTimeMs - previousTime) > SONG_TIME_JUMP_THRESHOLD_MS) {
        state.prevSongTimeMs = state.songTimeMs;
        state.prevSongTimeReceiveTs = state.songTimeReceiveTs;
    }

    if (hasAnyGraphModeEnabled()) {
        updateGraphCursor(state.pauseDetectionEnabled && state.isPaused ? state.frozenInterpMs : state.songTimeMs);
    }
}

export function setupSocketListener() {
    socket.api_v2((data) => {
        noteTelemetryActivity();
        const normalizedClientStateName = normalizeClientStateName(data?.state?.name);
        if (normalizedClientStateName) {
            const wasInPlayState = state.isInPlayState;
            const nextInPlayState = isPlayStateName(normalizedClientStateName);
            const nextIsResultScreen = isResultScreenStateName(normalizedClientStateName);
            const enteredPlayState = !wasInPlayState && nextInPlayState;
            const leftPlayState = wasInPlayState && !nextInPlayState;

            state.clientStateName = normalizedClientStateName;
            state.isInPlayState = nextInPlayState;
            updateCardPlayVisibility();

            if (enteredPlayState || (leftPlayState && !nextIsResultScreen)) {
                resetPauseRuntime(true);
            } else if (leftPlayState) {
                resetPauseRuntime(false);
            }
        }

        const modData = getModData(data);
        if (modData.client) {
            state.client = modData.client;
        }
        if (modData.hasModPayload) {
            state.modCodes = modData.modCodes || [];
            state.classicMod = Boolean(modData.classic);
        }

        updateSongTimeState(data);

        // 每消息实时 PP：内部自带 early-return 守卫（成本极低），必须在
        // beatmap 守卫之前，保证 play/resultScreen 状态变化也走此路径。
        updateLivePp(data);

        const beatmap = data?.beatmap;
        if (!beatmap) return;

        const normalizeText = (value) => {
            if (value == null) return "";
            return String(value).trim();
        };

        const normalizePathText = (value) => {
            const normalized = normalizeText(value).replace(/\\/g, "/");
            if (!normalized) return "";
            return normalized.replace(/\/+/g, "/").toLowerCase();
        };

        const normalizeNumberText = (value) => {
            const num = Number(value);
            if (!Number.isFinite(num) || num <= 0) {
                return "";
            }
            return String(Math.trunc(num));
        };

        const beatmapId = normalizeNumberText(beatmap?.id);
        const beatmapHash = normalizeText(beatmap?.md5 || beatmap?.checksum).toLowerCase();
        const beatmapPath = normalizePathText(data?.files?.beatmap || data?.directPath?.beatmapFile);
        const beatmapTitleKey = [
            normalizeText(beatmap?.artist),
            normalizeText(beatmap?.title),
            normalizeText(beatmap?.version),
            normalizeText(beatmap?.mapper),
        ].join("::").toLowerCase();

        // 曲（mapset）单位的标识：不含 version/难度名，也不含 md5/id/path，
        // 这样同一 mapset 内切换难度时 songKey 保持不变，可据此区分
        // "换歌" 与 "换难度"。来源单一优先 set > dir > meta——
        // 避免 api_v2 各状态字段集合波动导致 key 变化（误判为换歌）。
        const beatmapSetId = normalizeNumberText(beatmap?.set || beatmap?.setId || beatmap?.beatmapSetId);
        const beatmapFolderPath = (() => {
            const folder = normalizePathText(data?.directPath?.beatmapBackground
                || data?.directPath?.audioFile
                || data?.folders?.beatmap);
            if (folder) return folder;
            if (!beatmapPath) return "";
            // 退而求其次：取谱面文件所在目录作为 mapset 归属。
            const lastSlash = beatmapPath.lastIndexOf("/");
            return lastSlash > 0 ? beatmapPath.slice(0, lastSlash) : beatmapPath;
        })();
        const songMetaKey = [
            normalizeText(beatmap?.artist),
            normalizeText(beatmap?.title),
            normalizeText(beatmap?.mapper),
        ].join("::").toLowerCase();
        const nextSongKey = buildSongKey({ beatmapSetId, beatmapFolderPath, songMetaKey });

        const previousBeatmapIdentity = state.lastBeatmapIdentity || "";
        const previousModSignature = state.modSignature || "";
        const previousSongKey = state.lastSongKey || "";

        const identityParts = [];
        if (beatmapId) {
            identityParts.push(`id:${beatmapId}`);
        }
        if (beatmapHash) {
            identityParts.push(`hash:${beatmapHash}`);
        }
        if (beatmapPath) {
            identityParts.push(`path:${beatmapPath}`);
        }

        const hasMetadataIdentity = beatmapTitleKey.replace(/[:]/g, "").length > 0;
        if (identityParts.length === 0 && hasMetadataIdentity) {
            identityParts.push(`meta:${beatmapTitleKey}`);
        }

        const nextBeatmapIdentity = identityParts.join("|");
        if (!nextBeatmapIdentity) return;

        // api_v2 packets can be partial. Only apply incoming mod state when
        // mod payload is explicitly present; otherwise keep current state.
        const shouldApplyModState = !previousModSignature
            || (modData.hasModPayload && (modData.hasModInfo || modData.hasExplicitNoMod));
        const nextModSignature = shouldApplyModState
            ? modData.modSignature
            : previousModSignature;

        const hasStateMismatch = nextBeatmapIdentity !== previousBeatmapIdentity
            || nextModSignature !== previousModSignature;
        if (!hasStateMismatch) return;

        if (shouldApplyModState) {
            state.speedRate = modData.speedRate;
            state.odFlag = modData.odFlag;
            state.cvtFlag = modData.cvtFlag;
            state.modSignature = nextModSignature;
        }

        // 区分本次变化的类型，供渲染层选择对应的入场动画（song/difficulty/mod）。
        state.pendingChangeKind = resolveChangeKind({
            previousBeatmapIdentity,
            nextBeatmapIdentity,
            previousSongKey,
            nextSongKey,
        });

        state.lastBeatmapIdentity = nextBeatmapIdentity;
        // 空 key（partial 包）不污染历史，避免后续完整包比较被污染。
        if (nextSongKey) state.lastSongKey = nextSongKey;
        state.lastBeatmapIdentitySource = identityParts.length > 1
            ? "composite"
            : (identityParts[0]?.split(":")[0] || "");

        // 仅在谱面本身（非单纯改 mod）发生变化时，重新取封面主色刷新主题。
        // 取色异步进行、失败自动退默认，绝不阻塞分析流程。
        if (state.enableCoverArt && nextBeatmapIdentity !== previousBeatmapIdentity) {
            applyCoverThemeForBeatmap(nextBeatmapIdentity).catch(() => {});
        }
        const key = `${nextBeatmapIdentity}|${nextModSignature}`;
        resetPauseRuntime(true);
        state.lastBeatmapKey = key;

        socket.sendCommand("getSettings", getCounterPathForCommand());

        scheduleRecompute("beatmap/mod changed", true);
    });
}
