/**
 * Settings form rendering for the preset manager page — split out of manager.js.
 * Pure DOM construction driven by the settings schema entries; all mutable
 * state (formValues / formIncluded / formEl) is injected via the api object so
 * manager.js stays the single owner of page state.
 */

const EXCLUDED_KEYS = new Set(["preset", "presetStorage"]);

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
            if (included && key in formValues) {
                snapshot[key] = formValues[key];
            }
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
