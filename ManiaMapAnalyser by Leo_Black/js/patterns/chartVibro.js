// vibro 纯函数（共享模块，Node/浏览器同款）：
//
// - detectVibroFromMetadata：谱面元数据（Title / Version）含关键词即判 vibro，
//   关键词表在 config.js 的 APP_CONFIG.vibroKeywords，大小写不敏感。
// - detectVibro：Etterna MSD 口径（JackSpeed / Overall ≥ threshold）。
// - detectVibroFromLongjackPattern：pattern report 口径（BPM ≥ minBpm 的簇里 Longjacks 占比 ≥ threshold）。
//
// 本模块为共享纯函数：禁止 window/document，禁止 import js/app/。

import { APP_CONFIG } from "../../config.js";

/**
 * 元数据关键词直判：标题或难度名包含关键词（大小写不敏感）即视为 vibro。
 * 关键词表在 config.js 的 APP_CONFIG.vibroKeywords。
 */
export function detectVibroFromMetadata(metaData) {
    const keywords = Array.isArray(APP_CONFIG.vibroKeywords) ? APP_CONFIG.vibroKeywords : [];
    if (keywords.length === 0 || !metaData || typeof metaData !== "object") return false;
    const haystack = `${metaData.Title ?? metaData.title ?? ""}\n${metaData.Version ?? metaData.version ?? ""}`.toLowerCase();
    if (!haystack.trim()) return false;
    return keywords.some((keyword) => {
        const needle = String(keyword ?? "").toLowerCase();
        return needle.length > 0 && haystack.includes(needle);
    });
}

// ─────────────── 既有判据（原 js/app/vibro.js，已合并进本共享模块） ───────────────
// 这两个判据自插件早期就存在，随 vibro 检测统一收敛到共享模块（Node/浏览器同款纯函数），
// 语义与阈值未改动；浏览器专属的 MSD 取值仍留在 js/app/analysis.js（resolveVibroMsdValues）。

function pickNumber(obj, keys) {
    if (!obj || typeof obj !== "object") {
        return null;
    }

    for (const key of keys) {
        const value = Number(obj[key]);
        if (Number.isFinite(value)) {
            return value;
        }
    }

    return null;
}

/**
 * Etterna MSD 口径的 vibro 判据：JackSpeed / Overall ≥ threshold（插件默认 0.95）。
 * 调用方负责提供 MSD values（浏览器侧 4K 固定用 0.72.3 基准，见 analysis.js 的 resolveVibroMsdValues）。
 */
export function detectVibro(values, threshold) {
    const overall = pickNumber(values, ["Overall", "overall"]);
    const jackSpeed = pickNumber(values, ["JackSpeed", "Jackspeed", "jackSpeed", "jackspeed"]);

    if (!Number.isFinite(overall) || overall <= 0 || !Number.isFinite(jackSpeed)) {
        return false;
    }

    return (jackSpeed / overall) >= threshold;
}

/**
 * pattern report 口径的 vibro 判据：存在 BPM ≥ minBpm 且 Longjacks 占比 ≥ threshold 的簇。
 */
export function detectVibroFromLongjackPattern(patternReport, threshold, minBpm) {
    if (!patternReport || !Array.isArray(patternReport.Clusters)) {
        return false;
    }

    const bpmLimit = Number.isFinite(minBpm) && minBpm > 0 ? minBpm : 0;

    for (const cluster of patternReport.Clusters) {
        if (!Array.isArray(cluster.SpecificTypes)) {
            continue;
        }
        const clusterBpm = Number(cluster.BPM);
        if (!Number.isFinite(clusterBpm) || clusterBpm < bpmLimit) {
            continue;
        }
        for (const [name, ratio] of cluster.SpecificTypes) {
            if (name === "Longjacks" && Number.isFinite(ratio) && ratio >= threshold) {
                return true;
            }
        }
    }

    return false;
}
