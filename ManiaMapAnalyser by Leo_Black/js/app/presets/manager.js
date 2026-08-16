/**
 * Presets manager page (presets.html).
 * Renders a self-extending settings form (generated from settings.json),
 * the preset list, CRUD, Apply / Save actions and export/import.
 */

import { socket } from "../appContext.js";
import {
    getCustomPresets,
    getBuiltinPresets,
    getCurrentPreset,
    onPresetsChanged,
    applyPresetByName,
    applyCustomSnapshot,
    createCustomPreset,
    renameCustomPreset,
    deleteCustomPreset,
} from "./core.js";
import {
    exportPresetToFile,
    exportLibraryToFile,
    importPresetFromFile,
} from "./io.js";
import { loadSettingsSchema } from "./schema.js";
import {
    AUTO_SAVE_PRESET_NAME,
    DEFAULT_SLOT_NAMES,
} from "./storage.js";

// ---------------------------------------------------------------------------
// Module state
// ---------------------------------------------------------------------------

let schema = null;
let entries = [];
const formValues = {};
const formIncluded = {};

let listEl = null;
let formEl = null;
let hintEl = null;
let saveNameInput = null;
let saveBtn = null;
let applyBtn = null;
let loadCurrentBtn = null;
let exportAllBtn = null;
let importInput = null;
let editorEl = null;

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

function showHint(message, isError) {
    if (!hintEl) {
        return;
    }
    hintEl.textContent = message;
    hintEl.classList.toggle("error", Boolean(isError));
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
        <h1>Presets Manager</h1>
        <div class="presets-header-actions">
            <input id="preset-save-name" class="presets-save-name" type="text"
                   placeholder="New preset name..." maxlength="40">
            <button id="preset-save-btn" class="presets-btn presets-btn-primary" type="button">Save as Preset</button>
            <button id="preset-apply-btn" class="presets-btn presets-btn-primary" type="button">Apply Checked</button>
            <button id="preset-load-current-btn" class="presets-btn" type="button">Use Current Settings</button>
            <button id="preset-export-all-btn" class="presets-btn" type="button">Export All</button>
            <button id="preset-import-btn" class="presets-btn" type="button">Import</button>
            <input id="preset-import-file" type="file" accept="application/json,.json" hidden>
        </div>
    `;
    root.appendChild(header);

    const main = document.createElement("main");
    main.className = "presets-main";

    listEl = document.createElement("aside");
    listEl.className = "presets-list";
    main.appendChild(listEl);

    const editorWrap = document.createElement("section");
    editorWrap.className = "presets-editor-wrap";
    editorEl = document.createElement("div");
    editorEl.className = "presets-editor";
    editorEl.textContent = "Loading settings…";
    editorWrap.appendChild(editorEl);
    main.appendChild(editorWrap);

    root.appendChild(main);

    hintEl = document.createElement("p");
    hintEl.className = "presets-hint";
    root.appendChild(hintEl);

    saveNameInput = document.getElementById("preset-save-name");
    saveBtn = document.getElementById("preset-save-btn");
    applyBtn = document.getElementById("preset-apply-btn");
    loadCurrentBtn = document.getElementById("preset-load-current-btn");
    exportAllBtn = document.getElementById("preset-export-all-btn");
    importInput = document.getElementById("preset-import-file");

    saveBtn.addEventListener("click", () => {
        const name = String(saveNameInput.value || "").trim();
        if (!name) {
            showHint("Enter a preset name first.", true);
            return;
        }
        const existed = getCustomPresets().some((preset) => preset.name === name);
        const preset = createCustomPreset(name, collectCheckedSnapshot());
        if (!preset) {
            showHint("Invalid preset name (reserved or duplicate system name).", true);
            return;
        }
        saveNameInput.value = "";
        showHint(existed ? `Preset "${preset.name}" updated.` : `Preset "${preset.name}" saved.`, false);
    });

    applyBtn.addEventListener("click", async () => {
        const snapshot = collectCheckedSnapshot();
        if (Object.keys(snapshot).length === 0) {
            showHint("Nothing checked to apply.", true);
            return;
        }
        await applyCustomSnapshot(snapshot);
        showHint("Checked settings applied and synced to tosu.", false);
    });

    loadCurrentBtn.addEventListener("click", () => {
        fillFormFromDefaults();
        showHint("Form reset to factory defaults (values refresh from tosu broadcast).", false);
    });

    exportAllBtn.addEventListener("click", () => {
        if (getCustomPresets().filter((p) => p.name !== AUTO_SAVE_PRESET_NAME).length === 0) {
            showHint("No custom presets to export.", true);
            return;
        }
        exportLibraryToFile();
    });

    const importBtn = document.getElementById("preset-import-btn");
    importBtn.addEventListener("click", () => importInput.click());
    importInput.addEventListener("change", async () => {
        const file = importInput.files && importInput.files[0];
        importInput.value = "";
        if (!file) {
            return;
        }
        const result = await importPresetFromFile(file);
        showHint(result.message, !result.ok);
    });
}

// ---------------------------------------------------------------------------
// Form (self-extending, generated from settings.json)
// ---------------------------------------------------------------------------

function renderForm() {
    editorEl.textContent = "";
    formEl = document.createElement("div");
    formEl.className = "presets-form";
    editorEl.appendChild(formEl);

    let currentGroup = null;
    for (const entry of entries) {
        if (entry.type === "header") {
            currentGroup = document.createElement("div");
            currentGroup.className = "presets-group";
            const title = document.createElement("h2");
            title.className = "presets-group-title";
            title.textContent = entry.title;
            currentGroup.appendChild(title);
            formEl.appendChild(currentGroup);
            continue;
        }
        if (entry.type === "button") {
            continue;
        }
        if (!currentGroup) {
            currentGroup = document.createElement("div");
            currentGroup.className = "presets-group";
            formEl.appendChild(currentGroup);
        }
        currentGroup.appendChild(buildSettingRow(entry));
    }
}

function buildSettingRow(entry) {
    const key = entry.uniqueID;
    formValues[key] = entry.value;
    formIncluded[key] = true;

    const row = document.createElement("label");
    row.className = "presets-setting";
    row.dataset.presetKey = key;

    const include = document.createElement("input");
    include.type = "checkbox";
    include.className = "presets-setting-include";
    include.checked = true;
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
    const rows = formEl ? formEl.querySelectorAll(".presets-setting") : [];
    for (const row of rows) {
        const key = row.dataset.presetKey;
        if (!key || !(key in formValues)) {
            continue;
        }
        const control = row.querySelector(".presets-setting-control");
        if (!control) {
            continue;
        }
        const input = control.firstElementChild;
        if (!input) {
            continue;
        }
        if (input.type === "checkbox" && input.classList.contains("presets-setting-include")) {
            continue;
        }
        if (document.activeElement === input) {
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
        if (entry.type === "header" || entry.type === "button" || !(entry.uniqueID in formValues)) {
            continue;
        }
        formValues[entry.uniqueID] = entry.value;
    }
    syncFormControls();
}

/** Builds the partial snapshot from the checked form fields. */
function collectCheckedSnapshot() {
    const snapshot = {};
    for (const [key, included] of Object.entries(formIncluded)) {
        if (included && key in formValues) {
            snapshot[key] = formValues[key];
        }
    }
    return snapshot;
}

function loadPresetIntoForm(preset) {
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
    const rows = formEl ? formEl.querySelectorAll(".presets-setting") : [];
    for (const row of rows) {
        const include = row.querySelector(".presets-setting-include");
        if (include) {
            include.checked = formIncluded[row.dataset.presetKey] === true;
        }
    }
}

// ---------------------------------------------------------------------------
// Preset list
// ---------------------------------------------------------------------------

function buildPresetRow(preset, { isSystem, active, actions }) {
    const row = document.createElement("div");
    row.className = `presets-item${active ? " active" : ""}`;
    row.dataset.presetName = preset.name;

    const name = escapeHtml(preset.name);
    const desc = escapeHtml(preset.description || "");
    let actionsHtml = "";
    if (actions !== "none") {
        actionsHtml = `<div class="presets-item-actions">
            <button type="button" class="presets-btn presets-btn-apply" data-action="apply">Apply</button>
            ${actions === "all"
                ? `<button type="button" class="presets-btn" data-action="edit">Edit</button>
                   <button type="button" class="presets-btn" data-action="rename">Rename</button>
                   <button type="button" class="presets-btn presets-btn-danger" data-action="delete">Delete</button>
                   <button type="button" class="presets-btn" data-action="export">Export</button>`
                : ""}
        </div>`;
    }

    row.innerHTML = `
        <div class="presets-item-info">
            <div class="presets-item-name">${name}${isSystem ? '<span class="presets-item-badge">System</span>' : ""}</div>
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

    // Custom presets.
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
            listEl.appendChild(buildPresetRow(preset, {
                isSystem: false,
                active: activeName === preset.name,
                actions: DEFAULT_SLOT_NAMES.includes(preset.name) ? "apply" : "all",
            }));
        }
    }

    // System presets.
    const systemSection = document.createElement("div");
    systemSection.className = "presets-section";
    systemSection.textContent = "System";
    listEl.appendChild(systemSection);

    for (const preset of getBuiltinPresets()) {
        listEl.appendChild(buildPresetRow(preset, {
            isSystem: true,
            active: activeName === preset.name,
        }));
    }

    listEl.appendChild(buildPresetRow(
        {
            name: AUTO_SAVE_PRESET_NAME,
            description: "Automatically keeps the latest manual configuration after you change settings.",
        },
        { isSystem: true, active: activeName === AUTO_SAVE_PRESET_NAME, actions: "none" },
    ));
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
    if (!renameCustomPreset(preset.id, input.value)) {
        showHint("Invalid or duplicate name.", true);
        renderList();
        return;
    }
    showHint("", false);
}

async function handleListClick(event) {
    const actionBtn = event.target.closest("[data-action]");
    if (!actionBtn) {
        return;
    }
    const row = actionBtn.closest(".presets-item");
    if (!row) {
        return;
    }
    const name = row.dataset.presetName;

    switch (actionBtn.dataset.action) {
        case "apply": {
            if (name === AUTO_SAVE_PRESET_NAME) {
                showHint("Last Saved Preset keeps following your manual changes.", false);
                return;
            }
            if (await applyPresetByName(name)) {
                showHint(`Preset "${name}" applied and synced to tosu.`, false);
            } else {
                showHint(`Preset "${name}" not found.`, true);
            }
            break;
        }
        case "edit": {
            const preset = getCustomPresets().find((item) => item.name === name);
            if (preset) {
                loadPresetIntoForm(preset);
                showHint(`Preset "${name}" loaded into the form. Uncheck fields to exclude them.`, false);
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
            if (window.confirm(`Delete preset "${preset.name}"?`)) {
                deleteCustomPreset(preset.id);
                showHint("Preset deleted.", false);
            }
            break;
        }
        case "export": {
            const preset = getCustomPresets().find((item) => item.name === name);
            if (preset) {
                exportPresetToFile(preset);
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
