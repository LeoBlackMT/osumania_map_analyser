/**
 * Presets manager page (presets.html).
 * Left: categorized preset list (System incl. Default + My Presets).
 * Right: metadata panel + self-extending settings form (checkboxes control
 * which fields are included in the snapshot). Top: independent action bar.
 * All destructive actions require a confirmation modal; feedback uses toasts.
 */

import { socket } from "../appContext.js";
import {
    getCustomPresets,
    getBuiltinPresets,
    getBuiltinSettings,
    getCurrentPreset,
    onPresetsChanged,
    applyPresetByName,
    applyCustomSnapshot,
    createCustomPreset,
    updatePresetMetadata,
    deleteCustomPreset,
    captureCurrentSettings,
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

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

// System settings never shown in the editor form.
const EXCLUDED_KEYS = new Set(["preset", "presetStorage"]);
const PRESET_NAME_RE = /^[A-Za-z0-9_-]{1,40}$/;

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

function confirmDialog(message, { title = "Please confirm", danger = false } = {}) {
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
        cancelBtn.textContent = "Cancel";
        cancelBtn.addEventListener("click", () => {
            modalRoot.hidden = true;
            resolve(false);
        });

        const okBtn = document.createElement("button");
        okBtn.type = "button";
        okBtn.className = `presets-btn ${danger ? "presets-btn-danger" : "presets-btn-primary"}`;
        okBtn.textContent = "Confirm";
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
            <h1>Presets Manager</h1>
            <p class="presets-header-sub">Create, edit, apply, export and import preset configurations for the Mania Map Analyser overlay.</p>
        </div>
    `;
    root.appendChild(header);

    const actionBar = document.createElement("div");
    actionBar.className = "presets-actionbar";
    actionBar.innerHTML = `
        <button id="act-new" class="presets-btn" type="button">New</button>
        <button id="act-save" class="presets-btn presets-btn-primary" type="button">Save as Preset</button>
        <button id="act-apply" class="presets-btn presets-btn-primary" type="button">Apply Checked</button>
        <button id="act-load-current" class="presets-btn" type="button">Load Current</button>
        <span class="presets-actionbar-sep"></span>
        <button id="act-select-all" class="presets-btn" type="button">Select All</button>
        <button id="act-invert" class="presets-btn" type="button">Invert</button>
        <button id="act-clear" class="presets-btn" type="button">Clear Selection</button>
        <span class="presets-actionbar-sep"></span>
        <button id="act-export-current" class="presets-btn" type="button">Export Current</button>
        <button id="act-export-all" class="presets-btn" type="button">Export All</button>
        <button id="act-import" class="presets-btn" type="button">Import</button>
        <input id="preset-import-file" type="file" accept="application/json,.json" hidden>
    `;
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
        <div class="presets-meta-title">Preset Info</div>
        <label class="presets-meta-field">
            <span>Name</span>
            <input id="meta-name" type="text" maxlength="40" placeholder="my_preset">
            <small>English letters, digits, _ and - only, max 40 chars.</small>
        </label>
        <label class="presets-meta-field">
            <span>Description</span>
            <input id="meta-desc" type="text" maxlength="200" placeholder="What is this preset for?">
        </label>
        <div class="presets-meta-row">
            <label class="presets-meta-field">
                <span>Version</span>
                <input id="meta-version" type="number" min="1" step="1" value="1">
                <small>Whole number; higher = newer.</small>
            </label>
            <label class="presets-meta-field">
                <span>ID (auto)</span>
                <input id="meta-id" type="text" readonly>
            </label>
        </div>
    `;
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
    actionBar.querySelector("#act-new").addEventListener("click", async () => {
        const reset = await confirmDialog("Clear the editor and start a new preset?", { title: "New preset" });
        if (!reset) {
            return;
        }
        resetEditor();
        showToast("Editor cleared — configure the form and save as a new preset.", "info");
    });

    actionBar.querySelector("#act-save").addEventListener("click", () => saveCurrentPreset());

    actionBar.querySelector("#act-apply").addEventListener("click", async () => {
        const snapshot = collectCheckedSnapshot();
        if (Object.keys(snapshot).length === 0) {
            showToast("Nothing is checked to apply.", "error");
            return;
        }
        const ok = await confirmDialog(
            `Apply the checked settings (${Object.keys(snapshot).length} fields) to the overlay now?`,
            { title: "Apply settings" },
        );
        if (!ok) {
            return;
        }
        await applyCustomSnapshot(snapshot);
        showToast("Checked settings applied and synced to tosu.", "success");
    });

    actionBar.querySelector("#act-load-current").addEventListener("click", async () => {
        const current = await captureCurrentSettings();
        for (const [key, value] of Object.entries(current)) {
            if (key in formValues) {
                formValues[key] = value;
            }
        }
        syncFormControls();
        showToast("Form values loaded from the current overlay settings.", "success");
    });

    actionBar.querySelector("#act-select-all").addEventListener("click", () => {
        selectAllCheckboxes(true);
        showToast("All settings selected.", "info", 1800);
    });

    actionBar.querySelector("#act-invert").addEventListener("click", () => {
        invertCheckboxes();
        showToast("Selection inverted.", "info", 1800);
    });

    actionBar.querySelector("#act-clear").addEventListener("click", () => {
        selectAllCheckboxes(false);
        showToast("Selection cleared.", "info", 1800);
    });

    actionBar.querySelector("#act-export-current").addEventListener("click", () => {
        const name = metaNameInput.value.trim();
        if (!PRESET_NAME_RE.test(name)) {
            showToast("Enter a valid preset name first (letters, digits, _ or -; import needs it).", "error");
            return;
        }
        const snapshot = collectCheckedSnapshot();
        if (Object.keys(snapshot).length === 0) {
            showToast("Nothing checked to export.", "error");
            return;
        }
        const version = Number(metaVersionInput.value);
        exportCurrentToFile(name, metaDescInput.value.trim(), version, snapshot);
        showToast("Current editor state exported.", "success");
    });

    actionBar.querySelector("#act-export-all").addEventListener("click", () => {
        const count = getCustomPresets().filter((p) => p.name !== AUTO_SAVE_PRESET_NAME).length;
        if (count === 0) {
            showToast("No custom presets to export.", "error");
            return;
        }
        exportLibraryToFile();
        showToast(`Exported ${count} presets.`, "success");
    });

    const importBtn = actionBar.querySelector("#act-import");
    const importInput = actionBar.querySelector("#preset-import-file");
    importBtn.addEventListener("click", () => importInput.click());
    importInput.addEventListener("change", async () => {
        const file = importInput.files && importInput.files[0];
        importInput.value = "";
        if (!file) {
            return;
        }
        const result = await importPresetFromFile(file);
        showToast(result.message, result.ok ? "success" : "error", result.ok ? 3500 : 6000);
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
    fillFormFromDefaults();
    selectAllCheckboxes(false);
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
    syncFormControls();
    highlightActiveRow(preset.name);
    showToast(`"${preset.name}" loaded into the editor. Uncheck fields to exclude them.`, "info", 2500);
}

async function saveCurrentPreset() {
    const name = metaNameInput.value.trim();
    if (!name) {
        showToast("Enter a preset name first.", "error");
        return;
    }
    if (!PRESET_NAME_RE.test(name)) {
        showToast("Name must be English letters, digits, _ or - (max 40 chars).", "error", 5000);
        return;
    }
    const description = metaDescInput.value.trim();
    const version = Number(metaVersionInput.value);
    const snapshot = collectCheckedSnapshot();
    if (Object.keys(snapshot).length === 0) {
        showToast("Nothing is checked — the preset would be empty.", "error");
        return;
    }

    const existing = getCustomPresets().find((preset) => preset.name === name);
    if (existing) {
        const ok = await confirmDialog(
            `Preset "${name}" already exists. Overwrite it?`,
            { title: "Overwrite preset", danger: true },
        );
        if (!ok) {
            return;
        }
        const updated = createCustomPreset(name, snapshot, { description, version });
        if (!updated) {
            showToast("Preset could not be created (reserved or duplicate system name).", "error");
            return;
        }
        editingId = updated.id;
        metaIdReadout.value = updated.id;
        showToast(`Preset "${name}" updated.`, "success");
        return;
    }

    const created = createCustomPreset(name, snapshot, { description, version });
    if (!created) {
        showToast("Preset could not be created (reserved or duplicate system name).", "error");
        return;
    }
    editingId = created.id;
    metaIdReadout.value = created.id;
    showToast(`Preset "${name}" saved.`, "success");
}

// ---------------------------------------------------------------------------
// Form (self-extending, generated from settings.json)
// ---------------------------------------------------------------------------

function renderForm() {
    const wrap = document.getElementById("presets-app").querySelector(".presets-form-scroll");
    wrap.textContent = "";
    formEl = document.createElement("div");
    formEl.className = "presets-form";
    wrap.appendChild(formEl);

    let currentGroup = null;
    let pendingHeader = null;
    for (const entry of entries) {
        if (EXCLUDED_KEYS.has(entry.uniqueID)) {
            continue;
        }
        if (entry.type === "header") {
            // Groups are created lazily: a header with no following setting
            // (e.g. Links, whose entries are all buttons) renders nothing.
            currentGroup = null;
            pendingHeader = entry;
            continue;
        }
        if (entry.type === "button") {
            continue;
        }
        if (!currentGroup) {
            currentGroup = document.createElement("div");
            currentGroup.className = "presets-group";
            if (pendingHeader) {
                const title = document.createElement("h2");
                title.className = "presets-group-title";
                title.textContent = pendingHeader.title;
                currentGroup.appendChild(title);
                pendingHeader = null;
            }
            formEl.appendChild(currentGroup);
        }
        currentGroup.appendChild(buildSettingRow(entry));
    }
}

function buildSettingRow(entry) {
    const key = entry.uniqueID;
    formValues[key] = entry.value;
    // Default: nothing included — the user checks what they want to manage.
    formIncluded[key] = false;

    const row = document.createElement("label");
    row.className = "presets-setting";
    row.dataset.presetKey = key;

    const include = document.createElement("input");
    include.type = "checkbox";
    include.className = "presets-setting-include";
    include.checked = formIncluded[key];
    include.addEventListener("change", () => {
        formIncluded[key] = include.checked;
    });

    const info = document.createElement("span");
    info.className = "presets-setting-info";
    info.innerHTML = `<span class="presets-setting-title">${escapeHtml(entry.title)}</span>`
        + (entry.description ? `<span class="presets-setting-desc">${escapeHtml(entry.description)}</span>` : "");

    const control = buildControl(entry, key);

    row.appendChild(include);
    row.appendChild(info);
    row.appendChild(control);
    return row;
}

function buildControl(entry, key) {
    const control = document.createElement("span");
    control.className = "presets-setting-control";
    const current = formValues[key];

    switch (entry.type) {
        case "checkbox": {
            const input = document.createElement("input");
            input.type = "checkbox";
            input.dataset.presetKey = key;
            input.checked = current === true;
            input.addEventListener("change", () => {
                formValues[key] = input.checked;
            });
            control.appendChild(input);
            break;
        }
        case "options": {
            const select = document.createElement("select");
            select.dataset.presetKey = key;
            for (const option of entry.options || []) {
                const optionEl = document.createElement("option");
                optionEl.value = option;
                optionEl.textContent = option;
                if (option === current) {
                    optionEl.selected = true;
                }
                select.appendChild(optionEl);
            }
            select.addEventListener("change", () => {
                formValues[key] = select.value;
            });
            control.appendChild(select);
            break;
        }
        case "color": {
            const input = document.createElement("input");
            input.type = "color";
            input.dataset.presetKey = key;
            input.value = String(current || "#000000");
            input.addEventListener("input", () => {
                formValues[key] = input.value;
            });
            control.appendChild(input);
            break;
        }
        case "number": {
            const input = document.createElement("input");
            input.type = "number";
            input.dataset.presetKey = key;
            input.value = String(current ?? "");
            input.addEventListener("input", () => {
                formValues[key] = Number.isFinite(Number(input.value)) ? Number(input.value) : input.value;
            });
            control.appendChild(input);
            break;
        }
        case "commands": {
            const readout = document.createElement("span");
            readout.className = "presets-setting-readonly";
            readout.textContent = `[commands] ${JSON.stringify(current ?? [])}`;
            control.appendChild(readout);
            break;
        }
        default: {
            const input = document.createElement("input");
            input.type = "text";
            input.dataset.presetKey = key;
            input.value = String(current ?? "");
            input.addEventListener("input", () => {
                formValues[key] = input.value;
            });
            control.appendChild(input);
        }
    }
    return control;
}

function syncFormControls() {
    if (!formEl) {
        return;
    }
    const rows = formEl.querySelectorAll(".presets-setting");
    for (const row of rows) {
        const key = row.dataset.presetKey;
        if (!key || !(key in formValues)) {
            continue;
        }
        const control = row.querySelector(".presets-setting-control");
        const input = control && control.firstElementChild;
        if (!input || document.activeElement === input) {
            continue;
        }
        const value = formValues[key];
        if (input.tagName === "SELECT") {
            input.value = value ?? "";
        } else if (input.type === "checkbox") {
            input.checked = value === true;
        } else if (input.type === "color") {
            input.value = String(value || "#000000");
        } else if (input.type === "number") {
            input.value = String(value ?? "");
        } else if (input.type === "text") {
            input.value = String(value ?? "");
        }
    }
}

function fillFormFromDefaults() {
    for (const entry of entries) {
        if (EXCLUDED_KEYS.has(entry.uniqueID) || entry.type === "header" || entry.type === "button") {
            continue;
        }
        if (!(entry.uniqueID in formValues)) {
            continue;
        }
        formValues[entry.uniqueID] = entry.value;
    }
    syncFormControls();
}

function selectAllCheckboxes(checked) {
    if (!formEl) {
        return;
    }
    const rows = formEl.querySelectorAll(".presets-setting");
    for (const row of rows) {
        const include = row.querySelector(".presets-setting-include");
        if (include) {
            include.checked = checked;
            formIncluded[row.dataset.presetKey] = checked;
        }
    }
}

function invertCheckboxes() {
    if (!formEl) {
        return;
    }
    const rows = formEl.querySelectorAll(".presets-setting");
    for (const row of rows) {
        const include = row.querySelector(".presets-setting-include");
        if (include) {
            const next = !include.checked;
            include.checked = next;
            formIncluded[row.dataset.presetKey] = next;
        }
    }
}

function collectCheckedSnapshot() {
    const snapshot = {};
    for (const [key, included] of Object.entries(formIncluded)) {
        if (included && key in formValues) {
            snapshot[key] = formValues[key];
        }
    }
    return snapshot;
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
            return `<button type="button" class="presets-btn ${className}" data-action="${action}">${action[0].toUpperCase()}${action.slice(1)}</button>`;
        }).join("")}</div>`
        : "";

    row.innerHTML = `
        <div class="presets-item-info">
            <div class="presets-item-name">${name}${isSystem ? '<span class="presets-item-badge">System</span>' : ""}${version ? `<span class="presets-item-version">${escapeHtml(version)}</span>` : ""}</div>
            <div class="presets-item-desc"${preset.description ? ` title="${escapeHtml(preset.description)}"` : ""}>${desc}</div>
        </div>
        ${actionsHtml}
    `;
    return row;
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
    customSection.textContent = "My Presets";
    listEl.appendChild(customSection);

    const userPresets = getCustomPresets()
        .filter((preset) => preset.name !== AUTO_SAVE_PRESET_NAME)
        .filter((preset) => !getBuiltinPresets().some((builtin) => builtin.name === preset.name));

    if (userPresets.length === 0) {
        const empty = document.createElement("div");
        empty.className = "presets-empty";
        empty.textContent = "No custom presets yet. Configure the form and click Save as Preset.";
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
    systemSection.textContent = "System";
    listEl.appendChild(systemSection);

    listEl.appendChild(buildPresetRow(
        {
            name: "Default",
            description: "Reset to the factory default configuration (values from settings.json).",
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

    listEl.appendChild(buildPresetRow(
        {
            name: AUTO_SAVE_PRESET_NAME,
            description: "Automatically keeps the latest manual configuration after you change settings.",
        },
        { isSystem: true, active: activeName === AUTO_SAVE_PRESET_NAME, actions: [] },
    ));
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
    confirmBtn.textContent = "Save";
    confirmBtn.dataset.action = "rename-confirm";
    const cancelBtn = document.createElement("button");
    cancelBtn.type = "button";
    cancelBtn.className = "presets-btn";
    cancelBtn.textContent = "Cancel";
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
        showToast("Invalid or duplicate name.", "error");
        renderList();
        return;
    }
    showToast("Preset renamed.", "success");
}

/** Loads any preset (Default / built-in / custom) into the editor. */
async function loadPresetByName(name) {
    if (name === "Default") {
        const defaults = await buildDefaultSnapshot();
        await loadPresetIntoEditor({
            name: "Default",
            description: "Factory default configuration.",
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
                showToast("LastSavedPreset keeps following your manual changes.", "info");
                return;
            }
            const ok = await confirmDialog(`Apply preset "${name}" now?`, { title: "Apply preset" });
            if (!ok) {
                return;
            }
            if (await applyPresetByName(name)) {
                showToast(`Preset "${name}" applied and synced to tosu.`, "success");
            } else {
                showToast(`Preset "${name}" not found.`, "error");
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
                `Delete preset "${preset.name}"? This cannot be undone.`,
                { title: "Delete preset", danger: true },
            );
            if (ok) {
                deleteCustomPreset(preset.id);
                if (editingId === preset.id) {
                    resetEditor();
                }
                showToast("Preset deleted.", "success");
            }
            break;
        }
        case "export": {
            const preset = getCustomPresets().find((item) => item.name === name);
            if (preset) {
                exportPresetToFile(preset);
                showToast(`Preset "${preset.name}" exported.`, "success");
            }
            break;
        }
        default:
            break;
    }
}

// ---------------------------------------------------------------------------
// Init
// ---------------------------------------------------------------------------

async function init() {
    // Register the settings-stream listener and load the preset library.
    // (index.html does this via index.js; the manager page must do it itself.)
    initPresets();

    schema = await loadSettingsSchema();
    entries = schema.entries;

    injectStylesheet();
    buildLayout();
    renderList();
    renderForm();

    listEl.addEventListener("click", handleListClick);
    onPresetsChanged(() => renderList());

    // Live value sync from the tosu settings stream (skip focused controls).
    socket.commands((packet) => {
        const payload = Array.isArray(packet)
            ? packet
            : (packet && typeof packet === "object" && packet.command === "getSettings" ? packet.message : null);
        if (!payload) {
            return;
        }
        for (const key of Object.keys(formValues)) {
            if (payload[key] !== undefined) {
                formValues[key] = payload[key];
            }
        }
        syncFormControls();
    });
}

init();
