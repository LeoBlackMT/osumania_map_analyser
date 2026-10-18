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
const LEAVE_ANIMATION_MS = 150;

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
let countdownEl = null;
let activeContext = null;
let activeStats = null;
let showTimer = 0;
let hideTimer = 0;
let retryTimer = 0;
let retryDeadline = 0;
let leavingTimer = 0;
let lastPromptToken = "";
let lastResultToken = "";
let submitting = false;
let infoShownFor = "";

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
    if (leavingTimer) { clearTimeout(leavingTimer); leavingTimer = 0; }
}

function hideStrip({ resetToken = true } = {}) {
    clearTimers();
    activeContext = null;
    activeStats = null;
    submitting = false;
    if (buttonsEl) {
        buttonsEl.querySelectorAll("button").forEach((button) => {
            button.disabled = false;
            button.classList.remove("is-picked");
        });
    }
    if (stripEl && !stripEl.hidden) {
        // 淡出动画结束后再真正隐藏；期间若被重新显示，show 路径会清掉本定时器。
        stripEl.classList.add("is-leaving");
        leavingTimer = setTimeout(() => {
            leavingTimer = 0;
            stripEl.classList.remove("is-leaving");
            stripEl.classList.remove("is-visible");
            stripEl.hidden = true;
        }, LEAVE_ANIMATION_MS);
    } else if (stripEl) {
        stripEl.classList.remove("is-leaving");
        stripEl.hidden = true;
    }
    if (resetToken) lastResultToken = "";
}

/** 顶部倒计时条：从满格线性收缩到 0（时长 = 提示超时）。 */
function startCountdown(ms) {
    if (!countdownEl) return;
    countdownEl.style.transition = "none";
    countdownEl.style.transform = "scaleX(1)";
    void countdownEl.offsetWidth;
    countdownEl.style.transition = `transform ${ms}ms linear`;
    countdownEl.style.transform = "scaleX(0)";
}

/** 提交后冻结并收起倒计时条。 */
function finishCountdown() {
    if (!countdownEl) return;
    countdownEl.style.transition = "transform 150ms ease";
    countdownEl.style.transform = "scaleX(0)";
}

/** 出场前清掉上一次的离场动画，并重新触发入场动画。 */
function presentStrip() {
    if (leavingTimer) { clearTimeout(leavingTimer); leavingTimer = 0; }
    stripEl.classList.remove("is-leaving");
    stripEl.hidden = false;
    stripEl.classList.remove("is-visible");
    void stripEl.offsetWidth;
    stripEl.classList.add("is-visible");
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
    presentStrip();
    startCountdown(STRIP_TIMEOUT_MS);
    hideTimer = setTimeout(() => hideStrip(), STRIP_TIMEOUT_MS);
}

/** 非交互提示（lazer 原生读取没有结算数据等）：一次结果屏只显示一次。 */
function showInfoStrip(text) {
    if (!stripEl || !questionEl) return;
    const key = `${state.lastBeatmapIdentity || ""}|${state.speedRate}`;
    if (infoShownFor === key) return;
    infoShownFor = key;
    clearTimers();
    retryDeadline = 0;
    activeContext = null;
    activeStats = null;
    questionEl.textContent = text;
    buttonsEl.hidden = true;
    hintEl.hidden = true;
    presentStrip();
    startCountdown(STRIP_TIMEOUT_MS);
    hideTimer = setTimeout(() => hideStrip({ resetToken: false }), STRIP_TIMEOUT_MS);
}

/** 最近一次 gate 结果暴露到 window，便于在 devtools 里排查（`__MMAFeedbackLastGate`）。 */
function rememberGate(result) {
    try {
        const stats = result.stats || null;
        window.__MMAFeedbackLastGate = {
            at: new Date().toISOString(),
            reason: result.reason,
            stats: stats ? {
                total: stats.total,
                acc: Number(stats.acc.toFixed(2)),
                completion: Number(stats.completion.toFixed(3)),
            } : null,
        };
        if (window.__MMA_FEEDBACK_DEBUG) {
            console.debug("[mma-feedback] gate:", result.reason, window.__MMAFeedbackLastGate);
        }
    } catch {
        // 调试辅助绝不影响主流程。
    }
}

function buildStrip() {
    const host = document.getElementById("feedback-strip");
    if (!host || stripEl) return;
    stripEl = host;
    questionEl = host.querySelector(".feedback-question");
    buttonsEl = host.querySelector(".feedback-actions");
    hintEl = host.querySelector(".feedback-hint");
    countdownEl = host.querySelector(".feedback-countdown");
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
        // 注意：context.od 为 null（未知）时必须发 null，绝不能被 Number(null)=0 吞成 0——
        // 后端会用 osu! API 的 OD 覆盖 null，但 0 会被当成"报告值 0"而与 API 比对失败。
        od: typeof context.od === "number" && Number.isFinite(context.od) ? context.od : null,
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
        finishCountdown();
        questionEl.textContent = "Thanks — recorded.";
        buttonsEl.hidden = true;
        hintEl.hidden = true;
        submitting = false;
        hideTimer = setTimeout(() => hideStrip(), STRIP_THANKS_MS);
    } catch (error) {
        console.warn(`mma feedback: ${error.message}`);
        finishCountdown();
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
        infoShownFor = "";
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
        rememberGate(result);
        // lazer 原生读取没有结算数据（result_hits 仅 stable 提供）：给出明确提示而不是静默。
        if (result.reason === "no_hits" && String(state.client || "") === "lazer") {
            showInfoStrip("lazer native reading has no result data yet — RC feedback needs osu!stable (or a future lazer offset table).");
            return;
        }
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

    rememberGate(result);
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
    infoShownFor = "";
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
