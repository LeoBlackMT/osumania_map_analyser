export function toNumberOrNull(value) {
    const parsed = Number(String(value ?? "").trim());
    return Number.isFinite(parsed) ? parsed : null;
}

function parseBenchmarkLine(line) {
    const cells = String(line ?? "").split(",");
    if (cells.length < 7) {
        return null;
    }

    const pivot = cells.length - 6;
    const name = cells.slice(0, pivot).join(",").trim();

    return {
        name,
        expectedRaw: String(cells[pivot] ?? "").trim(),
        gotRaw: String(cells[pivot + 1] ?? "").trim(),
        deltaRaw: String(cells[pivot + 2] ?? "").trim(),
        deltaAbsRaw: String(cells[pivot + 3] ?? "").trim(),
        pattern: String(cells[pivot + 4] ?? "").trim(),
        subPattern: String(cells[pivot + 5] ?? "").trim(),
    };
}

export function parseBenchmarkCsv(csvText) {
    const normalized = String(csvText ?? "").replace(/\r\n/g, "\n");
    const lines = normalized.split("\n");
    const header = String(lines[0] ?? "").trim();

    const rows = [];
    for (let index = 1; index < lines.length; index += 1) {
        const line = String(lines[index] ?? "");
        if (!line.trim()) {
            continue;
        }

        const parsed = parseBenchmarkLine(line);
        if (!parsed || !parsed.name) {
            continue;
        }

        rows.push({
            ...parsed,
            expected: toNumberOrNull(parsed.expectedRaw),
            got: toNumberOrNull(parsed.gotRaw),
            delta: toNumberOrNull(parsed.deltaRaw),
            deltaAbs: toNumberOrNull(parsed.deltaAbsRaw),
        });
    }

    return {
        header,
        rows,
    };
}
