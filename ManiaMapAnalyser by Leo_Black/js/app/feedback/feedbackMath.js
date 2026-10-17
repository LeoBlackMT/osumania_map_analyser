// RC 难度反馈的纯函数（无 DOM、无网络）：ScoreV1 准度、weight 曲线、RC 判定、设置 hash。
//
// 这些函数是页面 gate 与 shell `/feedback` 的共同口径；两边任何一处改动都要同步。

import {
    ACC_MAX,
    ACC_MIN,
    COMPLETION_MIN,
    MISS_RATE_MAX,
    NON_RC_MODE_TAGS,
    RC_LN_MAX,
    TOTAL_HITS_MIN,
    WEIGHT_CENTER,
    WEIGHT_FLOOR,
    WEIGHT_MAX,
    WEIGHT_MIN,
    WEIGHT_SIGMA,
} from "./feedbackSpec.js";

/** 六键计数 → 总 note 数（与 livePpCounts.totalCount 同义，但不依赖其导出顺序）。 */
export function totalHits(counts) {
    return (counts.perfect || 0) + (counts.great || 0) + (counts.good || 0)
        + (counts.ok || 0) + (counts.meh || 0) + (counts.miss || 0);
}

/** 失误率；total 为 0 时返回 1（视作不可用）。 */
export function missRate(counts) {
    const total = totalHits(counts);
    return total > 0 ? (counts.miss || 0) / total : 1;
}

/**
 * ScoreV1 准度（0–100）。
 *
 * 统一到 V1 的理由：lazer 恒 ScoreV2、stable 默认 V1 且 SV2 不上传官网；
 * 用 hits 重算 V1 可以抹掉两端的判定/计分差异，gate 与 weight 只看这一个值。
 */
export function scoreV1(counts) {
    const total = totalHits(counts);
    if (total <= 0) return 0;
    const weighted = 300 * ((counts.perfect || 0) + (counts.great || 0))
        + 200 * (counts.good || 0)
        + 100 * (counts.ok || 0)
        + 50 * (counts.meh || 0);
    return (100 * weighted) / (300 * total);
}

/** weight 曲线：中心 96.5、σ 2.0、底 0.35，再夹到 [0.40, 1.00]（gate 已限制 acc 范围）。 */
export function weightForAcc(acc) {
    if (!Number.isFinite(acc)) return WEIGHT_MIN;
    const raw = WEIGHT_FLOOR
        + (1 - WEIGHT_FLOOR) * Math.exp(-((acc - WEIGHT_CENTER) ** 2) / (2 * WEIGHT_SIGMA * WEIGHT_SIGMA));
    return Math.min(WEIGHT_MAX, Math.max(WEIGHT_MIN, raw));
}

/** 完成率；objectsTotal 不可用时返回 0（gate 会拒绝）。 */
export function completionRatio(total, objectsTotal) {
    const objects = Number(objectsTotal);
    if (!Number.isFinite(objects) || objects <= 0) return 0;
    return total / objects;
}

/** 双槽位数量：`RC || LN` → 2。 */
export function slotCount(estDiff) {
    if (typeof estDiff !== "string" || estDiff.length === 0) return 0;
    return estDiff.split("||").map((part) => part.trim()).filter((part) => part.length > 0).length;
}

/** RC-only 判定：LN 占比、模式标签、单槽位。 */
export function isRcChart({ lnRatio, modeTag, slots }) {
    const ratio = Number(lnRatio);
    if (!Number.isFinite(ratio) || ratio < 0 || ratio > RC_LN_MAX) return false;
    if (NON_RC_MODE_TAGS.includes(String(modeTag || ""))) return false;
    return slots === 1;
}

/** 结算门槛的单一入口；返回首个失败原因，全部通过时 reason 为 "ok"。 */
export function evaluatePlayStats({ counts, objectsTotal }) {
    const total = totalHits(counts);
    if (total < TOTAL_HITS_MIN) return { ok: false, reason: "total_hits", total };
    const completion = completionRatio(total, objectsTotal);
    if (completion < COMPLETION_MIN) return { ok: false, reason: "completion", total, completion };
    const miss = missRate(counts);
    if (miss > MISS_RATE_MAX) return { ok: false, reason: "miss_rate", total, miss };
    const acc = scoreV1(counts);
    if (!Number.isFinite(acc) || acc < ACC_MIN || acc > ACC_MAX) {
        return { ok: false, reason: "accuracy", total, acc };
    }
    return { ok: true, reason: "ok", total, completion, miss, acc };
}

/** FNV-1a 32-bit → 8 hex。只用于把影响估算的设置压成一个短指纹，不上传具体值。 */
export function fnv1aHex(text) {
    let hash = 0x811c9dc5;
    const value = String(text);
    for (let i = 0; i < value.length; i += 1) {
        hash ^= value.charCodeAt(i);
        hash = Math.imul(hash, 0x01000193) >>> 0;
    }
    return hash.toString(16).padStart(8, "0");
}

/** 影响显示数值的设置项（固定顺序）；任何新增项都要同步文档与后端 spec。 */
export const SETTINGS_HASH_FIELDS = Object.freeze([
    "estimatorAlgorithm",
    "forceSunnyWindow",
    "azusaSunnyReferenceHo",
    "extendedEstimationRange",
    "display6kLevel",
    "etternaVersion",
    "companellaEtternaVersion",
    "enableLNDifficulty",
    "enableAlwaysShowLNDifficulty",
    "useSvDetection",
    "vibroDetection",
    "client",
    "classicMod",
]);

export function settingsHash(state) {
    const parts = SETTINGS_HASH_FIELDS.map((key) => {
        const value = state ? state[key] : "";
        return `${key}=${value === null || value === undefined ? "" : String(value)}`;
    });
    return fnv1aHex(parts.join("|"));
}
