// 整图 vibro 检测（移植自 mania-hub `live-backend/src/dan/vibro-detection.ts`）
//
// 移植范围：**整张图**级别的判定，即 rice 六档 + LN vibro + 按速率臂。
// 刻意**未移植**段落模型（`vibro-sections.ts`）与动作学臂（`vibro-motion.ts`）：
// 源文件在 4K rice 上会委派给段落模型（`usesSectionVibro` → `analyzeVibroSections`），
// 本模块把 4K rice 也交给六档阶梯处理（等于源文件对非 4K / hold 偏多谱面的旧策略）。
//
// 阈值与注释逐字保留源文件语义（含语料实测依据）；数值未做任何调整。
// 本模块为共享纯函数：禁止 window/document，禁止 import js/app/。

import { APP_CONFIG } from "../../config.js";

// LN vibro: chart-wide staggered hold spam (the "gabe power" shape - dense LN
// rolls you play by shaking, not reading). The longjack detector only sees
// rice jack clusters, so these charts sailed through with inflated LN dans.
// A p75 row gap this tight sustained over a whole chart is beyond any legit
// LN chart: the densest ranked LN dumps (Denouement) sit at ~75ms rows, the
// calibration corpus bottoms out at 54ms p50 / 76ms p75, vibro at 22ms.
const LN_VIBRO_MIN_ROWS = 150;
const LN_VIBRO_MIN_HOLD_RATIO = 0.5;
const LN_VIBRO_MAX_P75_ROW_GAP_MS = 40;

// Rice vibro measured directly from note timing, because the longjack-cluster
// detector only fires on clusters labeled "Longjacks": chord-wall vibro reads
// as chordjack/quadstream and sailed through (Tamania's "impossible vibro pack"
// indexed as beta++ jack). Thresholds were calibrated against the local corpus:
// vibro-titled packs vs ranked jack files and celebrated dense charts
// (Gengaozo Innocence 1.05x, STRONG 280 1.1x, hurricanic 1.2x all stay clean).
//
// Tier 1 (any keymode): sustained same-column hammering. A run of 24+ hits with
// gaps <= 92ms (~11/s) is beyond human jacking when a quarter of the chart's
// column gaps are that fast; legit speedjack bursts stay under ~16 hits and
// ranked jack files measure runs <= 6.
const RICE_VIBRO_MIN_NOTES = 300;
const RICE_VIBRO_COLUMN_GAP_MS = 92;
const RICE_VIBRO_COLUMN_MIN_RUN = 24;
const RICE_VIBRO_COLUMN_MIN_RATIO = 0.25;
// Tier 2 (4K only): slower chord-wall vibro (~97-105ms quads you shake, not
// jack). Needs both recurring 4-note wall rows and a chart soaked in fast
// column repeats; dense legit 4K charts top out at ~3.3% wall rows, and the
// legit charts that do carry wall rows keep their column repeats at 105ms+,
// so their <=98ms column ratio measures 0.0 - the 0.32 floor has full margin.
const RICE_VIBRO_WALL_GAP_MS = 105;
const RICE_VIBRO_WALL_MIN_ROWS = 12;
const RICE_VIBRO_WALL_MIN_ROW_RATIO = 0.035;
const RICE_VIBRO_WALL_COLUMN_GAP_MS = 98;
const RICE_VIBRO_WALL_COLUMN_MIN_RATIO = 0.32;
// Tier 3 (any keymode): burst-soak vibro. Packs full of 8-23-note same-column
// bursts at <=100ms slip tiers 1-2 (runs too short for tier 1, no quad walls
// for tier 2), but a chart where a fifth of all column gaps sit inside such
// runs is nothing but bursts. Legit files with occasional speedjack stay far
// under: the calibration corpus's densest unflagged charts (William Tell EX
// piano rolls, Gengaozo 7K Z O) measure ~0.13, ranked jack files ~0.
const RICE_VIBRO_BURST_MIN_NOTES = 200;
const RICE_VIBRO_BURST_GAP_MS = 100;
const RICE_VIBRO_BURST_MIN_RUN = 8;
const RICE_VIBRO_BURST_MIN_RUNS = 4;
const RICE_VIBRO_BURST_MIN_FRACTION = 0.2;
// Tier 4 (any keymode): superhuman row density. Tiers 1-3 all measure repeats
// within a column, so a chart that sprays its spam across columns as jumps or
// quads slips every one of them - the "Hello (BPM) 2023" shape, where 4
// seconds of 15ms jumps closing an otherwise ordinary LN chart carry 19% of
// the notes and drag MinaCalc's chordjack from 22 to 76. Rows rather than
// notes, because a wide chord is one action: 7K "This Future" peaks at 91
// notes/s but only 13 rows/s and is entirely legit. Measured across every
// analyzed ranked and loved 4/6/7K chart (n=27,892), peak rows/s tops out at
// 55 (4K), 57 (7K) and 49 (6K), so 65 clears the corpus by 14%; it fires on
// 88 of 128,784 analyzed charts, none of them ranked or loved.
const RICE_VIBRO_ROW_RATE_WINDOW_MS = 1000;
const RICE_VIBRO_MAX_ROWS_PER_SECOND = 65;
// Tier 5 (any keymode): chord jacks faster than a hand can jack. Tier 2 only
// counts *quad* pairs, so a 280BPM file alternating triples and quads slips
// it (the "Buddah Attachments [280BPM CJ]" shape measures 0.027 against that
// tier's 0.035 floor) while tier 1's run test misses because no single column
// ever holds 24 consecutive fast gaps - the chart spreads them. Measuring
// chord rows directly catches the whole jack-pack family: adjacent rows that
// both carry a near-full chord inside 70ms, which is a 214BPM chord jack, as
// a share of all row transitions. Chord size scales with the keymode because
// a 3-note chord is a wall in 4K and everyday density in 7K. Across every
// analyzed ranked and loved chart this tops out at 0.0040 (4K, n=20,735),
// 0.0000 (6K) and 0.0006 (7K), so 0.02 clears the corpus five times over; it
// fires on 0.32% of analyzed 4K charts, none of them ranked or loved.
const RICE_VIBRO_CHORD_WALL_GAP_MS = 70;
const RICE_VIBRO_CHORD_WALL_MIN_RATIO = 0.02;
// Tier 6 (4K only): roll vibro, the per-finger speed of the chart's rolls at
// the played rate. A 163BPM 1/16 four-column roll hits each finger every 92ms
// and breaks every 8-9 notes, which every tier above lets through - and at 1.5x
// it is a 61ms per-finger shake nobody rolls. Two measures, both at the played
// rate, and both have to hold.
//  - per-finger: the share of all column gaps at or under 70ms (~15/s per
//    finger).
//  - roll: the share of row transitions at or under 25ms that move to other
//    columns (same-column flams do not count): a jack is one finger, a roll is
//    the whole hand cycling.
// No ranked or loved 4K chart meets both at 1.0x. 4K only: ranked 7K carries
// 55ms column repeats routinely, so the per-finger measure says nothing there.
const RICE_VIBRO_ROLL_GAP_MS = 70;
const RICE_VIBRO_ROLL_MIN_RATIO = 0.25;
const RICE_VIBRO_ROLL_ROW_GAP_MS = 25;
const RICE_VIBRO_ROLL_MIN_ROW_RATIO = 0.3;

// A rate can also turn ordinary chordjack into vibro without ever looking like
// a roll: 128BPM chord walls become 218BPM at 1.7x, and in the reported shape
// 68% of all row transitions are then near-full chords inside tier 5's 70ms
// window. The ordinary tier-5 floor (2%) is deliberately too broad for
// play-side rate checks: scaling it catches a small fast chordjack burst in
// otherwise legit DT files. Requiring half of the whole chart says the
// superhuman chord wall IS the chart.
const RATE_VIBRO_CHORD_WALL_MIN_RATIO = 0.5;

// Repeated chords can reload two fingers on every row while rotating the
// third finger out. Individual jack runs stay short and near-full chord pairs
// need not occupy half the chart. Require speed, prevalence and sustained work
// together so isolated DT bursts and ordinary fast chordjack stay eligible.
// Calibrated on 4K rice (<=10% holds): no matches among 8,090 unique ranked/
// loved charts at 1.0x or 5,599 with a recorded 96%+ DT clear.
const SUSTAINED_CHORD_VIBRO_BANDS = [
    { gapMs: 70, columnShare: 0.4 },
    { gapMs: 60, columnShare: 0.35 },
];
const SUSTAINED_CHORD_VIBRO_MIN_ROW_SHARE = 0.2;
// 32 consecutive chord rows are about two seconds near the 70ms boundary.
const SUSTAINED_CHORD_VIBRO_MIN_ROWS = 32;
const SUSTAINED_CHORD_VIBRO_MAX_HOLD_RATIO = 0.1;

// 判定原因标识（用于展示/调试；名称与源文件的档位一一对应）
export const VIBRO_REASON = {
    riceSustainedColumn: "rice_sustained_column",
    riceSlowChordWall: "rice_slow_chord_wall",
    riceBurstSoak: "rice_burst_soak",
    riceRowDensity: "rice_row_density",
    riceSustainedChord: "rice_sustained_chord",
    riceChordWall: "rice_chord_wall",
    riceRoll: "rice_roll",
    lnStaggeredSpam: "ln_staggered_spam",
    metadataKeyword: "metadata_keyword",
};

function columnFastGaps(map, cutoffMs) {
    const byColumn = new Map();
    for (const note of map.notes) {
        const list = byColumn.get(note.column) ?? [];
        list.push(note.time);
        byColumn.set(note.column, list);
    }
    let maxRun = 0;
    let fast = 0;
    let total = 0;
    for (const times of byColumn.values()) {
        times.sort((a, b) => a - b);
        let run = 0;
        for (let index = 1; index < times.length; index++) {
            const gap = times[index] - times[index - 1];
            if (gap <= 0) continue;
            total++;
            if (gap <= cutoffMs) {
                fast++;
                run++;
                if (run > maxRun) maxRun = run;
            } else {
                run = 0;
            }
        }
    }
    return { maxRun, ratio: total > 0 ? fast / total : 0 };
}

// Share of row-to-row transitions at or under cutoffMs that move to other
// columns. A roll cycles the hand, so consecutive rows share no column; a
// same-column pair that close is a flam or a stack, which is not the shape
// this measures.
function fastRollRowShare(map, cutoffMs) {
    const rows = new Map();
    for (const note of map.notes) {
        const columns = rows.get(note.time) ?? new Set();
        columns.add(note.column);
        rows.set(note.time, columns);
    }
    const times = [...rows.keys()].sort((a, b) => a - b);
    if (times.length < 2) return 0;
    let fast = 0;
    for (let index = 1; index < times.length; index++) {
        if (times[index] - times[index - 1] > cutoffMs) continue;
        const previous = rows.get(times[index - 1]);
        let shared = false;
        for (const column of rows.get(times[index])) {
            if (previous.has(column)) {
                shared = true;
                break;
            }
        }
        if (!shared) fast++;
    }
    return fast / (times.length - 1);
}

function columnBurstRuns(map, cutoffMs, minRun) {
    const byColumn = new Map();
    for (const note of map.notes) {
        const list = byColumn.get(note.column) ?? [];
        list.push(note.time);
        byColumn.set(note.column, list);
    }
    let runs = 0;
    let inRuns = 0;
    let total = 0;
    for (const times of byColumn.values()) {
        times.sort((a, b) => a - b);
        let run = 0;
        const flush = () => {
            if (run >= minRun) {
                runs++;
                inRuns += run;
            }
            run = 0;
        };
        for (let index = 1; index < times.length; index++) {
            const gap = times[index] - times[index - 1];
            if (gap <= 0) continue;
            total++;
            if (gap <= cutoffMs) run++;
            else flush();
        }
        flush();
    }
    return { runs, fraction: total > 0 ? inRuns / total : 0 };
}

function quadWallRows(map, cutoffMs) {
    const rowSizes = new Map();
    for (const note of map.notes) rowSizes.set(note.time, (rowSizes.get(note.time) ?? 0) + 1);
    const times = [...rowSizes.keys()].sort((a, b) => a - b);
    let rows = 0;
    for (let index = 1; index < times.length; index++) {
        const gap = times[index] - times[index - 1];
        if (gap <= 0 || gap > cutoffMs) continue;
        if ((rowSizes.get(times[index]) ?? 0) >= 4 && (rowSizes.get(times[index - 1]) ?? 0) >= 4) rows++;
    }
    return { rows, ratio: times.length > 1 ? rows / (times.length - 1) : 0 };
}

// Share of row transitions where both rows carry a near-full chord and sit
// inside cutoffMs (the tier-5 chord-jack signal).
function chordWallRatio(map, cutoffMs) {
    const rowSizes = new Map();
    for (const note of map.notes) rowSizes.set(note.time, (rowSizes.get(note.time) ?? 0) + 1);
    const times = [...rowSizes.keys()].sort((a, b) => a - b);
    if (times.length < 2) return 0;
    const minChord = Math.max(2, map.keyCount - 1);
    let walls = 0;
    for (let index = 1; index < times.length; index++) {
        const gap = times[index] - times[index - 1];
        if (gap <= 0 || gap > cutoffMs) continue;
        if ((rowSizes.get(times[index]) ?? 0) >= minChord && (rowSizes.get(times[index - 1]) ?? 0) >= minChord) walls++;
    }
    return walls / (times.length - 1);
}

// Peak count of distinct hit instants inside any one real-time second. The
// window is chart time, so a rate widens it: 1500ms of a 1.5x chart is a
// second of play.
function peakRowsPerSecond(map, windowMs) {
    const times = [...new Set(map.notes.map((note) => note.time))].sort((a, b) => a - b);
    let peak = 0;
    let start = 0;
    for (let index = 0; index < times.length; index++) {
        while (times[index] - times[start] > windowMs) start++;
        const rows = index - start + 1;
        if (rows > peak) peak = rows;
    }
    return peak;
}

/** Tier 6 on its own; the rate arms combine the play-side-safe tiers. */
export function detectRollVibro(map, rate = 1) {
    if (map.keyCount !== 4 || map.notes.length < RICE_VIBRO_MIN_NOTES) return false;
    return columnFastGaps(map, RICE_VIBRO_ROLL_GAP_MS * rate).ratio >= RICE_VIBRO_ROLL_MIN_RATIO
        && fastRollRowShare(map, RICE_VIBRO_ROLL_ROW_GAP_MS * rate) >= RICE_VIBRO_ROLL_MIN_ROW_RATIO;
}

/** A row qualifies when at least two distinct fingers each re-hit within
 * the band's time window. A continuous section contains only such rows. */
export function detectSustainedChordVibro(map, rate = 1) {
    if (map.keyCount !== 4 || map.notes.length < RICE_VIBRO_MIN_NOTES || !Number.isFinite(rate) || rate <= 0) return false;
    const rows = new Map();
    let holds = 0;
    for (const note of map.notes) {
        rows.set(note.time, (rows.get(note.time) ?? 0) | (1 << note.column));
        if (note.isHold) holds++;
    }
    if (holds / map.notes.length > SUSTAINED_CHORD_VIBRO_MAX_HOLD_RATIO) return false;

    const times = [...rows.keys()].sort((a, b) => a - b);
    for (const band of SUSTAINED_CHORD_VIBRO_BANDS) {
        const lastColumnTimes = new Array(4).fill(-Infinity);
        const cutoff = band.gapMs * rate;
        let columnGaps = 0;
        let fastColumnGaps = 0;
        let chordRows = 0;
        let consecutiveRows = 0;
        let longestRun = 0;
        for (const time of times) {
            const mask = rows.get(time);
            let fastFingers = 0;
            for (let column = 0; column < 4; column++) {
                if (!(mask & (1 << column))) continue;
                const gap = time - lastColumnTimes[column];
                if (Number.isFinite(gap) && gap > 0) {
                    columnGaps++;
                    if (gap <= cutoff) {
                        fastColumnGaps++;
                        fastFingers++;
                    }
                }
                lastColumnTimes[column] = time;
            }
            if (fastFingers >= 2) {
                chordRows++;
                consecutiveRows++;
                longestRun = Math.max(longestRun, consecutiveRows);
            } else {
                consecutiveRows = 0;
            }
        }
        if (columnGaps > 0
            && fastColumnGaps / columnGaps >= band.columnShare
            && chordRows / rows.size >= SUSTAINED_CHORD_VIBRO_MIN_ROW_SHARE
            && longestRun >= SUSTAINED_CHORD_VIBRO_MIN_ROWS) return true;
    }
    return false;
}

export function detectLnVibro(map, rate = 1) {
    let holds = 0;
    const rowTimes = new Set();
    for (const note of map.notes) {
        if (note.isHold && note.endTime > note.time) holds++;
        rowTimes.add(note.time);
    }
    if (map.notes.length === 0 || rowTimes.size < LN_VIBRO_MIN_ROWS) return false;
    if (holds / map.notes.length < LN_VIBRO_MIN_HOLD_RATIO) return false;
    const times = [...rowTimes].sort((a, b) => a - b);
    const gaps = [];
    for (let index = 1; index < times.length; index++) gaps.push(times[index] - times[index - 1]);
    gaps.sort((a, b) => a - b);
    const p75 = gaps[Math.min(gaps.length - 1, Math.floor(gaps.length * 0.75))];
    // Gaps are chart-time; a rate rescales what the player experiences.
    return p75 <= LN_VIBRO_MAX_P75_ROW_GAP_MS * rate;
}

/**
 * 整图 rice 六档，返回命中的档位（空数组 = 未命中）。
 * 与源文件的差别只有一处：4K rice 不再委派段落模型（本模块只做整图判定），
 * 因此这里的 4K 结果等于源文件对其它键数/非 rice 谱面使用的旧策略。
 */
export function detectRiceVibroReasons(map, rate = 1) {
    const reasons = [];
    if (!map || !Array.isArray(map.notes) || map.notes.length === 0) return reasons;
    if (!Number.isFinite(rate) || rate <= 0) return reasons;

    if (map.notes.length >= RICE_VIBRO_MIN_NOTES) {
        const sustained = columnFastGaps(map, RICE_VIBRO_COLUMN_GAP_MS * rate);
        if (sustained.maxRun >= RICE_VIBRO_COLUMN_MIN_RUN && sustained.ratio >= RICE_VIBRO_COLUMN_MIN_RATIO) {
            reasons.push(VIBRO_REASON.riceSustainedColumn);
        }
        if (detectSustainedChordVibro(map, rate)) reasons.push(VIBRO_REASON.riceSustainedChord);

        // The wall tier's chord-size floor assumes 4 columns; wider keymodes carry
        // legit 4-note chords constantly, so it stays 4K-scoped.
        if (map.keyCount === 4) {
            const walls = quadWallRows(map, RICE_VIBRO_WALL_GAP_MS * rate);
            if (walls.rows >= RICE_VIBRO_WALL_MIN_ROWS && walls.ratio >= RICE_VIBRO_WALL_MIN_ROW_RATIO) {
                const fast = columnFastGaps(map, RICE_VIBRO_WALL_COLUMN_GAP_MS * rate);
                if (fast.ratio >= RICE_VIBRO_WALL_COLUMN_MIN_RATIO) reasons.push(VIBRO_REASON.riceSlowChordWall);
            }
            if (detectRollVibro(map, rate)) reasons.push(VIBRO_REASON.riceRoll);
        }
    }

    // Tiers 3 and 4 share a lower size floor: TV-size burst packs sit under the
    // tier-1 floor but their soak fraction is unambiguous.
    if (map.notes.length >= RICE_VIBRO_BURST_MIN_NOTES) {
        const bursts = columnBurstRuns(map, RICE_VIBRO_BURST_GAP_MS * rate, RICE_VIBRO_BURST_MIN_RUN);
        if (bursts.runs >= RICE_VIBRO_BURST_MIN_RUNS && bursts.fraction >= RICE_VIBRO_BURST_MIN_FRACTION) {
            reasons.push(VIBRO_REASON.riceBurstSoak);
        }

        // Tier 4 shares that floor: the shape is a burst, so chart length says
        // nothing about it, and the smallest chart the sweep flags carries 217 notes.
        if (peakRowsPerSecond(map, RICE_VIBRO_ROW_RATE_WINDOW_MS * rate) >= RICE_VIBRO_MAX_ROWS_PER_SECOND) {
            reasons.push(VIBRO_REASON.riceRowDensity);
        }

        if (chordWallRatio(map, RICE_VIBRO_CHORD_WALL_GAP_MS * rate) >= RICE_VIBRO_CHORD_WALL_MIN_RATIO) {
            reasons.push(VIBRO_REASON.riceChordWall);
        }
    }

    return reasons;
}

/** 整图 rice 判定（布尔），等价于源文件的 detectRiceVibro 在非段落路径下的语义。 */
export function detectRiceVibro(map, rate = 1) {
    return detectRiceVibroReasons(map, rate).length > 0;
}

/**
 * 按速率的整图判定：roll / 持续和弦 / 和弦墙占比 ≥ 0.5。
 * 源文件在 4K rice 上会先问段落模型；本模块只做整图判定。
 */
export function detectRateVibro(map, rate = 1) {
    if (!map || !Array.isArray(map.notes) || map.notes.length === 0) return false;
    if (detectRollVibro(map, rate)) return true;
    if (detectSustainedChordVibro(map, rate)) return true;
    if (map.notes.length < RICE_VIBRO_BURST_MIN_NOTES) return false;
    return chordWallRatio(map, RICE_VIBRO_CHORD_WALL_GAP_MS * rate) >= RATE_VIBRO_CHORD_WALL_MIN_RATIO;
}

/**
 * 元数据关键词直判：标题或难度名包含关键词（大小写不敏感）即视为 vibro。
 * 关键词表在 config.js 的 APP_CONFIG.vibroKeywords。
 */
export function detectVibroFromMetadata(metaData) {
    const keywords = Array.isArray(APP_CONFIG.vibroKeywords) ? APP_CONFIG.vibroKeywords : [];
    if (keywords.length === 0 || !metaData || typeof metaData !== "object") return false;
    const haystack = `${metaData.Title ?? metaData.title ?? ""}\n${metaData.Version ?? metaData.version ?? ""}`.toLowerCase();
    if (!haystack.trim()) return false;
    return keywords.some((keyword) => {
        const needle = String(keyword ?? "").toLowerCase();
        return needle.length > 0 && haystack.includes(needle);
    });
}

/**
 * 从解析结果构造本模块需要的 map（notes 为 { column, time, endTime, isHold }）。
 * 输入是 OsuFileParser.getParsedData() 的形态。
 */
export function notesFromParsedData(parsedData) {
    const columns = Array.isArray(parsedData?.columns) ? parsedData.columns : [];
    const starts = Array.isArray(parsedData?.noteStarts) ? parsedData.noteStarts : [];
    const ends = Array.isArray(parsedData?.noteEnds) ? parsedData.noteEnds : [];
    const types = Array.isArray(parsedData?.noteTypes) ? parsedData.noteTypes : [];
    const count = Math.min(columns.length, starts.length, types.length);
    const notes = [];
    for (let index = 0; index < count; index++) {
        const type = Number(types[index]) || 0;
        const isHold = (type & 128) !== 0;
        const time = Number(starts[index]);
        if (!Number.isFinite(time)) continue;
        const endTime = isHold && Number.isFinite(Number(ends[index])) ? Number(ends[index]) : time;
        notes.push({
            column: Number(columns[index]),
            time,
            endTime,
            isHold,
        });
    }
    return notes;
}

/**
 * 整图 vibro 总入口：rice 六档 + LN vibro。
 * 返回 { vibro, reasons }；reasons 用于展示/调试（命中档位名或关键词标记）。
 */
export function detectChartVibro({ notes, keyCount, rate = 1, metaData = null } = {}) {
    const reasons = [];
    if (detectVibroFromMetadata(metaData)) reasons.push(VIBRO_REASON.metadataKeyword);

    const map = {
        notes: Array.isArray(notes) ? notes : [],
        keyCount: Number(keyCount) || 0,
    };
    if (map.notes.length > 0 && map.keyCount > 0 && Number.isFinite(rate) && rate > 0) {
        for (const reason of detectRiceVibroReasons(map, rate)) {
            if (!reasons.includes(reason)) reasons.push(reason);
        }
        if (detectLnVibro(map, rate) && !reasons.includes(VIBRO_REASON.lnStaggeredSpam)) {
            reasons.push(VIBRO_REASON.lnStaggeredSpam);
        }
    }

    return { vibro: reasons.length > 0, reasons };
}

// ─────────────── 既有判据（原 js/app/vibro.js，已合并进本共享模块） ───────────────
// 这两个判据自插件早期就存在，随 vibro 检测统一收敛到共享模块（Node/浏览器同款纯函数），
// 语义与阈值未改动；浏览器专属的 MSD 取值仍留在 js/app/analysis.js（resolveVibroMsdValues）。

function pickNumber(obj, keys) {
    if (!obj || typeof obj !== "object") {
        return null;
    }

    for (const key of keys) {
        const value = Number(obj[key]);
        if (Number.isFinite(value)) {
            return value;
        }
    }

    return null;
}

/**
 * Etterna MSD 口径的 vibro 判据：JackSpeed / Overall ≥ threshold（插件默认 0.95）。
 * 调用方负责提供 MSD values（浏览器侧 4K 固定用 0.72.3 基准，见 analysis.js 的 resolveVibroMsdValues）。
 */
export function detectVibro(values, threshold) {
    const overall = pickNumber(values, ["Overall", "overall"]);
    const jackSpeed = pickNumber(values, ["JackSpeed", "Jackspeed", "jackSpeed", "jackspeed"]);

    if (!Number.isFinite(overall) || overall <= 0 || !Number.isFinite(jackSpeed)) {
        return false;
    }

    return (jackSpeed / overall) >= threshold;
}

/**
 * pattern report 口径的 vibro 判据：存在 BPM ≥ minBpm 且 Longjacks 占比 ≥ threshold 的簇。
 */
export function detectVibroFromLongjackPattern(patternReport, threshold, minBpm) {
    if (!patternReport || !Array.isArray(patternReport.Clusters)) {
        return false;
    }

    const bpmLimit = Number.isFinite(minBpm) && minBpm > 0 ? minBpm : 0;

    for (const cluster of patternReport.Clusters) {
        if (!Array.isArray(cluster.SpecificTypes)) {
            continue;
        }
        const clusterBpm = Number(cluster.BPM);
        if (!Number.isFinite(clusterBpm) || clusterBpm < bpmLimit) {
            continue;
        }
        for (const [name, ratio] of cluster.SpecificTypes) {
            if (name === "Longjacks" && Number.isFinite(ratio) && ratio >= threshold) {
                return true;
            }
        }
    }

    return false;
}
