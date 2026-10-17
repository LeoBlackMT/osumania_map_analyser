// 结算屏五档反馈条（shell 页内固定条，不放进卡片、不弹窗、不抢焦点）。
//
// 数据流：页面 gate → POST 127.0.0.1:24061/feedback（壳重算并签名）→ POST 后端。
// 页面只决定"要不要问"和"用户点了哪一档"；客观字段全部由壳签名。

import { state } from "../appContext.js";
import {
    FEEDBACK_VOTES,
    STRIP_DELAY_MS,
    STRIP_THANKS_MS,
    STRIP_TIMEOUT_MS,
    feedbackEndpoint,
    isShellRuntime,
} from "./feedbackSpec.js";
import { markLocallyDeduped, evaluateFeedbackCandidate, isLocallyDeduped } from "./feedbackGate.js";
import { settingsHash } from "./feedbackMath.js";

const SHELL_FEEDBACK_URL = "http://127.0.0.1:24061/feedback";
const RETRY_WINDOW_MS = 2000;
const RETRY_INTERVAL_MS = 500;
const REQUEST_TIMEOUT_MS = 6000;

const VOTE_LABELS = [
    { vote: -1, text: "Much easier", title: "This map is at least one tier easier than shown" },
    { vote: -0.5, text: "Easier", title: "This map is slightly easier than shown" },
    { vote: 0, text: "Accurate", title: "The estimate matches how it felt" },
    { vote: 0.5, text: "Harder", title: "This map is slightly harder than shown" },
    { vote: 1, text: "Much harder", title: "This map is at least one tier harder than shown" },
];

let stripEl = null;
let questionEl = null;
let buttonsEl = null;
let hintEl = null;
let activeContext = null;
let activeStats = null;
let showTimer = 0;
let hideTimer = 0;
let retryTimer = 0;
let retryDeadline = 0;
let lastPromptToken = "";
let lastResultToken = "";
let submitting = false;

// 鼠标穿透状态：结果屏出现时向壳查一次（`GET /shell-config` → window.clickThrough）。
// 不用 state 帧：穿透切换不会广播 state，轮询一次比改契约更轻。
let clickThrough = false;
let clickThroughFetched = false;
let clickThroughPending = false;
let lastFrame = null;

async function refreshClickThrough(data) {
    if (clickThroughPending) return;
    clickThroughPending = true;
    try {
        const response = await fetch("http://127.0.0.1:24061/shell-config", { cache: "no-store" });
        if (response.ok) {
            const payload = await response.json();
            clickThrough = Boolean(payload?.window?.clickThrough);
        }
    } catch {
        clickThrough = false;
    } finally {
        clickThroughPending = false;
        clickThroughFetched = true;
        if (lastFrame) noteFeedbackFrame(lastFrame);
    }
}

function tokenFor(context, stats) {
    return `${context.beatmapId}|${context.rate}|${stats.total}|${Number(stats.miss || 0).toFixed(0)}`;
}

function clearTimers() {
    if (showTimer) { clearTimeout(showTimer); showTimer = 0; }
    if (hideTimer) { clearTimeout(hideTimer); hideTimer = 0; }
    if (retryTimer) { clearTimeout(retryTimer); retryTimer = 0; }
}

function hideStrip({ resetToken = true } = {}) {
    clearTimers();
    activeContext = null;
    activeStats = null;
    submitting = false;
    if (stripEl) stripEl.hidden = true;
    if (buttonsEl) {
        buttonsEl.querySelectorAll("button").forEach((button) => {
            button.disabled = false;
            button.classList.remove("is-picked");
        });
    }
    if (resetToken) lastResultToken = "";
}

function showStrip(context, stats) {
    if (!stripEl || !questionEl || !buttonsEl || !hintEl) return;
    activeContext = context;
    activeStats = stats;
    const label = `${context.selectedAlgorithm || "Estimate"} ${Number(context.numericDifficulty).toFixed(2)}`;
    questionEl.textContent = `${label} — how did this feel?`;
    buttonsEl.hidden = context.clickThrough;
    hintEl.hidden = !context.clickThrough;
    hintEl.textContent = "Disable click-through (Ctrl+Shift+C) to rate.";
    stripEl.hidden = false;
    hideTimer = setTimeout(() => hideStrip(), STRIP_TIMEOUT_MS);
}

function buildStrip() {
    const host = document.getElementById("feedback-strip");
    if (!host || stripEl) return;
    stripEl = host;
    questionEl = host.querySelector(".feedback-question");
    buttonsEl = host.querySelector(".feedback-actions");
    hintEl = host.querySelector(".feedback-hint");
    buttonsEl.innerHTML = "";
    for (const item of VOTE_LABELS) {
        const button = document.createElement("button");
        button.type = "button";
        button.className = "feedback-vote";
        button.dataset.vote = String(item.vote);
        button.textContent = item.text;
        button.title = item.title;
        buttonsEl.appendChild(button);
    }
    buttonsEl.addEventListener("click", (event) => {
        const button = event.target.closest("button[data-vote]");
        if (!button || submitting) return;
        submitVote(Number(button.dataset.vote), button);
    });
}

async function postJson(url, body) {
    const controller = typeof AbortController !== "undefined" ? new AbortController() : null;
    const timer = setTimeout(() => controller && controller.abort(), REQUEST_TIMEOUT_MS);
    try {
        const response = await fetch(url, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify(body),
            signal: controller ? controller.signal : undefined,
        });
        const text = await response.text();
        let json = null;
        try { json = text ? JSON.parse(text) : null; } catch { json = null; }
        return { ok: response.ok, status: response.status, json };
    } finally {
        clearTimeout(timer);
    }
}

async function submitVote(vote, button) {
    if (!activeContext || !activeStats || submitting) return;
    if (!FEEDBACK_VOTES.includes(vote)) return;
    submitting = true;
    if (hideTimer) { clearTimeout(hideTimer); hideTimer = 0; }
    const context = activeContext;
    const stats = activeStats;
    button.classList.add("is-picked");
    buttonsEl.querySelectorAll("button").forEach((item) => { item.disabled = true; });

    const payload = {
        vote,
        displayed_val: Number(context.numericDifficulty),
        actual_algorithm: String(context.actualAlgorithm || ""),
        selected_algorithm: String(context.selectedAlgorithm || ""),
        key_count: Number(context.keyCount) || 0,
        ln_ratio: Number(context.lnRatio),
        pattern: String(context.pattern || "unknown"),
        mode_tag: String(context.modeTag || ""),
        od: Number.isFinite(Number(context.od)) ? Number(context.od) : null,
        settings_hash: settingsHash(state),
    };

    try {
        const signed = await postJson(SHELL_FEEDBACK_URL, payload);
        if (!signed.ok || !signed.json?.payload || !signed.json?.auth) {
            throw new Error(`shell rejected (${signed.status})`);
        }
        const sent = await postJson(feedbackEndpoint(), signed.json);
        if (!sent.ok) throw new Error(`server rejected (${sent.status})`);

        markLocallyDeduped({
            beatmapId: context.beatmapId,
            rate: context.rate,
        }, Date.now());
        questionEl.textContent = "Thanks — recorded.";
        buttonsEl.hidden = true;
        hintEl.hidden = true;
        submitting = false;
        hideTimer = setTimeout(() => hideStrip(), STRIP_THANKS_MS);
    } catch (error) {
        console.warn(`mma feedback: ${error.message}`);
        questionEl.textContent = "Not recorded.";
        buttonsEl.hidden = true;
        hintEl.hidden = true;
        submitting = false;
        hideTimer = setTimeout(() => hideStrip(), STRIP_THANKS_MS * 2);
    }
    lastPromptToken = tokenFor(context, stats);
}

/**
 * 每帧调用（socketHandlers 在 applyBeatmapState 之后）。只在壳窗口 + 设置开启时工作。
 * @param {object} data api_v2 原始帧
 */
export function noteFeedbackFrame(data) {
    if (!stripEl || !state.enableDifficultyFeedback) {
        if (stripEl && !stripEl.hidden) hideStrip();
        return;
    }
    if (String(state.clientStateName || "") !== "resultscreen") {
        clickThroughFetched = false;
        if (stripEl && !stripEl.hidden) hideStrip();
        return;
    }

    lastFrame = data;
    if (!clickThroughFetched) {
        refreshClickThrough(data);
        return;
    }

    const now = Date.now();
    const deduped = isLocallyDeduped({
        beatmapId: state.currentBeatmapId,
        rate: state.speedRate,
    }, now);

    const result = evaluateFeedbackCandidate({
        state,
        data,
        now,
        shellClickThrough: clickThrough,
        deduped,
    });

    if (!result.show) {
        // 结算帧的 hits/分析结果可能晚到：在 2s 窗口内重试，不重复弹条。
        const retryable = ["no_hits", "no_context", "no_numeric"].includes(result.reason);
        if (retryable) {
            if (!retryDeadline) retryDeadline = now + RETRY_WINDOW_MS;
            if (now < retryDeadline && !retryTimer) {
                retryTimer = setTimeout(() => {
                    retryTimer = 0;
                    noteFeedbackFrame(data);
                }, RETRY_INTERVAL_MS);
            }
        }
        return;
    }

    retryDeadline = 0;
    const token = tokenFor(result.context, result.stats);
    lastResultToken = token;
    if (token === lastPromptToken) return;
    if (showTimer) return;
    if (stripEl && !stripEl.hidden) return;

    lastPromptToken = token;
    const context = result.context;
    const stats = result.stats;
    showTimer = setTimeout(() => {
        showTimer = 0;
        showStrip(context, stats);
    }, STRIP_DELAY_MS);
}

/** 换图/清卡时调用：撤销待显示与已显示。 */
export function resetFeedbackStrip() {
    lastResultToken = "";
    lastPromptToken = "";
    retryDeadline = 0;
    hideStrip();
}

/** 设置开关变化时调用：关闭立即收起。 */
export function syncFeedbackVisibility() {
    if (!stripEl) return;
    if (!state.enableDifficultyFeedback) hideStrip();
}

export function initFeedback() {
    if (!isShellRuntime()) return;
    if (typeof document === "undefined") return;
    buildStrip();
}
