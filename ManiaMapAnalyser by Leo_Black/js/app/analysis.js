import { runSunnyEstimatorFromText } from "../estimator/sunnyEstimator.js";
import { runSunnyWindowEstimatorFromText } from "../estimator/sunnyWindowEstimator.js"
import { runDanielEstimatorFromText } from "../estimator/danielEstimator.js";
import { runAzusaEstimatorFromText } from "../estimator/azusaEstimator.js";
import { runRoxyEstimatorFromText } from "../estimator/roxyEstimator.js";
import {
    applyCompanellaToMixedResult,
    runMixedEstimatorFromText,
} from "../estimator/mixedEstimator.js";
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
    GRAPH_SUPPORTED_KEY_SET,
    mainCardEl,
    patternClustersEl,
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
    renderRightCapsule,
    playStarBlockEntrance,
    setEstimateDifficultyText,
    showCategoryValue,
    showInterludeValue,
    showMsdValue,
    showNumericStarValue,
    renderFullModeSeparators,
} from "./display.js";
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
    currentEstimatorAlgorithm,
    isAutoDisplayEnabledNow,
    refreshAutoDisplayProfile,
    setEffectiveContentBarForMap,
} from "./settings.js";
import { scheduleRecompute } from "./scheduler.js";
import { detectVibro } from "./vibro.js";

function parseMetadataFromBeatmap(osuText) {
    const parser = new OsuFileParser(osuText);
    parser.process();
    const parsed = parser.getParsedData();
    return {
        metadata: parsed.metaData || {},
        lnRatio: Number(parsed.lnRatio) || 0,
        columnCount: Number(parsed.columnCount) || 0,
    };
}

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

    ettSkillBarsEl.innerHTML = `
        <li class="ett-skill-item body-error">
            <div class="body-error-title">Etterna Analyze Failed</div>
            <div class="body-error-text">${safeMessage}</div>
        </li>
    `;
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

    try {
        const response = await fetch(getEndpoint(), {
            method: "GET",
            cache: "no-store",
        });
        if (isStaleRequest()) return;

        if (!response.ok) {
            throw new Error(`Request failed with status ${response.status}`);
        }

        const rawText = await response.text();
        if (isStaleRequest()) return;
        if (!rawText || !rawText.trim()) {
            throw new Error("Empty beatmap content.");
        }

        const parsedInfo = parseMetadataFromBeatmap(rawText);
        const parsedKeycount = Number(parsedInfo.columnCount) || 0;
        // In Full mode the graph block shows its own "Unsupported Keys" notice,
        // so don't collapse the whole body to Pattern on unsupported keycounts.
        const shouldFallbackBodyToPattern = parsedKeycount > 0
            && !GRAPH_SUPPORTED_KEY_SET.has(parsedKeycount)
            && state.contentBar !== "None"
            && state.contentBar !== "Full";
        setEffectiveContentBarForMap(shouldFallbackBodyToPattern ? "Pattern" : null);
        const activeContentBar = getActiveContentBar();
        const showsPattern = contentBarShows("Pattern");
        const showsEtterna = contentBarShows("Etterna");
        const showsGraph = contentBarShows("Graph");

        const shouldDelayBodyRender = shouldShowBodySkeletonDuringExpand(previousCardHeight, activeContentBar);
        let bodyRenderDelayPromise = null;
        if (shouldDelayBodyRender) {
            renderContentSkeleton();
            bodyRenderDelayPromise = waitForMainCardResizeTransition();
        }

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

        const errors = [];
        let rework = null;
        let patternResult = null;
        let patternReport = null;
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

        const estimatorAlgorithm = currentEstimatorAlgorithm();
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
        try {
            const estimatorOptions = {
                speedRate: state.speedRate,
                odFlag: state.odFlag,
                cvtFlag: state.cvtFlag,
                withGraph: state.diffText === "Graph" || showsGraph,
            };

            const azusaOptions = {
                ...estimatorOptions,
                forceSunnyReferenceHo: state.azusaSunnyReferenceHo,
            };

            let selectedRework = null;
            let nextEstDiff = null;
            let nextNumericDifficulty = null;
            let nextNumericDifficultyHint = null;
            let actualEstimatorAlgorithm = estimatorAlgorithm;

            const isValidEstimatorResult = (result) => Boolean(result)
                && Number.isFinite(result.star)
                && Number.isFinite(result.numericDifficulty)
                && typeof result.estDiff === "string";

            if (estimatorAlgorithm === "Daniel") {
                const wp = runInWorker(rawText, { ...estimatorOptions, estimatorAlgorithm });
                selectedRework = wp ? await wp : runDanielEstimatorFromText(rawText, estimatorOptions);
                nextEstDiff = selectedRework.estDiff;
                nextNumericDifficulty = selectedRework.numericDifficulty;
                nextNumericDifficultyHint = selectedRework.numericDifficultyHint;
            } else if (estimatorAlgorithm === "Azusa") {
                const wp = runInWorker(rawText, { ...estimatorOptions, estimatorAlgorithm, forceSunnyReferenceHo: state.azusaSunnyReferenceHo });
                selectedRework = wp ? await wp : runAzusaEstimatorFromText(rawText, azusaOptions);
                actualEstimatorAlgorithm = selectedRework?.actualEstimatorAlgorithm || actualEstimatorAlgorithm;
                if (!isValidEstimatorResult(selectedRework)) {
                    selectedRework = runSunnyEstimatorFromText(rawText, estimatorOptions);
                    actualEstimatorAlgorithm = "Sunny";
                }
                nextEstDiff = selectedRework.estDiff;
                nextNumericDifficulty = selectedRework.numericDifficulty;
                nextNumericDifficultyHint = selectedRework.numericDifficultyHint;
            } else if (estimatorAlgorithm === "Roxy") {
                const wp = runInWorker(rawText, { ...estimatorOptions, estimatorAlgorithm });
                selectedRework = wp ? await wp : runRoxyEstimatorFromText(rawText, estimatorOptions);
                actualEstimatorAlgorithm = selectedRework?.actualEstimatorAlgorithm || actualEstimatorAlgorithm;
                if (!isValidEstimatorResult(selectedRework)) {
                    selectedRework = runSunnyEstimatorFromText(rawText, estimatorOptions);
                    actualEstimatorAlgorithm = "Sunny";
                }
                nextEstDiff = selectedRework.estDiff;
                nextNumericDifficulty = selectedRework.numericDifficulty;
                nextNumericDifficultyHint = selectedRework.numericDifficultyHint;
            } else if (estimatorAlgorithm === "Companella") {
                selectedRework = runSunnyEstimatorFromText(rawText, estimatorOptions);
                nextEstDiff = selectedRework.estDiff;
                nextNumericDifficulty = selectedRework.numericDifficulty;
                nextNumericDifficultyHint = selectedRework.numericDifficultyHint;
                pendingCompanellaEstimate = Number(selectedRework.columnCount) === 4;
            } else if (estimatorAlgorithm === "Mixed") {
                selectedRework = runMixedEstimatorFromText(rawText, estimatorOptions);
                nextEstDiff = selectedRework.estDiff;
                nextNumericDifficulty = selectedRework.numericDifficulty;
                nextNumericDifficultyHint = selectedRework.numericDifficultyHint;
                pendingMixedCompanellaContext = selectedRework.mixedCompanellaPlan || null;
            } else if (estimatorAlgorithm === "SunnyWindow") {
                selectedRework = runSunnyWindowEstimatorFromText(rawText, estimatorOptions);
                nextEstDiff = selectedRework.estDiff;
                nextNumericDifficulty = selectedRework.numericDifficulty;
                nextNumericDifficultyHint = selectedRework.numericDifficultyHint;
                typePercentageData = selectedRework.typePercentageData;
                lnStar = selectedRework.lnStar;
            } else {
                const wp = runInWorker(rawText, { ...estimatorOptions, estimatorAlgorithm: "Sunny" });
                selectedRework = wp ? await wp : runSunnyEstimatorFromText(rawText, estimatorOptions);
                nextEstDiff = selectedRework.estDiff;
                nextNumericDifficulty = selectedRework.numericDifficulty;
                nextNumericDifficultyHint = selectedRework.numericDifficultyHint;
            }

            rework = selectedRework;
            state.actualEstimatorAlgorithm = actualEstimatorAlgorithm;
            if (isStaleRequest()) return;

            // 拿到结果、即将首次写入 star 区块时再触发入场动画，
            // 与数值/难度名/图表的刷新同帧，换歌才整块入场，换难度只做轻量过渡。
            playStarBlockEntranceOnce();
            showNumericStarValue(rework.star);
            resolvedEstDiff = nextEstDiff;
            resolvedNumericDifficulty = nextNumericDifficulty;
            resolvedNumericDifficultyHint = nextNumericDifficultyHint;

            // 如果强制使用SunnyWindow，在这里替换LN部分
            if (shouldForceSunnyWindow && actualEstimatorAlgorithm != "SunnyWindow") {
                const sunnyWindowRework = runSunnyWindowEstimatorFromText(rawText, estimatorOptions);
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
            updateDiffTextVisibility();

            if (state.diffText === "Graph" || showsGraph) {
                if (!GRAPH_SUPPORTED_KEY_SET.has(rework.columnCount)) {
                    showDiffGraphError("Unsupported Keys");
                } else {
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
        } catch (error) {
            resetReworkDisplay();
            if (state.diffText === "Graph" || showsGraph) {
                showDiffGraphError("Graph unavailable");
            }
            errors.push(`Rework failed: ${error.message}`);
        }

        if (needInterludeValue) {
            try {
                interludeStar = await calculateInterludeStar(rawText, state.speedRate, state.cvtFlag);
                if (isStaleRequest()) return;
            } catch (error) {
                errors.push(`Interlude analyze failed: ${error.message}`);
            }
        }

        if (needPatternAnalysis) {
            try {
                patternResult = analyzePatternFromText(rawText);
                patternReport = patternResult?.report || null;
                const allClusters = patternResult?.report?.Clusters || patternResult?.topFiveClusters || [];
                const mergedClusters = mergeDuplicateClusters(allClusters);

                if (state.debugUseAmount) {
                    mergedClusters.sort((a, b) => b.Amount - a.Amount);
                    if (patternReport && mergedClusters.length > 0) {
                        const topSpecific = mergedClusters[0]?.SpecificTypes?.[0];
                        if (topSpecific && Number(topSpecific[1]) > 0.05) {
                            patternReport.Category = topSpecific[0];
                        } else {
                            patternReport.Category = mergedClusters[0].Pattern;
                        }
                    }
                }

                if (showsPattern) {
                    if (!(await waitForBodyRenderReady())) return;
                    renderPatternClusters(mergedClusters);
                }
            } catch (error) {
                if (showsPattern) {
                    if (!(await waitForBodyRenderReady())) return;
                    renderBodySectionError("Pattern", error.message);
                }
                errors.push(`Pattern analyze failed: ${error.message}`);
            }
        } else {
            patternClustersEl.innerHTML = "";
        }

        if (needEtternaAnalysis) {
            try {
                ettResult = await analyzeEtternaFromText(
                    rawText,
                    buildEtternaAnalyzeOptions(state.etternaVersion),
                );
                if (isStaleRequest()) return;

                const reworkStarValue = Number(rework?.star);
                const vibroEligible = Number.isFinite(reworkStarValue) && reworkStarValue > 5.0;
                isVibroMap = state.vibroDetection
                    && vibroEligible
                    && detectVibro(ettResult?.values, VIBRO_JACKSPEED_RATIO_THRESHOLD);

                if (showsEtterna) {
                    if (!(await waitForBodyRenderReady())) return;
                    const columnCount = Number(rework?.columnCount) || Number(parsedInfo.columnCount) || 0;
                    renderEtternaSkillBars(ettResult?.values || {}, columnCount);
                }
            } catch (error) {
                const isKeycountError = /unsupported keycount/i.test(String(error?.message ?? ""));
                if (showsEtterna) {
                    if (!(await waitForBodyRenderReady())) return;
                    renderBodySectionError("Etterna", isKeycountError ? "Unsupported Keycount" : error.message);
                    state.etternaTechnicalHidden = false;
                    mainCardEl.classList.remove("bars-etterna-compact");
                }
                if (shouldReportEtternaError && !isKeycountError) {
                    errors.push(`Etterna analyze failed: ${error.message}`);
                }
            }
        } else {
            state.etternaTechnicalHidden = false;
            mainCardEl.classList.remove("bars-etterna-compact");
            ettSkillBarsEl.innerHTML = "";
        }

        if (rework) {
            const shouldRunCompanella = Number(rework.columnCount) === 4
                && (pendingCompanellaEstimate || pendingMixedCompanellaContext != null);

            if (shouldRunCompanella) {
                let companellaMsdValues = ettResult?.values;
                const companellaEtternaVersion = String(
                    state.companellaEtternaVersion || state.etternaVersion,
                ).trim() || state.etternaVersion;

                if (state.etternaVersion !== companellaEtternaVersion) {
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

            const rawDiffText = formatDiffForDisplay(resolvedEstDiff);
            const diffText = (Number.isFinite(resolvedNumericDifficulty) && resolvedNumericDifficulty >= 18.5)
                ? "> Cloverwisp Theta high"
                : rawDiffText;
            setEstimateDifficultyText(diffText);
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

        const lnRatio = Number(rework?.lnRatio ?? parsedInfo.lnRatio)
        state.lnStar = lnStar ?? (state.enableAlwaysShowLNDifficulty || lnRatio > 0.15 ? rework?.star : 0) ?? 0;
        if (typePercentageData) {
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
        if (state.srText === "Pattern") {
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
