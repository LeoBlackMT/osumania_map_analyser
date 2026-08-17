// Anonymous usage statistics (telemetry).
//
// Opt-out (controlled by the enableTelemetry setting), fully anonymous, and
// fail-silent: every network call is fire-and-forget and never affects the
// plugin. Only aggregate, non-identifying fields are sent — no username, no
// beatmap identity, no IP (the server never stores IP either). The endpoint is
// hardcoded in index.js (window.__MMA_TELEMETRY_ENDPOINT).
import { state } from "./appContext.js";

const INSTALL_ID_KEY = "mma.telemetry.installId.v1";
const HEARTBEAT_INTERVAL_MS = 10 * 60 * 1000;
const ACTIVITY_WINDOW_MS = 30 * 1000;
const REQUEST_TIMEOUT_MS = 5000;

let enabled = false;
let endpoint = "";
let version = "";
let installId = null;
let lastActivityAt = 0;
let booted = false;
let heartbeatTimerId = 0;

function readStorage(key) {
    try {
        const value = window.localStorage.getItem(key);
        if (value != null) {
            return value;
        }
    } catch {
        // Ignore localStorage failures.
    }
    try {
        return window.sessionStorage.getItem(key);
    } catch {
        return null;
    }
}

function writeStorage(key, value) {
    try {
        window.localStorage.setItem(key, value);
        return;
    } catch {
        // Ignore localStorage failures.
    }
    try {
        window.sessionStorage.setItem(key, value);
    } catch {
        // Ignore storage failures and keep runtime working.
    }
}

function randomId() {
    if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
        try {
            return crypto.randomUUID();
        } catch {
            // Fall through to the manual generator below.
        }
    }

    const bytes = new Uint8Array(16);
    if (typeof crypto !== "undefined" && typeof crypto.getRandomValues === "function") {
        crypto.getRandomValues(bytes);
    } else {
        for (let i = 0; i < bytes.length; i += 1) {
            bytes[i] = Math.floor(Math.random() * 256);
        }
    }

    const hex = Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
    return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

function normalizeEndpoint(value) {
    return String(value ?? "").trim().replace(/\/+$/, "");
}

function getInstallId() {
    if (installId) {
        return installId;
    }

    const stored = readStorage(INSTALL_ID_KEY);
    if (typeof stored === "string" && stored.length > 0) {
        installId = stored;
        return installId;
    }

    installId = randomId();
    writeStorage(INSTALL_ID_KEY, installId);
    return installId;
}

function isActive() {
    return enabled && endpoint.length > 0;
}

function readConfig() {
    enabled = Boolean(state.enableTelemetry);
    endpoint = normalizeEndpoint(
        typeof window.__MMA_TELEMETRY_ENDPOINT === "string" ? window.__MMA_TELEMETRY_ENDPOINT : "",
    );
    version = typeof window.__MMA_VERSION === "string" ? window.__MMA_VERSION : "";
}

function send(kind, data) {
    if (!isActive()) {
        return;
    }

    const payload = {
        id: getInstallId(),
        kind,
        version,
        data: data || {},
    };

    try {
        let controller = null;
        let timeoutId = 0;
        if (typeof AbortController !== "undefined") {
            controller = new AbortController();
            timeoutId = setTimeout(() => controller.abort(), REQUEST_TIMEOUT_MS);
        }

        const clearTimer = () => {
            if (timeoutId) {
                clearTimeout(timeoutId);
            }
        };

        fetch(`${endpoint}/api/v1/event`, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify(payload),
            keepalive: true,
            signal: controller ? controller.signal : undefined,
        }).then(clearTimer, clearTimer);
    } catch {
        // Telemetry must never break the plugin.
    }
}

function maybeBoot() {
    if (isActive() && !booted) {
        booted = true;
        send("boot");
    }
}

export function initTelemetry() {
    readConfig();
    maybeBoot();
}

// Called by settings.js whenever telemetry settings change at runtime.
export function setTelemetryConfig() {
    readConfig();
    syncTelemetryHeartbeat();
    maybeBoot();
}

// Called on every api_v2 packet so the heartbeat only fires while a game is
// connected (menu counts as connected, see socketHandlers.js).
export function noteTelemetryActivity() {
    lastActivityAt = Date.now();
}

export function stopTelemetryHeartbeat() {
    if (heartbeatTimerId) {
        clearInterval(heartbeatTimerId);
        heartbeatTimerId = 0;
    }
}

function syncTelemetryHeartbeat() {
    if (isActive()) {
        if (heartbeatTimerId) {
            return;
        }

        heartbeatTimerId = window.setInterval(() => {
            if (!isActive()) {
                return;
            }
            if (Date.now() - lastActivityAt > ACTIVITY_WINDOW_MS) {
                return;
            }
            send("heartbeat");
        }, HEARTBEAT_INTERVAL_MS);
    } else {
        stopTelemetryHeartbeat();
    }
}

export function startTelemetryHeartbeat() {
    syncTelemetryHeartbeat();
}

export function trackTelemetryAnalyze(data) {
    send("analyze", data || {});
}
