// js/app/i18n/index.js
// 壳内页面（settings.html / presets.html）的极简 i18n：词典 + t(key, fallback)。
//
// 设计约束：
//   - 插件没有构建系统，不能引入需要打包的 i18n 库；
//   - 英文原文留在代码里（作为 fallback），中文只存在于 zh-CN.js，避免同一句话维护两份；
//   - settings.json 的标题/描述必须保持英文（tosu 面板不支持中文），因此这里只用
//     `settings.<uniqueID>.title|description` 覆盖显示，缺译时自动回落英文；
//   - 当前语言可从 ?lang= / 壳配置（mma-shell-config.json 的 language）/ localStorage /
//     浏览器语言依次推导。
import zhCN from "./zh-CN.js";

const STORAGE_KEY = "mma.lang";
const HTML_LANG = Object.freeze({ "en-US": "en", "zh-CN": "zh-CN" });
const DICTS = Object.freeze({ "zh-CN": zhCN });
const listeners = new Set();

let current = "en-US";

export function supportedLanguages() {
    return ["en-US", "zh-CN"];
}

/** "zh", "zh-CN", "zh_CN", "en-GB" → 受支持的语言；"auto"/空/未知 → ""。 */
export function normalizeLanguage(value) {
    const text = String(value ?? "").trim().toLowerCase();
    if (!text || text === "auto") {
        return "";
    }
    if (text === "zh" || text.startsWith("zh-") || text.startsWith("zh_")) {
        return "zh-CN";
    }
    if (text === "en" || text.startsWith("en-") || text.startsWith("en_")) {
        return "en-US";
    }
    return "";
}

/** 按优先级推导语言：URL 覆盖 > 壳配置 > 本地存储 > 浏览器语言。 */
export function resolveLanguage(candidates = []) {
    for (const candidate of candidates) {
        const normalized = normalizeLanguage(candidate);
        if (normalized) {
            return normalized;
        }
    }
    return "en-US";
}

export function getLanguage() {
    return current;
}

function readQueryOverride() {
    try {
        return new URLSearchParams(window.location.search).get("lang") || "";
    } catch {
        return "";
    }
}

function readStored() {
    try {
        return window.localStorage.getItem(STORAGE_KEY) || "";
    } catch {
        return "";
    }
}

function writeStored(language) {
    try {
        window.localStorage.setItem(STORAGE_KEY, language);
    } catch {
        // 隐私模式等场景下写不进去：忽略，本次会话仍然生效。
    }
}

function applyDocumentLanguage() {
    if (typeof document === "undefined") {
        return;
    }
    document.documentElement.lang = HTML_LANG[current] || "en";
    if (document.title && document.documentElement.dataset.mmaTitleKey) {
        document.title = t(document.documentElement.dataset.mmaTitleKey, document.title);
    }
}

/** 启动时调用：确定初始语言（不再通知订阅者）。 */
export function initLanguage({ shellLanguage = "" } = {}) {
    current = resolveLanguage([
        readQueryOverride(),
        shellLanguage,
        readStored(),
        typeof navigator !== "undefined" ? navigator.language : "",
    ]);
    applyDocumentLanguage();
    return current;
}

/** 切换语言：写 localStorage 并通知订阅者重渲染。 */
export function setLanguage(language) {
    const next = normalizeLanguage(language) || "en-US";
    if (next === current) {
        return current;
    }
    current = next;
    writeStored(next);
    applyDocumentLanguage();
    for (const listener of listeners) {
        try {
            listener(next);
        } catch (error) {
            console.error("i18n listener failed", error);
        }
    }
    return current;
}

export function onLanguageChange(listener) {
    listeners.add(listener);
    return () => listeners.delete(listener);
}

/**
 * 取词：中文词典命中则用中文，否则用调用处传入的英文原文。
 * @param {string} key
 * @param {string} [fallback] 英文原文（缺省时返回 key 本身，便于发现漏配）
 */
export function t(key, fallback) {
    const dict = DICTS[current];
    const hit = dict && Object.prototype.hasOwnProperty.call(dict, key) ? dict[key] : "";
    if (hit) {
        return hit;
    }
    return fallback ?? key;
}

/**
 * 带占位符的取词：模板里的 `{name}` 用 params 替换。
 * @param {string} key
 * @param {Record<string, unknown>} params
 * @param {string} [fallback]
 */
export function tf(key, params, fallback) {
    return t(key, fallback).replace(/\{(\w+)\}/g, (whole, name) => (
        params && Object.prototype.hasOwnProperty.call(params, name) ? String(params[name]) : whole
    ));
}

/** 把带 data-i18n / data-i18n-title / data-i18n-aria 的静态节点刷成当前语言。 */
export function applyStaticText(root = typeof document !== "undefined" ? document : null) {
    if (!root || typeof root.querySelectorAll !== "function") {
        return;
    }
    for (const el of root.querySelectorAll("[data-i18n]")) {
        const key = el.dataset.i18n;
        el.textContent = t(key, el.dataset.i18nFallback || el.textContent);
    }
    for (const el of root.querySelectorAll("[data-i18n-title]")) {
        el.title = t(el.dataset.i18nTitle, el.dataset.i18nTitleFallback || el.title);
    }
    for (const el of root.querySelectorAll("[data-i18n-aria]")) {
        el.setAttribute("aria-label", t(el.dataset.i18nAria, el.getAttribute("aria-label") || ""));
    }
    for (const el of root.querySelectorAll("[data-i18n-placeholder]")) {
        el.placeholder = t(el.dataset.i18nPlaceholder, el.placeholder);
    }
}
