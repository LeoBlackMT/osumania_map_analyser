import createMinaCalc740 from "./minaclac-74.0.js";
import createMinaCalc680Unofficial from "./minaclac-68.0-unofficial.js";
import createMinaCalc700 from "./minaclac-70.0.js";
import createMinaCalc720 from "./minaclac-72.0.js";
import createMinaCalc723 from "./minaclac-72.3.js";

const ETTERNA_VERSION_REGISTRY = Object.freeze({
    "0.68.0-Unofficial": {
        loader: createMinaCalc680Unofficial,
        reason: null,
    },
    "0.70.0": {
        loader: createMinaCalc700,
        reason: null,
    },
    "0.72.0": {
        loader: createMinaCalc720,
        reason: null,
    },
    "0.72.3": {
        loader: createMinaCalc723,
        reason: null,
    },
    "0.74.0": {
        loader: createMinaCalc740,
        reason: null,
    },
});

export const DEFAULT_ETTERNA_VERSION = "0.72.3";

function resolveAvailableFallbackVersion(preferredVersion) {
    const preferredEntry = ETTERNA_VERSION_REGISTRY[preferredVersion];
    if (preferredEntry && typeof preferredEntry.loader === "function") {
        return preferredVersion;
    }

    for (const [version, entry] of Object.entries(ETTERNA_VERSION_REGISTRY)) {
        if (typeof entry.loader === "function") {
            return version;
        }
    }

    return null;
}

export function listEtternaVersions() {
    return Object.keys(ETTERNA_VERSION_REGISTRY);
}

export function normalizeEtternaVersion(value) {
    const trimmed = typeof value === "string" ? value.trim() : "";
    const normalized = trimmed === "0.68.0" ? "0.68.0-Unofficial" : trimmed;
    if (normalized && ETTERNA_VERSION_REGISTRY[normalized]) {
        return normalized;
    }
    return DEFAULT_ETTERNA_VERSION;
}

export function resolveEtternaVersionLoader(value) {
    const requestedVersion = normalizeEtternaVersion(value);
    const requestedEntry = ETTERNA_VERSION_REGISTRY[requestedVersion];

    if (requestedEntry && typeof requestedEntry.loader === "function") {
        return {
            requestedVersion,
            version: requestedVersion,
            loader: requestedEntry.loader,
            fallbackReason: null,
        };
    }

    const fallbackVersion = resolveAvailableFallbackVersion(DEFAULT_ETTERNA_VERSION);
    const fallbackEntry = fallbackVersion ? ETTERNA_VERSION_REGISTRY[fallbackVersion] : null;
    if (!fallbackEntry || typeof fallbackEntry.loader !== "function") {
        throw new Error("No Etterna MinaCalc wasm loader is available");
    }

    return {
        requestedVersion,
        version: fallbackVersion,
        loader: fallbackEntry.loader,
        fallbackReason: requestedEntry?.reason || "Requested Etterna version is unavailable",
    };
}
