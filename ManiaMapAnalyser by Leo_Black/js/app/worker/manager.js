/**
 * Worker Manager — centralized lifecycle for the compute worker.
 *
 * Guarantees:
 *  - Only the LATEST request's result reaches the caller.
 *  - Stale/in-flight requests are cancelled when superseded.
 *  - Crashed workers are recreated on next request.
 *  - Fallback to main-thread if workers are unsupported.
 */

let worker = null;
let nextId = 0;
let activeRequest = null;

function makeCancellationError() {
    const error = new Error("Worker request superseded");
    error.name = "AbortError";
    return error;
}

function terminateWorker(target = worker) {
    if (!target) return;
    try {
        target.terminate();
    } catch {
        // The worker may already have terminated after an error.
    }
    if (worker === target) worker = null;
}

function ensureWorker() {
    if (worker) return worker;
    try {
        const w = new Worker(
            new URL("./compute.worker.js", import.meta.url),
            { type: "module" },
        );
        worker = w;
    } catch (_) {
        worker = null;
    }
    return worker;
}

function generateId() {
    nextId += 1;
    return `req-${nextId}-${Date.now()}`;
}

/**
 * Run an estimator in the worker thread.
 *
 * @param {string} osuText - beatmap .osu file content
 * @param {object} options - { speedRate, estimatorAlgorithm, ... }
 * @returns {Promise<object>} estimator result (same shape as sync functions)
 */
export function runInWorker(osuText, options) {
    if (activeRequest) {
        activeRequest.cancel();
        terminateWorker();
    }

    const w = ensureWorker();
    if (!w) return null; // caller should fall back to sync

    const id = generateId();

    return new Promise((resolve, reject) => {
        let settled = false;
        let timeoutId = null;

        const cleanup = () => {
            w.removeEventListener("message", onMessage);
            w.removeEventListener("error", onError);
            if (timeoutId != null) clearTimeout(timeoutId);
            if (activeRequest?.id === id) activeRequest = null;
        };

        const finish = (callback, value) => {
            if (settled) return;
            settled = true;
            cleanup();
            callback(value);
        };

        const onMessage = (event) => {
            const { id: respId, result, error } = event.data || {};
            if (respId !== id) return;
            if (error) {
                finish(reject, new Error(error));
            } else {
                finish(resolve, result);
            }
        };

        const onError = (event) => {
            const message = event?.message || "Worker crashed";
            finish(reject, new Error(message));
            terminateWorker(w);
        };

        activeRequest = {
            id,
            cancel: () => finish(reject, makeCancellationError()),
        };

        w.addEventListener("message", onMessage);
        w.addEventListener("error", onError);

        timeoutId = setTimeout(() => {
            finish(reject, new Error("Worker timeout"));
            terminateWorker(w);
        }, 30000);

        try {
            w.postMessage({ id, osuText, options });
        } catch (error) {
            finish(reject, error instanceof Error ? error : new Error(String(error)));
            terminateWorker(w);
        }
    });
}

/**
 * Check if worker-based computation is available.
 */
export function isWorkerAvailable() {
    return ensureWorker() !== null;
}
