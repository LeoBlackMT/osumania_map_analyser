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


