import {
    getActiveContentBar,
    MOD_BIT_FLAG_ENTRIES,
    NOTE_END_MARGIN_MS,
    PAUSE_DETECT_EPSILON_MS,
    SONG_TIME_JUMP_THRESHOLD_MS,
    SORTED_KNOWN_MOD_CODES,
    socket,
    state,
} from "./appContext.js";
import { computePauseTransition } from "./pauseDetection.js";
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
    clearAllPauseMarkers,
    resetPauseRuntime,
    updateGraphCursor,
} from "./graph.js";
import { updateCardPlayVisibility } from "./hud.js";
import { scheduleRecompute } from "./scheduler.js";

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

        if (state.diffText === "Graph" || getActiveContentBar() === "Graph") {
            updateGraphCursor(state.songTimeMs);
        }
        return;
    }

    if (state.pauseDetectionEnabled && state.isInPlayState) {
        const pauseTransition = computePauseTransition({
            previousTimeMs: previousTime,
            currentTimeMs: scaledLiveTimeMs,
            isPaused: state.isPaused,
            jumpThresholdMs: SONG_TIME_JUMP_THRESHOLD_MS,
            noteEndMarginMs: NOTE_END_MARGIN_MS,
            timelineStartMs: state.songStartMs,
            timelineEndMs: state.songEndMs,
            epsilonMs: PAUSE_DETECT_EPSILON_MS,
        });

        if (pauseTransition.shouldClearMarkers) {
            clearAllPauseMarkers();
        }

        if (pauseTransition.shouldAddMarker) {
            addPauseMarker(pauseTransition.pauseTimeMs);
            state.pauseTimeMs = pauseTransition.pauseTimeMs;
            state.frozenInterpMs = pauseTransition.frozenInterpMs;
        }

        state.isPaused = pauseTransition.nextPaused;
        if (!state.isPaused) {
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

    if (state.diffText === "Graph" || getActiveContentBar() === "Graph") {
        updateGraphCursor(state.pauseDetectionEnabled && state.isPaused ? state.frozenInterpMs : state.songTimeMs);
    }
}

export function setupSocketListener() {
    socket.api_v2((data) => {
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

        updateSongTimeState(data);

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

        const previousBeatmapIdentity = state.lastBeatmapIdentity || "";
        const previousModSignature = state.modSignature || "";

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

        state.lastBeatmapIdentity = nextBeatmapIdentity;
        state.lastBeatmapIdentitySource = identityParts.length > 1
            ? "composite"
            : (identityParts[0]?.split(":")[0] || "");
        const key = `${nextBeatmapIdentity}|${nextModSignature}`;
        resetPauseRuntime(true);
        state.lastBeatmapKey = key;

        scheduleRecompute("beatmap/mod changed", true);
    });
}
