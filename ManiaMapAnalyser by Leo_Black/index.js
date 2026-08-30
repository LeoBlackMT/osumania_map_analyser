import { initialize } from "./js/app/main.js";

const _VERSION = "2.0.2";

const TELEMETRY_ENDPOINT = "https://mma-stats.leoblack.top";

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


