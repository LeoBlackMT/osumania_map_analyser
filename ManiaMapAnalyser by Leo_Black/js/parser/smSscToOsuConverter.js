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
// 额外兼容 Etterna `Steps:GetDifficulty()` 返回的 `Difficulty_*` 前缀枚举
// （桥文件 difficulty=Difficulty_Hard 等）——切难度时若不解前缀，
// `difficulty_hard` 匹配不到 .sm 块内无前缀的 hard → 恒回退首块（同谱面
// 多难度只分析第一个难度的根因）。
const DIFFICULTY_ALIASES = { easy: "basic", trick: "difficult", another: "difficult", medium: "difficult", maniac: "expert", hard: "expert", ssr: "expert" };

function normalizeDifficulty(name) {
    const lower = String(name || "").toLowerCase();
    // Etterna 枚举：Difficulty_Beginner/Basic/Difficult/Expert/Challenge/Edit
    const stripped = lower.startsWith("difficulty_") ? lower.slice("difficulty_".length) : lower;
    return DIFFICULTY_ALIASES[stripped] || stripped;
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
        const row = String(arrow.direction || "");
        // vendor 会把注释行（"// measure 1"）也放进 arrows——跳过，避免
        // 把注释长度当列数（Tori 等 4K 谱 keys 被撑到 12 的根因）。
        if (row.startsWith("//") || !/^[0123MKL]+$/.test(row.trim())) {
            continue;
        }
        const cols = row.trim();
        keys = Math.max(keys, cols.length);
        for (let col = 0; col < cols.length; col += 1) {
            if (cols[col] === "1") {
                taps.push({ col, measure: arrow.offset });
            }
        }
    }
    for (const freeze of chart.freezes || []) {
        keys = Math.max(keys, Number(freeze.direction) + 1);
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
 * 预处理：把 `#TAG:value` 中跨行的值合并（Etterna 允许 BPM/STOPS 等 tag 值
 * 换行续写，如 `#BPMS:0.000=576.923\n,4.000=276.000`）。vendor 按行解析 tag，
 * 跨行值会被截断成只有第一行 → 后续 BPM 全丢 → 时间轴塌缩、星数严重偏高
 * （Kami Teki Souzou 实测 5 段 BPM 只剩 1 段，>Theta High 的根因）。
 *
 * 只处理「值未以分号结束的行」：把该行与后续以逗号/分号继续的行连接，
 * 直到遇到分号或下一个 `#TAG`。`#NOTES` 块（含 `#NOTES:` 行）保持原样。
 */
function normalizeMultilineTags(text) {
    const lines = text.split(/\r?\n/);
    const out = [];
    let inNotes = false;
    for (let i = 0; i < lines.length; i += 1) {
        const line = lines[i];
        const trimmed = line.trim();
        if (trimmed.startsWith("#NOTES")) {
            inNotes = true;
            out.push(line);
            continue;
        }
        if (inNotes) {
            out.push(line);
            // #NOTES 块以行首分号结束（vendor 的 concludesANoteTag）。
            if (trimmed === ";") {
                inNotes = false;
            }
            continue;
        }
        // 非 #NOTES：若行含 tag 且值未闭合（无分号），往后合并续行。
        const tagMatch = /^#([A-Za-z]+)\s*:\s*(.*)$/.exec(trimmed);
        if (tagMatch && !line.includes(";")) {
            let merged = line;
            let j = i + 1;
            while (j < lines.length) {
                const next = lines[j];
                const nextTrimmed = next.trim();
                // 下一个 #TAG 或 #NOTES 开头 → 停止（本 tag 缺分号，保原样）。
                if (/^#/.test(nextTrimmed)) {
                    break;
                }
                merged += next;
                if (next.includes(";")) {
                    j += 1;
                    break;
                }
                j += 1;
            }
            out.push(merged);
            i = j - 1;
            continue;
        }
        out.push(line);
    }
    return out.join("\n");
}

/**
 * 把 .sm/.ssc 文本转为 .osu v14 文本。
 * @param {{text: string, format: "sm"|"ssc", difficulty?: string|null}} input
 * @returns {{osuText: string, meta: {title, artist, version, keys, noteCount, holdCount, bpmPoints}}}
 */
export function convertSmSscToOsuText({ text, format, difficulty = null } = {}) {
    const normalized = normalizeMultilineTags(text);
    const parsed = format === "ssc" ? parseSsc(normalized) : parseSm(normalized);
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
    // #OFFSET（秒）→ 时间轴整体偏移（ms）。Etterna 谱面常用负偏移对齐音频；
    // 不处理会让所有 note 时间偏差 offset 秒（Kami Teki 等谱面星数偏高）。
    const offsetMs = (() => {
        const m = /^#OFFSET\s*:\s*(-?[\d.]+)/m.exec(normalized);
        if (!m) return 0;
        const sec = Number(m[1]);
        return Number.isFinite(sec) ? Math.round(sec * 1000) : 0;
    })();
    const osuText = renderOsu(title, artist, version, keys, bpmTable, stopTable, objects, offsetMs);
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

function renderOsu(title, artist, version, keys, bpmTable, stopTable, objects, offsetMs = 0) {
    // 负时间保护：OFFSET 平移后最早对象可能为负（Sunny 等估算器对负时间
    // 崩溃 → star NaN → 卡片 No data/Etterna Analyze Failed）。整体再平移
    // 使最早对象落在 0，时间间隔不变（星数不受影响）。
    let minMs = Infinity;
    for (const obj of objects) {
        const start = Math.round(obj.startMs) + offsetMs;
        if (start < minMs) minMs = start;
    }
    const extraShift = Number.isFinite(minMs) && minMs < 0 ? -minMs : 0;

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
            lines.push(`${Math.round(bakeToMs(seg.start, bpmTable, stopTable)) + offsetMs + extraShift},${beatLength.toFixed(2)},4,1,0,0,1,0`);
        }
    }
    lines.push("");
    lines.push("[HitObjects]");
    for (const obj of objects) {
        const x = columnToX(obj.col, keys);
        const start = Math.round(obj.startMs) + offsetMs + extraShift;
        if (obj.endMs == null) {
            lines.push(`${x},192,${start},1,0,0:0:0:0:`);
        } else {
            const end = Math.max(Math.round(obj.endMs) + offsetMs + extraShift, start + 1);
            lines.push(`${x},192,${start},128,0,${end}:0:0:0:0:`);
        }
    }
    return lines.join("\n");
}