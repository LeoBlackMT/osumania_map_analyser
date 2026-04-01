import { initialize } from "./js/app/main.js";

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


