import {
    mainCardEl,
    modeTagSubGroupEl,
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

const SV_TAG_EXIT_DURATION_MS = 220;
let svTagHideTimerId = 0;

function restartAnimationClass(element, className) {
    if (!element || !className) {
        return;
    }

    element.classList.remove(className);
    void element.offsetWidth;
    element.classList.add(className);
}

function clearSvTagHideTimer() {
    if (svTagHideTimerId) {
        clearTimeout(svTagHideTimerId);
        svTagHideTimerId = 0;
    }
}

function hideSvTagImmediately() {
    if (!svTagEl) {
        return;
    }

    clearSvTagHideTimer();
    svTagEl.hidden = true;
    svTagEl.classList.remove("visible", "sv-enter", "sv-exit");
}

function showSvTagAnimated() {
    if (!svTagEl) {
        return;
    }

    clearSvTagHideTimer();
    const wasVisible = !svTagEl.hidden && svTagEl.classList.contains("visible");

    svTagEl.hidden = false;
    svTagEl.classList.remove("sv-exit");
    svTagEl.classList.add("visible");

    if (!wasVisible) {
        restartAnimationClass(svTagEl, "sv-enter");
    }
}

function hideSvTagAnimated() {
    if (!svTagEl) {
        return;
    }

    if (svTagEl.hidden) {
        svTagEl.classList.remove("visible", "sv-enter", "sv-exit");
        return;
    }

    clearSvTagHideTimer();
    svTagEl.classList.remove("sv-enter", "visible");
    restartAnimationClass(svTagEl, "sv-exit");

    svTagHideTimerId = setTimeout(() => {
        svTagEl.hidden = true;
        svTagEl.classList.remove("sv-exit");
        svTagHideTimerId = 0;
    }, SV_TAG_EXIT_DURATION_MS);
}

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
    state.currentModeTag = tag;

    if (!modeTagSubGroupEl) {
        return;
    }
    modeTagSubGroupEl.hidden = !state.showModeTagCapsule;

    if (modeTagSubGroupEl.children.length < 1) {
        const span = document.createElement("span");
        modeTagSubGroupEl.appendChild(span);
    }

    const modeTagEl = modeTagSubGroupEl.children[0];
    const text = tag;
    const nextClassName = `mode-tag mode-${tag.toLowerCase()}`;
    const changed = modeTagEl.textContent !== text || modeTagEl.className !== nextClassName;
    modeTagEl.textContent = text;
    modeTagEl.className = nextClassName;
    if (changed && state.showModeTagCapsule) {
        restartAnimationClass(modeTagEl, "capsule-switch");
    }

    for (let i = 1; i < modeTagSubGroupEl.children.length; i++) {
        modeTagSubGroupEl.children[i].classList.add("hidden-tag");
    }
}

export function setModeTagAdvanced(tag, lnRatio) {
    let tagList = tag;
    if (typeof tagList !== "object") {
        const realTag = MODE_TAG_OPTIONS.includes(tag) ? tag : "Mix"
        tagList = [["All", 1], [realTag, 1]];
    }
    const allCount = tagList.shift()[1];

    tagList.sort((a, b) => {
        if (a[1] == b[1]) return MODE_TAG_OPTIONS.indexOf(b[0]) - MODE_TAG_OPTIONS.indexOf(a[0]);
        else return b[1] - a[1]
    });
    tagList.forEach((a) => a[1] = a[1] *100 /allCount);
    tagList = tagList.filter((a) => a[1] > 0);

    if (lnRatio > 0.15) { // currentModeTag用于graph的显示，我们保留这一逻辑
        state.currentModeTag = tagList.filter((a) => a[0] == "LN" || a[0] == "HB")[0][0];
    }
    else state.currentModeTag = "RC";

    if (!modeTagSubGroupEl) {
        return;
    }
    modeTagSubGroupEl.hidden = !state.showModeTagCapsule;

    for (let i = 0; i < tagList.length; i++) {
        if (modeTagSubGroupEl.children.length == i) {
            const span = document.createElement("span");
            modeTagSubGroupEl.appendChild(span);
        }
        const modeTagEl = modeTagSubGroupEl.children[i];
        const text = tagList[i][1] === 100 ? tagList[i][0] : tagList[i][0] + " " + Math.round(tagList[i][1]) + "%";
        const nextClassName = `mode-tag mode-${tagList[i][0].toLowerCase()}`;
        const changed = modeTagEl.textContent !== text || modeTagEl.className !== nextClassName;
        modeTagEl.textContent = text;
        modeTagEl.className = nextClassName;
        if (changed && state.showModeTagCapsule) {
            restartAnimationClass(modeTagEl, "capsule-switch");
        }
    }
    for (let i = tagList.length; i < modeTagSubGroupEl.children.length; i++) {
        modeTagSubGroupEl.children[i].classList.add("hidden-tag");
    }
}

export function updateModeTagVisibility() {
    if (modeTagSubGroupEl) {
        modeTagSubGroupEl.hidden = !state.showModeTagCapsule;
    }

    if (!svTagEl) {
        return;
    }

    if (!state.showModeTagCapsule) {
        hideSvTagImmediately();
        return;
    }

    if (state.showSvTag) {
        showSvTagAnimated();
    } else {
        hideSvTagAnimated();
    }
}

export function setSvTagVisible(visible) {
    state.showSvTag = Boolean(visible);

    if (!svTagEl) {
        return;
    }

    if (!state.showModeTagCapsule) {
        hideSvTagImmediately();
        return;
    }

    if (state.showSvTag) {
        showSvTagAnimated();
    } else {
        hideSvTagAnimated();
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

    // Always hide the card when on the menu screen.
    if (state.clientStateName === "menu") {
        mainCardEl.classList.toggle("card-hidden-by-play", true);
        mainCardEl.setAttribute("aria-hidden", "true");
        return;
    }

    let shouldHide = false;
    if (state.cardVisibility === "DuringPlay") {
        shouldHide = !state.isInPlayState;
    } else if (state.cardVisibility === "OutsidePlay") {
        shouldHide = state.isInPlayState;
    }
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
