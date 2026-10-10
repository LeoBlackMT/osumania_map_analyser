/**
 * Lightweight, zero-dependency i18n management for ManiaMapAnalyser.
 * Supports dynamic locale switching, fallback resolution, and settings schema translation.
 */

import en from "./locales/en.js";
import zhCN from "./locales/zh-CN.js";

export const SUPPORTED_LOCALES = [
    { code: "zh-CN", label: "中文" },
    { code: "en", label: "English" },
];

const LOCALES = {
    "zh-CN": zhCN,
    "en": en,
};

const STORAGE_KEY = "mma_settings_locale";
const listeners = new Set();

let currentLocale = "zh-CN";

/**
 * Resolves dot-separated key path in a dictionary.
 */
function resolvePath(obj, path) {
    if (!obj || typeof obj !== "object") return undefined;
    const parts = path.split(".");
    let curr = obj;
    for (const part of parts) {
        if (curr == null || typeof curr !== "object") return undefined;
        curr = curr[part];
    }
    return curr;
}

/**
 * Initializes locale resolution:
 * 1. URL search param (`?lang=zh` or `?lang=en` or `?lang=zh-CN`)
 * 2. LocalStorage cache (`mma_settings_locale`)
 * 3. Browser language (`navigator.language`)
 * 4. Default: "zh-CN"
 */
export function initLocale() {
    try {
        const urlParam = new URLSearchParams(window.location.search).get("lang");
        if (urlParam) {
            const lower = urlParam.toLowerCase();
            if (lower === "en") {
                currentLocale = "en";
                return currentLocale;
            }
            if (lower.startsWith("zh")) {
                currentLocale = "zh-CN";
                return currentLocale;
            }
        }
        const cached = localStorage.getItem(STORAGE_KEY);
        if (cached && LOCALES[cached]) {
            currentLocale = cached;
            return currentLocale;
        }
        if (typeof navigator !== "undefined" && typeof navigator.language === "string") {
            currentLocale = navigator.language.toLowerCase().startsWith("zh") ? "zh-CN" : "en";
            return currentLocale;
        }
    } catch (_) {
        // Fallback silently if localStorage or navigator is inaccessible
    }
    currentLocale = "zh-CN";
    return currentLocale;
}

/**
 * Returns currently active locale code ("zh-CN" or "en").
 */
export function getLocale() {
    return currentLocale;
}

/**
 * Switches current locale and notifies all subscribers.
 */
export function setLocale(locale) {
    if (!LOCALES[locale] || locale === currentLocale) {
        return;
    }
    currentLocale = locale;
    try {
        localStorage.setItem(STORAGE_KEY, locale);
    } catch (_) {}

    try {
        if (typeof document !== "undefined" && document.documentElement) {
            document.documentElement.lang = locale === "zh-CN" ? "zh-Hans" : "en";
        }
    } catch (_) {}

    for (const listener of listeners) {
        try {
            listener(currentLocale);
        } catch (err) {
            console.error("i18n listener error:", err);
        }
    }
}

/**
 * Registers a locale change listener.
 * Returns an unsubscription function.
 */
export function onLocaleChange(fn) {
    listeners.add(fn);
    return () => listeners.delete(fn);
}

/**
 * Translates a key path (e.g. "shell.title" or "nav.cardSettings").
 * Falls back to English, then to provided fallback, then to the key itself.
 */
export function t(path, fallback = "") {
    const val = resolvePath(LOCALES[currentLocale], path);
    if (typeof val === "string") {
        return val;
    }
    const enVal = resolvePath(LOCALES["en"], path);
    if (typeof enVal === "string") {
        return enVal;
    }
    return fallback || path;
}

/**
 * Translates a setting entry from settings.json.
 * Returns an entry copy with translated title, description, and optional optionLabels.
 */
export function translateEntry(entry) {
    if (!entry || !entry.uniqueID) {
        return entry;
    }
    const key = entry.uniqueID;
    const trans = LOCALES[currentLocale]?.cardSettings?.[key] || LOCALES["en"]?.cardSettings?.[key];
    if (!trans) {
        return entry;
    }

    return {
        ...entry,
        title: trans.title || entry.title,
        description: trans.description != null ? trans.description : entry.description,
        optionLabels: trans.options || null,
    };
}

/**
 * Translates a header entry (e.g. hLinks, hTheme) from settings.json.
 */
export function translateHeader(header) {
    if (!header || !header.uniqueID) {
        return header;
    }
    const key = header.uniqueID;
    const headerTitle = LOCALES[currentLocale]?.headers?.[key] || LOCALES["en"]?.headers?.[key];
    if (headerTitle) {
        return {
            ...header,
            title: headerTitle,
        };
    }
    return header;
}
