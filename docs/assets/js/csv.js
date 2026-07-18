export function toNumberOrNull(value) {
    const text = String(value ?? "").trim();
    if (!text) {
        return null;
    }
    const parsed = Number(text);
    return Number.isFinite(parsed) ? parsed : null;
}

function toBidOrNull(value) {
    const text = String(value ?? "").trim();
    if (!text) {
        return null;
    }

    const parsed = Number(text);
    if (!Number.isInteger(parsed) || parsed <= 0) {
        return null;
    }

    return parsed;
}

function parseBenchmarkLine(line) {
    const cells = String(line ?? "").split(",");
    if (cells.length < 7) {
        return null;
    }

    if (cells.length >= 8) {
        const pivot = cells.length - 6;
        return {
            bidRaw: String(cells[0] ?? "").trim(),
            name: cells.slice(1, pivot).join(",").trim(),
            pattern: String(cells[pivot] ?? "").trim(),
            subPattern: String(cells[pivot + 1] ?? "").trim(),
            expectedRaw: String(cells[pivot + 2] ?? "").trim(),
            gotRaw: String(cells[pivot + 3] ?? "").trim(),
            deltaRaw: String(cells[pivot + 4] ?? "").trim(),
            deltaAbsRaw: String(cells[pivot + 5] ?? "").trim(),
        };
    }

    const pivot = cells.length - 6;
    return {
        bidRaw: "",
        name: cells.slice(0, pivot).join(",").trim(),
        pattern: String(cells[pivot + 4] ?? "").trim(),
        subPattern: String(cells[pivot + 5] ?? "").trim(),
        expectedRaw: String(cells[pivot] ?? "").trim(),
        gotRaw: String(cells[pivot + 1] ?? "").trim(),
        deltaRaw: String(cells[pivot + 2] ?? "").trim(),
        deltaAbsRaw: String(cells[pivot + 3] ?? "").trim(),
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

        const expected = toNumberOrNull(parsed.expectedRaw);
        const got = toNumberOrNull(parsed.gotRaw);
        const hasPair = Number.isFinite(expected) && Number.isFinite(got);
        const delta = hasPair ? expected - got : null;

        rows.push({
            ...parsed,
            subPattern: parsed.subPattern || "Unsigned",
            bid: toBidOrNull(parsed.bidRaw),
            expected,
            got,
            delta,
            deltaAbs: hasPair ? Math.abs(delta) : null,
        });
    }

    return {
        header,
        rows,
    };
}
