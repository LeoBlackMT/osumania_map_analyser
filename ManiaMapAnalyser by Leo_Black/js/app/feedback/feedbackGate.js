// RC 反馈条的唯一准入判定（纯函数：不做 DOM、不发网络）。
//
// 判定顺序与计划 §6.8 的 S1–S9 对齐；任何一步失败都不展示。
// 注意：这里只是"该不该问"，真正的客观字段由 shell 从 native 快照重算并签名，
// 后端还会再校验一次，因此页面侧宁可严格。

import { extractCounts } from "../livePpCounts.js";
import { ALLOWED_MOD_CODES, ACC_MAX, ACC_MIN, DEDUP_KEY_PREFIX, DEDUP_TTL_MS } from "./feedbackSpec.js";
import { evaluatePlayStats, isRcChart, slotCount } from "./feedbackMath.js";

function normalizedStateName(state) {
    return String(state?.clientStateName || "").toLowerCase();
}

function modsAllowed(modCodes) {
    const codes = Array.isArray(modCodes) ? modCodes : [];
    return codes.every((code) => ALLOWED_MOD_CODES.includes(code));
}

function dedupKey(context) {
    return `${DEDUP_KEY_PREFIX}${context.beatmapId}.${Number(context.rate).toFixed(2)}`;
}

/** 本地 30 天去重：命中即不再询问。 */
export function isLocallyDeduped(context, now, storage) {
    if (!context) return true;
    try {
        const store = storage || (typeof localStorage !== "undefined" ? localStorage : null);
        if (!store) return false;
        const previous = Number(store.getItem(dedupKey(context)) || 0);
        return previous > 0 && now - previous < DEDUP_TTL_MS;
    } catch {
        return false;
    }
}

export function markLocallyDeduped(context, now, storage) {
    try {
        const store = storage || (typeof localStorage !== "undefined" ? localStorage : null);
        if (!store) return;
        store.setItem(dedupKey(context), String(now));
    } catch {
        // 隐私模式下 localStorage 可能不可写；只影响再次询问，不影响本次提交。
    }
}

/**
 * @param {object} input
 * @param {object} input.state 页面状态
 * @param {object} input.data 当前 api_v2 原始帧
 * @param {number} input.now Date.now()
 * @param {boolean} input.shellClickThrough 壳窗口是否鼠标穿透
 * @param {boolean} input.deduped 本地是否已问过
 * @returns {{show: boolean, reason: string, stats?: object, context?: object}}
 */
export function evaluateFeedbackCandidate({ state, data, now, shellClickThrough, deduped }) {
    if (!state || !state.enableDifficultyFeedback) return { show: false, reason: "disabled" };
    if (normalizedStateName(state) !== "resultscreen") return { show: false, reason: "not_result_screen" };
    if (String(state.activeSource || "") !== "osu") return { show: false, reason: "not_osu" };

    const context = state.feedbackContext;
    if (!context || context.beatmapKey !== state.lastBeatmapKey) {
        return { show: false, reason: "no_context" };
    }
    if (!Number.isFinite(Number(context.numericDifficulty))) {
        return { show: false, reason: "no_numeric" };
    }
    if (!Number.isFinite(Number(context.lnRatio))) return { show: false, reason: "no_ln_ratio" };
    // OD 是签名的客观字段：必须是本地 .osu 解析出的数字（未知时不允许提问，
    // 否则用户点完只会被壳/后端拒绝）。
    if (typeof context.od !== "number" || !Number.isFinite(context.od)) {
        return { show: false, reason: "no_od" };
    }

    const slots = slotCount(context.estDiff);
    if (slots !== 1) return { show: false, reason: "multi_slot" };
    if (!isRcChart({ lnRatio: context.lnRatio, modeTag: context.modeTag, slots })) {
        return { show: false, reason: "not_rc" };
    }

    if (Number(state.speedRate) !== 1) return { show: false, reason: "rate" };
    if (!modsAllowed(state.modCodes)) return { show: false, reason: "mods" };

    // 谱面身份：resultScreen 帧可能不带 beatmap，用上一帧缓存值。
    const bid = Number(state.currentBeatmapId);
    const checksum = String(state.currentBeatmapChecksum || "").toLowerCase();
    if (!Number.isFinite(bid) || bid <= 0) return { show: false, reason: "no_bid" };
    if (!/^[0-9a-f]{32}$/.test(checksum)) return { show: false, reason: "no_checksum" };

    // 模式：tosu 供数时能直接读；壳原生包没有 mode 字段，交给后端用 osu! API 判定。
    const modeNumber = Number(data?.resultsScreen?.mode?.number);
    if (Number.isFinite(modeNumber) && modeNumber !== 3) return { show: false, reason: "not_mania" };

    const hits = data?.resultsScreen?.hits;
    if (!hits) return { show: false, reason: "no_hits" };

    const counts = extractCounts(hits);
    const objectsTotal = Number(data?.beatmap?.stats?.objects?.total);
    const stats = evaluatePlayStats({ counts, objectsTotal });
    if (!stats.ok) return { show: false, reason: stats.reason, stats };
    if (stats.acc < ACC_MIN || stats.acc > ACC_MAX) return { show: false, reason: "accuracy", stats };

    if (deduped) return { show: false, reason: "dedup" };

    return {
        show: true,
        reason: "ok",
        stats,
        context: {
            ...context,
            beatmapId: bid,
            checksum,
            rate: 1,
            keyCount: Number(context.keyCount) || 0,
            modCodes: Array.isArray(state.modCodes) ? [...state.modCodes] : [],
            clickThrough: Boolean(shellClickThrough),
        },
    };
}
