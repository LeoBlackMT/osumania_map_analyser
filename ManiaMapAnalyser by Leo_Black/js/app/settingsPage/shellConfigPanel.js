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
export const OFFSETS_STATUS_URL = "/offsets/status";
export const OFFSETS_GENERATE_URL = "/offsets/generate";
export const OFFSETS_UPDATE_URL = "/offsets/update";
export const SHADOW_RESET_URL = "/shadow/reset";

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

    let offsetsInfo = null;
    let offsetsActionMsg = "";
    let offsetsActionKind = "info";

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

    async function fetchOffsetsStatus() {
        const result = await requestJson(OFFSETS_STATUS_URL, { cache: "no-store" });
        if (result.ok && result.body) {
            offsetsInfo = result.body;
        }
    }

    async function generateOffsets() {
        offsetsActionMsg = "Scanning running osu! and generating offsets…";
        offsetsActionKind = "saving";
        render();
        const result = await requestJson(OFFSETS_GENERATE_URL, { method: "POST" });
        if (!result.ok) {
            const detail = result.body && result.body.message ? result.body.message : (result.error ? result.error.message : `HTTP ${result.status}`);
            offsetsActionMsg = `Generation failed: ${detail}`;
            offsetsActionKind = "error";
        } else {
            const msg = result.body && result.body.message ? result.body.message : "Offsets generated and verified successfully.";
            offsetsActionMsg = msg;
            offsetsActionKind = "ok";
        }
        await fetchOffsetsStatus();
        render();
    }

    async function updateOffsets() {
        offsetsActionMsg = "Checking remote manifest and verifying signatures…";
        offsetsActionKind = "saving";
        render();
        const result = await requestJson(OFFSETS_UPDATE_URL, { method: "POST" });
        if (!result.ok) {
            const detail = result.body && result.body.message ? result.body.message : (result.error ? result.error.message : `HTTP ${result.status}`);
            offsetsActionMsg = `Update failed: ${detail}`;
            offsetsActionKind = "error";
        } else {
            const updated = result.body && typeof result.body.updated === "number" ? result.body.updated : 0;
            const msg = result.body && result.body.message ? result.body.message : (updated > 0 ? `Updated ${updated} table(s) successfully.` : "Offsets are up to date.");
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
            if (offsetsInfo) {
                section.appendChild(buildOffsetsSection());
                if (offsetsInfo.shadow) {
                    section.appendChild(buildShadowSection());
                }
            }
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

    function buildOffsetsSection() {
        const wrap = document.createElement("div");
        wrap.className = "shell-config-subsection";

        const header = document.createElement("h3");
        header.className = "settings-group-title";
        header.textContent = "Memory Offsets Management";
        wrap.appendChild(header);

        const stableInfo = offsetsInfo.stable || {};
        const stableDesc = stableInfo.loaded
            ? `Active (${stableInfo.anchors_count || 7} anchors verified, arch: ${stableInfo.arch || "x86"})`
            : "Fallback table active";
        const stableBadge = document.createElement("span");
        stableBadge.className = `shell-config-badge ${stableInfo.loaded ? "badge-ok" : "badge-warn"}`;
        stableBadge.textContent = stableInfo.loaded ? "Active" : "Fallback";
        wrap.appendChild(buildRow("offsetsStable", "osu!stable Offsets", stableDesc, stableBadge));

        const lazerInfo = offsetsInfo.lazer || {};
        const lazerDesc = lazerInfo.loaded
            ? `v${lazerInfo.version || "unknown"} (runtime: ${lazerInfo.runtime_version || "unknown"}, ${lazerInfo.types_count || 0} types)`
            : "Fallback table active";
        const lazerBadge = document.createElement("span");
        lazerBadge.className = `shell-config-badge ${lazerInfo.loaded ? "badge-ok" : "badge-warn"}`;
        lazerBadge.textContent = lazerInfo.loaded ? "Active" : "Fallback";
        wrap.appendChild(buildRow("offsetsLazer", "osu!lazer Offsets", lazerDesc, lazerBadge));

        const actionsWrap = document.createElement("div");
        actionsWrap.className = "shell-config-actions";

        const genBtn = document.createElement("button");
        genBtn.type = "button";
        genBtn.className = "settings-action-btn";
        genBtn.textContent = "1-Click Live Generator";
        genBtn.title = offsetsInfo.generator_ready
            ? "Extract offsets from running osu! with zero .NET SDK"
            : "gen.exe not detected";
        genBtn.disabled = !offsetsInfo.generator_ready;
        genBtn.addEventListener("click", () => generateOffsets());
        actionsWrap.appendChild(genBtn);

        const updateBtn = document.createElement("button");
        updateBtn.type = "button";
        updateBtn.className = "settings-action-btn";
        updateBtn.textContent = "Check Remote Updates";
        updateBtn.title = "Verify Ed25519 signed manifest and sync updated tables";
        updateBtn.addEventListener("click", () => updateOffsets());
        actionsWrap.appendChild(updateBtn);

        if (offsetsActionMsg) {
            const statusEl = document.createElement("div");
            statusEl.className = `shell-config-action-status shell-config-status-${offsetsActionKind}`;
            statusEl.textContent = offsetsActionMsg;
            actionsWrap.appendChild(statusEl);
        }

        wrap.appendChild(buildRow("offsetsActions", "Offsets Actions", "Update or generate memory layout tables for osu! clients.", actionsWrap));

        return wrap;
    }

    function buildShadowSection() {
        const wrap = document.createElement("div");
        wrap.className = "shell-config-subsection";

        const header = document.createElement("h3");
        header.className = "settings-group-title";
        header.textContent = "Shadow Diagnostics (Native vs Tosu)";
        wrap.appendChild(header);

        const shadow = offsetsInfo.shadow || {};
        const total = shadow.total_compared || 0;
        const matches = shadow.matched_frames || 0;
        const mismatches = shadow.mismatched_frames || 0;
        const rate = typeof shadow.match_rate === "number" ? shadow.match_rate.toFixed(1) : "0.0";

        const desc = total === 0
            ? (shadow.tosu_connected
                ? "tosu connected — awaiting compared frames"
                : "Waiting for concurrent tosu and native data streams…")
            : `${total} frames compared: ${matches} matched (${rate}%), ${mismatches} mismatched`;

        const badge = document.createElement("span");
        if (total === 0) {
            badge.className = "shell-config-badge badge-idle";
            badge.textContent = "Idle";
        } else if (mismatches === 0) {
            badge.className = "shell-config-badge badge-ok";
            badge.textContent = "100% Match";
        } else {
            badge.className = "shell-config-badge badge-error";
            badge.textContent = `${mismatches} Differ`;
        }

        wrap.appendChild(buildRow("shadowStatus", "Comparison Status", desc, badge));

        if (mismatches > 0 && Array.isArray(shadow.last_mismatches) && shadow.last_mismatches.length > 0) {
            const diffDesc = `Last differing fields: ${shadow.last_mismatches.join(", ")}`;
            const diffTag = document.createElement("span");
            diffTag.className = "shell-config-diff-tag";
            diffTag.textContent = shadow.last_mismatches.join(", ");
            wrap.appendChild(buildRow("shadowDiffs", "Discrepancy Details", diffDesc, diffTag));
        }

        const actionsWrap = document.createElement("div");
        actionsWrap.className = "shell-config-actions";

        const refreshBtn = document.createElement("button");
        refreshBtn.type = "button";
        refreshBtn.className = "settings-action-btn";
        refreshBtn.textContent = "Refresh Stats";
        refreshBtn.addEventListener("click", async () => {
            await fetchOffsetsStatus();
            render();
        });
        actionsWrap.appendChild(refreshBtn);

        const resetBtn = document.createElement("button");
        resetBtn.type = "button";
        resetBtn.className = "settings-action-btn";
        resetBtn.textContent = "Reset Counts";
        resetBtn.addEventListener("click", () => resetShadow());
        actionsWrap.appendChild(resetBtn);

        wrap.appendChild(buildRow("shadowActions", "Diagnostics Controls", "Manage shadow verification metrics.", actionsWrap));

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
