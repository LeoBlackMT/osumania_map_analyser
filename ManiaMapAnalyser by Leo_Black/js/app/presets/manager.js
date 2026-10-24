/**
 * Presets manager page (presets.html).
 * Left: categorized preset list (System incl. Default + My Presets).
 * Right: metadata panel + self-extending settings form (checkboxes control
 * which fields are included in the snapshot). Top: independent action bar.
 * All destructive actions require a confirmation modal; feedback uses toasts.
 */

import { applyStaticText, initLanguage, isLanguageInitialized, t, tf } from "../i18n/index.js";
import {
    getCustomPresets,
    getBuiltinPresets,
    getBuiltinSettings,
    getCurrentPreset,
    getPresetTransport,
    onPresetsChanged,
    applyPresetByName,
    applyCustomSnapshot,
    createCustomPreset,
    updatePresetMetadata,
    deleteCustomPreset,
    captureCurrentSettings,
    ensureDefaultCustomSlots,
    ensureLibraryLoaded,
    didLastPersistFail,
    initPresets,
} from "./core.js";
import {
    exportPresetToFile,
    exportCurrentToFile,
    exportLibraryToFile,
    importPresetFromFile,
} from "./io.js";
import { loadSettingsSchema, buildDefaultSnapshot } from "./schema.js";
import {
    AUTO_SAVE_PRESET_NAME,
    DEFAULT_SLOT_NAMES,
} from "./storage.js";
import { createForm } from "./form.js";
import { LINK_COPIED_NOTICE, openExternalLink } from "../externalLink.js";

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const PRESET_NAME_RE = /^[A-Za-z0-9_-]{1,40}$/;

// The offline desktop settings window (settings.html) mounts this same editor;
// the LastSavedPreset row is a tosu-dashboard concept without an offline
// counterpart, so it is hidden there.
const OFFLINE_SCOPE = document.documentElement.dataset.mmaPage === "settings";

// ---------------------------------------------------------------------------
// Module state
// ---------------------------------------------------------------------------

let schema = null;
let entries = [];
const formValues = {};
const formIncluded = {};
let editingId = null; // id of the custom preset being edited (null = new)

let listEl = null;
let formEl = null;
let editorEl = null;
let metaNameInput = null;
let metaDescInput = null;
let metaVersionInput = null;
let metaIdReadout = null;
let toastRoot = null;
let modalRoot = null;

// Form API (created in init() once schema is loaded).
let form = null;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function escapeHtml(value) {
    return String(value ?? "")
        .replace(/&/g, "&amp;")
        .replace(/</g, "&lt;")
        .replace(/>/g, "&gt;")
        .replace(/"/g, "&quot;")
        .replace(/'/g, "&#39;");
}

// ---------------------------------------------------------------------------
// Toast notifications (top-right, stacked, auto-dismiss)
// ---------------------------------------------------------------------------

function showToast(message, type = "info", duration = 3500) {
    if (!toastRoot) {
        return;
    }
    const toast = document.createElement("div");
    toast.className = `presets-toast presets-toast-${type}`;
    toast.textContent = message;
    toastRoot.appendChild(toast);
    setTimeout(() => toast.classList.add("show"), 10);
    setTimeout(() => {
        toast.classList.remove("show");
        setTimeout(() => toast.remove(), 300);
    }, duration);
}

// ---------------------------------------------------------------------------
// Confirmation modal (async)
// ---------------------------------------------------------------------------

function confirmDialog(message, { title = t("ui.presets.pleaseConfirm", "Please confirm"), danger = false } = {}) {
    return new Promise((resolve) => {
        modalRoot.textContent = "";
        modalRoot.hidden = false;

        const box = document.createElement("div");
        box.className = "presets-modal-box";

        const heading = document.createElement("h3");
        heading.className = "presets-modal-title";
        heading.textContent = title;

        const body = document.createElement("p");
        body.className = "presets-modal-message";
        body.textContent = message;

        const actions = document.createElement("div");
        actions.className = "presets-modal-actions";

        const cancelBtn = document.createElement("button");
        cancelBtn.type = "button";
        cancelBtn.className = "presets-btn";
        cancelBtn.textContent = t("ui.presets.cancel", "Cancel");
        cancelBtn.addEventListener("click", () => {
            modalRoot.hidden = true;
            resolve(false);
        });

        const okBtn = document.createElement("button");
        okBtn.type = "button";
        okBtn.className = `presets-btn ${danger ? "presets-btn-danger" : "presets-btn-primary"}`;
        okBtn.textContent = t("ui.presets.confirm", "Confirm");
        okBtn.addEventListener("click", () => {
            modalRoot.hidden = true;
            resolve(true);
        });

        actions.appendChild(cancelBtn);
        actions.appendChild(okBtn);
        box.appendChild(heading);
        box.appendChild(body);
        box.appendChild(actions);
        modalRoot.appendChild(box);
        okBtn.focus();
    });
}

/** Runs an action with uniform error reporting (no silent failures). */
function guarded(action) {
    return async (...args) => {
        try {
            await action(...args);
        } catch (error) {
            console.error("[presets] action failed:", error);
            showToast(`${t("ui.presets.unexpected", "Unexpected error")}: ${error?.message || error}`, "error", 6000);
        }
    };
}

// ---------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------

function injectStylesheet() {
    if (document.getElementById("presets-page-style")) {
        return;
    }
    const link = document.createElement("link");
    link.id = "presets-page-style";
    link.rel = "stylesheet";
    link.href = "./styles/presets.css";
    document.head.appendChild(link);
}

function buildLayout() {
    const root = document.getElementById("presets-app");
    root.textContent = "";

    const header = document.createElement("header");
    header.className = "presets-header";
    header.innerHTML = `
        <div class="presets-header-info">
            <h1 data-i18n="ui.presets.headerTitle">Presets Manager</h1>
            <p class="presets-header-sub" data-i18n="ui.presets.headerSub">Create, edit, apply, export and import preset configurations for the Mania Map Analyser overlay.</p>
        </div>
    `;
    applyStaticText(header);
    root.appendChild(header);

    const actionBar = document.createElement("div");
    actionBar.className = "presets-actionbar";
    actionBar.innerHTML = `
        <button id="act-new" class="presets-btn" type="button" data-i18n="ui.presets.new">New</button>
        <button id="act-save" class="presets-btn presets-btn-primary" type="button" data-i18n="ui.presets.save">Save</button>
        <button id="act-apply" class="presets-btn presets-btn-primary" type="button" data-i18n="ui.presets.applyChecked">Apply Checked</button>
        <button id="act-load-current" class="presets-btn" type="button" data-i18n="ui.presets.loadCurrent">Load Current</button>
        <span class="presets-actionbar-sep"></span>
        <button id="act-select-all" class="presets-btn" type="button" data-i18n="ui.presets.selectAll">Select All</button>
        <button id="act-invert" class="presets-btn" type="button" data-i18n="ui.presets.invert">Invert</button>
        <button id="act-clear" class="presets-btn" type="button" data-i18n="ui.presets.clearSelection">Clear Selection</button>
        <span class="presets-actionbar-sep"></span>
        <button id="act-export-current" class="presets-btn" type="button" data-i18n="ui.presets.exportCurrent">Export Current</button>
        <button id="act-export-all" class="presets-btn" type="button" data-i18n="ui.presets.exportAll">Export All</button>
        <button id="act-import" class="presets-btn" type="button" data-i18n="ui.presets.import">Import</button>
        <input id="preset-import-file" type="file" accept="application/json,.json" hidden>
        <span class="presets-actionbar-sep"></span>
        <a id="act-guide" class="presets-btn presets-btn-guide" href="https://github.com/LeoBlackMT/osumania_map_analyser/blob/main/docs/presets-guide.md"
            target="_blank" rel="noopener noreferrer" role="button">
            <span data-i18n="ui.presets.guide">Guide</span>
        </a>
    `;
    applyStaticText(actionBar);
    root.appendChild(actionBar);

    const main = document.createElement("main");
    main.className = "presets-main";

    listEl = document.createElement("aside");
    listEl.className = "presets-list";
    main.appendChild(listEl);

    const editorWrap = document.createElement("section");
    editorWrap.className = "presets-editor-wrap";

    const meta = document.createElement("div");
    meta.className = "presets-meta";
    meta.innerHTML = `
        <div class="presets-meta-title" data-i18n="ui.presets.metaTitle">Preset Info</div>
        <label class="presets-meta-field">
            <span data-i18n="ui.presets.name">Name</span>
            <input id="meta-name" type="text" maxlength="40" placeholder="my_preset">
            <small data-i18n="ui.presets.nameHint">English letters, digits, _ and - only, max 40 chars.</small>
        </label>
        <label class="presets-meta-field">
            <span data-i18n="ui.presets.description">Description</span>
            <input id="meta-desc" type="text" maxlength="200" placeholder="What is this preset for?" data-i18n-placeholder="ui.presets.descPlaceholder">
        </label>
        <div class="presets-meta-row">
            <label class="presets-meta-field">
                <span data-i18n="ui.presets.version">Version</span>
                <input id="meta-version" type="number" min="1" step="1" value="1">
                <small data-i18n="ui.presets.versionHint">Whole number; higher = newer.</small>
            </label>
            <label class="presets-meta-field">
                <span data-i18n="ui.presets.id">ID (auto)</span>
                <input id="meta-id" type="text" readonly>
            </label>
        </div>
    `;
    applyStaticText(meta);
    editorWrap.appendChild(meta);

    editorEl = document.createElement("div");
    editorEl.className = "presets-form-scroll";
    editorEl.innerHTML = '<p class="presets-empty">Loading settings…</p>';
    editorWrap.appendChild(editorEl);

    main.appendChild(editorWrap);
    root.appendChild(main);

    toastRoot = document.createElement("div");
    toastRoot.className = "presets-toast-container";
    document.body.appendChild(toastRoot);

    modalRoot = document.createElement("div");
    modalRoot.className = "presets-modal";
    modalRoot.hidden = true;
    document.body.appendChild(modalRoot);

    metaNameInput = document.getElementById("meta-name");
    metaDescInput = document.getElementById("meta-desc");
    metaVersionInput = document.getElementById("meta-version");
    metaIdReadout = document.getElementById("meta-id");

    wireActions(actionBar);
}

function wireActions(actionBar) {
    actionBar.querySelector("#act-new").addEventListener("click", guarded(async () => {
        const reset = await confirmDialog(t("ui.presets.clearEditorTitle", "Clear the editor and start a new preset?"), { title: t("ui.presets.newPreset", "New preset") });
        if (!reset) {
            return;
        }
        resetEditor();
        showToast(t("ui.presets.cleared", "Editor cleared — configure the form and save as a new preset."), "info");
    }));

    actionBar.querySelector("#act-save").addEventListener("click", guarded(() => saveCurrentPreset()));

    actionBar.querySelector("#act-apply").addEventListener("click", guarded(async () => {
        const snapshot = form.collectCheckedSnapshot();
        if (Object.keys(snapshot).length === 0) {
            showToast(t("ui.presets.nothingCheckedApply", "Nothing is checked to apply."), "error");
            return;
        }
        const ok = await confirmDialog(
            tf("ui.presets.applyCheckedConfirm", { count: Object.keys(snapshot).length }, `Apply the checked settings (${Object.keys(snapshot).length} fields) to the overlay now?`),
            { title: t("ui.presets.applySettings", "Apply settings") },
        );
        if (!ok) {
            return;
        }
        await applyCustomSnapshot(snapshot);
        showToast(t("ui.presets.applied", "Checked settings applied."), "success");
    }));

    actionBar.querySelector("#act-load-current").addEventListener("click", guarded(async () => {
        const current = await captureCurrentSettings();
        for (const [key, value] of Object.entries(current)) {
            if (key in formValues) {
                formValues[key] = value;
            }
        }
        form.syncFormControls();
        showToast(t("ui.presets.loadedCurrent", "Form values loaded from the current overlay settings."), "success");
    }));

    actionBar.querySelector("#act-select-all").addEventListener("click", () => {
        form.selectAllCheckboxes(true);
        showToast(t("ui.presets.allSelected", "All settings selected."), "info", 1800);
    });

    actionBar.querySelector("#act-invert").addEventListener("click", () => {
        form.invertCheckboxes();
        showToast(t("ui.presets.inverted", "Selection inverted."), "info", 1800);
    });

    actionBar.querySelector("#act-clear").addEventListener("click", () => {
        form.selectAllCheckboxes(false);
        showToast(t("ui.presets.clearedSelection", "Selection cleared."), "info", 1800);
    });

    actionBar.querySelector("#act-export-current").addEventListener("click", async () => {
        const name = metaNameInput.value.trim();
        if (!PRESET_NAME_RE.test(name)) {
            showToast(t("ui.presets.nameInvalidShort", "Enter a valid preset name first (letters, digits, _ or -; import needs it)."), "error");
            return;
        }
        const snapshot = form.collectCheckedSnapshot();
        if (Object.keys(snapshot).length === 0) {
            showToast(t("ui.presets.nothingToExport", "Nothing checked to export."), "error");
            return;
        }
        const version = Number(metaVersionInput.value);
        const exported = await exportCurrentToFile(name, metaDescInput.value.trim(), version, snapshot);
        if (exported) {
            showToast(t("ui.presets.exportedCurrent", "Current editor state exported."), "success");
        }
    });

    actionBar.querySelector("#act-export-all").addEventListener("click", async () => {
        const count = getCustomPresets().filter((p) => p.name !== AUTO_SAVE_PRESET_NAME).length;
        if (count === 0) {
            showToast(t("ui.presets.noCustomToExport", "No custom presets to export."), "error");
            return;
        }
        const saved = await exportLibraryToFile();
        if (saved) {
            showToast(tf("ui.presets.exportedCount", { count }, `Exported ${count} presets.`), "success");
        }
    });

    const importBtn = actionBar.querySelector("#act-import");
    const importInput = actionBar.querySelector("#preset-import-file");
    importBtn.addEventListener("click", () => importInput.click());
    importInput.addEventListener("change", guarded(async () => {
        const file = importInput.files && importInput.files[0];
        importInput.value = "";
        if (!file) {
            return;
        }
        const result = await importPresetFromFile(file);
        showToast(result.message, result.ok ? "success" : "error", result.ok ? 3500 : 6000);
    }));

    // Guide is an external link: inside the shell webview a plain
    // `<a target="_blank">` click does nothing, so it goes through the shared
    // helper (window.open, else copy + notice). A normal browser tab is unchanged.
    const guideLink = actionBar.querySelector("#act-guide");
    guideLink.addEventListener("click", (event) => {
        if (event && typeof event.preventDefault === "function") {
            event.preventDefault();
        }
        openExternalLink(guideLink.href).then((opened) => {
            if (!opened) {
                showToast(t("ui.settings.linkCopied", LINK_COPIED_NOTICE), "info", 6000);
            }
        });
    });
}

// ---------------------------------------------------------------------------
// Editor state
// ---------------------------------------------------------------------------

function resetEditor() {
    editingId = null;
    metaNameInput.value = "";
    metaDescInput.value = "";
    metaVersionInput.value = "1";
    metaIdReadout.value = "";
    form.fillFormFromDefaults();
    form.selectAllCheckboxes(false);
    highlightActiveRow(null);
}

async function loadPresetIntoEditor(preset, { isBuiltin = false, isDefault = false } = {}) {
    editingId = isBuiltin || isDefault ? null : preset.id;
    metaNameInput.value = preset.name || "";
    metaDescInput.value = preset.description || "";
    metaVersionInput.value = String(preset.version || 1);
    metaIdReadout.value = isBuiltin || isDefault ? "" : (preset.id || "");

    for (const key of Object.keys(formIncluded)) {
        formIncluded[key] = false;
    }
    for (const [key, value] of Object.entries(preset.settings || {})) {
        if (key in formValues) {
            formValues[key] = value;
            formIncluded[key] = true;
        }
    }
    form.syncFormControls();
    syncIncludeCheckboxes();
    highlightActiveRow(preset.name);
    showToast(tf("ui.presets.loadedEditor", { name: preset.name }, `"${preset.name}" loaded into the editor. Uncheck fields to exclude them.`), "info", 2500);
}

/** Syncs the include-checkbox DOM to the formIncluded state. */
function syncIncludeCheckboxes() {
    if (!formEl) {
        return;
    }
    const rows = formEl.querySelectorAll(".presets-setting");
    for (const row of rows) {
        const include = row.querySelector(".presets-setting-include");
        if (include) {
            include.checked = formIncluded[row.dataset.presetKey] === true;
        }
    }
}

async function saveCurrentPreset() {
    const name = metaNameInput.value.trim();
    if (!name) {
        showToast(t("ui.presets.nameRequired", "Enter a preset name first."), "error");
        return;
    }
    if (!PRESET_NAME_RE.test(name)) {
        showToast(t("ui.presets.nameInvalid", "Name must be English letters, digits, _ or - (max 40 chars)."), "error", 5000);
        return;
    }
    const description = metaDescInput.value.trim();
    const version = Number(metaVersionInput.value);
    const snapshot = form.collectCheckedSnapshot();
    if (Object.keys(snapshot).length === 0) {
        showToast(t("ui.presets.emptyPreset", "Nothing is checked — the preset would be empty."), "error");
        return;
    }

    const existing = getCustomPresets().find((preset) => preset.name === name);
    if (existing) {
        const ok = await confirmDialog(
            tf("ui.presets.existsOverwrite", { name }, `Preset "${name}" already exists. Overwrite it?`),
            { title: t("ui.presets.overwrite", "Overwrite preset"), danger: true },
        );
        if (!ok) {
            return;
        }
        const updated = createCustomPreset(name, snapshot, { description, version });
        if (!updated) {
            showToast(t("ui.presets.createFailed", "Preset could not be created (reserved or duplicate system name)."), "error");
            return;
        }
        editingId = updated.id;
        metaIdReadout.value = updated.id;
        if (didLastPersistFail()) {
            showToast(t("ui.presets.persistFailed", "Preset kept in memory only — the settings file could not be written."), "error", 6000);
        } else {
            showToast(tf("ui.presets.updated", { name }, `Preset "${name}" updated.`), "success");
        }
        return;
    }

    const created = createCustomPreset(name, snapshot, { description, version });
    if (!created) {
        showToast(t("ui.presets.createFailed", "Preset could not be created (reserved or duplicate system name)."), "error");
        return;
    }
    editingId = created.id;
    metaIdReadout.value = created.id;
    if (didLastPersistFail()) {
        showToast(t("ui.presets.persistFailed", "Preset kept in memory only — the settings file could not be written."), "error", 6000);
    } else {
        showToast(tf("ui.presets.saved", { name }, `Preset "${name}" saved.`), "success");
    }
}

// ---------------------------------------------------------------------------
// Preset list
// ---------------------------------------------------------------------------

function buildPresetRow(preset, { isSystem = false, active = false, actions = [] }) {
    const row = document.createElement("div");
    row.className = `presets-item${active ? " active" : ""}`;
    row.dataset.presetName = preset.name;

    const name = escapeHtml(preset.name);
    const desc = escapeHtml(preset.description || "");
    const version = Number.isInteger(preset.version) && preset.version > 0 ? `v${preset.version}` : "";

    const actionsHtml = actions.length > 0
        ? `<div class="presets-item-actions">${actions.map((action) => {
            const className = action === "apply" ? "presets-btn-apply" : action === "delete" ? "presets-btn-danger" : "";
            const label = t(`ui.presets.action.${action}`, action[0].toUpperCase() + action.slice(1));
            return `<button type="button" class="presets-btn ${className}" data-action="${action}">${escapeHtml(label)}</button>`;
        }).join("")}</div>`
        : "";

    row.innerHTML = `
        <div class="presets-item-info">
            <div class="presets-item-name">${name}${isSystem ? `<span class="presets-item-badge">${escapeHtml(t("ui.presets.systemBadge", "System"))}</span>` : ""}${version ? `<span class="presets-item-version">${escapeHtml(version)}</span>` : ""}</div>
            <div class="presets-item-desc"${preset.description ? ` title="${escapeHtml(preset.description)}"` : ""}>${desc}</div>
        </div>
        ${actionsHtml}
    `;
    return row;
}

/** 只读传输（tosu 在线）下无法交付库，别把占位文案写成「正在加载」。 */
function isReadOnlyTransport() {
    try {
        const transport = getPresetTransport();
        return Boolean(transport && typeof transport.isReadOnly === "function" && transport.isReadOnly());
    } catch {
        return false;
    }
}

function renderList() {
    if (!listEl) {
        return;
    }
    listEl.textContent = "";
    const activeName = getCurrentPreset();

    // Custom presets come first (they are the user's own).
    const customSection = document.createElement("div");
    customSection.className = "presets-section";
    customSection.textContent = t("ui.presets.myPresets", "My Presets");
    listEl.appendChild(customSection);

    const userPresets = getCustomPresets()
        .filter((preset) => preset.name !== AUTO_SAVE_PRESET_NAME)
        .filter((preset) => !getBuiltinPresets().some((builtin) => builtin.name === preset.name));

    if (userPresets.length === 0) {
        const empty = document.createElement("div");
        empty.className = "presets-empty";
        // 不再有「加载中」占位：挂载前已经主动拉过一次库，拉不到就直接说没有，
        // 免得任何时序问题把列表永久钉在「正在加载预设…」。
        empty.textContent = isReadOnlyTransport()
            ? t("ui.presets.readonly", "tosu owns the settings file — manage presets on tosu's Presets page.")
            : t("ui.presets.emptyList", "No custom presets yet. Configure the form and click Save.");
        listEl.appendChild(empty);
    } else {
        for (const preset of userPresets) {
            const isSlot = DEFAULT_SLOT_NAMES.includes(preset.name);
            const actions = ["edit", "apply", "export"]
                .concat(isSlot ? [] : ["rename", "delete"]);
            listEl.appendChild(buildPresetRow(preset, {
                isSystem: false,
                active: activeName === preset.name,
                actions,
            }));
        }
    }

    // System presets (Default first, then built-ins).
    const systemSection = document.createElement("div");
    systemSection.className = "presets-section";
    systemSection.textContent = t("ui.presets.system", "System");
    listEl.appendChild(systemSection);

    listEl.appendChild(buildPresetRow(
        {
            name: "Default",
            description: t("ui.presets.defaultDesc", "Reset to the factory default configuration (values from settings.json)."),
            version: 1,
        },
        {
            isSystem: true,
            active: activeName === "Default",
            actions: ["edit", "apply"],
        },
    ));

    for (const preset of getBuiltinPresets()) {
        listEl.appendChild(buildPresetRow(preset, {
            isSystem: true,
            active: activeName === preset.name,
            actions: ["edit", "apply"],
        }));
    }

    if (!OFFLINE_SCOPE) {
        listEl.appendChild(buildPresetRow(
            {
                name: AUTO_SAVE_PRESET_NAME,
                description: t("ui.presets.autoSaveDesc", "Automatically keeps the latest manual configuration after you change settings."),
            },
            { isSystem: true, active: activeName === AUTO_SAVE_PRESET_NAME, actions: [] },
        ));
    }
}

function highlightActiveRow(name) {
    const rows = listEl ? listEl.querySelectorAll(".presets-item") : [];
    for (const row of rows) {
        row.classList.toggle("editing", row.dataset.presetName === name);
    }
}

// ---------------------------------------------------------------------------
// List interactions
// ---------------------------------------------------------------------------

function startRename(row) {
    const nameEl = row.querySelector(".presets-item-name");
    const presetName = row.dataset.presetName;
    if (!nameEl || DEFAULT_SLOT_NAMES.includes(presetName)) {
        return;
    }
    const preset = getCustomPresets().find((item) => item.name === presetName);
    if (!preset) {
        return;
    }

    const input = document.createElement("input");
    input.type = "text";
    input.className = "presets-rename-input";
    input.value = preset.name;
    input.maxLength = 40;

    const info = row.querySelector(".presets-item-info");
    info.replaceChildren(input);

    const actions = row.querySelector(".presets-item-actions");
    actions.textContent = "";
    const confirmBtn = document.createElement("button");
    confirmBtn.type = "button";
    confirmBtn.className = "presets-btn";
    confirmBtn.textContent = t("ui.presets.save", "Save");
    confirmBtn.dataset.action = "rename-confirm";
    const cancelBtn = document.createElement("button");
    cancelBtn.type = "button";
    cancelBtn.className = "presets-btn";
    cancelBtn.textContent = t("ui.presets.cancel", "Cancel");
    cancelBtn.dataset.action = "rename-cancel";
    actions.appendChild(confirmBtn);
    actions.appendChild(cancelBtn);

    input.focus();
    input.select();
}

function finishRename(row) {
    const input = row.querySelector(".presets-rename-input");
    if (!input) {
        return;
    }
    const presetName = row.dataset.presetName;
    const preset = getCustomPresets().find((item) => item.name === presetName);
    if (!preset) {
        return;
    }
    if (!updatePresetMetadata(preset.id, { name: input.value })) {
        showToast(t("ui.presets.renameInvalid", "Invalid or duplicate name."), "error");
        renderList();
        return;
    }
    showToast(t("ui.presets.renamed", "Preset renamed."), "success");
}

/** Loads any preset (Default / built-in / custom) into the editor. */
async function loadPresetByName(name) {
    if (name === "Default") {
        const defaults = await buildDefaultSnapshot();
        await loadPresetIntoEditor({
            name: "Default",
            description: t("ui.presets.factoryDefault", "Factory default configuration."),
            version: 1,
            settings: defaults,
        }, { isDefault: true });
        return;
    }
    const builtin = getBuiltinPresets().find((preset) => preset.name === name);
    if (builtin) {
        const settings = getBuiltinSettings(builtin.id);
        await loadPresetIntoEditor({ ...builtin, settings: settings || {} }, { isBuiltin: true });
        return;
    }
    const custom = getCustomPresets().find((preset) => preset.name === name);
    if (custom) {
        await loadPresetIntoEditor(custom);
    }
}

async function handleListClick(event) {
    // Clicking anywhere on a row (not a button or input) loads it into the editor.
    const actionBtn = event.target.closest("[data-action]");
    if (!actionBtn) {
        const row = event.target.closest(".presets-item");
        if (row && row.dataset.presetName && !event.target.closest("input")) {
            await loadPresetByName(row.dataset.presetName);
        }
        return;
    }
    const row = actionBtn.closest(".presets-item");
    if (!row) {
        return;
    }
    const name = row.dataset.presetName;

    switch (actionBtn.dataset.action) {
        case "edit": {
            await loadPresetByName(name);
            return;
        }
        case "apply": {
            if (name === AUTO_SAVE_PRESET_NAME) {
                showToast(t("ui.presets.autoSaveFollow", "LastSavedPreset keeps following your manual changes."), "info");
                return;
            }
            const ok = await confirmDialog(tf("ui.presets.applyConfirm", { name }, `Apply preset "${name}" now?`), { title: t("ui.presets.applyPreset", "Apply preset") });
            if (!ok) {
                return;
            }
            if (await applyPresetByName(name)) {
                showToast(tf("ui.presets.appliedNamed", { name }, `Preset "${name}" applied.`), "success");
            } else {
                showToast(tf("ui.presets.notFound", { name }, `Preset "${name}" not found.`), "error");
            }
            break;
        }
        case "rename":
            startRename(row);
            break;
        case "rename-confirm":
            finishRename(row);
            break;
        case "rename-cancel":
            renderList();
            break;
        case "delete": {
            const preset = getCustomPresets().find((item) => item.name === name);
            if (!preset) {
                return;
            }
            const ok = await confirmDialog(
                tf("ui.presets.deleteConfirm", { name: preset.name }, `Delete preset "${preset.name}"? This cannot be undone.`),
                { title: t("ui.presets.deletePreset", "Delete preset"), danger: true },
            );
            if (ok) {
                deleteCustomPreset(preset.id);
                if (editingId === preset.id) {
                    resetEditor();
                }
                showToast(t("ui.presets.deleted", "Preset deleted."), "success");
            }
            break;
        }
        case "export": {
            const preset = getCustomPresets().find((item) => item.name === name);
            if (preset) {
                const saved = await exportPresetToFile(preset);
                if (saved) {
                    showToast(tf("ui.presets.exportedNamed", { name: preset.name }, `Preset "${preset.name}" exported.`), "success");
                }
            }
            break;
        }
        default:
            break;
    }
}

// ---------------------------------------------------------------------------
// Live value sync
// ---------------------------------------------------------------------------

/**
 * Mirrors one settings payload into the editor's form values, then refreshes
 * the rendered controls (focused inputs are skipped by the form itself).
 * The form values are updated FIRST so collectCheckedSnapshot() / Load Current
 * capture what the stream just delivered.
 * @param {Array|object|null|undefined} payload settings values, or a legacy
 *        tosu getSettings array; anything empty is ignored.
 */
function syncFromPayload(payload) {
    if (!payload) {
        return;
    }
    for (const key of Object.keys(formValues)) {
        if (payload[key] !== undefined) {
            formValues[key] = payload[key];
        }
    }
    form.syncFormControls();
}

/**
 * Subscribes the live value sync ONCE to the active preset transport
 * (getPresetTransport().mode): "tosu" receives raw settings-stream packets,
 * "shell" receives the settings page's {command:"getSettings", message} frames.
 * The transport owns the event source in both modes, so this page never talks
 * to `socket` directly and shares a single connection with the other
 * subscribers (core.js).
 */
function subscribeLiveValueSync() {
    const transport = getPresetTransport();
    if (transport.mode === "shell") {
        transport.subscribe((packet) => syncFromPayload(packet?.message));
        return;
    }
    transport.subscribe((packet) => {
        const payload = Array.isArray(packet)
            ? packet
            : (packet && typeof packet === "object" && packet.command === "getSettings" ? packet.message : null);
        syncFromPayload(payload);
    });
}

// ---------------------------------------------------------------------------
// Init
// ---------------------------------------------------------------------------

async function init() {
    // 独立打开 presets.html 时才自己解析语言；设置页已经解析过，再调会把它覆盖掉
    // （例如按 localStorage/系统语言重算，配置里的 language 就失效了）。
    if (!isLanguageInitialized()) {
        initLanguage();
    }
    // 先订阅再渲染：库装载完成的通知可能早于下面的首次渲染，晚订阅会把它丢掉，
    // 「我的预设」就永远停在「正在加载预设…」。
    onPresetsChanged(() => renderList());
    // Register the settings-stream listener and load the preset library.
    // (index.html does this via index.js; the manager page must do it itself.)
    initPresets();

    // 主动拉一次库：不依赖设置流/定时器是否已经跑过，挂载时就有数据。
    await ensureLibraryLoaded();
    // 默认锚点槽 Custom1..3 必须在首屏之前就位（它们不允许删除/重命名，
    // 是「应用即捕获当前配置」的入口）。ensureDefaultCustomSlots 是幂等的：
    // 只补缺失的槽，已有槽原样保留。
    await ensureDefaultCustomSlots();

    schema = await loadSettingsSchema();
    entries = schema.entries;

    injectStylesheet();
    buildLayout();

    form = createForm({
        entries,
        formValues,
        formIncluded,
        getFormEl: () => formEl,
        setFormEl: (el) => { formEl = el; },
        escapeHtml,
    });

    renderList();
    form.renderForm();

    listEl.addEventListener("click", guarded(handleListClick));

    // 上面的 await 期间库可能已经装载完成，收尾再渲染一次。
    renderList();

    // Live value sync from the settings stream (skip focused controls).
    subscribeLiveValueSync();
}

init();
