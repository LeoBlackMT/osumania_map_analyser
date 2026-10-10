/**
 * Desktop settings page (settings.html) — plugin settings, shell configuration
 * and the offline preset panel.
 *
 * The page is served by the desktop shell on http://127.0.0.1:24061/settings.html
 * and talks to the same origin only. While tosu is offline the shell owns the
 * settings file and this page is writable; the moment tosu is online the page
 * turns read-only (tosu owns the file then, and POST /settings answers 403).
 *
 * Bootstrap order is fixed (plan Step 9):
 *   1. origin guard — anything that is not the shell's own origin gets a notice;
 *   2. preset transport injection — must happen BEFORE initPresets();
 *   3. bridge client — `hello` is the first-frame read-only authority;
 *   4. GET /settings → applySnapshot(values) → state.wsEndpoint → render
 *      (form, shell-config panel, read-only view, dashboard URL);
 *   5. initPresets() → loadBuiltinPresets() → applyPresetsView();
 *   6. transport subscription — pushed changes refresh form + views + presets.
 *
 * Settings reach `state` through applySnapshot() (per-key try/catch) — never
 * through the overlay's whole-payload settings applier, which assumes the card
 * DOM this page does not have.
 */

import { state } from "../appContext.js";
import { applySnapshot } from "../presets/snapshot.js";
import { initPresets, setPresetTransport } from "../presets/core.js";
import { loadBuiltinPresets } from "../presets/builtin.js";
import { loadSettingsSchema } from "../presets/schema.js";
import { initBridgeClient } from "../sources/bridgeClient.js";
import { applyShellState as applyShellStateFrame } from "../sources/shellState.js";
import { LINK_COPIED_NOTICE } from "../externalLink.js";
import { createShellPresetTransport } from "./shellTransport.js";
import { createSettingsForm } from "./settingsForm.js";
import { createSettingsLinks } from "./settingsLinks.js";
import { createShellConfigPanel } from "./shellConfigPanel.js";
import { initLocale, getLocale, setLocale, onLocaleChange, t } from "../i18n/index.js";

/** Port the desktop shell serves the plugin directory on. */
const SHELL_PORT = "24061";
/** Hosts a shell window can legitimately use. */
const LOOPBACK_HOSTS = new Set(["localhost", "127.0.0.1", "::1", "[::1]"]);
/** Fallback dashboard endpoint when the settings carry no wsEndpoint. */
const FALLBACK_ENDPOINT = "localhost:24050";
/** settings.json entry whose value points at the tosu presets page. */
const PRESETS_BUTTON_KEY = "PresetButton";

function getShellUnavailableNotice() {
    return t("status.shellUnavailable", "Cannot reach the shell (24061)");
}
function getShellOnlineNotice() {
    return t("status.shellOnline", "tosu is connected — settings are read-only here.");
}
function getShellOnlinePresetsNotice() {
    return t("status.shellOnlinePresets", "tosu is connected — settings are read-only here. "
        + "Manage presets from the Presets page of your tosu instance.");
}
function getOriginNotice() {
    return t("status.originNotice", "Open this page from the desktop shell: http://127.0.0.1:24061/settings.html");
}

const statusBarEl = document.getElementById("settings-status");
const linksRootEl = document.getElementById("settings-links-root");
const navRootEl = document.getElementById("settings-nav-root");
const settingsRootEl = document.getElementById("settings-root");
const shellConfigRootEl = document.getElementById("shell-config-root");
const presetsAppEl = document.getElementById("presets-app");

let schema = null;
let form = null;
let links = null;
let shellConfigPanel = null;
let pageTransport = null;
let settingsFormEl = null;
let readOnlyEl = null;
let dashboardUrlEl = null;
let presetsNoticeEl = null;
let toastRootEl = null;
// manager.js is imported on first need and evaluates exactly once; the flag also
// keeps two concurrent applyPresetsView() calls from importing it twice.
let presetsViewReady = false;
// Settings committed from the form and not yet confirmed by the shell broadcast.
const pendingWrites = new Map();

// ---------------------------------------------------------------------------
// Status bar / clipboard
// ---------------------------------------------------------------------------

function setStatus(message, kind = "info") {
    if (!statusBarEl) {
        return;
    }
    statusBarEl.textContent = message;
    statusBarEl.className = `settings-status settings-status-${kind}`;
}

/** Bottom-right toast (the status bar stays reserved for save-state messages). */
function showToast(message) {
    if (!toastRootEl) {
        toastRootEl = document.createElement("div");
        toastRootEl.id = "settings-toast";
        toastRootEl.className = "settings-toast-container";
        document.body.appendChild(toastRootEl);
    }
    const toast = document.createElement("div");
    toast.className = "settings-toast";
    toast.textContent = message;
    toastRootEl.appendChild(toast);
    setTimeout(() => toast.classList.add("show"), 10);
    setTimeout(() => toast.remove(), 4000);
}

function attachCopyButton(container, getText) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "settings-copy-btn";
    button.textContent = "Copy";
    button.addEventListener("click", async () => {
        const ok = await copyText(String(getText() || ""));
        button.textContent = ok ? "Copied" : "Copy failed";
        setTimeout(() => {
            button.textContent = "Copy";
        }, 1500);
    });
    container.appendChild(button);
}

async function copyText(text) {
    try {
        if (navigator.clipboard && typeof navigator.clipboard.writeText === "function") {
            await navigator.clipboard.writeText(text);
            return true;
        }
    } catch {
        // Clipboard API unavailable/denied — fall through to the textarea path.
    }
    try {
        const area = document.createElement("textarea");
        area.value = text;
        area.setAttribute("readonly", "readonly");
        area.style.position = "fixed";
        area.style.opacity = "0";
        document.body.appendChild(area);
        area.select();
        const ok = typeof document.execCommand === "function" ? document.execCommand("copy") : false;
        area.remove();
        return ok === true;
    } catch {
        return false;
    }
}

// ---------------------------------------------------------------------------
// Endpoints shown to the user (dashboard + tosu presets page)
// ---------------------------------------------------------------------------

function endpointValue() {
    const raw = typeof state.wsEndpoint === "string" ? state.wsEndpoint.trim() : "";
    return raw || FALLBACK_ENDPOINT;
}

function dashboardUrl() {
    const endpoint = endpointValue().replace(/^https?:\/\//i, "").replace(/\/+$/, "");
    return `http://${endpoint}/`;
}

/** settings.json button entry → URL; PresetButton's host:port is the tosu endpoint. */
function buttonUrl(entry) {
    const raw = entry && typeof entry.value === "string" ? entry.value.trim() : "";
    if (!raw) {
        return null;
    }
    if (entry.uniqueID !== PRESETS_BUTTON_KEY) {
        return raw;
    }
    const endpoint = endpointValue().replace(/^https?:\/\//i, "").replace(/\/+$/, "");
    return /^https?:\/\//i.test(raw) ? raw.replace(/^https?:\/\/[^/]+/i, `http://${endpoint}`) : raw;
}

/** settings.json PresetButton URL with its host:port rewritten to wsEndpoint. */
function presetsPageUrl() {
    const entry = schema && Array.isArray(schema.entries)
        ? schema.entries.find((item) => item && item.uniqueID === PRESETS_BUTTON_KEY)
        : null;
    return buttonUrl(entry);
}

// ---------------------------------------------------------------------------
// 1) Origin guard
// ---------------------------------------------------------------------------

function isShellOrigin() {
    return typeof location !== "undefined"
        && location.port === SHELL_PORT
        && LOOPBACK_HOSTS.has(String(location.hostname || "").toLowerCase());
}

function renderOriginNotice() {
    const notice = document.createElement("p");
    notice.className = "settings-origin-notice";
    notice.textContent = getOriginNotice();
    if (settingsRootEl) {
        settingsRootEl.appendChild(notice);
    }
    setStatus(getOriginNotice(), "error");
    document.title = t("originPageTitle", "Mania Map Analyser — Settings (desktop shell only)");
}

// ---------------------------------------------------------------------------
// 4) Layout: nav bar, read-only view, settings form section
// ---------------------------------------------------------------------------

function buildLayout() {
    if (navRootEl) {
        populateNav(navRootEl);
    }
    settingsRootEl.textContent = "";
    readOnlyEl = buildReadOnlyView();
    settingsRootEl.appendChild(readOnlyEl);

    const section = document.createElement("section");
    section.id = "settings-form-section";
    section.className = "settings-panel";

    const title = document.createElement("h2");
    title.id = "settings-form-title";
    title.textContent = t("nav.cardSettings", "Card Settings");
    section.appendChild(title);

    const sub = document.createElement("p");
    sub.id = "settings-form-sub";
    sub.className = "settings-panel-sub";
    sub.textContent = t("status.cardSettingsSub", "The desktop shell keeps these values in mma-settings.json while tosu is offline; "
        + "the overlay picks up every change immediately.");
    section.appendChild(sub);

    settingsFormEl = document.createElement("div");
    settingsFormEl.className = "settings-form";
    section.appendChild(settingsFormEl);

    settingsRootEl.appendChild(section);
}

function updateNavLabels() {
    const shellLink = document.getElementById("nav-shell-config");
    const cardLink = document.getElementById("nav-card-settings");
    const presetsLink = document.getElementById("nav-presets-app");
    if (shellLink) shellLink.textContent = t("nav.shellConfig", "Shell Configuration");
    if (cardLink) cardLink.textContent = t("nav.cardSettings", "Card Settings");
    if (presetsLink) presetsLink.textContent = t("nav.presetsManager", "Presets Manager");
}

function updateLangButtons(activeLocale) {
    const buttons = document.querySelectorAll(".settings-lang-btn");
    buttons.forEach((btn) => {
        const isActive = btn.dataset.lang === activeLocale;
        btn.classList.toggle("active", isActive);
        btn.setAttribute("aria-checked", isActive ? "true" : "false");
    });
}

let langSwitcherBound = false;
function setupLangSwitcher() {
    if (!langSwitcherBound) {
        langSwitcherBound = true;
        const buttons = document.querySelectorAll(".settings-lang-btn");
        buttons.forEach((btn) => {
            btn.addEventListener("click", () => {
                const lang = btn.dataset.lang;
                if (lang && lang !== getLocale()) {
                    setLocale(lang);
                }
            });
        });
    }
    updateLangButtons(getLocale());
}

function updatePageTitle() {
    document.title = t("pageTitle", "Mania Map Analyser — Settings");
    document.documentElement.lang = getLocale();
}

function updateLayoutText() {
    const titleEl = document.getElementById("settings-form-title");
    if (titleEl) titleEl.textContent = t("nav.cardSettings", "Card Settings");
    const subEl = document.getElementById("settings-form-sub");
    if (subEl) subEl.textContent = t("status.cardSettingsSub", "The desktop shell keeps these values in mma-settings.json while tosu is offline; "
        + "the overlay picks up every change immediately.");

    if (readOnlyEl) {
        const msgEl = readOnlyEl.querySelector(".settings-readonly-message");
        if (msgEl) msgEl.textContent = getShellOnlineNotice();
        const lblEl = readOnlyEl.querySelector(".settings-readonly-label");
        if (lblEl) lblEl.textContent = t("status.tosuDashboard", "tosu dashboard:");
    }

    if (presetsNoticeEl) {
        const textEl = presetsNoticeEl.querySelector(".settings-presets-notice-text");
        if (textEl) textEl.textContent = getShellOnlinePresetsNotice();
    }
}

function populateNav(nav) {
    const shellLink = document.getElementById("nav-shell-config");
    if (!shellLink) {
        nav.textContent = "";
        for (const [href, labelKey, fallback, id] of [
            ["#shell-config-section", "nav.shellConfig", "Shell Configuration", "nav-shell-config"],
            ["#settings-form-section", "nav.cardSettings", "Card Settings", "nav-card-settings"],
            ["#presets-app", "nav.presetsManager", "Presets Manager", "nav-presets-app"],
        ]) {
            const link = document.createElement("a");
            link.className = "settings-nav-link";
            link.href = href;
            link.id = id;
            link.textContent = t(labelKey, fallback);
            nav.appendChild(link);
        }
        const switcher = document.createElement("div");
        switcher.id = "settings-lang-switcher";
        switcher.className = "settings-lang-switcher";
        switcher.setAttribute("role", "radiogroup");
        switcher.setAttribute("aria-label", "Language");
        for (const [lang, text] of [["zh-CN", "中"], ["en", "EN"]]) {
            const btn = document.createElement("button");
            btn.type = "button";
            btn.className = "settings-lang-btn";
            btn.dataset.lang = lang;
            btn.textContent = text;
            switcher.appendChild(btn);
        }
        nav.appendChild(switcher);
    }
    updateNavLabels();
    setupLangSwitcher();
    setupNavScrollspy(nav);
}

let scrollspyBound = false;
function setupNavScrollspy(nav) {
    function updateActive() {
        const links = Array.from(nav.querySelectorAll(".settings-nav-link"));
        if (!links.length) return;
        if (window.innerHeight + window.scrollY >= document.documentElement.scrollHeight - 50) {
            links.forEach((link, idx) => {
                link.classList.toggle("active", idx === links.length - 1);
            });
            return;
        }
        const scrollY = window.scrollY + 140;
        let activeIdx = 0;
        for (let i = 0; i < links.length; i++) {
            const id = (links[i].getAttribute("href") || "").replace("#", "");
            const target = id ? document.getElementById(id) : null;
            if (target && target.getBoundingClientRect().top + window.scrollY <= scrollY) {
                activeIdx = i;
            }
        }
        links.forEach((link, idx) => {
            link.classList.toggle("active", idx === activeIdx);
        });
    }

    if (!scrollspyBound) {
        scrollspyBound = true;
        window.addEventListener("scroll", updateActive, { passive: true });
        window.addEventListener("resize", updateActive, { passive: true });
    }
    requestAnimationFrame(updateActive);
    setTimeout(updateActive, 300);
    setTimeout(updateActive, 1000);
}

if (navRootEl) {
    setupNavScrollspy(navRootEl);
}

function buildReadOnlyView() {
    const banner = document.createElement("div");
    banner.className = "settings-readonly";
    banner.hidden = true;

    const message = document.createElement("p");
    message.className = "settings-readonly-message";
    message.textContent = getShellOnlineNotice();
    banner.appendChild(message);

    const row = document.createElement("div");
    row.className = "settings-readonly-url";
    const label = document.createElement("span");
    label.className = "settings-readonly-label";
    label.textContent = t("status.tosuDashboard", "tosu dashboard:");
    dashboardUrlEl = document.createElement("code");
    dashboardUrlEl.className = "settings-url";
    dashboardUrlEl.textContent = dashboardUrl();
    row.appendChild(label);
    row.appendChild(dashboardUrlEl);
    attachCopyButton(row, () => dashboardUrlEl.textContent);
    banner.appendChild(row);
    return banner;
}

function refreshReadOnlyView() {
    const online = state.shellTosuOnline === true;
    if (readOnlyEl) {
        readOnlyEl.hidden = !online;
    }
    if (dashboardUrlEl) {
        dashboardUrlEl.textContent = dashboardUrl();
    }
    if (form) {
        form.setReadOnly(online);
    }
}

/**
 * Applies the read-only state. `hello` (bridge) and every state frame are the
 * authoritative sources; the transport's gate reads the same state field, so the
 * pull below either delivers (offline) or just re-bases the next write (online).
 */
function setReadOnly(online) {
    state.shellTosuOnline = Boolean(online);
    refreshReadOnlyView();
    applyPresetsView();
    if (pageTransport) {
        pageTransport.refresh();
    }
}

/** Shell `state` frame → page fields, then re-evaluate everything it gates. */
function applyShellState(payload) {
    applyShellStateFrame(payload);
    refreshReadOnlyView();
    applyPresetsView();
    if (pageTransport) {
        pageTransport.refresh();
    }
}

// ---------------------------------------------------------------------------
// Presets panel visibility (shell persistence path usable or not)
// ---------------------------------------------------------------------------

function ensurePresetsNotice() {
    if (presetsNoticeEl || !presetsAppEl || !presetsAppEl.parentNode) {
        return presetsNoticeEl;
    }
    presetsNoticeEl = document.createElement("div");
    presetsNoticeEl.className = "settings-presets-notice";
    presetsNoticeEl.hidden = true;
    presetsAppEl.parentNode.insertBefore(presetsNoticeEl, presetsAppEl);
    return presetsNoticeEl;
}

function showPresetsNotice() {
    const notice = ensurePresetsNotice();
    if (!notice) {
        return;
    }
    notice.textContent = "";

    const text = document.createElement("p");
    text.className = "settings-presets-notice-text";
    text.textContent = getShellOnlinePresetsNotice();
    notice.appendChild(text);

    const url = presetsPageUrl();
    if (url) {
        const row = document.createElement("div");
        row.className = "settings-presets-notice-url";
        const code = document.createElement("code");
        code.className = "settings-url";
        code.textContent = url;
        row.appendChild(code);
        attachCopyButton(row, () => url);
        notice.appendChild(row);
    }
    notice.hidden = false;
}

function hidePresetsNotice() {
    if (presetsNoticeEl) {
        presetsNoticeEl.hidden = true;
    }
}

async function applyPresetsView() {
    if (state.shellTosuOnline === true) {
        // tosu owns the settings file: presets must be managed on its own page,
        // and manager.js (a tosu-mode editor) must not be imported at all.
        if (presetsAppEl) {
            presetsAppEl.style.display = "none";
        }
        showPresetsNotice();
        return;
    }
    hidePresetsNotice();
    if (presetsAppEl) {
        presetsAppEl.style.display = "";
    }
    if (presetsViewReady) {
        return;
    }
    presetsViewReady = true;
    try {
        await loadBuiltinPresets();
        await import("../presets/manager.js");
    } catch (error) {
        presetsViewReady = false;
        console.error("[settings] presets panel failed to load:", error);
    }
}

// ---------------------------------------------------------------------------
// Form write path
// ---------------------------------------------------------------------------

function isForbiddenError(error) {
    if (error && typeof error.status === "number") {
        return error.status === 403;
    }
    return /\b403\b/.test(String((error && error.message) || ""));
}

async function commitSetting(key, value, previous) {
    if (!pageTransport) {
        return;
    }
    pendingWrites.set(key, { value, previous });
    setStatus(t("status.saving", "Saving…"), "saving");
    try {
        // quiet: this page has no overlay DOM, so missing-element failures are
        // expected here and must not surface as console errors.
        await applySnapshot({ [key]: value }, { quiet: true });
    } catch (error) {
        console.error(`[settings] applying "${key}" failed:`, error);
    }
    if (key === "wsEndpoint") {
        // applySnapshot() skips wsEndpoint on purpose — set it here so the
        // dashboard URL and the overlay connection follow the new value.
        state.wsEndpoint = value;
        refreshReadOnlyView();
    }
    pageTransport.patch({ [key]: value });
}

/** A pushed payload confirms the pending writes whose value it now carries. */
function confirmPendingWrites(values) {
    if (pendingWrites.size === 0) {
        return;
    }
    for (const [key, entry] of [...pendingWrites]) {
        if (values[key] === entry.value) {
            pendingWrites.delete(key);
        }
    }
    if (pendingWrites.size === 0) {
        setStatus(t("status.saved", "Saved."), "ok");
    }
}

function handleWriteError(error) {
    const message = error && error.message ? error.message : String(error);
    if (isForbiddenError(error)) {
        // tosu came online: nothing may be written, and this page turns read-only.
        // Already-read-only pages get the plain notice instead of an error: core.js
        // attempts one library write-back on load, and the shell's 403 is the
        // expected answer there (no user action failed).
        const wasWritable = state.shellTosuOnline !== true;
        pendingWrites.clear();
        setStatus(wasWritable ? `${t("status.saveFailed", "Save failed: ")}${message}` : getShellOnlineNotice(), "error");
        setReadOnly(true);
        return;
    }
    setStatus(`${t("status.saveFailed", "Save failed: ")}${message}`, "error");
    for (const [key, entry] of [...pendingWrites]) {
        pendingWrites.delete(key);
        if (entry.previous === undefined) {
            continue;
        }
        if (key === "wsEndpoint") {
            state.wsEndpoint = entry.previous;
        }
        applySnapshot({ [key]: entry.previous }, { quiet: true }).catch(() => {});
        if (form) {
            form.setValue(key, entry.previous);
        }
    }
    refreshReadOnlyView();
}

/** Step 6: every delivered payload (shell broadcast → pull) updates the page. */
function handlePushedSettings(packet) {
    const values = packet && typeof packet === "object" && packet.message ? packet.message : packet;
    if (!values || typeof values !== "object") {
        return;
    }
    applySnapshot(values, { quiet: true }).catch((error) => {
        console.error("[settings] applying pushed settings failed:", error);
    });
    state.wsEndpoint = values.wsEndpoint ?? state.wsEndpoint;
    if (form) {
        form.refreshValues(values);
    }
    refreshReadOnlyView();
    confirmPendingWrites(values);
    applyPresetsView();
}

// ---------------------------------------------------------------------------
// Bootstrap
// ---------------------------------------------------------------------------

async function runShellPage() {
    // 2) transport injection (before initPresets()).
    pageTransport = createShellPresetTransport({ onWriteError: handleWriteError });
    setPresetTransport(pageTransport);

    // 3) bridge client — hello decides the read-only state on the first frame.
    initBridgeClient({
        onHello: (payload) => setReadOnly(payload && payload.tosuOnline),
        onState: applyShellState,
        onSettings: (payload) => pageTransport.onShellFrame(payload),
        onSong: () => {},
        onMalody4Selection: () => {},
    });

    setStatus(t("status.loading", "Loading settings…"), "loading");

    // 4) current settings → state → UI.
    const values = await pageTransport.readValues();
    if (!values) {
        setStatus(getShellUnavailableNotice(), "error");
        return;
    }
    await applySnapshot(values, { quiet: true });
    state.wsEndpoint = values.wsEndpoint ?? state.wsEndpoint;

    // The settings schema (entries + appliers + defaults) drives the form. Only
    // the schema LOADER is used here — the overlay's whole-payload settings
    // applier is never called on this page (it assumes the card DOM).
    schema = await loadSettingsSchema();
    buildLayout();
    form = createSettingsForm({
        root: settingsFormEl,
        schema,
        initial: values,
        onCommit: commitSetting,
    });
    form.render();
    form.setReadOnly(state.shellTosuOnline === true);

    // settings.json button entries (Guide / Preset guide / Issue / Benchmark) —
    // links, not settings: they render into the top-of-page
    // #settings-links-root declared by settings.html. The tosu-hosted
    // PresetButton / DebugButton targets do not exist inside the shell and are
    // filtered out by settingsLinks.js (EXCLUDED_LINK_IDS).
    links = createSettingsLinks({
        root: linksRootEl,
        entries: schema.entries,
        urlFor: buttonUrl,
        onCopied: () => showToast(LINK_COPIED_NOTICE),
    });
    links.render();

    shellConfigPanel = createShellConfigPanel({ root: shellConfigRootEl });
    await shellConfigPanel.load();

    refreshReadOnlyView();
    if (state.shellTosuOnline === true) {
        // `hello` may have rendered the notice before settings.json was read; the
        // notice carries the PresetsButton URL, so re-render it now.
        showPresetsNotice();
    }
    setStatus(t("status.ready", "Ready"), "ok");

    // 5) presets.
    initPresets();
    await loadBuiltinPresets();
    await applyPresetsView();

    // 6) pushed changes.
    pageTransport.subscribe(handlePushedSettings);
}

async function bootstrap() {
    initLocale();
    updatePageTitle();
    setupLangSwitcher();

    onLocaleChange((locale) => {
        updatePageTitle();
        updateNavLabels();
        updateLangButtons(locale);
        updateLayoutText();
        if (form) {
            form.render();
            form.setReadOnly(state.shellTosuOnline === true);
        }
        if (links) {
            links.render();
        }
        if (shellConfigPanel && typeof shellConfigPanel.render === "function") {
            shellConfigPanel.render();
        }
        if (state.shellTosuOnline === true) {
            showPresetsNotice();
        }
    });

    if (!isShellOrigin()) {
        renderOriginNotice();
        return;
    }
    try {
        await runShellPage();
    } catch (error) {
        console.error("[settings] initialization failed:", error);
        setStatus(`Initialization failed: ${error && error.message ? error.message : error}`, "error");
    }
}

bootstrap();
