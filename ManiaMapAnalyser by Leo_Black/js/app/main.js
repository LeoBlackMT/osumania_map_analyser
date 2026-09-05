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
            // 离线配置拉取：壳按优先级链（tosu 设置文件 > mma-settings.json > 默认）
            // 返回 /settings；无 tosu 用户可直接编辑 mma-settings.json 后重启。
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
    // 延迟初始加载仅用于「壳离线页」（端口 24061，避免 tosu 抓取噪声）；
    // 浏览器模式（tosu 页）立即执行，否则首图/切图/背景会被 1.2s 延迟吞掉。
    const isShellOfflinePage = typeof window !== "undefined"
        && window.location
        && String(window.location.port) === "24061";
    const shellOffline = isShellOfflinePage && state.externalBridgeAvailable && !state.shellTosuOnline;
    if (shellOffline) {
        setTimeout(() => scheduleRecompute("initial load", false), 1200);
    } else {
        scheduleRecompute("initial load", false);
    }
}


