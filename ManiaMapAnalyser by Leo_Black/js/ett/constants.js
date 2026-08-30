// Single source of truth for the Etterna engine surface.
// calc.js (wasm filename map) and versions/index.js (loader registry) both
// build their keyed maps from ETTERNA_VERSION_KEYS; keycount checks use
// SUPPORTED_KEYS. Add a new version to ETTERNA_VERSION_KEYS, then register
// its loader (versions/index.js) and wasm filename (calc.js) mappings.
// Keycounts 4..18: 4/6/7 use their official per-keycount MinaCalc classes
// (0.74.0+), everything else falls back to the generic n-key Bazoinkazoink.
// Older wasm builds (<=0.72.3) only gate 4/6/7 in their FFI, which is safe:
// non-4K is always pinned to 0.74.0 by versions/index.js.
export const SUPPORTED_KEYS = new Set(
    Array.from({ length: 15 }, (_, i) => i + 4), // 4..18
);
export const ETTERNA_VERSION_KEYS = Object.freeze([
    "0.68.0-Unofficial",
    "0.70.0",
    "0.72.0",
    "0.72.3",
    "0.74.0",
    "0.75.0",
]);

// Cache-busting query value for the shipped .wasm files. Bump whenever the
// binary bytes under js/ett/versions/ change (e.g. the MSD cap patch, or the
// 0.74.0 recompile / 0.75.0 addition) so browsers refetch instead of serving
// a stale cached module.
export const WASM_ASSET_VERSION = "3";
