// aleju03：4K LN 难度估计器（移植自 mania-hub 自研 LN 算法）
//
// 计算层在 `js/estimator/aleju03/`（features.js 结构特征 + lnReference.js 参考邻域估计器，
// 参考数据表 lnReferenceCharts.js 逐字提取）。本文件只做"我们的估算器契约"适配：
//  - 入口签名 `runAleju03EstimatorFromText(osuText, options = {}, parsed = null)`（与其它估算器一致）；
//  - 星数口径：沿用 Sunny 原始 sr（与 Mixed/Azusa/Roxy 的星数胶囊口径一致），
//    优先复用 options.precomputedSunnyResult，避免重复计算；
//  - 只处理 4K；**输出只含 LN 难度**（形如 "LN 7 mid/high"，与区间表同一套 tier 词表），
//    不拼接 RC 半；`numericDifficulty` 置 null（该算法不产出 RC 数值）；
//  - 显式选定该算法时跳过源的 LN 候选门（forceLn=true），任意 4K 谱面都给出 LN 判决；
//  - 非 4K、解析失败或特征提取失败时整体回退 Sunny，`actualEstimatorAlgorithm` 记为 "Sunny"，
//    原因写在 `aleju03Ln.reason`。
// 共享纯函数：禁止 window/document，禁止 import js/app/。

import { OsuFileParser } from "../parser/osuFileParser.js";
import { runSunnyEstimatorFromText } from "./sunnyEstimator.js";
import { extractDanFeatures } from "./aleju03/features.js";
import { estimateLnDan } from "./aleju03/lnReference.js";

// Field-copy clone of a processed OsuFileParser（与 sunnyAlgorithm.js 的同名工具一致）：
// modIN/modHO 会原地改写 columns/noteStarts/noteTypes/noteEnds/breaks，
// 因此共享的 parsed 实例必须在克隆上转换，保持 pristine 供其它消费者使用。
function cloneOsuParser(src) {
    const parser = new OsuFileParser("");
    parser.od = src.od;
    parser.columnCount = src.columnCount;
    parser.columns = [...src.columns];
    parser.noteStarts = [...src.noteStarts];
    parser.noteEnds = [...src.noteEnds];
    parser.noteTypes = [...src.noteTypes];
    parser.gameMode = src.gameMode;
    parser.status = src.status;
    parser.lnRatio = src.lnRatio;
    parser.metaData = { ...src.metaData };
    parser.breaks = src.breaks.map((entry) => [...entry]);
    parser.objectIntervals = src.objectIntervals.map((entry) => [...entry]);
    parser.timingPoints = src.timingPoints.map((entry) => [...entry]);
    return parser;
}

function resolveParser(osuText, parsed, needsConvert) {
    if (!parsed) {
        const fresh = new OsuFileParser(osuText);
        fresh.process();
        return needsConvert ? cloneOsuParser(fresh) : fresh;
    }
    return needsConvert ? cloneOsuParser(parsed) : parsed;
}

// 把解析结果转成 aleju03 计算层的 map 形态：
// `{ notes: [{ column, time, endTime, isHold }], keyCount, totalLength, breakPeriods, title, version }`
function buildAlejuMap(parsedData) {
    const columns = Array.isArray(parsedData?.columns) ? parsedData.columns : [];
    const starts = Array.isArray(parsedData?.noteStarts) ? parsedData.noteStarts : [];
    const ends = Array.isArray(parsedData?.noteEnds) ? parsedData.noteEnds : [];
    const types = Array.isArray(parsedData?.noteTypes) ? parsedData.noteTypes : [];
    const count = Math.min(columns.length, starts.length, types.length);
    const notes = [];
    let totalLength = 0;
    for (let index = 0; index < count; index++) {
        const type = Number(types[index]) || 0;
        const time = Number(starts[index]);
        if (!Number.isFinite(time)) continue;
        const isHold = (type & 128) !== 0;
        const endTime = isHold && Number.isFinite(Number(ends[index])) ? Number(ends[index]) : time;
        notes.push({ column: Number(columns[index]), time, endTime, isHold });
        if (endTime > totalLength) totalLength = endTime;
    }
    const metaData = parsedData?.metaData || {};
    return {
        notes,
        keyCount: Number(parsedData?.columnCount) || 0,
        totalLength,
        breakPeriods: (Array.isArray(parsedData?.breaks) ? parsedData.breaks : [])
            .map((entry) => ({ startTime: Number(entry?.[0]), endTime: Number(entry?.[1]) }))
            .filter((period) => Number.isFinite(period.startTime) && Number.isFinite(period.endTime)),
        title: String(metaData.Title ?? ""),
        version: String(metaData.Version ?? ""),
    };
}

// aleju03 的变体后缀 → 区间表同款 tier 词表（low / mid-low / mid / mid-high / high），
// 使标签与现有 "LN n tier" 格式完全一致：下游按 "||" 与 tier 词解析，无需任何特例分支。
const LN_TIER_BY_VARIANT = { "++": "high", "+": "mid/high", "": "mid", "-": "mid/low", "--": "low" };

function formatLnLabel(estimate) {
    const level = String(estimate?.label ?? "").trim() || "1";
    const tier = LN_TIER_BY_VARIANT[String(estimate?.variant ?? "")] ?? "mid";
    return `LN ${level} ${tier}`;
}

/**
 * aleju03 入口。返回与其它估算器同形的对象（star/estDiff/lnRatio/columnCount + 附加诊断）。
 */
export function runAleju03EstimatorFromText(osuText, options = {}, parsed = null) {
    const sunny = options.precomputedSunnyResult || runSunnyEstimatorFromText(osuText, options, parsed);
    const rate = Number.isFinite(Number(options.speedRate)) && Number(options.speedRate) > 0
        ? Number(options.speedRate)
        : 1;
    const fallback = {
        ...sunny,
        actualEstimatorAlgorithm: "Sunny",
        aleju03Ln: { applied: false, reason: "no-ln-verdict" },
    };

    const columnCount = Number(sunny?.columnCount);
    if (columnCount !== 4) {
        return { ...fallback, aleju03Ln: { applied: false, reason: "unsupported-keycount" } };
    }

    const cvtFlag = String(options.cvtFlag ?? "");
    const needsConvert = cvtFlag.includes("IN") || cvtFlag.includes("HO");
    let parsedData = null;
    try {
        const parser = resolveParser(osuText, parsed, needsConvert);
        if (needsConvert) {
            try {
                if (cvtFlag.includes("IN")) parser.modIN();
                if (cvtFlag.includes("HO")) parser.modHO();
            } catch {
                // 转换失败时保留原谱面（与其它估算器的转换容错一致）
            }
        }
        parsedData = parser.getParsedData();
    } catch {
        return { ...fallback, aleju03Ln: { applied: false, reason: "parse-failed" } };
    }

    const map = buildAlejuMap(parsedData);
    if (map.notes.length === 0) {
        return { ...fallback, aleju03Ln: { applied: false, reason: "no-notes" } };
    }

    // LN% = 0（谱面完全不含长条）：参考邻域模型对这类谱面没有语义，回归兜底会给出误导性的
    // 数字，因此显式返回 Unknown difficulty（选择本算法时 LN%=0 一律 Unknown）。
    const holdCount = map.notes.filter((note) => note.isHold).length;
    if (holdCount === 0) {
        return {
            ...sunny,
            estDiff: "Unknown difficulty",
            numericDifficulty: null,
            numericDifficultyHint: null,
            actualEstimatorAlgorithm: "aleju03",
            aleju03Ln: { applied: false, reason: "no-ln-content" },
        };
    }

    const starRating = Number(sunny?.star);
    let features = null;
    try {
        features = extractDanFeatures(map, { starRating, rate }, rate);
    } catch {
        return { ...fallback, aleju03Ln: { applied: false, reason: "feature-extraction-failed" } };
    }

    let estimate = null;
    try {
        estimate = estimateLnDan(
            map,
            { starRating, rate },
            features.metrics,
            Number.isFinite(starRating) ? starRating : 0,
            features.durationMs,
            rate,
            true,
            extractDanFeatures,
            true,
        );
    } catch {
        return { ...fallback, aleju03Ln: { applied: false, reason: "ln-estimate-failed" } };
    }

    if (!estimate) {
        return { ...fallback, aleju03Ln: { applied: false, reason: "not-an-ln-candidate" } };
    }

    const lnLabel = formatLnLabel(estimate);
    return {
        ...sunny,
        estDiff: lnLabel,
        numericDifficulty: null,
        numericDifficultyHint: null,
        actualEstimatorAlgorithm: "aleju03",
        aleju03Ln: {
            applied: true,
            reason: estimate.reason,
            rawDan: Number(estimate.rawDan),
            displayName: lnLabel,
            variant: estimate.variant ?? null,
            confidence: Number(estimate.confidence),
        },
    };
}
