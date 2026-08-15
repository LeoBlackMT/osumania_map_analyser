import { fetchBeatmapFile } from "./analysis.js";
import { startGraphAnimationLoop } from "./graph.js";
import {
    updateCardPlayVisibility,
    updateModeTagVisibility,
    updatePauseCountVisibility,
} from "./hud.js";
import { setRecomputeHandler, scheduleRecompute } from "./scheduler.js";
import { loadSettings } from "./settings.js";
import { setupSocketListener } from "./socketHandlers.js";
import { initTriangleField } from "./triangles.js";
import { initTelemetry, startTelemetryHeartbeat } from "./telemetry.js";

setRecomputeHandler(fetchBeatmapFile);

export async function initialize() {
    initTriangleField();
    await loadSettings();
    initTelemetry();
    startTelemetryHeartbeat();
    updateModeTagVisibility();
    updatePauseCountVisibility();
    updateCardPlayVisibility();
    startGraphAnimationLoop();
    setupSocketListener();
    scheduleRecompute("initial load", false);
}


