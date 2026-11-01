// rawFrames.js —— 把 Lazer 原始 ReplayFrame（Time + ManiaAction 位掩码）转换成
// Stable 回放等价的 head/tail/14/15/13 事件序列，再交给 accumulator 的 SV1 状态机。
//
// 这里是**数据源适配**：不做另一套判定公式，只负责把原始按键帧映射成
// accumulator 已经验证过的事件语法。这样 10K/9K/4K 的稳定公式仍然只有一份。

const FRAME_ARRAY_DATA = 0x10; // 仅文档：List<T>._items 的元素起点

function mirrorMask(mask, keys) {
    let out = 0;
    for (let c = 0; c < keys; c += 1) {
        if (mask & (1 << c)) {
            out |= 1 << (keys - 1 - c);
        }
    }
    return out;
}

/**
 * @param {object} chart buildChart() 的轨道模型
 * @param {{time:number, keys:number}[]} frames 按时间升序的原始帧
 * @param {{miss:number, ok:number, meh:number}} windows stable 窗口
 * @param {string[]} mods 短码数组（MR 时反转列）
 * @param {number} speedRate 保留参数；ReplayFrame.Time 已是谱面时间，不缩放
 * @returns {{col:number, offset:number, result:number}[]}
 */
export function framesToSv1Events(chart, frames, windows, mods = [], speedRate = 1, noRegrabDrops = null, maxTime = Infinity) {
    const keys = chart.columnCount;
    const forceDrops = noRegrabDrops instanceof Set
        ? noRegrabDrops
        : new Set(Array.isArray(noRegrabDrops) ? noRegrabDrops : []);
    const mirror = Array.isArray(mods) && mods.includes("MR");
    const press = Array.from({ length: keys }, () => []);
    const release = Array.from({ length: keys }, () => []);
    let prev = 0;
    for (const frame of frames) {
        if (!Number.isFinite(frame?.time) || !Number.isFinite(frame?.keys)) {
            continue;
        }
        const time = frame.time;
        let mask = frame.keys >>> 0;
        if (mirror) {
            mask = mirrorMask(mask, keys);
        }
        const changed = (mask ^ prev) >>> 0;
        if (changed !== 0) {
            for (let c = 0; c < keys; c += 1) {
                const bit = 1 << c;
                if (changed & bit) {
                    (mask & bit ? press : release)[c].push(time);
                }
            }
        }
        prev = mask;
    }

    const byColumn = Array.from({ length: keys }, () => []);
    for (let note = 0; note < chart.noteCount; note += 1) {
        byColumn[chart.columns[note]].push(note);
    }
    for (const list of byColumn) {
        list.sort((a, b) => chart.noteStarts[a] - chart.noteStarts[b]);
    }

    const events = [];
    const push = (time, col, order, event) => {
        if (time > maxTime + 1) {
            return;
        }
        events.push({ time, col, order, event });
    };

    for (let c = 0; c < keys; c += 1) {
        const notes = byColumn[c];
        const pr = press[c];
        const rl = release[c];
        let pi = 0;
        let ri = 0;
        for (let k = 0; k < notes.length; k += 1) {
            const note = notes[k];
            const start = chart.noteStarts[note];
            const end = chart.isHold[note] ? chart.noteEnds[note] : start;
            const nextStart = k + 1 < notes.length ? chart.noteStarts[notes[k + 1]] : Number.POSITIVE_INFINITY;

            if (start > maxTime + windows.miss) {
                break;
            }
            while (pi < pr.length && pr[pi] < start - windows.miss) {
                pi += 1;
            }
            let pressTime = null;
            if (pi < pr.length && pr[pi] <= start + windows.ok) {
                pressTime = pr[pi];
                pi += 1;
            }
            if (pressTime === null && start + windows.miss > maxTime) {
                continue;
            }
            const headOffset = pressTime === null ? 0 : pressTime - start;
            push(start + headOffset, c, 0, {
                col: c,
                offset: headOffset,
                result: pressTime === null ? 1 : 6,
            });

            if (!chart.isHold[note]) {
                continue;
            }

            const anchor = pressTime === null ? start : pressTime;
            while (ri < rl.length && rl[ri] < anchor - 5) {
                ri += 1;
            }
            let releaseTime = null;
            if (ri < rl.length && rl[ri] <= end + windows.ok) {
                releaseTime = rl[ri];
                ri += 1;
            }
            const tailOffset = releaseTime === null ? 0 : releaseTime - end;
            const naturalDrop = releaseTime === null || tailOffset < -windows.meh;
            const forced = forceDrops.has(note) && releaseTime !== null && tailOffset < -windows.meh;
            const tailResult = forced ? 50 : naturalDrop ? 1 : 6;
            const tailTime = end + tailOffset;
            if (tailTime > maxTime + 1) {
                continue;
            }
            if (tailResult === 50) {
                // 先发正常尾事件占 slot，再发合成 50 让 accumulator 强制记 50/100。
                push(tailTime, c, 1, { col: c, offset: tailOffset, result: 1 });
                push(tailTime, c, 2, { col: c, offset: tailOffset, result: 50 });
            } else {
                push(tailTime, c, 1, { col: c, offset: tailOffset, result: tailResult });
                if (tailResult === 1) {
                    push(tailTime, c, 2, { col: c, offset: tailOffset, result: 15 });
                    push(tailTime, c, 3, { col: c, offset: tailOffset, result: 13 });
                } else {
                    push(tailTime, c, 2, { col: c, offset: 0, result: 14 });
                    push(tailTime, c, 3, { col: c, offset: tailOffset, result: 14 });
                }
            }
            // `nextStart` 目前不参与事件生成；保留读取是为了和 stable_sim 的逐列
            // “到下一 note 前”语义对齐，后续若要处理跨 note regrab 不需要再改签名。
            void nextStart;
        }
    }

    events.sort((a, b) => a.time - b.time || a.col - b.col || a.order - b.order);
    return events.map((entry) => entry.event);
}

export { FRAME_ARRAY_DATA };
