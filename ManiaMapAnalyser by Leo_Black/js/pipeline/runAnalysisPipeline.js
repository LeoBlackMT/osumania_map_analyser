// js/pipeline/runAnalysisPipeline.js
// 分析管线纯函数：解析 → 估算分派 → 归一化 → SunnyWindow → 派生值 → Interlude → Pattern → Ett → Companella 二次 Ett。
// 一次调用（worker 单次往返或主线程同步回退）返回全部产物，渲染/缓存/主线程专属逻辑由调用方负责。
//
// 共享模块约束（AGENTS.md）：DOM-free / state-free / JSON-safe，纯函数，Node 与浏览器一致。
// 逐段顺序与 analysis.js 旧估算分派 + 附属段顺序一致（估算 → 归一化 → SunnyWindow → 派生 → Interlude → Pattern → Ett → Companella）；
// 渲染段与缓存逻辑由调用方（analysis.js）负责，本模块不接触。
//
// 错误通道：
//   - 估算器 / SunnyWindow 抛错直接向上传播（与旧行为一致——analysis.js 外层 catch 格式化
//     "Rework failed: ..." 并 resetReworkDisplay；harness 按阶段 catch 记录）。
//   - 附属段（pattern/ett/interlude/companella-ett）各自 try/catch 软失败：字段置空/NaN，
//     错误文本放独立 xxxError 字段（不并入 errors[]）——旧代码的 errors.push 带展示条件
//     （shouldReportEtternaError / isKeycountError 过滤等）依赖调用方主线程状态，
//     由 analysis.js 按旧条件决定是否并入，保证逐字一致。errors[] 恒为空，保留为硬错误预留。

import { OsuFileParser } from "../parser/osuFileParser.js";
import { runSunnyEstimatorFromText } from "../estimator/sunnyEstimator.js";
import { runSunnyWindowEstimatorFromText } from "../estimator/sunnyWindowEstimator.js";
import { runDanielEstimatorFromText } from "../estimator/danielEstimator.js";
import { runAzusaEstimatorFromText } from "../estimator/azusaEstimator.js";
import { runRoxyEstimatorFromText } from "../estimator/roxyEstimator.js";
import { runMixedEstimatorFromText } from "../estimator/mixedEstimator.js";
import { analyzePatternFromText } from "../patterns/service.js";
import { analyzeEtternaFromText, DEFAULT_SCORE_GOAL as ETT_DEFAULT_SCORE_GOAL } from "../ett/index.js";
import { calculateInterludeStar } from "../interlude/index.js";

const NORMALIZATION_ALGORITHMS = new Set(["Azusa", "Roxy", "Mixed"]);

// Sunny 结果的 numericDifficulty 恒为 null；Azusa/Roxy 无效回退后的 Sunny 结果同样如此。
// 允许 null 通过可避免主线程冗余重算（与 analysis.js 旧 isValidEstimatorResult 一致）。
// 对 Azusa/Roxy 自身结果行为与 worker 严格版一致：错误结果 star 为 NaN（两边都判无效），
// 成功结果 numericDifficulty 恒为有限值（两边都判有效）。
function isValidResult(result) {
    return Boolean(result)
        && Number.isFinite(result.star)
        && (Number.isFinite(result.numericDifficulty) || result.numericDifficulty === null)
        && typeof result.estDiff === "string";
}

// Pattern report 的 cluster 对象带 format()/Importance getter（patterns/clustering.js:112-131），
// structuredClone / JSON 均无法直接传输（方法触发 DataCloneError）。pipeline 只输出 analysis.js
// 消费的纯数据子集（Category/SVAmount/ModeTag/Clusters/topFiveClusters），getter 求值结果
// （Importance）不输出——analysis.js 消费侧（mergeDuplicateClusters/render）只读数据字段。
function sanitizePatternResult(result) {
    const report = result && typeof result === "object" ? result.report : null;
    if (!report || typeof report !== "object") {
        return { report: null, topFiveClusters: [] };
    }
    const plainClusters = (Array.isArray(report.Clusters) ? report.Clusters : []).map((c) => ({
        Pattern: c?.Pattern ?? null,
        SpecificTypes: c?.SpecificTypes ?? [],
        RatingMultiplier: c?.RatingMultiplier ?? null,
        BPM: c?.BPM ?? null,
        Mixed: c?.Mixed ?? null,
        Amount: c?.Amount ?? null,
    }));
    return {
        report: {
            Clusters: plainClusters,
            Category: report.Category ?? null,
            LNPercent: report.LNPercent ?? null,
            HBRowRatio: report.HBRowRatio ?? null,
            ModeTag: report.ModeTag ?? null,
            SVAmount: report.SVAmount ?? null,
            Duration: report.Duration ?? null,
        },
        topFiveClusters: plainClusters.slice(0, 5),
    };
}

/**
 * 运行完整分析管线（异步：ett WASM 与 interlude 均为异步）。
 *
 * @param {object} input
 * @param {string} input.rawText  .osu 文件文本
 * @param {string} input.estimatorAlgorithm  Sunny / Daniel / Azusa / Roxy / Mixed / Companella
 * @param {object} input.options  显式传入的估算选项（禁止读 state）：speedRate, odFlag, cvtFlag,
 *   withGraph, forceSunnyReferenceHo, forceSunnyWindow, enableAnalyzeLN,
 *   enableAlwaysShowLNDifficulty, display6kLevel, extendedEstimationRange,
 *   withPattern, withEtterna, withInterlude, etternaVersion, companellaEtternaVersion
 * @param {object} [input.parsed]  可选：已 process() 的 OsuFileParser 实例（Node/harness 复用解析，
 *   浏览器 worker 不传——worker 内自行解析一次）
 * @returns {Promise<{ rework: object, actualEstimatorAlgorithm: string, sunnyStar: number|null,
 *   sunnyWindow: object|null, sixKConst: number|null,
 *   vibro: { star: number, eligible: boolean },
 *   parsedSummary: { metadata: object, lnRatio: number, columnCount: number },
 *   patternReport: object|null, patternTopFiveClusters: Array|null, patternError: string|null,
 *   ettResult: object|null, ettError: string|null,
 *   interludeStar: number, interludeError: string|null,
 *   companellaEttResult: object|null, companellaEttError: string|null,
 *   errors: string[] }>}
 */
export async function runAnalysisPipeline({ rawText, estimatorAlgorithm, options = {}, parsed = null }) {
    // 1. 解析一次（可选复用外部已解析实例），估算器/归一化/SunnyWindow/Interlude 共享同一实例。
    const parser = (parsed && typeof parsed?.getParsedData === "function")
        ? parsed
        : new OsuFileParser(rawText);
    if (!parsed || !parsed?.process) {
        parser.process();
    }
    const parsedData = parser.getParsedData();
    const parsedSummary = {
        metadata: parsedData.metaData || {},
        lnRatio: Number(parsedData.lnRatio) || 0,
        columnCount: Number(parsedData.columnCount) || 0,
    };

    // 归一化星数复用决策（决策表见 .omo/evidence/task-11-pipeline.txt）：
    // 仅当算法内部 Sunny 与归一化调用（runSunnyEstimatorFromText(rawText, options)）
    // 使用相同 options + 相同文本时可复用；通过 precomputedSunnyResult 把同一份 Sunny
    // 结果喂给估算器，并直接取其 star 完成归一化（避免多跑一次 Sunny）：
    //   - Mixed：sunnyBaseline 用原 options（含 withGraph）→ 一致 → 复用
    //   - Azusa(forceSunnyReferenceHo=false)：内部 sunnyOptions = options → 一致 → 复用
    //   - Azusa(forceSunnyReferenceHo=true)：内部 sunnyOptions = {...options, cvtFlag:"HO"}
    //     → cvtFlag 不一致 → 独立计算（且不可传 precomputedSunnyResult，否则数值语义改变）
    //   - Roxy：内部 reference sunny 用 canonicalizeOsuTiming 改写后的 analysisText
    //     （即使 speedRate=1 也做常数平移），且 metaOptions 硬编码 precomputedSunnyResult:null
    //     → 无法证明一致 → 独立计算
    const needsNormalization = NORMALIZATION_ALGORITHMS.has(estimatorAlgorithm);
    const canReuseSunny = needsNormalization
        && (estimatorAlgorithm === "Mixed"
            || (estimatorAlgorithm === "Azusa" && options.forceSunnyReferenceHo === false));
    let sharedSunnyResult = null;
    if (canReuseSunny) {
        sharedSunnyResult = runSunnyEstimatorFromText(rawText, options, parser);
    }

    // 2. 估算分派（白名单与 Azusa/Roxy 无效回退 Sunny 语义同 compute.worker.js:17-54）。
    let selectedRework = null;
    let actualEstimatorAlgorithm = estimatorAlgorithm;

    if (estimatorAlgorithm === "Daniel") {
        selectedRework = runDanielEstimatorFromText(rawText, options, parser);
    } else if (estimatorAlgorithm === "Azusa") {
        const azusaOpts = {
            ...options,
            forceSunnyReferenceHo: options.forceSunnyReferenceHo !== false,
            ...(sharedSunnyResult ? { precomputedSunnyResult: sharedSunnyResult } : {}),
        };
        selectedRework = runAzusaEstimatorFromText(rawText, azusaOpts, parser);
        actualEstimatorAlgorithm = selectedRework?.actualEstimatorAlgorithm || actualEstimatorAlgorithm;
        if (!isValidResult(selectedRework)) {
            // 回退 Sunny：可复用场景直接复用同一份结果（逐位一致），否则独立计算。
            selectedRework = sharedSunnyResult || runSunnyEstimatorFromText(rawText, options, parser);
            actualEstimatorAlgorithm = "Sunny";
        }
    } else if (estimatorAlgorithm === "Roxy") {
        selectedRework = runRoxyEstimatorFromText(rawText, options, parser);
        actualEstimatorAlgorithm = selectedRework?.actualEstimatorAlgorithm || actualEstimatorAlgorithm;
        if (!isValidResult(selectedRework)) {
            selectedRework = runSunnyEstimatorFromText(rawText, options, parser);
            actualEstimatorAlgorithm = "Sunny";
        }
    } else if (estimatorAlgorithm === "Mixed") {
        const mixedOpts = sharedSunnyResult
            ? { ...options, precomputedSunnyResult: sharedSunnyResult }
            : options;
        selectedRework = runMixedEstimatorFromText(rawText, mixedOpts, parser);
    } else if (estimatorAlgorithm === "Companella") {
        // Companella 本体在主线程异步追加（§7.5），此处跑 Sunny 打底。
        selectedRework = runSunnyEstimatorFromText(rawText, options, parser);
    } else {
        // Sunny / 其他
        selectedRework = runSunnyEstimatorFromText(rawText, options, parser);
        actualEstimatorAlgorithm = "Sunny";
    }

    // 3. vibro 判定输入：归一化前的 star（与 analysis.js 旧 selectedRework?.star 顺序一致）。
    //    实际 isVibroMap 判定（detectVibro，浏览器专属）留在主线程，等 ettResult 就绪后执行。
    const vibroStar = Number(selectedRework?.star);
    const vibro = {
        star: vibroStar,
        eligible: Number.isFinite(vibroStar) && vibroStar > 5.0,
    };

    // 4. 归一化：Azusa/Roxy/Mixed 未回退时 star 统一为 Sunny 原始 sr（星数胶囊恒显 Sunny 口径；
    //    Daniel 独立算法排除；Companella/Sunny 本就是 Sunny sr）。
    let rework = selectedRework;
    let sunnyStar = null;
    if (NORMALIZATION_ALGORITHMS.has(actualEstimatorAlgorithm)) {
        const sunnyResult = sharedSunnyResult || runSunnyEstimatorFromText(rawText, options, parser);
        sunnyStar = Number(sunnyResult.star);
        rework = { ...selectedRework, star: sunnyStar };
    }

    // 5. SunnyWindow（forceSunnyWindow）：calculateSunny + calculateLN（带 parsed + enableAnalyzeLN）。
    let sunnyWindow = null;
    if (options.forceSunnyWindow) {
        sunnyWindow = runSunnyWindowEstimatorFromText(
            rawText,
            { ...options, enableAnalyzeLN: options.enableAnalyzeLN === true },
            parser,
        );
    }

    // 6. 6K 定数：display6kLevel && 6K 时按归一化后 star（6K 下恒为 Sunny sr）换算，2 位小数。
    let sixKConst = null;
    if (options.display6kLevel && parsedSummary.columnCount === 6) {
        const sunnySrc = Number(rework.star);
        if (Number.isFinite(sunnySrc) && sunnySrc > 0) {
            sixKConst = sunnySrc * 200 / 81 + 7 / 6;
            sixKConst = Math.round(sixKConst * 100) / 100;
        }
    }

    // 7-10. 附属段（analysis.js:606-738 并入）。顺序与旧主线程执行顺序一致：Interlude → Pattern
    //    → Ett → Companella 二次 Ett。各自 try/catch 软失败，字段置空/NaN，错误文本独立记录。

    // 7. Interlude（吃 sharedParsed——chartBuilder.buildInterludeRows 支持已解析 parser 实例）。
    let interludeStar = Number.NaN;
    let interludeError = null;
    if (options.withInterlude) {
        try {
            interludeStar = await calculateInterludeStar(parser, options.speedRate ?? 1, options.cvtFlag);
        } catch (err) {
            interludeError = String(err?.message || err);
        }
    }

    // 8. Pattern（保留 patternOsuParser 独立解析，不改语义；输出仅含纯数据子集）。
    let patternReport = null;
    let patternTopFiveClusters = null;
    let patternError = null;
    if (options.withPattern) {
        try {
            const sanitized = sanitizePatternResult(analyzePatternFromText(rawText));
            patternReport = sanitized.report;
            patternTopFiveClusters = sanitized.topFiveClusters;
        } catch (err) {
            patternError = String(err?.message || err);
        }
    }

    // 9. Ett（WASM，worker 内用 calc.js 现有 loader；import.meta.url 按模块文件解析，worker 同源可 fetch）。
    let ettResult = null;
    let ettError = null;
    if (options.withEtterna) {
        try {
            ettResult = await analyzeEtternaFromText(rawText, {
                musicRate: options.speedRate ?? 1,
                scoreGoal: ETT_DEFAULT_SCORE_GOAL,
                cvtFlag: options.cvtFlag,
                etternaVersion: options.etternaVersion,
            });
        } catch (err) {
            ettError = String(err?.message || err);
        }
    }

    // 10. Companella 二次 Ett：Companella/Mixed && 4K && companellaEtternaVersion 与主版本不同时
    //     在同一次 pipeline 内完成（一次往返）。失败不阻断（旧行为 console.warn + 回退主 ett values）。
    let companellaEttResult = null;
    let companellaEttError = null;
    const needsCompanellaData = estimatorAlgorithm === "Companella" || estimatorAlgorithm === "Mixed";
    const resolvedCompanellaEtternaVersion = String(options.companellaEtternaVersion || "").trim()
        || options.etternaVersion;
    if (needsCompanellaData
        && parsedSummary.columnCount === 4
        && options.etternaVersion
        && resolvedCompanellaEtternaVersion !== options.etternaVersion) {
        try {
            companellaEttResult = await analyzeEtternaFromText(rawText, {
                musicRate: options.speedRate ?? 1,
                scoreGoal: ETT_DEFAULT_SCORE_GOAL,
                cvtFlag: options.cvtFlag,
                etternaVersion: resolvedCompanellaEtternaVersion,
            });
        } catch (err) {
            companellaEttError = String(err?.message || err);
        }
    }

    return {
        rework,
        actualEstimatorAlgorithm,
        sunnyStar,
        sunnyWindow,
        sixKConst,
        vibro,
        parsedSummary,
        patternReport,
        patternTopFiveClusters,
        patternError,
        ettResult,
        ettError,
        interludeStar,
        interludeError,
        companellaEttResult,
        companellaEttError,
        errors: [],
    };
}
