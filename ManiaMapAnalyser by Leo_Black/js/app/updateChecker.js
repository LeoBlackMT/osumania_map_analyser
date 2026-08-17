const LATEST_RELEASE_API = "https://api.github.com/repos/LeoBlackMT/osumania_map_analyser/releases/latest";
const UPDATE_CHECK_CACHE_KEY = "mma.updateCheck.latestRelease.v1";
const UPDATE_CHECK_SESSION_KEY = "mma.updateCheck.checkedThisSession.v1";
const ONE_DAY_MS = 24 * 60 * 60 * 1000;
const REQUEST_TIMEOUT_MS = 5000;
let inFlightCheckPromise = null;

function normalizeVersionParts(value) {
    const text = String(value ?? "").trim().replace(/^v/i, "");
    const numbers = text.match(/\d+/g);
    if (!numbers || !numbers.length) {
        return null;
    }

    const parts = [0, 0, 0];
    for (let i = 0; i < Math.min(3, numbers.length); i += 1) {
        parts[i] = Number.parseInt(numbers[i], 10) || 0;
    }

    return parts;
}

function compareVersionParts(left, right) {
    for (let i = 0; i < 3; i += 1) {
        const a = left[i] || 0;
        const b = right[i] || 0;
        if (a > b) {
            return 1;
        }
        if (a < b) {
            return -1;
        }
    }
    return 0;
}

function isLatestReleaseNewer(currentVersion, latestTag) {
    const current = normalizeVersionParts(currentVersion);
    const latest = normalizeVersionParts(latestTag);

    if (!current || !latest) {
        return false;
    }

    return compareVersionParts(latest, current) > 0;
}

function readStorageValue(key) {
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

function writeStorageValue(key, value) {
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

function readCache() {
    try {
        const raw = readStorageValue(UPDATE_CHECK_CACHE_KEY);
        if (!raw) {
            return null;
        }

        const parsed = JSON.parse(raw);
        if (!parsed || typeof parsed !== "object") {
            return null;
        }

        return {
            checkedAt: Number(parsed.checkedAt) || 0,
            latestTag: typeof parsed.latestTag === "string" ? parsed.latestTag : "",
            latestUrl: typeof parsed.latestUrl === "string" ? parsed.latestUrl : "",
            hasUpdate: Boolean(parsed.hasUpdate),
        };
    } catch {
        return null;
    }
}

function writeCache(cache) {
    writeStorageValue(UPDATE_CHECK_CACHE_KEY, JSON.stringify(cache));
}

function markCheckedThisSession() {
    writeStorageValue(UPDATE_CHECK_SESSION_KEY, "1");
}

function wasCheckedThisSession() {
    return readStorageValue(UPDATE_CHECK_SESSION_KEY) === "1";
}

function emitResult(onResult, hasUpdate, latestTag = "", latestUrl = "") {
    if (typeof onResult !== "function") {
        return;
    }
    onResult({ hasUpdate: Boolean(hasUpdate), latestTag, latestUrl });
}

function resolveCachedHasUpdate(cache, currentVersion) {
    if (!cache || typeof cache !== "object") {
        return false;
    }

    if (typeof cache.latestTag === "string" && cache.latestTag.trim().length > 0) {
        return isLatestReleaseNewer(currentVersion, cache.latestTag);
    }

    return Boolean(cache.hasUpdate);
}

export async function runUpdateCheckIfDue({ enabled, currentVersion, onResult }) {
    return runUpdateCheckIfDueInternal({
        enabled,
        currentVersion,
        onResult,
        force: false,
    });
}

export async function runUpdateCheckNow({ enabled, currentVersion, onResult }) {
    return runUpdateCheckIfDueInternal({
        enabled,
        currentVersion,
        onResult,
        force: true,
    });
}

async function runUpdateCheckIfDueInternal({ enabled, currentVersion, onResult, force }) {
    if (!enabled) {
        emitResult(onResult, false);
        return;
    }

    if (inFlightCheckPromise) {
        await inFlightCheckPromise;
        const cacheAfterInflight = readCache();
        if (cacheAfterInflight) {
            emitResult(
                onResult,
                resolveCachedHasUpdate(cacheAfterInflight, currentVersion),
                cacheAfterInflight.latestTag,
                cacheAfterInflight.latestUrl,
            );
            return;
        }
    }

    const now = Date.now();
    const cache = readCache();

    if (!force && cache && wasCheckedThisSession()) {
        emitResult(onResult, resolveCachedHasUpdate(cache, currentVersion), cache.latestTag, cache.latestUrl);
        return;
    }

    if (!force && cache && cache.checkedAt > 0 && (now - cache.checkedAt) < ONE_DAY_MS) {
        markCheckedThisSession();
        emitResult(onResult, resolveCachedHasUpdate(cache, currentVersion), cache.latestTag, cache.latestUrl);
        return;
    }

    let timeoutId = 0;
    try {
        const hasAbortController = typeof AbortController !== "undefined";
        const controller = hasAbortController ? new AbortController() : null;
        timeoutId = setTimeout(() => {
            if (controller) {
                controller.abort();
            }
        }, REQUEST_TIMEOUT_MS);

        inFlightCheckPromise = fetch(LATEST_RELEASE_API, {
            method: "GET",
            cache: "no-store",
            headers: {
                Accept: "application/vnd.github+json",
            },
            signal: controller ? controller.signal : undefined,
        });
        const response = await inFlightCheckPromise;
        clearTimeout(timeoutId);
        timeoutId = 0;

        if (!response.ok) {
            throw new Error(`latest release request failed: ${response.status}`);
        }

        const payload = await response.json();
        const latestTag = typeof payload?.tag_name === "string" ? payload.tag_name : "";
        const latestUrl = typeof payload?.html_url === "string" ? payload.html_url : "";
        const hasUpdate = isLatestReleaseNewer(currentVersion, latestTag);

        writeCache({
            checkedAt: now,
            latestTag,
            latestUrl,
            hasUpdate,
        });
        markCheckedThisSession();

        emitResult(onResult, hasUpdate, latestTag, latestUrl);
    } catch {
        writeCache({
            checkedAt: now,
            latestTag: "",
            latestUrl: "",
            hasUpdate: false,
        });
        markCheckedThisSession();

        emitResult(onResult, false, "", "");
    } finally {
        if (timeoutId) {
            clearTimeout(timeoutId);
        }
        inFlightCheckPromise = null;
    }
}
