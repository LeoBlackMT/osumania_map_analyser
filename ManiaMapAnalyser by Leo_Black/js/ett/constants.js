// Single source of truth for the Etterna engine surface.
// calc.js (wasm filename map) and versions/index.js (loader registry) both
// build their keyed maps from ETTERNA_VERSION_KEYS; keycount checks use
// SUPPORTED_KEYS. Add a new version to ETTERNA_VERSION_KEYS, then register
// its loader (versions/index.js) and wasm filename (calc.js) mappings.
export const SUPPORTED_KEYS = new Set([4, 6, 7]);
export const ETTERNA_VERSION_KEYS = Object.freeze([
    "0.68.0-Unofficial",
    "0.70.0",
    "0.72.0",
    "0.72.3",
    "0.74.0",
]);

// Cache-busting query value for the shipped .wasm files. Bump whenever the
// binary bytes under js/ett/versions/ change (e.g. the MSD cap patch) so
// browsers refetch instead of serving a stale cached module.
export const WASM_ASSET_VERSION = "2";
