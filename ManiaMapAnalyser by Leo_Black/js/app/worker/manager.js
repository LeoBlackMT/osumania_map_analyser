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
let messageHandlerAttached = false;
const pendingRequests = new Map();

function ensureWorker() {
    if (worker) return worker;
    try {
        const w = new Worker(
            new URL("./compute.worker.js", import.meta.url),
            { type: "module" },
        );
        worker = w;
        messageHandlerAttached = false;

        w.addEventListener("error", () => {
            rejectAllPending(new Error("Worker crashed"));
            if (worker === w) {
                try {
                    w.terminate();
                } catch {
                    // Ignore terminate errors during cleanup.
                }
                worker = null;
                messageHandlerAttached = false;
            }
        });
    } catch (_) {
        worker = null;
        messageHandlerAttached = false;
    }
    return worker;
}

function generateId() {
    nextId += 1;
    return `req-${nextId}-${Date.now()}`;
}

function rejectAllPending(reason) {
    for (const entry of pendingRequests.values()) {
        clearTimeout(entry.timeoutId);
        entry.reject(reason);
    }
    pendingRequests.clear();
}

function attachMessageHandler(w) {
    if (messageHandlerAttached) return;
    messageHandlerAttached = true;

    w.addEventListener("message", (event) => {
        const { id, result, error } = event.data || {};
        const entry = pendingRequests.get(id);
        if (!entry) return;

        pendingRequests.delete(id);
        clearTimeout(entry.timeoutId);
        if (error) {
            entry.reject(new Error(error));
        } else {
            entry.resolve(result);
        }
    });
}

/**
 * Run the estimation pipeline in the worker thread.
 *
 * @param {object} input - { rawText, estimatorAlgorithm, options } for runAnalysisPipeline
 * @returns {Promise<object>} pipeline result (same shape as runAnalysisPipeline)
 */
export function runInWorker(input) {
    const w = ensureWorker();
    if (!w) return null; // caller should fall back to sync

    // A new request supersedes every pending one: settle their promises and
    // drop their listeners so stale work cannot accumulate in memory.
    rejectAllPending(new Error("Worker request superseded"));

    const id = generateId();
    attachMessageHandler(w);

    return new Promise((resolve, reject) => {
        const timeoutId = setTimeout(() => {
            const entry = pendingRequests.get(id);
            if (!entry) return;
            pendingRequests.delete(id);
            reject(new Error("Worker timeout"));
        }, 30000);

        pendingRequests.set(id, { resolve, reject, timeoutId });

        try {
            w.postMessage({ id, type: "pipeline", input });
        } catch (error) {
            const entry = pendingRequests.get(id);
            if (entry) {
                pendingRequests.delete(id);
                clearTimeout(entry.timeoutId);
            }
            if (worker === w) {
                try {
                    w.terminate();
                } catch {
                    // Ignore terminate errors during cleanup.
                }
                worker = null;
                messageHandlerAttached = false;
            }
            reject(error);
        }
    });
}

/**
 * Check if worker-based computation is available.
 */
export function isWorkerAvailable() {
    return ensureWorker() !== null;
}
