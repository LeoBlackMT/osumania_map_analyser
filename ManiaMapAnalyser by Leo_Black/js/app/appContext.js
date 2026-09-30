import WebSocketManager from "./socket.js";
import { DISPLAY_SKILLSET_ORDER } from "../ett/index.js";
import { APP_CONFIG } from "../../config.js";
import { createSettingsParsers } from "../parser/settingsParser.js";

export { APP_CONFIG };

export const ENDPOINT = APP_CONFIG.endpoint;
export const SOCKET_HOST = APP_CONFIG.socketHost;

/** osu 数据面的默认路径（= 现状常量；契约 v6 的 `osuTransport.{wsPath,filesPath}` 期望值）。 */
const OSU_WS_PATH = "/websocket/v2";
const OSU_FILES_PATH = "/files/beatmap";
/** 壳页端口：只有壳自己的窗口（24061）接受运行时端点覆盖。 */
const SHELL_PAGE_PORT = "24061";

/**
 * 单一 host 字符串（WS URL、`.osu` 端点、背景图全由它派生，DEC-20）：
 * 运行时覆盖（契约 v6：壳经 state 帧下发 `sources.osu.osuTransport`）优先，
 * 否则 = 设置里的 `wsEndpoint`（无壳 / `mode:"tosu"` 时行为逐字节不变）。
 *
 * **绝不写 `state.wsEndpoint`** —— 它在 `SETTING_CACHE_KEYS` 里（settings.js:783-789），
 * 走设置路径会每次切换清空结果缓存（DEC-12）。
 */
export function getSocketHost() {
    if (state.runtimeOsuHost) {
        return state.runtimeOsuHost;
    }
    const host = typeof state.wsEndpoint === "string" ? state.wsEndpoint.trim() : "";
    return host || SOCKET_HOST;
}

/** 运行时覆盖是否激活（native 传输）。覆盖只由壳页的 state 帧写入。 */
export function isRuntimeOsuOverrideActive() {
    return Boolean(state.runtimeOsuHost);
}

/**
 * 本页是否是**壳自己的页**（24061）。只有它接受运行时端点覆盖（见 `normalizeOsuTransport`）
 * 与壳的相位提示（`sources/osuScanHint.js`）：浏览器 tosu 页（24050）上壳也可能在跑，
 * 但那页面的数据面是 tosu，壳的读取器状态与它无关。
 */
export function isShellPage() {
    return typeof window !== "undefined"
        && Boolean(window.location)
        && String(window.location.port) === SHELL_PAGE_PORT;
}

export function getEndpoint() {
    return `http://${getSocketHost()}${OSU_FILES_PATH}/file`;
}

/**
 * 归一化壳下发的 osu 端点描述：不合格一律 `null`（= 回落 `wsEndpoint`，绝不猜）。
 *
 * 页面的 WS 路径与文件路径是硬编码的，故壳声明的 `wsPath`/`filesPath` 必须与之一致；
 * 对不上说明这个 origin 与页面对不上话，**拒绝覆盖**（fail-closed，仍走 tosu）。
 */
function normalizeOsuTransport(transport) {
    if (!transport || transport.mode !== "native") {
        return null;
    }
    if (!isShellPage()) {
        return null; // 浏览器 tosu 页：osu 是唯一数据面，绝不被壳换掉端点
    }
    const host = typeof transport.host === "string" ? transport.host.trim() : "";
    const port = Number(transport.port);
    if (!host || !Number.isInteger(port) || port <= 0 || port > 65535) {
        return null;
    }
    if (transport.wsPath !== OSU_WS_PATH || transport.filesPath !== OSU_FILES_PATH) {
        console.warn(`mma shell: refusing osu transport with unknown paths (wsPath=${transport.wsPath}, filesPath=${transport.filesPath})`);
        return null;
    }
    return { host: `${host}:${port}` };
}

/**
 * 应用壳下发的 osu 端点（契约 v6 的 `sources.osu.osuTransport`）。
 *
 * - 只接受 `mode:"native"` 且字段/路径合格的端点（见 `normalizeOsuTransport`）；
 * - 其余（`mode:"tosu"` / 字段缺失 / 端口非法 / 路径不符）⇒ 清覆盖，`getSocketHost()`
 *   回到 `state.wsEndpoint`；
 * - 覆盖变化只做 `socket.setHost(...)`（关掉旧连接 + 按新 host 重开）。**不碰结果缓存**：
 *   既不写 `state.wsEndpoint`，也不派发设置变更 ⇒ `wsEndpoint` 的缓存失效路径不会被触发。
 * @param {object|null} transport 壳 state 帧里的 `sources.osu.osuTransport`
 * @returns {boolean} 覆盖是否变化（true = 已按新端点重开 socket）
 */
export function applyOsuTransport(transport) {
    const next = normalizeOsuTransport(transport);
    const host = next ? next.host : "";
    if (host === state.runtimeOsuHost) {
        return false;
    }
    state.runtimeOsuHost = host;
    socket.setHost(getSocketHost(), true);
    return true;
}

export const STAR_BG_STOPS = APP_CONFIG.starStops.background;
export const STAR_TEXT_STOPS = APP_CONFIG.starStops.text;

export const statusEl = document.getElementById("status");
// 壳相位提示的**专属**元素（Step 9f）：与 `#status` 同排但写入者只有 `sources/osuScanHint.js`
// —— 共享状态行由分析流程掌控，提示借写在那里会被任何一次外部改写打掉（Step 9e 实测只闪 300 ms）。
export const osuScanHintEl = document.getElementById("osu-scan-hint");
export const reworkStarEl = document.getElementById("rework-star");
export const reworkDiffEl = document.getElementById("rework-diff");
export const reworkRightCapsuleEl = document.getElementById("rework-right-capsule");
export const reworkMetaEl = document.getElementById("rework-meta");
export const reworkBlockEl = document.getElementById("rework");
export const diffGraphWrapEl = document.getElementById("rework-diff-graph-wrap");
export const diffGraphSvgEl = document.getElementById("rework-diff-graph");
export const diffGraphFillEl = document.getElementById("rework-diff-graph-fill");
export const diffGraphFillPlayEl = document.getElementById("rework-diff-graph-fill-play");
export const diffGraphPlayClipRectEl = document.getElementById("rework-diff-graph-play-clip-rect");
export const diffGraphLineEl = document.getElementById("rework-diff-graph-line");
export const diffGraphCursorEl = document.getElementById("rework-diff-graph-cursor");
export const diffGraphCursorDotEl = document.getElementById("rework-diff-graph-cursor-dot");
export const diffGraphPauseMarkersEl = document.getElementById("rework-diff-graph-pause-markers");
export const diffGraphErrorEl = document.getElementById("rework-diff-graph-error");
export const bodyGraphWrapEl = document.getElementById("body-graph-wrap");
export const bodyGraphSvgEl = document.getElementById("body-graph");
export const bodyGraphFillEl = document.getElementById("body-graph-fill");
export const bodyGraphFillPlayEl = document.getElementById("body-graph-fill-play");
export const bodyGraphPlayClipRectEl = document.getElementById("body-graph-play-clip-rect");
export const bodyGraphLineEl = document.getElementById("body-graph-line");
export const bodyGraphCursorEl = document.getElementById("body-graph-cursor");
export const bodyGraphCursorDotEl = document.getElementById("body-graph-cursor-dot");
export const bodyGraphPauseMarkersEl = document.getElementById("body-graph-pause-markers");
export const bodyGraphErrorEl = document.getElementById("body-graph-error");
export const estDiffCaptionEl = document.getElementById("est-diff-caption");
export const patternClustersEl = document.getElementById("pattern-clusters");
export const ettSkillBarsEl = document.getElementById("ett-skill-bars");
export const pauseCountEl = document.getElementById("pause-count");
export const ppBarsEl = document.getElementById("pp-bars");
export const sepPpEl = document.getElementById("sep-pp");
export const overlayEl = document.getElementById("card-overlay");
export const overlaySpinnerEl = document.getElementById("overlay-spinner");
export const overlayTitleEl = document.getElementById("overlay-title");
export const overlayMessageEl = document.getElementById("overlay-message");
export const mainCardEl = document.querySelector(".main-card");
export const dashboardEl = document.querySelector(".dashboard");
export const titleIconEl = document.querySelector(".title-icon");
export const modeTagSubGroupEl = document.getElementById("mode-tag-subgroup");
export const svTagEl = document.getElementById("sv-tag");
export const starTipEl = document.getElementById("star-tip");

export const state = {
    lastBeatmapKey: "",
    lastBeatmapIdentity: "",
    lastBeatmapIdentitySource: "",
    // 本页面已通过自己的 tosu 数据面（直连 socket）收到过载荷——页面侧信号，
    // 与壳的 shellTosuOnline 探测位无关。
    tosuDataSeen: false,
    lastSongKey: "",
    pendingChangeKind: "",
    activeChangeKind: "",
    client: "",
    speedRate: 1.0,
    odFlag: null,
    cvtFlag: null,
    modSignature: "",
    ppMetrics: null,
    modCodes: [],
    classicMod: false,
    contentBar: APP_CONFIG.defaults.contentBar,
    effectiveContentBar: null,
    srText: APP_CONFIG.defaults.srText,
    userContentBar: APP_CONFIG.defaults.contentBar,
    userSrText: APP_CONFIG.defaults.srText,
    userDiffText: APP_CONFIG.defaults.diffText,
    debugUseAmount: APP_CONFIG.defaults.debugUseAmount,
    useSvDetection: APP_CONFIG.defaults.useSvDetection,
    display6kLevel: APP_CONFIG.defaults.display6kLevel,
    sunnySR: null,
    extendedEstimationRange: APP_CONFIG.defaults.extendedEstimationRange,
    diffText: APP_CONFIG.defaults.diffText,
    estimatorAlgorithm: APP_CONFIG.defaults.estimatorAlgorithm,
    actualEstimatorAlgorithm: APP_CONFIG.defaults.estimatorAlgorithm,
    azusaSunnyReferenceHo: APP_CONFIG.defaults.azusaSunnyReferenceHo,
    etternaVersion: APP_CONFIG.defaults.etternaVersion,
    companellaEtternaVersion: APP_CONFIG.defaults.companellaEtternaVersion,
    pauseDetectionEnabled: APP_CONFIG.defaults.pauseDetectionEnabled,
    enableEtternaRainbowBars: APP_CONFIG.defaults.enableEtternaRainbowBars,
    enableStatusMarquee: APP_CONFIG.defaults.enableStatusMarquee,
    enableNumericDifficulty: APP_CONFIG.defaults.enableNumericDifficulty,
    cardVisibility: APP_CONFIG.defaults.cardVisibility,
    cardOpacity: APP_CONFIG.defaults.cardOpacity,
    cardRadius: APP_CONFIG.defaults.cardRadius,
    cardBgBlur: APP_CONFIG.defaults.cardBgBlur,
    enableUpdateCheck: APP_CONFIG.defaults.enableUpdateCheck,
    enableResultCache: APP_CONFIG.defaults.enableResultCache,
    hasAvailableUpdate: false,
    reverseCardExtendDirection: APP_CONFIG.defaults.reverseCardExtendDirection,
    useOsuFont: APP_CONFIG.defaults.useOsuFont,
    enableOsuTheme: APP_CONFIG.defaults.enableOsuTheme,
    enableFloatingTriangles: APP_CONFIG.defaults.enableFloatingTriangles,
    enableCoverArt: APP_CONFIG.defaults.enableCoverArt,
    customBackgroundColor: APP_CONFIG.defaults.customBackgroundColor,
    vibroDetection: APP_CONFIG.defaults.vibroDetection,
    forceSunnyWindow: APP_CONFIG.defaults.forceSunnyWindow,
    enableLNDifficulty: APP_CONFIG.defaults.enableLNDifficulty,
    enableAnalyzeLN: APP_CONFIG.defaults.enableAnalyzeLN,
    enableAlwaysShowLNDifficulty: APP_CONFIG.defaults.enableAlwaysShowLNDifficulty,
    enableTelemetry: APP_CONFIG.defaults.enableTelemetry,
    numericDifficulty: null,
    numericDifficultyHint: null,
    lnStar: 0,
    forceHideNumericDifficulty: false,
    showModeTagCapsule: APP_CONFIG.defaults.showModeTagCapsule,
    showSvTag: false,
    statusText: "",
    statusKind: "loading",
    currentModeTag: "Mix",
    etternaTechnicalHidden: false,
    graphSeries: null,
    // 未裁剪的归一化序列 + 渲染时用的谱面时间线：时间线补全后据此重画 x 轴窗口。
    graphSeriesSource: null,
    graphSeriesTimelineStartMs: null,
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
    wsEndpoint: APP_CONFIG.defaults.wsEndpoint || SOCKET_HOST,
    // 运行时 osu 端点覆盖（契约 v6 / DEC-12）：壳页的 state 帧写入；空 = 用 wsEndpoint。
    // 只被 getSocketHost() 读，绝不参与设置路径（见 applyOsuTransport）。
    runtimeOsuHost: "",
    // 壳 `sources.osu` 的传输位：native 传输可用（sourceManager 的败方门控据此把 osu 当活源）。
    shellOsuNativeAlive: false,
    // 壳 `sources.osu` 的相位诊断（契约 v6 / Step 9e）：`waiting-for-game` / `attaching` /
    // `scanning` / `healthy` / `unavailable`。提示句 `notice` 由**壳**给（英文），
    // 页面只渲染它、不做自己的文案；`shellOsuProgress` 只作诊断（算子可读，页面不渲染）。
    shellOsuPhase: null,
    shellOsuNotice: null,
    shellOsuProgress: null,
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

// 注意：GRAPH_SUPPORTED_KEY_SET 已移除——Graph 数据 = estimator 的 star 序列
// （Sunny 核心为键数无关算法），任意键数均可渲染；渲染失败由
// showDiffGraphError("Graph unavailable") 兜底。

const KNOWN_MOD_CODES = APP_CONFIG.mods.knownCodes;
const MOD_BIT_FLAGS = APP_CONFIG.mods.bitFlags;
export const SORTED_KNOWN_MOD_CODES = [...KNOWN_MOD_CODES].sort((a, b) => b.length - a.length);
export const MOD_BIT_FLAG_ENTRIES = Object.entries(MOD_BIT_FLAGS);

export const {
    parseContentBarValue,
    parseSrTextValue,
    parseDebugUseAmountValue,
    parseDiffTextValue,
    parseAutoModeValue,
    parseEstimatorAlgorithmValue,
    parseAzusaSunnyReferenceHoValue,
    parseEtternaVersionValue,
    parseCompanellaEtternaVersionValue,
    parseEnablePauseDetectionValue,
    parseEnableResultCacheValue,
    parseVibroDetectionValue,
    parseEnableEtternaRainbowBarsValue,
    parseEnableStatusMarqueeValue,
    parseShowModeTagCapsuleValue,
    parseEnableNumericDifficultyValue,
    parseCardVisibilityValue,
    parseCardOpacityValue,
    parseCardRadiusValue,
    parseCardBgBlurValue,
    parseEnableUpdateCheckValue,
    parseReverseCardExtendDirectionValue,
    parseUseOsuFontValue,
    parseEnableOsuThemeValue,
    parseEnableFloatingTrianglesValue,
    parseEnableCoverArtValue,
    parseCustomBackgroundColorValue,
    parseSvDetectionValue,
    parseDisplay6kLevelValue,
    parseExtendedEstimationRangeValue,
    parseWsEndpointValue,
    parseForceSunnyWindowValue,
    parseEnableLNDifficultyValue,
    parseEnableAnalyzeLNValue,
    parseEnableAlwaysShowLNDifficultyValue,
    parseEnableTelemetryValue,
    parseGameClientValue,
} = createSettingsParsers(APP_CONFIG);

export function getActiveContentBar() {
    return state.effectiveContentBar || state.contentBar;
}

export function contentBarShows(section) {
    const active = getActiveContentBar();
    return active === section || active === "Full";
}

export const GRAPH_VIEW_DEFS = [
    {
        key: "header",
        wrapEl: diffGraphWrapEl,
        svgEl: diffGraphSvgEl,
        fillEl: diffGraphFillEl,
        fillPlayEl: diffGraphFillPlayEl,
        playClipRectEl: diffGraphPlayClipRectEl,
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
        fillPlayEl: bodyGraphFillPlayEl,
        playClipRectEl: bodyGraphPlayClipRectEl,
        lineEl: bodyGraphLineEl,
        cursorEl: bodyGraphCursorEl,
        cursorDotEl: bodyGraphCursorDotEl,
        pauseMarkersEl: bodyGraphPauseMarkersEl,
        errorEl: bodyGraphErrorEl,
        isEnabled: () => contentBarShows("Graph"),
    },
];

export function hasAnyGraphModeEnabled() {
    return state.diffText === "Graph" || contentBarShows("Graph");
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
