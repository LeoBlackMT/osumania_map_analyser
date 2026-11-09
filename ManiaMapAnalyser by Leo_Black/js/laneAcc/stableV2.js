// stableV2.js —— Stable ScoreV2 的 LaneAcc 每 note 判定（DOM-free，纯函数）。
//
// 数据源：壳的原始 ReplayFrame（`{time, keys}`，keys 为 1<<column 位掩码）+ chart。
// 不读 note-list 的最终档，也不依赖任何外部转换模块；按 Stable V2 规则自己算：
//   - tap：按下沿 vs V2 窗口；
//   - LN：头=按下沿，尾=|释放沿−note.end|/1.5 vs V2 窗口（未释放→50，游戏 miss→miss）。
// 输出与壳的 `mania_hit_events` 同文法：`{col, result, offset}`，result 1..6 = miss..MAX。

const RELEASE_LENIENCE = 1.5;

function difficultyRange(od, min, mid, max) {
    if (od > 5) return mid + (max - mid) * ((od - 5) / 5);
    if (od < 5) return mid + (mid - min) * ((od - 5) / 5);
    return mid;
}

/** Stable ScoreV2 六窗：Lazer 式 OD 范围，最后 `floor(x * totalMultiplier) + 0.5`。 */
export function stableV2Windows(od, speedRate = 1, mods = []) {
    const clamped = Math.min(10, Math.max(0, Number.isFinite(Number(od)) ? Number(od) : 8));
    const set = new Set((Array.isArray(mods) ? mods : []).map((m) => String(m).toUpperCase()));
    const rate = Number.isFinite(Number(speedRate)) && Number(speedRate) > 0
        ? Number(speedRate)
        : set.has("DT") || set.has("NC") ? 1.5 : set.has("HT") || set.has("DC") ? 0.75 : 1;
    const difficulty = set.has("HR") ? 1.4 : set.has("EZ") ? 1 / 1.4 : 1;
    const total = rate / difficulty;
    const scale = (value) => Math.floor(value * total) + 0.5;
    return {
        perfect: scale(difficultyRange(clamped, 22.4, 19.4, 13.9)),
        great: scale(difficultyRange(clamped, 64, 49, 34)),
        good: scale(difficultyRange(clamped, 97, 82, 67)),
        ok: scale(difficultyRange(clamped, 127, 112, 97)),
        meh: scale(difficultyRange(clamped, 151, 136, 121)),
        miss: scale(difficultyRange(clamped, 188, 173, 158)),
    };
}

function judgmentForOffset(offset, windows) {
    const delta = Math.abs(Math.round(offset));
    if (delta <= windows.perfect) return 1;
    if (delta <= windows.great) return 2;
    if (delta <= windows.good) return 3;
    if (delta <= windows.ok) return 4;
    if (delta <= windows.meh) return 5;
    if (delta <= windows.miss) return 6;
    return 0;
}

function headJudgment(offset, windows) {
    const rounded = Math.round(offset);
    // Stable V2 的 late OK 窗口是开区间：>= floor(ok) 且 <= miss 记 miss，否则不判。
    if (rounded >= Math.floor(windows.ok)) return rounded <= windows.miss ? 6 : 0;
    return judgmentForOffset(offset, windows);
}

function tailJudgment(offset, windows) {
    const delta = Math.abs(Math.round(offset));
    if (delta <= Math.floor(windows.perfect) * RELEASE_LENIENCE) return 1;
    if (delta <= Math.floor(windows.great) * RELEASE_LENIENCE) return 2;
    if (delta <= Math.floor(windows.good) * RELEASE_LENIENCE) return 3;
    if (offset >= Math.floor(windows.ok) * RELEASE_LENIENCE) return 6;
    if (delta <= Math.floor(windows.ok) * RELEASE_LENIENCE) return 4;
    if (delta <= Math.floor(windows.meh) * RELEASE_LENIENCE) return 5;
    if (delta <= Math.floor(windows.miss) * RELEASE_LENIENCE) return 6;
    return 0;
}

/** Lazer 的窗口边界用原始毫秒比较，不做四舍五入。 */
function capToMeh(judgment) {
    return judgment < 5 ? 5 : judgment;
}

function rawJudgmentForOffset(offset, windows) {
    const delta = Math.abs(offset);
    if (delta <= windows.perfect) return 1;
    if (delta <= windows.great) return 2;
    if (delta <= windows.good) return 3;
    if (delta <= windows.ok) return 4;
    if (delta <= windows.meh) return 5;
    if (delta <= windows.miss) return 6;
    return 0;
}

function buildColumnNoteLists(chart) {
    const lists = Array.from({ length: chart.columnCount }, () => []);
    for (let note = 0; note < chart.noteCount; note += 1) {
        lists[chart.columns[note]].push(note);
    }
    return lists;
}

function mirrorMask(mask, keys) {
    let out = 0;
    for (let c = 0; c < keys; c += 1) {
        if (mask & (1 << c)) {
            out |= 1 << (keys - 1 - c);
        }
    }
    return out;
}

function buildColumnSegments(chart, frames, mirror = false) {
    const segments = Array.from({ length: chart.columnCount }, () => []);
    const active = new Array(chart.columnCount).fill(null);
    for (const frame of frames) {
        const time = Number(frame?.time);
        let mask = Number(frame?.keys) >>> 0;
        if (!Number.isFinite(time)) continue;
        if (mirror) {
            mask = mirrorMask(mask, chart.columnCount);
        }
        for (let col = 0; col < chart.columnCount; col += 1) {
            const down = (mask & (1 << col)) !== 0;
            if (down && active[col] == null) {
                active[col] = time;
            } else if (!down && active[col] != null) {
                segments[col].push({ start: active[col], end: time });
                active[col] = null;
            }
        }
    }
    for (let col = 0; col < chart.columnCount; col += 1) {
        if (active[col] != null) {
            // 帧流里还没有 key-up：live 不能伪造释放；等真实释放帧或结算全量。
            segments[col].push({ start: active[col], end: null });
        }
    }
    return segments;
}

/**
 * 一条 LN 的尾档：从头顶段开始向后扫描，断手只封顶 50，释放落在尾窗内即判。
 * @returns {{judgment:number, offset:number}|null} null = 帧流还不足以确定（live 等下一拍）
 */
function lazerTailFrom(list, startIndex, end, windows, headMissed, cutoff) {
    const tailDeadline = end + windows.miss * RELEASE_LENIENCE;
    let bodyBreak = false;
    for (let index = startIndex; index < list.length; index += 1) {
        const segment = list[index];
        if (segment.end == null) {
            // 帧流里还没有释放：live 不能伪造，结算时按“一直按住超过尾时限”记 miss。
            return cutoff === Infinity ? { judgment: 6, offset: 0 } : null;
        }
        if (segment.end > tailDeadline) {
            return { judgment: 6, offset: segment.end - end };
        }
        const raw = rawJudgmentForOffset((segment.end - end) / RELEASE_LENIENCE, windows);
        if (raw) {
            return {
                judgment: headMissed || bodyBreak ? capToMeh(raw) : raw,
                offset: segment.end - end,
            };
        }
        if (segment.end >= tailDeadline) {
            return { judgment: 6, offset: segment.end - end };
        }
        bodyBreak = true;
        const next = list[index + 1];
        if (!next || next.start > tailDeadline) {
            return { judgment: 6, offset: 0 };
        }
    }
    return null;
}

/**
 * 头 miss 后由后续 regrab 段补的尾档（封顶 50）；按下落在下一条 note 上时不再补。
 */
function lazerRegrabTail(list, cursor, notes, notePosition, noteTimes, end, windows) {
    const tailDeadline = end + windows.miss * RELEASE_LENIENCE;
    for (let index = cursor; index < list.length; index += 1) {
        const segment = list[index];
        if (segment.start > end + windows.meh) break;
        if (segment.end == null || segment.end > tailDeadline) break;
        let nextAliveStart = null;
        for (let next = notePosition + 1; next < notes.length; next += 1) {
            const later = noteTimes[notes[next]];
            if (segment.start <= later + windows.meh) {
                nextAliveStart = later;
                break;
            }
        }
        if (nextAliveStart != null) {
            if (segment.start >= nextAliveStart) continue;
            if (nextAliveStart - segment.start <= windows.meh && end < nextAliveStart) break;
        }
        const raw = rawJudgmentForOffset((segment.end - end) / RELEASE_LENIENCE, windows);
        if (!raw) continue;
        return { judgment: capToMeh(raw), offset: segment.end - end };
    }
    return null;
}

/**
 * Lazer 式段归属：同列头取窗内最早的可判按下沿，且严格早于下一条 note；
 * 尾沿本条 LN 自己的段向后扫描，断手只封顶 50，不与下一条 note 的头段争抢。
 * 正常 Stable 的顶部 V1→V2 折算用它；Stable SV2 分轨仍走 Stable 规则。
 */
function lazerJudgedEvents(chart, frames, windows, cutoff, mirror) {
    const segments = buildColumnSegments(chart, frames, mirror);
    const notesByColumn = buildColumnNoteLists(chart);
    const events = [];
    const push = (col, judgment, offset, part, note) => {
        if (!judgment) return;
        events.push({ col, result: 7 - judgment, offset, part, judgment, note });
    };
    const tailWindow = windows.miss * RELEASE_LENIENCE;

    for (let col = 0; col < chart.columnCount; col += 1) {
        const list = segments[col];
        const notes = notesByColumn[col];
        let cursor = 0;
        for (let position = 0; position < notes.length; position += 1) {
            const note = notes[position];
            const time = chart.noteStarts[note];
            const end = chart.noteEnds[note];
            const hold = chart.isHold[note] !== 0;
            const nextTime = position + 1 < notes.length ? chart.noteStarts[notes[position + 1]] : Infinity;
            // 还没到最早可能判定窗口的 note 不能先判。
            if (time > cutoff + windows.miss) break;
            while (cursor < list.length && list[cursor].start < time - windows.miss) {
                cursor += 1;
            }
            let matched = -1;
            let headJudgment = 0;
            for (let index = cursor; index < list.length; index += 1) {
                const segment = list[index];
                // 下一条 note 已经可判时，这次按下属于它，不能再给本条头。
                if (segment.start >= nextTime) break;
                if (segment.start > time + windows.meh) break;
                const judgment = rawJudgmentForOffset(segment.start - time, windows);
                if (judgment) {
                    matched = index;
                    headJudgment = judgment;
                    break;
                }
            }
            if (matched < 0) {
                const headDeadline = Math.min(time + windows.meh, nextTime);
                if (cutoff < headDeadline) break;
                push(col, 6, 0, "head-miss", note);
                if (!hold) continue;
                if (cutoff < end + tailWindow) break;
                const regrab = lazerRegrabTail(list, cursor, notes, position, chart.noteStarts, end, windows);
                if (regrab) push(col, regrab.judgment, regrab.offset, "hold-tail", note);
                else push(col, 6, 0, "hold-tail", note);
                continue;
            }
            const segment = list[matched];
            push(col, headJudgment, segment.start - time, hold ? "hold-head" : "tap", note);
            cursor = matched + 1;
            if (!hold) continue;
            const tail = lazerTailFrom(list, matched, end, windows, headJudgment === 6, cutoff);
            if (!tail) break;
            push(col, tail.judgment, tail.offset, "hold-tail", note);
        }
    }
    return events;
}

/**
 * Stable ScoreV2 的 LaneAcc 事件。
 * @param {object} chart `buildChart()` 的 typed-array chart
 * @param {Array<{time:number, keys:number}>} frames 原始帧（keys = 1<<column 位掩码）
 * @param {{odFlag?:number|null, mods?:string[], speedRate?:number, cutoff?:number,
 *   judgementMode?:"stable"|"lazer", headMissNotes?:Set<number>|number[]}} [options]
 *   `judgementMode:"lazer"` = 顶部 V1→V2 折算的 Lazer 式段归属（Stable SV2 分轨必须用默认 "stable"）。
 *   `headMissNotes` = 壳 native 头判为 miss 的 note 下标（覆盖帧里被 LN 预选路由误用的按下沿）。
 * @returns {Array<{col:number, result:number, offset:number, part:string, judgment:number}>}
 */
export function framesToStableV2Events(chart, frames, options = {}) {
    if (!chart || chart.error || !chart.noteCount || !Array.isArray(frames)) return [];
    const od = options.odFlag != null && Number.isFinite(Number(options.odFlag))
        ? Number(options.odFlag)
        : chart.od;
    const mods = Array.isArray(options.mods) ? options.mods : [];
    const windows = stableV2Windows(od, options.speedRate ?? 1, mods);
    const mirror = mods.some((mod) => /^MR$/i.test(String(mod)));
    const rawCutoff = options.cutoff == null ? Infinity : Number(options.cutoff);
    // -Infinity 表示“还没有播放时钟”，必须保持为空判定；不能当成 Infinity 全量判。
    const cutoff = Number.isNaN(rawCutoff) ? Infinity : rawCutoff;
    if (options.judgementMode === "lazer") {
        return lazerJudgedEvents(chart, frames, windows, cutoff, mirror);
    }
    const tailWindow = windows.miss * RELEASE_LENIENCE;
    // 壳读到的游戏 per-note 头判（带 note 下标）：头 miss 的 note 不能再用帧里的
    // 按下沿补成命中，尾档按 V2 规则封顶 50。
    const headMissNotes = options.headMissNotes instanceof Set
        ? options.headMissNotes
        : new Set(Array.isArray(options.headMissNotes) ? options.headMissNotes : []);
    const segments = buildColumnSegments(chart, frames, mirror);
    const segCursor = new Array(chart.columnCount).fill(0);
    const events = [];
    const push = (col, judgment, offset, part, note) => {
        if (!judgment) return;
        events.push({ col, result: 7 - judgment, offset, part, judgment, note });
    };

    for (let i = 0; i < chart.noteCount; i += 1) {
        const time = chart.noteStarts[i];
        const end = chart.noteEnds[i];
        const col = chart.columns[i];
        const hold = chart.isHold[i] !== 0;
        // 只判已经到窗口的 note；未来 note 保持未判定，不能先记 miss。
        if (time > cutoff + windows.miss) continue;
        const list = segments[col];
        // 头必须是本 note 的按下沿：跳过更早的段（上一条 LN 断手/regrab 留下的残段）。
        while (segCursor[col] < list.length) {
            const candidate = list[segCursor[col]];
            const tooEarly = candidate.start < time - windows.miss
                || (candidate.end != null && candidate.end < time - windows.miss);
            if (!tooEarly) break;
            segCursor[col] += 1;
        }
        const seg = list[segCursor[col]];
        const hasHead = seg != null && seg.start >= time - windows.miss && seg.start <= time + windows.miss;
        if (!hasHead) {
            // live：当前帧流可能还没覆盖到这个 note，不能补 miss；但 note 头窗已经
            // 完全过去仍无按下沿，就是确定的 miss，必须立即计入（否则 live 少 miss）。
            if (cutoff === Infinity || time + windows.miss < cutoff) {
                push(col, 6, 0, "head-miss", i);
                if (hold) push(col, 6, 0, "tail-miss", i);
            }
            continue;
        }
        const forcedHeadMiss = Boolean(options.headMissNotes) && headMissNotes.has(i);
        if (forcedHeadMiss) {
            // 游戏头部实际 miss：帧里的按下沿只是被前一条 LN 的尾窗/预选路由走了。
            push(col, 6, 0, "head-miss", i);
        } else {
            push(col, headJudgment(seg.start - time, windows), seg.start - time, hold ? "hold-head" : "tap", i);
        }
        segCursor[col] += 1;
        if (!hold) continue;
        // 头判由游戏 native 覆盖为 miss 时，尾档仍按帧计算；优于 50 的档封顶 50。
        const pushTail = (judgment, offset) => {
            push(col, forcedHeadMiss && judgment < 5 ? 5 : judgment, offset, "hold-tail", i);
        };
        // 尾窗还没到：先只算头，尾等下一拍。
        if (end > cutoff + tailWindow) continue;
        // 同一 LN 内的断手/regrab：起点早于 note.end 的后续段都归这条 LN。
        let tailEnd = seg.end;
        let regrabs = 0;
        while (segCursor[col] < list.length && list[segCursor[col]].start < end - 10) {
            tailEnd = list[segCursor[col]].end;
            segCursor[col] += 1;
            regrabs += 1;
        }
        if (regrabs > 0) {
            // 断手后 regrab：首释放早于尾输入窗（tail−50 窗）才封顶 50；
            // 首释放仍在尾窗内则按该释放正常判尾（后续 regrab 不改变尾档）。
            const breakOffset = seg.end == null ? null : seg.end - end;
            const judgment = breakOffset != null && breakOffset >= -windows.meh
                ? tailJudgment(breakOffset, windows) || 5
                : 5;
            pushTail(judgment, breakOffset ?? 0);
        } else if (tailEnd == null) {
            // 按住过了尾判定时限：Stable 按 held 处理，尾为 MAX；live 等时限到再发。
            if (cutoff === Infinity || cutoff >= end + windows.meh) {
                pushTail(1, 0);
            }
        } else if (tailEnd - end > windows.meh) {
            // 迟放超过 50 窗口：同样按 held 到尾时限处理。
            pushTail(1, 0);
        } else {
            pushTail(tailJudgment(tailEnd - end, windows), tailEnd - end);
        }
    }
    return events;
}
