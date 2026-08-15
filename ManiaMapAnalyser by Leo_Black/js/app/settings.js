import {
    APP_CONFIG,
    bodyGraphWrapEl,
    contentBarShows,
    dashboardEl,
    ettSkillBarsEl,
    getActiveContentBar,
    hasAnyGraphModeEnabled,
    mainCardEl,
    parseAutoModeValue,
    parseAutoContentBarValue,
    parseCardOpacityValue,
    parseCardRadiusValue,
    parseCardBgBlurValue,
    parseContentBarValue,
    parseDebugUseAmountValue,
    parseDiffTextValue,
    parseEnableEtternaRainbowBarsValue,
    parseEnableStatusMarqueeValue,
    parseEnableOsuThemeValue,
    parseEnableFloatingTrianglesValue,
    parseEnableCoverArtValue,
    parseCustomBackgroundColorValue,
    parseEnablePauseDetectionValue,
    parseEnableResultCacheValue,
    parsePauseDetectionThresholdValue,
    parseEstimatorAlgorithmValue,
    parseAzusaSunnyReferenceHoValue,
    parseEtternaVersionValue,
    parseCompanellaEtternaVersionValue,
    parseEnableNumericDifficultyValue,
    parseCardVisibilityValue,
    parseShowModeTagCapsuleValue,
    parseEnableUpdateCheckValue,
    parseReverseCardExtendDirectionValue,
    parseUseOsuFontValue,
    parseSrTextValue,
    parseSvDetectionValue,
    parseDisplay6kLevelValue,
    parseExtendedEstimationRangeValue,
    parseVibroDetectionValue,
    parseWsEndpointValue,
    parseForceSunnyWindowValue,
    parseEnableLNDifficultyValue,
    parseEnableAnalyzeLNValue,
    parseEnableAlwaysShowLNDifficultyValue,
    parseEnableTelemetryValue,
    patternClustersEl,
    ppBarsEl,
    reworkStarEl,
    socket,
    state,
    SETTINGS_COMMAND_TIMEOUT_MS,
    titleIconEl,
} from "./appContext.js";
import {
    normalizeBooleanSetting,
    normalizeContentBarList,
    normalizeContentBarValue,
    normalizeDiffTextValue,
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
import { applyCoverThemeForBeatmap, resetCoverTheme } from "./coverTheme.js";
import { initTriangleField } from "./triangles.js";
import { scheduleRecompute } from "./scheduler.js";
import { runUpdateCheckIfDue, runUpdateCheckNow } from "./updateChecker.js";
import { clearResultCache } from "./resultCache.js";
import { setTelemetryConfig } from "./telemetry.js";

function isAutoDisplayEnabled() {
    return state.userSrText === "Auto" || state.autoContentBar === true;
}

function resolveRuntimeDisplayProfile(modeTag = state.currentModeTag || "Mix") {
    const auto = resolveAutoDisplayProfile(modeTag);
    return {
        contentBar: state.autoContentBar === true ? [auto.contentBar] : state.userContentBar,
        srText: state.userSrText === "Auto" ? auto.srText : state.userSrText,
        diffText: state.userDiffText,
    };
}

// 每个主体区块的 grid item：separator + block 成对，顺序由数组下标决定。
const CONTENT_BAR_SECTIONS = [
    { section: "Pattern", sepEl: null, blockEl: null },
    { section: "Etterna", sepEl: null, blockEl: null },
    { section: "Graph", sepEl: null, blockEl: null },
    { section: "ReworkPP", sepEl: null, blockEl: null },
];

function resolveContentBarSectionElements() {
    const sepMap = {
        Pattern: document.getElementById("sep-pattern"),
        Etterna: document.getElementById("sep-etterna"),
        Graph: document.getElementById("sep-graph"),
        ReworkPP: document.getElementById("sep-pp"),
    };
    const blockMap = {
        Pattern: patternClustersEl,
        Etterna: ettSkillBarsEl,
        Graph: bodyGraphWrapEl,
        ReworkPP: ppBarsEl,
    };
    for (const entry of CONTENT_BAR_SECTIONS) {
        entry.sepEl = sepMap[entry.section];
        entry.blockEl = blockMap[entry.section];
    }
}

function updateContentBarVisibility() {
    const activeContentBar = getActiveContentBar();
    const activeList = Array.isArray(activeContentBar) ? activeContentBar : [];
    const count = activeList.length;
    const isMulti = count >= 2;

    patternClustersEl.hidden = !contentBarShows("Pattern");
    ettSkillBarsEl.hidden = !contentBarShows("Etterna");
    if (bodyGraphWrapEl) {
        bodyGraphWrapEl.hidden = !contentBarShows("Graph");
    }
    if (ppBarsEl) {
        ppBarsEl.hidden = !contentBarShows("ReworkPP");
    }

    mainCardEl.classList.toggle("bars-multi", isMulti);
    mainCardEl.classList.toggle("bars-pattern", !isMulti && count === 1 && activeList[0] === "Pattern");
    mainCardEl.classList.toggle("bars-etterna", !isMulti && count === 1 && activeList[0] === "Etterna");
    mainCardEl.classList.toggle("bars-graph", !isMulti && count === 1 && activeList[0] === "Graph");
    mainCardEl.classList.toggle("bars-pp", !isMulti && count === 1 && activeList[0] === "ReworkPP");
    mainCardEl.classList.toggle("bars-none", count === 0);

    if (!contentBarShows("Etterna")) {
        mainCardEl.classList.remove("bars-etterna-compact");
    }

    // separator 仅多选时显示；按列表顺序给每个区块 (separator+block) 设 CSS order。
    resolveContentBarSectionElements();
    for (const entry of CONTENT_BAR_SECTIONS) {
        const index = activeList.indexOf(entry.section);
        const shown = index !== -1;
        if (entry.sepEl) {
            entry.sepEl.hidden = !(isMulti && shown);
            entry.sepEl.style.order = String(index * 2 + 1);
        }
        if (entry.blockEl) {
            entry.blockEl.style.order = String(index * 2 + 2);
        }
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
    const radiusMap = {
        Small: "12px",
        Medium: "16px",
        Large: "22px",
    };

    const opacity = opacityMap[state.cardOpacity] || opacityMap[APP_CONFIG.defaults.cardOpacity] || "0.95";
    const radius = radiusMap[state.cardRadius] || radiusMap[APP_CONFIG.defaults.cardRadius] || "16px";
    // "Off" maps to 0px so the blur() filter is a no-op; any "<n>px" value passes through.
    const bgBlur = (!state.cardBgBlur || state.cardBgBlur === "Off") ? "0px" : state.cardBgBlur;
    const shouldShowUpdateIcon = Boolean(state.enableUpdateCheck && state.hasAvailableUpdate);

    if (mainCardEl) {
        mainCardEl.style.setProperty("--card-opacity", opacity);
        mainCardEl.style.setProperty("--card-radius", radius);
        mainCardEl.style.setProperty("--ma-cover-blur", bgBlur);
        mainCardEl.style.setProperty("--card-extend-origin", state.reverseCardExtendDirection ? "bottom" : "top");
        mainCardEl.classList.toggle("hide-title-icon", !shouldShowUpdateIcon);
        mainCardEl.classList.toggle("use-osu-font", state.useOsuFont);
    }

    if (dashboardEl) {
        dashboardEl.classList.toggle("extend-upward", state.reverseCardExtendDirection);
    }

    if (titleIconEl) {
        titleIconEl.hidden = !shouldShowUpdateIcon;
        titleIconEl.style.display = shouldShowUpdateIcon ? "" : "none";
    }

}

// Reflect the three theme/effect toggles onto <html> classes. theme.css scopes
// all its osu-skin / triangle / cover rules behind these classes, so flipping a
// class is enough to switch the look live without re-rendering anything.
function applyThemeEffectClasses() {
    if (typeof document === "undefined") {
        return;
    }
    const root = document.documentElement;
    if (!root) {
        return;
    }
    root.classList.toggle("ma-theme-osu", state.enableOsuTheme);
    root.classList.toggle("ma-fx-triangles", state.enableOsuTheme && state.enableFloatingTriangles);
    root.classList.toggle("ma-fx-cover", state.enableCoverArt);
}

export function applyEnableOsuThemeSetting(value) {
    const next = normalizeBooleanSetting(value, APP_CONFIG.defaults.enableOsuTheme);
    const changed = state.enableOsuTheme !== next;
    state.enableOsuTheme = next;
    applyThemeEffectClasses();
    return changed;
}

export function applyEnableFloatingTrianglesSetting(value) {
    const next = normalizeBooleanSetting(value, APP_CONFIG.defaults.enableFloatingTriangles);
    const changed = state.enableFloatingTriangles !== next;
    state.enableFloatingTriangles = next;
    applyThemeEffectClasses();
    return changed;
}

export function applyEnableCoverArtSetting(value) {
    const next = normalizeBooleanSetting(value, APP_CONFIG.defaults.enableCoverArt);
    const changed = state.enableCoverArt !== next;
    state.enableCoverArt = next;
    applyThemeEffectClasses();

    if (!next) {
        // Drop --ma-accent/--ma-cover back to the default osu pink and clear the
        // cached identity so re-enabling re-extracts color for the current map.
        resetCoverTheme();
    } else if (changed && state.lastBeatmapIdentity) {
        applyCoverThemeForBeatmap(state.lastBeatmapIdentity).catch(() => {});
    }

    return changed;
}

export function applyCustomBackgroundColorSetting(value) {
    // 输入已由 parseCustomBackgroundColorValue 归一化（#rrggbb 或 #000000）——不再二次 parse。
    const next = value || "#000000";
    const changed = state.customBackgroundColor !== next;
    state.customBackgroundColor = next;

    const root = document.documentElement;
    if (next !== "#000000") {
        root.style.setProperty("--ma-accent", next);
    }
    // When #000000, do NOT override — let coverTheme auto-sampling take over

    return changed;
}

function getCurrentAppVersion() {
    if (typeof window !== "undefined" && typeof window.__MMA_VERSION === "string") {
        return window.__MMA_VERSION;
    }
    return "0.0.0";
}

function applyAvailableUpdateState(hasUpdate) {
    const next = Boolean(hasUpdate);
    const changed = state.hasAvailableUpdate !== next;
    state.hasAvailableUpdate = next;
    applyVisualStyleSettings();
    return changed;
}

function startUpdateCheckIfEnabled(force = false) {
    const runner = force ? runUpdateCheckNow : runUpdateCheckIfDue;
    runner({
        enabled: state.enableUpdateCheck,
        currentVersion: getCurrentAppVersion(),
        onResult: ({ hasUpdate }) => {
            applyAvailableUpdateState(hasUpdate);
        },
    });
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

export function applyUseSvDetectionSetting(value) {
    const next = normalizeBooleanSetting(value, APP_CONFIG.defaults.useSvDetection);
    const changed = state.useSvDetection !== next;
    state.useSvDetection = next;
    return changed;
}

export function applyDisplay6kLevelSetting(value) {
    const next = normalizeBooleanSetting(value, APP_CONFIG.defaults.display6kLevel);
    const changed = state.display6kLevel !== next;
    state.display6kLevel = next;
    return changed;
}
export function applyExtendedEstimationRangeSetting(value) {
    const next = normalizeBooleanSetting(value, APP_CONFIG.defaults.extendedEstimationRange);
    const changed = state.extendedEstimationRange !== next;
    state.extendedEstimationRange = next;
    return changed;
}

export function applyWsEndpointSetting(value) {
    // 输入已由 parseWsEndpointValue 归一化（trim、剥协议/路径）——不再二次 normalize。
    const next = value || APP_CONFIG.defaults.wsEndpoint || APP_CONFIG.socketHost;
    const changed = state.wsEndpoint !== next;
    state.wsEndpoint = next;

    if (changed && socket && typeof socket.setHost === "function") {
        socket.setHost(next, true);
    }

    return changed;
}

export function applyForceSunnyWindowSetting(value) {
    const next = normalizeBooleanSetting(value, APP_CONFIG.defaults.forceSunnyWindow);
    const changed = state.forceSunnyWindow !== next;
    state.forceSunnyWindow = next;
    return changed;
}

export function applyEnableLNDifficultySetting(value) {
    const next = normalizeBooleanSetting(value, APP_CONFIG.defaults.enableLNDifficulty);
    const changed = state.enableLNDifficulty !== next;
    state.enableLNDifficulty = next;
    return changed;
}

export function applyEnableAnalyzeLNSetting(value) {
    const next = normalizeBooleanSetting(value, APP_CONFIG.defaults.enableAnalyzeLN);
    const changed = state.enableAnalyzeLN !== next;
    state.enableAnalyzeLN = next;
    return changed;
}

export function applyEnableAlwaysShowLNDifficultySetting(value) {
    const next = normalizeBooleanSetting(value, APP_CONFIG.defaults.enableAlwaysShowLNDifficulty);
    const changed = state.enableAlwaysShowLNDifficulty !== next;
    state.enableAlwaysShowLNDifficulty = next;
    return changed;
}

export function setRuntimeContentBar(contentBar) {
    const previousCardHeight = mainCardEl ? (Number(mainCardEl.getBoundingClientRect().height) || 0) : 0;
    // 入参为有序 section 数组（或旧字符串迁移）；空/非法 → 空列表（None）
    const nextBar = normalizeContentBarList(contentBar, APP_CONFIG.options.contentBar);
    const changed = !arraysEqual(state.contentBar, nextBar);
    state.contentBar = nextBar;

    if (!contentBarShows("Pattern")) {
        patternClustersEl.innerHTML = "";
    } else if (!patternClustersEl.innerHTML.trim()) {
        patternClustersEl.innerHTML = "<li class=\"cluster-item empty\">No data</li>";
    }

    if (!contentBarShows("Etterna")) {
        ettSkillBarsEl.innerHTML = "";
    } else if (!ettSkillBarsEl.innerHTML.trim()) {
        ettSkillBarsEl.innerHTML = "<li class=\"ett-skill-item empty\">No data</li>";
    }

    if (!contentBarShows("ReworkPP")) {
        ppBarsEl.innerHTML = "";
    } else if (!ppBarsEl.innerHTML.trim()) {
        ppBarsEl.innerHTML = "<li class=\"pp-item empty\">No data</li>";
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
    // 谱面级覆盖：有序 section 数组；null 表示无覆盖
    const next = contentBarOrNull == null
        ? null
        : normalizeContentBarList(contentBarOrNull, APP_CONFIG.options.contentBar);
    const changed = !arraysEqual(state.effectiveContentBar, next);
    state.effectiveContentBar = next;

    if (!contentBarShows("Pattern")) {
        patternClustersEl.innerHTML = "";
    }
    if (!contentBarShows("Etterna")) {
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

// 数组内容比较（含 null 与空数组），避免引用比较误判。
function arraysEqual(a, b) {
    if (a === b) return true;
    if (a == null || b == null) return false;
    if (!Array.isArray(a) || !Array.isArray(b)) return false;
    if (a.length !== b.length) return false;
    for (let i = 0; i < a.length; i++) {
        if (a[i] !== b[i]) return false;
    }
    return true;
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
    // 输入已由 parseContentBarValue 归一化并校验——有序 section 数组（可空）。
    const nextBar = Array.isArray(contentBar) ? contentBar : [];
    const changed = !arraysEqual(state.userContentBar, nextBar);
    state.userContentBar = nextBar;

    if (state.autoContentBar === true) {
        refreshAutoDisplayProfile();
    } else {
        setRuntimeContentBar(state.userContentBar);
    }

    return changed;
}

export function applyAutoContentBarSetting(value) {
    const next = normalizeBooleanSetting(value, APP_CONFIG.defaults.autoContentBar);
    const changed = state.autoContentBar !== next;
    state.autoContentBar = next;

    if (state.autoContentBar === true) {
        refreshAutoDisplayProfile();
    } else {
        setRuntimeContentBar(state.userContentBar);
    }

    return changed;
}

export function applySrTextSetting(srText) {
    // 输入已由 parseSrTextValue 归一化并校验——不再二次 normalize。
    const nextText = srText || "ReworkSR";
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
    // 输入已由 parseDiffTextValue 归一化并校验——不再二次 normalize。
    const next = value || "Difficulty";
    const changed = state.userDiffText !== next;
    state.userDiffText = next;

    setRuntimeDiffText(next);

    return changed;
}

export function applyEstimatorAlgorithmSetting(value) {
    // 输入已由 parseEstimatorAlgorithmValue 归一化并校验——不再二次 normalize。
    const next = value || APP_CONFIG.defaults.estimatorAlgorithm;
    const changed = state.estimatorAlgorithm !== next;
    state.estimatorAlgorithm = next;
    return changed;
}

export function applyAzusaSunnyReferenceHoSetting(value) {
    const next = normalizeBooleanSetting(value, APP_CONFIG.defaults.azusaSunnyReferenceHo);
    const changed = state.azusaSunnyReferenceHo !== next;
    state.azusaSunnyReferenceHo = next;
    return changed;
}

export function applyEtternaVersionSetting(value) {
    // 输入已由 parseEtternaVersionValue 归一化并校验——不再二次 normalize。
    const next = value || APP_CONFIG.defaults.etternaVersion;
    const changed = state.etternaVersion !== next;
    state.etternaVersion = next;
    return changed;
}

export function applyCompanellaEtternaVersionSetting(value) {
    // 输入已由 parseCompanellaEtternaVersionValue 归一化并校验——不再二次 normalize。
    const next = value || APP_CONFIG.defaults.companellaEtternaVersion;
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
        state.pauseFreezeStartRealMs = 0;
        state.pauseFreezeSongTimeMs = 0;
        state.pauseMarkerTimes = [];
        state.pauseCount = 0;
    } else if (!Number.isFinite(state.frozenInterpMs)) {
        state.frozenInterpMs = state.songTimeMs;
    }

    updatePauseCountVisibility();
    redrawPauseMarkers();
    return changed;
}

export function applyPauseDetectionThresholdSetting(value) {
    const num = Number(value);
    const next = (Number.isFinite(num) && num > 0) ? Math.round(num) : APP_CONFIG.defaults.pauseDetectionThresholdMs;
    const changed = state.pauseDetectionThresholdMs !== next;
    state.pauseDetectionThresholdMs = next;
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

export function applyCardVisibilitySetting(value) {
    const valid = ["DuringPlay", "OutsidePlay", "Always"];
    const next = valid.includes(value) ? value : APP_CONFIG.defaults.cardVisibility;
    const changed = state.cardVisibility !== next;
    state.cardVisibility = next;
    updateCardPlayVisibility();
    return changed;
}

export function applyCardOpacitySetting(value) {
    // 输入已由 parseCardOpacityValue 归一化并校验（cardOpacitySet 成员）——不再二次 normalize。
    const next = value || APP_CONFIG.defaults.cardOpacity;
    const changed = state.cardOpacity !== next;
    state.cardOpacity = next;
    applyVisualStyleSettings();
    return changed;
}

export function applyCardRadiusSetting(value) {
    // 输入已由 parseCardRadiusValue 归一化并校验（cardRadiusSet 成员）——不再二次 normalize。
    const next = value || APP_CONFIG.defaults.cardRadius;
    const changed = state.cardRadius !== next;
    state.cardRadius = next;
    applyVisualStyleSettings();
    return changed;
}

export function applyCardBgBlurSetting(value) {
    // 输入已由 parseCardBgBlurValue 归一化并校验（cardBgBlurSet 成员）——不再二次 normalize。
    const next = value || APP_CONFIG.defaults.cardBgBlur;
    const changed = state.cardBgBlur !== next;
    state.cardBgBlur = next;
    applyVisualStyleSettings();
    return changed;
}

export function applyEnableUpdateCheckSetting(value) {
    const next = normalizeBooleanSetting(value, APP_CONFIG.defaults.enableUpdateCheck);
    const changed = state.enableUpdateCheck !== next;
    const wasEnabled = state.enableUpdateCheck;
    state.enableUpdateCheck = next;

    if (!next) {
        applyAvailableUpdateState(false);
    } else {
        const forceCheck = changed && !wasEnabled && next;
        startUpdateCheckIfEnabled(forceCheck);
    }

    applyVisualStyleSettings();
    return changed;
}

export function applyEnableResultCacheSetting(value) {
    const next = normalizeBooleanSetting(value, APP_CONFIG.defaults.enableResultCache);
    const changed = state.enableResultCache !== next;
    const wasEnabled = state.enableResultCache;
    state.enableResultCache = next;

    if (changed && wasEnabled && !next) {
        clearResultCache();
    }

    return changed;
}

export function applyEnableTelemetrySetting(value) {
    const next = normalizeBooleanSetting(value, APP_CONFIG.defaults.enableTelemetry);
    const changed = state.enableTelemetry !== next;
    state.enableTelemetry = next;
    setTelemetryConfig();
    return changed;
}

export function applyReverseCardExtendDirectionSetting(value) {
    const next = normalizeBooleanSetting(value, APP_CONFIG.defaults.reverseCardExtendDirection);
    const changed = state.reverseCardExtendDirection !== next;
    state.reverseCardExtendDirection = next;
    applyVisualStyleSettings();
    return changed;
}

export function applyUseOsuFontSetting(value) {
    const next = normalizeBooleanSetting(value, APP_CONFIG.defaults.useOsuFont);
    const changed = state.useOsuFont !== next;
    state.useOsuFont = next;
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

// 检测 contentBar 旧字符串值并主动写回 commands 数组格式。
// tosu dashboard 渲染 commands 类型时假设 value 是数组；旧版 values.json 里
// contentBar 是字符串（"Auto"/"Pattern"/"ReworkPP"/"Full"/"None"），会在
// .find()/.forEach() 处抛 TypeError 导致设置页卡死。插件启动收到 getSettings
// 后如果发现旧格式，立即通过 tosu 的 HTTP API 把 values.json 重写为数组，
// dashboard 下次打开即正常（无需修改 tosu，所有用户升级后自动修复）。
function migrateLegacyContentBarValue(payload) {
    const rawContentBar = Array.isArray(payload)
        ? payload.find((entry) => entry?.uniqueID === "contentBar")?.value
        : (payload && typeof payload === "object" ? payload.contentBar : undefined);
    if (rawContentBar === undefined || rawContentBar === null || Array.isArray(rawContentBar)) {
        return;
    }

    const normalized = normalizeContentBarValue(rawContentBar);
    if (!normalized) {
        return;
    }

    let newValue;
    if (normalized === "Full") {
        newValue = ["Pattern", "Etterna", "Graph", "ReworkPP"].map((section) => ({ section }));
    } else if (normalized === "None" || normalized === "Auto") {
        // None → 空列表；Auto → 由 autoContentBar 控制（写回时同时置 true）
        newValue = [];
    } else {
        newValue = [{ section: normalized }];
    }

    const writeBack = [{ uniqueID: "contentBar", value: newValue }];
    if (normalized === "Auto") {
        writeBack.push({ uniqueID: "autoContentBar", value: true });
    }

    const folderName = (typeof window.COUNTER_PATH === "string" ? window.COUNTER_PATH : "").trim();
    if (!folderName) {
        return;
    }

    fetch(`/api/counters/settings/${encodeURIComponent(folderName)}`, {
        method: "POST",
        body: JSON.stringify(writeBack),
    }).catch(() => {
        // 迁移失败静默处理：设置页仍可能卡死，但插件自身不受影响。
    });
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

        // 迁移：contentBar 自 2026-08-15 起为 commands 类型（数组），但旧版
        // values.json 里可能是单字符串（如 "Auto"/"ReworkPP"）。tosu dashboard
        // 渲染 commands 类型时对字符串调用 .find()/.forEach() 会抛 TypeError
        // 导致设置页卡死——这里检测到旧字符串就主动发 saveSettings 写回数组，
        // 让所有用户升级后自动修复（无需改 tosu）。
        migrateLegacyContentBarValue(payload);

        // Only apply a setting if it's actually present in the payload.
        // Otherwise the parser's config.js default would overwrite the
        // settings.json baseline for settings tosu didn't send.
        const hasKey = (key) => {
            if (Array.isArray(payload)) {
                return payload.some((entry) => entry?.uniqueID === key);
            }
            return Object.prototype.hasOwnProperty.call(payload, key);
        };
        const applyIf = (key, applyFn, parseResult) =>
            hasKey(key) ? applyFn(parseResult) : false;

        state.settingsReceivedFromCommand = true;
        const wsEndpointChanged = applyIf("wsEndpoint", applyWsEndpointSetting, parseWsEndpointValue(payload));
        const contentBarChanged = applyIf("contentBar", applyContentBarSetting, parseContentBarValue(payload));
        const autoContentBarChanged = applyIf("autoContentBar", applyAutoContentBarSetting, parseAutoContentBarValue(payload));
        const srTextChanged = applyIf("srText", applySrTextSetting, parseSrTextValue(payload));
        const debugChanged = applyIf("debugUseAmount", applyDebugUseAmountSetting, parseDebugUseAmountValue(payload));
        const diffTextChanged = applyIf("diffText", applyDiffTextSetting, parseDiffTextValue(payload));
        const estimatorChanged = applyIf("estimatorAlgorithm", applyEstimatorAlgorithmSetting, parseEstimatorAlgorithmValue(payload));
        const azusaSunnyReferenceHoChanged = applyIf("azusaSunnyReferenceHo", applyAzusaSunnyReferenceHoSetting, parseAzusaSunnyReferenceHoValue(payload));
        const etternaVersionChanged = applyIf("etternaVersion", applyEtternaVersionSetting, parseEtternaVersionValue(payload));
        const companellaEtternaVersionChanged = applyIf("companellaEtternaVersion", applyCompanellaEtternaVersionSetting, parseCompanellaEtternaVersionValue(payload));
        const pauseChanged = applyIf("enablePauseDetection", applyPauseDetectionSetting, parseEnablePauseDetectionValue(payload));
        const pauseThresholdChanged = applyIf("pauseDetectionThreshold", applyPauseDetectionThresholdSetting, parsePauseDetectionThresholdValue(payload));
        const rainbowChanged = applyIf("enableEtternaRainbowBars", applyEnableEtternaRainbowBarsSetting, parseEnableEtternaRainbowBarsValue(payload));
        const statusMarqueeChanged = applyIf("enableStatusMarquee", applyEnableStatusMarqueeSetting, parseEnableStatusMarqueeValue(payload));
        const vibroChanged = applyIf("VibroDetection", applyVibroDetectionSetting, parseVibroDetectionValue(payload));
        const modeTagVisibilityChanged = applyIf("showModeTagCapsule", applyShowModeTagCapsuleSetting, parseShowModeTagCapsuleValue(payload));
        const numericDifficultyChanged = applyIf("enableNumericDifficulty", applyEnableNumericDifficultySetting, parseEnableNumericDifficultyValue(payload));
        const cardVisibilityChanged = applyIf("cardVisibility", applyCardVisibilitySetting, parseCardVisibilityValue(payload));
        const cardOpacityChanged = applyIf("cardOpacity", applyCardOpacitySetting, parseCardOpacityValue(payload));
        const cardRadiusChanged = applyIf("cardRadius", applyCardRadiusSetting, parseCardRadiusValue(payload));
        const cardBgBlurChanged = applyIf("cardBgBlur", applyCardBgBlurSetting, parseCardBgBlurValue(payload));
        const enableUpdateCheckChanged = applyIf("enableUpdateCheck", applyEnableUpdateCheckSetting, parseEnableUpdateCheckValue(payload));
        const resultCacheChanged = applyIf("enableResultCache", applyEnableResultCacheSetting, parseEnableResultCacheValue(payload));
        const reverseCardDirectionChanged = applyIf("reverseCardExtendDirection", applyReverseCardExtendDirectionSetting, parseReverseCardExtendDirectionValue(payload));
        const osuFontChanged = applyIf("useOsuFont", applyUseOsuFontSetting, parseUseOsuFontValue(payload));
        const svChanged = applyIf("useSvDetection", applyUseSvDetectionSetting, parseSvDetectionValue(payload));
        const forceSunnyWindowChanged = applyIf("forceSunnyWindow", applyForceSunnyWindowSetting, parseForceSunnyWindowValue(payload));
        const enableLNDifficultyChanged = applyIf("enableLNDifficulty", applyEnableLNDifficultySetting, parseEnableLNDifficultyValue(payload));
        const enableAnalyzeLNChanged = applyIf("enableAnalyzeLN", applyEnableAnalyzeLNSetting, parseEnableAnalyzeLNValue(payload));
        const enableAlwaysShowLNDifficultyChanged = applyIf("enableAlwaysShowLNDifficulty", applyEnableAlwaysShowLNDifficultySetting, parseEnableAlwaysShowLNDifficultyValue(payload));
        const enableTelemetryChanged = applyIf("enableTelemetry", applyEnableTelemetrySetting, parseEnableTelemetryValue(payload));
        const display6kLevelChanged = applyIf("display6kLevel", applyDisplay6kLevelSetting, parseDisplay6kLevelValue(payload));
        const extendedEstimationRangeChanged = applyIf("extendedEstimationRange", applyExtendedEstimationRangeSetting, parseExtendedEstimationRangeValue(payload));
        const osuThemeChanged = applyIf("enableOsuTheme", applyEnableOsuThemeSetting, parseEnableOsuThemeValue(payload));
        const floatingTrianglesChanged = applyIf("enableFloatingTriangles", applyEnableFloatingTrianglesSetting, parseEnableFloatingTrianglesValue(payload));
        const coverArtChanged = applyIf("enableCoverArt", applyEnableCoverArtSetting, parseEnableCoverArtValue(payload));
        const customColorChanged = applyIf("customBackgroundColor", applyCustomBackgroundColorSetting, parseCustomBackgroundColorValue(payload));

        const legacyAutoMode = parseAutoModeValue(payload);
        if (legacyAutoMode && !isAutoDisplayEnabled()) {
            state.userSrText = "Auto";
            state.autoContentBar = true;
            refreshAutoDisplayProfile();
        }

        const changed = contentBarChanged
            || autoContentBarChanged
            || wsEndpointChanged
            || srTextChanged
            || debugChanged
            || diffTextChanged
            || estimatorChanged
            || azusaSunnyReferenceHoChanged
            || etternaVersionChanged
            || companellaEtternaVersionChanged
            || pauseChanged
            || pauseThresholdChanged
            || rainbowChanged
            || statusMarqueeChanged
            || vibroChanged
            || modeTagVisibilityChanged
            || numericDifficultyChanged
            || cardVisibilityChanged
            || cardOpacityChanged
            || cardRadiusChanged
            || cardBgBlurChanged
            || enableUpdateCheckChanged
            || resultCacheChanged
            || reverseCardDirectionChanged
            || osuFontChanged
            || osuThemeChanged
            || floatingTrianglesChanged
            || coverArtChanged
            || customColorChanged
            || svChanged
            || forceSunnyWindowChanged
            || enableLNDifficultyChanged
            || enableAnalyzeLNChanged
            || enableAlwaysShowLNDifficultyChanged
            || display6kLevelChanged
            || extendedEstimationRangeChanged
            || enableTelemetryChanged;

        const recomputeNeeded = contentBarChanged
            || autoContentBarChanged
            || srTextChanged
            || debugChanged
            || diffTextChanged
            || estimatorChanged
            || azusaSunnyReferenceHoChanged
            || etternaVersionChanged
            || companellaEtternaVersionChanged
            || pauseChanged
            || pauseThresholdChanged
            || rainbowChanged
            || vibroChanged
            || modeTagVisibilityChanged
            || svChanged
            || forceSunnyWindowChanged
            || enableLNDifficultyChanged
            || enableAnalyzeLNChanged
            || enableAlwaysShowLNDifficultyChanged
            || display6kLevelChanged
            || extendedEstimationRangeChanged;

        // Invalidate cached results when any computation-affecting setting changed.
        // wsEndpointChanged lives in `changed` only (not recomputeNeeded), so it is listed here explicitly.
        // debugChanged + display6kLevelChanged are display-only (toggle-diff proved zero output-contract
        // diffs; task 13 evidence): they stay in recomputeNeeded so the cache HIT path re-derives
        // (sixKConst / debugUseAmount post-processing) without clearing the cache or re-fetching.
        // enableAlwaysShowLNDifficulty stays here — toggle-diff showed real estDiff diffs on lnRatio<0.15 maps.
        if (estimatorChanged
            || azusaSunnyReferenceHoChanged
            || etternaVersionChanged
            || companellaEtternaVersionChanged
            || svChanged
            || vibroChanged
            || wsEndpointChanged
            || forceSunnyWindowChanged
            || enableLNDifficultyChanged
            || enableAnalyzeLNChanged
            || enableAlwaysShowLNDifficultyChanged
            || extendedEstimationRangeChanged) {
            clearResultCache();
        }

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
    // Always load settings.json first as baseline.
    // The command channel may not include every setting; settings.json fills the gaps.
    let fileSettings = null;
    try {
        const response = await fetch("./settings.json", {
            method: "GET",
            cache: "no-store",
        });
        if (response.ok) {
            fileSettings = await response.json();
        }
    } catch {
        fileSettings = null;
    }

    function applySettingsFrom(source) {
        applyWsEndpointSetting(parseWsEndpointValue(source));
        applyContentBarSetting(parseContentBarValue(source));
        applyAutoContentBarSetting(parseAutoContentBarValue(source));
        applySrTextSetting(parseSrTextValue(source));
        applyDebugUseAmountSetting(parseDebugUseAmountValue(source));
        applyDiffTextSetting(parseDiffTextValue(source));
        applyEstimatorAlgorithmSetting(parseEstimatorAlgorithmValue(source));
        applyAzusaSunnyReferenceHoSetting(parseAzusaSunnyReferenceHoValue(source));
        applyEtternaVersionSetting(parseEtternaVersionValue(source));
        applyCompanellaEtternaVersionSetting(parseCompanellaEtternaVersionValue(source));
        applyPauseDetectionSetting(parseEnablePauseDetectionValue(source));
        applyPauseDetectionThresholdSetting(parsePauseDetectionThresholdValue(source));
        applyEnableEtternaRainbowBarsSetting(parseEnableEtternaRainbowBarsValue(source));
        applyEnableStatusMarqueeSetting(parseEnableStatusMarqueeValue(source));
        applyVibroDetectionSetting(parseVibroDetectionValue(source));
        applyShowModeTagCapsuleSetting(parseShowModeTagCapsuleValue(source));
        applyEnableNumericDifficultySetting(parseEnableNumericDifficultyValue(source));
        applyCardVisibilitySetting(parseCardVisibilityValue(source));
        applyCardOpacitySetting(parseCardOpacityValue(source));
        applyCardRadiusSetting(parseCardRadiusValue(source));
        applyCardBgBlurSetting(parseCardBgBlurValue(source));
        applyEnableUpdateCheckSetting(parseEnableUpdateCheckValue(source));
        applyEnableResultCacheSetting(parseEnableResultCacheValue(source));
        applyReverseCardExtendDirectionSetting(parseReverseCardExtendDirectionValue(source));
        applyUseOsuFontSetting(parseUseOsuFontValue(source));
        applyEnableOsuThemeSetting(parseEnableOsuThemeValue(source));
        applyEnableFloatingTrianglesSetting(parseEnableFloatingTrianglesValue(source));
        applyEnableCoverArtSetting(parseEnableCoverArtValue(source));
        applyCustomBackgroundColorSetting(parseCustomBackgroundColorValue(source));
        applyUseSvDetectionSetting(parseSvDetectionValue(source));
        applyForceSunnyWindowSetting(parseForceSunnyWindowValue(source));
        applyEnableLNDifficultySetting(parseEnableLNDifficultyValue(source));
        applyEnableAnalyzeLNSetting(parseEnableAnalyzeLNValue(source));
        applyEnableAlwaysShowLNDifficultySetting(parseEnableAlwaysShowLNDifficultyValue(source));
        applyEnableTelemetrySetting(parseEnableTelemetryValue(source));
        applyDisplay6kLevelSetting(parseDisplay6kLevelValue(source));
        applyExtendedEstimationRangeSetting(parseExtendedEstimationRangeValue(source));
    }

    // Apply file settings as baseline immediately
    if (fileSettings) {
        applySettingsFrom(fileSettings);
    } else {
        // File unavailable — apply config defaults as fallback
        applySettingsFrom({
            wsEndpoint: APP_CONFIG.defaults.wsEndpoint || APP_CONFIG.socketHost,
            autoContentBar: APP_CONFIG.defaults.autoContentBar,
            contentBar: APP_CONFIG.defaults.contentBar,
            srText: APP_CONFIG.defaults.srText,
            debugUseAmount: APP_CONFIG.defaults.debugUseAmount,
            diffText: APP_CONFIG.defaults.diffText,
            estimatorAlgorithm: APP_CONFIG.defaults.estimatorAlgorithm,
            azusaSunnyReferenceHo: APP_CONFIG.defaults.azusaSunnyReferenceHo,
            etternaVersion: APP_CONFIG.defaults.etternaVersion,
            companellaEtternaVersion: APP_CONFIG.defaults.companellaEtternaVersion,
            enablePauseDetection: APP_CONFIG.defaults.pauseDetectionEnabled,
            pauseDetectionThreshold: APP_CONFIG.defaults.pauseDetectionThresholdMs,
            enableEtternaRainbowBars: APP_CONFIG.defaults.enableEtternaRainbowBars,
            enableStatusMarquee: APP_CONFIG.defaults.enableStatusMarquee,
            VibroDetection: APP_CONFIG.defaults.vibroDetection,
            showModeTagCapsule: APP_CONFIG.defaults.showModeTagCapsule,
            enableNumericDifficulty: APP_CONFIG.defaults.enableNumericDifficulty,
            cardVisibility: APP_CONFIG.defaults.cardVisibility,
            cardOpacity: APP_CONFIG.defaults.cardOpacity,
            cardRadius: APP_CONFIG.defaults.cardRadius,
            cardBgBlur: APP_CONFIG.defaults.cardBgBlur,
            enableUpdateCheck: APP_CONFIG.defaults.enableUpdateCheck,
            enableResultCache: APP_CONFIG.defaults.enableResultCache,
            reverseCardExtendDirection: APP_CONFIG.defaults.reverseCardExtendDirection,
            useOsuFont: APP_CONFIG.defaults.useOsuFont,
            enableOsuTheme: APP_CONFIG.defaults.enableOsuTheme,
            enableFloatingTriangles: APP_CONFIG.defaults.enableFloatingTriangles,
            enableCoverArt: APP_CONFIG.defaults.enableCoverArt,
            customBackgroundColor: APP_CONFIG.defaults.customBackgroundColor,
            useSvDetection: APP_CONFIG.defaults.useSvDetection,
            forceSunnyWindow: APP_CONFIG.defaults.forceSunnyWindow,
            enableLNDifficulty: APP_CONFIG.defaults.enableLNDifficulty,
            enableAnalyzeLN: APP_CONFIG.defaults.enableAnalyzeLN,
            enableAlwaysShowLNDifficulty: APP_CONFIG.defaults.enableAlwaysShowLNDifficulty,
            enableTelemetry: APP_CONFIG.defaults.enableTelemetry,
            display6kLevel: APP_CONFIG.defaults.display6kLevel,
            extendedEstimationRange: APP_CONFIG.defaults.extendedEstimationRange,
        });
    }

    // Register command listener for live settings updates (user changes in tosu UI).
    // The listener is set up after file baseline so command callbacks don't
    // overwrite file values with config defaults for settings tosu doesn't send.
    setupSettingsCommandListener();
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
