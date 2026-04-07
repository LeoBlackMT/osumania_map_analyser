import {
    APP_CONFIG,
    bodyGraphWrapEl,
    dashboardEl,
    ettSkillBarsEl,
    getActiveContentBar,
    hasAnyGraphModeEnabled,
    mainCardEl,
    parseAutoModeValue,
    parseCardBlurValue,
    parseCardOpacityValue,
    parseCardRadiusValue,
    parseContentBarValue,
    parseDebugUseAmountValue,
    parseDiffTextValue,
    parseEnableEtternaRainbowBarsValue,
    parseEnableStatusMarqueeValue,
    parseEnablePauseDetectionValue,
    parseEstimatorAlgorithmValue,
    parseEtternaVersionValue,
    parseCompanellaEtternaVersionValue,
    parseEnableNumericDifficultyValue,
    parseHideCardDuringPlayValue,
    parseShowModeTagCapsuleValue,
    parseShowTitleIconValue,
    parseReverseCardExtendDirectionValue,
    parseSrTextValue,
    parseSvDetectionValue,
    parseVibroDetectionValue,
    parseWsEndpointValue,
    patternClustersEl,
    reworkStarEl,
    socket,
    state,
    SETTINGS_COMMAND_TIMEOUT_MS,
    titleIconEl,
} from "./appContext.js";
import {
    normalizeBooleanSetting,
    normalizeCardBlurValue,
    normalizeCardOpacityValue,
    normalizeCardRadiusValue,
    normalizeContentBarValue,
    normalizeDiffTextValue,
    normalizeEtternaVersionValue,
    normalizeEstimatorAlgorithmValue,
    normalizeWsEndpointValue,
    normalizeSrTextValue,
} from "../parser/settingsParser.js";
import {
    clearDiffGraph,
    redrawPauseMarkers,
    setGraphCursorVisible,
    updateDiffTextVisibility,
} from "./graph.js";
import {
    updateCardPlayVisibility,
    updateModeTagVisibility,
    updatePauseCountVisibility,
    refreshStatusRendering,
} from "./hud.js";
import { resolveAutoDisplayProfile } from "./modeLogic.js";
import { scheduleRecompute } from "./scheduler.js";

function isAutoDisplayEnabled() {
    return state.userSrText === "Auto" || state.userContentBar === "Auto";
}

function resolveRuntimeDisplayProfile(modeTag = state.currentModeTag || "Mix") {
    const auto = resolveAutoDisplayProfile(modeTag);
    return {
        contentBar: state.userContentBar === "Auto" ? auto.contentBar : state.userContentBar,
        srText: state.userSrText === "Auto" ? auto.srText : state.userSrText,
        diffText: state.userDiffText,
    };
}

function updateContentBarVisibility() {
    const activeContentBar = getActiveContentBar();

    patternClustersEl.hidden = activeContentBar !== "Pattern";
    ettSkillBarsEl.hidden = activeContentBar !== "Etterna";
    if (bodyGraphWrapEl) {
        bodyGraphWrapEl.hidden = activeContentBar !== "Graph";
    }

    mainCardEl.classList.toggle("bars-pattern", activeContentBar === "Pattern");
    mainCardEl.classList.toggle("bars-etterna", activeContentBar === "Etterna");
    mainCardEl.classList.toggle("bars-graph", activeContentBar === "Graph");
    mainCardEl.classList.toggle("bars-none", activeContentBar === "None");

    if (activeContentBar !== "Etterna") {
        mainCardEl.classList.remove("bars-etterna-compact");
    }
}

let cardHeightTransitionTimerId = 0;
let cardHeightTransitionEndHandler = null;

function clearCardHeightTransitionState() {
    if (!mainCardEl) {
        return;
    }

    if (cardHeightTransitionTimerId) {
        clearTimeout(cardHeightTransitionTimerId);
        cardHeightTransitionTimerId = 0;
    }

    if (cardHeightTransitionEndHandler) {
        mainCardEl.removeEventListener("transitionend", cardHeightTransitionEndHandler);
        cardHeightTransitionEndHandler = null;
    }

    mainCardEl.style.removeProperty("height");
}

function animateCardHeightTransition(previousHeight) {
    if (!mainCardEl) {
        clearCardHeightTransitionState();
        return;
    }

    if (typeof window !== "undefined"
        && typeof window.matchMedia === "function"
        && window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
        clearCardHeightTransitionState();
        return;
    }

    const fromHeight = Number(previousHeight);
    const toHeight = Number(mainCardEl.getBoundingClientRect().height) || 0;
    if (!Number.isFinite(fromHeight) || !Number.isFinite(toHeight)) {
        clearCardHeightTransitionState();
        return;
    }

    if (Math.abs(toHeight - fromHeight) < 1) {
        clearCardHeightTransitionState();
        return;
    }

    clearCardHeightTransitionState();
    mainCardEl.style.height = `${fromHeight}px`;
    void mainCardEl.offsetHeight;
    mainCardEl.style.height = `${toHeight}px`;

    const cleanup = () => {
        clearCardHeightTransitionState();
    };

    cardHeightTransitionEndHandler = (event) => {
        if (event.target !== mainCardEl || event.propertyName !== "height") {
            return;
        }
        cleanup();
    };

    mainCardEl.addEventListener("transitionend", cardHeightTransitionEndHandler);
    cardHeightTransitionTimerId = setTimeout(cleanup, 420);
}

function applyVisualStyleSettings() {
    const opacityMap = {
        "100%": "1",
        "95%": "0.95",
        "90%": "0.9",
        "80%": "0.8",
        "70%": "0.7",
    };
    const blurMap = {
        Off: "0px",
        Soft: "6px",
        Strong: "12px",
    };
    const radiusMap = {
        Small: "12px",
        Medium: "16px",
        Large: "22px",
    };

    const opacity = opacityMap[state.cardOpacity] || opacityMap[APP_CONFIG.defaults.cardOpacity] || "0.95";
    const blur = blurMap[state.cardBlur] || blurMap[APP_CONFIG.defaults.cardBlur] || "6px";
    const radius = radiusMap[state.cardRadius] || radiusMap[APP_CONFIG.defaults.cardRadius] || "16px";

    if (mainCardEl) {
        mainCardEl.style.setProperty("--card-opacity", opacity);
        mainCardEl.style.setProperty("--card-backdrop-blur", blur);
        mainCardEl.style.setProperty("--card-radius", radius);
        mainCardEl.style.setProperty("--card-extend-origin", state.reverseCardExtendDirection ? "bottom" : "top");
        mainCardEl.classList.toggle("hide-title-icon", !state.showTitleIcon);
    }

    if (dashboardEl) {
        dashboardEl.classList.toggle("extend-upward", state.reverseCardExtendDirection);
    }

    if (titleIconEl) {
        titleIconEl.hidden = !state.showTitleIcon;
    }

}

export function getCounterPathForCommand() {
    if (typeof window.COUNTER_PATH === "string" && window.COUNTER_PATH.trim().length > 0) {
        return encodeURI(window.COUNTER_PATH);
    }

    const fallbackPath = `${window.location.pathname || "/"}${window.location.search || ""}`;
    return encodeURI(fallbackPath);
}

export function applyDebugUseAmountSetting(value) {
    const changed = state.debugUseAmount !== value;
    state.debugUseAmount = value;
    return changed;
}

export function applyDebugUseSvDetectionSetting(value) {
    const next = normalizeBooleanSetting(value, APP_CONFIG.defaults.svDetection);
    const changed = state.debugUseSvDetection !== next;
    state.debugUseSvDetection = next;
    return changed;
}

export function applyWsEndpointSetting(value) {
    const next = normalizeWsEndpointValue(value, APP_CONFIG.defaults.wsEndpoint || APP_CONFIG.socketHost);
    const changed = state.wsEndpoint !== next;
    state.wsEndpoint = next;

    if (changed && socket && typeof socket.setHost === "function") {
        socket.setHost(next, true);
    }

    return changed;
}

export function setRuntimeContentBar(contentBar) {
    const previousCardHeight = mainCardEl ? (Number(mainCardEl.getBoundingClientRect().height) || 0) : 0;
    const normalized = normalizeContentBarValue(contentBar);
    const nextBar = (!normalized || normalized === "Auto") ? "Pattern" : normalized;
    const changed = state.contentBar !== nextBar;
    state.contentBar = nextBar;

    const activeContentBar = getActiveContentBar();

    if (activeContentBar !== "Pattern") {
        patternClustersEl.innerHTML = "";
    } else if (!patternClustersEl.innerHTML.trim()) {
        patternClustersEl.innerHTML = "<li class=\"cluster-item empty\">No data</li>";
    }

    if (activeContentBar !== "Etterna") {
        ettSkillBarsEl.innerHTML = "";
    } else if (!ettSkillBarsEl.innerHTML.trim()) {
        ettSkillBarsEl.innerHTML = "<li class=\"ett-skill-item empty\">No data</li>";
    }

    updateContentBarVisibility();
    animateCardHeightTransition(previousCardHeight);
    if (!hasAnyGraphModeEnabled()) {
        clearDiffGraph();
    } else {
        setGraphCursorVisible(false);
    }
    return changed;
}

export function setEffectiveContentBarForMap(contentBarOrNull) {
    const previousCardHeight = mainCardEl ? (Number(mainCardEl.getBoundingClientRect().height) || 0) : 0;
    const normalized = normalizeContentBarValue(contentBarOrNull);
    const next = (!normalized || normalized === "Auto") ? null : normalized;
    const changed = state.effectiveContentBar !== next;
    state.effectiveContentBar = next;

    const activeContentBar = getActiveContentBar();
    if (activeContentBar !== "Pattern") {
        patternClustersEl.innerHTML = "";
    }
    if (activeContentBar !== "Etterna") {
        ettSkillBarsEl.innerHTML = "";
    }

    updateContentBarVisibility();
    animateCardHeightTransition(previousCardHeight);
    if (!hasAnyGraphModeEnabled()) {
        clearDiffGraph();
    } else {
        setGraphCursorVisible(false);
    }

    return changed;
}

export function setRuntimeSrText(srText) {
    const normalized = normalizeSrTextValue(srText);
    const nextText = (!normalized || normalized === "Auto") ? "ReworkSR" : normalized;
    const changed = state.srText !== nextText;
    state.srText = nextText;
    if (reworkStarEl) {
        reworkStarEl.classList.toggle("sr-reworksr", nextText === "ReworkSR");
    }
    return changed;
}

export function setRuntimeDiffText(value) {
    const next = normalizeDiffTextValue(value) || "Difficulty";
    const changed = state.diffText !== next;
    state.diffText = next;
    updateDiffTextVisibility();
    return changed;
}

export function setRuntimeDisplayProfile(profile) {
    const contentChanged = setRuntimeContentBar(profile.contentBar);
    const srChanged = setRuntimeSrText(profile.srText);
    const diffChanged = profile.diffText == null ? false : setRuntimeDiffText(profile.diffText);
    return contentChanged || srChanged || diffChanged;
}

export function refreshAutoDisplayProfile(modeTag = state.currentModeTag || "Mix") {
    const profile = resolveRuntimeDisplayProfile(modeTag);
    return setRuntimeDisplayProfile(profile);
}

export function applyContentBarSetting(contentBar) {
    const nextBar = normalizeContentBarValue(contentBar) || "Pattern";
    const changed = state.userContentBar !== nextBar;
    state.userContentBar = nextBar;

    if (state.userContentBar === "Auto") {
        refreshAutoDisplayProfile();
    } else {
        setRuntimeContentBar(state.userContentBar);
    }

    return changed;
}

export function applySrTextSetting(srText) {
    const nextText = normalizeSrTextValue(srText) || "ReworkSR";
    const changed = state.userSrText !== nextText;
    state.userSrText = nextText;

    if (state.userSrText === "Auto") {
        refreshAutoDisplayProfile();
    } else {
        setRuntimeSrText(state.userSrText);
    }

    return changed;
}

export function applyDiffTextSetting(value) {
    const next = normalizeDiffTextValue(value) || "Difficulty";
    const changed = state.userDiffText !== next;
    state.userDiffText = next;

    setRuntimeDiffText(next);

    return changed;
}

export function applyEstimatorAlgorithmSetting(value) {
    const next = normalizeEstimatorAlgorithmValue(value) || APP_CONFIG.defaults.estimatorAlgorithm;
    const changed = state.estimatorAlgorithm !== next;
    state.estimatorAlgorithm = next;
    return changed;
}

export function applyEtternaVersionSetting(value) {
    const next = normalizeEtternaVersionValue(value) || APP_CONFIG.defaults.etternaVersion;
    const changed = state.etternaVersion !== next;
    state.etternaVersion = next;
    return changed;
}

export function applyCompanellaEtternaVersionSetting(value) {
    const next = normalizeEtternaVersionValue(value) || APP_CONFIG.defaults.companellaEtternaVersion;
    const changed = state.companellaEtternaVersion !== next;
    state.companellaEtternaVersion = next;
    return changed;
}

export function applyPauseDetectionSetting(value) {
    const next = normalizeBooleanSetting(value, APP_CONFIG.defaults.pauseDetectionEnabled);
    const changed = state.pauseDetectionEnabled !== next;
    state.pauseDetectionEnabled = next;

    if (!state.pauseDetectionEnabled) {
        state.isPaused = false;
        state.pauseTimeMs = 0;
        state.frozenInterpMs = 0;
        state.pauseMarkerTimes = [];
        state.pauseCount = 0;
    } else if (!Number.isFinite(state.frozenInterpMs)) {
        state.frozenInterpMs = state.songTimeMs;
    }

    updatePauseCountVisibility();
    redrawPauseMarkers();
    return changed;
}

export function applyVibroDetectionSetting(value) {
    const next = normalizeBooleanSetting(value, APP_CONFIG.defaults.vibroDetection);
    const changed = state.vibroDetection !== next;
    state.vibroDetection = next;
    return changed;
}

export function applyEnableEtternaRainbowBarsSetting(value) {
    const next = normalizeBooleanSetting(value, APP_CONFIG.defaults.enableEtternaRainbowBars);
    const changed = state.enableEtternaRainbowBars !== next;
    state.enableEtternaRainbowBars = next;
    return changed;
}

export function applyEnableStatusMarqueeSetting(value) {
    const next = normalizeBooleanSetting(value, APP_CONFIG.defaults.enableStatusMarquee);
    const changed = state.enableStatusMarquee !== next;
    state.enableStatusMarquee = next;

    if (changed) {
        refreshStatusRendering();
    }

    return changed;
}

export function applyShowModeTagCapsuleSetting(value) {
    const next = normalizeBooleanSetting(value, APP_CONFIG.defaults.showModeTagCapsule);
    const changed = state.showModeTagCapsule !== next;
    state.showModeTagCapsule = next;
    updateModeTagVisibility();
    return changed;
}

export function applyEnableNumericDifficultySetting(value) {
    const next = normalizeBooleanSetting(value, APP_CONFIG.defaults.enableNumericDifficulty);
    const changed = state.enableNumericDifficulty !== next;
    state.enableNumericDifficulty = next;
    updateDiffTextVisibility();
    return changed;
}

export function applyHideCardDuringPlaySetting(value) {
    const next = normalizeBooleanSetting(value, APP_CONFIG.defaults.hideCardDuringPlay);
    const changed = state.hideCardDuringPlay !== next;
    state.hideCardDuringPlay = next;
    updateCardPlayVisibility();
    return changed;
}

export function applyCardOpacitySetting(value) {
    const next = normalizeCardOpacityValue(value) || APP_CONFIG.defaults.cardOpacity;
    const changed = state.cardOpacity !== next;
    state.cardOpacity = next;
    applyVisualStyleSettings();
    return changed;
}

export function applyCardBlurSetting(value) {
    const next = normalizeCardBlurValue(value) || APP_CONFIG.defaults.cardBlur;
    const changed = state.cardBlur !== next;
    state.cardBlur = next;
    applyVisualStyleSettings();
    return changed;
}

export function applyCardRadiusSetting(value) {
    const next = normalizeCardRadiusValue(value) || APP_CONFIG.defaults.cardRadius;
    const changed = state.cardRadius !== next;
    state.cardRadius = next;
    applyVisualStyleSettings();
    return changed;
}

export function applyShowTitleIconSetting(value) {
    const next = normalizeBooleanSetting(value, APP_CONFIG.defaults.showTitleIcon);
    const changed = state.showTitleIcon !== next;
    state.showTitleIcon = next;
    applyVisualStyleSettings();
    return changed;
}

export function applyReverseCardExtendDirectionSetting(value) {
    const next = normalizeBooleanSetting(value, APP_CONFIG.defaults.reverseCardExtendDirection);
    const changed = state.reverseCardExtendDirection !== next;
    state.reverseCardExtendDirection = next;
    applyVisualStyleSettings();
    return changed;
}

function extractSettingsPayloadFromCommandPacket(packet) {
    if (Array.isArray(packet)) {
        return packet;
    }

    if (packet && typeof packet === "object" && packet.command === "getSettings") {
        return packet.message;
    }

    return null;
}

export function setupSettingsCommandListener() {
    if (state.settingsCommandSubscribed) {
        return;
    }

    state.settingsCommandSubscribed = true;

    socket.commands((packet) => {
        const payload = extractSettingsPayloadFromCommandPacket(packet);
        if (!payload) {
            return;
        }

        state.settingsReceivedFromCommand = true;
        const wsEndpointChanged = applyWsEndpointSetting(parseWsEndpointValue(payload));
        const contentBarChanged = applyContentBarSetting(parseContentBarValue(payload));
        const srTextChanged = applySrTextSetting(parseSrTextValue(payload));
        const debugChanged = applyDebugUseAmountSetting(parseDebugUseAmountValue(payload));
        const diffTextChanged = applyDiffTextSetting(parseDiffTextValue(payload));
        const estimatorChanged = applyEstimatorAlgorithmSetting(parseEstimatorAlgorithmValue(payload));
        const etternaVersionChanged = applyEtternaVersionSetting(parseEtternaVersionValue(payload));
        const companellaEtternaVersionChanged = applyCompanellaEtternaVersionSetting(parseCompanellaEtternaVersionValue(payload));
        const pauseChanged = applyPauseDetectionSetting(parseEnablePauseDetectionValue(payload));
        const rainbowChanged = applyEnableEtternaRainbowBarsSetting(parseEnableEtternaRainbowBarsValue(payload));
        const statusMarqueeChanged = applyEnableStatusMarqueeSetting(parseEnableStatusMarqueeValue(payload));
        const vibroChanged = applyVibroDetectionSetting(parseVibroDetectionValue(payload));
        const modeTagVisibilityChanged = applyShowModeTagCapsuleSetting(parseShowModeTagCapsuleValue(payload));
        const numericDifficultyChanged = applyEnableNumericDifficultySetting(parseEnableNumericDifficultyValue(payload));
        const hideCardDuringPlayChanged = applyHideCardDuringPlaySetting(parseHideCardDuringPlayValue(payload));
        const cardOpacityChanged = applyCardOpacitySetting(parseCardOpacityValue(payload));
        const cardBlurChanged = applyCardBlurSetting(parseCardBlurValue(payload));
        const cardRadiusChanged = applyCardRadiusSetting(parseCardRadiusValue(payload));
        const showTitleIconChanged = applyShowTitleIconSetting(parseShowTitleIconValue(payload));
        const reverseCardDirectionChanged = applyReverseCardExtendDirectionSetting(parseReverseCardExtendDirectionValue(payload));
        const svChanged = applyDebugUseSvDetectionSetting(parseSvDetectionValue(payload));

        const legacyAutoMode = parseAutoModeValue(payload);
        if (legacyAutoMode && !isAutoDisplayEnabled()) {
            state.userSrText = "Auto";
            state.userContentBar = "Auto";
            refreshAutoDisplayProfile();
        }

        const changed = contentBarChanged
            || wsEndpointChanged
            || srTextChanged
            || debugChanged
            || diffTextChanged
            || estimatorChanged
            || etternaVersionChanged
            || companellaEtternaVersionChanged
            || pauseChanged
            || rainbowChanged
            || statusMarqueeChanged
            || vibroChanged
            || modeTagVisibilityChanged
            || numericDifficultyChanged
            || hideCardDuringPlayChanged
            || cardOpacityChanged
            || cardBlurChanged
            || cardRadiusChanged
            || showTitleIconChanged
            || reverseCardDirectionChanged
            || svChanged;

        const recomputeNeeded = contentBarChanged
            || srTextChanged
            || debugChanged
            || diffTextChanged
            || estimatorChanged
            || etternaVersionChanged
            || companellaEtternaVersionChanged
            || pauseChanged
            || rainbowChanged
            || vibroChanged
            || modeTagVisibilityChanged
            || svChanged;

        if (typeof state.initialSettingsResolver === "function") {
            const resolve = state.initialSettingsResolver;
            state.initialSettingsResolver = null;
            resolve();
        }

        if (recomputeNeeded) {
            scheduleRecompute("settings changed", true);
        } else if (changed) {
            // Caption-only changes (like numeric display toggle) are applied immediately.
        }
    });

    if (!state.settingsRequested) {
        state.settingsRequested = true;
        socket.sendCommand("getSettings", getCounterPathForCommand());
    }
}

function waitForInitialSettingsFromCommand(timeoutMs) {
    if (state.settingsReceivedFromCommand) {
        return Promise.resolve();
    }

    return new Promise((resolve, reject) => {
        const timeoutId = setTimeout(() => {
            if (state.initialSettingsResolver) {
                state.initialSettingsResolver = null;
            }
            reject(new Error("getSettings timeout"));
        }, timeoutMs);

        state.initialSettingsResolver = () => {
            clearTimeout(timeoutId);
            resolve();
        };
    });
}

export async function loadSettings() {
    setupSettingsCommandListener();

    try {
        await waitForInitialSettingsFromCommand(SETTINGS_COMMAND_TIMEOUT_MS);
        return;
    } catch {
        // Fall back to local settings file fetch when command channel is unavailable.
    }

    try {
        const response = await fetch("./settings.json", {
            method: "GET",
            cache: "no-store",
        });

        if (!response.ok) {
            throw new Error(`settings.json status ${response.status}`);
        }

        const settings = await response.json();
        applyWsEndpointSetting(parseWsEndpointValue(settings));
        applyContentBarSetting(parseContentBarValue(settings));
        applySrTextSetting(parseSrTextValue(settings));
        applyDebugUseAmountSetting(parseDebugUseAmountValue(settings));
        applyDiffTextSetting(parseDiffTextValue(settings));
        applyEstimatorAlgorithmSetting(parseEstimatorAlgorithmValue(settings));
        applyEtternaVersionSetting(parseEtternaVersionValue(settings));
        applyCompanellaEtternaVersionSetting(parseCompanellaEtternaVersionValue(settings));
        applyPauseDetectionSetting(parseEnablePauseDetectionValue(settings));
        applyEnableEtternaRainbowBarsSetting(parseEnableEtternaRainbowBarsValue(settings));
        applyEnableStatusMarqueeSetting(parseEnableStatusMarqueeValue(settings));
        applyVibroDetectionSetting(parseVibroDetectionValue(settings));
        applyShowModeTagCapsuleSetting(parseShowModeTagCapsuleValue(settings));
        applyEnableNumericDifficultySetting(parseEnableNumericDifficultyValue(settings));
        applyHideCardDuringPlaySetting(parseHideCardDuringPlayValue(settings));
        applyCardOpacitySetting(parseCardOpacityValue(settings));
        applyCardBlurSetting(parseCardBlurValue(settings));
        applyCardRadiusSetting(parseCardRadiusValue(settings));
        applyShowTitleIconSetting(parseShowTitleIconValue(settings));
        applyReverseCardExtendDirectionSetting(parseReverseCardExtendDirectionValue(settings));
        applyDebugUseSvDetectionSetting(parseSvDetectionValue(settings));
    } catch {
        applyWsEndpointSetting(APP_CONFIG.defaults.wsEndpoint || APP_CONFIG.socketHost);
        applyContentBarSetting(APP_CONFIG.defaults.contentBar);
        applySrTextSetting(APP_CONFIG.defaults.srText);
        applyDebugUseAmountSetting(APP_CONFIG.defaults.debugUseAmount);
        applyDiffTextSetting(APP_CONFIG.defaults.diffText);
        applyEstimatorAlgorithmSetting(APP_CONFIG.defaults.estimatorAlgorithm);
        applyEtternaVersionSetting(APP_CONFIG.defaults.etternaVersion);
        applyCompanellaEtternaVersionSetting(APP_CONFIG.defaults.companellaEtternaVersion);
        applyPauseDetectionSetting(APP_CONFIG.defaults.pauseDetectionEnabled);
        applyEnableEtternaRainbowBarsSetting(APP_CONFIG.defaults.enableEtternaRainbowBars);
        applyEnableStatusMarqueeSetting(APP_CONFIG.defaults.enableStatusMarquee);
        applyVibroDetectionSetting(APP_CONFIG.defaults.vibroDetection);
        applyShowModeTagCapsuleSetting(APP_CONFIG.defaults.showModeTagCapsule);
        applyEnableNumericDifficultySetting(APP_CONFIG.defaults.enableNumericDifficulty);
        applyHideCardDuringPlaySetting(APP_CONFIG.defaults.hideCardDuringPlay);
        applyCardOpacitySetting(APP_CONFIG.defaults.cardOpacity);
        applyCardBlurSetting(APP_CONFIG.defaults.cardBlur);
        applyCardRadiusSetting(APP_CONFIG.defaults.cardRadius);
        applyShowTitleIconSetting(APP_CONFIG.defaults.showTitleIcon);
        applyReverseCardExtendDirectionSetting(APP_CONFIG.defaults.reverseCardExtendDirection);
        applyDebugUseSvDetectionSetting(APP_CONFIG.defaults.svDetection);
    }
}

export function currentUseDanielAlgorithm() {
    return state.estimatorAlgorithm === "Daniel";
}

export function currentEstimatorAlgorithm() {
    return state.estimatorAlgorithm;
}

export function isAutoDisplayEnabledNow() {
    return isAutoDisplayEnabled();
}
