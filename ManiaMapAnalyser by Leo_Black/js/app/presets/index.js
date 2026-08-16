/**
 * Preset system entry point. Side-effect module: self-initializes on load
 * (imported exactly once from main.js / the manager page).
 */

import { initPresets } from "./core.js";

initPresets();
