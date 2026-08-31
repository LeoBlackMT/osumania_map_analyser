import { initialize } from "./js/app/main.js";
import { sendControl } from "./js/app/sources/bridgeClient.js";

const _VERSION = "2.1.0";

const TELEMETRY_ENDPOINT = "https://mma-stats.leoblack.top";

// 壳窗口拖动把手：HTML drag region 在透明窗口部分环境失效 → mousedown 经桥
// control 帧触发窗口 start_dragging（浏览器模式 no-op）。
document.addEventListener("DOMContentLoaded", () => {
    const dragBar = document.querySelector(".mma-drag-bar");
    if (dragBar) {
        dragBar.addEventListener("mousedown", (event) => {
            if (event.button === 0) {
                event.preventDefault();
                sendControl("dragStart", null);
            }
        });
    }
});

// 页面缩放（壳窗口持久化；浏览器模式同样生效但仅会话内）：
// Ctrl+滚轮 / Ctrl+加号减号 / Ctrl+0 复位，缩放值存 localStorage 启动恢复。
function applyZoom(factor) {
    const clamped = Math.min(2.0, Math.max(0.6, factor));
    document.documentElement.style.zoom = String(clamped);
    try {
        localStorage.setItem("mma.zoom", String(clamped));
    } catch {
        // 存储失败静默
    }
}

function currentZoom() {
    const raw = document.documentElement.style.zoom;
    return raw ? Number.parseFloat(raw) || 1 : 1;
}

(function initZoom() {
    let saved = 1;
    try {
        saved = Number.parseFloat(localStorage.getItem("mma.zoom")) || 1;
    } catch {
        saved = 1;
    }
    applyZoom(saved);
    document.addEventListener("wheel", (event) => {
        if (event.ctrlKey) {
            event.preventDefault();
            applyZoom(currentZoom() + (event.deltaY < 0 ? 0.1 : -0.1));
        }
    }, { passive: false });
    document.addEventListener("keydown", (event) => {
        if (!(event.ctrlKey || event.metaKey)) {
            return;
        }
        if (event.code === "Equal" || event.code === "NumpadAdd") {
            event.preventDefault();
            applyZoom(currentZoom() + 0.1);
        } else if (event.code === "Minus" || event.code === "NumpadSubtract") {
            event.preventDefault();
            applyZoom(currentZoom() - 0.1);
        } else if (event.code === "Digit0" || event.code === "Numpad0") {
            event.preventDefault();
            applyZoom(1);
        }
    });
})();

if (typeof window !== "undefined") {
	window.__MMA_VERSION = _VERSION;
	window.__MMA_TELEMETRY_ENDPOINT = TELEMETRY_ENDPOINT;
}

async function boot() {
	try {
		await initialize();
	} catch (error) {
		const statusEl = document.getElementById("status");
		if (statusEl) {
			statusEl.classList.remove("ok", "loading");
			statusEl.classList.add("error");
			const message = error instanceof Error ? error.message : String(error);
			statusEl.textContent = `Initialization failed: ${message}`;
		}
	}
}

boot();


