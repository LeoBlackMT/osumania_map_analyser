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

/** Local shell-config endpoint (desktop/src/server/http.rs). */
export const SHELL_CONFIG_URL = "/shell-config";

/** `gameClient` values the plugin's source router understands (Auto = decide). */
const GAME_CLIENT_OPTIONS = ["Auto", "osu!", "Etterna", "Malody", "Malody 4"];

/** Matches config::log_level()'s accepted values. */
const LOG_LEVEL_OPTIONS = ["debug", "info", "warn", "error", "off"];

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

const HOTKEY_HINT = "Restart the shell to apply hotkey changes.";
const UNREADABLE_NOTICE = "Shell config unreadable (mma-shell-config.json) — editing is unavailable here.";

/**
 * @param {object} options
 * @param {HTMLElement} options.root container element (#shell-config-root).
 * @param {(url: string, init?: object) => Promise<object>} [options.fetchImpl]
 *        fetch implementation (defaults to the global fetch).
 */
export function createShellConfigPanel({ root, fetchImpl = (url, init) => fetch(url, init) }) {
    let config = null;
    let resolved = {};
    let statusText = "";
    let statusKind = "info";
    let saveHint = "";

    async function requestJson(url, init) {
        try {
            const response = await fetchImpl(url, init);
            const status = response && typeof response.status === "number" ? response.status : 0;
            if (!response || !response.ok) {
                return { ok: false, status };
            }
            let body = null;
            try {
                body = await response.json();
            } catch {
                body = null;
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

    /** GET /shell-config → config + resolved; a 400 renders a read-only notice. */
    async function load() {
        const result = await requestJson(SHELL_CONFIG_URL, { cache: "no-store" });
        if (!result.ok || !result.body || !result.body.config || typeof result.body.config !== "object") {
            config = null;
            resolved = {};
            saveHint = "";
            setStatus(
                result.status === 400
                    ? UNREADABLE_NOTICE
                    : `Cannot read the shell config (HTTP ${result.status || "?"}).`,
                "error",
            );
            render();
            return false;
        }
        config = result.body.config;
        resolved = result.body.resolved && typeof result.body.resolved === "object" ? result.body.resolved : {};
        setStatus("", "info");
        render();
        return true;
    }

    /** POST a patch; re-render from the response (400 = nothing was written). */
    async function write(patch, { refreshResolvedAfter = false } = {}) {
        setStatus("Saving…", "saving");
        saveHint = "";
        render();
        const result = await requestJson(SHELL_CONFIG_URL, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify(patch),
        });
        if (!result.ok) {
            if (result.status === 400) {
                setStatus(`${UNREADABLE_NOTICE} (nothing was written)`, "error");
            } else {
                const detail = result.error && result.error.message ? result.error.message : `HTTP ${result.status || "?"}`;
                setStatus(`Save failed: ${detail}`, "error");
            }
            render();
            return false;
        }
        if (result.body && typeof result.body === "object") {
            config = result.body;
        }
        setStatus("Saved.", "ok");
        saveHint = refreshResolvedAfter ? "Refreshing the adopted paths…" : "";
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
        title.textContent = "Shell Configuration";
        section.appendChild(title);

        const sub = document.createElement("p");
        sub.className = "settings-panel-sub";
        sub.textContent = "Only the desktop shell reads these values (mma-shell-config.json, next to mma-shell.exe).";
        section.appendChild(sub);

        const status = document.createElement("p");
        status.className = `shell-config-status shell-config-status-${statusKind}`;
        status.textContent = statusText || saveHint;
        section.appendChild(status);

        if (config) {
            section.appendChild(buildGameClientRow());
            for (const field of ROOT_FIELDS) {
                section.appendChild(buildRootRow(field));
            }
            section.appendChild(buildHotkeysRow());
            section.appendChild(buildLogLevelRow());
        }
        root.appendChild(section);
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

    function buildGameClientRow() {
        const select = document.createElement("select");
        select.dataset.shellKey = "gameClient";
        const current = String(config.gameClient || "Auto");
        for (const option of GAME_CLIENT_OPTIONS) {
            const optionEl = document.createElement("option");
            optionEl.value = option;
            optionEl.textContent = option;
            optionEl.selected = option === current;
            select.appendChild(optionEl);
        }
        select.value = current;
        select.addEventListener("change", () => {
            write({ gameClient: select.value });
        });
        return buildRow(
            "gameClient",
            "Game client",
            "Auto picks the active source; a fixed value forces that source.",
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
        readout.textContent = `adopted: ${adopted || "not detected"}`;
        wrap.appendChild(readout);

        return buildRow(field.key, field.title, field.description, wrap);
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
            label.textContent = field.title;
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
        hint.textContent = HOTKEY_HINT;
        wrap.appendChild(hint);

        return buildRow("hotkeys", "Hotkeys", "Shell-wide shortcuts.", wrap);
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
        return buildRow("logLevel", "Log level", "Takes effect immediately.", select);
    }

    return {
        load,
        refresh: load,
        refreshResolved,
        getConfig: () => config,
        getResolved: () => resolved,
    };
}
