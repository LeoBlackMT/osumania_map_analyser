import { runReworkFromText } from "../estimator/reworkEstimator.js";
import { analyzePatternFromText } from "../patterns/service.js";
import { OsuFileParser } from "../file/osuFileParser.js";
import {
    analyzeEtternaFromText,
    DEFAULT_SCORE_GOAL as ETT_DEFAULT_SCORE_GOAL,
} from "../ett/index.js";
import { PATTERNS_CONFIG } from "../patterns/config.js";
import {
    ENDPOINT,
    ettSkillBarsEl,
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
    showCategoryValue,
    showMsdValue,
    showNumericStarValue,
} from "./display.js";
import { modeTagFromLnRatio } from "./modeLogic.js";
import { hideOverlay, setModeTag, setStatus, showOverlay } from "./hud.js";
import {
    clearAllPauseMarkers,
    clearDiffGraph,
    renderDiffGraph,
    setNumericDifficultyValue,
    showDiffGraphError,
    setGraphLoading,
} from "./graph.js";
import {
    currentUseDanielAlgorithm,
    isAutoDisplayEnabledNow,
    refreshAutoDisplayProfile,
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

export function resetReworkDisplay() {
    setNumericDifficultyValue(null);
    reworkStarEl.textContent = "-";
    reworkStarEl.classList.remove("category-mode");
    reworkDiffEl.textContent = "-";
    if (reworkRightCapsuleEl) {
        reworkRightCapsuleEl.textContent = "-";
        reworkRightCapsuleEl.classList.remove("category-mode", "numeric-mode", "high-contrast");
        reworkRightCapsuleEl.style.backgroundColor = "rgba(38, 50, 84, 0.45)";
        reworkRightCapsuleEl.style.color = "#f6fbff";
        reworkRightCapsuleEl.style.textShadow = "none";
    }
    clearDiffGraph();
    clearAllPauseMarkers();
    if (state.diffText === "Graph" || state.contentBar === "Graph") {
        showDiffGraphError("Graph unavailable");
    }
    reworkMetaEl.innerHTML = "LN%: -<br/>Keys: -";
    setModeTag("Mix");
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

    setStatus(`Loading beatmap file (${reason})...`, "loading");
    hideOverlay();

    if (state.diffText === "Graph" || state.contentBar === "Graph") {
        setGraphLoading(true);
    } else {
        clearDiffGraph();
    }

    renderContentSkeleton();

    try {
        const response = await fetch(ENDPOINT, {
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

        if (isAutoDisplayEnabledNow()) {
            const predictedModeTag = modeTagFromLnRatio(Number(parsedInfo.lnRatio));
            refreshAutoDisplayProfile(predictedModeTag);

            if (state.diffText === "Graph" || state.contentBar === "Graph") {
                setGraphLoading(true);
            } else {
                clearDiffGraph();
            }

            renderContentSkeleton();
        }

        const errors = [];
        let rework = null;
        let patternResult = null;
        let patternReport = null;
        let ettResult = null;
        let isVibroMap = false;

        const needPatternAnalysis = state.contentBar === "Pattern"
            || state.srText === "Pattern"
            || state.diffText === "Pattern"
            || state.debugUseSvDetection;
        const needMsdValue = state.srText === "MSD" || state.diffText === "MSD";
        const needVibroDetection = state.vibroDetection;
        const needEtternaAnalysis = state.contentBar === "Etterna" || needMsdValue || needVibroDetection;
        const shouldReportEtternaError = state.contentBar === "Etterna" || needMsdValue;

        try {
            rework = runReworkFromText(rawText, {
                speedRate: state.speedRate,
                odFlag: state.odFlag,
                cvtFlag: state.cvtFlag,
                withGraph: state.diffText === "Graph" || state.contentBar === "Graph",
                useDanielAlgorithm: currentUseDanielAlgorithm(),
            });
            if (isStaleRequest()) return;

            showNumericStarValue(rework.star);
            setNumericDifficultyValue(rework.numericDifficulty, rework.numericDifficultyHint);

            const diffText = GRAPH_SUPPORTED_KEY_SET.has(rework.columnCount)
                ? formatDiffForDisplay(rework.estDiff)
                : "Unsupported Keys";
            reworkDiffEl.textContent = diffText;

            if (state.diffText === "Graph" || state.contentBar === "Graph") {
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
            reworkMetaEl.innerHTML = `LN%: ${lnPercent}<br/>Keys: ${rework.columnCount}`;
            reworkMetaEl.classList.remove("loading");
        } catch (error) {
            resetReworkDisplay();
            if (state.diffText === "Graph" || state.contentBar === "Graph") {
                showDiffGraphError("Graph unavailable");
            }
            errors.push(`Rework failed: ${error.message}`);
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

                if (state.contentBar === "Pattern") {
                    renderPatternClusters(mergedClusters);
                }
            } catch (error) {
                if (state.contentBar === "Pattern") {
                    renderBodySectionError("Pattern", error.message);
                }
                errors.push(`Pattern analyze failed: ${error.message}`);
            }
        } else {
            patternClustersEl.innerHTML = "";
        }

        if (needEtternaAnalysis) {
            try {
                ettResult = await analyzeEtternaFromText(rawText, {
                    musicRate: state.speedRate,
                    scoreGoal: ETT_DEFAULT_SCORE_GOAL,
                    cvtFlag: state.cvtFlag,
                });
                if (isStaleRequest()) return;

                const reworkStarValue = Number(rework?.star);
                const vibroEligible = Number.isFinite(reworkStarValue) && reworkStarValue > 5.0;
                isVibroMap = state.vibroDetection
                    && vibroEligible
                    && detectVibro(ettResult?.values, VIBRO_JACKSPEED_RATIO_THRESHOLD);

                if (state.contentBar === "Etterna") {
                    const columnCount = Number(rework?.columnCount) || Number(parsedInfo.columnCount) || 0;
                    renderEtternaSkillBars(ettResult?.values || {}, columnCount);
                }
            } catch (error) {
                if (state.contentBar === "Etterna") {
                    renderBodySectionError("Etterna", error.message);
                    state.etternaTechnicalHidden = false;
                    mainCardEl.classList.remove("bars-etterna-compact");
                }
                if (shouldReportEtternaError) {
                    errors.push(`Etterna analyze failed: ${error.message}`);
                }
            }
        } else {
            state.etternaTechnicalHidden = false;
            mainCardEl.classList.remove("bars-etterna-compact");
            ettSkillBarsEl.innerHTML = "";
        }

        const fallbackModeTag = modeTagFromLnRatio(Number(rework?.lnRatio ?? parsedInfo.lnRatio));
        let resolvedModeTag = (state.contentBar === "None")
            ? fallbackModeTag
            : (patternResult?.report?.ModeTag || fallbackModeTag);

        if (state.debugUseSvDetection) {
            const svAmount = Number(patternReport?.SVAmount);
            if (Number.isFinite(svAmount) && svAmount >= PATTERNS_CONFIG.SV_AMOUNT_THRESHOLD) {
                resolvedModeTag = "SV";
                if (patternReport && typeof patternReport === "object") {
                    patternReport.Category = "SV";
                }
            }
        }

        setModeTag(resolvedModeTag);

        if (isAutoDisplayEnabledNow()) {
            const beforeContent = state.contentBar;
            const beforeSrText = state.srText;
            const profileChanged = refreshAutoDisplayProfile(resolvedModeTag);

            const missingEtterna = (
                state.contentBar === "Etterna"
                || state.srText === "MSD"
                || state.diffText === "MSD"
            ) && !needEtternaAnalysis;
            const missingPattern = (
                state.contentBar === "Pattern"
                || state.srText === "Pattern"
                || state.diffText === "Pattern"
                || state.debugUseSvDetection
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
        } else if (state.srText === "MSD") {
            const overallValue = Number(ettResult?.values?.Overall);
            if (Number.isFinite(overallValue)) {
                showMsdValue(overallValue);
                leftCapsuleUnit = "MSD";
            } else if (rework) {
                // Fallback to ReworkSR when MSD value is unavailable.
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
        );

        if (isVibroMap && state.diffText === "Difficulty") {
            reworkDiffEl.textContent = "VIBRO";
        }

        const metadataLine = formatMetadataStatus(parsedInfo.metadata);
        if (errors.length > 0) {
            setStatus(`${metadataLine} (partial errors)`, "error");
            hideOverlay();
        } else {
            setStatus(metadataLine, "ok");
            hideOverlay();
        }
    } catch (error) {
        if (isStaleRequest()) return;
        setStatus(`Failed to load beatmap file: ${error.message}`, "error");
        resetReworkDisplay();
        patternClustersEl.innerHTML = state.contentBar === "Pattern"
            ? "<li class=\"cluster-item empty\">No data</li>"
            : "";
        ettSkillBarsEl.innerHTML = state.contentBar === "Etterna"
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
