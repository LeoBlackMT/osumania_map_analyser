import {
    APP_CONFIG,
    bodyGraphWrapEl,
    ettSkillBarsEl,
    hasAnyGraphModeEnabled,
    mainCardEl,
    parseAutoModeValue,
    parseContentBarValue,
    parseDebugUseAmountValue,
    parseDiffTextValue,
    parseEnableEtternaRainbowBarsValue,
    parseEnablePauseDetectionValue,
    parseEstimatorAlgorithmValue,
    parseShowModeTagCapsuleValue,
    parseSrTextValue,
    parseSvDetectionValue,
    parseVibroDetectionValue,
    patternClustersEl,
    reworkStarEl,
    socket,
    state,
    SETTINGS_COMMAND_TIMEOUT_MS,
} from "./appContext.js";
import {
    normalizeBooleanSetting,
    normalizeContentBarValue,
    normalizeDiffTextValue,
    normalizeEstimatorAlgorithmValue,
    normalizeSrTextValue,
} from "./settingsParsers.js";
import {
    clearDiffGraph,
    redrawPauseMarkers,
    setGraphCursorVisible,
    updateDiffTextVisibility,
} from "./graph.js";
import { updateModeTagVisibility, updatePauseCountVisibility } from "./hud.js";
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
    patternClustersEl.hidden = state.contentBar !== "Pattern";
    ettSkillBarsEl.hidden = state.contentBar !== "Etterna";
    if (bodyGraphWrapEl) {
        bodyGraphWrapEl.hidden = state.contentBar !== "Graph";
    }

    mainCardEl.classList.toggle("bars-pattern", state.contentBar === "Pattern");
    mainCardEl.classList.toggle("bars-etterna", state.contentBar === "Etterna");
    mainCardEl.classList.toggle("bars-graph", state.contentBar === "Graph");
    mainCardEl.classList.toggle("bars-none", state.contentBar === "None");

    if (state.contentBar !== "Etterna") {
        mainCardEl.classList.remove("bars-etterna-compact");
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

export function setRuntimeContentBar(contentBar) {
    const normalized = normalizeContentBarValue(contentBar);
    const nextBar = (!normalized || normalized === "Auto") ? "Pattern" : normalized;
    const changed = state.contentBar !== nextBar;
    state.contentBar = nextBar;

    if (state.contentBar !== "Pattern") {
        patternClustersEl.innerHTML = "";
    } else if (!patternClustersEl.innerHTML.trim()) {
        patternClustersEl.innerHTML = "<li class=\"cluster-item empty\">No data</li>";
    }

    if (state.contentBar !== "Etterna") {
        ettSkillBarsEl.innerHTML = "";
    } else if (!ettSkillBarsEl.innerHTML.trim()) {
        ettSkillBarsEl.innerHTML = "<li class=\"ett-skill-item empty\">No data</li>";
    }

    updateContentBarVisibility();
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
    const next = normalizeEstimatorAlgorithmValue(value) || "Sunny";
    const changed = state.estimatorAlgorithm !== next;
    state.estimatorAlgorithm = next;
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

export function applyShowModeTagCapsuleSetting(value) {
    const next = normalizeBooleanSetting(value, APP_CONFIG.defaults.showModeTagCapsule);
    const changed = state.showModeTagCapsule !== next;
    state.showModeTagCapsule = next;
    updateModeTagVisibility();
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
        const contentBarChanged = applyContentBarSetting(parseContentBarValue(payload));
        const srTextChanged = applySrTextSetting(parseSrTextValue(payload));
        const debugChanged = applyDebugUseAmountSetting(parseDebugUseAmountValue(payload));
        const diffTextChanged = applyDiffTextSetting(parseDiffTextValue(payload));
        const estimatorChanged = applyEstimatorAlgorithmSetting(parseEstimatorAlgorithmValue(payload));
        const pauseChanged = applyPauseDetectionSetting(parseEnablePauseDetectionValue(payload));
        const rainbowChanged = applyEnableEtternaRainbowBarsSetting(parseEnableEtternaRainbowBarsValue(payload));
        const vibroChanged = applyVibroDetectionSetting(parseVibroDetectionValue(payload));
        const modeTagVisibilityChanged = applyShowModeTagCapsuleSetting(parseShowModeTagCapsuleValue(payload));
        const svChanged = applyDebugUseSvDetectionSetting(parseSvDetectionValue(payload));

        const legacyAutoMode = parseAutoModeValue(payload);
        if (legacyAutoMode && !isAutoDisplayEnabled()) {
            state.userSrText = "Auto";
            state.userContentBar = "Auto";
            refreshAutoDisplayProfile();
        }

        const changed = contentBarChanged
            || srTextChanged
            || debugChanged
            || diffTextChanged
            || estimatorChanged
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

        if (changed) {
            scheduleRecompute("settings changed", true);
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
        applyContentBarSetting(parseContentBarValue(settings));
        applySrTextSetting(parseSrTextValue(settings));
        applyDebugUseAmountSetting(parseDebugUseAmountValue(settings));
        applyDiffTextSetting(parseDiffTextValue(settings));
        applyEstimatorAlgorithmSetting(parseEstimatorAlgorithmValue(settings));
        applyPauseDetectionSetting(parseEnablePauseDetectionValue(settings));
        applyEnableEtternaRainbowBarsSetting(parseEnableEtternaRainbowBarsValue(settings));
        applyVibroDetectionSetting(parseVibroDetectionValue(settings));
        applyShowModeTagCapsuleSetting(parseShowModeTagCapsuleValue(settings));
        applyDebugUseSvDetectionSetting(parseSvDetectionValue(settings));
    } catch {
        applyContentBarSetting(APP_CONFIG.defaults.contentBar);
        applySrTextSetting(APP_CONFIG.defaults.srText);
        applyDebugUseAmountSetting(APP_CONFIG.defaults.debugUseAmount);
        applyDiffTextSetting(APP_CONFIG.defaults.diffText);
        applyEstimatorAlgorithmSetting(APP_CONFIG.defaults.estimatorAlgorithm);
        applyPauseDetectionSetting(APP_CONFIG.defaults.pauseDetectionEnabled);
        applyEnableEtternaRainbowBarsSetting(APP_CONFIG.defaults.enableEtternaRainbowBars);
        applyVibroDetectionSetting(APP_CONFIG.defaults.vibroDetection);
        applyShowModeTagCapsuleSetting(APP_CONFIG.defaults.showModeTagCapsule);
        applyDebugUseSvDetectionSetting(APP_CONFIG.defaults.svDetection);
    }
}

export function currentUseDanielAlgorithm() {
    return state.estimatorAlgorithm === "Daniel";
}

export function isAutoDisplayEnabledNow() {
    return isAutoDisplayEnabled();
}
