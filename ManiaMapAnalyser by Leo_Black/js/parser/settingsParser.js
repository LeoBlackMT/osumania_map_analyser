function createSet(values) {
    return new Set((values || []).map((item) => String(item).toLowerCase()));
}

export function normalizeContentBarValue(value) {
    if (typeof value !== "string") {
        return null;
    }

    const lowered = value.trim().toLowerCase();
    if (lowered === "auto") return "Auto";
    if (lowered === "pattern") return "Pattern";
    if (lowered === "etterna") return "Etterna";
    if (lowered === "graph") return "Graph";
    if (lowered === "none") return "None";
    return null;
}

export function normalizeSrTextValue(value) {
    if (typeof value !== "string") {
        return null;
    }

    const lowered = value.trim().toLowerCase();
    if (lowered === "auto") return "Auto";
    if (lowered === "reworksr") return "ReworkSR";
    if (lowered === "msd") return "MSD";
    if (lowered === "pattern") return "Pattern";
    if (lowered === "interludesr") return "InterludeSR";
    return null;
}

export function normalizeEstimatorAlgorithmValue(value) {
    if (typeof value !== "string") {
        return null;
    }

    const lowered = value.trim().toLowerCase();
    if (lowered === "mixed") return "Mixed";
    if (lowered === "mix") return "Mixed";
    if (lowered === "sunny") return "Sunny";
    if (lowered === "rework") return "Sunny";
    if (lowered === "direct") return "Sunny";
    if (lowered === "daniel") return "Daniel";
    if (lowered === "companella") return "Companella";
    if (lowered === "campanella") return "Companella";
    return null;
}

export function normalizeEtternaVersionValue(value) {
    if (typeof value !== "string") {
        return null;
    }

    const trimmed = value.trim();
    if (trimmed === "0.68.0") {
        return "0.68.0-Unofficial";
    }
    return trimmed.length > 0 ? trimmed : null;
}

export function normalizeCardOpacityValue(value) {
    if (typeof value === "number" && Number.isFinite(value)) {
        const clamped = Math.max(0, Math.min(100, Math.round(value)));
        return `${clamped}%`;
    }

    if (typeof value !== "string") {
        return null;
    }

    const trimmed = value.trim();
    if (!trimmed) {
        return null;
    }

    const match = trimmed.match(/^(\d{1,3})(%)?$/);
    if (!match) {
        return null;
    }

    const numeric = Number.parseInt(match[1], 10);
    if (!Number.isFinite(numeric)) {
        return null;
    }

    const clamped = Math.max(0, Math.min(100, numeric));
    return `${clamped}%`;
}

export function normalizeCardBlurValue(value) {
    if (typeof value !== "string") {
        return null;
    }

    const lowered = value.trim().toLowerCase();
    if (lowered === "off") return "Off";
    if (lowered === "soft") return "Soft";
    if (lowered === "strong") return "Strong";
    return null;
}

export function normalizeCardRadiusValue(value) {
    if (typeof value !== "string") {
        return null;
    }

    const lowered = value.trim().toLowerCase();
    if (lowered === "small") return "Small";
    if (lowered === "medium") return "Medium";
    if (lowered === "large") return "Large";
    return null;
}

export function normalizeWsEndpointValue(value, fallback = "localhost:24050") {
    if (typeof value !== "string") {
        return fallback;
    }

    let normalized = value.trim();
    if (!normalized) {
        return fallback;
    }

    normalized = normalized.replace(/^(wss?:\/\/|https?:\/\/)/i, "");
    const slashIndex = normalized.indexOf("/");
    if (slashIndex >= 0) {
        normalized = normalized.slice(0, slashIndex);
    }

    normalized = normalized.trim();
    return normalized || fallback;
}

export function normalizeDiffTextValue(value) {
    if (typeof value !== "string") {
        return null;
    }

    const lowered = value.trim().toLowerCase();
    if (lowered === "none") return "None";
    if (lowered === "msd") return "MSD";
    if (lowered === "pattern") return "Pattern";
    if (lowered === "reworksr") return "ReworkSR";
    if (lowered === "interludesr") return "InterludeSR";
    if (lowered === "graph") return "Graph";
    if (lowered === "difficulty") return "Difficulty";
    return null;
}

export function normalizeBooleanSetting(value, fallback = false) {
    if (typeof value === "boolean") {
        return value;
    }
    if (typeof value === "number") {
        return value !== 0;
    }
    if (typeof value === "string") {
        const normalized = value.trim().toLowerCase();
        if (normalized === "true") return true;
        if (normalized === "false") return false;
        if (normalized === "1") return true;
        if (normalized === "0") return false;
    }
    if (value === undefined || value === null) {
        return fallback;
    }
    return Boolean(value);
}

export function extractSettingValue(settingsPayload, settingKey) {
    if (Array.isArray(settingsPayload)) {
        const item = settingsPayload.find((entry) => entry?.uniqueID === settingKey);
        return item?.value;
    }

    if (settingsPayload && typeof settingsPayload === "object") {
        if (Object.prototype.hasOwnProperty.call(settingsPayload, settingKey)) {
            return settingsPayload[settingKey];
        }

        if (settingsPayload.settings && typeof settingsPayload.settings === "object") {
            const nested = settingsPayload.settings;
            if (Object.prototype.hasOwnProperty.call(nested, settingKey)) {
                return nested[settingKey];
            }
        }
    }

    return undefined;
}

export function createSettingsParsers(appConfig) {
    const contentBarSet = createSet(appConfig?.options?.contentBar);
    const srTextSet = createSet(appConfig?.options?.srText);
    const diffTextSet = createSet(appConfig?.options?.diffText);
    const estimatorAlgorithmSet = createSet(appConfig?.options?.estimatorAlgorithm);
    const etternaVersionSet = createSet(appConfig?.options?.etternaVersion);
    const companellaEtternaVersionSet = createSet(
        appConfig?.options?.companellaEtternaVersion || appConfig?.options?.etternaVersion,
    );
    const cardOpacitySet = createSet(appConfig?.options?.cardOpacity);
    const cardBlurSet = createSet(appConfig?.options?.cardBlur);
    const cardRadiusSet = createSet(appConfig?.options?.cardRadius);

    function parseEnablePatternValue(settingsPayload) {
        if (Array.isArray(settingsPayload)) {
            const item = settingsPayload.find((entry) => entry?.uniqueID === "enablePatternAnalysis");
            return normalizeBooleanSetting(item?.value, false);
        }

        if (settingsPayload && typeof settingsPayload === "object") {
            if (Object.prototype.hasOwnProperty.call(settingsPayload, "enablePatternAnalysis")) {
                return normalizeBooleanSetting(settingsPayload.enablePatternAnalysis, false);
            }

            if (settingsPayload.settings && typeof settingsPayload.settings === "object") {
                const nested = settingsPayload.settings;
                if (Object.prototype.hasOwnProperty.call(nested, "enablePatternAnalysis")) {
                    return normalizeBooleanSetting(nested.enablePatternAnalysis, false);
                }
            }
        }

        return false;
    }

    function parseContentBarValue(settingsPayload) {
        const value = extractSettingValue(settingsPayload, "contentBar");
        const normalized = normalizeContentBarValue(value);
        if (normalized && contentBarSet.has(normalized.toLowerCase())) {
            return normalized;
        }

        const enabled = parseEnablePatternValue(settingsPayload);
        return enabled ? "Pattern" : "None";
    }

    function parseSrTextValue(settingsPayload) {
        const value = extractSettingValue(settingsPayload, "srText");
        const normalized = normalizeSrTextValue(value);
        if (normalized && srTextSet.has(normalized.toLowerCase())) {
            return normalized;
        }
        return appConfig.defaults.srText;
    }

    function parseDebugUseAmountValue(settingsPayload) {
        const value = extractSettingValue(settingsPayload, "debugUseAmount");
        return normalizeBooleanSetting(value, appConfig.defaults.debugUseAmount);
    }

    function parseDiffTextValue(settingsPayload) {
        const value = extractSettingValue(settingsPayload, "diffText");
        const normalized = normalizeDiffTextValue(value);
        if (normalized && diffTextSet.has(normalized.toLowerCase())) {
            return normalized;
        }

        const legacyEnable = extractSettingValue(settingsPayload, "enableEstDiff");
        if (legacyEnable === undefined || legacyEnable === null) {
            return appConfig.defaults.diffText;
        }
        return normalizeBooleanSetting(legacyEnable, true) ? "Difficulty" : "None";
    }

    function parseAutoModeValue(settingsPayload) {
        const value = extractSettingValue(settingsPayload, "autoMode");
        return normalizeBooleanSetting(value, appConfig.defaults.autoMode);
    }

    function parseUseDanielAlgorithmValue(settingsPayload) {
        const estimator = parseEstimatorAlgorithmValue(settingsPayload);
        return estimator === "Daniel";
    }

    function parseEstimatorAlgorithmValue(settingsPayload) {
        const value = extractSettingValue(settingsPayload, "estimatorAlgorithm");
        const normalized = normalizeEstimatorAlgorithmValue(value);
        if (normalized && estimatorAlgorithmSet.has(normalized.toLowerCase())) {
            return normalized;
        }

        const legacyValue = extractSettingValue(settingsPayload, "useDanielAlgorithm");
        if (legacyValue !== undefined && legacyValue !== null) {
            return normalizeBooleanSetting(legacyValue, false) ? "Daniel" : "Sunny";
        }

        const fallback = normalizeEstimatorAlgorithmValue(appConfig.defaults.estimatorAlgorithm);
        if (fallback && estimatorAlgorithmSet.has(fallback.toLowerCase())) {
            return fallback;
        }

        return "Sunny";
    }

    function parseEtternaVersionValue(settingsPayload) {
        const value = extractSettingValue(settingsPayload, "etternaVersion");
        const normalized = normalizeEtternaVersionValue(value);
        if (normalized && etternaVersionSet.has(normalized.toLowerCase())) {
            return normalized;
        }

        const fallback = normalizeEtternaVersionValue(appConfig.defaults.etternaVersion);
        if (fallback && etternaVersionSet.has(fallback.toLowerCase())) {
            return fallback;
        }

        return appConfig.defaults.etternaVersion;
    }

    function parseCompanellaEtternaVersionValue(settingsPayload) {
        const value = extractSettingValue(settingsPayload, "companellaEtternaVersion");
        const normalized = normalizeEtternaVersionValue(value);
        if (normalized && companellaEtternaVersionSet.has(normalized.toLowerCase())) {
            return normalized;
        }

        const fallback = normalizeEtternaVersionValue(appConfig.defaults.companellaEtternaVersion);
        if (fallback && companellaEtternaVersionSet.has(fallback.toLowerCase())) {
            return fallback;
        }

        return appConfig.defaults.companellaEtternaVersion;
    }

    function parseEnablePauseDetectionValue(settingsPayload) {
        const value = extractSettingValue(settingsPayload, "enablePauseDetection");
        return normalizeBooleanSetting(value, appConfig.defaults.pauseDetectionEnabled);
    }

    function parseDisableVibroDetectionValue(settingsPayload) {
        return !parseVibroDetectionValue(settingsPayload);
    }

    function parseVibroDetectionValue(settingsPayload) {
        const value = extractSettingValue(settingsPayload, "VibroDetection");
        if (value !== undefined && value !== null) {
            return normalizeBooleanSetting(value, appConfig.defaults.vibroDetection);
        }

        const legacyValue = extractSettingValue(settingsPayload, "disableVibroDetection");
        if (legacyValue !== undefined && legacyValue !== null) {
            return !normalizeBooleanSetting(legacyValue, appConfig.defaults.disableVibroDetection);
        }

        return normalizeBooleanSetting(appConfig.defaults.vibroDetection, true);
    }

    function parseEnableEtternaRainbowBarsValue(settingsPayload) {
        const value = extractSettingValue(settingsPayload, "enableEtternaRainbowBars");
        return normalizeBooleanSetting(value, appConfig.defaults.enableEtternaRainbowBars);
    }

    function parseEnableStatusMarqueeValue(settingsPayload) {
        const value = extractSettingValue(settingsPayload, "enableStatusMarquee");
        return normalizeBooleanSetting(value, appConfig.defaults.enableStatusMarquee);
    }

    function parseShowModeTagCapsuleValue(settingsPayload) {
        const value = extractSettingValue(settingsPayload, "showModeTagCapsule");
        return normalizeBooleanSetting(value, appConfig.defaults.showModeTagCapsule);
    }

    function parseEnableNumericDifficultyValue(settingsPayload) {
        const value = extractSettingValue(settingsPayload, "enableNumericDifficulty");
        return normalizeBooleanSetting(value, appConfig.defaults.enableNumericDifficulty);
    }

    function parseHideCardDuringPlayValue(settingsPayload) {
        const value = extractSettingValue(settingsPayload, "hideCardDuringPlay");
        return normalizeBooleanSetting(value, appConfig.defaults.hideCardDuringPlay);
    }

    function parseCardOpacityValue(settingsPayload) {
        const value = extractSettingValue(settingsPayload, "cardOpacity");
        const normalized = normalizeCardOpacityValue(value);
        if (normalized && cardOpacitySet.has(normalized.toLowerCase())) {
            return normalized;
        }

        const fallback = normalizeCardOpacityValue(appConfig.defaults.cardOpacity);
        if (fallback && cardOpacitySet.has(fallback.toLowerCase())) {
            return fallback;
        }

        return appConfig.defaults.cardOpacity;
    }

    function parseCardBlurValue(settingsPayload) {
        const value = extractSettingValue(settingsPayload, "cardBlur");
        const normalized = normalizeCardBlurValue(value);
        if (normalized && cardBlurSet.has(normalized.toLowerCase())) {
            return normalized;
        }

        const fallback = normalizeCardBlurValue(appConfig.defaults.cardBlur);
        if (fallback && cardBlurSet.has(fallback.toLowerCase())) {
            return fallback;
        }

        return appConfig.defaults.cardBlur;
    }

    function parseCardRadiusValue(settingsPayload) {
        const value = extractSettingValue(settingsPayload, "cardRadius");
        const normalized = normalizeCardRadiusValue(value);
        if (normalized && cardRadiusSet.has(normalized.toLowerCase())) {
            return normalized;
        }

        const fallback = normalizeCardRadiusValue(appConfig.defaults.cardRadius);
        if (fallback && cardRadiusSet.has(fallback.toLowerCase())) {
            return fallback;
        }

        return appConfig.defaults.cardRadius;
    }

    function parseShowTitleIconValue(settingsPayload) {
        const value = extractSettingValue(settingsPayload, "showTitleIcon");
        return normalizeBooleanSetting(value, appConfig.defaults.showTitleIcon);
    }

    function parseReverseCardExtendDirectionValue(settingsPayload) {
        const value = extractSettingValue(settingsPayload, "reverseCardExtendDirection");
        return normalizeBooleanSetting(value, appConfig.defaults.reverseCardExtendDirection);
    }

    function parseSvDetectionValue(settingsPayload) {
        const value = extractSettingValue(settingsPayload, "debugUseSvDetection");
        return normalizeBooleanSetting(value, appConfig.defaults.svDetection);
    }

    function parseWsEndpointValue(settingsPayload) {
        const wsEndpointValue = extractSettingValue(settingsPayload, "wsEndpoint");
        const legacyWsHostValue = extractSettingValue(settingsPayload, "wsHost");
        const fallbackHost = appConfig.defaults.wsEndpoint || appConfig.socketHost;

        if (wsEndpointValue !== undefined && wsEndpointValue !== null) {
            return normalizeWsEndpointValue(wsEndpointValue, fallbackHost);
        }

        if (legacyWsHostValue !== undefined && legacyWsHostValue !== null) {
            return normalizeWsEndpointValue(legacyWsHostValue, fallbackHost);
        }

        return normalizeWsEndpointValue(fallbackHost, "localhost:24050");
    }

    return {
        parseEnablePatternValue,
        parseContentBarValue,
        parseSrTextValue,
        parseDebugUseAmountValue,
        parseDiffTextValue,
        parseAutoModeValue,
        parseUseDanielAlgorithmValue,
        parseEstimatorAlgorithmValue,
        parseEtternaVersionValue,
        parseCompanellaEtternaVersionValue,
        parseEnablePauseDetectionValue,
        parseDisableVibroDetectionValue,
        parseVibroDetectionValue,
        parseEnableEtternaRainbowBarsValue,
        parseEnableStatusMarqueeValue,
        parseShowModeTagCapsuleValue,
        parseEnableNumericDifficultyValue,
        parseHideCardDuringPlayValue,
        parseCardOpacityValue,
        parseCardBlurValue,
        parseCardRadiusValue,
        parseShowTitleIconValue,
        parseReverseCardExtendDirectionValue,
        parseSvDetectionValue,
        parseWsEndpointValue,
    };
}
