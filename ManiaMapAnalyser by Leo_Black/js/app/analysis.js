import { runAnalysisPipeline } from "../pipeline/runAnalysisPipeline.js";
import { calculateReworkPp } from "../rework/reworkPerformance.js";
import { applyCompanellaToMixedResult } from "../estimator/mixedEstimator.js";
import { rcLabelToNumeric } from "../estimator/rcDifficultyFormat.js";
import { classifyCompanellaDifficulty } from "../estimator/companellaEstimator.js";
import { calculateInterludeStar } from "../interlude/index.js";
import { analyzePatternFromText } from "../patterns/service.js";
import { OsuFileParser } from "../parser/osuFileParser.js";
import { runInWorker } from "./worker/manager.js";
import {
    analyzeEtternaFromText,
    DEFAULT_SCORE_GOAL as ETT_DEFAULT_SCORE_GOAL,
} from "../ett/index.js";
import { PATTERNS_CONFIG } from "../patterns/config.js";
import {
    ettSkillBarsEl,
    getEndpoint,
    getActiveContentBar,
    contentBarShows,
    mainCardEl,
    patternClustersEl,
    ppBarsEl,
    reworkDiffEl,
    reworkMetaEl,
    reworkRightCapsuleEl,
    reworkStarEl,
    state,
    VIBRO_JACKSPEED_RATIO_THRESHOLD,
} from "./appContext.js";
import {
    formatDiffForDisplay,
    formatMetadataStatus,
    mergeDuplicateClusters,
    renderContentSkeleton,
    renderEtternaSkillBars,
    renderPatternClusters,
    renderReworkPpBars,
    clearReworkPpBody,
    renderRightCapsule,
    playStarBlockEntrance,
    setEstimateDifficultyText,
    showCategoryValue,
    showInterludeValue,
    showMsdValue,
    showNumericStarValue,
    show6KConstValue,
    showReworkPpValue,
    renderFullModeSeparators,
} from "./display.js";
import { getLatestPpValue, resetLivePp } from "./livePp.js";
import { modeTagFromLnRatio } from "./modeLogic.js";
import {
    hideOverlay,
    setModeTag,
    setModeTagAdvanced,
    setStatus,
    setSvTagVisible,
    showOverlay,
} from "./hud.js";
import {
    clearAllPauseMarkers,
    clearDiffGraph,
    renderDiffGraph,
    setForceHideNumericDifficulty,
    setNumericDifficultyValue,
    showDiffGraphError,
    setGraphLoading,
    updateDiffTextVisibility,
} from "./graph.js";
import {
    animateCardHeightTransition,
    currentEstimatorAlgorithm,
    isAutoDisplayEnabledNow,
    refreshAutoDisplayProfile,
    setEffectiveContentBarForMap,
} from "./settings.js";
import { scheduleRecompute } from "./scheduler.js";
import { detectVibro } from "./vibro.js";
import { resultCache, resultCacheGeneration } from "./resultCache.js";
import { trackTelemetryAnalyze } from "./telemetry.js";

function escapeHtml(value) {
    return String(value ?? "")
        .replace(/&/g, "&amp;")
        .replace(/</g, "&lt;")
        .replace(/>/g, "&gt;")
        .replace(/\"/g, "&quot;")
        .replace(/'/g, "&#39;");
}

function buildMetaError(errors) {
    const merged = (errors || [])
        .map((entry) => String(entry ?? "").trim())
        .filter((entry) => entry.length > 0)
        .join(" | ")
        .replace(/\s+/g, " ");

    if (!merged) {
        return "";
    }

    const clipped = merged.length > 180 ? `${merged.slice(0, 177)}...` : merged;
    return `${escapeHtml(clipped)}`;
}

function renderBodySectionError(section, message) {
    const safeMessage = escapeHtml(message || "Unknown error");
    if (section === "Pattern") {
        patternClustersEl.innerHTML = `
            <li class="cluster-item body-error">
                <div class="body-error-title">Pattern Analyze Failed</div>
                <div class="body-error-text">${safeMessage}</div>
            </li>
        `;
        return;
    }

    if (section === "ReworkPP") {
        ppBarsEl.innerHTML = `
            <li class="pp-item body-error">
                <div class="body-error-title">Rework PP Unavailable</div>
                <div class="body-error-text">${safeMessage}</div>
            </li>
        `;
        return;
    }

    ettSkillBarsEl.innerHTML = `
        <li class="ett-skill-item body-error">
            <div class="body-error-title">Etterna Analyze Failed</div>
            <div class="body-error-text">${safeMessage}</div>
        </li>
    `;
}

// max 模式 PP 主体：谱面侧 ppMetrics + mod 修正组装 5 行柱状图（Task 13）。
// ppMetrics 缺失/无效 → 软失败空态（不进 errors[]，与附属段语义一致）。
function buildReworkPpDisplay(ppMetrics, modCodes) {
    if (!ppMetrics || !Number.isFinite(Number(ppMetrics.totalNotes)) || Number(ppMetrics.totalNotes) <= 0) {
        renderBodySectionError("ReworkPP", "PP data unavailable");
        return;
    }
    const ppRes = calculateReworkPp({
        starRating: ppMetrics.star,
        variety: ppMetrics.variety,
        accScalar: ppMetrics.accScalar,
        totalNotes: ppMetrics.totalNotes,
        perfect: ppMetrics.totalNotes,
        great: 0,
        good: 0,
        ok: 0,
        meh: 0,
        miss: 0,
        noFail: modCodes.includes("NF"),
        easy: modCodes.includes("EZ"),
    });
    if (!ppRes) {
        renderBodySectionError("ReworkPP", "PP data unavailable");
        return;
    }
    const rows = [
        { key: "pp", label: "Max PP", value: ppRes.pp, min: 0, max: 1200, centered: false },
        { key: "proportion", label: "Proportion", value: ppRes.proportion, min: 0, max: 1, centered: false },
        { key: "acc", label: "Acc Multiplier", value: ppRes.accMultiplier, min: 0.87, max: 1.13, centered: true },
        { key: "variety", label: "Variety Multiplier", value: ppRes.varietyMultiplier, min: 0.945, max: 1.055, centered: true },
        { key: "length", label: "Length Multiplier", value: ppRes.lengthMultiplier, min: 0.9, max: 1.1, centered: true },
    ].map((row) => ({ ...row, value: Math.max(0, row.value) }));
    renderReworkPpBars({ mode: "max", rows });
}

function setLeftCapsuleUnitBadge(unitText) {
    if (!reworkStarEl) {
        return;
    }

    const normalized = typeof unitText === "string" ? unitText.trim() : "";
    if (!normalized) {
        reworkStarEl.classList.remove("has-unit");
        reworkStarEl.removeAttribute("data-unit");
        return;
    }

    reworkStarEl.classList.add("has-unit");
    reworkStarEl.setAttribute("data-unit", normalized);
}

function buildEtternaAnalyzeOptions(etternaVersion) {
    return {
        musicRate: state.speedRate,
        scoreGoal: ETT_DEFAULT_SCORE_GOAL,
        cvtFlag: state.cvtFlag,
        etternaVersion,
    };
}

// Vibro 检测的 MSD 基准：4K 固定 0.72.3（判定不随用户选择的 Etterna 版本
// 漂移）；非 4K 直接使用主结果（= 0.74.0 n-key，其 Overall/JackSpeed 才有
// 意义；0.72.3 对非 4K 输出全 0，无法用于判定）。主结果已是 0.72.3 时复用。
const VIBRO_MSD_VERSION = "0.72.3";

async function resolveVibroMsdValues(rawText, baseEttResult) {
    if (Number(baseEttResult?.keycount) !== 4
        || baseEttResult?.etternaVersion === VIBRO_MSD_VERSION) {
        return baseEttResult?.values ?? null;
    }
    try {
        const vibroEtt = await analyzeEtternaFromText(
            rawText,
            buildEtternaAnalyzeOptions(VIBRO_MSD_VERSION),
        );
        return vibroEtt?.values ?? null;
    } catch {
        return baseEttResult?.values ?? null; // 补算失败回退主结果
    }
}

const CARD_EXTEND_TRANSITION_FALLBACK_MS = 420;

function waitForMainCardResizeTransition() {
    if (!mainCardEl) {
        return Promise.resolve();
    }

    return new Promise((resolve) => {
        let settled = false;

        const finish = () => {
            if (settled) {
                return;
            }
            settled = true;
            mainCardEl.removeEventListener("transitionend", onTransitionEnd);
            clearTimeout(timeoutId);
            resolve();
        };

        const onTransitionEnd = (event) => {
            if (event.target !== mainCardEl) {
                return;
            }

            if (event.propertyName === "min-height"
                || event.propertyName === "height"
                || event.propertyName === "grid-template-rows") {
                finish();
            }
        };

        const timeoutId = setTimeout(finish, CARD_EXTEND_TRANSITION_FALLBACK_MS);
        mainCardEl.addEventListener("transitionend", onTransitionEnd);
    });
}

function shouldShowBodySkeletonDuringExpand(previousCardHeight, activeContentBar) {
    if (!mainCardEl || typeof window === "undefined") {
        return false;
    }

    if (activeContentBar !== "Pattern" && activeContentBar !== "Etterna" && activeContentBar !== "Full") {
        return false;
    }

    const currentHeight = Number(mainCardEl.getBoundingClientRect().height) || 0;
    const computedStyle = window.getComputedStyle(mainCardEl);
    const targetMinHeight = Number.parseFloat(computedStyle.minHeight) || 0;
    const baseline = Math.max(Number(previousCardHeight) || 0, currentHeight);

    return targetMinHeight > baseline + 1;
}

export function resetReworkDisplay() {
    state.actualEstimatorAlgorithm = state.estimatorAlgorithm;
    state.ppMetrics = null;
    clearReworkPpBody();
    resetLivePp();
    setNumericDifficultyValue(null);
    setForceHideNumericDifficulty(false);
    reworkStarEl.textContent = "-";
    reworkStarEl.classList.remove("category-mode");
    reworkDiffEl.textContent = "-";
    if (reworkRightCapsuleEl) {
        reworkRightCapsuleEl.textContent = "-";
        reworkRightCapsuleEl.classList.remove("category-mode", "numeric-mode", "high-contrast", "has-unit");
        reworkRightCapsuleEl.removeAttribute("data-unit");
        reworkRightCapsuleEl.style.backgroundColor = "rgba(38, 50, 84, 0.45)";
        reworkRightCapsuleEl.style.color = "#f6fbff";
        reworkRightCapsuleEl.style.textShadow = "none";
    }
    clearDiffGraph();
    clearAllPauseMarkers();
    setEffectiveContentBarForMap(null);
    if (state.diffText === "Graph" || contentBarShows("Graph")) {
        showDiffGraphError("Graph unavailable");
    }
    reworkMetaEl.innerHTML = "LN%: -<br/>Keys: -";
    setModeTag("Mix");
    setSvTagVisible(false);
    reworkMetaEl.classList.remove("loading");
    reworkStarEl.style.color = "#f6fbff";
    reworkStarEl.style.backgroundColor = "rgba(38, 50, 84, 0.45)";
    reworkStarEl.style.textShadow = "none";
    reworkStarEl.classList.remove("high-contrast");
    reworkStarEl.classList.remove("unit-badge-light");
    setLeftCapsuleUnitBadge("");
}

export async function fetchBeatmapFile(reason) {
    const requestSeq = (state.analysisRequestSeq || 0) + 1;
    state.analysisRequestSeq = requestSeq;
    const isStaleRequest = () => requestSeq !== state.analysisRequestSeq;
    const genAtStart = resultCacheGeneration();
    const analysisStartedAt = performance.now();
    const previousCardHeight = mainCardEl ? (Number(mainCardEl.getBoundingClientRect().height) || 0) : 0;

    // 取出 socket 层判定的本次变化类型并清空，避免之后纯改设置的 recompute
    // 误用上一次换歌的入场动画。换歌没拿到种类时（如初次加载）按换歌处理，
    // 其余无种类的重算（改设置等）按换难度的轻量过渡处理。
    let changeKind = state.pendingChangeKind;
    if (!changeKind) {
        changeKind = reason === "initial load" ? "song" : "difficulty";
    }
    state.pendingChangeKind = "";
    state.activeChangeKind = changeKind;
    let starBlockEntrancePlayed = false;
    const playStarBlockEntranceOnce = () => {
        if (starBlockEntrancePlayed) {
            return;
        }
        starBlockEntrancePlayed = true;
        playStarBlockEntrance(state.activeChangeKind);
    };

    setStatus(`Loading beatmap file (${reason})...`, "loading");
    hideOverlay();

    if (state.diffText === "Graph" || contentBarShows("Graph")) {
        setGraphLoading(true);
    } else {
        clearDiffGraph();
    }

    // 结果缓存：fetch 之前查缓存，覆盖检查（computed 需求）不匹配视为 miss。
    // graph 覆盖与估算器的 withGraph 条件一致（diffText=Graph 或主体显示 Graph）。
    // needComputed 用 fetch 前的保守值（尚未经过 setEffectiveContentBarForMap 的
    // 谱面级 override，contentBarShows 读的是上一张图的 effectiveContentBar），
    // 仅用于覆盖检查；实际 shows*/need* 在执行块内 override 之后重新计算。
    const needComputed = {
        pattern: contentBarShows("Pattern")
            || state.srText === "Pattern"
            || state.diffText === "Pattern"
            || state.useSvDetection
            || state.vibroDetection
            || isAutoDisplayEnabledNow(),
        ett: contentBarShows("Etterna")
            || state.srText === "MSD"
            || state.diffText === "MSD"
            || state.vibroDetection
            || (currentEstimatorAlgorithm() === "Companella" || currentEstimatorAlgorithm() === "Mixed"),
        graph: state.diffText === "Graph" || contentBarShows("Graph"),
        interlude: state.srText === "InterludeSR"
            || state.diffText === "InterludeSR"
            || currentEstimatorAlgorithm() === "Companella"
            || currentEstimatorAlgorithm() === "Mixed",
        pp: contentBarShows("ReworkPP") || state.srText === "ReworkPP",
    };
    // 缓存键加版本段：star 口径统一为 Sunny 原始 sr 后，旧快照（存的是 Azusa/Roxy 映射 star）必须失效。
    const CACHE_KEY_STAR_UNIFIED_VERSION = "star-v2";
    const cacheKey = `${CACHE_KEY_STAR_UNIFIED_VERSION}|${state.estimatorAlgorithm}|${state.lastBeatmapIdentity}|${state.modSignature}`;
    const isMetaDegraded = String(state.lastBeatmapIdentity || "").startsWith("meta:");
    let cached = null;
    if (state.enableResultCache && state.lastBeatmapIdentity) {
        const snapshot = resultCache.get(cacheKey);
        if (snapshot
            && snapshot.computed.graph === needComputed.graph
            && snapshot.computed.pattern === needComputed.pattern
            && snapshot.computed.ett === needComputed.ett
            && snapshot.computed.interlude === needComputed.interlude
            && snapshot.computed.pp === needComputed.pp) {
            cached = snapshot;
        }
    }

    try {
        let parsedInfo = null;
        let rawText = null;
        // 内容栏遵循用户设置：任何键数都不再强制降级为 Pattern。
        // （Graph 对任意键数均可渲染——star 序列为键数无关的 estimator 输出。）
        const applyContentBarOverride = (columnCount) => {
            setEffectiveContentBarForMap(null);
        };
        if (cached) {
            parsedInfo = cached.parsedInfo;
            applyContentBarOverride(parsedInfo.columnCount);
        } else {
            const response = await fetch(getEndpoint(), {
                method: "GET",
                cache: "no-store",
            });
            if (isStaleRequest()) return;

            if (!response.ok) {
                throw new Error(`Request failed with status ${response.status}`);
            }

            rawText = await response.text();
            if (isStaleRequest()) return;
            if (!rawText || !rawText.trim()) {
                throw new Error("Empty beatmap content.");
            }
            // parse-once：解析在 pipeline 内完成一次，parsedSummary 回传后（下方）才做 override。
        }

        // 估算分派 + 附属段（pattern/ett/interlude/Companella 二次 Ett）收敛为一次 pipeline
        // 调用（worker 单次往返或主线程同步回退）。pipeline 内部解析一次并共享给估算器/归一化/
        // SunnyWindow/Interlude（pattern 保留独立 patternOsuParser 解析、ett 保留自身解析），
        // 输出 parsedSummary 供 override 与渲染使用——主线程不再二次解析。
        // 逐段顺序与旧 analysis.js 完全一致（估算 → 归一化 → SunnyWindow → 派生 → Interlude →
        // Pattern → Ett → Companella）；渲染段与缓存键/写门不变。
        const errors = [];
        const estimatorAlgorithm = currentEstimatorAlgorithm();
        let pipelineResult = null;
        if (!cached) {
            try {
                const estimatorOptions = {
                    speedRate: state.speedRate,
                    odFlag: state.odFlag,
                    cvtFlag: state.cvtFlag,
                    // graph 需要与否用 fetch 前的保守值（needComputed.graph = diffText=Graph 或主体
                    // 显示 Graph）。override 只会把主体降级为 Pattern（不会引入 Graph），保守值 ⊇ 实际 showsGraph。
                    withGraph: needComputed.graph,
                    extendedEstimationRange: state.extendedEstimationRange,
                    enableAlwaysShowLNDifficulty: state.enableAlwaysShowLNDifficulty,
                };
                const pipelineOptions = {
                    ...estimatorOptions,
                    forceSunnyReferenceHo: state.azusaSunnyReferenceHo,
                    forceSunnyWindow: state.forceSunnyWindow,
                    enableAnalyzeLN: state.enableAnalyzeLN,
                    display6kLevel: state.display6kLevel,
                    // 附属段开关（needComputed 保守值，与缓存覆盖检查同源）：
                    withPattern: needComputed.pattern,
                    withEtterna: needComputed.ett,
                    withInterlude: needComputed.interlude,
                    withPpMetrics: needComputed.pp,
                    classicMod: state.classicMod === true,
                    etternaVersion: state.etternaVersion,
                    companellaEtternaVersion: state.companellaEtternaVersion,
                };
                const pipelineInput = { rawText, estimatorAlgorithm, options: pipelineOptions };
                const wp = runInWorker(pipelineInput);
                pipelineResult = wp ? await wp : await runAnalysisPipeline(pipelineInput);
                parsedInfo = pipelineResult.parsedSummary;
                applyContentBarOverride(parsedInfo.columnCount);
            } catch (error) {
                if (isStaleRequest()) return;
                resetReworkDisplay();
                if (state.diffText === "Graph" || contentBarShows("Graph")) {
                    showDiffGraphError("Graph unavailable");
                }
                errors.push(`Rework failed: ${error.message}`);
                // 失败路径回退：pipeline 抛错（估算器失败）未返回 parsedSummary——用最小元信息解析补齐，
                // 与旧代码 parseMetadataFromBeatmap 行为一致（正常路径保持 parse-once，不在此解析）。
                try {
                    const fallbackParser = new OsuFileParser(rawText);
                    fallbackParser.process();
                    const fallbackParsed = fallbackParser.getParsedData();
                    parsedInfo = {
                        metadata: fallbackParsed.metaData || {},
                        lnRatio: Number(fallbackParsed.lnRatio) || 0,
                        columnCount: Number(fallbackParsed.columnCount) || 0,
                    };
                    applyContentBarOverride(parsedInfo.columnCount);
                } catch (_) {
                    // 元信息解析也失败：降级为空元信息（避免下游渲染读 null；columnCount 0 → 无 override）。
                    parsedInfo = { metadata: {}, lnRatio: 0, columnCount: 0 };
                }
            }
        }

        // override 之后才计算 shows*/activeContentBar（恢复 main 顺序），
        // 供下方各计算块与渲染段使用；needComputed 仍用 fetch 前的保守值。
        const activeContentBar = getActiveContentBar();
        const showsPattern = contentBarShows("Pattern");
        const showsEtterna = contentBarShows("Etterna");
        const showsGraph = contentBarShows("Graph");
        const showsReworkPp = contentBarShows("ReworkPP");

        const shouldDelayBodyRender = shouldShowBodySkeletonDuringExpand(previousCardHeight, activeContentBar);
        let bodyRenderDelayPromise = null;
        if (shouldDelayBodyRender) {
            renderContentSkeleton();
            bodyRenderDelayPromise = waitForMainCardResizeTransition();
        }

        // 主体渲染前快照卡片高度（骨架路径下为骨架撑起的高度）。卡片高度现在是
        // height:auto（随内容生长），内容渲染后的高度变化是瞬时布局——渲染完成
        // 后用它补一次 height 过渡，恢复平滑伸长（与 setRuntimeContentBar 的
        // animateCardHeightTransition 用法一致）。
        const heightBeforeBodyRender = mainCardEl ? (Number(mainCardEl.getBoundingClientRect().height) || 0) : 0;

        const waitForBodyRenderReady = async () => {
            if (!bodyRenderDelayPromise) {
                return true;
            }
            await bodyRenderDelayPromise;
            if (isStaleRequest()) {
                return false;
            }
            bodyRenderDelayPromise = null;
            return true;
        };

        const autoDisplayEnabled = isAutoDisplayEnabledNow();

        let rework = null;
        let patternResult = null;
        let patternReport = null;
        let mergedClusters = null;
        let ettResult = null;
        let interludeStar = Number.NaN;
        let isVibroMap = false;
        let resolvedEstDiff = null;
        let resolvedNumericDifficulty = null;
        let resolvedNumericDifficultyHint = null;
        let resolvedMetaHtml = "LN%: -<br/>Keys: -";
        let typePercentageData = null;
        let pendingCompanellaEstimate = false;
        let pendingMixedCompanellaContext = null;
        let sixKConst = null;
        // vibro 星数门槛：pipeline 用归一化前的 star 判定（与旧 selectedRework?.star 意图一致）。
        let vibroEligible = false;

        const estimatorNeedsCompanellaData = estimatorAlgorithm === "Companella"
            || estimatorAlgorithm === "Mixed";

        const needVibroDetection = state.vibroDetection;
        const needPatternAnalysis = showsPattern
            || state.srText === "Pattern"
            || state.diffText === "Pattern"
            || state.useSvDetection
            || needVibroDetection
            || autoDisplayEnabled;
        const needMsdValue = state.srText === "MSD" || state.diffText === "MSD";
        const needInterludeValue = state.srText === "InterludeSR"
            || state.diffText === "InterludeSR"
            || estimatorNeedsCompanellaData;
        const needEtternaAnalysis = showsEtterna
            || needMsdValue
            || needVibroDetection
            || estimatorNeedsCompanellaData;
        const shouldReportEtternaError = showsEtterna
            || needMsdValue
            || estimatorNeedsCompanellaData;
        const shouldForceSunnyWindow = state.forceSunnyWindow;
        let lnStar = null;
        if (cached) {
            rework = cached.rework;
            state.actualEstimatorAlgorithm = cached.actualEstimatorAlgorithm;
            state.ppMetrics = cached.ppMetrics || null;
            resolvedEstDiff = cached.rework.estDiff;
            resolvedNumericDifficulty = cached.rework.numericDifficulty;
            resolvedNumericDifficultyHint = cached.rework.numericDifficultyHint;
            // 命中重派生：display6kLevel 已移出失效清单（toggle-diff 零输出契约差异，证据
            // task-13-settings.txt）。缓存的 sixKConst 反映写时刻的 display6kLevel——按
            // runAnalysisPipeline §6 同公式从缓存 star 重算（4K 恒 null；6K 下 rework.star
            // 恒为 Sunny sr，公式逐字一致），写关时置 null，写开时与缓存值逐位相同。
            const hitSunnySrc = Number(rework.star);
            sixKConst = state.display6kLevel && Number(rework.columnCount) === 6
                && Number.isFinite(hitSunnySrc) && hitSunnySrc > 0
                ? Math.round((hitSunnySrc * 200 / 81 + 7 / 6) * 100) / 100
                : null;
            lnStar = cached.rework.lnStar;
            state.lnStar = cached.rework.lnStar;
            typePercentageData = cached.rework.typePercentageData;
        } else if (pipelineResult) {
            rework = pipelineResult.rework;
            state.actualEstimatorAlgorithm = pipelineResult.actualEstimatorAlgorithm;
            state.ppMetrics = pipelineResult.ppMetrics || null;
            vibroEligible = pipelineResult.vibro.eligible;
            errors.push(...pipelineResult.errors);
            if (isStaleRequest()) return;

            resolvedEstDiff = pipelineResult.rework.estDiff;
            resolvedNumericDifficulty = pipelineResult.rework.numericDifficulty;
            resolvedNumericDifficultyHint = pipelineResult.rework.numericDifficultyHint;

            if (estimatorAlgorithm === "Companella") {
                pendingCompanellaEstimate = Number(pipelineResult.rework.columnCount) === 4;
            }
            if (estimatorAlgorithm === "Mixed") {
                pendingMixedCompanellaContext = pipelineResult.rework.mixedCompanellaPlan || null;
            }

            // 如果强制使用SunnyWindow，在这里替换LN部分
            const sunnyWindowRework = pipelineResult.sunnyWindow;
            if (sunnyWindowRework) {
                const sunnyWindowLNEstDiff = sunnyWindowRework.estDiff.split("||").map((part) => part.trim()).filter((part) => part.length > 0)[1];
                typePercentageData = sunnyWindowRework.typePercentageData;
                if (sunnyWindowLNEstDiff) {
                    resolvedEstDiff = resolvedEstDiff.split("||").map((part) => part.trim()).filter((part) => part.length > 0)[0] + " || " + sunnyWindowLNEstDiff;
                    if (pendingMixedCompanellaContext) {
                        pendingMixedCompanellaContext.lnDifficulty = sunnyWindowLNEstDiff;
                        pendingMixedCompanellaContext.lnRatio = 4e65;
                    }
                }
                lnStar = sunnyWindowRework.lnStar;
            }

            // 6K 定数: compute Sunny SR for constant rating display
            // 6K 谱面下 rework.star 恒为 Sunny sr（Daniel/Azusa/Roxy 非 4K 均回退 Sunny，
            // Mixed 走 sunnyBaseline，Companella 直接跑 Sunny，且 pipeline 已对未回退算法归一化），
            // pipeline 按同一公式计算；此处仅回写 sunnySR 状态字段（write-only）。
            sixKConst = pipelineResult.sixKConst;
            if (sixKConst !== null) {
                state.sunnySR = Number(rework.star);
            }

            state.lnStar = lnStar ?? (state.enableAlwaysShowLNDifficulty || Number(rework?.lnRatio ?? parsedInfo.lnRatio) > 0.15 ? rework?.star : 0) ?? 0;
        }

        // 拿到结果、即将首次写入 star 区块时再触发入场动画，
        // 与数值/难度名/图表的刷新同帧，换歌才整块入场，换难度只做轻量过渡。
        if (rework) {
            playStarBlockEntranceOnce();
            showNumericStarValue(rework.star);
            updateDiffTextVisibility();

            if (state.diffText === "Graph" || showsGraph) {
                // Graph 数据 = estimator 的 star 序列，对任意键数均可用
                // （Sunny 核心为键数无关算法）；渲染失败才提示。
                {
                    const ok = renderDiffGraph(rework.graph);
                    if (!ok) {
                        showDiffGraphError("Graph unavailable");
                    }
                }
            } else {
                clearDiffGraph();
            }

            const lnPercent = `${(rework.lnRatio * 100).toFixed(1)}%`;
            resolvedMetaHtml = `LN%: ${lnPercent}<br/>Keys: ${rework.columnCount}`;
            reworkMetaEl.innerHTML = resolvedMetaHtml;
            reworkMetaEl.classList.remove("loading");
        }

        if (needInterludeValue) {
            if (cached) {
                interludeStar = cached.interludeStar;
            } else if (pipelineResult?.interludeError != null || Number.isFinite(pipelineResult?.interludeStar)) {
                // pipeline 结果：成功取 star；失败记录错误（needInterludeValue 时旧代码无条件入 errors）。
                if (pipelineResult.interludeError != null) {
                    errors.push(`Interlude analyze failed: ${pipelineResult.interludeError}`);
                } else {
                    interludeStar = pipelineResult.interludeStar;
                }
            } else {
                // 回退：pipeline 估算失败或保守开关未覆盖（override 后 need* 变真）→ 主线程直接计算（旧路径）。
                try {
                    interludeStar = await calculateInterludeStar(rawText, state.speedRate, state.cvtFlag);
                    if (isStaleRequest()) return;
                } catch (error) {
                    errors.push(`Interlude analyze failed: ${error.message}`);
                }
            }
        }

        if (needPatternAnalysis) {
            let patternAnalysisError = null;
            // debugUseAmount 后处理：按 Amount 排序 + Category 覆盖。hit/miss 共用
            // （任务 13 提取；miss 的 applyPatternData 与 hit 的缓存重放都走这里）。
            const applyDebugUseAmountPostProcess = (clusters, report) => {
                if (!state.debugUseAmount) {
                    return clusters;
                }
                clusters.sort((a, b) => b.Amount - a.Amount);
                if (report && clusters.length > 0) {
                    const topSpecific = clusters[0]?.SpecificTypes?.[0];
                    if (topSpecific && Number(topSpecific[1]) > 0.05) {
                        report.Category = topSpecific[0];
                    } else {
                        report.Category = clusters[0].Pattern;
                    }
                }
                return clusters;
            };
            // 从 pattern 结果对象设置 patternReport/mergedClusters + debugUseAmount 后处理。
            const applyPatternData = (result) => {
                patternResult = result;
                patternReport = result?.report || null;
                const allClusters = result?.report?.Clusters || result?.topFiveClusters || [];
                mergedClusters = mergeDuplicateClusters(allClusters);
                applyDebugUseAmountPostProcess(mergedClusters, patternReport);
            };
            if (cached) {
                patternResult = cached.patternReport ? { report: cached.patternReport } : null;
                patternReport = cached.patternReport;
                // 命中重派生：缓存存的是写时刻的 mergedClusters（debugUseAmount 可能已变，
                // 已移出失效清单）——从缓存 report.Clusters 重放 merge + 后处理，与 miss 同路径。
                mergedClusters = mergeDuplicateClusters(
                    cached.patternReport?.Clusters || cached.mergedClusters || [],
                );
                applyDebugUseAmountPostProcess(mergedClusters, patternReport);
            } else if (pipelineResult?.patternReport || pipelineResult?.patternError) {
                if (pipelineResult.patternError) {
                    patternAnalysisError = new Error(pipelineResult.patternError);
                    errors.push(`Pattern analyze failed: ${pipelineResult.patternError}`);
                } else {
                    applyPatternData({
                        report: pipelineResult.patternReport,
                        topFiveClusters: pipelineResult.patternTopFiveClusters,
                    });
                }
            } else {
                // 回退：pipeline 估算失败或保守开关未覆盖 → 主线程直接计算（旧路径）。
                try {
                    applyPatternData(analyzePatternFromText(rawText));
                } catch (error) {
                    patternAnalysisError = error;
                    errors.push(`Pattern analyze failed: ${error.message}`);
                }
            }

            if (showsPattern) {
                if (!(await waitForBodyRenderReady())) return;
                if (patternAnalysisError) {
                    renderBodySectionError("Pattern", patternAnalysisError.message);
                } else {
                    renderPatternClusters(mergedClusters);
                }
            }
        } else {
            patternClustersEl.innerHTML = "";
        }

        if (needEtternaAnalysis) {
            let ettAnalysisError = null;
            if (cached) {
                ettResult = cached.ettResult;
                isVibroMap = cached.isVibroMap;
            } else if (pipelineResult?.ettResult || pipelineResult?.ettError) {
                if (pipelineResult.ettError) {
                    ettAnalysisError = new Error(pipelineResult.ettError);
                    const isKeycountError = /unsupported keycount/i.test(pipelineResult.ettError);
                    if (shouldReportEtternaError && !isKeycountError) {
                        errors.push(`Etterna analyze failed: ${pipelineResult.ettError}`);
                    }
                } else {
                    ettResult = pipelineResult.ettResult;
                    // 用算法自身 star 判定（pipeline 保留归一化前的 star），保持 vibro 检测既有行为不变。
                    // MSD 基准固定 0.72.3（VIBRO_MSD_VERSION）。
                    if (state.vibroDetection && vibroEligible) {
                        const vibroValues = await resolveVibroMsdValues(rawText, ettResult);
                        if (isStaleRequest()) return;
                        isVibroMap = detectVibro(vibroValues, VIBRO_JACKSPEED_RATIO_THRESHOLD);
                    }
                }
            } else {
                // 回退：pipeline 估算失败或保守开关未覆盖 → 主线程直接计算（旧路径）。
                try {
                    ettResult = await analyzeEtternaFromText(
                        rawText,
                        buildEtternaAnalyzeOptions(state.etternaVersion),
                    );
                    if (isStaleRequest()) return;

                    // MSD 基准固定 0.72.3（VIBRO_MSD_VERSION）。
                    if (state.vibroDetection && vibroEligible) {
                        const vibroValues = await resolveVibroMsdValues(rawText, ettResult);
                        if (isStaleRequest()) return;
                        isVibroMap = detectVibro(vibroValues, VIBRO_JACKSPEED_RATIO_THRESHOLD);
                    }
                } catch (error) {
                    ettAnalysisError = error;
                    const isKeycountError = /unsupported keycount/i.test(String(error?.message ?? ""));
                    if (shouldReportEtternaError && !isKeycountError) {
                        errors.push(`Etterna analyze failed: ${error.message}`);
                    }
                }
            }

            if (showsEtterna) {
                if (!(await waitForBodyRenderReady())) return;
                if (ettAnalysisError) {
                    const isKeycountError = /unsupported keycount/i.test(String(ettAnalysisError?.message ?? ""));
                    renderBodySectionError("Etterna", isKeycountError ? "Unsupported Keycount" : ettAnalysisError.message);
                    state.etternaTechnicalHidden = false;
                    mainCardEl.classList.remove("bars-etterna-compact");
                } else {
                    const columnCount = Number(rework?.columnCount) || Number(parsedInfo.columnCount) || 0;
                    renderEtternaSkillBars(ettResult?.values || {}, columnCount);
                }
            }
        } else {
            state.etternaTechnicalHidden = false;
            mainCardEl.classList.remove("bars-etterna-compact");
            ettSkillBarsEl.innerHTML = "";
        }

        // ReworkPP 主体：谱面侧 ppMetrics（miss 来自 pipelineResult、hit 来自快照）+
        // 当前 mod 修正，max 模式渲染。ppMetrics 缺失时渲染错误空态。
        if (showsReworkPp) {
            if (!(await waitForBodyRenderReady())) return;
            buildReworkPpDisplay(state.ppMetrics, state.modCodes || []);
        }

        if (rework) {
            const shouldRunCompanella = Number(rework.columnCount) === 4
                && (pendingCompanellaEstimate || pendingMixedCompanellaContext != null);

            if (shouldRunCompanella && !cached) {
                let companellaMsdValues = ettResult?.values;
                const companellaEtternaVersion = String(
                    state.companellaEtternaVersion || state.etternaVersion,
                ).trim() || state.etternaVersion;

                if (state.etternaVersion !== companellaEtternaVersion) {
                    if (pipelineResult?.companellaEttResult) {
                        // 二次 Ett 已在 pipeline（worker 一次往返）内完成。
                        companellaMsdValues = pipelineResult.companellaEttResult.values;
                    } else if (pipelineResult?.companellaEttError) {
                        // 与旧行为一致：失败仅 console.warn（不阻断，回退主 etternaVersion 的 values）。
                        console.warn(`Companella Etterna (${companellaEtternaVersion}) analyze failed: ${pipelineResult.companellaEttError}`);
                    } else {
                        // 回退：pipeline 未计算二次 Ett（估算失败等）→ 主线程直接计算（旧路径）。
                        try {
                            const forcedCompanellaEtterna = await analyzeEtternaFromText(
                                rawText,
                                buildEtternaAnalyzeOptions(companellaEtternaVersion),
                            );
                            if (isStaleRequest()) return;

                            companellaMsdValues = forcedCompanellaEtterna?.values;
                        } catch (error) {
                            console.warn(`Companella Etterna (${companellaEtternaVersion}) analyze failed: ${error.message}`);
                        }
                    }
                }

                try {
                    const companellaResult = await classifyCompanellaDifficulty({
                        msdValues: companellaMsdValues,
                        interludeStar,
                        sunnyStar: Number(rework.star),
                    });
                    if (isStaleRequest()) return;

                    if (pendingCompanellaEstimate) {
                        resolvedEstDiff = companellaResult.estDiff;
                        resolvedNumericDifficulty = companellaResult.numericDifficulty;
                        resolvedNumericDifficultyHint = companellaResult.numericDifficultyHint;
                    }

                    if (pendingMixedCompanellaContext) {
                        const mixedAfterCompanella = applyCompanellaToMixedResult({
                            estDiff: resolvedEstDiff,
                            numericDifficulty: resolvedNumericDifficulty,
                            numericDifficultyHint: resolvedNumericDifficultyHint,
                            mixedCompanellaPlan: pendingMixedCompanellaContext,
                        }, companellaResult);

                        resolvedEstDiff = mixedAfterCompanella.estDiff;
                        resolvedNumericDifficulty = mixedAfterCompanella.numericDifficulty;
                        resolvedNumericDifficultyHint = mixedAfterCompanella.numericDifficultyHint;
                        pendingMixedCompanellaContext = null;
                    }
                } catch (error) {
                    console.warn(`Companella estimate failed: ${error.message}`);
                }
            }

            // 写缓存：companella 完成后、SV/auto-profile 段之前。
            // 门控：miss && 开关开 && 全成功 && generation 未变（防 clear 后旧分析写回）。
            if (!cached && state.enableResultCache && state.lastBeatmapIdentity
                && errors.length === 0
                && rework && !isStaleRequest()
                && genAtStart === resultCacheGeneration()) {
                // clustering.js 的 cluster 对象带 format()/Importance 方法，
                // structuredClone 无法拷贝（resultCache 契约要求 JSON-safe），
                // 快照只存渲染所需的普通字段。
                const jsonSafe = (value) => (value == null ? value : JSON.parse(JSON.stringify(value)));
                resultCache.put(cacheKey, {
                    rework: {
                        star: rework.star,
                        estDiff: resolvedEstDiff,
                        numericDifficulty: resolvedNumericDifficulty,
                        numericDifficultyHint: resolvedNumericDifficultyHint,
                        graph: rework.graph,
                        lnRatio: rework.lnRatio,
                        columnCount: rework.columnCount,
                        lnStar: lnStar,
                        typePercentageData: jsonSafe(typePercentageData)
                    },
                    patternReport: jsonSafe(patternReport),
                    mergedClusters: jsonSafe(mergedClusters),
                    ettResult,
                    interludeStar,
                    isVibroMap,
                    sixKConst,
                    ppMetrics: pipelineResult.ppMetrics || null,
                    actualEstimatorAlgorithm: state.actualEstimatorAlgorithm,
                    parsedInfo: {
                        metadata: parsedInfo.metadata,
                        lnRatio: parsedInfo.lnRatio,
                        columnCount: parsedInfo.columnCount,
                    },
                    computed: needComputed,
                }, { skip: isMetaDegraded });
            }

            if (Number.isFinite(resolvedNumericDifficulty) && resolvedNumericDifficulty >= 18.5) {
                if (resolvedEstDiff) {
                    const strList = resolvedEstDiff.split("||");
                    strList[0] = "> Cloverwisp Theta high";
                    setEstimateDifficultyText(formatDiffForDisplay(strList.join("||")));
                }
                else {
                    setEstimateDifficultyText("> Cloverwisp Theta high");
                }
            }
            else setEstimateDifficultyText(formatDiffForDisplay(resolvedEstDiff));
        }

        const fallbackModeTag = modeTagFromLnRatio(Number(rework?.lnRatio ?? parsedInfo.lnRatio));
        let resolvedModeTag = (activeContentBar === "None")
            ? fallbackModeTag
            : (patternResult?.report?.ModeTag || fallbackModeTag);
        let shouldShowSvTag = false;

        if (state.useSvDetection) {
            const svAmount = Number(patternReport?.SVAmount);
            if (Number.isFinite(svAmount) && svAmount >= PATTERNS_CONFIG.SV_AMOUNT_THRESHOLD) {
                shouldShowSvTag = true;
                if (patternReport && typeof patternReport === "object") {
                    patternReport.Category = "SV";
                }
            }
        }

        if (typePercentageData) {
            const lnRatio = Number(rework?.lnRatio ?? parsedInfo.lnRatio)
            setModeTagAdvanced(typePercentageData, lnRatio);
        } else {
            setModeTag(resolvedModeTag);
        }
        setSvTagVisible(shouldShowSvTag);

        if (rework) {
            const cappedDiff = Number.isFinite(resolvedNumericDifficulty) && resolvedNumericDifficulty >= 18.5
                ? null
                : resolvedNumericDifficulty;
            const cappedHint = cappedDiff === null ? "N/A" : resolvedNumericDifficultyHint;
            setNumericDifficultyValue(cappedDiff, cappedHint);
        }

        setForceHideNumericDifficulty(isVibroMap);

        if (autoDisplayEnabled) {
            const beforeContent = state.contentBar;
            const beforeSrText = state.srText;
            const profileChanged = refreshAutoDisplayProfile(resolvedModeTag);

            const missingEtterna = (
                showsEtterna
                || state.srText === "MSD"
                || state.diffText === "MSD"
            ) && !needEtternaAnalysis;
            const missingPattern = (
                showsPattern
                || state.srText === "Pattern"
                || state.diffText === "Pattern"
                || state.useSvDetection
            ) && !needPatternAnalysis;

            if (profileChanged && ((missingEtterna || missingPattern)
                || state.contentBar !== beforeContent
                || state.srText !== beforeSrText)) {
                scheduleRecompute("auto profile switched", false);
                return;
            }
        }

        let leftCapsuleUnit = "";

        // PP capsule fixed width: tabular-nums + measured width kills horizontal flicker.
        // Toggling the class off (SR/MSD) restores auto width.
        reworkStarEl.classList.toggle("pp-capsule", state.srText === "ReworkPP");

        // 6K 定数: force override left capsule when enabled and map is 6K —
        // but never override an explicit ReworkPP srText selection (PP wins).
        if (sixKConst !== null && state.srText !== "ReworkPP") {
            show6KConstValue(sixKConst);
            leftCapsuleUnit = "LV";
        } else if (state.srText === "Pattern") {
            if (rework) {
                showCategoryValue(patternReport?.Category || "-");
            }
        } else if (state.srText === "InterludeSR") {
            if (Number.isFinite(interludeStar)) {
                showInterludeValue(interludeStar);
                leftCapsuleUnit = "ISR";
            } else if (rework) {
                showNumericStarValue(rework.star);
                leftCapsuleUnit = "SR";
            }
        } else if (state.srText === "MSD") {
            const overallValue = Number(ettResult?.values?.Overall);
            if (Number.isFinite(overallValue)) {
                showMsdValue(overallValue);
                leftCapsuleUnit = "MSD";
            } else if (rework) {
                showNumericStarValue(rework.star);
                leftCapsuleUnit = "SR";
            }
        } else if (state.srText === "ReworkPP") {
            let ppVal = getLatestPpValue();   // from livePp.js (max PP when idle, live PP in play)
            // livePp 的 cheap guard 在 max 模式下（选图/菜单）可能因 counts 不变而短路，
            // latestPpValue 滞留旧图值 — ppMetrics 存在时直接计算 max PP，保证换图刷新。
            if ((ppVal == null || !Number.isFinite(ppVal)) && state.ppMetrics) {
                const ppRes = calculateReworkPp({
                    starRating: state.ppMetrics.star,
                    variety: state.ppMetrics.variety,
                    accScalar: state.ppMetrics.accScalar,
                    totalNotes: state.ppMetrics.totalNotes,
                    perfect: state.ppMetrics.totalNotes, great: 0, good: 0, ok: 0, meh: 0, miss: 0,
                    noFail: state.modCodes.includes("NF"),
                    easy: state.modCodes.includes("EZ"),
                });
                ppVal = ppRes ? ppRes.pp : null;
            }
            if (ppVal != null && Number.isFinite(ppVal)) {
                showReworkPpValue(ppVal);
                leftCapsuleUnit = "PP";
            } else if (rework) {
                showNumericStarValue(rework.star);   // ppMetrics 缺失 fallback
                leftCapsuleUnit = "SR";
            }
        } else if (rework) {
            showNumericStarValue(rework.star);
            if (state.srText === "ReworkSR") {
                leftCapsuleUnit = "SR";
            }
        }

        setLeftCapsuleUnitBadge(leftCapsuleUnit);

        renderRightCapsule(
            state.diffText,
            Number(rework?.star),
            patternReport?.Category || "-",
            Number(ettResult?.values?.Overall),
            Number(interludeStar),
        );

        const overallValue = Number(ettResult?.values?.Overall);
        renderFullModeSeparators(overallValue);

        if (isVibroMap && state.diffText === "Difficulty") {
            setEstimateDifficultyText("VIBRO");
        }

        // 主体渲染完成：auto 高度下内容撑开是瞬时的，这里从渲染前高度补过渡动画。
        // 放在 refreshAutoDisplayProfile 之后，避免 profile 切换已触发的动画被打断重播。
        animateCardHeightTransition(heightBeforeBodyRender);

        const metadataLine = formatMetadataStatus(parsedInfo.metadata);
        const metadataErrors = errors.filter((entry) => {
            const text = String(entry ?? "").trim().toLowerCase();
            return !text.startsWith("companella ");
        });

        reworkMetaEl.innerHTML = resolvedMetaHtml;

        if (metadataErrors.length > 0) {
            const errorText = buildMetaError(metadataErrors);
            setStatus(`[Error] ${errorText}`, "error");
            hideOverlay();
        } else {
            setStatus(metadataLine, "ok");
            hideOverlay();
        }

        if (rework && metadataErrors.length === 0 && !isStaleRequest()) {
            // 数值化难度（Reform 段位体系，.0 = mid）：Azusa/Roxy 的原生值已是
            // 标准尺度，直接用；其余算法（Sunny/Companella/Daniel——Daniel 的
            // 原生值是其 DP 尺度，比标准约高 0.5）用 estDiff 标签反向换算。
            // 边界标签（< Alpha Low 等）反解为 null → 不上报该字段。
            const rcNative = state.actualEstimatorAlgorithm === "Azusa" || state.actualEstimatorAlgorithm === "Roxy";
            const numericDifficulty = rcNative
                && typeof resolvedNumericDifficulty === "number"
                && Number.isFinite(resolvedNumericDifficulty)
                ? resolvedNumericDifficulty
                : rcLabelToNumeric(resolvedEstDiff);
            const payload = {
                algorithm: state.estimatorAlgorithm,
                actualAlgorithm: state.actualEstimatorAlgorithm,
                keycount: Number(rework.columnCount),
                mods: state.modCodes || [],
                speedRate: Number(state.speedRate) || 1,
                mode: shouldShowSvTag ? "SV" : state.currentModeTag,
                star: Number(rework.star),
                lnRatio: Number(rework.lnRatio ?? parsedInfo.lnRatio),
                typeBreakdown: typePercentageData ?? null,
                durationMs: Math.max(0, Math.round(performance.now() - analysisStartedAt)),
            };
            if (Number.isFinite(numericDifficulty)) {
                payload.numericDifficulty = numericDifficulty;
            }
            trackTelemetryAnalyze(payload);
        }
    } catch (error) {
        if (isStaleRequest()) return;
        setStatus(`Failed to load beatmap file: ${error.message}`, "error");
        resetReworkDisplay();
        patternClustersEl.innerHTML = contentBarShows("Pattern")
            ? "<li class=\"cluster-item empty\">No data</li>"
            : "";
        ettSkillBarsEl.innerHTML = contentBarShows("Etterna")
            ? "<li class=\"ett-skill-item empty\">No data</li>"
            : "";
        ppBarsEl.innerHTML = contentBarShows("ReworkPP")
            ? "<li class=\"pp-item empty\">No data</li>"
            : "";
        showOverlay({
            title: "Load failed",
            message: String(error.message || "Unknown error"),
            isError: true,
            showSpinner: false,
        });
    } finally {
        if (isStaleRequest()) return;
        reworkMetaEl.classList.remove("loading");
    }
}
