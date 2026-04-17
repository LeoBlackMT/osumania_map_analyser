export function normalizeLooseName(value) {
    return String(value ?? "").trim().toLowerCase();
}

export function normalizePattern(value) {
    return String(value ?? "").trim().toLowerCase();
}

export function normalizeSubPattern(value) {
    const text = String(value ?? "").trim();
    return text || "Unsigned";
}

export function normalizeScope(value, lnScope) {
    const text = String(value ?? "").trim().toUpperCase();
    return text === lnScope ? lnScope : "RC";
}

export function isCsvFileName(fileName) {
    return /\.csv$/i.test(String(fileName ?? "").trim());
}

export function stripCsvSuffix(fileName) {
    return String(fileName ?? "").replace(/\.csv$/i, "").trim();
}

export function escapeHtml(value) {
    return String(value ?? "")
        .replace(/&/g, "&amp;")
        .replace(/</g, "&lt;")
        .replace(/>/g, "&gt;")
        .replace(/\"/g, "&quot;")
        .replace(/'/g, "&#39;");
}

export function appendTimestamp(url) {
    const ts = Date.now();
    return url.includes("?") ? `${url}&ts=${ts}` : `${url}?ts=${ts}`;
}

export function toErrorMessage(error) {
    if (error instanceof Error && error.message) {
        return error.message;
    }
    return String(error || "Unknown error");
}

export function formatNumber(value, digits = 2) {
    return Number.isFinite(value) ? Number(value).toFixed(digits) : "-";
}

export function formatPercent(value) {
    return `${Number(value || 0).toFixed(1)}%`;
}

export function formatSigned(value, digits = 2) {
    if (!Number.isFinite(value)) {
        return "-";
    }

    const fixed = Number(value).toFixed(digits);
    return value > 0 ? `+${fixed}` : fixed;
}

export function formatGeneratedAt(value) {
    if (!value) {
        return "Unknown";
    }

    try {
        const parsed = new Date(value);
        if (!Number.isNaN(parsed.getTime())) {
            return parsed.toLocaleString();
        }
    } catch {
        // keep raw value as fallback
    }

    return String(value);
}

export function hasValidBid(row) {
    return Number.isInteger(row?.bid) && row.bid > 0;
}

const GOT_TIER_STEPS = Object.freeze([
    { delta: -0.4, label: "low" },
    { delta: -0.2, label: "mid/low" },
    { delta: 0.0, label: "mid" },
    { delta: 0.2, label: "mid/high" },
    { delta: 0.4, label: "high" },
]);

const GOT_BASE_LABELS = Object.freeze({
    11: "Alpha",
    12: "Beta",
    13: "Gamma",
    14: "Delta",
    15: "Epsilon",
    16: "Zeta",
    17: "Eta",
    18: "Theta",
    19: "iota",
    20: "kappa",
});

function pickNearestGotTier(decimalPart) {
    let best = GOT_TIER_STEPS[0];
    let distance = Number.POSITIVE_INFINITY;

    for (const tier of GOT_TIER_STEPS) {
        const nextDistance = Math.abs(decimalPart - tier.delta);
        if (nextDistance < distance) {
            distance = nextDistance;
            best = tier;
        }
    }

    return best;
}

export function formatGotDifficultyFromNumeric(value, rowPattern) {
    if (!Number.isFinite(value)) {
        return "-";
    }

    const tier = pickNearestGotTier(value - Math.round(value));
    const base = Math.round(value - tier.delta);

    if (normalizePattern(rowPattern) === "ln") {
        return `${base} ${tier.label}`;
    }

    if (base === -2) {
        return `Intro 1 ${tier.label}`;
    }
    if (base === -1) {
        return `Intro 2 ${tier.label}`;
    }
    if (base === 0) {
        return `Intro 3 ${tier.label}`;
    }
    if (base >= 1 && base <= 10) {
        return `Reform ${base} ${tier.label}`;
    }

    const baseLabel = GOT_BASE_LABELS[base] || String(base);
    return `${baseLabel} ${tier.label}`;
}

export function sanitizeFileNameToken(value, fallback = "dataset") {
    const text = String(value ?? "").trim();
    if (!text) {
        return fallback;
    }

    const normalized = text
        .toLowerCase()
        .replace(/[^a-z0-9]+/g, "-")
        .replace(/-+/g, "-")
        .replace(/^-|-$/g, "");

    return normalized || fallback;
}

export function sanitizeNameForSearch(name) {
    let text = String(name ?? "").trim();
    if (!text) {
        return "";
    }

    let previous = "";
    while (previous !== text) {
        previous = text;
        text = text.replace(/\([^()]*\)|\[[^\[\]]*\]/g, " ");
    }

    text = text.replace(/\bx\s*\d+(?:\.\d+)?\b/gi, " ");
    text = text.replace(/\b\d+(?:\.\d+)?\s*x\b/gi, " ");
    return text.replace(/\s+/g, " ").trim();
}

export function getMapSearchUrl(name) {
    const keyword = sanitizeNameForSearch(name);
    if (!keyword) {
        return "https://osu.ppy.sh/beatmapsets?m=3&s=any";
    }

    return `https://osu.ppy.sh/beatmapsets?m=3&q=${encodeURIComponent(keyword)}&s=any`;
}

export function getBeatmapDownloadUrl(bid) {
    return `https://osu.ppy.sh/osu/${encodeURIComponent(String(bid))}`;
}
