import WebSocketManager from "./socket.js";
import { DISPLAY_SKILLSET_ORDER } from "../ett/index.js";
import { APP_CONFIG } from "../../config.js";
import { createSettingsParsers } from "./settingsParsers.js";

export { APP_CONFIG };

export const ENDPOINT = APP_CONFIG.endpoint;
export const SOCKET_HOST = APP_CONFIG.socketHost;

export function getSocketHost() {
    const host = typeof state.wsEndpoint === "string" ? state.wsEndpoint.trim() : "";
    return host || SOCKET_HOST;
}

export function getEndpoint() {
    return `http://${getSocketHost()}/files/beatmap/file`;
}

export const STAR_BG_STOPS = APP_CONFIG.starStops.background;
export const STAR_TEXT_STOPS = APP_CONFIG.starStops.text;

export const statusEl = document.getElementById("status");
export const reworkStarEl = document.getElementById("rework-star");
export const reworkDiffEl = document.getElementById("rework-diff");
export const reworkRightCapsuleEl = document.getElementById("rework-right-capsule");
export const reworkMetaEl = document.getElementById("rework-meta");
export const diffGraphWrapEl = document.getElementById("rework-diff-graph-wrap");
export const diffGraphSvgEl = document.getElementById("rework-diff-graph");
export const diffGraphFillEl = document.getElementById("rework-diff-graph-fill");
export const diffGraphLineEl = document.getElementById("rework-diff-graph-line");
export const diffGraphCursorEl = document.getElementById("rework-diff-graph-cursor");
export const diffGraphCursorDotEl = document.getElementById("rework-diff-graph-cursor-dot");
export const diffGraphPauseMarkersEl = document.getElementById("rework-diff-graph-pause-markers");
export const diffGraphErrorEl = document.getElementById("rework-diff-graph-error");
export const bodyGraphWrapEl = document.getElementById("body-graph-wrap");
export const bodyGraphSvgEl = document.getElementById("body-graph");
export const bodyGraphFillEl = document.getElementById("body-graph-fill");
export const bodyGraphLineEl = document.getElementById("body-graph-line");
export const bodyGraphCursorEl = document.getElementById("body-graph-cursor");
export const bodyGraphCursorDotEl = document.getElementById("body-graph-cursor-dot");
export const bodyGraphPauseMarkersEl = document.getElementById("body-graph-pause-markers");
export const bodyGraphErrorEl = document.getElementById("body-graph-error");
export const estDiffCaptionEl = document.getElementById("est-diff-caption");
export const patternClustersEl = document.getElementById("pattern-clusters");
export const ettSkillBarsEl = document.getElementById("ett-skill-bars");
export const pauseCountEl = document.getElementById("pause-count");
export const overlayEl = document.getElementById("card-overlay");
export const overlaySpinnerEl = document.getElementById("overlay-spinner");
export const overlayTitleEl = document.getElementById("overlay-title");
export const overlayMessageEl = document.getElementById("overlay-message");
export const mainCardEl = document.querySelector(".main-card");
export const modeTagEl = document.getElementById("mode-tag");

export const state = {
    lastBeatmapKey: "",
    client: "",
    speedRate: 1.0,
    odFlag: null,
    cvtFlag: null,
    modSignature: "",
    contentBar: APP_CONFIG.defaults.contentBar,
    srText: APP_CONFIG.defaults.srText,
    userContentBar: APP_CONFIG.defaults.contentBar,
    userSrText: APP_CONFIG.defaults.srText,
    userDiffText: APP_CONFIG.defaults.diffText,
    debugUseAmount: APP_CONFIG.defaults.debugUseAmount,
    debugUseSvDetection: APP_CONFIG.defaults.svDetection,
    diffText: APP_CONFIG.defaults.diffText,
    estimatorAlgorithm: APP_CONFIG.defaults.estimatorAlgorithm,
    pauseDetectionEnabled: APP_CONFIG.defaults.pauseDetectionEnabled,
    enableEtternaRainbowBars: APP_CONFIG.defaults.enableEtternaRainbowBars,
    enableNumericDifficulty: APP_CONFIG.defaults.enableNumericDifficulty,
    hideCardDuringPlay: APP_CONFIG.defaults.hideCardDuringPlay,
    vibroDetection: APP_CONFIG.defaults.vibroDetection,
    numericDifficulty: null,
    numericDifficultyHint: null,
    showModeTagCapsule: APP_CONFIG.defaults.showModeTagCapsule,
    currentModeTag: "Mix",
    etternaTechnicalHidden: false,
    graphSeries: null,
    pauseMarkerTimes: [],
    pauseCount: 0,
    isPaused: false,
    pauseTimeMs: 0,
    frozenInterpMs: 0,
    hasSongTimeSample: false,
    clientStateName: "",
    isInPlayState: false,
    songTimeMs: 0,
    prevSongTimeMs: 0,
    songTimeReceiveTs: 0,
    prevSongTimeReceiveTs: 0,
    songStartMs: null,
    songEndMs: null,
    graphAnimationStarted: false,
    recalcTimerId: null,
    settingsCommandSubscribed: false,
    settingsRequested: false,
    settingsReceivedFromCommand: false,
    initialSettingsResolver: null,
    analysisRequestSeq: 0,
    wsEndpoint: APP_CONFIG.defaults.wsEndpoint || SOCKET_HOST
};

export const MODE_TAG_OPTIONS = APP_CONFIG.options.modeTag;
export const ETT_SKILLSET_ORDER = DISPLAY_SKILLSET_ORDER.filter((name) => name !== "Overall");
export const ETT_SKILLSET_ORDER_NO_TECHNICAL = ETT_SKILLSET_ORDER.filter((name) => name !== "Technical");
export const ETT_MAX_SKILL_VALUE = APP_CONFIG.etterna.maxSkillValue;
export const VIBRO_JACKSPEED_RATIO_THRESHOLD = APP_CONFIG.etterna.vibroJackspeedRatioThreshold;

export const GRAPH_VIEWBOX_WIDTH = APP_CONFIG.graph.viewboxWidth;
export const GRAPH_VIEWBOX_HEIGHT = APP_CONFIG.graph.viewboxHeight;
export const GRAPH_PADDING_X = APP_CONFIG.graph.paddingX;
export const GRAPH_PADDING_TOP = APP_CONFIG.graph.paddingTop;
export const GRAPH_PADDING_BOTTOM = APP_CONFIG.graph.paddingBottom;
export const GRAPH_RESAMPLE_INTERVAL_MS = APP_CONFIG.graph.resampleIntervalMs;
export const PAUSE_LINE_COLOR = APP_CONFIG.graph.pauseLineColor;
export const PAUSE_LINE_WIDTH = APP_CONFIG.graph.pauseLineWidth;

export const GRAPH_LOADING_BASELINE_Y = GRAPH_VIEWBOX_HEIGHT - GRAPH_PADDING_BOTTOM;

export const SONG_TIME_JUMP_THRESHOLD_MS = APP_CONFIG.timing.songTimeJumpThresholdMs;
export const NOTE_END_MARGIN_MS = APP_CONFIG.timing.noteEndMarginMs;
export const PAUSE_DETECT_EPSILON_MS = APP_CONFIG.timing.pauseDetectEpsilonMs;

export const SOCKET_RECALC_LAZY_DELAY_MS = APP_CONFIG.timing.socketRecalcLazyDelayMs;
export const SETTINGS_COMMAND_TIMEOUT_MS = APP_CONFIG.timing.settingsCommandTimeoutMs;

export const socket = new WebSocketManager(getSocketHost());

export const GRAPH_SUPPORTED_KEY_SET = new Set([4, 6, 7]);

const KNOWN_MOD_CODES = APP_CONFIG.mods.knownCodes;
const MOD_BIT_FLAGS = APP_CONFIG.mods.bitFlags;
export const SORTED_KNOWN_MOD_CODES = [...KNOWN_MOD_CODES].sort((a, b) => b.length - a.length);
export const MOD_BIT_FLAG_ENTRIES = Object.entries(MOD_BIT_FLAGS);

export const PATTERN_BAR_GRADIENT = "linear-gradient(90deg, #4ec7ff 0%, #58f0d9 60%, #f6ef6b 100%)";

export const {
    parseContentBarValue,
    parseSrTextValue,
    parseDebugUseAmountValue,
    parseDiffTextValue,
    parseAutoModeValue,
    parseUseDanielAlgorithmValue,
    parseEstimatorAlgorithmValue,
    parseEnablePauseDetectionValue,
    parseDisableVibroDetectionValue,
    parseVibroDetectionValue,
    parseEnableEtternaRainbowBarsValue,
    parseShowModeTagCapsuleValue,
    parseEnableNumericDifficultyValue,
    parseHideCardDuringPlayValue,
    parseSvDetectionValue,
    parseWsEndpointValue,
} = createSettingsParsers(APP_CONFIG);

export const GRAPH_VIEW_DEFS = [
    {
        key: "header",
        wrapEl: diffGraphWrapEl,
        svgEl: diffGraphSvgEl,
        fillEl: diffGraphFillEl,
        lineEl: diffGraphLineEl,
        cursorEl: diffGraphCursorEl,
        cursorDotEl: diffGraphCursorDotEl,
        pauseMarkersEl: diffGraphPauseMarkersEl,
        errorEl: diffGraphErrorEl,
        isEnabled: () => state.diffText === "Graph",
    },
    {
        key: "body",
        wrapEl: bodyGraphWrapEl,
        svgEl: bodyGraphSvgEl,
        fillEl: bodyGraphFillEl,
        lineEl: bodyGraphLineEl,
        cursorEl: bodyGraphCursorEl,
        cursorDotEl: bodyGraphCursorDotEl,
        pauseMarkersEl: bodyGraphPauseMarkersEl,
        errorEl: bodyGraphErrorEl,
        isEnabled: () => state.contentBar === "Graph",
    },
];

export function hasAnyGraphModeEnabled() {
    return state.diffText === "Graph" || state.contentBar === "Graph";
}

export function forEachGraphView(callback) {
    for (const view of GRAPH_VIEW_DEFS) {
        callback(view);
    }
}

export function forEachEnabledGraphView(callback) {
    for (const view of GRAPH_VIEW_DEFS) {
        if (view.isEnabled()) {
            callback(view);
        }
    }
}

export function isAutoSrTextEnabled() {
    return state.userSrText === "Auto";
}

export function isAutoContentBarEnabled() {
    return state.userContentBar === "Auto";
}
