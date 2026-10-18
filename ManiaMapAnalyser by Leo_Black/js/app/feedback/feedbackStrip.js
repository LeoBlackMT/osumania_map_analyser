// 结算屏五档反馈条：停靠在 .main-card 内部底边，不弹窗、不抢焦点。
//
// 数据流：页面 gate → POST 127.0.0.1:24061/feedback（壳重算并签名）→ POST 后端。
// 页面只决定"要不要问"和"用户点了哪一档"；客观字段全部由壳签名。
//
// 交互约定：
// - 顶部倒计时条 = 本次提示的剩余时间；鼠标悬停时暂停（倒计时与自动收起一起暂停）。
// - 鼠标穿透时按钮全部禁用并给出提示（穿透状态下本来也点不到）。
// - "Don't ask again" 立即本机静默，并尽力把设置项关掉（离线壳可写；tosu 在线只读）。

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
const SHELL_SETTINGS_URL = "http://127.0.0.1:24061/settings";
const SHELL_CONFIG_URL = "http://127.0.0.1:24061/shell-config";
const CLICK_POLL_MS = 1000;
const RETRY_WINDOW_MS = 2000;
const RETRY_INTERVAL_MS = 500;
const REQUEST_TIMEOUT_MS = 6000;
const LEAVE_ANIMATION_MS = 150;
const OPTOUT_KEY = "mma.feedback.optout.v1";

// 文本用"段位差"而不是模糊的 easier/harder：1 dan = numeric difficulty 1.0
// （Reform 2 → Reform 3），0.5 dan 即半段。tooltip 给出具体例子。
const VOTE_LABELS = [
    {
        vote: -1,
        text: "−1 dan",
        title: "A full dan easier than shown (e.g. shown Reform 3, felt like Reform 2)",
    },
    {
        vote: -0.5,
        text: "−0.5 dan",
        title: "Half a dan easier than shown (e.g. shown Reform 2 high, felt like Reform 2 mid)",
    },
    {
        vote: 0,
        text: "About right",
        title: "Matches the shown estimate (within half a dan)",
    },
    {
        vote: 0.5,
        text: "+0.5 dan",
        title: "Half a dan harder than shown (e.g. shown Reform 2 mid, felt like Reform 2 high)",
    },
    {
        vote: 1,
        text: "+1 dan",
        title: "A full dan harder than shown (e.g. shown Reform 2, felt like Reform 3)",
    },
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
let hideDeadline = 0;
let hideRemaining = 0;
let hideOpts = { resetToken: true };
let retryTimer = 0;
let retryDeadline = 0;
let leavingTimer = 0;
let lastPromptToken = "";
let lastResultToken = "";
let submitting = false;
let infoShownFor = "";

// 鼠标穿透状态：结果屏出现时向壳查一次（`GET /shell-config` → window.clickThrough）。
let clickThrough = false;
let clickThroughFetched = false;
let clickThroughPending = false;
let clickPollTimer = 0;
let lastFrame = null;

function isOptedOut() {
    try {
        return localStorage.getItem(OPTOUT_KEY) === "1";
    } catch {
        return false;
    }
}

function clearOptedOut() {
    try {
        localStorage.removeItem(OPTOUT_KEY);
    } catch {
        // 隐私模式下不可写；忽略。
    }
}

async function refreshClickThrough(data) {
    if (clickThroughPending) return;
    clickThroughPending = true;
    try {
        const response = await fetch(SHELL_CONFIG_URL, { cache: "no-store" });
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

/** 提示显示期间轮询穿透状态：用户按 Ctrl+Shift+C 后按钮立刻启用/禁用。 */
async function syncClickThroughLive() {
    try {
        const response = await fetch(SHELL_CONFIG_URL, { cache: "no-store" });
        if (!response.ok) return;
        const payload = await response.json();
        const next = Boolean(payload?.window?.clickThrough);
        if (next === clickThrough) return;
        clickThrough = next;
        if (activeContext) activeContext.clickThrough = next;
        setButtonsEnabled(!next && !submitting);
        if (hintEl) hintEl.hidden = !next;
    } catch {
        // 壳不可达：保持现状。
    }
}

function startClickPolling() {
    if (clickPollTimer) return;
    clickPollTimer = setInterval(syncClickThroughLive, CLICK_POLL_MS);
}

function stopClickPolling() {
    if (!clickPollTimer) return;
    clearInterval(clickPollTimer);
    clickPollTimer = 0;
}

function tokenFor(context, stats) {
    return `${context.beatmapId}|${context.rate}|${stats.total}|${Number(stats.miss || 0).toFixed(0)}`;
}

function clearTimers() {
    if (showTimer) { clearTimeout(showTimer); showTimer = 0; }
    if (hideTimer) { clearTimeout(hideTimer); hideTimer = 0; }
    if (retryTimer) { clearTimeout(retryTimer); retryTimer = 0; }
    if (leavingTimer) { clearTimeout(leavingTimer); leavingTimer = 0; }
    stopClickPolling();
    hideDeadline = 0;
    hideRemaining = 0;
    if (stripEl) stripEl.classList.remove("is-paused");
}

/** 统一的自动收起调度：支持 hover 暂停/恢复。 */
function scheduleHide(ms, opts = { resetToken: true }) {
    if (hideTimer) clearTimeout(hideTimer);
    hideOpts = opts;
    hideDeadline = Date.now() + ms;
    hideRemaining = 0;
    hideTimer = setTimeout(() => {
        hideTimer = 0;
        hideStrip(opts);
    }, ms);
}

function pauseHideTimer() {
    if (!hideTimer) return;
    clearTimeout(hideTimer);
    hideTimer = 0;
    hideRemaining = Math.max(0, hideDeadline - Date.now());
    if (stripEl) stripEl.classList.add("is-paused");
}

function resumeHideTimer() {
    if (!hideRemaining) {
        if (stripEl) stripEl.classList.remove("is-paused");
        return;
    }
    const remaining = hideRemaining;
    hideRemaining = 0;
    hideDeadline = Date.now() + remaining;
    hideTimer = setTimeout(() => {
        hideTimer = 0;
        hideStrip(hideOpts);
    }, remaining);
    if (stripEl) stripEl.classList.remove("is-paused");
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
        // 淡出动画结束后再真正隐藏；期间若被重新显示，presentStrip 会清掉本定时器。
        stripEl.classList.add("is-leaving");
        leavingTimer = setTimeout(() => {
            leavingTimer = 0;
            stripEl.classList.remove("is-leaving");
            stripEl.classList.remove("is-visible");
            stripEl.classList.remove("is-counting");
            stripEl.hidden = true;
        }, LEAVE_ANIMATION_MS);
    } else if (stripEl) {
        stripEl.classList.remove("is-leaving");
        stripEl.classList.remove("is-counting");
        stripEl.hidden = true;
    }
    if (resetToken) lastResultToken = "";
}

/** 顶部倒计时条：CSS 动画从满格线性收缩到 0（时长 = 提示超时），hover 时暂停。 */
function startCountdown(ms) {
    if (!stripEl || !countdownEl) return;
    stripEl.style.setProperty("--feedback-ms", `${ms}ms`);
    stripEl.classList.remove("is-counting");
    countdownEl.style.transition = "none";
    countdownEl.style.transform = "scaleX(1)";
    void countdownEl.offsetWidth;
    stripEl.classList.add("is-counting");
}

/** 提交后收起倒计时条。 */
function finishCountdown() {
    if (!stripEl || !countdownEl) return;
    stripEl.classList.remove("is-counting");
    countdownEl.style.animation = "none";
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

function setButtonsEnabled(enabled) {
    if (!buttonsEl) return;
    buttonsEl.querySelectorAll("button").forEach((button) => {
        button.disabled = !enabled;
    });
}

function showStrip(context, stats) {
    if (!stripEl || !questionEl || !buttonsEl || !hintEl) return;
    activeContext = context;
    activeStats = stats;
    const estimate = String(context.estDiff || "").trim()
        || `Reform ${Number(context.numericDifficulty).toFixed(2)}`;
    const algorithm = String(context.actualAlgorithm || "").trim() || "the estimator";
    questionEl.textContent = `${estimate} (${algorithm}) — was this about right?`;
    buttonsEl.hidden = false;
    setButtonsEnabled(!context.clickThrough);
    hintEl.hidden = !context.clickThrough;
    hintEl.textContent = "Disable click-through (Ctrl+Shift+C) to rate.";
    presentStrip();
    startCountdown(STRIP_TIMEOUT_MS);
    startClickPolling();
    scheduleHide(STRIP_TIMEOUT_MS);
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
    scheduleHide(STRIP_TIMEOUT_MS, { resetToken: false });
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
    const dismiss = document.createElement("button");
    dismiss.type = "button";
    dismiss.className = "feedback-vote feedback-dismiss";
    dismiss.dataset.dismiss = "1";
    dismiss.textContent = "Don't ask again";
    dismiss.title = "Turn the difficulty feedback setting off";
    buttonsEl.appendChild(dismiss);

    buttonsEl.addEventListener("click", (event) => {
        if (event.target.closest("button[data-dismiss]")) {
            optOutForGood();
            return;
        }
        const button = event.target.closest("button[data-vote]");
        if (!button || submitting || button.disabled) return;
        submitVote(Number(button.dataset.vote), button);
    });

    host.addEventListener("mouseenter", pauseHideTimer);
    host.addEventListener("mouseleave", resumeHideTimer);
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

/** 不再提示：本机立即静默 + 尽力关闭设置项（离线壳可写；tosu 在线只读）。 */
async function optOutForGood() {
    try {
        localStorage.setItem(OPTOUT_KEY, "1");
    } catch {
        // 忽略隐私模式写入失败。
    }
    clearTimers();
    activeContext = null;
    activeStats = null;
    // 离线壳：/settings 可写，设置窗口里的开关会同步变灰；tosu 在线返回 403，只保留本机静默。
    try {
        await fetch(SHELL_SETTINGS_URL, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ enableDifficultyFeedback: false }),
        });
    } catch {
        // 壳不可达时忽略。
    }
    finishCountdown();
    questionEl.textContent = "Feedback disabled. Re-enable it in settings.";
    buttonsEl.hidden = true;
    hintEl.hidden = true;
    scheduleHide(STRIP_THANKS_MS, { resetToken: false });
}

async function submitVote(vote, button) {
    if (!activeContext || !activeStats || submitting) return;
    if (!FEEDBACK_VOTES.includes(vote)) return;
    submitting = true;
    if (hideTimer) { clearTimeout(hideTimer); hideTimer = 0; }
    const context = activeContext;
    const stats = activeStats;
    button.classList.add("is-picked");
    setButtonsEnabled(false);

    const payload = {
        vote,
        displayed_val: Number(context.numericDifficulty),
        actual_algorithm: String(context.actualAlgorithm || ""),
        selected_algorithm: String(context.selectedAlgorithm || ""),
        key_count: Number(context.keyCount) || 0,
        ln_ratio: Number(context.lnRatio),
        pattern: String(context.pattern || "unknown"),
        mode_tag: String(context.modeTag || ""),
        // OD 是客观字段：必须来自本地 .osu 解析；未知时 gate 已拦截，这里再兜一层
        // （绝不能用 Number(null)=0 把它吞成 0）。
        od: typeof context.od === "number" && Number.isFinite(context.od) ? context.od : null,
        settings_hash: settingsHash(state),
    };

    let stage = "shell";
    let failure = "";
    try {
        const signed = await postJson(SHELL_FEEDBACK_URL, payload);
        if (!signed.ok || !signed.json?.payload || !signed.json?.auth) {
            failure = String(signed.json?.error || `HTTP ${signed.status}`);
            throw new Error(`shell rejected (${failure})`);
        }
        stage = "server";
        const sent = await postJson(feedbackEndpoint(), signed.json);
        if (!sent.ok) {
            failure = String(sent.json?.error || `HTTP ${sent.status}`);
            throw new Error(`server rejected (${failure})`);
        }

        markLocallyDeduped({
            beatmapId: context.beatmapId,
            rate: context.rate,
        }, Date.now());
        finishCountdown();
        questionEl.textContent = "Thanks — recorded.";
        buttonsEl.hidden = true;
        hintEl.hidden = true;
        submitting = false;
        scheduleHide(STRIP_THANKS_MS);
    } catch (error) {
        console.warn(`mma feedback: ${error.message}`);
        finishCountdown();
        // 壳拒绝 = 从未上传（Not submitted）；后端拒绝 = 已上传但未记录（Not recorded）。
        const prefix = stage === "shell" ? "Not submitted" : "Not recorded";
        questionEl.textContent = failure ? `${prefix} (${failure}).` : `${prefix}.`;
        buttonsEl.hidden = true;
        hintEl.hidden = true;
        submitting = false;
        scheduleHide(STRIP_THANKS_MS * 2);
    }
    lastPromptToken = tokenFor(context, stats);
}

/**
 * 每帧调用（socketHandlers 在 applyBeatmapState 之后）。只在壳窗口 + 设置开启时工作。
 * @param {object} data api_v2 原始帧
 */
export function noteFeedbackFrame(data) {
    if (!stripEl || !state.enableDifficultyFeedback || isOptedOut()) {
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

/** 设置开关变化时调用：关闭立即收起；重新打开会清掉本机"不再提示"。 */
export function syncFeedbackVisibility() {
    if (!stripEl) return;
    if (!state.enableDifficultyFeedback) {
        hideStrip();
        return;
    }
    clearOptedOut();
}

export function initFeedback() {
    if (!isShellRuntime()) return;
    if (typeof document === "undefined") return;
    buildStrip();
}
