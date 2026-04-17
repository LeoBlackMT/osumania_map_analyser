import { buildRowKey, classifyBand } from "../stats.js";
import { normalizePattern, normalizeSubPattern } from "./utils.js";

export function isRowValidForStats(row) {
    const expected = row.expected;
    const got = row.got;
    return Number.isFinite(expected) && Number.isFinite(got);
}

export function buildAlgorithmMeta(rows) {
    const lnRows = rows.filter((row) => normalizePattern(row.pattern) === "ln");
    const lnValidRows = lnRows.filter((row) => isRowValidForStats(row));

    return {
        lnTotal: lnRows.length,
        lnValid: lnValidRows.length,
        hasUsableLn: lnRows.length > 0 && lnValidRows.length > 0,
    };
}

export function normalizeLoadedRows(rows) {
    return rows.map((row) => ({
        ...row,
        pattern: String(row.pattern ?? "").trim(),
        subPattern: normalizeSubPattern(row.subPattern),
        gotRaw: String(row.gotRaw ?? "").trim(),
    }));
}

export function parseErrorInfoFromRawGot(rawGot) {
    const raw = String(rawGot ?? "").trim();

    if (!raw) {
        return {
            type: "Failed",
            detail: "Got value is empty.",
            raw,
        };
    }

    const invalidMatch = raw.match(/^invalid\b\s*[:：-]?\s*(.*)$/i);
    if (invalidMatch) {
        return {
            type: "Invalid",
            detail: invalidMatch[1] ? invalidMatch[1].trim() : "Difficulty label contains bound symbol.",
            raw,
        };
    }

    const failedMatch = raw.match(/^failed\b\s*[:：-]?\s*(.*)$/i);
    if (failedMatch) {
        return {
            type: "Failed",
            detail: failedMatch[1] ? failedMatch[1].trim() : "Estimator execution or parsing failed.",
            raw,
        };
    }

    const missingMatch = raw.match(/^missing\b\s*[:：-]?\s*(.*)$/i);
    if (missingMatch) {
        return {
            type: "Missing",
            detail: missingMatch[1]
                ? missingMatch[1].trim()
                : "Local map file is missing.",
            raw,
        };
    }

    return {
        type: "Failed",
        detail: raw,
        raw,
    };
}

export function getRowErrorInfo(row) {
    if (row?._errorInfo !== undefined) {
        return row._errorInfo;
    }

    const got = row.got;
    if (Number.isFinite(got)) {
        return null;
    }

    return parseErrorInfoFromRawGot(row.gotRaw);
}

export function getRowBand(row) {
    if (row?._band) {
        return row._band;
    }

    if (getRowErrorInfo(row)) {
        return "error";
    }

    return classifyBand(row.deltaAbs);
}

export function getWinnerLabel(better, compareEnabled) {
    if (!compareEnabled) {
        return "-";
    }

    if (better === "base") {
        return "Base";
    }
    if (better === "compare") {
        return "Compare";
    }
    if (better === "tie") {
        return "Tie";
    }
    return "-";
}

export function applyScopeRows(rows, scope, lnScope) {
    if (String(scope).toUpperCase() === String(lnScope).toUpperCase()) {
        return rows.filter((row) => normalizePattern(row.pattern) === "ln");
    }

    return rows.filter((row) => normalizePattern(row.pattern) !== "ln");
}

function decorateDisplayRow(row) {
    const normalizedSubPattern = normalizeSubPattern(row.subPattern);
    const errorInfo = Number.isFinite(row.got) ? null : parseErrorInfoFromRawGot(row.gotRaw);
    const band = errorInfo ? "error" : classifyBand(row.deltaAbs);

    const searchHaystack = [
        row.name,
        row.pattern,
        normalizedSubPattern,
        row.gotRaw,
        errorInfo?.type,
        errorInfo?.detail,
    ]
        .map((part) => String(part || "").toLowerCase())
        .join(" ");

    return {
        ...row,
        subPattern: normalizedSubPattern,
        _normalizedSubPattern: normalizedSubPattern,
        _errorInfo: errorInfo,
        _band: band,
        _searchHaystack: searchHaystack,
    };
}

export function mergeRowsForDisplay(baseRows, compareRows, compareEnabled) {
    if (!compareEnabled || !Array.isArray(compareRows) || compareRows.length === 0) {
        return baseRows.map((row) => decorateDisplayRow({
            ...row,
            compareGot: null,
            compareGotRaw: "",
            compareDeltaAbs: null,
            better: "na",
        }));
    }

    const compareMap = new Map(compareRows.map((row) => [buildRowKey(row), row]));

    return baseRows.map((row) => {
        const peer = compareMap.get(buildRowKey(row));
        if (!peer) {
            return decorateDisplayRow({
                ...row,
                compareGot: null,
                compareGotRaw: "",
                compareDeltaAbs: null,
                better: "na",
            });
        }

        const baseDeltaAbs = Number.isFinite(row.deltaAbs) ? row.deltaAbs : null;
        const compareDeltaAbs = Number.isFinite(peer.deltaAbs) ? peer.deltaAbs : null;

        let better = "na";
        if (Number.isFinite(baseDeltaAbs) && Number.isFinite(compareDeltaAbs)) {
            if (baseDeltaAbs < compareDeltaAbs) {
                better = "base";
            } else if (baseDeltaAbs > compareDeltaAbs) {
                better = "compare";
            } else {
                better = "tie";
            }
        }

        return decorateDisplayRow({
            ...row,
            compareGot: Number.isFinite(peer.got) ? peer.got : null,
            compareGotRaw: String(peer.gotRaw || ""),
            compareDeltaAbs,
            better,
        });
    });
}

export function getActiveFilters(dom) {
    return {
        searchText: String(dom.searchInput.value || "").trim().toLowerCase(),
        pattern: dom.patternFilter.value,
        subPattern: dom.subPatternFilter.value,
        band: dom.bandFilter.value,
    };
}

export function hasActiveFilters(filters) {
    return Boolean(
        filters.searchText
        || (filters.pattern && filters.pattern !== "all")
        || (filters.subPattern && filters.subPattern !== "all")
        || (filters.band && filters.band !== "all"),
    );
}

function matchesFilters(row, filters) {
    if (filters.pattern && filters.pattern !== "all" && row.pattern !== filters.pattern) {
        return false;
    }

    if (
        filters.subPattern
        && filters.subPattern !== "all"
        && row._normalizedSubPattern !== filters.subPattern
    ) {
        return false;
    }

    if (filters.band && filters.band !== "all" && row._band !== filters.band) {
        return false;
    }

    if (filters.searchText && !row._searchHaystack.includes(filters.searchText)) {
        return false;
    }

    return true;
}

export function filterDisplayRows(rows, filters) {
    return rows.filter((row) => matchesFilters(row, filters));
}

function compareValues(a, b, key) {
    if (key === "band") {
        const rank = {
            exact: 0,
            close: 1,
            moderate: 2,
            miss: 3,
            error: 4,
        };
        return (rank[a._band] ?? 99) - (rank[b._band] ?? 99);
    }

    if (key === "better") {
        const rank = { base: 0, compare: 1, tie: 2, na: 3 };
        return (rank[a.better] ?? 3) - (rank[b.better] ?? 3);
    }

    if (key === "subPattern") {
        return String(a._normalizedSubPattern || "").localeCompare(String(b._normalizedSubPattern || ""));
    }

    const numericKeys = new Set(["expected", "got", "delta", "deltaAbs", "compareGot", "compareDeltaAbs"]);
    if (numericKeys.has(key)) {
        const aVal = a[key];
        const bVal = b[key];
        const aFinite = Number.isFinite(aVal);
        const bFinite = Number.isFinite(bVal);

        if (!aFinite && !bFinite) {
            return String(a[key] ?? "").localeCompare(String(b[key] ?? ""));
        }
        if (!aFinite) {
            return 1;
        }
        if (!bFinite) {
            return -1;
        }
        return aVal - bVal;
    }

    return String(a[key] ?? "").localeCompare(String(b[key] ?? ""));
}

export function sortRows(rows, sortKey, sortDirection) {
    const sorted = [...rows];
    sorted.sort((a, b) => {
        const aErrorPriority = a._errorInfo ? 1 : 0;
        const bErrorPriority = b._errorInfo ? 1 : 0;
        if (aErrorPriority !== bErrorPriority) {
            return aErrorPriority - bErrorPriority;
        }

        const base = compareValues(a, b, sortKey);
        return sortDirection === "asc" ? base : -base;
    });
    return sorted;
}

export function pickFilteredCompareRows(filteredBaseRows, scopedCompareRows) {
    const keys = new Set(filteredBaseRows.map((row) => buildRowKey(row)));
    return scopedCompareRows.filter((row) => keys.has(buildRowKey(row)));
}
