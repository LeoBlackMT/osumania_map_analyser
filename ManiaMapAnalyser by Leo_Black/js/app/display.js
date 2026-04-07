import {
    ETT_MAX_SKILL_VALUE,
    ETT_SKILLSET_ORDER,
    ETT_SKILLSET_ORDER_NO_TECHNICAL,
    ettSkillBarsEl,
    getActiveContentBar,
    mainCardEl,
    PATTERN_BAR_GRADIENT,
    patternClustersEl,
    reworkDiffEl,
    reworkRightCapsuleEl,
    reworkStarEl,
    STAR_BG_STOPS,
    STAR_TEXT_STOPS,
    state,
} from "./appContext.js";

const NUMERIC_ANIMATION_DURATION_MS = 400;
const ITEM_STAGGER_DELAY_MS = 80;
const numericAnimationTokens = new WeakMap();

function hexToRgb(hex) {
    const h = hex.replace("#", "");
    const full = h.length === 3
        ? h.split("").map((ch) => ch + ch).join("")
        : h;
    const int = Number.parseInt(full, 16);
    return {
        r: (int >> 16) & 255,
        g: (int >> 8) & 255,
        b: int & 255,
    };
}

function rgbToHex(r, g, b) {
    const toHex = (v) => v.toString(16).padStart(2, "0");
    return `#${toHex(r)}${toHex(g)}${toHex(b)}`;
}

function interpolateColor(hexA, hexB, t) {
    const a = hexToRgb(hexA);
    const b = hexToRgb(hexB);
    const r = Math.round(a.r + (b.r - a.r) * t);
    const g = Math.round(a.g + (b.g - a.g) * t);
    const bch = Math.round(a.b + (b.b - a.b) * t);
    return rgbToHex(r, g, bch);
}

export function starColorFor(starValue) {
    if (!Number.isFinite(starValue)) return "#6d7894";

    if (starValue <= STAR_BG_STOPS[0][0]) {
        return STAR_BG_STOPS[0][1];
    }

    for (let i = 0; i < STAR_BG_STOPS.length - 1; i += 1) {
        const [lVal, lColor] = STAR_BG_STOPS[i];
        const [rVal, rColor] = STAR_BG_STOPS[i + 1];
        if (starValue >= lVal && starValue <= rVal) {
            const t = (starValue - lVal) / (rVal - lVal || 1);
            return interpolateColor(lColor, rColor, t);
        }
    }

    return STAR_BG_STOPS[STAR_BG_STOPS.length - 1][1];
}

function buildFullTrackRainbowGradient() {
    const stops = [];

    for (const [value, color] of STAR_BG_STOPS) {
        const numeric = Number(value);
        if (!Number.isFinite(numeric)) {
            continue;
        }

        const clamped = Math.max(0, Math.min(numeric, 9));
        if (stops.length === 0 || stops[stops.length - 1][0] !== clamped) {
            stops.push([clamped, color]);
        } else {
            stops[stops.length - 1][1] = color;
        }

        if (numeric >= 9) {
            break;
        }
    }

    if (stops.length === 0 || stops[0][0] > 0) {
        stops.unshift([0, starColorFor(0)]);
    }
    if (stops[stops.length - 1][0] < 9) {
        stops.push([9, starColorFor(9)]);
    }

    const stopText = stops
        .map(([value, color]) => `${color} ${((value / 9) * 100).toFixed(3)}%`)
        .join(", ");
    return `linear-gradient(90deg, ${stopText})`;
}

const ETT_FULL_TRACK_RAINBOW_GRADIENT = buildFullTrackRainbowGradient();

function starTextColorFor(starValue) {
    if (!Number.isFinite(starValue)) return "#f6fbff";

    if (starValue <= STAR_TEXT_STOPS[0][0]) {
        return STAR_TEXT_STOPS[0][1];
    }

    for (let i = 0; i < STAR_TEXT_STOPS.length - 1; i += 1) {
        const [lVal, lColor] = STAR_TEXT_STOPS[i];
        const [rVal, rColor] = STAR_TEXT_STOPS[i + 1];
        if (starValue >= lVal && starValue <= rVal) {
            const t = (starValue - lVal) / (rVal - lVal || 1);
            return interpolateColor(lColor, rColor, t);
        }
    }

    return STAR_TEXT_STOPS[STAR_TEXT_STOPS.length - 1][1];
}

function relativeLuminance(hexColor) {
    const { r, g, b } = hexToRgb(hexColor);
    const convert = (v) => {
        const c = v / 255;
        if (c <= 0.03928) {
            return c / 12.92;
        }
        return ((c + 0.055) / 1.055) ** 2.4;
    };
    return 0.2126 * convert(r) + 0.7152 * convert(g) + 0.0722 * convert(b);
}

function contrastRatio(hexA, hexB) {
    const l1 = relativeLuminance(hexA);
    const l2 = relativeLuminance(hexB);
    const bright = Math.max(l1, l2);
    const dark = Math.min(l1, l2);
    return (bright + 0.05) / (dark + 0.05);
}

function pickReadableTextColor(starValue, bgColor, preferredColor) {
    if (Number.isFinite(starValue) && starValue > 12) {
        return "#6563de";
    }

    if (Number.isFinite(starValue) && starValue >= 6.0 && starValue < 6.5) {
        return "#000000";
    }

    if (Number.isFinite(starValue) && starValue >= 6.5 && starValue <= 8.9) {
        return "#ffd966";
    }

    const preferred = preferredColor || "#f6fbff";
    if (contrastRatio(bgColor, preferred) >= 4.5) {
        return preferred;
    }

    const candidateDark = "#111111";
    const candidateLight = "#f6fbff";
    const candidateGold = "#FFD966";

    const darkRatio = contrastRatio(bgColor, candidateDark);
    const lightRatio = contrastRatio(bgColor, candidateLight);
    const goldRatio = contrastRatio(bgColor, candidateGold);

    if (starValue >= 7.0 && starValue <= 10.0) {
        if (goldRatio >= 4.5) return candidateGold;
        if (lightRatio >= darkRatio) return candidateLight;
        return candidateDark;
    }

    if (darkRatio >= 4.5 || darkRatio > lightRatio) {
        return candidateDark;
    }
    return candidateLight;
}

function shouldUseLightUnitBadgeText(textColor) {
    const normalized = typeof textColor === "string" ? textColor.trim() : "";
    if (!/^#([0-9a-f]{3}|[0-9a-f]{6})$/i.test(normalized)) {
        return false;
    }
    return relativeLuminance(normalized) <= 0.09;
}

function syncLeftUnitBadgeContrast(textColor) {
    if (!reworkStarEl) {
        return;
    }

    reworkStarEl.classList.toggle("unit-badge-light", shouldUseLightUnitBadgeText(textColor));
}

function setRightCapsuleUnitBadge(unitText) {
    if (!reworkRightCapsuleEl) {
        return;
    }

    const normalized = typeof unitText === "string" ? unitText.trim() : "";
    if (!normalized) {
        reworkRightCapsuleEl.classList.remove("has-unit");
        reworkRightCapsuleEl.removeAttribute("data-unit");
        return;
    }

    reworkRightCapsuleEl.classList.add("has-unit");
    reworkRightCapsuleEl.setAttribute("data-unit", normalized);
}

function formatClusterSpecificTypes(specificTypes) {
    if (!specificTypes || !specificTypes.length) {
        return "-";
    }

    return specificTypes
        .map(([name, ratio]) => `${name} (${(ratio * 100).toFixed(1)}%)`)
        .join(", ");
}

function restartAnimationClass(element, className) {
    if (!element || !className) {
        return;
    }

    element.classList.remove(className);
    void element.offsetWidth;
    element.classList.add(className);
}

function estimateBodySkeletonItemCount(mode) {
    const isEtterna = mode === "etterna";
    const minimum = isEtterna ? (state.etternaTechnicalHidden ? 6 : 7) : 5;
    const maximum = isEtterna ? 12 : 10;

    if (!mainCardEl || typeof window === "undefined") {
        return minimum;
    }

    const computedStyle = window.getComputedStyle(mainCardEl);
    const minHeight = Number.parseFloat(computedStyle.minHeight) || 0;
    const measuredHeight = Number(mainCardEl.getBoundingClientRect().height) || 0;
    const cardHeight = Math.max(minHeight, measuredHeight);
    if (!(cardHeight > 0)) {
        return minimum;
    }

    const RESERVED_TOP_SECTION_PX = 220;
    const availableHeight = Math.max(0, cardHeight - RESERVED_TOP_SECTION_PX);
    const rowHeight = isEtterna ? 19 : 24;
    const rowGap = isEtterna ? 8 : 7;

    // Include the inter-row gap in the estimate so skeleton density tracks card length naturally.
    const estimated = Math.floor((availableHeight + rowGap) / (rowHeight + rowGap));
    return Math.max(minimum, Math.min(maximum, estimated));
}

export function mergeDuplicateClusters(clusters) {
    const mergedMap = new Map();

    for (const cluster of clusters) {
        const key = cluster.Pattern;
        if (!mergedMap.has(key)) {
            mergedMap.set(key, {
                Pattern: cluster.Pattern,
                Amount: 0,
                BPM: cluster.BPM,
                SpecificTypes: new Map(),
            });
        }

        const merged = mergedMap.get(key);
        merged.Amount += Number(cluster.Amount) || 0;
        merged.BPM = Math.max(Number(merged.BPM) || 0, Number(cluster.BPM) || 0);

        const specificTypes = Array.isArray(cluster.SpecificTypes) ? cluster.SpecificTypes : [];
        for (const [name, ratio] of specificTypes) {
            const weighted = (Number(ratio) || 0) * (Number(cluster.Amount) || 0);
            merged.SpecificTypes.set(name, (merged.SpecificTypes.get(name) || 0) + weighted);
        }
    }

    return [...mergedMap.values()]
        .map((item) => {
            const total = item.Amount > 0 ? item.Amount : 1;
            const normalizedSpecific = [...item.SpecificTypes.entries()]
                .map(([name, weighted]) => [name, weighted / total])
                .sort((a, b) => b[1] - a[1]);
            return {
                Pattern: item.Pattern,
                Amount: item.Amount,
                BPM: item.BPM,
                SpecificTypes: normalizedSpecific,
            };
        });
}

export function renderClusterSkeleton() {
    if (getActiveContentBar() !== "Pattern") {
        patternClustersEl.innerHTML = "";
        return;
    }

    const itemCount = estimateBodySkeletonItemCount("pattern");
    patternClustersEl.innerHTML = Array.from({ length: itemCount })
        .map(() => `
            <li class="cluster-item skeleton">
                <div class="skeleton-line"></div>
                <div class="skeleton-track"></div>
            </li>
        `)
        .join("");
}

export function renderEtternaSkeleton() {
    if (getActiveContentBar() !== "Etterna") {
        ettSkillBarsEl.innerHTML = "";
        return;
    }

    const itemCount = estimateBodySkeletonItemCount("etterna");
    ettSkillBarsEl.innerHTML = Array.from({ length: itemCount })
        .map(() => `
            <li class="ett-skill-item skeleton">
                <div class="skeleton-line"></div>
                <div class="skeleton-track"></div>
            </li>
        `)
        .join("");
}

export function renderContentSkeleton() {
    renderClusterSkeleton();
    renderEtternaSkeleton();
}

export function setEstimateDifficultyText(value) {
    if (!reworkDiffEl) {
        return;
    }

    const nextText = String(value ?? "-");
    if (reworkDiffEl.textContent === nextText) {
        return;
    }

    reworkDiffEl.textContent = nextText;
    restartAnimationClass(reworkDiffEl, "diff-swap");
}

export function showNumericStarValue(starValue) {
    reworkStarEl.classList.remove("category-mode");
    animateNumericCapsuleValue(reworkStarEl, starValue);
    const starBg = starColorFor(starValue);
    const preferredText = starTextColorFor(starValue);
    const starText = pickReadableTextColor(starValue, starBg, preferredText);
    reworkStarEl.style.backgroundColor = starBg;
    reworkStarEl.style.color = starText;
    reworkStarEl.style.textShadow = "none";
    reworkStarEl.classList.remove("high-contrast");
    syncLeftUnitBadgeContrast(starText);
}

function animateNumericCapsuleValue(element, targetValue) {
    if (!element) return;
    const numericTarget = Number(targetValue);
    if (!Number.isFinite(numericTarget)) {
        numericAnimationTokens.delete(element);
        element.textContent = "-";
        return;
    }

    const clampedTarget = Math.max(0, numericTarget);
    const token = Symbol("numeric-animation");
    numericAnimationTokens.set(element, token);
    const startTs = performance.now();
    const tick = (now) => {
        if (numericAnimationTokens.get(element) !== token) return;
        const progress = Math.min(1, (now - startTs) / NUMERIC_ANIMATION_DURATION_MS);
        const eased = 1 - ((1 - progress) ** 3);
        const animatedValue = clampedTarget * eased;
        const safeDisplayValue = animatedValue <= 0.0005 ? 0 : animatedValue;
        element.textContent = safeDisplayValue.toFixed(2);
        if (progress < 1) {
            requestAnimationFrame(tick);
        }
    };

    requestAnimationFrame(tick);
}

function sanitizeCategoryText(categoryText) {
    if (typeof categoryText !== "string") {
        return "-";
    }
    return categoryText.replace(/\s*\(Tag:\s*[^)]*\)\s*$/i, "").trim() || "-";
}

export function showCategoryValue(categoryText) {
    numericAnimationTokens.delete(reworkStarEl);
    reworkStarEl.classList.add("category-mode");
    reworkStarEl.textContent = sanitizeCategoryText(categoryText);
    reworkStarEl.style.backgroundColor = "rgba(38, 50, 84, 0.45)";
    reworkStarEl.style.color = "#f6fbff";
    reworkStarEl.style.textShadow = "none";
    reworkStarEl.classList.remove("high-contrast");
    syncLeftUnitBadgeContrast("#f6fbff");
}

function overallToStarValue(overallValue) {
    const normalized = Math.max(0, Math.min(overallValue, ETT_MAX_SKILL_VALUE)) / ETT_MAX_SKILL_VALUE;
    return normalized * 10.0;
}

export function interludeToSrColorValue(interludeStarValue) {
    const isr = Number(interludeStarValue);
    if (!Number.isFinite(isr)) {
        return Number.NaN;
    }
    return (isr * 10.0) / 15.0;
}

export function showMsdValue(overallValue) {
    reworkStarEl.classList.remove("category-mode");
    animateNumericCapsuleValue(reworkStarEl, overallValue);
    const mappedStar = overallToStarValue(overallValue);
    const starBg = starColorFor(mappedStar);
    const preferredText = starTextColorFor(mappedStar);
    const starText = pickReadableTextColor(mappedStar, starBg, preferredText);
    reworkStarEl.style.backgroundColor = starBg;
    reworkStarEl.style.color = starText;
    reworkStarEl.style.textShadow = "none";
    reworkStarEl.classList.remove("high-contrast");
    syncLeftUnitBadgeContrast(starText);
}

export function showInterludeValue(interludeStarValue) {
    reworkStarEl.classList.remove("category-mode");
    animateNumericCapsuleValue(reworkStarEl, interludeStarValue);
    const mappedStar = interludeToSrColorValue(interludeStarValue);
    const starBg = starColorFor(mappedStar);
    const preferredText = starTextColorFor(mappedStar);
    const starText = pickReadableTextColor(mappedStar, starBg, preferredText);
    reworkStarEl.style.backgroundColor = starBg;
    reworkStarEl.style.color = starText;
    reworkStarEl.style.textShadow = "none";
    reworkStarEl.classList.remove("high-contrast");
    syncLeftUnitBadgeContrast(starText);
}

function showRightCapsuleNumericValue(targetValue, mappedStarValue, unitText = "") {
    if (!reworkRightCapsuleEl) {
        return;
    }
    reworkRightCapsuleEl.classList.remove("category-mode");
    reworkRightCapsuleEl.classList.add("numeric-mode");
    reworkRightCapsuleEl.classList.remove("high-contrast");
    setRightCapsuleUnitBadge(unitText);
    animateNumericCapsuleValue(reworkRightCapsuleEl, targetValue);

    if (!Number.isFinite(mappedStarValue)) {
        reworkRightCapsuleEl.style.backgroundColor = "rgba(38, 50, 84, 0.45)";
        reworkRightCapsuleEl.style.color = "#f6fbff";
        reworkRightCapsuleEl.style.textShadow = "none";
        return;
    }

    const bg = starColorFor(mappedStarValue);
    const preferredText = starTextColorFor(mappedStarValue);
    const textColor = pickReadableTextColor(mappedStarValue, bg, preferredText);
    reworkRightCapsuleEl.style.backgroundColor = bg;
    reworkRightCapsuleEl.style.color = textColor;
    reworkRightCapsuleEl.style.textShadow = "none";
}

function showRightCapsuleCategoryValue(categoryText) {
    if (!reworkRightCapsuleEl) {
        return;
    }
    numericAnimationTokens.delete(reworkRightCapsuleEl);
    reworkRightCapsuleEl.classList.remove("numeric-mode");
    reworkRightCapsuleEl.classList.add("category-mode");
    reworkRightCapsuleEl.classList.remove("high-contrast");
    setRightCapsuleUnitBadge("");
    reworkRightCapsuleEl.textContent = sanitizeCategoryText(categoryText);
    reworkRightCapsuleEl.style.backgroundColor = "rgba(38, 50, 84, 0.45)";
    reworkRightCapsuleEl.style.color = "#f6fbff";
    reworkRightCapsuleEl.style.textShadow = "none";
}

export function renderRightCapsule(diffMode, reworkStarValue, patternCategoryText, etternaOverallValue, interludeStarValue) {
    if (!reworkRightCapsuleEl) {
        return;
    }

    if (diffMode === "ReworkSR") {
        showRightCapsuleNumericValue(reworkStarValue, reworkStarValue);
        return;
    }

    if (diffMode === "MSD") {
        const mappedStar = Number.isFinite(etternaOverallValue) ? overallToStarValue(etternaOverallValue) : NaN;
        showRightCapsuleNumericValue(etternaOverallValue, mappedStar);
        return;
    }

    if (diffMode === "InterludeSR") {
        const mappedStar = interludeToSrColorValue(interludeStarValue);
        showRightCapsuleNumericValue(interludeStarValue, mappedStar, "");
        return;
    }

    if (diffMode === "Pattern") {
        showRightCapsuleCategoryValue(patternCategoryText);
        return;
    }

    reworkRightCapsuleEl.classList.remove("category-mode");
    reworkRightCapsuleEl.classList.remove("numeric-mode");
    reworkRightCapsuleEl.classList.remove("high-contrast");
    setRightCapsuleUnitBadge("");
    numericAnimationTokens.delete(reworkRightCapsuleEl);
    reworkRightCapsuleEl.textContent = "-";
    reworkRightCapsuleEl.style.backgroundColor = "rgba(38, 50, 84, 0.45)";
    reworkRightCapsuleEl.style.color = "#f6fbff";
    reworkRightCapsuleEl.style.textShadow = "none";
}

export function renderPatternClusters(clusters) {
    const topFive = [...(clusters || [])].slice(0, 5);
    const maxAmount = Math.max(...topFive.map((cluster) => Number(cluster?.Amount) || 0), 1);

    while (topFive.length < 5) {
        topFive.push(null);
    }

    patternClustersEl.innerHTML = topFive
        .map((cluster, index) => {
            if (!cluster) {
                return `
                    <li class="cluster-item empty" style="--item-delay:${index * ITEM_STAGGER_DELAY_MS}ms">
                        <div class="cluster-label">-</div>
                        <div class="cluster-track">
                            <div class="cluster-fill" style="--bar-width:0%"></div>
                        </div>
                        <div class="cluster-subtype">-</div>
                    </li>
                `;
            }

            const ratio = Math.max(0, Math.min((cluster.Amount / maxAmount) * 100, 100));
            const subtype = formatClusterSpecificTypes(cluster.SpecificTypes);
            return `
                <li class="cluster-item" style="--item-delay:${index * ITEM_STAGGER_DELAY_MS}ms">
                    <div class="cluster-label">${cluster.Pattern}</div>
                    <div class="cluster-track">
                        <div class="cluster-fill" style="--bar-width:${ratio.toFixed(2)}%"></div>
                    </div>
                    <div class="cluster-subtype">${subtype}</div>
                </li>
            `;
        })
        .join("");
}

export function renderEtternaSkillBars(values, columnCount) {
    if (getActiveContentBar() !== "Etterna") {
        state.etternaTechnicalHidden = false;
        mainCardEl.classList.remove("bars-etterna-compact");
        ettSkillBarsEl.innerHTML = "";
        return;
    }

    const safeValues = values && typeof values === "object" ? values : {};
    const hideTechnical = columnCount === 6 || columnCount === 7;
    state.etternaTechnicalHidden = hideTechnical;
    mainCardEl.classList.toggle("bars-etterna-compact", hideTechnical);

    const skillOrder = hideTechnical ? ETT_SKILLSET_ORDER_NO_TECHNICAL : ETT_SKILLSET_ORDER;

    ettSkillBarsEl.innerHTML = skillOrder
        .map((skillName, index) => {
            const rawValue = Number(safeValues[skillName]) || 0;
            const clampedValue = Math.max(0, Math.min(rawValue, ETT_MAX_SKILL_VALUE));
            const ratio = clampedValue / ETT_MAX_SKILL_VALUE;
            const width = ratio * 100;
            const labelPos = Math.max(8.0, Math.min(width, 97.0));
            const fillBackground = state.enableEtternaRainbowBars
                ? ETT_FULL_TRACK_RAINBOW_GRADIENT
                : PATTERN_BAR_GRADIENT;
            const fillBackgroundSize = state.enableEtternaRainbowBars
                ? `${(100 / Math.max(ratio, 0.001)).toFixed(3)}% 100%`
                : "100% 100%";

            return `
                <li class="ett-skill-item" style="--item-delay:${index * 60}ms">
                    <div class="ett-skill-label">${skillName}</div>
                    <div class="ett-skill-track">
                        <div class="ett-skill-track-inner">
                            <div class="ett-skill-fill" style="--bar-width:${width.toFixed(2)}%;--ett-fill-bg:${fillBackground};--ett-fill-bg-size:${fillBackgroundSize}"></div>
                        </div>
                        <div class="ett-skill-head" style="--label-pos:${labelPos.toFixed(2)}%">${rawValue.toFixed(2)}</div>
                    </div>
                </li>
            `;
        })
        .join("");
}

export function formatDiffForDisplay(diffText) {
    if (!diffText) {
        return "-";
    }
    return String(diffText).split("||").map((part) => part.trim()).join("\n");
}

export function formatMetadataStatus(metadata) {
    const artist = metadata.Artist || "Unknown Artist";
    const title = metadata.Title || "Unknown Title";
    const version = metadata.Version || "Unknown Difficulty";
    const creator = metadata.Creator || "Unknown Mapper";
    return `${artist} - ${title} [${version}] // ${creator}`;
}
