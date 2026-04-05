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
    svTagEl,
} from "./appContext.js";

function applyStatusMarquee(messageText) {
    if (!statusEl) {
        return;
    }

    statusEl.classList.remove("marquee");
    statusEl.style.removeProperty("--status-marquee-distance");
    statusEl.style.removeProperty("--status-marquee-duration");
    statusEl.textContent = messageText;

    if (!state.enableStatusMarquee) {
        return;
    }

    const prefersReducedMotion = typeof window !== "undefined"
        && typeof window.matchMedia === "function"
        && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (prefersReducedMotion) {
        return;
    }

    if (statusEl.clientWidth <= 0) {
        return;
    }

    const overflowPx = statusEl.scrollWidth - statusEl.clientWidth;
    if (!(overflowPx > 12)) {
        return;
    }

    const distance = overflowPx + 28;
    const duration = Math.max(7, Math.min(20, distance / 18));
    const trackEl = document.createElement("span");
    trackEl.className = "status-track";
    trackEl.textContent = messageText;

    statusEl.textContent = "";
    statusEl.appendChild(trackEl);
    statusEl.style.setProperty("--status-marquee-distance", `${Math.round(distance)}px`);
    statusEl.style.setProperty("--status-marquee-duration", `${duration.toFixed(2)}s`);
    statusEl.classList.add("marquee");
}

export function setStatus(message, kind) {
    const text = String(message ?? "");
    state.statusText = text;
    state.statusKind = kind;

    if (!statusEl) {
        return;
    }

    statusEl.className = `status ${kind}`;
    applyStatusMarquee(text);
}

export function refreshStatusRendering() {
    setStatus(state.statusText || "", state.statusKind || "loading");
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
    if (modeTagEl) {
        modeTagEl.hidden = !state.showModeTagCapsule;
    }

    if (svTagEl) {
        svTagEl.hidden = !state.showModeTagCapsule || !state.showSvTag;
    }
}

export function setSvTagVisible(visible) {
    state.showSvTag = Boolean(visible);
    if (svTagEl) {
        svTagEl.hidden = !state.showModeTagCapsule || !state.showSvTag;
    }
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
