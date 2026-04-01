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
    return null;
}

export function normalizeEstimatorAlgorithmValue(value) {
    if (typeof value !== "string") {
        return null;
    }

    const lowered = value.trim().toLowerCase();
    if (lowered === "rework") return "Rework";
    if (lowered === "direct") return "Rework";
    if (lowered === "daniel") return "Daniel";
    return null;
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
            return normalizeBooleanSetting(legacyValue, false) ? "Daniel" : "Rework";
        }

        const fallback = normalizeEstimatorAlgorithmValue(appConfig.defaults.estimatorAlgorithm);
        if (fallback && estimatorAlgorithmSet.has(fallback.toLowerCase())) {
            return fallback;
        }

        return "Rework";
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

    function parseShowModeTagCapsuleValue(settingsPayload) {
        const value = extractSettingValue(settingsPayload, "showModeTagCapsule");
        return normalizeBooleanSetting(value, appConfig.defaults.showModeTagCapsule);
    }

    function parseSvDetectionValue(settingsPayload) {
        const value = extractSettingValue(settingsPayload, "debugUseSvDetection");
        return normalizeBooleanSetting(value, appConfig.defaults.svDetection);
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
        parseEnablePauseDetectionValue,
        parseDisableVibroDetectionValue,
        parseVibroDetectionValue,
        parseEnableEtternaRainbowBarsValue,
        parseShowModeTagCapsuleValue,
        parseSvDetectionValue,
    };
}
