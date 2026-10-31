// 难度反馈条的唯一准入判定（纯函数：不做 DOM、不发网络）。
//
// 判定顺序与计划 §6.8 的 S1–S9 对齐；任何一步失败都不展示。
// 注意：这里只是"该不该问"，真正的客观字段由 shell 从 native 快照重算并签名，
// 后端还会再校验一次，因此页面侧宁可严格。

import { extractCounts } from "../live/livePpCounts.js";
import {
    ALLOWED_MODE_TAGS_6K7K,
    ALLOWED_MOD_CODES,
    ALLOWED_RATES,
    ALLOWED_TARGET_SIDES,
    ACC_MAX,
    ACC_MIN,
    DEDUP_KEY_PREFIX,
    DEDUP_TTL_MS,
    SUPPORTED_FEEDBACK_KEYS,
} from "./feedbackSpec.js";
import {
    chooseTargetSide,
    evaluatePlayStats,
    isRcChart,
    labelToNumeric,
    slotCount,
    splitDifficultyLabels,
} from "./feedbackMath.js";

function normalizedStateName(state) {
    return String(state?.clientStateName || "").toLowerCase();
}

function modsAllowed(modCodes) {
    const codes = Array.isArray(modCodes) ? modCodes : [];
    return codes.every((code) => ALLOWED_MOD_CODES.includes(code));
}

function dedupKey(context) {
    const base = `${DEDUP_KEY_PREFIX}${context.beatmapId}.${Number(context.rate).toFixed(2)}`;
    // 4K 保持旧键（已有用户的本地去重记录继续有效）；6K/7K 按侧分别去重，
    // 这样同一张图/倍速的 RC 和 LN 都能各问一次。
    const keyCount = Number(context.keyCount);
    if (keyCount !== 6 && keyCount !== 7) return base;
    const side = String(context.targetSide || "RC").toUpperCase() === "LN" ? "LN" : "RC";
    return `${base}.${side}`;
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
 * @param {boolean} input.shellReplay 壳侧说"这一局来自回放"（`/shell-config` 的 feedback.replay）
 * @param {boolean} input.freshPlay 进入本次结算屏时是否从 play 态过来（刚打完的一局）
 * @param {boolean} input.deduped 4K 的本地是否已问过（旧键）
 * @param {string[]} input.dedupedSides 6K/7K 已本地问过的侧（按侧去重）
 * @returns {{show: boolean, reason: string, stats?: object, context?: object}}
 */
export function evaluateFeedbackCandidate({ state, data, now, shellClickThrough, shellReplay, freshPlay, deduped, dedupedSides = [] }) {
    if (!state || !state.enableDifficultyFeedback) return { show: false, reason: "disabled" };
    if (normalizedStateName(state) !== "resultscreen") return { show: false, reason: "not_result_screen" };
    if (String(state.activeSource || "") !== "osu") return { show: false, reason: "not_osu" };
    // 只有"刚打完的一局"才提问：从选曲/本地排行榜直接打开的结算屏（查看历史成绩）
    // 也是结算屏、也有完整判定数，但它不是玩家刚打出的成绩，不该弹反馈条。
    if (freshPlay !== true) return { show: false, reason: "not_fresh_play" };
    // 回放（lazer ReplayPlayer）不代表真实游玩，不允许评分。两个来源都要看：
    // `data.replay` 是原生载荷里的位；`shellReplay` 是壳快照里的同一事实——后者在页面
    // 数据面还没切到原生端点（吃 tosu 帧）时也能挡住回放结果。
    if (data?.replay === true || shellReplay === true) return { show: false, reason: "replay" };

    const context = state.feedbackContext;
    if (!context || context.beatmapKey !== state.lastBeatmapKey) {
        return { show: false, reason: "no_context" };
    }
    if (!Number.isFinite(Number(context.lnRatio))) return { show: false, reason: "no_ln_ratio" };
    // OD 是签名的客观字段：必须是本地 .osu 解析出的数字（未知时不允许提问，
    // 否则用户点完只会被壳/后端拒绝）。
    if (typeof context.od !== "number" || !Number.isFinite(context.od)) {
        return { show: false, reason: "no_od" };
    }

    const keyCount = Number(context.keyCount) || 0;
    if (!SUPPORTED_FEEDBACK_KEYS.includes(keyCount)) {
        return { show: false, reason: "bad_keycount" };
    }
    const numeric = Number(context.numericDifficulty);
    // 4K 保持原判定顺序：numeric 缺失在 ln/模式判定之前拒绝。
    if (keyCount === 4 && !Number.isFinite(numeric)) {
        return { show: false, reason: "no_numeric" };
    }
    const labels = splitDifficultyLabels(context.estDiff);
    const slots = slotCount(context.estDiff);
    let targetSide = "RC";
    let displayedVal = null;

    if (keyCount === 4) {
        // 4K 的条件与限制保持不变：RC-only、单槽位、旧 numeric 语义。
        if (slots !== 1) return { show: false, reason: "multi_slot" };
        if (!isRcChart({ lnRatio: context.lnRatio, modeTag: context.modeTag, slots })) {
            return { show: false, reason: "not_rc" };
        }
        displayedVal = Number(context.numericDifficulty);
        if (!Number.isFinite(displayedVal)) return { show: false, reason: "no_numeric" };
    } else {
        // 6K/7K：算法支持的所有类型都可问；双标签时先问未反馈过的一侧。
        if (!ALLOWED_MODE_TAGS_6K7K.includes(String(context.modeTag || "").toUpperCase())) {
            return { show: false, reason: "bad_mode" };
        }
        if (slots < 1 || slots > 2) return { show: false, reason: "multi_slot" };
        targetSide = chooseTargetSide({
            beatmapId: state.currentBeatmapId,
            rate: state.speedRate,
            keyCount,
            labels,
            modeTag: context.modeTag,
            takenSides: dedupedSides,
        });
        if (!targetSide) return { show: false, reason: "dedup" };
        if (!ALLOWED_TARGET_SIDES.includes(targetSide)) {
            return { show: false, reason: "bad_side" };
        }
        const selectedLabel = targetSide === "LN" ? labels[labels.length - 1] : labels[0];
        displayedVal = labelToNumeric(selectedLabel);
        // 边界标签（`< ...` / `> ...`）没有 numeric；RC 侧可退回 context 的 numeric。
        if (!Number.isFinite(displayedVal) && targetSide === "RC") {
            displayedVal = Number(context.numericDifficulty);
        }
        if (!Number.isFinite(displayedVal)) return { show: false, reason: "no_numeric" };
    }

    if (!ALLOWED_RATES.includes(Number(state.speedRate))) return { show: false, reason: "rate" };
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
    // 结算帧通常不带 beatmap：用最近一次带谱面的帧缓存的对象总数补（否则完成率门
    // 会把"晚到一帧"误判成数据缺失）。
    const rawTotal = Number(data?.beatmap?.stats?.objects?.total);
    const objectsTotal = Number.isFinite(rawTotal) && rawTotal > 0
        ? rawTotal
        : Number(state.currentObjectsTotal);
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
            // 本地去重键用**实际倍速**（读取侧用 state.speedRate）：写 1 会让 DT/HT
            // 永远不命中、又被 NM 的键误伤。
            rate: Number(state.speedRate),
            keyCount: Number(context.keyCount) || 0,
            targetSide,
            displayedVal,
            modCodes: Array.isArray(state.modCodes) ? [...state.modCodes] : [],
            clickThrough: Boolean(shellClickThrough),
        },
    };
}
