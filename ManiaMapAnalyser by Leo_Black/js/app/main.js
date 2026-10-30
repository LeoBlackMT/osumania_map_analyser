import { fetchBeatmapFile } from "./analysis.js";
import { isRuntimeOsuOverrideActive, state } from "./appContext.js";
import { startGraphAnimationLoop } from "./graph.js";
import {
    updateCardPlayVisibility,
    updateModeTagVisibility,
    updatePauseCountVisibility,
} from "./hud.js";
import { setRecomputeHandler, scheduleRecompute } from "./scheduler.js";
import {
    applyCombinedModeSetting,
    applyLaneAccSpaceMode,
    applySettingsPayload,
    loadSettings,
    setTabContentBar,
} from "./settings.js";
import { initTriangleField } from "./triangles.js";
import { initContentBarTabs } from "./contentBarTabs.js";
import { fetchShellConfig, isShellConfigPayload } from "./shellConfigClient.js";
// 壳窗口可视提示：拖动把手跟随卡片顶沿 + 鼠标靠近窗口边缘时点亮那条边
// （浏览器/tosu 模式内部自会 no-op，shell-mode 由桥连接后异步挂上）。
import { initWindowAffordances } from "./windowAffordances.js";
// Side-effect import: presets module self-initializes (registers the preset
// settings-stream listener) on load; it must be loaded exactly once.
import "./presets/index.js";
import { initTelemetry, startTelemetryHeartbeat } from "./telemetry.js";
import { initBridgeClient, sendLaneAccSync } from "./sources/bridgeClient.js";
import { handleSongFrame } from "./sources/externalSource.js";
import { applyShellState, applyMalody4Selection } from "./sources/shellState.js";
import { initSourceManager } from "./sources/sourceManager.js";
import { setupSocketListener, resumeBufferedOsuState } from "./socketHandlers.js";
import { initFeedback } from "./feedback/feedbackStrip.js";
import { initGraphSection } from "./graphSection/index.js";
import { laneAccView } from "./laneAcc/view.js";

setRecomputeHandler(fetchBeatmapFile);

/**
 * 壳配置的启动读取：只有壳窗口会真的发请求（fetchShellConfig 内部判 isShellRuntime）。
 * 失败时保持 combinedMode 默认关闭，后续 settings 广播仍可补上。
 */
async function initShellConfig() {
    const config = await fetchShellConfig();
    if (config) {
        applyCombinedModeSetting(config.combinedMode);
        applyLaneAccSpaceMode(config.laneAccSpaceMode);
    }
}

export async function initialize() {
    initTriangleField();
    initWindowAffordances();
    initContentBarTabs({ onSelect: setTabContentBar });
    laneAccView.init();
    await initShellConfig();
    if (typeof window !== "undefined" && window.location?.port === "24061") {
        fetch("http://127.0.0.1:24061/offsets/status")
            .then((response) => (response.ok ? response.json() : null))
            .then((body) => {
                state.laneAccScoreReady = body?.lazer?.score_ready === true;
                if (state.laneAccScoreReady === false) {
                    laneAccView.setUnavailable("offsets");
                }
            })
            .catch(() => {});
    }
    await loadSettings();
    initTelemetry();
    startTelemetryHeartbeat();
    // 仅壳窗口（24061 或壳的 hash 标记）会真的建出反馈条；浏览器/OBS 无 DOM。
    initFeedback();
    // 图表选区交互同理：非壳环境内部直接 return，不挂任何监听。
    initGraphSection();
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
    // Temporary replay-event dump switch: ?hitdump=1 or
    // localStorage.MMA_HIT_DUMP=1; call window.__mmaDownloadHitDump().
    const hitDump = { events: [] };
    const hitDumpEnabled = (() => {
        try {
            return new URLSearchParams(window.location.search).get("hitdump") === "1"
                || window.localStorage.getItem("MMA_HIT_DUMP") === "1";
        } catch {
            return false;
        }
    })();
    window.__mmaHitDump = hitDump.events;
    window.__mmaDownloadHitDump = () => {
        const blob = new Blob([JSON.stringify(hitDump.events)], { type: "application/json" });
        const url = URL.createObjectURL(blob);
        const a = document.createElement("a");
        a.href = url;
        a.download = "mma-hit-events.json";
        a.click();
        URL.revokeObjectURL(url);
    };
    initBridgeClient({
        onState: applyShellState,
        onSong: handleSongFrame,
        onMalody4Selection: applyMalody4Selection,
        onSettings: (payload) => {
            // 壳配置与插件设置共用 settings 帧；壳配置 payload 带 combinedMode，
            // 先应用它，再交给插件设置处理器（后者只认自己的 uniqueID）。
            if (isShellConfigPayload(payload)) {
                applyCombinedModeSetting(payload.combinedMode);
                applyLaneAccSpaceMode(payload.laneAccSpaceMode);
            }
            applySettingsPayload(payload);
        },
        onManiaHitEvents: (payload) => {
            if (hitDumpEnabled && Array.isArray(payload?.events)) {
                if (payload.reset) {
                    hitDump.events.length = 0;
                }
                hitDump.events.push(...payload.events);
            }
            laneAccView.applyBatch(payload);
        },
        onManiaReplayFrames: (payload) => {
            laneAccView.applyReplayFrames(payload);
        },
        onHello: (payload) => {
            state.shellContractVersion = Number(payload?.contract) || 0;
            if (state.shellContractVersion < 9) {
                laneAccView.setUnavailable("contract");
            }
            sendLaneAccSync();
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
    // 页面自己的 tosu 数据面已收到载荷（state.tosuDataSeen）时同样立即执行——
    // 有数据就不会有"抓取噪声"，等 1.2s 只是白等。
    const isShellOfflinePage = typeof window !== "undefined"
        && window.location
        && String(window.location.port) === "24061";
    const shellOffline = isShellOfflinePage
        && !state.shellTosuOnline && !state.tosuDataSeen && !isRuntimeOsuOverrideActive();
    if (shellOffline) {
        // 兜底定时器登记在 state.recalcTimerId 上：窗口期内 tosu 载荷到达时，
        // socketHandlers 的 scheduleRecompute 会清掉它并在 200ms 内计算，
        // 不会白等满 1.2s、也不会多跑一次 "initial load"。
        state.recalcTimerId = setTimeout(() => {
            state.recalcTimerId = null;
            scheduleRecompute("initial load", false);
        }, 1200);
    } else {
        scheduleRecompute("initial load", false);
    }
}


