/**
 * Settings form rendering for the preset manager page — split out of manager.js.
 * Pure DOM construction driven by the settings schema entries; all mutable
 * state (formValues / formIncluded / formEl) is injected via the api object so
 * manager.js stays the single owner of page state.
 */

import { t } from "../i18n/index.js";
import { state } from "../core/appContext.js";
import { isShellRuntime } from "../feedback/feedbackSpec.js";
import { parseCombinedModeModuleList } from "../ui/combinedMode.js";

const EXCLUDED_KEYS = new Set(["preset", "presetStorage"]);

// 桌面壳专属表单键（仅壳内渲染）。它们不直接进快照：collectCheckedSnapshot
// 把勾选的字段合并成一个 `combinedMode` 对象，未勾选的字段回落当前卡片 state。
const SHELL_FORM_KEYS = new Set(["shellCombinedEnabled", "shellCombinedModules", "shellLaneAccSpaceMode"]);

/**
 * @param {object} api
 * @param {Array} api.entries settings schema entries (from loadSettingsSchema)
 * @param {object} api.formValues key -> current value
 * @param {object} api.formIncluded key -> boolean (checked = included in snapshot)
 * @param {() => HTMLElement|null} api.getFormEl
 * @param {(el: HTMLElement|null) => void} api.setFormEl
 * @param {(s: string) => string} api.escapeHtml
 */
export function createForm(api) {
    const { entries, formValues, formIncluded, getFormEl, setFormEl, escapeHtml } = api;

    function renderForm() {
        const wrap = document.getElementById("presets-app").querySelector(".presets-form-scroll");
        wrap.textContent = "";
        const formEl = document.createElement("div");
        formEl.className = "presets-form";
        wrap.appendChild(formEl);
        setFormEl(formEl);

        // 桌面壳专属组永远在最前（在「模块自定义」等所有 settings.json 分组
        // 之前）；浏览器/OBS 页面不支持这些字段，整组不渲染。
        if (isShellRuntime()) {
            formEl.appendChild(buildShellGroup());
        }

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
                    title.textContent = t(`settings.${pendingHeader.uniqueID}.title`, pendingHeader.title);
                    currentGroup.appendChild(title);
                    pendingHeader = null;
                }
                formEl.appendChild(currentGroup);
            }
            currentGroup.appendChild(buildSettingRow(entry));
        }
    }

    /**
     * 「桌面壳专属」组：combinedMode 的开关与模块顺序两行。初值取当前卡片
     * state（Load Current 之外唯一的实时来源），默认不勾选。
     */
    function buildShellGroup() {
        const group = document.createElement("div");
        group.className = "presets-group presets-group-shell";
        const title = document.createElement("h2");
        title.className = "presets-group-title";
        title.textContent = t("ui.presets.shellGroup.title", "Desktop Shell Only");
        group.appendChild(title);
        group.appendChild(buildSettingRow({
            uniqueID: "shellCombinedEnabled",
            type: "checkbox",
            value: state.combinedModeEnabled === true,
            title: "Combined mode",
            description: "Show several card sections at once (desktop shell only).",
        }));
        group.appendChild(buildSettingRow({
            uniqueID: "shellCombinedModules",
            type: "text",
            value: [...state.combinedModeModules].join(", "),
            title: t("ui.shell.combinedMode.modulesTitle", "Sections and order"),
            description: t("ui.shell.combinedMode.modulesDescription", "Comma-separated: Pattern, Etterna, Graph, ReworkPP, LaneAcc"),
        }));
        group.appendChild(buildSettingRow({
            uniqueID: "shellLaneAccSpaceMode",
            type: "options",
            options: ["independent", "left", "right"],
            value: state.laneAccSpaceMode || "independent",
            title: t("ui.shell.laneAcc.space.title", "Odd-key middle lane"),
            description: t("ui.shell.laneAcc.space.description", "independent = separate Space hand; left/right = join that hand."),
        }));
        return group;
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
        info.innerHTML = `<span class="presets-setting-title">${escapeHtml(t(`settings.${key}.title`, entry.title))}</span>`
            + (entry.description ? `<span class="presets-setting-desc">${escapeHtml(t(`settings.${key}.description`, entry.description))}</span>` : "");

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
                    optionEl.textContent = t(`option.${option}`, option);
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
        const formEl = getFormEl();
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
        // 壳专属行回落到当前卡片 state（新预设 = 跟随现状，而非凭空默认）。
        if ("shellCombinedEnabled" in formValues) {
            formValues.shellCombinedEnabled = state.combinedModeEnabled === true;
            formValues.shellCombinedModules = [...state.combinedModeModules].join(", ");
        }
        syncFormControls();
    }

    function selectAllCheckboxes(checked) {
        const formEl = getFormEl();
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
        const formEl = getFormEl();
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
            if (!included || !(key in formValues) || SHELL_FORM_KEYS.has(key)) {
                continue;
            }
            snapshot[key] = formValues[key];
        }
        // 壳专属两行合并为一个 combinedMode 对象；未勾选的字段回落当前卡片
        // state。只在壳内可能发生（浏览器根本不渲染这两行）。
        if (formIncluded.shellCombinedEnabled || formIncluded.shellCombinedModules) {
            snapshot.combinedMode = {
                enabled: formIncluded.shellCombinedEnabled
                    ? formValues.shellCombinedEnabled === true
                    : state.combinedModeEnabled === true,
                modules: formIncluded.shellCombinedModules
                    ? parseCombinedModeModuleList(formValues.shellCombinedModules)
                    : [...state.combinedModeModules],
            };
        }
        if (formIncluded.shellLaneAccSpaceMode) {
            snapshot.laneAccSpaceMode = formValues.shellLaneAccSpaceMode;
        }
        return snapshot;
    }

    return {
        renderForm,
        syncFormControls,
        fillFormFromDefaults,
        selectAllCheckboxes,
        invertCheckboxes,
        collectCheckedSnapshot,
    };
}
