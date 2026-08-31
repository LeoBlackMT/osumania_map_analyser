import { fetchBeatmapFile } from "./analysis.js";
import { startGraphAnimationLoop } from "./graph.js";
import {
    updateCardPlayVisibility,
    updateModeTagVisibility,
    updatePauseCountVisibility,
} from "./hud.js";
import { setRecomputeHandler, scheduleRecompute } from "./scheduler.js";
import { loadSettings, applySettingsPayload } from "./settings.js";
import { initTriangleField } from "./triangles.js";
// Side-effect import: presets module self-initializes (registers the preset
// settings-stream listener) on load; it must be loaded exactly once.
import "./presets/index.js";
import { initTelemetry, startTelemetryHeartbeat } from "./telemetry.js";
import { initBridgeClient } from "./sources/bridgeClient.js";
import { handleSongFrame } from "./sources/externalSource.js";
import { applyShellState } from "./sources/shellState.js";
import { initSourceManager } from "./sources/sourceManager.js";
import { setupSocketListener, resumeBufferedOsuState } from "./socketHandlers.js";

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
    initSourceManager({
        onApplied: (next, prev) => {
            // 切回 osu：先回放缓冲的 tosu 状态再 recompute（缓存键对齐）。
            if (next === "osu" && prev !== "osu") {
                resumeBufferedOsuState();
            }
        },
    });
    setupSocketListener();
    initBridgeClient({
        onState: applyShellState,
        onSong: handleSongFrame,
        onSettings: applySettingsPayload,
        onHello: () => {
            // 离线配置（mma-shell.json）拉取：无 tosu 用户可直接编辑该文件，
            // 壳启动时已加载；此处兜底同步一次。
            fetch("http://127.0.0.1:24061/settings")
                .then((r) => (r.ok ? r.json() : null))
                .then((payload) => {
                    if (payload) {
                        applySettingsPayload(payload);
                    }
                })
                .catch(() => {});
        },
    });
    // 延迟初始加载：先等壳桥探测/hello（离线模式避免 "Failed to fetch" 噪声）。
    setTimeout(() => scheduleRecompute("initial load", false), 1200);
}


