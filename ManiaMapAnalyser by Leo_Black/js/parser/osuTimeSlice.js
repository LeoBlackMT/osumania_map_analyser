// .osu 文本按时间区间切片（纯函数：DOM-free / state-free / 零依赖，Node 可测）。
// 用途：桌面壳「特定时间点谱面分析」——把整张谱面截成 [startMs, endMs] 片段后交给
// runAnalysisPipeline 全链路重算。规则见 docs/features/graph-section-analysis.md。
// 三条铁律：
//   1. 头信息（[General]/[Editor]/[Metadata]/[Difficulty]/[Colours]）原样保留——
//      Mode/CircleSize/OverallDifficulty 一变，键数/OD/估算语义全歪。
//   2. 时间戳不平移：所有算法只吃相对差值，平移只会引入额外偏移风险。
//   3. 长条（type & 128）按「区间交集」保留，不截头不截尾。

const KEEP_AS_IS_SECTIONS = new Set([
    "[General]",
    "[Editor]",
    "[Metadata]",
    "[Difficulty]",
    "[Colours]",
]);

const SECTION_EVENTS = "[Events]";
const SECTION_TIMING = "[TimingPoints]";
const SECTION_OBJECTS = "[HitObjects]";

const TIMING_UNINHERITED_INDEX = 6;

function isSectionHeader(line) {
    return line.startsWith("[") && line.endsWith("]");
}

/** 解析一条 timing point。返回 { t, uninherited }；无法解析返回 null。 */
function parseTimingPoint(line) {
    const parts = line.split(",");
    if (parts.length < 2) {
        return null;
    }
    const t = Number.parseFloat(parts[0]);
    const beatLength = Number.parseFloat(parts[1]);
    if (!Number.isFinite(t) || !Number.isFinite(beatLength)) {
        return null;
    }
    // 第 7 列为 uninherited；缺列时 osu 语义为 1（红线）。
    const raw = parts.length > TIMING_UNINHERITED_INDEX && parts[TIMING_UNINHERITED_INDEX]
        ? Number.parseInt(parts[TIMING_UNINHERITED_INDEX], 10)
        : 1;
    return { t: Math.trunc(t), uninherited: raw === 1 };
}

/** 解析一条 hit object 的 [start, end]（LN 取第 6 段冒号前的结束时间）。 */
function parseObjectWindow(line) {
    const parts = line.split(",");
    if (parts.length < 4) {
        return null;
    }
    const start = Number.parseInt(parts[2], 10);
    const type = Number.parseInt(parts[3], 10);
    if (!Number.isFinite(start) || !Number.isFinite(type)) {
        return null;
    }
    let end = start;
    if ((type & 128) !== 0 && parts.length >= 6) {
        const endRaw = Number.parseInt(String(parts[5]).split(":")[0], 10);
        if (Number.isFinite(endRaw)) {
            end = endRaw;
        }
    }
    return { start, end };
}

/** 解析一条 Break 事件（`2,start,end` 或 `Break,start,end`）。 */
function parseBreakWindow(line) {
    const parts = line.split(",");
    if (parts.length < 3) {
        return null;
    }
    const head = parts[0].trim();
    if (head !== "2" && head !== "Break") {
        return null;
    }
    const start = Number.parseInt(parts[1], 10);
    const end = Number.parseInt(parts[2], 10);
    if (!Number.isFinite(start) || !Number.isFinite(end)) {
        return null;
    }
    return { start, end };
}

/**
 * 按区间截取 .osu 文本。
 * @param {string} rawText .osu 原文
 * @param {number} startMs 物理谱面起始毫秒（含）
 * @param {number} endMs   物理谱面结束毫秒（含）
 * @returns {{ text: string, objectCount: number, timingPointCount: number }}
 */
export function sliceOsuTextByTimeRange(rawText, startMs, endMs) {
    const text = typeof rawText === "string" ? rawText : "";
    const a = Number(startMs);
    const b = Number(endMs);
    if (!Number.isFinite(a) || !Number.isFinite(b)) {
        return { text, objectCount: 0, timingPointCount: 0 };
    }
    const rangeStart = Math.min(a, b);
    const rangeEnd = Math.max(a, b);

    const lines = text.split(/\r?\n/);
    const sections = [];
    let current = { header: "", lines: [] };
    for (const line of lines) {
        if (isSectionHeader(line)) {
            if (current.header || current.lines.length > 0) {
                sections.push(current);
            }
            current = { header: line.trim(), lines: [] };
            continue;
        }
        current.lines.push(line);
    }
    if (current.header || current.lines.length > 0) {
        sections.push(current);
    }

    // 先算全谱的 timing points：区间前最后一条红线/绿线要带进切片。
    const allTiming = [];
    for (const section of sections) {
        if (section.header !== SECTION_TIMING) {
            continue;
        }
        for (const line of section.lines) {
            const parsed = parseTimingPoint(line);
            if (parsed) {
                allTiming.push({ ...parsed, raw: line });
            }
        }
    }

    let objectCount = 0;
    let timingPointCount = 0;
    const output = [];

    for (const section of sections) {
        if (section.header) {
            output.push(section.header);
        }

        if (section.header === SECTION_TIMING) {
            const kept = selectTimingPoints(allTiming, rangeStart, rangeEnd);
            for (const point of kept) {
                output.push(point.raw);
            }
            timingPointCount += kept.length;
            continue;
        }

        if (section.header === SECTION_OBJECTS) {
            for (const line of section.lines) {
                if (!line.trim()) {
                    continue;
                }
                const window = parseObjectWindow(line);
                if (!window) {
                    output.push(line);
                    continue;
                }
                if (window.start <= rangeEnd && window.end >= rangeStart) {
                    output.push(line);
                    objectCount += 1;
                }
            }
            continue;
        }

        if (section.header === SECTION_EVENTS) {
            for (const line of section.lines) {
                if (!line.trim()) {
                    continue;
                }
                const window = parseBreakWindow(line);
                if (!window) {
                    output.push(line);
                    continue;
                }
                if (window.start <= rangeEnd && window.end >= rangeStart) {
                    output.push(line);
                }
            }
            continue;
        }

        if (section.header && !KEEP_AS_IS_SECTIONS.has(section.header)) {
            // 未知段前向兼容：原样保留（不让新段类型在切片里消失）。
            for (const line of section.lines) {
                if (line.trim()) {
                    output.push(line);
                }
            }
            continue;
        }

        // 头信息段：原样保留（保留空行也无所谓，这里统一去掉行尾空行）。
        for (const line of section.lines) {
            output.push(line);
        }
    }

    while (output.length > 0 && output[output.length - 1] === "") {
        output.pop();
    }

    return {
        text: `${output.join("\n")}\n`,
        objectCount,
        timingPointCount,
    };
}

/**
 * 选出切片要保留的 timing points：
 *   - 区间内的全部点（红线/绿线都留，SV 与 BPM 都要）；
 *   - 区间前最后一条红线（BPM 基准，缺了会退回 120）；
 *   - 区间前最后一条绿线（SV 状态延续，LN 子评分依赖它）。
 * 若文件里没有任何红线 ≤ start，则补文件首条红线（osu 语义：首红线向前生效）。
 */
function selectTimingPoints(allTiming, rangeStart, rangeEnd) {
    if (allTiming.length === 0) {
        return [];
    }

    const keepIndexes = new Set();
    let lastUninheritedBefore = -1;
    let lastInheritedBefore = -1;
    let firstUninherited = -1;

    for (let i = 0; i < allTiming.length; i += 1) {
        const point = allTiming[i];
        if (point.uninherited && firstUninherited < 0) {
            firstUninherited = i;
        }
        if (point.t >= rangeStart && point.t <= rangeEnd) {
            keepIndexes.add(i);
            continue;
        }
        if (point.t < rangeStart) {
            if (point.uninherited) {
                lastUninheritedBefore = i;
            } else {
                lastInheritedBefore = i;
            }
        }
    }

    if (lastUninheritedBefore >= 0) {
        keepIndexes.add(lastUninheritedBefore);
    } else if (firstUninherited >= 0) {
        keepIndexes.add(firstUninherited);
    }
    if (lastInheritedBefore >= 0) {
        keepIndexes.add(lastInheritedBefore);
    }

    return [...keepIndexes]
        .sort((ia, ib) => (allTiming[ia].t - allTiming[ib].t) || (ia - ib))
        .map((index) => allTiming[index]);
}
