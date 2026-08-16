/**
 * Preset system — fully self-contained module.
 *
 * Design goals (minimal footprint on the original plugin):
 *  - No changes to settings.js / appContext.js / config.js / main.js /
 *    index.html / styles. The ONLY repo change besides this file is the
 *    "preset" option added to settings.json.
 *  - Opens its own /websocket/commands connection (WebSocketManager supports
 *    multiple connections) to observe the tosu settings stream, detects
 *    preset-picker changes and manual settings changes by diffing snapshots,
 *    applies presets through the existing apply* functions and syncs results
 *    back to tosu via the settings API.
 *  - Creates its manager UI (and stylesheet link) dynamically; initializes
 *    itself on module load.
 */

import { APP_CONFIG, socket, state } from "./appContext.js";
import {
    applyAzusaSunnyReferenceHoSetting,
    applyCardBgBlurSetting,
    applyCardOpacitySetting,
    applyCardRadiusSetting,
    applyCardVisibilitySetting,
    applyCompanellaEtternaVersionSetting,
    applyContentBarSetting,
    applyCustomBackgroundColorSetting,
    applyDebugUseAmountSetting,
    applyDiffTextSetting,
    applyDisplay6kLevelSetting,
    applyEnableAlwaysShowLNDifficultySetting,
    applyEnableAnalyzeLNSetting,
    applyEnableCoverArtSetting,
    applyEnableEtternaRainbowBarsSetting,
    applyEnableFloatingTrianglesSetting,
    applyEnableLNDifficultySetting,
    applyEnableNumericDifficultySetting,
    applyEnableOsuThemeSetting,
    applyEnableResultCacheSetting,
    applyEnableStatusMarqueeSetting,
    applyEnableUpdateCheckSetting,
    applyEstimatorAlgorithmSetting,
    applyEtternaVersionSetting,
    applyExtendedEstimationRangeSetting,
    applyForceSunnyWindowSetting,
    applyPauseDetectionSetting,
    applyPauseDetectionThresholdSetting,
    applyReverseCardExtendDirectionSetting,
    applyShowModeTagCapsuleSetting,
    applySrTextSetting,
    applyUseOsuFontSetting,
    applyUseSvDetectionSetting,
    applyVibroDetectionSetting,
    applyWsEndpointSetting,
} from "./settings.js";
import { clearResultCache } from "./resultCache.js";
import { scheduleRecompute } from "./scheduler.js";

const CUSTOM_PRESETS_KEY = "mma.presets.custom.v1";
const ACTIVE_PRESET_KEY = "mma.presets.active.v1";
// System-managed container that follows manual settings changes when no custom
// preset is anchored. It is NOT an applicable snapshot — selecting "Auto" only
// marks "keep following my manual changes". Reserved name.
const AUTO_SAVE_PRESET_NAME = "Last Saved Preset";
// Default anchor slots, created automatically on first load. They behave like
// any other custom preset (rename/delete allowed); re-creation is skipped once
// present. Picking them in the dashboard dropdown materializes them on demand.
const DEFAULT_SLOT_NAMES = ["Custom 1", "Custom 2", "Custom 3"];

// Built-in presets (moved here so config.js stays untouched): each is a full
// snapshot = APP_CONFIG.defaults + these overrides.
const PRESET_DEFS = [
    {
        id: "default",
        name: "Default",
        description: "Reset to the factory default configuration.",
        settings: {},
    },
    {
        id: "mini",
        name: "mini",
        description: "Star rating only: no card body content, no top-right content, no map tag capsule.",
        settings: {
            contentBar: "None",
            srText: "Pattern",
            diffText: "Difficulty",
            debugUseAmount: false,
            estimatorAlgorithm: "Mixed",
            azusaSunnyReferenceHo: true,
            etternaVersion: "0.72.3",
            companellaEtternaVersion: "0.74.0",
            enablePauseDetection: true,
            pauseDetectionThreshold: "500",
            enableEtternaRainbowBars: false,
            enableStatusMarquee: true,
            VibroDetection: false,
            showModeTagCapsule: true,
            enableNumericDifficulty: true,
            cardVisibility: "Always",
            cardOpacity: "95%",
            cardRadius: "Medium",
            cardBgBlur: "4px",
            enableUpdateCheck: true,
            enableResultCache: true,
            reverseCardExtendDirection: false,
            useOsuFont: true,
            enableOsuTheme: true,
            enableFloatingTriangles: true,
            enableCoverArt: true,
            customBackgroundColor: "#000000",
            useSvDetection: false,
            forceSunnyWindow: true,
            enableLNDifficulty: true,
            enableAnalyzeLN: true,
            enableAlwaysShowLNDifficulty: true,
            display6kLevel: true,
            extendedEstimationRange: false,
        },
    },
    {
        id: "im-osu-main",
        name: "For osu Player",
        description: "Difficulty graph in the card body, pattern in the top-left capsule, estimated difficulty at top-right.",
        settings: {
            contentBar: "Graph",
            srText: "Pattern",
            diffText: "Difficulty",
            debugUseAmount: false,
            estimatorAlgorithm: "Mixed",
            azusaSunnyReferenceHo: true,
            etternaVersion: "0.72.3",
            companellaEtternaVersion: "0.74.0",
            enablePauseDetection: true,
            pauseDetectionThreshold: "500",
            enableEtternaRainbowBars: false,
            enableStatusMarquee: true,
            VibroDetection: true,
            showModeTagCapsule: true,
            enableNumericDifficulty: true,
            cardVisibility: "Always",
            cardOpacity: "95%",
            cardRadius: "Medium",
            cardBgBlur: "4px",
            enableUpdateCheck: true,
            enableResultCache: true,
            reverseCardExtendDirection: false,
            useOsuFont: true,
            enableOsuTheme: true,
            enableFloatingTriangles: true,
            enableCoverArt: true,
            customBackgroundColor: "#000000",
            useSvDetection: true,
            forceSunnyWindow: true,
            enableLNDifficulty: true,
            enableAnalyzeLN: true,
            enableAlwaysShowLNDifficulty: true,
            display6kLevel: true,
            extendedEstimationRange: false,
        },
    },
    {
        id: "im-etterna-main",
        name: "For Etterna Player",
        description: "Etterna skillset bars in the card body with MSD on both capsules.",
        settings: {
            contentBar: "Etterna",
            srText: "MSD",
            diffText: "Difficulty",
            debugUseAmount: false,
            estimatorAlgorithm: "Mixed",
            azusaSunnyReferenceHo: true,
            etternaVersion: "0.72.3",
            companellaEtternaVersion: "0.74.0",
            enablePauseDetection: true,
            pauseDetectionThreshold: "500",
            enableEtternaRainbowBars: false,
            enableStatusMarquee: true,
            VibroDetection: false,
            showModeTagCapsule: true,
            enableNumericDifficulty: true,
            cardVisibility: "Always",
            cardOpacity: "95%",
            cardRadius: "Medium",
            cardBgBlur: "4px",
            enableUpdateCheck: true,
            enableResultCache: true,
            reverseCardExtendDirection: false,
            useOsuFont: true,
            enableOsuTheme: true,
            enableFloatingTriangles: true,
            enableCoverArt: true,
            customBackgroundColor: "#000000",
            useSvDetection: false,
            forceSunnyWindow: false,
            enableLNDifficulty: false,
            enableAnalyzeLN: false,
            enableAlwaysShowLNDifficulty: true,
            display6kLevel: true,
            extendedEstimationRange: false,
        },
    },
    {
        id: "im-interlude-main",
        name: "For Interlude Player",
        description: "Pattern analysis, InterludeSR on the left, shown outside play.",
        settings: {
            contentBar: "Pattern",
            srText: "InterludeSR",
            diffText: "Difficulty",
            debugUseAmount: false,
            estimatorAlgorithm: "Mixed",
            azusaSunnyReferenceHo: true,
            etternaVersion: "0.72.3",
            companellaEtternaVersion: "0.74.0",
            enablePauseDetection: true,
            pauseDetectionThreshold: "500",
            enableEtternaRainbowBars: false,
            enableStatusMarquee: true,
            VibroDetection: false,
            showModeTagCapsule: true,
            enableNumericDifficulty: true,
            cardVisibility: "OutsidePlay",
            cardOpacity: "95%",
            cardRadius: "Medium",
            cardBgBlur: "4px",
            enableUpdateCheck: true,
            enableResultCache: true,
            reverseCardExtendDirection: false,
            useOsuFont: true,
            enableOsuTheme: true,
            enableFloatingTriangles: true,
            enableCoverArt: true,
            customBackgroundColor: "#000000",
            useSvDetection: false,
            forceSunnyWindow: true,
            enableLNDifficulty: true,
            enableAnalyzeLN: true,
            enableAlwaysShowLNDifficulty: true,
            display6kLevel: true,
            extendedEstimationRange: false,
        },
    },
    {
        id: "pattern-focus",
        name: "Pattern Focus",
        description: "Pattern analysis in the card body and the top-left capsule.",
        settings: {
            contentBar: "Pattern",
            srText: "Pattern",
            diffText: "Difficulty",
            debugUseAmount: false,
            estimatorAlgorithm: "Mixed",
            azusaSunnyReferenceHo: true,
            etternaVersion: "0.72.3",
            companellaEtternaVersion: "0.74.0",
            enablePauseDetection: true,
            pauseDetectionThreshold: "500",
            enableEtternaRainbowBars: false,
            enableStatusMarquee: true,
            VibroDetection: false,
            showModeTagCapsule: true,
            enableNumericDifficulty: true,
            cardVisibility: "OutsidePlay",
            cardOpacity: "95%",
            cardRadius: "Medium",
            cardBgBlur: "4px",
            enableUpdateCheck: true,
            enableResultCache: true,
            reverseCardExtendDirection: false,
            useOsuFont: true,
            enableOsuTheme: true,
            enableFloatingTriangles: true,
            enableCoverArt: true,
            customBackgroundColor: "#000000",
            useSvDetection: false,
            forceSunnyWindow: true,
            enableLNDifficulty: true,
            enableAnalyzeLN: true,
            enableAlwaysShowLNDifficulty: true,
            display6kLevel: true,
            extendedEstimationRange: false,
        },
    },
    {
        id: "full-overview",
        name: "Full Overview",
        description: "Pattern, Etterna and graph together, ReworkSR on the left, graph at top-right.",
        settings: {
            contentBar: "Full",
            srText: "Pattern",
            diffText: "Difficulty",
            debugUseAmount: false,
            estimatorAlgorithm: "Mixed",
            azusaSunnyReferenceHo: true,
            etternaVersion: "0.72.3",
            companellaEtternaVersion: "0.74.0",
            enablePauseDetection: true,
            pauseDetectionThreshold: "500",
            enableEtternaRainbowBars: false,
            enableStatusMarquee: true,
            VibroDetection: false,
            showModeTagCapsule: true,
            enableNumericDifficulty: true,
            cardVisibility: "Always",
            cardOpacity: "95%",
            cardRadius: "Medium",
            cardBgBlur: "4px",
            enableUpdateCheck: true,
            enableResultCache: true,
            reverseCardExtendDirection: true,
            useOsuFont: true,
            enableOsuTheme: true,
            enableFloatingTriangles: true,
            enableCoverArt: true,
            customBackgroundColor: "#000000",
            useSvDetection: false,
            forceSunnyWindow: true,
            enableLNDifficulty: true,
            enableAnalyzeLN: true,
            enableAlwaysShowLNDifficulty: true,
            display6kLevel: true,
            extendedEstimationRange: false,
        },
    },
    {
        id: "vibro-player",
        name: "Vibro Player",
        description: "Etterna skillset bars with MSD, graph at top-right.",
        settings: {
            contentBar: "Etterna",
            srText: "MSD",
            diffText: "Graph",
            debugUseAmount: false,
            estimatorAlgorithm: "Mixed",
            azusaSunnyReferenceHo: true,
            etternaVersion: "0.72.3",
            companellaEtternaVersion: "0.74.0",
            enablePauseDetection: true,
            pauseDetectionThreshold: "500",
            enableEtternaRainbowBars: false,
            enableStatusMarquee: true,
            VibroDetection: false,
            showModeTagCapsule: true,
            enableNumericDifficulty: true,
            cardVisibility: "Always",
            cardOpacity: "95%",
            cardRadius: "Medium",
            cardBgBlur: "4px",
            enableUpdateCheck: true,
            enableResultCache: true,
            reverseCardExtendDirection: false,
            useOsuFont: true,
            enableOsuTheme: true,
            enableFloatingTriangles: true,
            enableCoverArt: true,
            customBackgroundColor: "#000000",
            useSvDetection: false,
            forceSunnyWindow: false,
            enableLNDifficulty: false,
            enableAnalyzeLN: false,
            enableAlwaysShowLNDifficulty: true,
            display6kLevel: true,
            extendedEstimationRange: false,
        },
    },
    {
        id: "jack-player",
        name: "Jack Player",
        description: "Difficulty graph, MSD on the left, estimated difficulty at top-right.",
        settings: {
            contentBar: "Graph",
            srText: "MSD",
            diffText: "Difficulty",
            debugUseAmount: false,
            estimatorAlgorithm: "Mixed",
            azusaSunnyReferenceHo: true,
            etternaVersion: "0.72.3",
            companellaEtternaVersion: "0.74.0",
            enablePauseDetection: true,
            pauseDetectionThreshold: "500",
            enableEtternaRainbowBars: false,
            enableStatusMarquee: true,
            VibroDetection: false,
            showModeTagCapsule: true,
            enableNumericDifficulty: true,
            cardVisibility: "Always",
            cardOpacity: "95%",
            cardRadius: "Medium",
            cardBgBlur: "4px",
            enableUpdateCheck: true,
            enableResultCache: true,
            reverseCardExtendDirection: false,
            useOsuFont: true,
            enableOsuTheme: true,
            enableFloatingTriangles: true,
            enableCoverArt: true,
            customBackgroundColor: "#000000",
            useSvDetection: false,
            forceSunnyWindow: true,
            enableLNDifficulty: true,
            enableAnalyzeLN: true,
            enableAlwaysShowLNDifficulty: true,
            display6kLevel: true,
            extendedEstimationRange: false,
        },
    },
    {
        id: "the-limit-does-not-exist",
        name: "The Limit Does Not Exist",
        description: "Difficulty graph, MSD, Sunny estimation with extended range.",
        settings: {
            contentBar: "Graph",
            srText: "MSD",
            diffText: "Difficulty",
            debugUseAmount: false,
            estimatorAlgorithm: "Sunny",
            azusaSunnyReferenceHo: true,
            etternaVersion: "0.72.3",
            companellaEtternaVersion: "0.74.0",
            enablePauseDetection: true,
            pauseDetectionThreshold: "500",
            enableEtternaRainbowBars: false,
            enableStatusMarquee: true,
            VibroDetection: false,
            showModeTagCapsule: true,
            enableNumericDifficulty: true,
            cardVisibility: "Always",
            cardOpacity: "95%",
            cardRadius: "Medium",
            cardBgBlur: "4px",
            enableUpdateCheck: true,
            enableResultCache: true,
            reverseCardExtendDirection: false,
            useOsuFont: true,
            enableOsuTheme: true,
            enableFloatingTriangles: true,
            enableCoverArt: true,
            customBackgroundColor: "#000000",
            useSvDetection: false,
            forceSunnyWindow: true,
            enableLNDifficulty: true,
            enableAnalyzeLN: true,
            enableAlwaysShowLNDifficulty: true,
            display6kLevel: true,
            extendedEstimationRange: true,
        },
    },
    {
        id: "daniel-like",
        name: "Daniel-like",
        description: "Difficulty graph, MSD, Daniel estimator.",
        settings: {
            contentBar: "Graph",
            srText: "MSD",
            diffText: "Difficulty",
            debugUseAmount: false,
            estimatorAlgorithm: "Daniel",
            azusaSunnyReferenceHo: true,
            etternaVersion: "0.72.3",
            companellaEtternaVersion: "0.74.0",
            enablePauseDetection: true,
            pauseDetectionThreshold: "500",
            enableEtternaRainbowBars: false,
            enableStatusMarquee: true,
            VibroDetection: false,
            showModeTagCapsule: true,
            enableNumericDifficulty: true,
            cardVisibility: "Always",
            cardOpacity: "95%",
            cardRadius: "Medium",
            cardBgBlur: "4px",
            enableUpdateCheck: true,
            enableResultCache: true,
            reverseCardExtendDirection: false,
            useOsuFont: true,
            enableOsuTheme: true,
            enableFloatingTriangles: true,
            enableCoverArt: true,
            customBackgroundColor: "#000000",
            useSvDetection: false,
            forceSunnyWindow: false,
            enableLNDifficulty: true,
            enableAnalyzeLN: false,
            enableAlwaysShowLNDifficulty: true,
            display6kLevel: true,
            extendedEstimationRange: false,
        },
    },
    {
        id: "tyrcs-wild-dan",
        name: "Wild Dan (WIP)",
        description: "Difficulty graph, ReworkSR, Sunny estimation with extended range.",
        settings: {
            contentBar: "Graph",
            srText: "ReworkSR",
            diffText: "Difficulty",
            debugUseAmount: false,
            estimatorAlgorithm: "Sunny",
            azusaSunnyReferenceHo: true,
            etternaVersion: "0.72.3",
            companellaEtternaVersion: "0.74.0",
            enablePauseDetection: true,
            pauseDetectionThreshold: "500",
            enableEtternaRainbowBars: false,
            enableStatusMarquee: true,
            VibroDetection: false,
            showModeTagCapsule: true,
            enableNumericDifficulty: true,
            cardVisibility: "Always",
            cardOpacity: "95%",
            cardRadius: "Medium",
            cardBgBlur: "4px",
            enableUpdateCheck: true,
            enableResultCache: true,
            reverseCardExtendDirection: false,
            useOsuFont: true,
            enableOsuTheme: true,
            enableFloatingTriangles: true,
            enableCoverArt: true,
            customBackgroundColor: "#000000",
            useSvDetection: false,
            forceSunnyWindow: true,
            enableLNDifficulty: true,
            enableAnalyzeLN: true,
            enableAlwaysShowLNDifficulty: true,
            display6kLevel: true,
            extendedEstimationRange: true,
        },
    },
];

// Every schema key a preset snapshot covers, mapped to its apply function.
// Keep in sync with applySettingsFrom() in settings.js.
const PRESET_APPLIERS = {
    contentBar: applyContentBarSetting,
    srText: applySrTextSetting,
    diffText: applyDiffTextSetting,
    debugUseAmount: applyDebugUseAmountSetting,
    estimatorAlgorithm: applyEstimatorAlgorithmSetting,
    azusaSunnyReferenceHo: applyAzusaSunnyReferenceHoSetting,
    etternaVersion: applyEtternaVersionSetting,
    companellaEtternaVersion: applyCompanellaEtternaVersionSetting,
    enablePauseDetection: applyPauseDetectionSetting,
    pauseDetectionThreshold: applyPauseDetectionThresholdSetting,
    enableEtternaRainbowBars: applyEnableEtternaRainbowBarsSetting,
    enableStatusMarquee: applyEnableStatusMarqueeSetting,
    VibroDetection: applyVibroDetectionSetting,
    showModeTagCapsule: applyShowModeTagCapsuleSetting,
    enableNumericDifficulty: applyEnableNumericDifficultySetting,
    cardVisibility: applyCardVisibilitySetting,
    cardOpacity: applyCardOpacitySetting,
    cardRadius: applyCardRadiusSetting,
    cardBgBlur: applyCardBgBlurSetting,
    enableUpdateCheck: applyEnableUpdateCheckSetting,
    enableResultCache: applyEnableResultCacheSetting,
    reverseCardExtendDirection: applyReverseCardExtendDirectionSetting,
    useOsuFont: applyUseOsuFontSetting,
    enableOsuTheme: applyEnableOsuThemeSetting,
    enableFloatingTriangles: applyEnableFloatingTrianglesSetting,
    enableCoverArt: applyEnableCoverArtSetting,
    customBackgroundColor: applyCustomBackgroundColorSetting,
    useSvDetection: applyUseSvDetectionSetting,
    forceSunnyWindow: applyForceSunnyWindowSetting,
    enableLNDifficulty: applyEnableLNDifficultySetting,
    enableAnalyzeLN: applyEnableAnalyzeLNSetting,
    enableAlwaysShowLNDifficulty: applyEnableAlwaysShowLNDifficultySetting,
    display6kLevel: applyDisplay6kLevelSetting,
    extendedEstimationRange: applyExtendedEstimationRangeSetting,
    wsEndpoint: applyWsEndpointSetting,
};

// The same keys mapped to a getter that reads the CURRENT user value from state.
const PRESET_STATE_GETTERS = {
    contentBar: () => state.userContentBar,
    srText: () => state.userSrText,
    diffText: () => state.userDiffText,
    debugUseAmount: () => state.debugUseAmount,
    estimatorAlgorithm: () => state.estimatorAlgorithm,
    azusaSunnyReferenceHo: () => state.azusaSunnyReferenceHo,
    etternaVersion: () => state.etternaVersion,
    companellaEtternaVersion: () => state.companellaEtternaVersion,
    enablePauseDetection: () => state.pauseDetectionEnabled,
    pauseDetectionThreshold: () => String(state.pauseDetectionThresholdMs),
    enableEtternaRainbowBars: () => state.enableEtternaRainbowBars,
    enableStatusMarquee: () => state.enableStatusMarquee,
    VibroDetection: () => state.vibroDetection,
    showModeTagCapsule: () => state.showModeTagCapsule,
    enableNumericDifficulty: () => state.enableNumericDifficulty,
    cardVisibility: () => state.cardVisibility,
    cardOpacity: () => state.cardOpacity,
    cardRadius: () => state.cardRadius,
    cardBgBlur: () => state.cardBgBlur,
    enableUpdateCheck: () => state.enableUpdateCheck,
    enableResultCache: () => state.enableResultCache,
    reverseCardExtendDirection: () => state.reverseCardExtendDirection,
    useOsuFont: () => state.useOsuFont,
    enableOsuTheme: () => state.enableOsuTheme,
    enableFloatingTriangles: () => state.enableFloatingTriangles,
    enableCoverArt: () => state.enableCoverArt,
    customBackgroundColor: () => state.customBackgroundColor,
    useSvDetection: () => state.useSvDetection,
    forceSunnyWindow: () => state.forceSunnyWindow,
    enableLNDifficulty: () => state.enableLNDifficulty,
    enableAnalyzeLN: () => state.enableAnalyzeLN,
    enableAlwaysShowLNDifficulty: () => state.enableAlwaysShowLNDifficulty,
    display6kLevel: () => state.display6kLevel,
    extendedEstimationRange: () => state.extendedEstimationRange,
    wsEndpoint: () => state.wsEndpoint,
};

// Keep in sync with the recomputeNeeded key set in settings.js.
const RECOMPUTE_KEYS = new Set([
    "contentBar", "srText", "debugUseAmount", "diffText",
    "estimatorAlgorithm", "azusaSunnyReferenceHo", "etternaVersion",
    "companellaEtternaVersion", "enablePauseDetection", "pauseDetectionThreshold",
    "enableEtternaRainbowBars", "vibroDetection", "showModeTagCapsule",
    "useSvDetection", "forceSunnyWindow", "enableLNDifficulty",
    "enableAnalyzeLN", "enableAlwaysShowLNDifficulty", "display6kLevel",
    "extendedEstimationRange",
]);

// Keep in sync with the result-cache invalidation key set in settings.js.
const CACHE_KEYS = new Set([
    "estimatorAlgorithm", "azusaSunnyReferenceHo", "etternaVersion",
    "companellaEtternaVersion", "debugUseAmount", "useSvDetection",
    "vibroDetection", "wsEndpoint", "forceSunnyWindow", "enableLNDifficulty",
    "enableAnalyzeLN", "enableAlwaysShowLNDifficulty", "display6kLevel",
    "extendedEstimationRange",
]);

let customPresets = [];
let currentPreset = "Default";
let lastValues = null;
let initialized = false;
let managerRootEl = null;
let managerBodyEl = null;
let managerSaveInputEl = null;
let managerHintEl = null;

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

function readStorageValue(key) {
    try {
        return window.localStorage.getItem(key);
    } catch {
        return null;
    }
}

function writeStorageValue(key, value) {
    try {
        window.localStorage.setItem(key, value);
    } catch {
        // Ignore storage failures and keep the runtime working.
    }
}

function loadCustomPresets() {
    try {
        const raw = readStorageValue(CUSTOM_PRESETS_KEY);
        if (!raw) {
            return [];
        }
        const parsed = JSON.parse(raw);
        if (!Array.isArray(parsed)) {
            return [];
        }
        return parsed.filter(
            (preset) => preset
                && typeof preset.id === "string"
                && typeof preset.name === "string"
                && preset.name.trim().length > 0
                && preset.settings && typeof preset.settings === "object",
        );
    } catch {
        return [];
    }
}

function persistCustomPresets() {
    writeStorageValue(CUSTOM_PRESETS_KEY, JSON.stringify(customPresets));
}

function persistActivePreset() {
    writeStorageValue(ACTIVE_PRESET_KEY, currentPreset);
}

function loadActivePreset() {
    try {
        const raw = readStorageValue(ACTIVE_PRESET_KEY);
        if (typeof raw === "string" && raw.trim()) {
            return raw.trim();
        }
    } catch {
        // ignore
    }
    return "Default";
}

// ---------------------------------------------------------------------------
// Snapshot helpers
// ---------------------------------------------------------------------------

function buildDefaultSnapshot() {
    const defaults = APP_CONFIG.defaults;
    return {
        contentBar: defaults.contentBar,
        srText: defaults.srText,
        diffText: defaults.diffText,
        debugUseAmount: defaults.debugUseAmount,
        estimatorAlgorithm: defaults.estimatorAlgorithm,
        azusaSunnyReferenceHo: defaults.azusaSunnyReferenceHo,
        etternaVersion: defaults.etternaVersion,
        companellaEtternaVersion: defaults.companellaEtternaVersion,
        enablePauseDetection: defaults.pauseDetectionEnabled,
        pauseDetectionThreshold: String(defaults.pauseDetectionThresholdMs),
        enableEtternaRainbowBars: defaults.enableEtternaRainbowBars,
        enableStatusMarquee: defaults.enableStatusMarquee,
        VibroDetection: defaults.vibroDetection,
        showModeTagCapsule: defaults.showModeTagCapsule,
        enableNumericDifficulty: defaults.enableNumericDifficulty,
        cardVisibility: defaults.cardVisibility,
        cardOpacity: defaults.cardOpacity,
        cardRadius: defaults.cardRadius,
        cardBgBlur: defaults.cardBgBlur,
        enableUpdateCheck: defaults.enableUpdateCheck,
        enableResultCache: defaults.enableResultCache,
        reverseCardExtendDirection: defaults.reverseCardExtendDirection,
        useOsuFont: defaults.useOsuFont,
        enableOsuTheme: defaults.enableOsuTheme,
        enableFloatingTriangles: defaults.enableFloatingTriangles,
        enableCoverArt: defaults.enableCoverArt,
        customBackgroundColor: defaults.customBackgroundColor,
        useSvDetection: defaults.useSvDetection,
        forceSunnyWindow: defaults.forceSunnyWindow,
        enableLNDifficulty: defaults.enableLNDifficulty,
        enableAnalyzeLN: defaults.enableAnalyzeLN,
        enableAlwaysShowLNDifficulty: defaults.enableAlwaysShowLNDifficulty,
        display6kLevel: defaults.display6kLevel,
        extendedEstimationRange: defaults.extendedEstimationRange,
        // wsEndpoint is intentionally NOT part of built-in preset snapshots:
        // it is a connection parameter (e.g. a LAN address for other devices),
        // so applying a preset must never drop or change the socket connection.
        // Custom presets still capture it via PRESET_STATE_GETTERS.
    };
}

function resolveSnapshot(preset) {
    // Built-in presets are "defaults + overrides"; custom presets already hold
    // a full snapshot. Merging over defaults covers both (and heals missing
    // keys from older custom presets).
    return { ...buildDefaultSnapshot(), ...preset.settings };
}

function applySnapshot(snapshot) {
    let anyChanged = false;
    let recomputeNeeded = false;
    let cacheNeeded = false;

    for (const [key, value] of Object.entries(snapshot)) {
        const applier = PRESET_APPLIERS[key];
        if (!applier) {
            continue;
        }
        const changed = applier(value);
        if (changed) {
            anyChanged = true;
            if (RECOMPUTE_KEYS.has(key)) {
                recomputeNeeded = true;
            }
            if (CACHE_KEYS.has(key)) {
                cacheNeeded = true;
            }
        }
    }

    if (cacheNeeded) {
        clearResultCache();
    }
    if (recomputeNeeded) {
        scheduleRecompute("preset applied", true);
    }

    return anyChanged;
}

function writeBackToTosu(presetName, snapshot) {
    // Only the browser page (127.0.0.1) writes back. The in-game overlays load
    // from localhost — a DIFFERENT origin that shares no localStorage with the
    // browser page — so their write-back dedup records are invisible to the
    // browser page. If they wrote back, a lagging overlay would misjudge a
    // preset-apply echo as a manual edit, auto-save to "Last Saved Preset" /
    // its anchored preset and jump every page's picker. Overlays stay read-only.
    if (window.location.hostname !== "127.0.0.1") {
        return;
    }

    const folderName = typeof window.COUNTER_PATH === "string"
        ? window.COUNTER_PATH.trim()
        : "";
    if (!folderName) {
        return;
    }

    // Same shape the dashboard POSTs: [{ uniqueID, value }, ...]. Only keys
    // present in the snapshot are sent (tosu merges, never replaces), so
    // built-in presets leave wsEndpoint untouched.
    const values = Object.keys(snapshot).map((key) => ({
        uniqueID: key,
        value: snapshot[key],
    }));
    values.push({ uniqueID: "preset", value: presetName });

    fetch(`/api/counters/settings/${encodeURIComponent(folderName)}`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(values),
    }).catch(() => {
        // Write-back is a best-effort sync; preset application still worked.
    });
}

/** Captures the currently applied user settings as a full snapshot. */
export function captureCurrentSettings() {
    const snapshot = {};
    for (const key of Object.keys(PRESET_STATE_GETTERS)) {
        snapshot[key] = PRESET_STATE_GETTERS[key]();
    }
    return snapshot;
}

// ---------------------------------------------------------------------------
// Preset lookup / application
// ---------------------------------------------------------------------------

function findBuiltinPresetByName(name) {
    return PRESET_DEFS.find((preset) => preset.name === name) || null;
}

function findPresetByName(name) {
    return findBuiltinPresetByName(name)
        || customPresets.find((preset) => preset.name === name)
        || null;
}

/**
 * Applies a preset by name (built-in or user-defined) and syncs the resulting
 * configuration back to tosu. Unknown names are lazily materialized ONLY for
 * the default "Custom N" slots; any other unknown name is a no-op.
 *
 * @returns {boolean} true when a preset was applied.
 */
export function applyPresetByName(name) {
    let preset = findPresetByName(name);
    if (!preset && DEFAULT_SLOT_NAMES.includes(name)) {
        createCustomPreset(name, captureCurrentSettings());
        preset = findPresetByName(name);
    }
    if (!preset) {
        return false;
    }

    const snapshot = resolveSnapshot(preset);
    applySnapshot(snapshot);
    currentPreset = name;
    persistActivePreset();
    if (shouldWriteBack(snapshot, name)) {
        writeBackToTosu(name, snapshot);
        markWritten(snapshot, name);
    }
    // Mirror the write-back into lastValues so the echo broadcast of the same
    // values is not mistaken for a manual settings change (no auto-save loop).
    // Spread the previous lastValues first so keys absent from built-in
    // snapshots (e.g. wsEndpoint) keep their last known value.
    lastValues = { ...lastValues, ...snapshot, preset: name };
    renderPresetManager();
    return true;
}

/** Returns the name of the currently active preset ("Default" when none). */
/**
 * Auto-save the current configuration after a dashboard settings change:
 *  - if a custom preset is anchored (currentPreset names one), update it and
 *    keep the anchor;
 *  - otherwise follow changes in the fixed "Auto" container and move the
 *    dashboard preset picker to "Auto".
 * Both paths sync the snapshot (plus the preset picker value) back to tosu.
 */
export function autoSaveCurrentPreset() {
    // The broadcast payload (lastValues) is the ONLY source: every page of
    // this origin receives the same values, so snapshots built from it are
    // identical across pages. Merging the live state here would let each page
    // fill payload gaps with its own (possibly preset-tainted) state values,
    // producing divergent write-backs and an endless write-back loop.
    const snapshot = { ...lastValues };

    const anchored = customPresets.find((preset) => preset.name === currentPreset);
    if (anchored) {
        anchored.settings = snapshot;
        anchored.updatedAt = Date.now();
        persistCustomPresets();
        renderPresetManager();
        if (!recentlyWritten() && shouldWriteBack(snapshot, anchored.name)) {
            writeBackToTosu(anchored.name, snapshot);
            markWritten(snapshot, anchored.name);
        }
        lastValues = { ...lastValues, ...snapshot, preset: anchored.name };
        return;
    }

    // Not anchored to a custom preset: the change belongs to Last Saved Preset.
    saveToLastSavedPreset();
}

/**
 * Overwrites ONLY the "Last Saved Preset" container with the given snapshot
 * and moves the picker there. Used when the picker moved to a built-in preset
 * (or Auto) with a manual change — the edit belongs to Last Saved Preset,
 * never to whatever custom preset happened to be anchored before.
 */
function saveToLastSavedPreset() {
    const snapshot = { ...lastValues };
    updateAutoContainer(snapshot);
    persistCustomPresets();
    currentPreset = AUTO_SAVE_PRESET_NAME;
    persistActivePreset();
    renderPresetManager();
    if (!recentlyWritten() && shouldWriteBack(snapshot, AUTO_SAVE_PRESET_NAME)) {
        writeBackToTosu(AUTO_SAVE_PRESET_NAME, snapshot);
        markWritten(snapshot, AUTO_SAVE_PRESET_NAME);
    }
    lastValues = { ...lastValues, ...snapshot, preset: AUTO_SAVE_PRESET_NAME };
}

/**
 * Creates or updates the fixed "Last Saved Preset" container in memory.
 * Callers decide when to persist and write back.
 */
function updateAutoContainer(snapshot) {
    const auto = customPresets.find((preset) => preset.name === AUTO_SAVE_PRESET_NAME);
    if (auto) {
        auto.settings = snapshot;
        auto.updatedAt = Date.now();
        return auto;
    }
    const created = {
        id: `auto-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`,
        name: AUTO_SAVE_PRESET_NAME,
        settings: snapshot,
        createdAt: Date.now(),
    };
    customPresets.push(created);
    return created;
}

// ---------------------------------------------------------------------------
// Custom preset CRUD
// ---------------------------------------------------------------------------

/** Creates or updates (same-name overwrite) a user preset from a snapshot. */
export function createCustomPreset(name, snapshot) {
    const cleanName = String(name || "").trim();
    if (!cleanName || cleanName === "Custom" || cleanName === AUTO_SAVE_PRESET_NAME) {
        return null;
    }
    if (findBuiltinPresetByName(cleanName)) {
        return null;
    }

    // Same name => overwrite the existing snapshot (saving is replacing).
    const existing = customPresets.find((preset) => preset.name === cleanName);
    if (existing) {
        existing.settings = snapshot || {};
        existing.updatedAt = Date.now();
        persistCustomPresets();
        renderPresetManager();
        return existing;
    }

    // The "Auto" container does not count towards the user preset cap.
    // No hard limit: presets live in localStorage and are small, so users can
    // create as many as they want (browser storage is the only ceiling).
    const preset = {
        id: `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`,
        name: cleanName,
        settings: snapshot || {},
        createdAt: Date.now(),
    };
    customPresets.push(preset);
    persistCustomPresets();
    renderPresetManager();
    return preset;
}

/** Renames a user preset by id. Returns true on success. */
export function renameCustomPreset(id, newName) {
    const cleanName = String(newName || "").trim();
    if (!cleanName || cleanName === "Custom" || cleanName === AUTO_SAVE_PRESET_NAME) {
        return false;
    }
    const preset = customPresets.find((item) => item.id === id);
    if (!preset) {
        return false;
    }
    if (findBuiltinPresetByName(cleanName)) {
        return false;
    }
    if (customPresets.some((item) => item.id !== id && item.name === cleanName)) {
        return false;
    }

    preset.name = cleanName;
    persistCustomPresets();
    renderPresetManager();
    return true;
}

/** Deletes a user preset by id. Returns true on success. */
export function deleteCustomPreset(id) {
    const index = customPresets.findIndex((item) => item.id === id);
    if (index === -1) {
        return false;
    }
    // Fixed anchor slots cannot be deleted.
    if (DEFAULT_SLOT_NAMES.includes(customPresets[index].name)) {
        return false;
    }
    customPresets.splice(index, 1);
    persistCustomPresets();
    renderPresetManager();
    return true;
}

/** Ensures the default "Custom 1..N" anchor slots exist. */
function ensureDefaultCustomSlots() {
    const snapshot = captureCurrentSettings();
    for (const name of DEFAULT_SLOT_NAMES) {
        const existing = customPresets.some((preset) => preset.name === name);
        if (!existing && createCustomPreset(name, snapshot) === null) {
            break; // cap reached or name rejected — stop trying the rest
        }
    }
}

// ---------------------------------------------------------------------------
// tosu settings stream (own /websocket/commands connection)
// ---------------------------------------------------------------------------

function extractSettingsPayload(packet) {
    if (Array.isArray(packet)) {
        return packet;
    }
    if (packet && typeof packet === "object" && packet.command === "getSettings") {
        return packet.message;
    }
    return null;
}

function extractPresetValue(payload) {
    if (Array.isArray(payload)) {
        const item = payload.find((entry) => entry?.uniqueID === "preset");
        return typeof item?.value === "string" && item.value.trim() ? item.value.trim() : null;
    }
    if (payload && typeof payload === "object") {
        const value = payload.preset;
        return typeof value === "string" && value.trim() ? value.trim() : null;
    }
    return null;
}

function snapshotOf(payload) {
    if (Array.isArray(payload)) {
        const out = {};
        for (const entry of payload) {
            if (entry && typeof entry.uniqueID === "string") {
                out[entry.uniqueID] = entry.value;
            }
        }
        return out;
    }
    return { ...(payload || {}) };
}

function hasKeyChanged(prev, next, key) {
    return Object.prototype.hasOwnProperty.call(next, key)
        && next[key] !== prev[key];
}

/**
 * Settings stream handler:
 *  - first batch: record baseline, restore/apply the stored preset picker;
 *  - preset picker change: apply (or mark Auto);
 *  - any other settings change: auto-save into the anchored custom preset or
 *    the Auto container (write-back echoes the same values -> no loop).
 */
/** Applies every payload key present in the settings schema to the live state. */
function applyPayloadToState() {
    for (const [key, applyFn] of Object.entries(PRESET_APPLIERS)) {
        if (Object.prototype.hasOwnProperty.call(lastValues, key) && lastValues[key] !== undefined) {
            try {
                applyFn(lastValues[key]);
            } catch {
                // Ignore per-key apply failures; the settings.js listener
                // will still apply valid keys from the same broadcast.
            }
        }
    }
}

/**
 * Write-back dedup shared across ALL pages of this origin via localStorage.
 * The shared record is re-read on EVERY check (no in-memory cache — a stale
 * cache let pages miss each other's latest write and feed echo loops), plus a
 * short per-preset throttle that collapses bursts of identical write-backs
 * from many open pages (browser overlay, ingame overlays, OBS sources).
 */
const LAST_WRITTEN_KEY = "mma.presets.lastWritten.v1";
const WRITE_BACK_THROTTLE_MS = 1500;
const LAST_WRITTEN_DEPTH = 3;

function readLastWritten() {
    try {
        const raw = window.localStorage.getItem(LAST_WRITTEN_KEY);
        if (!raw) {
            return null;
        }
        const parsed = JSON.parse(raw);
        if (Array.isArray(parsed)) {
            return parsed;
        }
        // Legacy single-record format.
        return [parsed];
    } catch {
        return null;
    }
}

function shouldWriteBack(snapshot, presetName) {
    const list = readLastWritten();
    const last = list && list[0];
    if (!last || last.presetName !== presetName) {
        return true;
    }
    for (const key of Object.keys(PRESET_APPLIERS)) {
        if (snapshot[key] !== last.snapshot[key]) {
            return true;
        }
    }
    return false;
}

/**
 * True when ANY preset was written back very recently (by any page). Used to
 * throttle AUTO-SAVE write-backs only: a second page whose snapshot lags can
 * misjudge a preset-apply echo as a manual edit and write back "Last Saved
 * Preset", jumping every open page's picker. 1.5s is far shorter than real
 * user interactions, so legit auto-saves pass. Explicit writes (preset apply,
 * user edits) are NOT throttled — skipping them would leave tosu's values.json
 * stale and cause exactly the "picker jumps back" bug.
 */
function recentlyWritten() {
    const list = readLastWritten();
    return Boolean(list && list.some((r) => Date.now() - r.t < WRITE_BACK_THROTTLE_MS));
}

function markWritten(snapshot, presetName) {
    const list = readLastWritten() || [];
    list.unshift({ presetName, snapshot: { ...snapshot }, t: Date.now() });
    if (list.length > LAST_WRITTEN_DEPTH) {
        list.length = LAST_WRITTEN_DEPTH;
    }
    try {
        window.localStorage.setItem(LAST_WRITTEN_KEY, JSON.stringify(list));
    } catch {
        // Storage failure only costs us cross-page dedup; keep going.
    }
}

/** Updates (or creates) the fixed "Auto" container with a snapshot. */
function syncAutoPreset(snapshot) {
    updateAutoContainer(snapshot);
}

/**
 * User edited settings with a custom preset selected: overwrite that preset
 * AND keep Auto in sync; the picker stays on the custom preset (the editing
 * target). Snapshot comes from the broadcast payload ONLY (single source of
 * truth) so all open pages write identical content.
 */
function overwriteCustomPreset(presetValue) {
    applyPayloadToState();
    const snapshot = { ...lastValues };
    const target = customPresets.find((preset) => preset.name === presetValue);
    if (target) {
        target.settings = snapshot;
        target.updatedAt = Date.now();
    }
    syncAutoPreset(snapshot);
    persistCustomPresets();
    currentPreset = presetValue;
    persistActivePreset();
    renderPresetManager();
    if (!recentlyWritten() && shouldWriteBack(snapshot, presetValue)) {
        writeBackToTosu(presetValue, snapshot);
        markWritten(snapshot, presetValue);
    }
    lastValues = { ...lastValues, ...snapshot, preset: presetValue };
}

function handleSettingsPacket(packet) {
    const payload = extractSettingsPayload(packet);
    if (!payload) {
        return;
    }

    const presetValue = extractPresetValue(payload);

    if (lastValues === null) {
        lastValues = snapshotOf(payload);
        if (presetValue && presetValue !== currentPreset) {
            if (presetValue === "Default" || !applyPresetByName(presetValue)) {
                // "Custom" (or any unresolvable value) means "no preset": the
                // current manual configuration stays, anchored to nothing.
                currentPreset = "Default";
                persistActivePreset();
                renderPresetManager();
            }
        }
        return;
    }

    const prev = lastValues;
    lastValues = snapshotOf(payload);

    // True when the user actually changed settings in the dashboard (the
    // broadcast differs from the page's last known snapshot). Used to decide
    // between "use this preset" (no change) and "overwrite with my changes".
    // wsEndpoint is excluded: tosu always includes it in every broadcast and
    // it is not part of preset snapshots, so it would otherwise produce a
    // false "manual change" on every preset write-back echo.
    const hasManualChange = Object.keys(PRESET_APPLIERS)
        .filter((key) => key !== "wsEndpoint")
        .some((key) => hasKeyChanged(prev, lastValues, key));

    // Echo broadcast: the payload matches one of the recent write-backs (by
    // this page or another). A delayed echo of an earlier Apply can arrive
    // AFTER the user applied a different preset — without this guard it would
    // be treated as "switch back to that preset" and jump the picker. The
    // write-back history is a short queue, because the latest entry may belong
    // to a different preset by the time the delayed echo arrives.
    // wsEndpoint is excluded (it appears in every broadcast but never in
    // snapshots); keys absent from the written snapshot are ignored.
    const lastWrittenNow = readLastWritten();
    const isWriteBackEcho = Boolean(lastWrittenNow) && lastWrittenNow.some((record) =>
        record.presetName === presetValue
        && Object.keys(PRESET_APPLIERS)
            .filter((key) => key !== "wsEndpoint")
            .every((key) =>
                !(key in record.snapshot) || record.snapshot[key] === lastValues[key]));

    if (presetValue && presetValue !== currentPreset && !isWriteBackEcho) {
        // The preset picker moved to a different preset.
        if (presetValue === AUTO_SAVE_PRESET_NAME) {
            if (hasManualChange) {
                // User edited settings then picked Auto: overwrite Auto
                // (never the currently anchored custom preset).
                applyPayloadToState();
                saveToLastSavedPreset();
            } else {
                // Just switched the picker to Auto: follow mode, no overwrite.
                currentPreset = AUTO_SAVE_PRESET_NAME;
                persistActivePreset();
                renderPresetManager();
            }
            return;
        }

        const isCustom = customPresets.some((preset) => preset.name === presetValue);
        if (isCustom) {
            if (hasManualChange) {
                overwriteCustomPreset(presetValue);
            } else {
                // No edits: "use" the custom preset (apply its saved content).
                if (!applyPresetByName(presetValue)) {
                    currentPreset = "Default";
                    persistActivePreset();
                    renderPresetManager();
                }
            }
            return;
        }

        // Built-in (read-only) preset, including "Default".
        if (hasManualChange) {
            // User edited settings with a built-in preset selected: the edits
            // become the new Auto preset and the picker moves to Auto.
            // Deliberately NOT autoSaveCurrentPreset(): that would also update
            // the currently anchored custom preset (e.g. a lingering Custom 3)
            // with the broadcast payload and keep its anchor — jumping the
            // picker back to it. A picker change to a built-in preset always
            // means "edits go to Last Saved Preset".
            applyPayloadToState();
            saveToLastSavedPreset();
        } else {
            // No edits: "use" the built-in preset (apply its content).
            if (!applyPresetByName(presetValue)) {
                currentPreset = "Default";
                persistActivePreset();
                renderPresetManager();
            }
        }
        return;
    }

    // The picker stayed on the same preset: any change is an edit of whatever
    // is selected (Auto, a custom preset or a built-in one) -> auto-save.
    // Write-back echoes never count as edits.
    if (hasManualChange && !isWriteBackEcho) {
        applyPayloadToState();
        autoSaveCurrentPreset();
    }
}

// ---------------------------------------------------------------------------
// Manager UI (created dynamically; visible only with ?edit=1)
// ---------------------------------------------------------------------------

function isEditMode() {
    try {
        return new URLSearchParams(window.location.search).has("edit");
    } catch {
        return false;
    }
}

function injectStylesheet() {
    if (document.getElementById("preset-manager-style")) {
        return;
    }
    const link = document.createElement("link");
    link.id = "preset-manager-style";
    link.rel = "stylesheet";
    link.href = "./styles/presets.css";
    document.head.appendChild(link);
}

function ensureManagerDom() {
    if (managerRootEl) {
        return;
    }

    injectStylesheet();

    const root = document.createElement("aside");
    root.id = "preset-manager";
    root.className = "preset-manager";
    root.hidden = true;
    root.innerHTML = `
        <div class="preset-manager-header">
            <span class="preset-manager-title">Presets</span>
            <button id="preset-manager-close" class="preset-manager-close" type="button"
                    title="Hide preset manager" aria-label="Hide preset manager">&times;</button>
        </div>
        <div id="preset-manager-body" class="preset-manager-body"></div>
        <p id="preset-manager-hint" class="preset-manager-hint"></p>
        <div class="preset-manager-save">
            <input id="preset-save-name" class="preset-save-name" type="text"
                   placeholder="New preset name..." maxlength="40">
            <button id="preset-save-btn" class="preset-btn preset-save-btn" type="button">Save current</button>
        </div>
    `;
    document.body.appendChild(root);

    managerRootEl = root;
    managerBodyEl = root.querySelector("#preset-manager-body");
    managerSaveInputEl = root.querySelector("#preset-save-name");
    managerHintEl = root.querySelector("#preset-manager-hint");

    root.querySelector("#preset-manager-close").addEventListener("click", () => {
        root.hidden = true;
    });

    const saveCurrent = () => {
        const cleanName = String(managerSaveInputEl.value || "").trim();
        const existed = customPresets.some((preset) => preset.name === cleanName);
        const preset = createCustomPreset(cleanName, captureCurrentSettings());
        if (!preset) {
            showManagerHint("Invalid preset name.", true);
            return;
        }
        managerSaveInputEl.value = "";
        showManagerHint(
            existed ? `Preset "${preset.name}" updated.` : `Preset "${preset.name}" saved.`,
            false,
        );
    };
    root.querySelector("#preset-save-btn").addEventListener("click", saveCurrent);
    managerSaveInputEl.addEventListener("keydown", (event) => {
        if (event.key === "Enter") {
            saveCurrent();
        }
    });

    root.addEventListener("click", handleManagerClick);
}

function showManagerHint(message, isError) {
    if (!managerHintEl) {
        return;
    }
    managerHintEl.textContent = message;
    managerHintEl.classList.toggle("error", Boolean(isError));
}

function escapeHtml(value) {
    return String(value ?? "")
        .replace(/&/g, "&amp;")
        .replace(/</g, "&lt;")
        .replace(/>/g, "&gt;")
        .replace(/"/g, "&quot;")
        .replace(/'/g, "&#39;");
}

function buildPresetRow(preset, { isSystem, active, actions = isSystem ? "apply" : "all" }) {
    const row = document.createElement("div");
    row.className = `preset-item${active ? " active" : ""}`;
    row.dataset.presetName = preset.name;

    const name = escapeHtml(preset.name);
    const desc = escapeHtml(preset.description || "");
    const actionsHtml = actions !== "none"
        ? `<div class="preset-item-actions">
            <button type="button" class="preset-btn preset-btn-apply" data-action="apply">Apply</button>
            ${actions === "all"
                ? `<button type="button" class="preset-btn" data-action="rename">Rename</button>
                   <button type="button" class="preset-btn preset-btn-danger" data-action="delete">Delete</button>`
                : ""}
        </div>`
        : "";

    row.innerHTML = `
        <div class="preset-item-info">
            <div class="preset-item-name">${name}${isSystem ? '<span class="preset-item-badge">System</span>' : ""}</div>
            <div class="preset-item-desc"${preset.description ? ` title="${escapeHtml(preset.description)}"` : ""}>${desc}</div>
        </div>
        ${actionsHtml}
    `;
    return row;
}

function renderPresetManager() {
    if (!managerBodyEl) {
        return;
    }

    managerBodyEl.textContent = "";

    const activeName = currentPreset;

    // User presets come first.
    const customSection = document.createElement("div");
    customSection.className = "preset-section";
    customSection.textContent = "My Presets";
    managerBodyEl.appendChild(customSection);

    const userPresets = customPresets
        .filter((preset) => preset.name !== AUTO_SAVE_PRESET_NAME)
        // Presets promoted to built-in system presets are no longer shown as
        // user presets (the localStorage copy may still exist).
        .filter((preset) => !findBuiltinPresetByName(preset.name));

    if (userPresets.length === 0) {
        const empty = document.createElement("div");
        empty.className = "preset-empty";
        empty.textContent = "No custom presets yet. Save your current settings below.";
        managerBodyEl.appendChild(empty);
    } else {
        for (const preset of userPresets) {
            managerBodyEl.appendChild(buildPresetRow(preset, {
                isSystem: false,
                active: activeName === preset.name,
                // Fixed anchor slots (Custom 1/2/3) cannot be renamed/deleted;
                // only user-created named presets get the full action set.
                actions: DEFAULT_SLOT_NAMES.includes(preset.name) ? "apply" : "all",
            }));
        }
    }

    // System presets.
    const systemSection = document.createElement("div");
    systemSection.className = "preset-section";
    systemSection.textContent = "System";
    managerBodyEl.appendChild(systemSection);

    for (const preset of PRESET_DEFS) {
        managerBodyEl.appendChild(buildPresetRow(preset, {
            isSystem: true,
            active: activeName === preset.name,
        }));
    }

    // The system-managed "Last Saved Preset" container lives in System too
    // (read-only; it is maintained automatically).
    managerBodyEl.appendChild(buildPresetRow(
        {
            name: AUTO_SAVE_PRESET_NAME,
            description: "Automatically keeps the latest manual configuration after you change settings.",
        },
        { isSystem: true, active: activeName === AUTO_SAVE_PRESET_NAME, actions: "none" },
    ));
}

function startRename(row) {
    const nameEl = row.querySelector(".preset-item-name");
    if (!nameEl) {
        return;
    }
    const presetName = row.dataset.presetName;
    // Fixed anchor slots cannot be renamed.
    if (DEFAULT_SLOT_NAMES.includes(presetName)) {
        return;
    }
    const preset = customPresets.find((item) => item.name === presetName);
    if (!preset) {
        return;
    }

    const input = document.createElement("input");
    input.type = "text";
    input.className = "preset-rename-input";
    input.value = preset.name;
    input.maxLength = 40;

    const info = row.querySelector(".preset-item-info");
    if (!info) {
        return;
    }
    info.replaceChildren(input);

    const actions = row.querySelector(".preset-item-actions");
    if (!actions) {
        return;
    }
    actions.textContent = "";

    const confirmBtn = document.createElement("button");
    confirmBtn.type = "button";
    confirmBtn.className = "preset-btn";
    confirmBtn.textContent = "Save";
    confirmBtn.dataset.action = "rename-confirm";

    const cancelBtn = document.createElement("button");
    cancelBtn.type = "button";
    cancelBtn.className = "preset-btn";
    cancelBtn.textContent = "Cancel";
    cancelBtn.dataset.action = "rename-cancel";
    actions.appendChild(confirmBtn);
    actions.appendChild(cancelBtn);

    input.focus();
    input.select();
}

function finishRename(row) {
    const input = row.querySelector(".preset-rename-input");
    if (!input) {
        return;
    }
    const presetName = row.dataset.presetName;
    const preset = customPresets.find((item) => item.name === presetName);
    if (!preset) {
        return;
    }
    if (!renameCustomPreset(preset.id, input.value)) {
        showManagerHint("Invalid or duplicate name.", true);
        renderPresetManager();
        return;
    }
    showManagerHint("", false);
}

function handleManagerClick(event) {
    const actionBtn = event.target.closest("[data-action]");
    if (!actionBtn) {
        return;
    }
    const row = actionBtn.closest(".preset-item");
    if (!row) {
        return;
    }

    switch (actionBtn.dataset.action) {
        case "apply": {
            const name = row.dataset.presetName;
            if (name === AUTO_SAVE_PRESET_NAME) {
                // No-op: Auto is a follow-mode marker, not an applicable snapshot.
                showManagerHint("Auto keeps following your manual changes.", false);
                return;
            }
            if (applyPresetByName(name)) {
                showManagerHint(`Preset "${name}" applied and synced to tosu.`, false);
            } else {
                showManagerHint(`Preset "${name}" not found.`, true);
            }
            break;
        }
        case "rename":
            startRename(row);
            break;
        case "rename-confirm":
            finishRename(row);
            break;
        case "rename-cancel":
            renderPresetManager();
            break;
        case "delete": {
            const name = row.dataset.presetName;
            const preset = customPresets.find((item) => item.name === name);
            if (!preset) {
                return;
            }
            if (window.confirm(`Delete preset "${preset.name}"?`)) {
                deleteCustomPreset(preset.id);
                showManagerHint("Preset deleted.", false);
            }
            break;
        }
        default:
            break;
    }
}

// ---------------------------------------------------------------------------
// Init (self-contained — no main.js wiring needed)
// ---------------------------------------------------------------------------

function initPresets() {
    if (initialized) {
        return;
    }
    initialized = true;

    customPresets = loadCustomPresets();

    // Migrate the pre-rename "Auto" container to the new display name.
    const LEGACY_AUTO_NAME = "Auto";
    if (!customPresets.some((preset) => preset.name === AUTO_SAVE_PRESET_NAME)) {
        const legacy = customPresets.find((preset) => preset.name === LEGACY_AUTO_NAME);
        if (legacy) {
            legacy.name = AUTO_SAVE_PRESET_NAME;
            persistCustomPresets();
        }
    }

    currentPreset = loadActivePreset();
    if (currentPreset === LEGACY_AUTO_NAME) {
        currentPreset = AUTO_SAVE_PRESET_NAME;
        persistActivePreset();
    }
    ensureDefaultCustomSlots();

    // Observe the tosu settings stream on our own commands connection.
    socket.commands(handleSettingsPacket);

    // Cross-page sync: another page of this origin may add/rename/delete
    // presets or change the active one; refresh our in-memory copy + UI.
    window.addEventListener("storage", (event) => {
        if (event.key === CUSTOM_PRESETS_KEY || event.key === ACTIVE_PRESET_KEY || event.key === LAST_WRITTEN_KEY) {
            customPresets = loadCustomPresets();
            if (event.key === ACTIVE_PRESET_KEY) {
                currentPreset = loadActivePreset();
            }
            if (isEditMode() && managerBodyEl) {
                renderPresetManager();
            }
        }
    });

    if (isEditMode()) {
        ensureManagerDom();
        if (managerRootEl) {
            managerRootEl.hidden = false;
            renderPresetManager();
        }
    }
}

initPresets();
