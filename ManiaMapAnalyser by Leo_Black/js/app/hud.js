import {
    mainCardEl,
    modeTagEl,
    MODE_TAG_OPTIONS,
    overlayEl,
    overlayMessageEl,
    overlaySpinnerEl,
    overlayTitleEl,
    pauseCountEl,
    state,
    statusEl,
} from "./appContext.js";

export function setStatus(message, kind) {
    statusEl.textContent = message;
    statusEl.className = `status ${kind}`;
}

export function setModeTag(tag) {
    const normalized = MODE_TAG_OPTIONS.includes(tag) ? tag : "Mix";
    state.currentModeTag = normalized;

    if (!modeTagEl) {
        return;
    }

    modeTagEl.textContent = normalized;
    modeTagEl.className = `mode-tag mode-${normalized.toLowerCase()}`;
    modeTagEl.hidden = !state.showModeTagCapsule;
}

export function updateModeTagVisibility() {
    if (!modeTagEl) {
        return;
    }
    modeTagEl.hidden = !state.showModeTagCapsule;
}

export function updatePauseCountVisibility() {
    if (!pauseCountEl) {
        return;
    }

    pauseCountEl.classList.remove("active");
    pauseCountEl.classList.remove("idle");

    if (!state.pauseDetectionEnabled) {
        pauseCountEl.textContent = "";
        pauseCountEl.hidden = true;
        return;
    }

    if (state.pauseCount > 0) {
        pauseCountEl.textContent = `Pause Count: ${state.pauseCount}`;
        pauseCountEl.classList.add("active");
        pauseCountEl.hidden = false;
        return;
    }

    pauseCountEl.textContent = "Pause Detection Enabled";
    pauseCountEl.classList.add("idle");
    pauseCountEl.hidden = false;
}

export function updateCardPlayVisibility() {
    if (!mainCardEl) {
        return;
    }

    const shouldHide = state.hideCardDuringPlay && state.clientStateName === "play";
    mainCardEl.classList.toggle("card-hidden-by-play", shouldHide);
    mainCardEl.setAttribute("aria-hidden", shouldHide ? "true" : "false");
}

export function showOverlay({
    title,
    message = "",
    isError = false,
    showSpinner = false,
}) {
    overlayEl.hidden = false;
    overlayEl.classList.toggle("error", isError);
    overlayTitleEl.textContent = title;
    overlayMessageEl.textContent = message;
    overlaySpinnerEl.hidden = !showSpinner;
}

export function hideOverlay() {
    overlayEl.hidden = true;
    overlayEl.classList.remove("error");
    overlayTitleEl.textContent = "";
    overlayMessageEl.textContent = "";
    overlaySpinnerEl.hidden = true;
}
