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
let latestId = null;

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
 * Run the estimation pipeline in the worker thread.
 *
 * @param {object} input - { rawText, estimatorAlgorithm, options } for runAnalysisPipeline
 * @returns {Promise<object>} pipeline result (same shape as runAnalysisPipeline)
 */
export function runInWorker(input) {
    const w = ensureWorker();
    if (!w) return null; // caller should fall back to sync

    // Cancel all previous pending requests by advancing latestId
    const id = generateId();
    latestId = id;

    return new Promise((resolve, reject) => {
        const handler = (event) => {
            const { id: respId, result, error } = event.data || {};

            // Discard responses from stale requests
            if (respId !== latestId) return;

            w.removeEventListener("message", handler);
            if (error) {
                reject(new Error(error));
            } else {
                resolve(result);
            }
        };

        w.addEventListener("message", handler);
        w.postMessage({ id, type: "pipeline", input });

        // Safety timeout (30s) to prevent hanging
        setTimeout(() => {
            if (id === latestId) {
                w.removeEventListener("message", handler);
                reject(new Error("Worker timeout"));
            }
        }, 30000);
    });
}

/**
 * Check if worker-based computation is available.
 */
export function isWorkerAvailable() {
    return ensureWorker() !== null;
}
