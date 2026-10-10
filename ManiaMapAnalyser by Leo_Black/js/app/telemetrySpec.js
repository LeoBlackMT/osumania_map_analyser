// Shared telemetry specification contract (mirrored with backend/internal/spec/spec.go).

export const ALLOWED_ALGORITHMS = Object.freeze([
    "Mixed",
    "Sunny",
    "Azusa",
    "Daniel",
    "Roxy",
    "Companella",
]);

export const ALLOWED_ACTUAL_ALGORITHMS = Object.freeze([
    "Sunny",
    "Daniel",
    "Azusa",
    "Roxy",
    "Companella",
]);

export const CAPSULE_ALIASES = Object.freeze({
    "Azusa+Companella": "Azusa",
});

// Maximum duration for a map analysis event to be considered valid (30 seconds).
// Events taking longer than this (e.g. debugger paused or sleep) must discard duration.
export const MAX_COMPUTE_DURATION_MS = 30000;

const SEMVER_PATTERN = /^v?[0-9]+\.[0-9]+(?:\.[0-9]+)?$/;

/**
 * Checks whether a given version string represents an official release.
 * Versions containing dev/test/dirty/debug/local/unknown markers return false.
 * @param {string} version
 * @returns {boolean}
 */
export function isOfficialVersion(version) {
    const text = String(version ?? "").trim();
    if (!text || text === "unknown") {
        return false;
    }
    const lower = text.toLowerCase();
    if (
        lower.includes("dev") ||
        lower.includes("test") ||
        lower.includes("dirty") ||
        lower.includes("debug") ||
        lower.includes("local")
    ) {
        return false;
    }
    return SEMVER_PATTERN.test(text);
}

/**
 * Maps composite capsule labels and verifies against the actual algorithm whitelist.
 * @param {string} value
 * @returns {string|null}
 */
export function toTelemetryActualAlgorithm(value) {
    const text = String(value ?? "").trim();
    const name = CAPSULE_ALIASES[text] ?? text;
    return ALLOWED_ACTUAL_ALGORITHMS.includes(name) ? name : null;
}

/**
 * Verifies whether the selected algorithm is valid.
 * @param {string} value
 * @returns {string|null}
 */
export function toTelemetryAlgorithm(value) {
    const text = String(value ?? "").trim();
    return ALLOWED_ALGORITHMS.includes(text) ? text : null;
}
