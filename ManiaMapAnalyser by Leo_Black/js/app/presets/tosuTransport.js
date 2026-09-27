/**
 * tosu preset storage transport — the default preset transport.
 *
 * The three tosu I/O paths that used to live in core.js are moved here
 * VERBATIM (same literals, same best-effort error handling) and wrapped in a
 * factory so the preset logic no longer talks to `socket` / `window.COUNTER_PATH`
 * directly. The offline desktop shell injects its own implementation through
 * setPresetTransport() (see shellTransport.js in this folder for the generic
 * shell transport and js/app/settingsPage/shellTransport.js for the page glue).
 *
 * ---------------------------------------------------------------------------
 * Preset transport contract (exact — core.js / manager.js code against this)
 * ---------------------------------------------------------------------------
 *   {
 *     mode: "tosu" | "shell",
 *     isAvailable(): boolean,
 *     readStore(): Promise<{store: {presets: Array, lastWritten: Array}, raw: string|null}|null>,
 *     writeLibrary(serialized: string): boolean,   // SYNC: true = accepted & request dispatched
 *     writeBack(values: Array<{uniqueID, value}>): void,
 *     subscribe(handler): () => void,              // multi-subscriber, returns unsubscribe
 *     requestInitial(): void,                      // ask the source for an initial payload
 *   }
 *
 * `mode` is chosen by the transport itself (it is a property of the created
 * object, not a parameter): "tosu" for this factory, "shell" for the desktop
 * shell transport. core.js reads `getPresetTransport().mode` to decide whether
 * the tosu dashboard picker/auto-save half of handleSettingsPacket applies.
 *
 * Semantics:
 * - writeLibrary() is synchronous and answers "was this accepted and dispatched"
 *   (today's meaning), NOT "did the server accept it". An async failure must not
 *   be reported by throwing; the shell transport reports it through onWriteError
 *   (see the B2 contract below) and core.js simply does not advance its persist
 *   fingerprint when the call returns false.
 * - subscribe()/requestInitial() are the ONLY ways for callers to reach the
 *   transport's event source; no caller may touch `socket` directly.
 *
 * ---------------------------------------------------------------------------
 * Shell-mode contract (implemented by js/app/presets/shellTransport.js)
 * ---------------------------------------------------------------------------
 * B1 — full-object merge base (hard):
 *   EVERY shell write path (writeLibrary / writeBack / any extra patch helper)
 *   merges onto "the `base` of the most recent successful GET": the request body
 *   is {...base, ...patchObject}, a FULL settings object (the shell merges it
 *   request-key-first into its local file). When `base` is null (no successful
 *   GET yet) the transport MUST REFUSE to write: no request is sent,
 *   writeLibrary() returns false, the caller's fingerprint must not advance, and
 *   the failure callback fires.
 * B2 — success is 2xx, failures are visible, one bounded retry (hard):
 *   writeOk only affects the RETURN VALUE, never whether a request is sent:
 *   every writeLibrary()/writeBack() call still dispatches, so a single failure
 *   can never short-circuit later writes (that would be the silent lost write
 *   this design forbids). On a non-2xx response or a network error the transport
 *   sets writeOk = false so the NEXT writeLibrary() returns false (core.js then
 *   leaves its fingerprint untouched and retries the persist later), schedules
 *   ONE bounded retry after 2s that merges the patches accumulated meanwhile,
 *   and reports the failure through onWriteError(err). ANY 2xx — including that
 *   retry — resets writeOk = true; a failed retry must not schedule another.
 */

import { socket } from "../appContext.js";
import { getCounterPathForCommand } from "../settings.js";
import {
    PRESET_STORAGE_SETTING,
    storeFromPayload,
} from "./storage.js";

/** True when the page runs in a regular browser (localhost / 127.0.0.1). */
function isBrowserOrigin() {
    const host = window.location.hostname;
    return host === "127.0.0.1" || host === "localhost";
}

/** tosu's plugin folder name, injected by tosu into window.COUNTER_PATH. */
function counterFolderName() {
    return typeof window.COUNTER_PATH === "string" ? window.COUNTER_PATH.trim() : "";
}

/**
 * Creates the tosu transport (browser page talking to tosu directly).
 * Side-effect free: the tosu command socket is opened lazily on the first
 * subscribe(), so importing/creating a transport never touches the network.
 */
export function createTosuTransport() {
    const subscribers = new Set();
    let streamBound = false;

    /**
     * Opens tosu's /websocket/commands stream ONCE and fans every packet out to
     * all current subscribers (the pre-split code opened this socket directly in
     * initPresets). Late subscribers share the same connection; unsubscribing
     * only drops that handler — the socket stays open for the page lifetime,
     * exactly as before.
     */
    function bindStreamOnce() {
        if (streamBound) {
            return;
        }
        streamBound = true;
        socket.commands((packet) => {
            // Iterate a copy: a handler may unsubscribe while the packet is
            // being dispatched, which must not skip the remaining handlers.
            for (const handler of [...subscribers]) {
                handler(packet);
            }
        });
    }

    function isAvailable() {
        return isBrowserOrigin() && counterFolderName() !== "";
    }

    /**
     * Writes the library into the presetStorage tosu setting.
     * Returns true when the POST was actually issued (today's semantics: true =
     * accepted & dispatched, not "the server stored it").
     * @param {string} serialized serialized store (see storage.serializeStore)
     */
    function writeLibrary(serialized) {
        // Write-back happens only from a browser page (the manager page or the
        // overlay in a browser tab): localhost and 127.0.0.1 are both fine.
        // The in-game CEF overlay never opens presets.html, so it stays read-only.
        if (!isAvailable()) {
            return false;
        }
        const folderName = counterFolderName();
        fetch(`/api/counters/settings/${encodeURIComponent(folderName)}`, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify([{
                uniqueID: PRESET_STORAGE_SETTING,
                value: serialized,
            }]),
        }).catch(() => {
            // Best-effort sync; the library stays in memory and re-syncs on next
            // successful write.
        });
        return true;
    }

    /**
     * Pulls the preset store straight from tosu's values file
     * (GET /api/counters/settings/<folder>). Origin-independent: localhost and
     * 127.0.0.1 read the same data here.
     * @returns {Promise<{store: {presets: Array, lastWritten: Array}, raw: string|null}|null>}
     */
    async function readStore() {
        if (!isAvailable()) {
            return null;
        }
        const folderName = counterFolderName();
        try {
            const response = await fetch(
                `/api/counters/settings/${encodeURIComponent(folderName)}`,
                { cache: "no-store" },
            );
            if (!response.ok) {
                return null;
            }
            const data = await response.json();
            const values = (data && data.values) || {};
            return {
                store: storeFromPayload(values),
                raw: typeof values[PRESET_STORAGE_SETTING] === "string"
                    ? values[PRESET_STORAGE_SETTING]
                    : null,
            };
        } catch {
            return null;
        }
    }

    /**
     * Posts a write-back (preset apply echo) to tosu. Best-effort: an
     * unavailable origin/COUNTER_PATH is a silent no-op, exactly as before.
     * @param {Array<{uniqueID: string, value: unknown}>} values
     */
    function writeBack(values) {
        if (!isAvailable()) {
            return;
        }
        const folderName = counterFolderName();
        fetch(`/api/counters/settings/${encodeURIComponent(folderName)}`, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify(values),
        }).catch(() => {
            // Write-back is a best-effort sync; preset application still worked.
        });
    }

    function subscribe(handler) {
        subscribers.add(handler);
        bindStreamOnce();
        return () => subscribers.delete(handler);
    }

    /**
     * Requests the initial settings payload on the tosu command stream. The
     * payload arrives through subscribe() like every other packet; this is
     * idempotent (duplicates are harmless) so the 3s fallback may call it again.
     */
    function requestInitial() {
        // The manager page does not go through loadSettings(), so request the
        // settings stream explicitly.
        if (typeof socket.sendCommand === "function") {
            socket.sendCommand("getSettings", getCounterPathForCommand());
        }
    }

    return {
        mode: "tosu",
        isAvailable,
        readStore,
        writeLibrary,
        writeBack,
        subscribe,
        requestInitial,
    };
}
