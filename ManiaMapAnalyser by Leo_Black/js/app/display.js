import {
    ETT_MAX_SKILL_VALUE,
    ETT_SKILLSET_ORDER,
    ETT_SKILLSET_ORDER_NO_TECHNICAL,
    ettSkillBarsEl,
    contentBarShows,
    mainCardEl,
    patternClustersEl,
    ppBarsEl,
    reworkBlockEl,
    reworkDiffEl,
    reworkRightCapsuleEl,
    reworkStarEl,
    STAR_BG_STOPS,
    STAR_TEXT_STOPS,
    starTipEl,
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

const RC_SUFFIX_LEVELS = {
    "low": 1,
    "mid/low": 2,
    "mid": 3,
    "mid/high": 4,
    "high": 5,
};

const RC_SUFFIX_ORDER = Object.keys(RC_SUFFIX_LEVELS).sort((a, b) => b.length - a.length);

function parseRcDifficultyLevel(diffText) {
    if (!diffText || diffText === "-") {
        return 0;
    }

    // For combined "RC || LN", take only the RC part (first line after formatDiffForDisplay split)
    // Lowercased: Daniel's DAN labels are Title-case ("Epsilon High"), RC suffixes are lowercase.
    const rcPart = diffText.split("\n")[0].trim().toLowerCase();
    if (!rcPart) {
        return 0;
    }

    // Check longer (more specific) suffixes first: "mid/low" and "mid/high"
    // before the shorter "low", "mid", "high"
    for (const suffix of RC_SUFFIX_ORDER) {
        if (rcPart.endsWith(suffix)) {
            return RC_SUFFIX_LEVELS[suffix];
        }
    }

    return 0;
}

export function updateStarTipDots(diffText) {
    if (!starTipEl) {
        return;
    }

    const level = parseRcDifficultyLevel(diffText);
    const dots = starTipEl.querySelectorAll(".star-dot");

    dots.forEach((dot) => {
        const dotLevel = Number(dot.dataset.level);
        dot.classList.toggle("lit", dotLevel >= 1 && dotLevel <= level);
    });
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
    if (!contentBarShows("Pattern")) {
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
    if (!contentBarShows("Etterna")) {
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
    renderReworkPpSkeleton();
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
    // 换歌时难度名做横向滑入，换难度时维持原来的纵向轻弹，从动作方向上区分两种场景。
    reworkDiffEl.classList.remove("diff-swap", "song-swap");
    const swapClass = state.activeChangeKind === "song" ? "song-swap" : "diff-swap";
    void reworkDiffEl.offsetWidth;
    reworkDiffEl.classList.add(swapClass);

    updateStarTipDots(nextText);
}

// 换歌时让整个 star 区块做一次明显的入场（淡入 + 上移 + 轻微放大），
// 换难度 / 仅改 mod 时不触发，避免每次切难度都“整块重刷”显得吵闹。
export function playStarBlockEntrance(changeKind) {
    if (!reworkBlockEl) {
        return;
    }

    reworkBlockEl.classList.remove("song-enter");
    if (changeKind !== "song") {
        return;
    }
    void reworkBlockEl.offsetWidth;
    reworkBlockEl.classList.add("song-enter");
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

export function show6KConstValue(constValue) {
    reworkStarEl.classList.remove("category-mode");
    animateNumericCapsuleValue(reworkStarEl, constValue);
    // LV 5→SR 1, LV 23→SR 10; below LV 5 clamped to SR 1
    const mappedStar = constValue < 5 ? 1 : 1 + (constValue - 5) * 0.5;
    const starBg = starColorFor(mappedStar);
    const preferredText = starTextColorFor(mappedStar);
    const starText = pickReadableTextColor(mappedStar, starBg, preferredText);
    reworkStarEl.style.backgroundColor = starBg;
    reworkStarEl.style.color = starText;
    reworkStarEl.style.textShadow = "none";
    reworkStarEl.classList.remove("high-contrast");
    syncLeftUnitBadgeContrast(starText);
}

// Pure animation interpolation (from → target over eased progress), exported
// for smoke-test assertion; from defaults to 0 so legacy calls animate 0→target.
export function numericAnimationValue(from, target, progress) {
    return from + (target - from) * progress;
}

function animateNumericCapsuleValue(element, targetValue, format, fromValue, durationMs) {
    if (!element) return;
    const numericTarget = Number(targetValue);
    if (!Number.isFinite(numericTarget)) {
        numericAnimationTokens.delete(element);
        element.textContent = "-";
        return;
    }

    // Per-call duration override: PP capsule animates faster (250ms) than the
    // default 400ms; star/MSD/6K calls keep the default.
    const duration = Number.isFinite(Number(durationMs)) && durationMs > 0 ? durationMs : NUMERIC_ANIMATION_DURATION_MS;
    const clampedTarget = Math.max(0, numericTarget);
    const from = Number.isFinite(Number(fromValue)) ? Math.max(0, Number(fromValue)) : 0;
    const token = Symbol("numeric-animation");
    numericAnimationTokens.set(element, token);
    const startTs = performance.now();
    const tick = (now) => {
        if (numericAnimationTokens.get(element) !== token) return;
        const progress = Math.min(1, (now - startTs) / duration);
        const eased = 1 - ((1 - progress) ** 3);
        const animatedValue = numericAnimationValue(from, clampedTarget, eased);
        const safeDisplayValue = animatedValue <= 0.0005 ? 0 : animatedValue;
        element.textContent = format ? format(safeDisplayValue) : safeDisplayValue.toFixed(2);
        if (progress < 1) {
            requestAnimationFrame(tick);
        }
    };

    requestAnimationFrame(tick);
}

// Always exactly 4 significant figures (250 → "250.0", 123.45 → "123.5").
// toPrecision(4) only emits exponent form above ~10000, which PP never reaches —
// defensive fallback anyway.
export function formatPpValue(value) {
    const num = Number(value);
    if (!Number.isFinite(num)) return "-";
    const s = num.toPrecision(4);
    if (s.includes("e")) return num.toFixed(0);
    return s;
}

// Map PP (0~1200, per ROW_SPECS) onto the 0~10 star color scale.
function ppToColorScaleValue(ppValue) {
    return Math.min(Math.max(Number(ppValue) || 0, 0) / 1200, 1) * 10.0;
}

// Last PP value the ReworkPP capsule animated to — consecutive live hits slide
// from it instead of restarting at 0. Stale across capsule mode switches
// (SR fallback) is acceptable: next showReworkPpValue just slides from the old
// number, which reads as a continuation, not a flash.
let lastShownPp = null;

// PP capsule fixed width: PP renders 4 significant digits (toPrecision(4)),
// i.e. 4 digits + 1 decimal point (e.g. "735.6"/"250.0"/"999.9"). With
// tabular-nums every digit is the same width, so measuring the widest common
// candidate "999.9" and caching it as --pp-capsule-width makes the capsule
// strictly match real content with zero slack. The rare low-PP form "0.0200"
// (6 chars) overflows by one digit — acceptable, PP < 0.1 is extremely rare.
let ppCapsuleWidth = null;

function measurePpCapsuleWidth() {
    if (ppCapsuleWidth != null) return ppCapsuleWidth;
    if (!reworkStarEl || !document.body) return 140;
    const probe = document.createElement("span");
    const cs = getComputedStyle(reworkStarEl);
    probe.style.position = "absolute";
    probe.style.visibility = "hidden";
    probe.style.whiteSpace = "nowrap";
    probe.style.fontFamily = cs.fontFamily;
    probe.style.fontSize = cs.fontSize;
    probe.style.fontWeight = cs.fontWeight;
    probe.style.fontVariantNumeric = cs.fontVariantNumeric;
    probe.style.lineHeight = cs.lineHeight;
    probe.style.padding = cs.padding;
    // cs.border is empty for shorthand (borderWidth/style/Color are the
    // exposed longhands); .star-value has a uniform 1px border on all sides
    // and the box is border-box, so include it or the probe measures 2px short.
    probe.style.borderWidth = cs.borderTopWidth || "1px";
    probe.style.borderStyle = "solid";
    probe.style.borderColor = "transparent";
    probe.textContent = "999.9";
    document.body.appendChild(probe);
    const width = Math.ceil(probe.getBoundingClientRect().width);
    probe.remove();
    ppCapsuleWidth = width;
    return width;
}

export function showReworkPpValue(ppValue) {
    reworkStarEl.classList.remove("category-mode");
    reworkStarEl.style.setProperty("--pp-capsule-width", `${measurePpCapsuleWidth()}px`);
    const from = lastShownPp != null ? lastShownPp : 0;
    // PP capsule: 250ms (medium speed-up vs. default 400ms)
    animateNumericCapsuleValue(reworkStarEl, ppValue, formatPpValue, from, 250);
    lastShownPp = Number(ppValue) || 0;
    const mappedStar = ppToColorScaleValue(ppValue);
    const starBg = starColorFor(mappedStar);
    const preferredText = starTextColorFor(mappedStar);
    const starText = pickReadableTextColor(mappedStar, starBg, preferredText);
    reworkStarEl.style.backgroundColor = starBg;
    reworkStarEl.style.color = starText;
    reworkStarEl.style.textShadow = "none";
    reworkStarEl.classList.remove("high-contrast");
    syncLeftUnitBadgeContrast(starText);
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

function buildPatternItemData(clusters) {
    const topFive = [...(clusters || [])].slice(0, 5);
    const maxAmount = Math.max(...topFive.map((cluster) => Number(cluster?.Amount) || 0), 1);

    while (topFive.length < 5) {
        topFive.push(null);
    }

    return topFive.map((cluster) => {
        if (!cluster) {
            return { empty: true, label: "-", subtype: "-", width: "0" };
        }
        const ratio = Math.max(0, Math.min((cluster.Amount / maxAmount) * 100, 100));
        return {
            empty: false,
            label: cluster.Pattern,
            subtype: formatClusterSpecificTypes(cluster.SpecificTypes),
            width: ratio.toFixed(2),
        };
    });
}

// 换难度 / 改设置时复用现有的条目结构，只更新文字和填充宽度，让进度条横向
// 平滑过渡（progress-bar 风格），而不是整组重建后再逐条弹入。只有换歌时才整组
// 重建走入场动画。
function canUpdateBarsInPlace(listEl, expectedCount, fillSelector) {
    if (state.activeChangeKind === "song") {
        return false;
    }
    const items = listEl.querySelectorAll(":scope > li");
    if (items.length !== expectedCount) {
        return false;
    }
    return [...items].every((el) => el.querySelector(fillSelector));
}

export function renderPatternClusters(clusters) {
    const items = buildPatternItemData(clusters);

    if (canUpdateBarsInPlace(patternClustersEl, items.length, ".cluster-fill")) {
        patternClustersEl.classList.add("bars-live");
        const rows = patternClustersEl.querySelectorAll(":scope > li");
        items.forEach((item, index) => {
            const row = rows[index];
            row.classList.toggle("empty", item.empty);
            const labelEl = row.querySelector(".cluster-label");
            const subtypeEl = row.querySelector(".cluster-subtype");
            const fillEl = row.querySelector(".cluster-fill");
            if (labelEl) labelEl.textContent = item.label;
            if (subtypeEl) subtypeEl.textContent = item.subtype;
            if (fillEl) fillEl.style.setProperty("--bar-width", `${item.width}%`);
        });
        return;
    }

    patternClustersEl.classList.remove("bars-live");
    patternClustersEl.innerHTML = items
        .map((item, index) => `
                <li class="cluster-item${item.empty ? " empty" : ""}" style="--item-delay:${index * ITEM_STAGGER_DELAY_MS}ms">
                    <div class="cluster-label">${item.label}</div>
                    <div class="cluster-track">
                        <div class="cluster-fill" style="--bar-width:${item.width}%"></div>
                    </div>
                    <div class="cluster-subtype">${item.subtype}</div>
                </li>
            `)
        .join("");
}

export function renderEtternaSkillBars(values, columnCount) {
    if (!contentBarShows("Etterna")) {
        state.etternaTechnicalHidden = false;
        mainCardEl.classList.remove("bars-etterna-compact");
        ettSkillBarsEl.innerHTML = "";
        return;
    }

    const safeValues = values && typeof values === "object" ? values : {};
    // Technical is hidden for every non-4K keycount: the n-key pipeline does
    // not produce a meaningful TechBase (values collapse to ~0.18 noise), so
    // showing it would mislead. 4K keeps the real 8-skill bar set.
    const hideTechnical = columnCount !== 4;
    state.etternaTechnicalHidden = hideTechnical;
    mainCardEl.classList.toggle("bars-etterna-compact", hideTechnical);

    const skillOrder = hideTechnical ? ETT_SKILLSET_ORDER_NO_TECHNICAL : ETT_SKILLSET_ORDER;

    const rowData = skillOrder.map((skillName) => {
        const rawValue = Number(safeValues[skillName]) || 0;
        const clampedValue = Math.max(0, Math.min(rawValue, ETT_MAX_SKILL_VALUE));
        const ratio = clampedValue / ETT_MAX_SKILL_VALUE;
        const width = ratio * 100;
        const labelPos = Math.max(8.0, Math.min(width, 97.0));
        // Rainbow on: inline the rainbow gradient + stretched size for the
        // sweep effect. Rainbow off: leave --ett-fill-bg unset so the
        // theme.css accent rule (html.ma-theme-osu .ett-skill-fill) takes
        // over, making the bar follow the cover-art color like pattern's
        // cluster-fill does. Without the osu theme it falls back to the
        // bars.css default gradient.
        const fillStyle = state.enableEtternaRainbowBars
            ? `--bar-width:${width.toFixed(2)}%;--ett-fill-bg:${ETT_FULL_TRACK_RAINBOW_GRADIENT};--ett-fill-bg-size:${(100 / Math.max(ratio, 0.001)).toFixed(3)}% 100%`
            : `--bar-width:${width.toFixed(2)}%`;
        return {
            skillName,
            value: rawValue.toFixed(2),
            width: width.toFixed(2),
            labelPos: labelPos.toFixed(2),
            fillStyle,
        };
    });

    // 换难度 / 改设置且条目数量不变时，原地更新数值与填充宽度，让进度条平滑横向
    // 过渡，不重建整组、不重放逐条弹入动画。换歌或列数变化时回到整组重建。
    if (canUpdateBarsInPlace(ettSkillBarsEl, rowData.length, ".ett-skill-fill")) {
        ettSkillBarsEl.classList.add("bars-live");
        const rows = ettSkillBarsEl.querySelectorAll(":scope > li");
        rowData.forEach((item, index) => {
            const row = rows[index];
            const labelEl = row.querySelector(".ett-skill-label");
            const fillEl = row.querySelector(".ett-skill-fill");
            const headEl = row.querySelector(".ett-skill-head");
            if (labelEl) labelEl.textContent = item.skillName;
            if (fillEl) fillEl.setAttribute("style", item.fillStyle);
            if (headEl) {
                headEl.textContent = item.value;
                headEl.style.setProperty("--label-pos", `${item.labelPos}%`);
            }
        });
        return;
    }

    ettSkillBarsEl.classList.remove("bars-live");
    ettSkillBarsEl.innerHTML = rowData
        .map((item, index) => `
                <li class="ett-skill-item" style="--item-delay:${index * 60}ms">
                    <div class="ett-skill-label">${item.skillName}</div>
                    <div class="ett-skill-track">
                        <div class="ett-skill-track-inner">
                            <div class="ett-skill-fill" style="${item.fillStyle}"></div>
                        </div>
                        <div class="ett-skill-head" style="--label-pos:${item.labelPos}%">${item.value}</div>
                    </div>
                </li>
            `)
        .join("");
}

// ─── ReworkPP bars ────────────────────────────────────────────────────────────
// DOM/CSS contract with Task 11 (styles/bars.css):
//   .pp-bars            — <ol> container (id="pp-bars")
//   .pp-item            — <li> row; centered rows also carry .pp-item--center
//   .pp-label           — row label
//   .pp-track/.pp-track-inner — track frame (same nesting as .ett-skill-*)
//   .pp-fill            — fill bar; width driven by --pp-width (inline style)
//   .pp-fill--center    — centered row variant; anchored at the track's 50%
//                         point (where value 1.0 sits). Positioning via the
//                         inline `--pp-side` var: "left" → left:50% +
//                         transform-origin:left (grows rightward),
//                         "right" → right:50% + transform-origin:right
//                         (grows leftward). Width still via --pp-width.
//   .pp-head            — value pill; positioned by --label-pos (as % of track)
//   Rainbow (optional): --ett-fill-bg + --ett-fill-bg-size on .pp-fill
//                       (same vars the .ett-skill-fill rainbow path uses)
const PP_ROW_COUNT = 5;
const PP_CENTER_LABEL_POS = 50; // % of track where value 1.0 anchors
// 换歌入场动画总窗口：340ms item-float-in/fill-grow-fast + 4×60ms --item-delay stagger。
// 窗口内 livePp 的 inPlaceOnly 渲染让路（见 renderReworkPpBars），避免 bars-live 掐断动画。
const PP_ENTRANCE_WINDOW_MS = 580;
let ppBarsRebuiltAt = 0;

function buildReworkPpRowData(item, index, mode) {
    const value = Number(item.value);
    const min = Number(item.min);
    const max = Number(item.max);
    const range = max - min || 1;
    const centered = Boolean(item.centered);
    // Row 1 (pp) label depends on the live/max mode, not the caller's label.
    const label = index === 0
        ? (mode === "live" ? "Live PP" : "Max PP")
        : item.label;

    let widthPct;
    let labelPos;
    let side = "";
    if (centered) {
        // Multiplier rows anchor at value 1.0 (track center): value >= 1.0
        // extends right from the center, < 1.0 extends left. The pill sits at
        // the fill's far end (away from the center anchor).
        widthPct = Math.min((Math.abs(value - 1.0) / range) * 100, 100);
        side = value >= 1.0 ? "left" : "right";
        labelPos = value >= 1.0
            ? PP_CENTER_LABEL_POS + widthPct
            : PP_CENTER_LABEL_POS - widthPct;
    } else {
        widthPct = Math.max(0, Math.min((value - min) / range, 1)) * 100;
        labelPos = widthPct;
    }
    // Pill must not overflow the card edges (8~97 keeps the 3dp capsule inside).
    labelPos = Math.max(8.0, Math.min(labelPos, 97.0));

    const ratio = widthPct / 100;
    const baseStyle = state.enableEtternaRainbowBars
        ? `--pp-width:${widthPct.toFixed(2)}%;--ett-fill-bg:${ETT_FULL_TRACK_RAINBOW_GRADIENT};--ett-fill-bg-size:${(100 / Math.max(ratio, 0.001)).toFixed(3)}% 100%`
        : `--pp-width:${widthPct.toFixed(2)}%`;
    const fillStyle = centered
        ? `--pp-side:${side};${baseStyle}`
        : baseStyle;

    return {
        label,
        // 前端不显示负数：Proportion 在 acc<=0.8 时值为 0（不为负），PP/乘子行恒非负，
        // Math.max(0, ...) 只是无害保险，防止任何意外负值泄漏到胶囊文本。
        // Proportion 按百分比显示（0.956 → "95.6%"），柱宽仍按 0~1 原始值计算（widthPct）。
        value: item.key === "proportion"
            ? `${(Math.max(0, value) * 100).toFixed(1)}%`
            : Math.max(0, value).toFixed(3),
        labelPos: labelPos.toFixed(2),
        fillStyle,
        centered,
        side,
    };
}

export function renderReworkPpBars(data, options = {}) {
    if (!contentBarShows("ReworkPP")) {
        ppBarsEl.innerHTML = "";
        return;
    }

    const mode = data && data.mode === "live" ? "live" : "max";
    const rows = data && Array.isArray(data.rows) ? data.rows : [];
    const rowData = rows.map((item, index) => buildReworkPpRowData(item, index, mode));

    if (rowData.length === 0) {
        ppBarsEl.innerHTML = '<li class="pp-item empty">No data</li>';
        return;
    }

    // 换难度 / 改设置且条目数量不变时，原地更新 label/值/样式，进度条平滑过渡；
    // 换歌或行数变化时回到整组重建并重放逐条弹入动画（与 etterna 双路径一致）。
    // inPlaceOnly（livePp 实时路径）：永远原地更新（420ms CSS 过渡平滑），忽略换歌
    // 检查 — 换图入场动画由 analysis 路径负责；仅校验行数与结构是否就绪。
    // 例外：换歌重建后的入场窗口内（580ms）让路，否则 bars-live 的 animation:none
    // 会掐断正在播放的弹入动画；窗口过后恢复正常原地更新。
    if (options.inPlaceOnly && performance.now() - ppBarsRebuiltAt < PP_ENTRANCE_WINDOW_MS) {
        return;
    }
    const canInPlace = options.inPlaceOnly
        ? (ppBarsEl.querySelectorAll(":scope > li").length === 5 && Boolean(ppBarsEl.querySelector(".pp-fill")))
        : canUpdateBarsInPlace(ppBarsEl, rowData.length, ".pp-fill");
    if (canInPlace) {
        ppBarsEl.classList.add("bars-live");
        const items = ppBarsEl.querySelectorAll(":scope > li");
        rowData.forEach((item, index) => {
            const row = items[index];
            const labelEl = row.querySelector(".pp-label");
            const fillEl = row.querySelector(".pp-fill");
            const headEl = row.querySelector(".pp-head");
            row.classList.toggle("pp-item--center", item.centered);
            if (labelEl) labelEl.textContent = item.label;
            if (fillEl) {
                fillEl.classList.toggle("pp-fill--center", item.centered);
                fillEl.setAttribute("style", item.fillStyle);
            }
            if (headEl) {
                headEl.textContent = item.value;
                headEl.style.setProperty("--label-pos", `${item.labelPos}%`);
            }
        });
        return;
    }

    ppBarsEl.classList.remove("bars-live");
    ppBarsRebuiltAt = performance.now(); // 重建=入场动画窗口起点，inPlaceOnly 在此窗口内让路
    ppBarsEl.innerHTML = rowData
        .map((item, index) => `
                <li class="pp-item${item.centered ? " pp-item--center" : ""}" style="--item-delay:${index * 60}ms">
                    <div class="pp-label">${item.label}</div>
                    <div class="pp-track">
                        <div class="pp-track-inner">
                            <div class="pp-fill${item.centered ? " pp-fill--center" : ""}" style="${item.fillStyle}"></div>
                        </div>
                        <div class="pp-head" style="--label-pos:${item.labelPos}%">${item.value}</div>
                    </div>
                </li>
            `)
        .join("");
}

export function renderReworkPpSkeleton() {
    if (!contentBarShows("ReworkPP")) {
        ppBarsEl.innerHTML = "";
        return;
    }

    ppBarsEl.innerHTML = Array.from({ length: PP_ROW_COUNT })
        .map(() => `
            <li class="pp-item skeleton">
                <div class="skeleton-line"></div>
                <div class="skeleton-track"></div>
            </li>
        `)
        .join("");
}

export function clearReworkPpBody() {
    ppBarsEl.classList.remove("bars-live");
    ppBarsEl.innerHTML = "";
}

export function formatDiffForDisplay(diffText) {
    if (!diffText) {
        return "-";
    }
    return String(diffText).split("||").map((part) => part.trim()).join("\n");
}

export function renderFullModeSeparators(ettOverallValue) {
    const sepEtterna = document.getElementById("sep-etterna");
    if (!sepEtterna) return;

    if (Number.isFinite(ettOverallValue)) {
        sepEtterna.textContent = `Etterna | Overall: ${ettOverallValue.toFixed(2)}`;
    } else {
        sepEtterna.textContent = "Etterna | Overall: --";
    }
}

export function formatMetadataStatus(metadata) {
    const artist = metadata.Artist || "Unknown Artist";
    const title = metadata.Title || "Unknown Title";
    const version = metadata.Version || "Unknown Difficulty";
    const creator = metadata.Creator || "Unknown Mapper";
    return `${artist} - ${title} [${version}] // ${creator}`;
}
