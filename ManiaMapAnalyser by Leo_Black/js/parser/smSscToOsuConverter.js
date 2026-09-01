// .sm/.ssc → .osu v14 转换器（mania）。
//
// 共享模块（浏览器 + Node benchmark 双环境）：零 DOM 依赖。
// 时间轴语义遵循 Etterna/StepMania TimingData：事件按 BPM 分段换算毫秒后，
// 把位于其之前（含同拍）的 STOPS/DELAYS 时长累加后移（烘焙进时间戳——osu
// 无 stop 概念）；#WARPS 罕见，按折叠处理（忽略其时间影响，文档标注近似）。
// 依赖 vendor/simfile-parser（MIT，NOTICE.md 记录来源与列宽补丁）。

import { parseSm } from "./vendor/simfile-parser/parsers/parseSm.js";
import { parseSsc } from "./vendor/simfile-parser/parsers/parseSsc.js";

const CONVERT_OD = 9;
const CONVERT_HP = 8;
const CONVERT_AR = 5;

function columnToX(column, keys) {
    return Math.floor((512 * column + 256) / keys);
}

// 难度名归一化（对齐 simfile-parser 的 normalizedDifficultyMap 别名）。
const DIFFICULTY_ALIASES = { easy: "basic", trick: "difficult", another: "difficult", medium: "difficult", maniac: "expert", hard: "expert", ssr: "expert" };

function normalizeDifficulty(name) {
    const lower = String(name || "").toLowerCase();
    return DIFFICULTY_ALIASES[lower] || lower;
}

// ── 时间轴 ─────────────────────────────────────────────

// bpms: [{startOffset(measure), endOffset(null 表示末尾), bpm}] → 分段前缀表。
// 注意：vendor mergeSimilarBpmRanges 会把 BPM 差值 <1 的相邻段合并，导致合并段
// 的 endOffset 是「最后一段的末尾」，中间实际存在的 BPM 变化点丢失——若直接按
// 合并段计算，中间所有 measure 会塌缩到同一时间（Vospi 谱面 545 对象 → 12）。
// 因此这里把相邻分段补齐成连续区间（gap 用前一段 BPM 填充），保证时间单调。
function buildBpmTable(bpms) {
    const segs = [...bpms].sort((a, b) => a.startOffset - b.startOffset);
    const table = [];
    let acc = 0;
    for (let i = 0; i < segs.length; i += 1) {
        const seg = segs[i];
        const next = segs[i + 1];
        const start = seg.startOffset;
        // 本段结尾：下一段的开始（vendor 合并后可能跳过中间 BPM 点，但
        // 「时间连续」的正确做法是段与段之间无缝衔接——用 next.start）。
        const end = next ? next.startOffset : Infinity;
        if (start == null || !Number.isFinite(start)) {
            continue;
        }
        table.push({ start, end, bpm: seg.bpm, baseMs: acc });
        if (Number.isFinite(end)) {
            acc += (end - start) * 4 * 60000 / seg.bpm;
        }
    }
    return table;
}

function bpmAt(bpmTable, measure) {
    let bpm = 120;
    for (const seg of bpmTable) {
        if (measure >= seg.start) {
            bpm = seg.bpm;
        } else {
            break;
        }
    }
    return bpm;
}

// 无 stop 世界的时间（measure → ms）。
function measureToMs(bpmTable, measure) {
    let ms = 0;
    for (const seg of bpmTable) {
        if (measure >= seg.end) {
            ms = seg.baseMs + (seg.end - seg.start) * 4 * 60000 / seg.bpm;
        } else if (measure >= seg.start) {
            ms = seg.baseMs + (measure - seg.start) * 4 * 60000 / seg.bpm;
            break;
        } else {
            break;
        }
    }
    return ms;
}

// stops: [{offset(measure), duration(beats)}] → 前缀表（stop 时长按该拍 BPM 换算 ms）。
function buildStopTable(stops, bpmTable) {
    const sorted = [...stops]
        .map((s) => ({ offset: s.offset, ms: s.duration * 60000 / bpmAt(bpmTable, s.offset) }))
        .sort((a, b) => a.offset - b.offset);
    const table = [];
    let acc = 0;
    for (const item of sorted) {
        acc += item.ms;
        table.push({ offset: item.offset, accMs: acc });
    }
    return table;
}

// 烘焙：事件时间 = 无 stop 时间 + 该拍（含同拍）之前所有 stop 的毫秒和。
function bakeToMs(measure, bpmTable, stopTable) {
    const base = measureToMs(bpmTable, measure);
    let extra = 0;
    for (const stop of stopTable) {
        if (measure >= stop.offset) {
            extra = stop.accMs;
        } else {
            break;
        }
    }
    return base + extra;
}

// ── 事件抽取 ───────────────────────────────────────────

function extractNotes(chart) {
    const taps = [];
    const holds = [];
    let keys = 0;
    for (const arrow of chart.arrows || []) {
        const row = arrow.direction;
        keys = Math.max(keys, row.length);
        for (let col = 0; col < row.length; col += 1) {
            if (row[col] === "1") {
                taps.push({ col, measure: arrow.offset });
            }
        }
    }
    for (const freeze of chart.freezes || []) {
        keys = Math.max(keys, freeze.direction + 1);
        holds.push({
            col: freeze.direction,
            startMeasure: freeze.startOffset,
            endMeasure: freeze.endOffset,
        });
    }
    return { taps, holds, keys };
}

// ── 入口 ───────────────────────────────────────────────

/**
 * 把 .sm/.ssc 文本转为 .osu v14 文本。
 * @param {{text: string, format: "sm"|"ssc", difficulty?: string|null}} input
 * @returns {{osuText: string, meta: {title, artist, version, keys, noteCount, holdCount, bpmPoints}}}
 */
export function convertSmSscToOsuText({ text, format, difficulty = null } = {}) {
    const parsed = format === "ssc" ? parseSsc(text) : parseSm(text);
    let info = parsed.availableTypes[0];
    if (difficulty) {
        const wanted = normalizeDifficulty(difficulty);
        const found = parsed.availableTypes
            .find((t) => normalizeDifficulty(t.difficulty) === wanted);
        if (found) {
            info = found;
        }
    }
    if (!info || !parsed.charts[info.slug]) {
        throw new Error(`${format} 解析失败：未找到任何谱面难度块`);
    }

    const chart = parsed.charts[info.slug];
    const { taps, holds, keys } = extractNotes(chart);
    if (keys === 0) {
        throw new Error(`${format} 解析失败：无法推导键数`);
    }

    const bpmTable = buildBpmTable(chart.bpm || []);
    const stopTable = buildStopTable(chart.stops || [], bpmTable);

    const objects = [];
    const seenMs = new Set();
    // 先收集 hold 头占位（hold 优先于同拍 tap；osu 同列同 ms 只允许一个对象）。
    const holdEntries = holds.map((hold) => ({
        col: hold.col,
        startMs: bakeToMs(hold.startMeasure, bpmTable, stopTable),
        endMs: bakeToMs(hold.endMeasure, bpmTable, stopTable),
    }));
    for (const h of holdEntries) {
        const key = `${h.col}:${h.startMs}`;
        if (seenMs.has(key)) {
            continue; // 同列同 ms 重复 hold 头（罕见，osu 非法）去重
        }
        seenMs.add(key);
        objects.push(h);
    }
    for (const tap of taps) {
        const startMs = bakeToMs(tap.measure, bpmTable, stopTable);
        const key = `${tap.col}:${startMs}`;
        if (seenMs.has(key)) {
            continue; // 同列同 ms 重复 tap / 与 hold 头冲突（osu 非法）去重
        }
        seenMs.add(key);
        objects.push({ col: tap.col, startMs, endMs: null });
    }
    objects.sort((a, b) => a.startMs - b.startMs || a.col - b.col);

    // LN 尾冲突修复：同列下一条起始前 1ms 截尾；尾 ≤ 头时压为头 + 1ms（osu 同列重叠非法）。
    const byCol = new Map();
    for (const obj of objects) {
        if (!byCol.has(obj.col)) {
            byCol.set(obj.col, []);
        }
        byCol.get(obj.col).push(obj);
    }
    for (const list of byCol.values()) {
        for (let i = 0; i < list.length; i += 1) {
            const obj = list[i];
            if (obj.endMs == null) {
                continue;
            }
            const next = list[i + 1];
            const target = next ? next.startMs - 1 : obj.endMs;
            if (obj.endMs > target || obj.endMs <= obj.startMs) {
                obj.endMs = Math.max(target, obj.startMs + 1);
            }
        }
    }

    const title = parsed.title || "Unknown Title";
    const artist = parsed.artist || "Unknown Artist";
    const version = `${info.difficulty} ${info.feet}`;
    const osuText = renderOsu(title, artist, version, keys, bpmTable, stopTable, objects);
    return {
        osuText,
        meta: {
            title,
            artist,
            version,
            keys,
            noteCount: objects.length,
            holdCount: holds.length,
            bpmPoints: (chart.bpm || []).length,
        },
    };
}

function renderOsu(title, artist, version, keys, bpmTable, stopTable, objects) {
    const lines = [];
    lines.push("osu file format v14", "");
    lines.push("[General]", "AudioFilename: audio.mp3", "AudioLeadIn: 0", "PreviewTime: -1",
        "Countdown: 0", "SampleSet: Soft", "StackLeniency: 0.7", "Mode: 3",
        "LetterboxInBreaks: 0", "SpecialStyle: 0", "WidescreenStoryboard: 0", "");
    lines.push("[Editor]", "DistanceSpacing: 1.2", "BeatDivisor: 4", "GridSize: 8",
        "TimelineZoom: 2.4", "");
    lines.push("[Metadata]");
    lines.push(`Title:${title}`);
    lines.push(`Artist:${artist}`);
    lines.push("Creator:unknown");
    lines.push(`Version:${version}`);
    lines.push("Source:Etterna", "Tags:Converted from sm/ssc by LeosMma",
        "BeatmapID:0", "BeatmapSetID:-1", "");
    lines.push("[Difficulty]");
    lines.push(`HPDrainRate:${CONVERT_HP}`, `CircleSize:${keys}`, `OverallDifficulty:${CONVERT_OD}`,
        `ApproachRate:${CONVERT_AR}`, "SliderMultiplier:1.4", "SliderTickRate:1", "");
    lines.push("[Events]", "//Background and Video events", "");
    lines.push("[TimingPoints]");
    for (const seg of bpmTable) {
        if (Number.isFinite(seg.start)) {
            const beatLength = 60000 / seg.bpm;
            lines.push(`${Math.round(bakeToMs(seg.start, bpmTable, stopTable))},${beatLength.toFixed(2)},4,1,0,0,1,0`);
        }
    }
    lines.push("");
    lines.push("[HitObjects]");
    for (const obj of objects) {
        const x = columnToX(obj.col, keys);
        if (obj.endMs == null) {
            lines.push(`${x},192,${Math.round(obj.startMs)},1,0,0:0:0:0:`);
        } else {
            lines.push(`${x},192,${Math.round(obj.startMs)},128,0,${Math.round(obj.endMs)}:0:0:0:0:`);
        }
    }
    return lines.join("\n");
}