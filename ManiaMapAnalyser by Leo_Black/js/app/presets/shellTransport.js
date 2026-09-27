/**
 * Shell (offline desktop) preset transport — persists through the desktop
 * shell's local HTTP endpoint (127.0.0.1:24061) instead of tosu's
 * /api/counters/settings/<folder>.
 *
 * This is the framework-free half of the shell transport: it implements the
 * preset transport contract documented at the top of tosuTransport.js plus the
 * hard B1/B2 rules, without knowing anything about the shell's bridge frames.
 * The settings page (Step 9, js/app/settingsPage/shellTransport.js) composes it:
 * the bridge `settings` frame triggers a pull-on-notify refresh which calls
 * readStore() and then deliver({command: "getSettings", message: values}) so
 * core.js sees the same packet shape tosu broadcasts.
 *
 * Shell endpoint contract (desktop/src/server/http.rs):
 *   GET  /settings → 200 full settings object (the same object tosu keeps in
 *                    settings/<plugin folder>.json, or the local copy offline)
 *   POST /settings → 200 merged full settings object | 403 (tosu online) | 500
 *
 * B1 — full-object merge base: every write is {...base, ...patch} where `base`
 * is the body of the most recent SUCCESSFUL GET. With base === null the write is
 * refused outright (no request, writeLibrary() === false, onWriteError fires) so
 * the caller never advances its persist fingerprint on a write that never left.
 * The shell merges the request into its file request-key-first, which is why a
 * full object with a possibly stale base is safe.
 *
 * B2 — 2xx decides success, failures are visible, retries are bounded:
 * writeOk only ever affects the RETURN VALUE. Every call still dispatches, so
 * one failure can never short-circuit the following writes. A non-2xx or a
 * network error sets writeOk = false (the next writeLibrary() returns false so
 * core.js keeps its fingerprint and retries the persist later) and schedules ONE
 * retry 2s later that re-sends the patches accumulated meanwhile. Any 2xx —
 * including that retry — resets writeOk = true; a failed retry schedules nothing
 * more (the retry is re-armed only by a later 2xx), and failures are reported
 * through onWriteError(err) for the page's status bar.
 */

import {
    PRESET_STORAGE_SETTING,
    storeFromPayload,
} from "./storage.js";

/** Local settings endpoint of the desktop shell. */
export const SHELL_SETTINGS_URL = "/settings";

/** Delay before the single bounded write retry. */
export const SHELL_WRITE_RETRY_MS = 2000;

/**
 * Creates the shell transport.
 * @param {object} [options]
 * @param {(url: string, init?: object) => Promise<object>} [options.fetchImpl]
 *        fetch implementation (injectable for tests); defaults to global fetch.
 * @param {string} [options.settingsUrl] settings endpoint, default "/settings".
 * @param {number} [options.retryDelayMs] bounded-retry delay, default 2000.
 * @param {(error: Error) => void} [options.onWriteError] failure reporter
 *        (status bar / banner). Exceptions from it are swallowed.
 */
export function createShellTransport({
    fetchImpl = (url, init) => fetch(url, init),
    settingsUrl = SHELL_SETTINGS_URL,
    retryDelayMs = SHELL_WRITE_RETRY_MS,
    onWriteError = null,
} = {}) {
    const subscribers = new Set();

    // B1: merge base = body of the most recent successful GET (null until then).
    let base = null;
    // B2: return-value only — never a send gate. Starts true so the first
    // accepted write reports "dispatched", matching the tosu transport.
    let writeOk = true;
    // Patches not yet confirmed by a 2xx. Every request body carries all of
    // them merged (later values win) and a 2xx confirms exactly the patches its
    // own body carried, so concurrent distinct patches cannot lose each other.
    let unconfirmed = [];
    let retryTimer = null;
    // True once a retry has been scheduled for the current failure episode:
    // "a failed retry must not schedule another" is enforced with it.
    let retryExhausted = false;

    function isAvailable() {
        return base !== null;
    }

    /** Merges every unconfirmed patch (later patches win per key). */
    function mergeUnconfirmed() {
        const merged = {};
        for (const patch of unconfirmed) {
            Object.assign(merged, patch);
        }
        return merged;
    }

    /** Converts the core write-back value list into a patch object. */
    function patchFromValues(values) {
        const patch = {};
        for (const entry of values || []) {
            if (entry && typeof entry.uniqueID === "string") {
                patch[entry.uniqueID] = entry.value;
            }
        }
        return patch;
    }

    function clearRetryTimer() {
        if (retryTimer !== null) {
            clearTimeout(retryTimer);
            retryTimer = null;
        }
    }

    function reportWriteError(error) {
        writeOk = false;
        try {
            if (typeof onWriteError === "function") {
                onWriteError(error);
            }
        } catch {
            // A broken reporter must not break the transport.
        }
        scheduleRetry();
    }

    function scheduleRetry() {
        if (retryTimer !== null || retryExhausted) {
            return;
        }
        retryExhausted = true;
        retryTimer = setTimeout(() => {
            retryTimer = null;
            if (unconfirmed.length === 0) {
                // Everything was confirmed by another successful write — the
                // retry has nothing left to do.
                return;
            }
            // Re-send every still-unconfirmed patch (including the ones added
            // while this retry was waiting) on the same base.
            dispatch({});
        }, retryDelayMs);
    }

    /**
     * POSTs {...base, ...allUnconfirmedPatches}. Always called for every write —
     * the writeOk flag never suppresses a request (B2).
     */
    function dispatch(patch) {
        unconfirmed.push(patch);
        const body = { ...base, ...mergeUnconfirmed() };
        const carriedByThisRequest = unconfirmed.slice();
        Promise.resolve()
            .then(() => fetchImpl(settingsUrl, {
                method: "POST",
                headers: { "Content-Type": "application/json" },
                body: JSON.stringify(body),
            }))
            .then((response) => {
                if (response && response.ok) {
                    // Any 2xx confirms every patch this request's body carried.
                    unconfirmed = unconfirmed.filter((entry) => !carriedByThisRequest.includes(entry));
                    writeOk = true;
                    if (unconfirmed.length === 0) {
                        // Everything is confirmed — nothing left to retry. Patches
                        // that are still unconfirmed keep the pending retry alive
                        // (and stay in the list, so any later write carries them).
                        retryExhausted = false;
                        clearRetryTimer();
                    }
                    return;
                }
                const status = response && typeof response.status === "number" ? response.status : "?";
                reportWriteError(new Error(`shell settings write failed: HTTP ${status}`));
            })
            .catch((error) => {
                reportWriteError(error instanceof Error ? error : new Error(String(error)));
            });
    }

    /** Shared entry point of every write path (B1 + B2). */
    function write(patch) {
        if (base === null) {
            // B1: never GET successfully → refuse: no request, false, reporter fires.
            try {
                if (typeof onWriteError === "function") {
                    onWriteError(new Error("shell settings base unavailable - write refused"));
                }
            } catch {
                // A broken reporter must not break the transport.
            }
            return false;
        }
        dispatch(patch);
        return writeOk;
    }

    /**
     * GETs the full settings object and adopts it as the merge base (B1).
     * @returns {Promise<{store: {presets: Array, lastWritten: Array}, raw: string|null}|null>}
     */
    async function readStore() {
        try {
            const response = await fetchImpl(settingsUrl, { cache: "no-store" });
            if (!response || !response.ok) {
                return null;
            }
            const values = await response.json();
            if (!values || typeof values !== "object") {
                return null;
            }
            base = values;
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
     * GETs the full settings object, adopts it as the merge base (B1) and hands
     * the values back. readStore() drops them on purpose (it answers the
     * preset-library contract only); the settings page needs the object itself to
     * compare the payload before delivering it to its subscribers and to
     * re-render its form — and it must not GET twice, because the merge base has
     * to be exactly the object the page acted on (a stale base would let the next
     * full-object write revert keys changed in between).
     * @returns {Promise<object|null>} null when the GET failed (base untouched).
     */
    async function readValues() {
        const result = await readStore();
        if (result === null || base === null) {
            return null;
        }
        return { ...base };
    }

    function writeLibrary(serialized) {
        return write({ [PRESET_STORAGE_SETTING]: serialized });
    }

    function writeBack(values) {
        write(patchFromValues(values));
    }

    function subscribe(handler) {
        subscribers.add(handler);
        return () => subscribers.delete(handler);
    }

    /** Fans a getSettings-shaped packet out to every subscriber (Step 9 glue). */
    function deliver(packet) {
        for (const handler of [...subscribers]) {
            handler(packet);
        }
    }

    /**
     * One GET; on success the full values object is delivered to subscribers in
     * the same packet shape tosu uses ("getSettings"), so core.js runs its
     * regular first-batch path. The desktop online/offline decision stays with
     * the settings page, which can override this member.
     */
    function requestInitial() {
        readStore().then((result) => {
            if (result === null || base === null) {
                return;
            }
            deliver({ command: "getSettings", message: { ...base } });
        });
    }

    return {
        mode: "shell",
        isAvailable,
        readStore,
        writeLibrary,
        writeBack,
        subscribe,
        requestInitial,
        // Extra (not part of the core preset contract): readStore() with the
        // values kept, for the settings page's pull-on-notify refresh.
        readValues,
        // Extra (not part of the core contract): used by the settings-page glue
        // for pull-on-notify delivery.
        deliver,
    };
}
