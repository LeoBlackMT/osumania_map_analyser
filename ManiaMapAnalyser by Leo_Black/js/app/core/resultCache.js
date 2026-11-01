// resultCache.js — LRU result cache for analysis results.
// Keys are constructed by the caller; values MUST be JSON-safe data so they
// can survive deepClone (structuredClone, with JSON round-trip fallback).
// Pure module: no imports, no DOM. Safe to load in Node (benchmark runner).

function deepClone(value) {
    if (typeof structuredClone === "function") {
        return structuredClone(value);
    }
    // ponytail: JSON fallback for Node <17 — breaks on undefined/Date, but
    // callers are contractually JSON-safe.
    return JSON.parse(JSON.stringify(value));
}

export function createResultCache({ maxSize = 200 } = {}) {
    const map = new Map(); // insertion order = recency order (oldest first)
    let generation = 0;

    function touch(key) {
        const value = map.get(key);
        if (value !== undefined) {
            map.delete(key);
            map.set(key, value);
            return true;
        }
        return false;
    }

    return {
        get(key) {
            return touch(key) ? deepClone(map.get(key)) : undefined;
        },

        put(key, value, { skip = false } = {}) {
            if (skip) return; // no capacity consumed, no eviction
            if (map.has(key)) {
                map.delete(key);
            } else if (map.size >= maxSize) {
                // evict least-recently-used (first in Map iteration order)
                map.delete(map.keys().next().value);
            }
            map.set(key, deepClone(value));
        },

        has(key) {
            return touch(key);
        },

        clear() {
            map.clear();
            generation += 1;
        },

        get size() {
            return map.size;
        },

        get generation() {
            return generation;
        },
    };
}

// ponytail: single shared instance + clearResultCache() so callers (settings.js,
// analysis.js) can invalidate the same cache without owning the instance.
export const resultCache = createResultCache();

export function clearResultCache() {
    resultCache.clear();
}

export function resultCacheGeneration() {
    return resultCache.generation;
}
