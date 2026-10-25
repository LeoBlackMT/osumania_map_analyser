/**
 * Shell-config panel of the desktop settings page.
 *
 * Edits mma-shell-config.json through the shell's local endpoint
 * (GET/POST /shell-config). The page never guesses the file's shape: every
 * write is a request-key-first patch, and every render comes from a response —
 * the POST one for the config itself, plus a re-GET for the `resolved` block
 * (the paths the shell actually adopted, which the POST response cannot know).
 *
 * Patches are single-key, except `hotkeys`, which is submitted as a whole object
 * (the four fields belong together; a half-written hotkey set would leave the
 * shell with a mix of old and new bindings).
 */

import { setLanguage, t } from "../i18n/index.js";
import {
    COMBINED_MODE_LABELS,
    COMBINED_MODE_MODULES,
    normalizeCombinedMode,
} from "../combinedMode.js";

/** Local shell-config endpoint (desktop/src/server/http.rs). */
export const SHELL_CONFIG_URL = "/shell-config";
export const OFFSETS_STATUS_URL = "/offsets/status";
export const OFFSETS_GENERATE_URL = "/offsets/generate";
export const OFFSETS_UPDATE_URL = "/offsets/update";
export const SHADOW_RESET_URL = "/shadow/reset";

/** `gameClient` values the plugin's source router understands (Auto = decide). */
const GAME_CLIENT_OPTIONS = ["Auto", "osu!", "Etterna", "Malody", "Malody 4"];

/** Matches config::log_level()'s accepted values. */
const LOG_LEVEL_OPTIONS = ["debug", "info", "warn", "error", "off"];

// 语言下拉的取值与壳配置 `language` 字段一致。
const LANGUAGE_OPTIONS = [
    { value: "auto", labelKey: "ui.settings.language.auto", fallback: "Auto" },
    { value: "zh-CN", label: "简体中文" },
    { value: "en-US", label: "English" },
];

const ROOT_FIELDS = [
    {
        key: "etternaRoot",
        title: "Etterna root",
        description: "Folder that holds the Etterna install. Empty = let the shell detect it.",
    },
    {
        key: "malodyRoot",
        title: "Malody V root",
        description: "Folder that holds the Malody V install. Empty = let the shell detect it.",
    },
    {
        key: "malody4Root",
        title: "Malody 4.3.7 root",
        description: "Folder that holds the Malody 4.3.7 install. Empty = let the shell detect it.",
    },
];

const HOTKEY_FIELDS = [
    { key: "topmost", title: "Toggle topmost" },
    { key: "clickThrough", title: "Toggle click-through" },
    { key: "close", title: "Close overlay" },
    { key: "settings", title: "Open settings window" },
];

// 下面的常量保持英文：模块加载时语言还没确定，t() 必须在渲染时调用。
const HOTKEY_HINT = "Restart the shell to apply hotkey changes.";
const UNREADABLE_NOTICE = "Shell config unreadable (mma-shell-config.json) — editing is unavailable here.";

/**
 * @param {object} options
 * @param {HTMLElement} options.root container element (#shell-config-root).
 * @param {(url: string, init?: object) => Promise<object>} [options.fetchImpl]
 *        fetch implementation (defaults to the global fetch).
 */
export function createShellConfigPanel({ root, fetchImpl = (url, init) => fetch(url, init), onLanguageChange = null }) {
    let config = null;
    let resolved = {};
    let windowState = { topmost: true, clickThrough: false };
    let statusText = "";
    let statusKind = "info";
    let saveHint = "";

    let offsetsInfo = null;
    let offsetsActionMsg = "";
    let offsetsActionKind = "info";

    async function requestJson(url, init) {
        try {
            const response = await fetchImpl(url, init);
            const status = response && typeof response.status === "number" ? response.status : 0;
            let body = null;
            if (response) {
                try {
                    body = await response.json();
                } catch {
                    body = null;
                }
            }
            if (!response || !response.ok) {
                return { ok: false, status, body };
            }
            return { ok: true, status, body };
        } catch (error) {
            return { ok: false, status: 0, error };
        }
    }

    function setStatus(text, kind) {
        statusText = text || "";
        statusKind = kind || "info";
    }

    async function fetchOffsetsStatus() {
        const result = await requestJson(OFFSETS_STATUS_URL, { cache: "no-store" });
        if (result.ok && result.body) {
            offsetsInfo = result.body;
        }
    }

    async function generateOffsets() {
        offsetsActionMsg = t("ui.shell.offsets.scanning", "Scanning running osu! and generating offsets…");
        offsetsActionKind = "saving";
        render();
        const result = await requestJson(OFFSETS_GENERATE_URL, { method: "POST" });
        if (!result.ok) {
            const detail = result.body && result.body.message ? result.body.message : (result.error ? result.error.message : `HTTP ${result.status}`);
            offsetsActionMsg = `${t("ui.shell.offsets.generateFailed", "Generation failed")}: ${detail}`;
            offsetsActionKind = "error";
        } else {
            const msg = result.body && result.body.message ? result.body.message : t("ui.shell.offsets.generated", "Offsets generated and verified successfully.");
            offsetsActionMsg = msg;
            offsetsActionKind = "ok";
        }
        await fetchOffsetsStatus();
        render();
    }

    async function updateOffsets() {
        offsetsActionMsg = t("ui.shell.offsets.checking", "Checking remote manifest and verifying signatures…");
        offsetsActionKind = "saving";
        render();
        const result = await requestJson(OFFSETS_UPDATE_URL, { method: "POST" });
        if (!result.ok) {
            const detail = result.body && result.body.message ? result.body.message : (result.error ? result.error.message : `HTTP ${result.status}`);
            offsetsActionMsg = `${t("ui.shell.offsets.updateFailed", "Update failed")}: ${detail}`;
            offsetsActionKind = "error";
        } else {
            const updated = result.body && typeof result.body.updated === "number" ? result.body.updated : 0;
            const msg = result.body && result.body.message ? result.body.message : (updated > 0 ? `${t("ui.shell.offsets.updatedCount", "Updated")} ${updated} ${t("ui.shell.offsets.tables", "table(s)")}` : t("ui.shell.offsets.upToDate", "Offsets are up to date."));
            offsetsActionMsg = msg;
            offsetsActionKind = "ok";
        }
        await fetchOffsetsStatus();
        render();
    }

    async function resetShadow() {
        await requestJson(SHADOW_RESET_URL, { method: "POST" });
        await fetchOffsetsStatus();
        render();
    }

    /** GET /shell-config → config + resolved; a 400 renders a read-only notice. */
    async function load() {
        await fetchOffsetsStatus();
        const result = await requestJson(SHELL_CONFIG_URL, { cache: "no-store" });
        if (!result.ok || !result.body || !result.body.config || typeof result.body.config !== "object") {
            config = null;
            resolved = {};
            saveHint = "";
            setStatus(
                result.status === 400
                    ? t("ui.shell.status.unreadable", UNREADABLE_NOTICE)
                    : `${t("ui.shell.offsets.unreadableHttp", "Cannot read the shell config")} (HTTP ${result.status || "?"}).`,
                "error",
            );
            render();
            return false;
        }
        config = result.body.config;
        resolved = result.body.resolved && typeof result.body.resolved === "object" ? result.body.resolved : {};
        if (result.body.window && typeof result.body.window === "object") {
            windowState = result.body.window;
        }
        setStatus("", "info");
        render();
        return true;
    }

    /** POST a patch; re-render from the response (400 = nothing was written). */
    async function write(patch, { refreshResolvedAfter = false } = {}) {
        setStatus(t("ui.shell.status.saving", "Saving…"), "saving");
        saveHint = "";
        render();
        const result = await requestJson(SHELL_CONFIG_URL, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify(patch),
        });
        if (!result.ok) {
            if (result.status === 400) {
                setStatus(`${t("ui.shell.status.unreadable", UNREADABLE_NOTICE)} (${t("ui.shell.status.nothingWritten", "nothing was written")})`, "error");
            } else {
                const detail = result.error && result.error.message ? result.error.message : `HTTP ${result.status || "?"}`;
                setStatus(`${t("ui.shell.status.saveFailed", "Save failed")}: ${detail}`, "error");
            }
            render();
            return false;
        }
        if (result.body && typeof result.body === "object") {
            if (result.body.config && typeof result.body.config === "object") {
                config = result.body.config;
            } else if (!result.body.window) {
                config = result.body;
            }
            if (result.body.window && typeof result.body.window === "object") {
                windowState = result.body.window;
            }
        }
        setStatus(t("ui.shell.status.saved", "Saved."), "ok");
        saveHint = refreshResolvedAfter ? t("ui.shell.status.refreshing", "Refreshing the adopted paths…") : "";
        render();
        if (refreshResolvedAfter) {
            await refreshResolved();
            saveHint = "";
        }
        return true;
    }

    /** Re-reads only the `resolved` block (after a root-path write). */
    async function refreshResolved() {
        const result = await requestJson(SHELL_CONFIG_URL, { cache: "no-store" });
        if (!result.ok || !result.body || !result.body.resolved) {
            return false;
        }
        resolved = result.body.resolved;
        render();
        return true;
    }

    function render() {
        root.textContent = "";
        const section = document.createElement("section");
        section.id = "shell-config-section";
        section.className = "settings-panel";

        const title = document.createElement("h2");
        title.textContent = t("ui.shell.sectionTitle", "Shell Configuration");
        section.appendChild(title);

        const sub = document.createElement("p");
        sub.className = "settings-panel-sub";
        sub.textContent = t("ui.shell.sectionSub", "Only the desktop shell reads these values (mma-shell-config.json, next to mma-shell.exe).");
        section.appendChild(sub);

        const status = document.createElement("p");
        status.className = `shell-config-status shell-config-status-${statusKind}`;
        status.textContent = statusText || saveHint;
        section.appendChild(status);

        if (config) {
            section.appendChild(buildLanguageRow());
            section.appendChild(buildGameClientRow());
            for (const field of ROOT_FIELDS) {
                section.appendChild(buildRootRow(field));
            }
            section.appendChild(buildWindowTopmostRow());
            section.appendChild(buildWindowClickThroughRow());
            section.appendChild(buildHotkeysRow());
            section.appendChild(buildCombinedModeSection());
            section.appendChild(buildLogLevelRow());
            if (offsetsInfo) {
                section.appendChild(buildOffsetsSection());
                if (offsetsInfo.shadow && isShadowDiagnosticsEnabled()) {
                    section.appendChild(buildShadowSection());
                }
            }
        }
        root.appendChild(section);
    }

    function isShadowDiagnosticsEnabled() {
        try {
            const params = new URLSearchParams(window.location.search);
            if (params.has("debug") || params.has("shadow") || params.has("diag")) {
                return true;
            }
        } catch (_) {}
        if (offsetsInfo && (offsetsInfo.shadow_enabled === true || offsetsInfo.shadowDiagnostics === true)) {
            return true;
        }
        if (config && config.shadowDiagnostics === true) {
            return true;
        }
        return false;
    }

    function buildRow(key, title, description, control) {
        const row = document.createElement("div");
        row.className = "shell-config-row";
        row.dataset.shellKey = key;

        const info = document.createElement("span");
        info.className = "settings-row-info";
        const titleEl = document.createElement("span");
        titleEl.className = "settings-row-title";
        titleEl.textContent = title;
        info.appendChild(titleEl);
        if (description) {
            const descEl = document.createElement("span");
            descEl.className = "settings-row-desc";
            descEl.textContent = description;
            info.appendChild(descEl);
        }

        const controlWrap = document.createElement("span");
        controlWrap.className = "settings-row-control";
        controlWrap.dataset.shellKey = key;
        controlWrap.appendChild(control);

        row.appendChild(info);
        row.appendChild(controlWrap);
        return row;
    }

    function buildLanguageRow() {
        const select = document.createElement("select");
        select.dataset.shellKey = "language";
        const current = String(config.language || "auto");
        for (const option of LANGUAGE_OPTIONS) {
            const optionEl = document.createElement("option");
            optionEl.value = option.value;
            optionEl.textContent = option.labelKey ? t(option.labelKey, option.fallback) : option.label;
            optionEl.selected = option.value === current;
            select.appendChild(optionEl);
        }
        select.value = current;
        select.addEventListener("change", async () => {
            const value = select.value;
            // 先切内存里的语言（并写 localStorage），再落盘：即便写盘失败，本次会话也对。
            setLanguage(value === "auto" ? (typeof navigator !== "undefined" ? navigator.language : "") : value);
            await write({ language: value });
            if (typeof onLanguageChange === "function") {
                try {
                    onLanguageChange(value);
                } catch (error) {
                    console.error("[shell-config] language change handler failed:", error);
                }
            }
            // 整页重载最彻底（预设面板等由各自 init 重建）；重载失效时上面的就地重渲染已兜底。
            window.location.reload();
        });
        return buildRow(
            "language",
            t("ui.shell.field.language.title", "Interface language"),
            t("ui.shell.field.language.description", "Only affects the shell's own pages; tosu's settings panel stays English."),
            select,
        );
    }

    function buildGameClientRow() {
        const select = document.createElement("select");
        select.dataset.shellKey = "gameClient";
        const current = String(config.gameClient || "Auto");
        for (const option of GAME_CLIENT_OPTIONS) {
            const optionEl = document.createElement("option");
            optionEl.value = option;
            optionEl.textContent = t(`option.${option}`, option);
            optionEl.selected = option === current;
            select.appendChild(optionEl);
        }
        select.value = current;
        select.addEventListener("change", () => {
            write({ gameClient: select.value });
        });
        return buildRow(
            "gameClient",
            t("ui.shell.field.gameClient.title", "Game client"),
            t("ui.shell.field.gameClient.description", "Auto picks the active source; a fixed value forces that source."),
            select,
        );
    }

    function buildRootRow(field) {
        const wrap = document.createElement("span");
        wrap.className = "settings-root-field";

        const input = document.createElement("input");
        input.type = "text";
        input.dataset.shellKey = field.key;
        input.value = typeof config[field.key] === "string" ? config[field.key] : "";
        input.addEventListener("change", () => {
            write({ [field.key]: input.value }, { refreshResolvedAfter: true });
        });
        wrap.appendChild(input);

        const adopted = resolved ? resolved[field.key] : null;
        const readout = document.createElement("span");
        readout.className = "shell-config-resolved";
        readout.dataset.resolvedKey = field.key;
        readout.textContent = `${t("ui.shell.resolved.adopted", "adopted")}: ${adopted || t("ui.shell.resolved.notDetected", "not detected")}`;
        wrap.appendChild(readout);

        return buildRow(
            field.key,
            t(`ui.shell.field.${field.key}.title`, field.title),
            t(`ui.shell.field.${field.key}.description`, field.description),
            wrap,
        );
    }

    function buildWindowTopmostRow() {
        const input = document.createElement("input");
        input.type = "checkbox";
        input.dataset.shellKey = "topmost";
        input.checked = Boolean(windowState && windowState.topmost);
        input.addEventListener("change", () => {
            write({ window: { topmost: input.checked } });
        });
        return buildRow(
            "topmost",
            t("ui.shell.field.topmost.title", "Always on Top"),
            t("ui.shell.field.topmost.description", "Keep the overlay window above all other windows."),
            input,
        );
    }

    function buildWindowClickThroughRow() {
        const input = document.createElement("input");
        input.type = "checkbox";
        input.dataset.shellKey = "clickThrough";
        input.checked = Boolean(windowState && windowState.clickThrough);
        input.addEventListener("change", () => {
            write({ window: { clickThrough: input.checked } });
        });
        return buildRow(
            "clickThrough",
            t("ui.shell.field.clickThrough.title", "Click-Through"),
            t("ui.shell.field.clickThrough.description", "Pass mouse clicks through the overlay window to the game or desktop."),
            input,
        );
    }

    function buildHotkeysRow() {
        const wrap = document.createElement("span");
        wrap.className = "shell-config-hotkeys";
        const inputs = new Map();
        const hotkeys = config.hotkeys && typeof config.hotkeys === "object" ? config.hotkeys : {};

        for (const field of HOTKEY_FIELDS) {
            const fieldWrap = document.createElement("label");
            fieldWrap.className = "shell-config-hotkey";
            const label = document.createElement("span");
            label.className = "shell-config-hotkey-label";
            label.textContent = t(`ui.shell.hotkey.${field.key}`, field.title);
            const input = document.createElement("input");
            input.type = "text";
            input.dataset.shellKey = field.key;
            input.value = typeof hotkeys[field.key] === "string" ? hotkeys[field.key] : "";
            input.addEventListener("change", () => {
                const next = {};
                for (const [key, el] of inputs) {
                    next[key] = el.value;
                }
                write({ hotkeys: next });
            });
            inputs.set(field.key, input);
            fieldWrap.appendChild(label);
            fieldWrap.appendChild(input);
            wrap.appendChild(fieldWrap);
        }

        const hint = document.createElement("span");
        hint.className = "shell-config-hint";
        hint.textContent = t("ui.shell.hotkey.hint", HOTKEY_HINT);
        wrap.appendChild(hint);

        return buildRow("hotkeys", t("ui.shell.field.hotkeys.title", "Hotkeys"), t("ui.shell.field.hotkeys.description", "Shell-wide shortcuts."), wrap);
    }

    function buildCombinedModeSection() {
        const current = normalizeCombinedMode(config.combinedMode);
        const section = document.createElement("div");
        section.className = "shell-config-subsection";
        section.id = "shell-combined-mode-section";

        const title = document.createElement("h3");
        title.className = "settings-group-title";
        title.textContent = t("ui.shell.combinedMode.title", "Combined Mode");
        section.appendChild(title);

        const toggle = document.createElement("input");
        toggle.type = "checkbox";
        toggle.dataset.shellKey = "combinedMode-enabled";
        toggle.checked = current.enabled;
        toggle.addEventListener("change", () => {
            list.hidden = !toggle.checked;
            write({ combinedMode: { enabled: toggle.checked, modules: current.modules } });
        });
        section.appendChild(buildRow(
            "combinedMode",
            t("ui.shell.combinedMode.enable", "Enable combined mode"),
            t("ui.shell.combinedMode.description", "Show several card sections at once, in the order below."),
            toggle,
        ));

        const list = document.createElement("div");
        list.className = "shell-combined-mode-list";
        list.hidden = !current.enabled;
        list.setAttribute(
            "aria-label",
            t("ui.shell.combinedMode.modulesTitle", "Sections and order"),
        );

        // 已勾选的模块按保存顺序在前；未勾选的按默认顺序追加，方便重新勾选。
        const ordered = [
            ...current.modules,
            ...COMBINED_MODE_MODULES.filter((module) => !current.modules.includes(module)),
        ];
        for (const module of ordered) {
            const checked = current.modules.includes(module);
            const moduleKey = module === "ReworkPP" ? "pp" : module.toLowerCase();
            const labelText = t(`ui.shell.combinedMode.module.${moduleKey}`, COMBINED_MODE_LABELS[module]);
            const row = document.createElement("div");
            row.className = "shell-combined-mode-item";
            row.dataset.module = module;

            const label = document.createElement("label");
            label.className = "shell-combined-mode-label";
            const checkbox = document.createElement("input");
            checkbox.type = "checkbox";
            checkbox.dataset.shellKey = `combinedMode-${module}`;
            checkbox.checked = checked;
            // 只剩最后一个已勾选模块时禁止取消，保证 Combined 至少一个模块。
            checkbox.disabled = checked && current.modules.length === 1;
            if (checkbox.disabled) {
                checkbox.title = t("ui.shell.combinedMode.atLeastOne", "Keep at least one section.");
            }
            checkbox.addEventListener("change", () => {
                let next;
                if (checkbox.checked) {
                    next = current.modules.includes(module)
                        ? current.modules.slice()
                        : [...current.modules, module];
                } else {
                    next = current.modules.filter((item) => item !== module);
                    if (next.length === 0) {
                        checkbox.checked = true;
                        return;
                    }
                }
                write({ combinedMode: { enabled: current.enabled, modules: next } });
            });
            label.appendChild(checkbox);
            label.appendChild(document.createTextNode(labelText));
            row.appendChild(label);

            const index = current.modules.indexOf(module);
            for (const [direction, symbol, labelKey, fallback] of [
                ["up", "↑", "ui.shell.combinedMode.moveUp", "Move up"],
                ["down", "↓", "ui.shell.combinedMode.moveDown", "Move down"],
            ]) {
                const button = document.createElement("button");
                button.type = "button";
                button.className = "settings-action-btn shell-combined-mode-move";
                button.dataset.move = direction;
                button.textContent = symbol;
                button.title = t(labelKey, fallback);
                button.setAttribute("aria-label", `${t(labelKey, fallback)}: ${labelText}`);
                button.disabled = !checked
                    || (direction === "up" && index === 0)
                    || (direction === "down" && index === current.modules.length - 1);
                button.addEventListener("click", () => {
                    const next = current.modules.slice();
                    const from = next.indexOf(module);
                    const to = direction === "up" ? from - 1 : from + 1;
                    if (from < 0 || to < 0 || to >= next.length) {
                        return;
                    }
                    [next[from], next[to]] = [next[to], next[from]];
                    write({ combinedMode: { enabled: current.enabled, modules: next } });
                });
                row.appendChild(button);
            }

            list.appendChild(row);
        }
        section.appendChild(list);

        const hint = document.createElement("p");
        hint.className = "shell-config-hint";
        hint.textContent = t(
            "ui.shell.combinedMode.hint",
            "Desktop shell only. The card grows with the number of checked sections.",
        );
        section.appendChild(hint);

        return section;
    }

    function buildLogLevelRow() {
        const select = document.createElement("select");
        select.dataset.shellKey = "logLevel";
        const current = String(config.logLevel || "info");
        for (const option of LOG_LEVEL_OPTIONS) {
            const optionEl = document.createElement("option");
            optionEl.value = option;
            optionEl.textContent = option;
            optionEl.selected = option === current;
            select.appendChild(optionEl);
        }
        select.value = current;
        select.addEventListener("change", () => {
            write({ logLevel: select.value });
        });
        return buildRow("logLevel", t("ui.shell.field.logLevel.title", "Log level"), t("ui.shell.field.logLevel.description", "Takes effect immediately."), select);
    }

    function buildOffsetsSection() {
        const wrap = document.createElement("div");
        wrap.className = "shell-config-subsection";

        const header = document.createElement("h3");
        header.className = "settings-group-title";
        header.textContent = t("ui.shell.offsets.title", "Memory Offsets Management");
        wrap.appendChild(header);

        const stableInfo = offsetsInfo.stable || {};
        const stableDesc = stableInfo.loaded
            ? `${t("ui.shell.offsets.active", "Active")} (${stableInfo.anchors_count || 7} ${t("ui.shell.offsets.anchors", "anchors")}, ${stableInfo.arch || "x86"})`
            : t("ui.shell.offsets.fallbackDesc", "Fallback table active");
        const stableBadge = document.createElement("span");
        stableBadge.className = `shell-config-badge ${stableInfo.loaded ? "badge-ok" : "badge-warn"}`;
        stableBadge.textContent = stableInfo.loaded ? t("ui.shell.offsets.active", "Active") : t("ui.shell.offsets.fallback", "Fallback");
        wrap.appendChild(buildRow("offsetsStable", t("ui.shell.offsets.stableTitle", "osu!stable Offsets"), stableDesc, stableBadge));

        const lazerInfo = offsetsInfo.lazer || {};
        const lazerDesc = lazerInfo.loaded
            ? `v${lazerInfo.version || "unknown"} (runtime: ${lazerInfo.runtime_version || "unknown"}, ${lazerInfo.types_count || 0} types)`
            : t("ui.shell.offsets.fallbackDesc", "Fallback table active");
        const lazerBadge = document.createElement("span");
        lazerBadge.className = `shell-config-badge ${lazerInfo.loaded ? "badge-ok" : "badge-warn"}`;
        lazerBadge.textContent = lazerInfo.loaded ? t("ui.shell.offsets.active", "Active") : t("ui.shell.offsets.fallback", "Fallback");
        wrap.appendChild(buildRow("offsetsLazer", t("ui.shell.offsets.lazerTitle", "osu!lazer Offsets"), lazerDesc, lazerBadge));

        const actionsWrap = document.createElement("div");
        actionsWrap.className = "shell-config-actions";

        const genBtn = document.createElement("button");
        genBtn.type = "button";
        genBtn.className = "settings-action-btn";
        genBtn.textContent = t("ui.shell.offsets.generate", "Live Generator");
        genBtn.title = offsetsInfo.generator_ready
            ? t("ui.shell.offsets.generateHint", "Extract offsets from running osu! with zero .NET SDK")
            : t("ui.shell.offsets.generateMissing", "gen.exe not detected");
        genBtn.disabled = !offsetsInfo.generator_ready;
        genBtn.addEventListener("click", () => generateOffsets());
        actionsWrap.appendChild(genBtn);

        const updateBtn = document.createElement("button");
        updateBtn.type = "button";
        updateBtn.className = "settings-action-btn";
        updateBtn.textContent = t("ui.shell.offsets.update", "Check Remote Updates");
        updateBtn.title = t("ui.shell.offsets.updateHint", "Verify Ed25519 signed manifest and sync updated tables");
        updateBtn.addEventListener("click", () => updateOffsets());
        actionsWrap.appendChild(updateBtn);

        if (offsetsActionMsg) {
            const statusEl = document.createElement("div");
            statusEl.className = `shell-config-action-status shell-config-status-${offsetsActionKind}`;
            statusEl.textContent = offsetsActionMsg;
            actionsWrap.appendChild(statusEl);
        }

        wrap.appendChild(buildRow("offsetsActions", t("ui.shell.offsets.actionsTitle", "Offsets Actions"), t("ui.shell.offsets.actionsDescription", "Update or generate memory layout tables for osu! clients."), actionsWrap));

        return wrap;
    }

    function buildShadowSection() {
        const wrap = document.createElement("div");
        wrap.className = "shell-config-subsection";

        const header = document.createElement("h3");
        header.className = "settings-group-title";
        header.textContent = t("ui.shell.shadow.title", "Shadow Diagnostics (Native vs Tosu)");
        wrap.appendChild(header);

        const shadow = offsetsInfo.shadow || {};
        const total = shadow.total_compared || 0;
        const matches = shadow.matched_frames || 0;
        const mismatches = shadow.mismatched_frames || 0;
        const rate = typeof shadow.match_rate === "number" ? shadow.match_rate.toFixed(1) : "0.0";

        const desc = total === 0
            ? (shadow.tosu_connected
                ? t("ui.shell.shadow.tosuConnected", "tosu connected — awaiting compared frames")
                : t("ui.shell.shadow.waiting", "Waiting for concurrent tosu and native data streams…"))
            : `${total} ${t("ui.shell.shadow.framesCompared", "frames compared")}: ${matches} ${t("ui.shell.shadow.matched", "matched")} (${rate}%), ${mismatches} ${t("ui.shell.shadow.mismatched", "mismatched")}`;

        const badge = document.createElement("span");
        if (total === 0) {
            badge.className = "shell-config-badge badge-idle";
            badge.textContent = t("ui.shell.shadow.idle", "Idle");
        } else if (mismatches === 0) {
            badge.className = "shell-config-badge badge-ok";
            badge.textContent = t("ui.shell.shadow.fullMatch", "100% Match");
        } else {
            badge.className = "shell-config-badge badge-error";
            badge.textContent = `${mismatches} ${t("ui.shell.shadow.differ", "Differ")}`;
        }

        wrap.appendChild(buildRow("shadowStatus", t("ui.shell.shadow.statusTitle", "Comparison Status"), desc, badge));

        if (mismatches > 0 && Array.isArray(shadow.last_mismatches) && shadow.last_mismatches.length > 0) {
            const diffDesc = `${t("ui.shell.shadow.lastDiffs", "Last differing fields")}: ${shadow.last_mismatches.join(", ")}`;
            const diffTag = document.createElement("span");
            diffTag.className = "shell-config-diff-tag";
            diffTag.textContent = shadow.last_mismatches.join(", ");
            wrap.appendChild(buildRow("shadowDiffs", t("ui.shell.shadow.diffTitle", "Discrepancy Details"), diffDesc, diffTag));
        }

        const actionsWrap = document.createElement("div");
        actionsWrap.className = "shell-config-actions";

        const refreshBtn = document.createElement("button");
        refreshBtn.type = "button";
        refreshBtn.className = "settings-action-btn";
        refreshBtn.textContent = t("ui.shell.shadow.refresh", "Refresh Stats");
        refreshBtn.addEventListener("click", async () => {
            await fetchOffsetsStatus();
            render();
        });
        actionsWrap.appendChild(refreshBtn);

        const resetBtn = document.createElement("button");
        resetBtn.type = "button";
        resetBtn.className = "settings-action-btn";
        resetBtn.textContent = t("ui.shell.shadow.reset", "Reset Counts");
        resetBtn.addEventListener("click", () => resetShadow());
        actionsWrap.appendChild(resetBtn);

        wrap.appendChild(buildRow("shadowActions", t("ui.shell.shadow.actionsTitle", "Diagnostics Controls"), t("ui.shell.shadow.actionsDescription", "Manage shadow verification metrics."), actionsWrap));

        return wrap;
    }

    return {
        load,
        refresh: load,
        refreshResolved,
        getConfig: () => config,
        getResolved: () => resolved,
    };
}
