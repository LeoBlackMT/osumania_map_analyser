import createMinaCalc740 from "./minaclac-74.0.js";
import createMinaCalc750 from "./minaclac-75.0.js";
import createMinaCalc680Unofficial from "./minaclac-68.0-unofficial.js";
import createMinaCalc700 from "./minaclac-70.0.js";
import createMinaCalc720 from "./minaclac-72.0.js";
import createMinaCalc723 from "./minaclac-72.3.js";
import { ETTERNA_VERSION_KEYS, SUPPORTED_KEYS } from "../constants.js";

const COMMON_SUPPORTED_KEYCOUNTS = Object.freeze([...SUPPORTED_KEYS]);
// All non-4K keycounts are pinned to 0.74.0: it is the first MinaCalc with a
// real n-key pipeline (internal 515), while older builds' FFI only gates
// 4/6/7 and the 4K MinaCalc rejects anything wider. 0.75.0 (527) shares the
// same n-key structure but stays selectable for 4K comparisons.
export const NON_4K_ETTERNA_FALLBACK_VERSION = "0.74.0";

// Version -> loader mapping (keys guaranteed by ETTERNA_VERSION_KEYS; a
// missing loader entry degrades via resolveAvailableFallbackVersion).
const LOADER_BY_VERSION = Object.freeze({
    "0.68.0-Unofficial": createMinaCalc680Unofficial,
    "0.70.0": createMinaCalc700,
    "0.72.0": createMinaCalc720,
    "0.72.3": createMinaCalc723,
    "0.74.0": createMinaCalc740,
    "0.75.0": createMinaCalc750,
});

const ETTERNA_VERSION_REGISTRY = Object.freeze(Object.fromEntries(
    ETTERNA_VERSION_KEYS.map((version) => [
        version,
        {
            loader: LOADER_BY_VERSION[version],
            reason: null,
            supportedKeycounts: COMMON_SUPPORTED_KEYCOUNTS,
        },
    ]),
));

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

export function supportsEtternaKeycount(version, keycount) {
    const entry = ETTERNA_VERSION_REGISTRY[version];
    if (!entry || !Array.isArray(entry.supportedKeycounts)) {
        return false;
    }

    const parsedKeycount = Number(keycount);
    if (!Number.isFinite(parsedKeycount)) {
        return false;
    }

    return entry.supportedKeycounts.includes(parsedKeycount);
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

export function resolveEtternaVersionLoaderForKeycount(value, keycount) {
    const resolved = resolveEtternaVersionLoader(value);
    const parsedKeycount = Number(keycount);

    const shouldPreferNon4kStableVersion = parsedKeycount !== 4;
    if (shouldPreferNon4kStableVersion
        && resolved.version !== NON_4K_ETTERNA_FALLBACK_VERSION
        && supportsEtternaKeycount(NON_4K_ETTERNA_FALLBACK_VERSION, parsedKeycount)) {
        const preferredEntry = ETTERNA_VERSION_REGISTRY[NON_4K_ETTERNA_FALLBACK_VERSION];
        const preferenceReason = `Using ${NON_4K_ETTERNA_FALLBACK_VERSION} for non-4K stability`;
        return {
            requestedVersion: resolved.requestedVersion,
            version: NON_4K_ETTERNA_FALLBACK_VERSION,
            loader: preferredEntry.loader,
            fallbackReason: [resolved.fallbackReason, preferenceReason]
                .filter(Boolean)
                .join("; "),
        };
    }

    if (supportsEtternaKeycount(resolved.version, parsedKeycount)) {
        return resolved;
    }

    const preferredFallbackVersion = NON_4K_ETTERNA_FALLBACK_VERSION;
    const fallbackVersion = supportsEtternaKeycount(preferredFallbackVersion, parsedKeycount)
        ? preferredFallbackVersion
        : listEtternaVersions().find((version) => supportsEtternaKeycount(version, parsedKeycount));

    if (!fallbackVersion) {
        return resolved;
    }

    const fallbackEntry = ETTERNA_VERSION_REGISTRY[fallbackVersion];
    if (!fallbackEntry || typeof fallbackEntry.loader !== "function") {
        return resolved;
    }

    const keycountReason = `Etterna ${resolved.version} does not support ${parsedKeycount}K; fell back to ${fallbackVersion}`;
    return {
        requestedVersion: resolved.requestedVersion,
        version: fallbackVersion,
        loader: fallbackEntry.loader,
        fallbackReason: [resolved.fallbackReason, keycountReason]
            .filter(Boolean)
            .join("; "),
    };
}
