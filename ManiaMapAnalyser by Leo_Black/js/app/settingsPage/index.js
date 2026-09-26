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
import { createShellPresetTransport } from "./shellTransport.js";
import { createSettingsForm } from "./settingsForm.js";
import { createShellConfigPanel } from "./shellConfigPanel.js";

/** Port the desktop shell serves the plugin directory on. */
const SHELL_PORT = "24061";
/** Hosts a shell window can legitimately use. */
const LOOPBACK_HOSTS = new Set(["localhost", "127.0.0.1", "::1", "[::1]"]);
/** Fallback dashboard endpoint when the settings carry no wsEndpoint. */
const FALLBACK_ENDPOINT = "localhost:24050";
/** settings.json entry whose value points at the tosu presets page. */
const PRESETS_BUTTON_KEY = "PresetButton";

const SHELL_UNAVAILABLE_NOTICE = "Cannot reach the shell (24061)";
const SHELL_ONLINE_NOTICE = "tosu is connected — settings are read-only here.";
const SHELL_ONLINE_PRESETS_NOTICE = "tosu is connected — settings are read-only here. "
    + "Manage presets from the Presets page of your tosu instance.";
const ORIGIN_NOTICE = "Open this page from the desktop shell: http://127.0.0.1:24061/settings.html";

const statusBarEl = document.getElementById("settings-status");
const settingsRootEl = document.getElementById("settings-root");
const shellConfigRootEl = document.getElementById("shell-config-root");
const presetsAppEl = document.getElementById("presets-app");

let schema = null;
let form = null;
let shellConfigPanel = null;
let pageTransport = null;
let settingsFormEl = null;
let readOnlyEl = null;
let dashboardUrlEl = null;
let presetsNoticeEl = null;
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

/** settings.json PresetButton URL with its host:port rewritten to wsEndpoint. */
function presetsPageUrl() {
    const entry = schema && Array.isArray(schema.entries)
        ? schema.entries.find((item) => item && item.uniqueID === PRESETS_BUTTON_KEY)
        : null;
    const raw = entry && typeof entry.value === "string" ? entry.value.trim() : "";
    if (!raw) {
        return null;
    }
    const endpoint = endpointValue().replace(/^https?:\/\//i, "").replace(/\/+$/, "");
    return /^https?:\/\//i.test(raw) ? raw.replace(/^https?:\/\/[^/]+/i, `http://${endpoint}`) : raw;
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
    notice.textContent = ORIGIN_NOTICE;
    if (settingsRootEl) {
        settingsRootEl.appendChild(notice);
    }
    setStatus(ORIGIN_NOTICE, "error");
    document.title = "Mania Map Analyser — Settings (desktop shell only)";
}

// ---------------------------------------------------------------------------
// 4) Layout: nav bar, read-only view, settings form section
// ---------------------------------------------------------------------------

function buildLayout() {
    settingsRootEl.textContent = "";
    settingsRootEl.appendChild(buildNav());
    readOnlyEl = buildReadOnlyView();
    settingsRootEl.appendChild(readOnlyEl);

    const section = document.createElement("section");
    section.id = "settings-form-section";
    section.className = "settings-panel";

    const title = document.createElement("h2");
    title.textContent = "Plugin Settings";
    section.appendChild(title);

    const sub = document.createElement("p");
    sub.className = "settings-panel-sub";
    sub.textContent = "The desktop shell keeps these values in mma-settings.json while tosu is offline; "
        + "the overlay picks up every change immediately.";
    section.appendChild(sub);

    settingsFormEl = document.createElement("div");
    settingsFormEl.className = "settings-form";
    section.appendChild(settingsFormEl);
    settingsRootEl.appendChild(section);
}

function buildNav() {
    const nav = document.createElement("nav");
    nav.className = "settings-nav";
    for (const [href, label] of [
        ["#settings-form-section", "Settings"],
        ["#shell-config-section", "Shell"],
        ["#presets-app", "Presets"],
    ]) {
        const link = document.createElement("a");
        link.className = "settings-nav-link";
        link.href = href;
        link.textContent = label;
        nav.appendChild(link);
    }
    return nav;
}

function buildReadOnlyView() {
    const banner = document.createElement("div");
    banner.className = "settings-readonly";
    banner.hidden = true;

    const message = document.createElement("p");
    message.className = "settings-readonly-message";
    message.textContent = SHELL_ONLINE_NOTICE;
    banner.appendChild(message);

    const row = document.createElement("div");
    row.className = "settings-readonly-url";
    const label = document.createElement("span");
    label.className = "settings-readonly-label";
    label.textContent = "tosu dashboard:";
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
    text.textContent = SHELL_ONLINE_PRESETS_NOTICE;
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
    setStatus("Saving…", "saving");
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
        setStatus("Saved", "ok");
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
        setStatus(wasWritable ? `Save failed: ${message}` : SHELL_ONLINE_NOTICE, "error");
        setReadOnly(true);
        return;
    }
    setStatus(`Save failed: ${message}`, "error");
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

    setStatus("Loading settings…", "loading");

    // 4) current settings → state → UI.
    const values = await pageTransport.readValues();
    if (!values) {
        setStatus(SHELL_UNAVAILABLE_NOTICE, "error");
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

    shellConfigPanel = createShellConfigPanel({ root: shellConfigRootEl });
    await shellConfigPanel.load();

    refreshReadOnlyView();
    if (state.shellTosuOnline === true) {
        // `hello` may have rendered the notice before settings.json was read; the
        // notice carries the PresetsButton URL, so re-render it now.
        showPresetsNotice();
    }
    setStatus("Ready", "ok");

    // 5) presets.
    initPresets();
    await loadBuiltinPresets();
    await applyPresetsView();

    // 6) pushed changes.
    pageTransport.subscribe(handlePushedSettings);
}

async function bootstrap() {
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
