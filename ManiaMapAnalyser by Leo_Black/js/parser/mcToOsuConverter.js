// .mc（Malody chart，JSON）→ .osu v14 转换器（mania）。
//
// 移植自 mc_to_osu.py（nonebot_plugin_osumania_toolkit，MIT；其逻辑源自
// Jakads/malody2osu）——共享模块（浏览器 + Node 双环境），零 DOM 依赖。
// 语义：time[] → 绝对时间 BPM 点；effect[]（SV）→ osu 负红线（100/|scroll|，
// scroll=0 用 1E+308）；note endbeat → type 128 LN；LN 尾同毫秒冲突自动 -1ms
// 微调；仅支持 Key 模式（meta.mode === 0）。

const CONVERT_OD = 9;
const CONVERT_HP = 8;
const CONVERT_AR = 5;

function ms(beats, bpm, offset) {
    return 1000.0 * (60.0 / bpm) * beats + offset;
}

function beatValue(beatArr) {
    return beatArr[0] + beatArr[1] / beatArr[2];
}

function columnToX(column, keys) {
    return Math.floor((512 * column + 256) / keys);
}

/**
 * 把 .mc JSON 文本转为 .osu v14 文本。
 * @param {string} mcText
 * @returns {{osuText: string, meta: {title, artist, version, keys, noteCount, holdCount, bpmPoints, svCount}}}
 */
export function convertMcToOsuText(mcText) {
    let data;
    try {
        data = JSON.parse(mcText);
    } catch (e) {
        throw new Error(`.mc 解析失败：不是有效 JSON（${e.message}）`);
    }
    if (!data.meta) {
        throw new Error(`.mc 解析失败：缺少 meta 字段`);
    }
    if (data.meta.mode !== 0) {
        throw new Error(`.mc 解析失败：仅支持 Key 模式（mode 0），当前 mode=${data.meta.mode}`);
    }
    const keys = data.meta.mode_ext && data.meta.mode_ext.column;
    if (!keys) {
        throw new Error(`.mc 解析失败：缺少 mode_ext.column 字段`);
    }
    const line = data.time;
    const note = data.note;
    if (!Array.isArray(line) || line.length === 0) {
        throw new Error(`.mc 解析失败：缺少 time 字段或为空`);
    }
    if (!Array.isArray(note)) {
        throw new Error(`.mc 解析失败：缺少 note 字段`);
    }
    const effect = Array.isArray(data.effect) ? data.effect : [];

    // 第一个非普通 note（携 offset 的 keysound 参考点）
    const soundnote = note.find((n) => n.type !== 0) || {};

    // BPM 序列与绝对偏移
    const bpmList = [line[0].bpm];
    const bpmOffset = [-Number(soundnote.offset || 0)];
    for (let j = 0; j < line.length - 1; j += 1) {
        const x = line[j + 1];
        const offset = ms(beatValue(x.beat) - beatValue(line[j].beat), line[j].bpm, bpmOffset[j]);
        bpmList.push(x.bpm);
        bpmOffset.push(offset);
    }

    const segmentIndexAt = (beatVal) => {
        let idx = 0;
        for (let i = 0; i < line.length; i += 1) {
            if (beatValue(line[i].beat) > beatVal) {
                break;
            }
            idx = i;
        }
        return idx;
    };

    const meta = data.meta;
    const title = meta.song ? meta.song.title : "Unknown Title";
    const artist = meta.song ? meta.song.artist : "Unknown Artist";

    const lines = [];
    lines.push("osu file format v14", "");
    lines.push("[General]", "AudioFilename: audio.mp3", "AudioLeadIn: 0",
        `PreviewTime: ${meta.preview == null ? -1 : meta.preview}`,
        "Countdown: 0", "SampleSet: Soft", "StackLeniency: 0.7", "Mode: 3",
        "LetterboxInBreaks: 0", "SpecialStyle: 0", "WidescreenStoryboard: 0", "");
    lines.push("[Editor]", "DistanceSpacing: 1.2", "BeatDivisor: 4", "GridSize: 8",
        "TimelineZoom: 2.4", "");
    lines.push("[Metadata]");
    lines.push(`Title:${title}`);
    lines.push(`Artist:${artist}`);
    lines.push(`Creator:${meta.creator || "unknown"}`);
    lines.push(`Version:${meta.version || "Unknown"}`);
    lines.push("Source:Malody", "Tags:Converted from mc by LeosMma",
        "BeatmapID:0", "BeatmapSetID:-1", "");
    lines.push("[Difficulty]");
    lines.push(`HPDrainRate:${CONVERT_HP}`, `CircleSize:${keys}`, `OverallDifficulty:${CONVERT_OD}`,
        `ApproachRate:${CONVERT_AR}`, "SliderMultiplier:1.4", "SliderTickRate:1", "");
    lines.push("[Events]", "//Background and Video events", "");
    lines.push("[TimingPoints]");
    for (let i = 0; i < bpmList.length; i += 1) {
        const meter = line[i].sign || 4;
        lines.push(`${Math.round(bpmOffset[i])},${(60000 / bpmList[i]).toFixed(2)},${meter},1,0,0,1,0`);
    }
    for (const sv of effect) {
        const svBeat = beatValue(sv.beat);
        const idx = segmentIndexAt(svBeat);
        const svTime = ms(svBeat - beatValue(line[idx].beat), bpmList[idx], bpmOffset[idx]);
        const scroll = typeof sv.scroll === "number" ? sv.scroll : 1.0;
        const svValue = scroll === 0 ? "1E+308" : String(100 / Math.abs(scroll));
        const meter = line[idx].sign || 4;
        lines.push(`${Math.round(svTime)},-${svValue},${meter},1,0,0,0,0`);
    }
    lines.push("");
    lines.push("[HitObjects]");

    // 转换 note，先收集以做 LN 尾冲突修复
    const converted = [];
    const startCounter = new Map();
    for (const n of note) {
        if (n.type !== 0) {
            continue;
        }
        const column = Number(n.column);
        const nBeat = beatValue(n.beat);
        const idx = segmentIndexAt(nBeat);
        const nTime = Math.round(ms(nBeat - beatValue(line[idx].beat), bpmList[idx], bpmOffset[idx]));
        const key = `${column}:${nTime}`;
        startCounter.set(key, (startCounter.get(key) || 0) + 1);
        let endTime = null;
        let typeStr = "1";
        if (n.endbeat != null) {
            const endBeat = beatValue(n.endbeat);
            const endIdx = segmentIndexAt(endBeat);
            endTime = Math.round(ms(endBeat - beatValue(line[endIdx].beat), bpmList[endIdx], bpmOffset[endIdx]));
            typeStr = "128";
        }
        converted.push({
            column,
            startTime: nTime,
            endTime,
            typeStr,
            vol: n.vol != null ? n.vol : 100,
            sound: n.sound != null ? n.sound : 0,
        });
    }

    // LN 尾同毫秒冲突：同列同毫秒有 note 起始时尾 -1ms；尾 ≤ 头则 +1ms。
    for (const item of converted) {
        if (item.endTime == null) {
            continue;
        }
        let tail = item.endTime;
        while (tail > item.startTime && (startCounter.get(`${item.column}:${tail}`) || 0) > 0) {
            tail -= 1;
        }
        if (tail <= item.startTime) {
            tail = item.startTime + 1;
        }
        item.endTime = tail;
    }

    converted.sort((a, b) => a.startTime - b.startTime || a.column - b.column);
    let holdCount = 0;
    for (const item of converted) {
        const x = columnToX(item.column, keys);
        if (item.endTime != null) {
            holdCount += 1;
            lines.push(`${x},192,${item.startTime},${item.typeStr},${item.sound},${item.endTime}:0:0:0:${item.vol}:`);
        } else {
            lines.push(`${x},192,${item.startTime},${item.typeStr},${item.sound},0:0:0:${item.vol}:`);
        }
    }

    return {
        osuText: lines.join("\n"),
        meta: {
            title,
            artist,
            version: meta.version || "Unknown",
            keys,
            noteCount: converted.length,
            holdCount,
            bpmPoints: bpmList.length,
            svCount: effect.length,
        },
    };
}