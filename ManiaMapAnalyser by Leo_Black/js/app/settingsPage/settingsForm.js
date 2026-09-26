/**
 * Plugin-settings form for the desktop settings page (settings.html).
 *
 * Same control shapes as presets/form.js (checkbox / options / color / number /
 * commands / text) and the same header grouping, but:
 *   - no "include in snapshot" checkbox column — this page edits settings, the
 *     presets editor keeps that column (see the plan, Step 10);
 *   - `preset` / `presetStorage` are never rendered, and neither are buttons;
 *   - only keys with an applier in the settings schema are rendered, exactly the
 *     set the preset system can apply — so both editors agree on what a
 *     "setting" is;
 *   - every `change` commits immediately through the caller's onCommit.
 */

const EXCLUDED_KEYS = new Set(["preset", "presetStorage"]);

/**
 * @param {object} options
 * @param {HTMLElement} options.root container the form is rendered into.
 * @param {object} options.schema settings schema ({entries, appliers, defaults}).
 * @param {object} [options.initial] current values (GET /settings); a key that is
 *        absent falls back to the settings.json default.
 * @param {(key: string, value: any, previous: any) => void} options.onCommit
 *        called on every accepted change.
 */
export function createSettingsForm({ root, schema, initial = {}, onCommit }) {
    const { entries, appliers, defaults } = schema;
    const controls = new Map(); // key -> input/select element
    const values = {}; // key -> last known value (what the page believes the file holds)
    let readOnly = false;

    function render() {
        root.textContent = "";
        let currentGroup = null;
        let pendingHeader = null;
        for (const entry of entries) {
            const key = entry && entry.uniqueID;
            if (!key || EXCLUDED_KEYS.has(key) || entry.type === "button") {
                continue;
            }
            if (entry.type === "header") {
                // Groups are created lazily: a header whose entries are all
                // buttons or non-appliable keys renders nothing.
                currentGroup = null;
                pendingHeader = entry;
                continue;
            }
            if (!appliers.has(key)) {
                continue;
            }
            if (!currentGroup) {
                currentGroup = document.createElement("div");
                currentGroup.className = "settings-group";
                if (pendingHeader) {
                    const title = document.createElement("h3");
                    title.className = "settings-group-title";
                    title.textContent = pendingHeader.title;
                    currentGroup.appendChild(title);
                    pendingHeader = null;
                }
                root.appendChild(currentGroup);
            }
            currentGroup.appendChild(buildRow(entry));
        }
        applyReadOnlyState();
    }

    function buildRow(entry) {
        const key = entry.uniqueID;
        values[key] = initial[key] ?? defaults[key];

        const row = document.createElement("div");
        row.className = "settings-row";
        row.dataset.settingsKey = key;

        const info = document.createElement("span");
        info.className = "settings-row-info";
        const title = document.createElement("span");
        title.className = "settings-row-title";
        title.textContent = entry.title || key;
        info.appendChild(title);
        if (entry.description) {
            const description = document.createElement("span");
            description.className = "settings-row-desc";
            description.textContent = entry.description;
            info.appendChild(description);
        }

        row.appendChild(info);
        row.appendChild(buildControl(entry, key));
        return row;
    }

    function buildControl(entry, key) {
        const wrap = document.createElement("span");
        wrap.className = "settings-row-control";
        const current = values[key];
        let control = null;

        switch (entry.type) {
            case "checkbox": {
                const input = document.createElement("input");
                input.type = "checkbox";
                input.checked = current === true;
                control = input;
                break;
            }
            case "options": {
                const select = document.createElement("select");
                for (const option of entry.options || []) {
                    const optionEl = document.createElement("option");
                    optionEl.value = option;
                    optionEl.textContent = option;
                    if (option === current) {
                        optionEl.selected = true;
                    }
                    select.appendChild(optionEl);
                }
                control = select;
                break;
            }
            case "color": {
                const input = document.createElement("input");
                input.type = "color";
                input.value = String(current || "#000000");
                control = input;
                break;
            }
            case "number": {
                const input = document.createElement("input");
                input.type = "number";
                input.value = String(current ?? "");
                control = input;
                break;
            }
            case "commands": {
                const readout = document.createElement("span");
                readout.className = "settings-row-readonly";
                readout.textContent = `[commands] ${JSON.stringify(current ?? [])}`;
                wrap.appendChild(readout);
                return wrap;
            }
            default: {
                const input = document.createElement("input");
                input.type = "text";
                input.value = String(current ?? "");
                control = input;
                break;
            }
        }

        control.dataset.settingsKey = key;
        control.addEventListener("change", () => handleChange(entry, control));
        controls.set(key, control);
        wrap.appendChild(control);
        return wrap;
    }

    function handleChange(entry, control) {
        const key = entry.uniqueID;
        const previous = values[key];
        const next = readControlValue(entry, control);
        if (next === previous) {
            return;
        }
        values[key] = next;
        onCommit(key, next, previous);
    }

    function readControlValue(entry, control) {
        switch (entry.type) {
            case "checkbox":
                return control.checked === true;
            case "number":
                return Number.isFinite(Number(control.value)) ? Number(control.value) : control.value;
            default:
                return control.value;
        }
    }

    /** Applies pushed values (shell broadcast) to the controls, skipping focus. */
    function refreshValues(next) {
        if (!next || typeof next !== "object") {
            return;
        }
        for (const [key, control] of controls) {
            if (next[key] === undefined) {
                continue;
            }
            values[key] = next[key];
            syncControl(key, control);
        }
    }

    /** Restores one control (write-failure rollback) without firing a change. */
    function setValue(key, value) {
        values[key] = value;
        const control = controls.get(key);
        if (control) {
            syncControl(key, control);
        }
    }

    function syncControl(key, control) {
        if (document.activeElement === control) {
            return;
        }
        const value = values[key];
        if (control.tagName === "SELECT") {
            control.value = value ?? "";
        } else if (control.type === "checkbox") {
            control.checked = value === true;
        } else if (control.type === "color") {
            control.value = String(value || "#000000");
        } else {
            control.value = String(value ?? "");
        }
    }

    /** Read-only mode: tosu is online and owns the settings file. */
    function setReadOnly(next) {
        readOnly = Boolean(next);
        applyReadOnlyState();
    }

    function applyReadOnlyState() {
        for (const control of controls.values()) {
            control.disabled = readOnly;
        }
        if (root.classList) {
            root.classList.toggle("settings-form-readonly", readOnly);
        }
    }

    return {
        render,
        setReadOnly,
        refreshValues,
        setValue,
        valueOf: (key) => values[key],
        isReadOnly: () => readOnly,
    };
}
