/**
 * Settings-page shell transport — a THIN ADAPTER over the framework-free
 * transport in ../presets/shellTransport.js (B1/B2 live there, not here).
 *
 * It adds exactly the three page-specific parts:
 *   1. the bridge `settings` frame handler: pull-on-notify. The shell pushes
 *      both shell-config and plugin settings under the same `settings` frame
 *      type (assumption 9), so the frame payload must NEVER be merged into the
 *      write base — the page re-GETs /settings and delivers to its subscribers
 *      only when the payload actually changed;
 *   2. the tosu-online gate: while `state.shellTosuOnline` is true tosu owns the
 *      settings file, so the refresh only re-bases the next write and never
 *      delivers (no read-only page ever re-applies tosu values into state);
 *   3. requestInitial(): one GET + delivery, which is how core.js receives its
 *      first settings packet on this page.
 *
 * `patch({key: value})` goes through the composed transport's writeBack(), i.e.
 * the very same write() path as writeLibrary (B1 full-object base merge, B2
 * bounded retry + onWriteError reporting) — no second implementation.
 */

import { createShellTransport } from "../presets/shellTransport.js";
import { state } from "../appContext.js";

/** Default gate: tosu online means the page is read-only (shell owns the file). */
function defaultIsReadOnly() {
    return state.shellTosuOnline === true;
}

/**
 * @param {object} [options]
 * @param {(url: string, init?: object) => Promise<object>} [options.fetchImpl]
 *        fetch implementation, forwarded to the composed transport.
 * @param {(error: Error) => void} [options.onWriteError] write-failure reporter
 *        (status bar), forwarded to the composed transport.
 * @param {() => boolean} [options.isReadOnly] read-only gate, default
 *        `() => state.shellTosuOnline === true`.
 */
export function createShellPresetTransport({
    fetchImpl,
    onWriteError = null,
    isReadOnly = defaultIsReadOnly,
} = {}) {
    const store = createShellTransport({ fetchImpl, onWriteError });

    // JSON fingerprint of the last payload handed to the subscribers: the shell
    // broadcasts on every write and on source transitions, and re-delivering an
    // unchanged payload would just re-run the appliers for nothing.
    let lastDelivered = null;
    // Refreshes are serialized so two concurrent notifies cannot interleave two
    // GETs and deliver the older response last.
    let refreshChain = Promise.resolve();

    /** Fan-out through the composed transport (one subscriber set for every user). */
    function subscribe(handler) {
        return store.subscribe(handler);
    }

    function deliver(values) {
        try {
            store.deliver({ command: "getSettings", message: values });
        } catch (error) {
            // A broken subscriber must not break the pull chain of the others.
            console.error("[settings] delivering pushed settings failed:", error);
        }
    }

    async function doRefresh() {
        const values = await store.readValues();
        if (values === null) {
            return null;
        }
        if (isReadOnly()) {
            // Online: refresh the merge base (so a later offline write starts
            // from the file as it is now) but never deliver.
            return values;
        }
        let fingerprint;
        try {
            fingerprint = JSON.stringify(values);
        } catch {
            fingerprint = null;
        }
        if (fingerprint !== null && fingerprint === lastDelivered) {
            return values;
        }
        lastDelivered = fingerprint;
        deliver(values);
        return values;
    }

    /** GET /settings (+ deliver when offline and changed). Used by pull-on-notify. */
    function refresh() {
        refreshChain = refreshChain.then(doRefresh, doRefresh);
        return refreshChain;
    }

    /** Bridge `settings` frame → pull-on-notify (the payload itself is ignored). */
    function onShellFrame() {
        return refresh();
    }

    /** First settings packet of the page: one GET + delivery (offline only). */
    function requestInitial() {
        return refresh();
    }

    /**
     * Submits a single-key (or few-key) patch through the shared base-merge path.
     * The synchronous return value of the composed writeBack is void by contract;
     * failures arrive through onWriteError (including the "no base - refused"
     * case), which is what the page's status bar reports.
     */
    function patch(patchObject) {
        const values = Object.entries(patchObject || {}).map(([uniqueID, value]) => ({ uniqueID, value }));
        store.writeBack(values);
    }

    return {
        mode: "shell",
        isAvailable: () => store.isAvailable(),
        readStore: () => store.readStore(),
        readValues: () => store.readValues(),
        writeLibrary: (serialized) => store.writeLibrary(serialized),
        writeBack: (values) => store.writeBack(values),
        patch,
        subscribe,
        requestInitial,
        onShellFrame,
        refresh,
        isReadOnly: () => isReadOnly(),
    };
}
