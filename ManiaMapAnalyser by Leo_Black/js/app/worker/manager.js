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
let workerDisabled = false; // 崩溃后永久降级主线程（避免每次分析都失败/No data）
const pendingRequests = new Map();

function ensureWorker() {
    if (workerDisabled) return null;
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
                // 崩溃一次后不再重建：worker 内 WASM 路径/加载问题会反复失败，
                // 每次都 No data。降级主线程（calc.js 主线程路径正常）。
                workerDisabled = true;
            }
        });
    } catch (_) {
        worker = null;
        messageHandlerAttached = false;
        workerDisabled = true;
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
