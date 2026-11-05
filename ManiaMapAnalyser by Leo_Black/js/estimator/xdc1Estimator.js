// XDC1 —— 4K osu!mania 难度估算器（共享 DOM-free 纯函数模块；Node benchmark 与浏览器同一实现）。
//
// 移植自 TheBagelOfMan/xdc1 v1.0（MIT）：C++ 源码 https://github.com/TheBagelOfMan/xdc1
// 算法本体逐函数转写，系数与求值顺序保持不变。插件只消费 C++ 的 `rating`（0–100
// 显示值）与 `graph`（去体力后的 local strain）；C++ 的 `rating_rice` 与 7 项 skillset
// 只服务 CLI 诊断输出，插件无消费方，且不参与 rating/graph 计算，故未移植。
//
// 作用域：仅 4K（C++ 入口同样拒绝其他键数）；其他键数/解析失败/低于 Alpha Low
// 返回 `Unsupported: ...`，不切换到 Sunny。
// 固定 OD9 判定窗（`hitWindowUnit()`，与原实现一致，不读 odFlag）。
// 输出：`rating` 经上游作者使用的 rating→dan 标定折线（见下方 `XDC1_RATING_DAN_KNOTS`）
// 转成 Reform 段位数值；`rawNumericDifficulty` 保留原始 rating 供调试。

import { OsuFileParser } from "../parser/osuFileParser.js";
import { numericToRcLabel } from "./rcDifficultyFormat.js";
import { cloneOsuParser } from "../rework/sunnyCore.js";
import { smoothDForGraph } from "../rework/reworkMathCore.js";

// ── 共享网格工具：所有曲线都落在有序时间网格（ms）上，每个值保持到下一个点 ──
const WINDOW_MS = 500.0;
const LN_TAP_MS = 80.0;

const clamp = (value, min, max) => Math.max(min, Math.min(max, value));

function zeros(size) {
    return new Array(size).fill(0.0);
}

function lowerBound(sorted, value) {
    let lo = 0;
    let hi = sorted.length;
    while (lo < hi) {
        const mid = (lo + hi) >> 1;
        if (sorted[mid] < value) lo = mid + 1;
        else hi = mid;
    }
    return lo;
}

function upperBound(sorted, value) {
    let lo = 0;
    let hi = sorted.length;
    while (lo < hi) {
        const mid = (lo + hi) >> 1;
        if (sorted[mid] <= value) lo = mid + 1;
        else hi = mid;
    }
    return lo;
}

// 判定窗单位，固定 OD9。
function hitWindowUnit() {
    const x = 0.3 * Math.sqrt((64.5 - Math.ceil(9.0 * 3.0)) / 500.0);
    return Math.min(x, 0.6 * (x - 0.09) + 0.09);
}

// 分段常数 f 在 [x[i]-window, x[i]+window] 上的积分，裁剪到 x 的范围；windowSum 乘 scale。
function windowIntegral(x, f, window) {
    const n = x.length;
    const out = zeros(n);
    if (n < 2) return out;
    const prefix = zeros(n);
    for (let i = 1; i < n; i += 1) prefix[i] = prefix[i - 1] + f[i - 1] * (x[i] - x[i - 1]);
    let ca = 0;
    let cb = 0;
    for (let i = 0; i < n; i += 1) {
        const a = clamp(x[i] - window, x[0], x[n - 1]);
        const b = clamp(x[i] + window, x[0], x[n - 1]);
        while (ca < n && x[ca] < a) ca += 1;
        while (cb < n && x[cb] < b) cb += 1;
        const ia = ca ? ca - 1 : 0;
        const ib = cb ? cb - 1 : 0;
        out[i] = (prefix[ib] + f[ib] * (b - x[ib])) - (prefix[ia] + f[ia] * (a - x[ia]));
    }
    return out;
}

function windowSum(x, f, window, scale) {
    const out = windowIntegral(x, f, window);
    for (let i = 0; i < out.length; i += 1) out[i] *= scale;
    return out;
}

function windowAvg(x, f, window) {
    const n = x.length;
    const out = windowIntegral(x, f, window);
    let ca = 0;
    let cb = 0;
    for (let i = 0; i < n; i += 1) {
        const a = clamp(x[i] - window, x[0], x[n - 1]);
        const b = clamp(x[i] + window, x[0], x[n - 1]);
        while (ca < n && x[ca] < a) ca += 1;
        while (cb < n && x[cb] < b) cb += 1;
        const span = b - a;
        out[i] = span > 0.0 ? out[i] / span : 0.0;
    }
    return out;
}

// 把 [t0,t1) 上的常数 val 按网格单元时间平均累加，短跨度在网格落点处也能完整计入。
function addSpan(f, grid, t0, t1, val) {
    if (grid.length < 2 || t1 <= t0) return;
    let q = upperBound(grid, t0);
    if (q > 0) q -= 1;
    for (; q + 1 < grid.length && grid[q] < t1; q += 1) {
        const lo = Math.max(t0, grid[q]);
        const hi = Math.min(t1, grid[q + 1]);
        if (hi > lo) f[q] += val * (hi - lo) / (grid[q + 1] - grid[q]);
    }
}

// 按 transition 取值：值 i 从 starts[i] 保持到 starts[i+1]。
function stepsToGrid(starts, vals, grid) {
    const out = zeros(grid.length);
    if (vals.length === 0) return out;
    let c = 0;
    for (let i = 0; i < grid.length; i += 1) {
        while (c < vals.length && starts[c] <= grid[i]) c += 1;
        out[i] = vals[c ? c - 1 : 0];
    }
    return out;
}

// ── 音符与行 ────────────────────────────────────────────────────────────────
// Note: { k: 列, h: 头时间, t: 尾时间（taps 为 -1） }，始终按 (h, k) 排序。
function headTimes(notes) {
    const out = new Array(notes.length);
    for (let i = 0; i < notes.length; i += 1) out[i] = notes[i].h;
    return out;
}

function columnCount(mask) {
    return (mask & 1) + ((mask >> 1) & 1) + ((mask >> 2) & 1) + ((mask >> 3) & 1);
}

function buildRows(notes) {
    const rows = [];
    for (const note of notes) {
        if (rows.length === 0 || rows[rows.length - 1].t !== note.h) {
            rows.push({ t: note.h, mask: 0, size: 0 });
        }
        const row = rows[rows.length - 1];
        row.mask |= 1 << note.k;
        row.size = columnCount(row.mask);
    }
    return rows;
}

function rowTimes(rows) {
    return rows.map((row) => row.t);
}

// ── 按压 strain（每只手）────────────────────────────────────────────────────
const TECH_WINDOW_MS = 244.92;
const TECH_SCALE = 0.001;

const MOTION_X_COEF = 0.39104;
const MOTION_W = 5.7561;
const MOTION_REPEAT_W = 0.55187;
const FINGER_MIX_P = 2.3762;
const FLAM_MS = 11.247;
const FINGER_JACK_CRIT_MS = 218.21;
const JACK_FLOOR_MS = 35.809;
const MINIJACK_MS = 72.836;
const LN_HOLD_OCC = 0.754;
const ALT_CRIT_MS = 21.197;

const WRIST_W = 3.3377;
const WRIST_CHORD_W = 0.81919;
const WRIST_GAP_POW = 2.6413;
const WRIST_GAP_PIVOT_MS = 100.0;
const STROKE_CAP_MS = 1000.0;
const STROKE_TIE_FRAC = 0.125;

const CHORD_PHASE = 0.26009;
const ANCHOR_RUN_LO = 16.0;
const ANCHOR_RUN_HI = 48.0;
const MANIP_TOL_MS = 76.009;
const MANIP_TOL_POW = 2.6;
const MANIP_WINDOW = 12;
const MANIP_PERIOD_MAX = 12;
const MANIP_PERIOD_COST = 0.02;
const MANIP_PACE = 0.25;
const MANIP_PACE_FLOOR_MS = 4.0;
const MANIP_FLAM_MS = 10.0;
const MANIP_CONS_LO = 0.15;
const MANIP_CONS_HI = 0.75;
const WRIST_BLEND_MS = 64.443;

const JITTER_ONSET_MS = 95.0;
const JITTER_POW = 1.5;
const JITTER_REP_LO = 3.0;
const JITTER_REP_HI = 7.0;

function softenGap(gapMs) {
    return Math.sqrt(gapMs * gapMs + FLAM_MS * FLAM_MS);
}

// (1/d)(1/(d+x_term))，d 以秒计。
function gapRate(deltaMs, xTerm) {
    const d = 0.001 * deltaMs;
    return d > 0.0 ? (1.0 / d) * (1.0 / (d + xTerm)) : 0.0;
}

function techSum(grid, f) {
    return windowSum(grid, f, TECH_WINDOW_MS, TECH_SCALE);
}

function smoothstep01(u) {
    const t = clamp(u, 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

// 每个重复的间隔，带 minijack 允许量：out[i] 是结束于 press i+1 的间隔。
function minijackGaps(times) {
    const count = times.length > 1 ? times.length - 1 : 0;
    const gaps = zeros(count);
    for (let i = 0; i < count; i += 1) gaps[i] = times[i + 1] - times[i];
    const out = gaps.slice();
    for (let i = 0; i < count; i += 1) {
        if (gaps[i] >= MINIJACK_MS) continue;
        const before = i > 0 ? Math.max(0.5 * (gaps[i - 1] - gaps[i]), 0.0) : MINIJACK_MS;
        const after = i + 1 < count ? Math.max(0.5 * (gaps[i + 1] - gaps[i]), 0.0) : MINIJACK_MS;
        out[i] = Math.min(gaps[i] + before + after, MINIJACK_MS);
    }
    return out;
}

// 重复：两只手指共用一条 rate 曲线。
function repeatCost(handNotes, grid) {
    const fingerTimes = [[], []];
    for (const note of handNotes) fingerTimes[note.f].push(note.t);
    const softGaps = [minijackGaps(fingerTimes[0]), minijackGaps(fingerTimes[1])];
    const index = [0, 0];
    const rate = zeros(grid.length);
    const last = [-1.0, -1.0];
    const freeAt = [0.0, 0.0];
    for (const note of handNotes) {
        const f = note.f;
        const l = last[f];
        if (l >= 0.0 && note.t > l) {
            const raw = softGaps[f][index[f] - 1];
            const gap = softenGap(raw - Math.min(LN_HOLD_OCC * freeAt[f], raw - 1.0));
            const c = FINGER_JACK_CRIT_MS / Math.sqrt(gap * gap + JACK_FLOOR_MS * JACK_FLOOR_MS);
            const pen = c * c;
            addSpan(rate, grid, l, note.t, (1.0 + pen) * gapRate(gap, 0.0) * raw / (note.t - l));
        }
        index[f] += 1;
        last[f] = note.t;
        freeAt[f] = note.tail >= note.t
            ? Math.max(note.tail - note.t - LN_TAP_MS, 0.0)
            : 0.0;
    }
    return techSum(grid, rate);
}

// 动作：每个间隔按它结束时的动作加权（和弦、重复、交替）。
function motionCost(x, handNotes, grid) {
    const times = [];
    const masks = [];
    for (const note of handNotes) {
        if (times.length > 0 && times[times.length - 1] === note.t) {
            masks[masks.length - 1] |= 1 << note.f;
            continue;
        }
        times.push(note.t);
        masks.push(1 << note.f);
    }
    const xt = MOTION_X_COEF * Math.pow(x, 0.25);
    const rate = zeros(grid.length);
    for (let i = 0; i + 1 < times.length; i += 1) {
        const gap = softenGap(times[i + 1] - times[i]);
        let w;
        if (masks[i + 1] === 3) w = 1.0;
        else if ((masks[i + 1] & masks[i]) !== 0) w = MOTION_REPEAT_W;
        else w = 1.0 + (ALT_CRIT_MS / gap) * (ALT_CRIT_MS / gap);
        addSpan(rate, grid, times[i], times[i + 1], w * gapRate(gap, xt));
    }
    return techSum(grid, rate);
}

function fingerMix(jack, motion) {
    const a = jack;
    const b = MOTION_W * motion;
    const hi = Math.max(a, b);
    if (hi <= 0.0) return 0.0;
    return hi * Math.pow(1.0 + Math.pow(Math.min(a, b) / hi, FINGER_MIX_P), 1.0 / FINGER_MIX_P);
}

// 一次 stroke：和弦，或一个音符加上另一只手在它之前的第一个音（若合适）。
function groupStrokes(handNotes) {
    const fingerTimes = [[], []];
    for (let i = 0; i < handNotes.length; i += 1) {
        const note = handNotes[i];
        const chord = (i > 0 && handNotes[i - 1].t === note.t)
            || (i + 1 < handNotes.length && handNotes[i + 1].t === note.t);
        if (!chord) fingerTimes[note.f].push(note.t);
    }
    const strokes = { t: [], width: [], size: [], mask: [], lead: [], first: [] };
    for (let i = 0; i < handNotes.length;) {
        let j = i + 1;
        if (j < handNotes.length
            && handNotes[j].f !== handNotes[i].f
            && handNotes[j].t - handNotes[i].t <= STROKE_CAP_MS
            && !(j + 1 < handNotes.length
                && handNotes[j + 1].f === handNotes[i].f
                && handNotes[j + 1].t - handNotes[j].t
                    < (handNotes[j].t - handNotes[i].t) * (1.0 - STROKE_TIE_FRAC))) {
            j += 1;
        }
        strokes.t.push(0.5 * (handNotes[i].t + handNotes[j - 1].t));
        let width = handNotes[j - 1].t - handNotes[i].t;
        if (j - i === 1) {
            const other = fingerTimes[1 - handNotes[i].f];
            let d = STROKE_CAP_MS;
            const q = lowerBound(other, handNotes[i].t);
            if (q < other.length) d = Math.min(d, other[q] - handNotes[i].t);
            if (q > 0) d = Math.min(d, handNotes[i].t - other[q - 1]);
            width = d < STROKE_CAP_MS ? d : 0.0;
        }
        strokes.width.push(width);
        strokes.size.push(j - i);
        strokes.lead.push(handNotes[i].f);
        strokes.first.push(i);
        let mask = 0;
        for (let q = i; q < j; q += 1) mask |= 1 << handNotes[q].f;
        strokes.mask.push(mask);
        i = j;
    }
    return strokes;
}

// note i 周围的重复程度：1 减去最佳 lag 的缺失比例，再减去 lag 代价。
function noteConsistency(noteTimes, noteColumns, i) {
    const n = noteTimes.length;
    const a = i >= MANIP_WINDOW ? i - MANIP_WINDOW : 0;
    const b = Math.min(n, i + MANIP_WINDOW + 1);
    let best = 0.0;
    for (let p = 1; p <= MANIP_PERIOD_MAX; p += 1) {
        if (b < a + p + 4) break;
        let miss = 0.0;
        let count = 0;
        for (let q = a; q + p < b; q += 1) {
            if (q + p + 1 >= b) break;
            if (noteColumns[q] !== noteColumns[q + p]
                || noteColumns[q + 1] !== noteColumns[q + p + 1]) {
                miss += 1.0;
                count += 1;
                continue;
            }
            const g1 = noteTimes[q + 1] - noteTimes[q];
            const g2 = noteTimes[q + p + 1] - noteTimes[q + p];
            const med = 0.5 * (g1 + g2);
            const tol = Math.sqrt(MANIP_PACE * med * MANIP_PACE * med
                + MANIP_PACE_FLOOR_MS * MANIP_PACE_FLOOR_MS);
            miss += smoothstep01(Math.abs(g1 - g2) / tol);
            count += 1;
        }
        if (count <= 0) continue;
        best = Math.max(best, 1.0 - miss / count - MANIP_PERIOD_COST * (p - 1));
    }
    return best;
}

// 每个 stroke 的 consistency：其音符的平均值，按双手合并后的序列读取。
function strokeConsistency(handNotesByHand, strokesByHand) {
    const all = [];
    for (let h = 0; h < 2; h += 1) {
        for (let q = 0; q < handNotesByHand[h].length; q += 1) {
            const note = handNotesByHand[h][q];
            all.push({ t: note.t, c: 2 * h + note.f, h, q });
        }
    }
    all.sort((a, b) => (a.t !== b.t ? a.t - b.t : a.c - b.c));
    // 相距 MANIP_FLAM_MS 内的音符共享组首时间，列按顺序排列。
    for (let e = 0; e < all.length;) {
        let f = e + 1;
        while (f < all.length && all[f].t - all[e].t <= MANIP_FLAM_MS) {
            all[f].t = all[e].t;
            f += 1;
        }
        const group = all.slice(e, f).sort((a, b) => a.c - b.c);
        for (let k = 0; k < group.length; k += 1) all[e + k] = group[k];
        e = f;
    }
    const noteTimes = all.map((event) => event.t);
    const noteColumns = all.map((event) => event.c);
    const per = [zeros(handNotesByHand[0].length), zeros(handNotesByHand[1].length)];
    for (let e = 0; e < all.length; e += 1) {
        per[all[e].h][all[e].q] = noteConsistency(noteTimes, noteColumns, e);
    }
    const out = [];
    for (let h = 0; h < 2; h += 1) {
        const strokes = strokesByHand[h];
        const values = zeros(strokes.t.length);
        for (let k = 0; k < values.length; k += 1) {
            let sum = 0.0;
            for (let q = 0; q < strokes.size[k]; q += 1) {
                sum += per[h][strokes.first[k] + q];
            }
            values[k] = sum / strokes.size[k];
        }
        out.push(values);
    }
    return out;
}

function wristCost(handNotes, strokes, consistency, otherTimes, grid) {
    const n = strokes.t.length;
    const chord = zeros(n);
    const manip = zeros(n);
    const merged = zeros(n);
    const strike = zeros(n);
    for (let k = 0; k < n; k += 1) {
        let period = 0.0;
        if (k > 0) period = Math.max(period, strokes.t[k] - strokes.t[k - 1]);
        if (k + 1 < n) period = Math.max(period, strokes.t[k + 1] - strokes.t[k]);
        const phase = strokes.width[k] <= 0.0 ? 0.0 : (period > 0.0 ? strokes.width[k] / period : 1.0);
        const near = 1.0 - smoothstep01(phase / CHORD_PHASE);
        const pair = strokes.size[k] === 2 && strokes.width[k] > 0.0;
        chord[k] = pair ? 0.0 : near;
        // 一对音符之间夹着另一只手的音符时不能合并。
        const lo = strokes.t[k] - 0.5 * strokes.width[k];
        const hi = strokes.t[k] + 0.5 * strokes.width[k];
        const q = lowerBound(otherTimes, lo);
        const across = q < otherTimes.length && otherTimes[q] > lo && otherTimes[q] < hi;
        manip[k] = pair && !across
            ? 1.0 - (1.0 - near) * (1.0 - smoothstep01((consistency[k] - MANIP_CONS_LO) / (MANIP_CONS_HI - MANIP_CONS_LO)))
            : 0.0;
        merged[k] = 1.0 - (1.0 - chord[k]) * (1.0 - manip[k]);
        strike[k] = strokes.size[k] === 1 ? 1.0 : smoothstep01((merged[k] - 0.2) / 0.6);
    }

    // anchor run：连续 strike 中同一手指一直在场，做软化以让 rate 平滑移动。
    const run = zeros(n);
    for (let f = 1; f <= 2; f <<= 1) {
        const forward = zeros(n);
        const backward = zeros(n);
        for (let k = 0; k < n; k += 1) {
            if (strokes.mask[k] & f) {
                forward[k] = strike[k] * (1.0 + (k > 0 && (strokes.mask[k - 1] & f) ? forward[k - 1] : 0.0));
            }
        }
        for (let k = n - 1; k >= 0; k -= 1) {
            if (strokes.mask[k] & f) {
                backward[k] = strike[k] * (1.0 + (k + 1 < n && (strokes.mask[k + 1] & f) ? backward[k + 1] : 0.0));
            }
        }
        for (let k = 0; k < n; k += 1) {
            if (strokes.mask[k] & f) {
                run[k] = Math.max(run[k], forward[k] + backward[k] - strike[k]);
            }
        }
    }

    // jitter 的 repeat run：取手内行序列与 stroke 序列中较长者；只有真正的休息才打断。
    const rep = new Array(n).fill(1.0);
    {
        const rowTimesList = [];
        const rowMasks = [];
        const rowOf = zeros(handNotes.length);
        for (let q = 0; q < handNotes.length; q += 1) {
            if (rowTimesList.length === 0 || handNotes[q].t !== rowTimesList[rowTimesList.length - 1]) {
                rowTimesList.push(handNotes[q].t);
                rowMasks.push(0);
            }
            rowMasks[rowMasks.length - 1] |= 1 << handNotes[q].f;
            rowOf[q] = rowTimesList.length - 1;
        }
        const nr = rowTimesList.length;
        const jacked = new Array(nr).fill(false);
        for (let k = 0; k < nr; k += 1) {
            jacked[k] = (k > 0 && rowMasks[k - 1] === rowMasks[k])
                || (k + 1 < nr && rowMasks[k + 1] === rowMasks[k]);
        }
        const pace = (g1, g2) => 1.0 - smoothstep01(
            Math.max(g1, g2) / Math.max(Math.min(g1, g2), 1.0) - 2.0,
        );
        const same = zeros(nr);
        for (let k = 1; k < nr; k += 1) {
            if (rowMasks[k] === rowMasks[k - 1]
                || (jacked[k] && jacked[k - 1] && rowMasks[k] !== 3 && rowMasks[k - 1] !== 3)) {
                same[k] = pace(
                    k >= 2 ? rowTimesList[k - 1] - rowTimesList[k - 2] : rowTimesList[k] - rowTimesList[k - 1],
                    rowTimesList[k] - rowTimesList[k - 1],
                );
            }
        }
        const forward = zeros(nr);
        const backward = zeros(nr);
        for (let k = 1; k < nr; k += 1) forward[k] = same[k] * (1.0 + forward[k - 1]);
        for (let k = nr - 1; k >= 1; k -= 1) backward[k - 1] = same[k] * (1.0 + backward[k]);

        const strokeSame = zeros(n);
        for (let k = 1; k < n; k += 1) {
            if (strokes.mask[k] === strokes.mask[k - 1]) {
                strokeSame[k] = pace(
                    k >= 2 ? strokes.t[k - 1] - strokes.t[k - 2] : strokes.t[k] - strokes.t[k - 1],
                    strokes.t[k] - strokes.t[k - 1],
                );
            }
        }
        const strokeForward = zeros(n);
        const strokeBackward = zeros(n);
        for (let k = 1; k < n; k += 1) strokeForward[k] = strokeSame[k] * (1.0 + strokeForward[k - 1]);
        for (let k = n - 1; k >= 1; k -= 1) strokeBackward[k - 1] = strokeSame[k] * (1.0 + strokeBackward[k]);

        for (let k = 0; k < n; k += 1) {
            const r = rowOf[strokes.first[k]];
            rep[k] = 1.0 + Math.max(forward[r] + backward[r], strokeForward[k] + strokeBackward[k]);
        }
    }

    const rate = zeros(grid.length);
    const rateManip = zeros(grid.length);
    const able = zeros(grid.length);
    const manipCurve = zeros(grid.length);
    const jitterWeight = zeros(grid.length);
    for (let i = 0; i + 1 < n; i += 1) {
        const gap = strokes.t[i + 1] - strokes.t[i];
        const r = (strokes.mask[i + 1] & strokes.mask[i]) !== 0
            ? smoothstep01((run[i] - ANCHOR_RUN_LO) / (ANCHOR_RUN_HI - ANCHOR_RUN_LO))
            : 0.0;
        const a = 1.0 - (1.0 - chord[i]) * (1.0 - r);
        addSpan(able, grid, strokes.t[i], strokes.t[i + 1], a);
        addSpan(manipCurve, grid, strokes.t[i], strokes.t[i + 1], manip[i]);
        const j = smoothstep01((rep[i] - JITTER_REP_LO) / (JITTER_REP_HI - JITTER_REP_LO));
        const share = gap > 0.0 && gap < JITTER_ONSET_MS
            ? Math.pow(gap / JITTER_ONSET_MS, JITTER_POW)
            : 1.0;
        addSpan(jitterWeight, grid, strokes.t[i], strokes.t[i + 1], j * Math.min(1.0, (1.0 - share) / 0.1));
        const load = 1.0 + WRIST_CHORD_W * (strokes.size[i + 1] - 1.0);
        const off = strokes.size[i + 1] === 2 ? strokes.width[i + 1] / MANIP_TOL_MS : 0.0;
        const c = load * gapRate(gap, 0.0)
            * (gap > 0.0 ? Math.pow(WRIST_GAP_PIVOT_MS / gap, WRIST_GAP_POW - 2.0) : 1.0)
            * (1.0 - j * (1.0 - share));
        addSpan(rate, grid, strokes.t[i], strokes.t[i + 1], c);
        addSpan(rateManip, grid, strokes.t[i], strokes.t[i + 1], c * (1.0 + Math.pow(off, MANIP_TOL_POW)));
    }

    const cost = techSum(grid, rate).map((value) => value * WRIST_W);
    const costManip = techSum(grid, rateManip).map((value) => value * WRIST_W);
    return {
        cost,
        costManip,
        able: windowAvg(grid, able, WRIST_BLEND_MS),
        manip: windowAvg(grid, manipCurve, WRIST_BLEND_MS),
        jitw: windowAvg(grid, jitterWeight, WRIST_BLEND_MS),
    };
}

function handCost(finger, wrist, index) {
    const w = clamp(wrist.able[index], 0.0, 1.0);
    const m = clamp(wrist.manip[index], 0.0, 1.0);
    const h = (1.0 - w) * ((1.0 - m) * finger + m * Math.min(finger, wrist.costManip[index]))
        + w * wrist.cost[index];
    const j = clamp(wrist.jitw[index], 0.0, 1.0);
    return (1.0 - j) * h + j * Math.min(h, wrist.costManip[index]);
}

// 列 0-1 为左手，2-3 为右手。返回每只手的 strain，并填充每只手的 wristability。
function techniqueStrain(x, byColumn, grid) {
    const handNotes = [[], []];
    const strokes = [null, null];
    for (let h = 0; h < 2; h += 1) {
        const list = handNotes[h];
        for (let c = 2 * h; c < 2 * h + 2; c += 1) {
            for (const note of byColumn[c]) {
                list.push({ t: note.h, f: c - 2 * h, tail: note.t });
            }
        }
        list.sort((a, b) => (a.t !== b.t ? a.t - b.t : a.f - b.f));
        strokes[h] = groupStrokes(list);
    }
    const consistency = strokeConsistency(handNotes, strokes);

    const out = [[], []];
    const able = [[], []];
    for (let h = 0; h < 2; h += 1) {
        const notes = handNotes[h];
        const jack = repeatCost(notes, grid);
        const motion = motionCost(x, notes, grid);
        const other = handNotes[1 - h].map((note) => note.t);
        const wrist = wristCost(notes, strokes[h], consistency[h], other, grid);
        out[h] = zeros(grid.length);
        able[h] = zeros(grid.length);
        for (let i = 0; i < grid.length; i += 1) {
            out[h][i] = handCost(fingerMix(jack[i], motion[i]), wrist, i);
            const w = clamp(wrist.able[i], 0.0, 1.0);
            const m = clamp(wrist.manip[i], 0.0, 1.0);
            able[h][i] = w + (1.0 - w) * m;
        }
    }
    return { strain: out, able };
}

// ── 体力：每只手一个能量槽，高于地图自身等级时消耗、低于时回复，另有缓慢耐久损失 ──
const STAMINA_GAIN = 2.2908;
const STAMINA_PROP = 0.90996;
const STAMINA_CAP = 1.6526;
const STAMINA_TANK = 50.918;
const STAMINA_REFILL_REST_MS = 5000.0;
const STAMINA_REFILL_LEVEL_MS = 15000.0;
const STAM_INTERVAL_MS = 500.0;
const STAMINA_DURABILITY = 0.029925;
const STAMINA_LEVEL_POW = 5.0;
const STAMINA_EASY = 0.17545;
const STAMINA_EASY_WINDOW_MS = 4000;

function mapLevel(grid, strain, weight) {
    const n = strain[0].length;
    const both = zeros(n);
    for (let i = 0; i < n; i += 1) both[i] = 0.5 * (strain[0][i] + strain[1][i]);
    const g = windowAvg(grid, both, STAMINA_EASY_WINDOW_MS);
    let acc = 0.0;
    let totalWeight = 0.0;
    for (let h = 0; h < 2; h += 1) {
        for (let i = 0; i < n; i += 1) {
            if (!(g[i] > 0.0)) continue;
            const wd = Math.pow(g[i], STAMINA_LEVEL_POW) * weight[h][i];
            acc += wd * g[i];
            totalWeight += wd;
        }
    }
    if (totalWeight <= 0.0 || acc <= 0.0) return 0.0;
    const easy = STAMINA_EASY * acc / totalWeight;
    let acc5 = 0.0;
    let weight5 = 0.0;
    for (let h = 0; h < 2; h += 1) {
        for (let i = 0; i < n; i += 1) {
            if (g[i] < easy) continue;
            const d = strain[h][i];
            const d2 = d * d;
            acc5 += d2 * d2 * d * weight[h][i];
            weight5 += weight[h][i];
        }
    }
    if (weight5 <= 0.0 || acc5 <= 0.0) return 0.0;
    return Math.pow(acc5 / weight5, 0.2);
}

function runTank(grid, strain, level) {
    const modCurve = new Array(strain.length).fill(1.0);
    let tank = 1.0;
    let prev = 0.0;
    let work = 0.0;
    for (let i = 0; i < strain.length; i += 1) {
        const step = i === 0 ? 0.0 : (grid[i] - grid[i - 1]) / STAM_INTERVAL_MS;
        const average = 0.5 * (prev + strain[i]);
        prev = strain[i];
        const rel = average / level;
        if (rel > 1.0) {
            tank -= (rel - 1.0) * step / STAMINA_TANK;
        } else {
            const tau = STAMINA_REFILL_REST_MS + STAMINA_REFILL_LEVEL_MS * rel * rel;
            tank += (1.0 - tank) * (1.0 - Math.exp(-step * STAM_INTERVAL_MS / tau));
        }
        tank = Math.max(tank, 0.0);
        work += rel * step * STAM_INTERVAL_MS / 60000.0;
        modCurve[i] = (1.0 + (STAMINA_CAP - 1.0) * (1.0 - tank))
            * (1.0 + STAMINA_DURABILITY * Math.sqrt(work));
    }
    return modCurve;
}

function applyStamina(grid, strain, weight) {
    const level = STAMINA_PROP * mapLevel(grid, strain, weight);
    if (!(level > 0.0)) return;
    for (let h = 0; h < 2; h += 1) {
        const mod = runTank(grid, strain[h], level);
        for (let i = 0; i < mod.length; i += 1) {
            strain[h][i] *= Math.max(0.0, 1.0 + STAMINA_GAIN * (mod[i] - 1.0));
        }
    }
}

// ── 双手合并 ────────────────────────────────────────────────────────────────
const HAND_P_SYNC = 6.1324;
const HAND_P_ALT = 2.526;
const SWITCH_MIX = 0.16864;
const SWITCH_WINDOW_MS = 500.0;

function handSync(able, rows, grid) {
    const sync = zeros(able[0].length);
    const switchCurve = zeros(grid.length);
    const points = [];
    const changes = [];
    for (let r = 1; r < rows.length; r += 1) {
        const a = rows[r - 1].mask;
        const b = rows[r].mask;
        if (rows[r - 1].size !== 1 || rows[r].size !== 1) continue;
        points.push(rows[r].t);
        changes.push(((a & 3) !== 0) !== ((b & 3) !== 0) ? 1.0 : 0.0);
    }
    const cumulative = zeros(changes.length + 1);
    for (let k = 0; k < changes.length; k += 1) cumulative[k + 1] = cumulative[k] + changes[k];
    let lo = 0;
    let hi = 0;
    for (let i = 0; i < grid.length; i += 1) {
        while (lo < points.length && points[lo] < grid[i] - SWITCH_WINDOW_MS) lo += 1;
        while (hi < points.length && points[hi] <= grid[i] + SWITCH_WINDOW_MS) hi += 1;
        switchCurve[i] = hi > lo ? (cumulative[hi] - cumulative[lo]) / (hi - lo) : 0.0;
    }
    for (let i = 0; i < sync.length; i += 1) {
        const a = clamp(0.5 * (able[0][i] + able[1][i]), 0.0, 1.0);
        sync[i] = clamp((1.0 - SWITCH_MIX) * a + SWITCH_MIX * (1.0 - switchCurve[i]), 0.0, 1.0);
    }
    return sync;
}

function combineHands(strain, sync) {
    const out = zeros(strain[0].length);
    for (let i = 0; i < out.length; i += 1) {
        const a = Math.max(strain[0][i], 0.0);
        const b = Math.max(strain[1][i], 0.0);
        const hi = Math.max(a, b);
        if (hi <= 0.0) continue;
        if (Math.min(a, b) <= 0.0) {
            out[i] = hi;
            continue;
        }
        const s = clamp(sync[i], 0.0, 1.0);
        const p = HAND_P_ALT + (HAND_P_SYNC - HAND_P_ALT) * s;
        const lo = Math.min(a, b) / hi;
        out[i] = hi * Math.pow(1.0 + Math.pow(lo, p), 1.0 / p);
    }
    return out;
}

// ── 长条代价：释放时机、每个 hold 的固定权重、按住时另一列被按下 ──────────────
const HOLD_PRESS_W = [
    [0.00, 1.00, 0.20, 0.15],
    [1.00, 0.00, 0.20, 0.20],
    [0.20, 0.20, 0.00, 1.00],
    [0.15, 0.20, 1.00, 0.00],
];
const LN_WEIGHT = 0.05;
const REL_GAIN = 0.75;
const REL_MID = 0.75;
const REL_GAP_WEIGHT = 0.5;
const LX_GAIN = 0.8;
const LX_RAMP_MS = 150.0;
const LX_STACK = 1.0;

function holdCosts(x, byColumn, noteSequence, grid) {
    const tailSequence = noteSequence.filter((note) => note.t >= 0);
    tailSequence.sort((a, b) => a.t - b.t);
    const releaseStep = zeros(grid.length);
    const weightStep = zeros(grid.length);
    const crossStep = zeros(grid.length);
    const invX = 1.0 / x;
    const sigmoid = (value) => 1.0 / (1.0 + Math.exp(-5.0 * (value - REL_MID)));
    const columnTimes = byColumn.map((notes) => headTimes(notes));

    // release：按超过 LN_TAP_MS 的 hold 长度与列内下一个头的间隔计分，节奏由连续尾时间决定。
    const releases = [];
    const intensities = [];
    for (const note of tailSequence) {
        const length = note.t - note.h;
        if (length <= LN_TAP_MS) continue;
        let nextHead = 1e9;
        const times = columnTimes[note.k];
        const index = lowerBound(times, note.h);
        if (index + 1 < times.length) nextHead = times[index + 1];
        const sHead = sigmoid(0.001 * (length - LN_TAP_MS) * invX);
        const sTail = sigmoid(0.001 * Math.max(nextHead - note.t, 0.0) * invX);
        releases.push(note);
        intensities.push((sHead + REL_GAP_WEIGHT * sTail) / (1.0 + REL_GAP_WEIGHT));
    }
    for (let i = 0; i + 1 < releases.length; i += 1) {
        const tStart = releases[i].t;
        const tEnd = releases[i + 1].t;
        if (tEnd <= tStart) continue;
        const delta = 0.001 * (tEnd - tStart);
        const value = REL_GAIN * 0.08 / Math.sqrt(delta) * invX
            * (1.0 + 0.8 * (intensities[i] + intensities[i + 1]));
        addSpan(releaseStep, grid, tStart, tEnd, value);
    }

    // weight：每个 hold 在头上给一个固定权重。
    for (const note of noteSequence) {
        if (note.t < 0) continue;
        addSpan(weightStep, grid, note.h, note.h + 1.0, 1000.0 * LN_WEIGHT);
    }

    // cross：每次按下与另一列仍存活的 hold 计代价，LX_RAMP_MS 内渐入；头/尾按下免费。
    const holds = [[], [], [], []];
    for (let k = 0; k < 4; k += 1) {
        for (const note of byColumn[k]) {
            if (note.t >= 0) holds[k].push(note);
        }
    }
    const cursor = [0, 0, 0, 0];
    for (const press of noteSequence) {
        const t = press.h;
        let sum = 0.0;
        let worst = 0.0;
        for (let k = 0; k < 4; k += 1) {
            if (k === press.k) continue;
            const columnHolds = holds[k];
            while (cursor[k] < columnHolds.length && columnHolds[cursor[k]].t < t) cursor[k] += 1;
            if (cursor[k] >= columnHolds.length) continue;
            const hold = columnHolds[cursor[k]];
            const d = Math.min(t - hold.h, hold.t - t);
            if (d <= 0.0) continue;
            const pair = HOLD_PRESS_W[k][press.k] * Math.min(1.0, d / LX_RAMP_MS);
            sum += pair;
            worst = Math.max(worst, pair);
        }
        if (sum <= 0.0) continue;
        const accumulated = worst + LX_STACK * (sum - worst);
        addSpan(crossStep, grid, t, t + 1.0, 1000.0 * LX_GAIN * accumulated);
    }

    return {
        rel: windowSum(grid, releaseStep, WINDOW_MS, 0.001),
        weight: windowSum(grid, weightStep, WINDOW_MS, 0.001),
        cross: windowSum(grid, crossStep, WINDOW_MS, 0.001),
    };
}

// ── 期望准确率聚合 ──────────────────────────────────────────────────────────
const RATING_SCALE = 7.528655953984781;
const AGG_POWER = 3.8724;
const AGG_TARGET_LOSS = 0.02;
const SOLVE_STEPS = 40;
const NOTE_DAMP_N = 60.0;
const SCALE_A = 30.0;
const SCALE_C = 290.0;

function binStrain(strain) {
    const bin = new Array(strain.length).fill(-1);
    const lo = Math.log(1e-4);
    const step = Math.log(1.01);
    const groups = new Map();
    for (let i = 0; i < strain.length; i += 1) {
        if (!(strain[i] > 1e-4)) continue;
        const key = Math.trunc((Math.log(strain[i]) - lo) / step);
        if (!groups.has(key)) groups.set(key, []);
        groups.get(key).push(i);
    }
    const keys = [...groups.keys()].sort((a, b) => a - b);
    const dp = [];
    for (const key of keys) {
        const indexes = groups.get(key);
        let sum = 0.0;
        for (const index of indexes) sum += strain[index];
        for (const index of indexes) bin[index] = dp.length;
        dp.push(Math.pow(sum / indexes.length, AGG_POWER));
    }
    return { bin, dp };
}

// 在 log s 上二分，求期望损失为 AGG_TARGET_LOSS * meanWeight 的技能值。
function solveSkill(binWeights, dp, meanWeight) {
    const target = AGG_TARGET_LOSS * meanWeight;
    let capacity = 0.0;
    let maxDp = 0.0;
    for (let b = 0; b < binWeights.length; b += 1) {
        if (binWeights[b] > 0.0) {
            capacity += binWeights[b];
            maxDp = Math.max(maxDp, dp[b]);
        }
    }
    if (capacity <= target || maxDp <= 0.0) return 0.0;
    let lo = Math.log(maxDp) - 200.0;
    let hi = Math.log(maxDp) + 60.0;
    for (let step = 0; step < SOLVE_STEPS; step += 1) {
        const mid = 0.5 * (lo + hi);
        const inv = Math.exp(-mid);
        let loss = 0.0;
        for (let b = 0; b < binWeights.length; b += 1) {
            if (binWeights[b] > 0.0) loss += binWeights[b] * (1.0 - Math.exp(-0.05 * dp[b] * inv));
        }
        if (loss > target) lo = mid;
        else hi = mid;
    }
    return Math.exp(0.5 * (lo + hi) / AGG_POWER);
}

function ratingFromBins(binned, weights, totalNotes) {
    let totalWeight = 0.0;
    for (const weight of weights) totalWeight += weight;
    if (totalWeight <= 0.0) return 0.0;
    const binWeights = zeros(binned.dp.length);
    for (let i = 0; i < weights.length; i += 1) {
        if (binned.bin[i] >= 0) binWeights[binned.bin[i]] += weights[i];
    }
    let raw = solveSkill(binWeights, binned.dp, totalWeight);
    raw *= totalNotes / (totalNotes + NOTE_DAMP_N);
    return raw * 0.975 * RATING_SCALE;
}

function noteWeights(grid, notes) {
    const times = headTimes(notes);
    const n = grid.length;
    const weights = zeros(n);
    let lo = 0;
    let hi = 0;
    for (let i = 0; i < n; i += 1) {
        while (lo < times.length && times[lo] < grid[i] - WINDOW_MS) lo += 1;
        while (hi < times.length && times[hi] < grid[i] + WINDOW_MS) hi += 1;
        const gap = i === 0
            ? (grid[1] - grid[0]) / 2.0
            : (i === n - 1 ? (grid[n - 1] - grid[n - 2]) / 2.0 : (grid[i + 1] - grid[i - 1]) / 2.0);
        weights[i] = (hi - lo) * gap;
    }
    return weights;
}

function ratingPoints(raw) {
    return raw > 0.0 ? SCALE_A * Math.log1p(raw / SCALE_C) : 0.0;
}

function strainPoints(strain) {
    return ratingPoints(strain * 0.975 * RATING_SCALE);
}

const GRID_MS = 25.0;
const DUP_MS = 10;
const LN_W = 3.997;
const LN_X = 1.3275;

// 音符按 (h,k) 排序、非负 ms 且已应用速率；同列 DUP_MS 内重复的音符合并到前一个。
function analyse(notes) {
    const sequence = [];
    const last = [-1, -1, -1, -1];
    for (const note of notes) {
        const j = last[note.k];
        if (j >= 0 && note.h - sequence[j].h <= DUP_MS) {
            sequence[j].t = Math.max(sequence[j].t, note.t);
            continue;
        }
        last[note.k] = sequence.length;
        sequence.push({ ...note });
    }
    if (sequence.length < 2) {
        return { rating: 0.0, grid: [], graph: [] };
    }

    const byColumn = [[], [], [], []];
    const byHand = [[], []];
    let end = 0;
    for (const note of sequence) {
        byColumn[note.k].push(note);
        byHand[Math.floor(note.k / 2)].push(note);
        end = Math.max(end, note.h, note.t);
    }
    const grid = [];
    for (let t = 0.0; t <= end + 1000.0; t += GRID_MS) grid.push(t);
    const x = hitWindowUnit();

    const { strain, able } = techniqueStrain(x, byColumn, grid);

    // 每只手的 hold 代价加到按压 strain 上。
    const holds = [null, null];
    for (let h = 0; h < 2; h += 1) {
        const columns = [[], [], [], []];
        for (let c = 2 * h; c < 2 * h + 2; c += 1) columns[c] = byColumn[c];
        holds[h] = holdCosts(x, columns, byHand[h], grid);
        for (let i = 0; i < grid.length; i += 1) {
            const l = holds[h].rel[i] + holds[h].weight[i] + holds[h].cross[i];
            if (l > 0.0) strain[h][i] += LN_W * Math.pow(l, LN_X);
        }
    }
    const rested = strain.map((values) => values.slice());

    // 体力（每只手）。
    const weights = [noteWeights(grid, byHand[0]), noteWeights(grid, byHand[1])];
    applyStamina(grid, strain, weights);

    // 合并双手并聚合。
    const rows = buildRows(sequence);
    const sync = handSync(able, rows, grid);
    const combined = combineHands(strain, sync);
    const graph = combineHands(rested, sync).map(strainPoints);

    const overallWeights = noteWeights(grid, sequence);
    const totalNotes = sequence.length;
    const binned = binStrain(combined);
    const raw = ratingFromBins(binned, overallWeights, totalNotes);

    return { rating: ratingPoints(raw), grid, graph };
}

// ── rating → dan 标定 ───────────────────────────────────────────────────────
// C++ 的 `rating` 是 0–100 量纲的显示值（例如 97.8），不是段位数值。作者提交的
// C++ 的 `rating` 是 0–100 显示值，不是段位。上游另有一条 rating→dan 标定，
// 但未随公开 C++ 发布；这里用作者发布的参考输出重建一条只依赖 rating 的单调折线
// （节点间线性插值）。56.2281 是作者判定 "Alpha Low 以下 Invalid" 的边界。
const XDC1_SCOPE_MIN_RATING = 56.2281;
// knots=302
const XDC1_RATING_DAN_KNOTS = Object.freeze([
    [56.2457, 10.5], [56.2855, 10.52], [56.4662, 10.58], [56.6748, 10.66], [56.6979, 10.66], [56.8091, 10.7], 
    [56.9228, 10.75], [56.9387, 10.75], [56.9793, 10.77], [56.9979, 10.77], [57.0081, 10.78], [57.2723, 10.88], 
    [57.7027, 11.03], [57.7613, 11.06], [57.8102, 11.07], [57.8729, 11.1], [58.0741, 11.17], [58.0859, 11.18], 
    [58.8904, 11.47], [59.0046, 11.52], [59.1394, 11.56], [59.2864, 11.62], [59.3629, 11.64], [59.5332, 11.71], 
    [59.7566, 11.78], [59.9043, 11.84], [59.9836, 11.86], [60.4924, 12.04], [60.8486, 12.15], [61.1076, 12.24], 
    [61.225, 12.27], [61.3015, 12.3], [61.3909, 12.32], [61.8702, 12.47], [62.0354, 12.51], [62.1477, 12.55], 
    [62.2152, 12.56], [62.2251, 12.57], [62.4736, 12.64], [62.5819, 12.66], [62.7811, 12.72], [62.9239, 12.75], 
    [63.1747, 12.82], [63.2892, 12.84], [63.6497, 12.93], [64.0341, 13.01], [64.0417, 13.02], [64.1167, 13.03], 
    [64.2274, 13.06], [64.6646, 13.14], [64.6774, 13.15], [64.776, 13.17], [64.9719, 13.2], [65.1055, 13.23], 
    [65.1436, 13.23], [65.1539, 13.24], [65.2878, 13.26], [65.3145, 13.27], [65.4167, 13.28], [65.7317, 13.34], 
    [65.7761, 13.34], [65.7848, 13.35], [65.8349, 13.35], [66.0271, 13.39], [66.147, 13.4], [66.3024, 13.43], 
    [66.3418, 13.43], [66.3663, 13.44], [66.5209, 13.46], [66.5409, 13.47], [66.6634, 13.48], [66.7362, 13.5], 
    [66.7933, 13.5], [66.8095, 13.51], [66.8476, 13.51], [66.8772, 13.52], [67.2771, 13.57], [67.5118, 13.61], 
    [67.5686, 13.61], [67.7197, 13.64], [67.7625, 13.64], [67.7925, 13.65], [67.9381, 13.67], [67.9881, 13.67], 
    [68.0871, 13.69], [68.1492, 13.69], [68.1598, 13.7], [68.2924, 13.71], [68.3093, 13.72], [68.38, 13.72], 
    [68.4731, 13.74], [68.615, 13.75], [68.6215, 13.76], [68.7583, 13.77], [68.7744, 13.78], [68.8377, 13.78], 
    [68.8557, 13.79], [68.9151, 13.79], [68.9306, 13.8], [69.0069, 13.8], [69.017, 13.81], [69.0842, 13.81], 
    [69.1041, 13.82], [69.1629, 13.82], [69.175, 13.83], [69.2287, 13.83], [69.248, 13.84], [69.3271, 13.84], 
    [69.3657, 13.85], [69.4067, 13.85], [69.4107, 13.86], [69.4818, 13.86], [69.5068, 13.87], [69.5477, 13.87], 
    [69.5719, 13.88], [69.6477, 13.88], [69.6548, 13.89], [69.7284, 13.89], [69.7482, 13.9], [69.8116, 13.9], 
    [69.8224, 13.91], [69.8932, 13.91], [69.92, 13.92], [69.9782, 13.92], [69.9869, 13.93], [70.0357, 13.93], 
    [70.0666, 13.94], [70.1457, 13.94], [70.1506, 13.95], [70.2254, 13.95], [70.244, 13.96], [70.3066, 13.96], 
    [70.3194, 13.97], [70.3934, 13.97], [70.402, 13.98], [70.4753, 13.98], [70.4834, 13.99], [70.5603, 13.99], 
    [70.5692, 14], [70.647, 14], [70.65, 14.01], [70.7332, 14.01], [70.7368, 14.02], [70.8105, 14.02], 
    [70.8187, 14.03], [70.8975, 14.03], [70.9041, 14.04], [70.9857, 14.04], [70.9969, 14.05], [71.053, 14.05], 
    [71.0759, 14.06], [71.1564, 14.06], [71.1613, 14.07], [71.2388, 14.07], [71.2423, 14.08], [71.3266, 14.08], 
    [71.3522, 14.09], [71.4048, 14.09], [71.4122, 14.1], [71.5797, 14.11], [71.6064, 14.12], [71.6637, 14.12], 
    [71.6789, 14.13], [71.751, 14.13], [71.7559, 14.14], [71.8359, 14.14], [71.8491, 14.15], [71.922, 14.15], 
    [71.9238, 14.16], [72.0061, 14.16], [72.0124, 14.17], [72.0811, 14.17], [72.0966, 14.18], [72.1747, 14.18], 
    [72.1799, 14.19], [72.257, 14.19], [72.2684, 14.2], [72.3338, 14.2], [72.35, 14.21], [72.4286, 14.21], 
    [72.4397, 14.22], [72.5129, 14.22], [72.5221, 14.23], [72.5981, 14.23], [72.6134, 14.24], [72.6868, 14.24], 
    [72.6932, 14.25], [72.7629, 14.25], [72.7733, 14.26], [72.8507, 14.26], [72.8653, 14.27], [72.9365, 14.27], 
    [72.9603, 14.28], [73.0111, 14.28], [73.0397, 14.29], [73.0927, 14.29], [73.3052, 14.32], [73.3518, 14.32], 
    [73.3702, 14.33], [73.4169, 14.33], [73.5323, 14.35], [73.5941, 14.35], [73.6983, 14.37], [73.7752, 14.37], 
    [73.8081, 14.38], [73.8526, 14.38], [73.8734, 14.39], [74.1039, 14.41], [74.1103, 14.42], [74.1779, 14.42], 
    [74.1987, 14.43], [74.3418, 14.44], [74.3572, 14.45], [74.5098, 14.46], [74.5352, 14.47], [74.9719, 14.52], 
    [75.0777, 14.54], [75.3078, 14.56], [75.3196, 14.57], [75.3833, 14.57], [75.4193, 14.58], [75.7882, 14.63], 
    [75.906, 14.64], [75.9489, 14.65], [76.001, 14.65], [76.0137, 14.66], [76.2776, 14.69], [76.3135, 14.7], 
    [76.3765, 14.7], [76.4059, 14.71], [76.6656, 14.74], [76.688, 14.75], [76.7485, 14.75], [76.7591, 14.76], 
    [77.0198, 14.79], [77.1932, 14.82], [77.2617, 14.82], [77.2816, 14.83], [77.746, 14.89], [77.9723, 14.93], 
    [78.1229, 14.95], [78.1682, 14.95], [78.1856, 14.96], [78.4403, 14.99], [78.4634, 15], [78.7372, 15.04], 
    [78.7869, 15.04], [78.7955, 15.05], [78.9889, 15.07], [79.3872, 15.14], [79.4355, 15.14], [79.5183, 15.16], 
    [79.6351, 15.17], [79.8522, 15.21], [79.896, 15.21], [79.9137, 15.22], [80.0735, 15.24], [80.0883, 15.25], 
    [80.1513, 15.26], [80.2043, 15.26], [80.2763, 15.28], [80.4111, 15.3], [80.4514, 15.3], [80.4731, 15.31], 
    [80.7166, 15.35], [80.7612, 15.35], [80.7761, 15.36], [81.1938, 15.42], [81.3234, 15.45], [81.484, 15.47], 
    [81.5002, 15.48], [81.7908, 15.53], [81.8287, 15.53], [81.9506, 15.55], [82.4888, 15.65], [82.7535, 15.69], 
    [82.7743, 15.7], [82.8119, 15.7], [83.1718, 15.77], [83.3822, 15.8], [83.6818, 15.86], [83.9397, 15.9], 
    [83.9539, 15.91], [84.23, 15.96], [84.2587, 15.96], [84.2792, 15.97], [84.3806, 15.98], [85.0276, 16.1], 
    [85.2025, 16.14], [85.6174, 16.21], [86.5663, 16.39], [86.838, 16.45], [86.8875, 16.45], [89.0059, 16.86], 
    [91.2128, 17.26], [91.3217, 17.27], [92.4651, 17.46], [94.525, 17.77], [97.0001, 18.11], [97.8078, 18.23], 
    [98.3042, 18.29], [100.2364, 18.56]
]);

function xdc1RatingToDan(rating) {
    const value = Number(rating);
    if (!Number.isFinite(value) || value <= 0) return 0.0;
    if (value < XDC1_SCOPE_MIN_RATING) {
        // 图曲线专用：Alpha Low 以下按 (0,0)→(56.2281,10.5) 线性延伸，保持形状。
        return value * (10.5 / XDC1_SCOPE_MIN_RATING);
    }
    const knots = XDC1_RATING_DAN_KNOTS;
    if (value <= knots[0][0]) return knots[0][1];
    const last = knots.length - 1;
    if (value >= knots[last][0]) return knots[last][1];
    let lo = 0;
    let hi = last;
    while (hi - lo > 1) {
        const mid = (lo + hi) >> 1;
        if (knots[mid][0] <= value) lo = mid;
        else hi = mid;
    }
    const [x0, y0] = knots[lo];
    const [x1, y1] = knots[hi];
    return y0 + ((value - x0) / (x1 - x0)) * (y1 - y0);
}

const starFromDan = (dan) => 3.4 + 0.38 * dan;

// ── 插件入口 ────────────────────────────────────────────────────────────────

function buildNotes(parsedData, speedRate) {
    const rate = Number(speedRate) > 0 ? Number(speedRate) : 1.0;
    const notes = [];
    const count = parsedData.noteStarts.length;
    for (let i = 0; i < count; i += 1) {
        const column = Number(parsedData.columns[i]);
        let head = Number(parsedData.noteStarts[i]);
        let tail = (Number(parsedData.noteTypes[i]) & 128) !== 0
            ? Number(parsedData.noteEnds[i])
            : -1;
        if (rate !== 1.0) {
            head = Math.floor(head / rate);
            if (tail >= 0) tail = Math.floor(tail / rate);
        }
        if (head < 0) continue;
        if (tail >= 0 && tail < head) tail = -1;
        notes.push({ k: column, h: head, t: tail });
    }
    notes.sort((a, b) => (a.h !== b.h ? a.h - b.h : a.k - b.k));
    return notes;
}

function unsupportedResult(columnCount, lnRatio, reason) {
    return {
        star: Number.NaN,
        lnRatio,
        columnCount,
        graph: null,
        estDiff: `Unsupported: ${reason}`,
        numericDifficulty: null,
        numericDifficultyHint: null,
        rawNumericDifficulty: null,
        actualEstimatorAlgorithm: "XDC1",
        unsupported: true,
    };
}

/**
 * 运行 XDC1 估算器。
 *
 * @param {string} osuText .osu 文本
 * @param {object} options 与其他估算器同构（speedRate/withGraph/cvtFlag 等）
 * @param {object|null} parsed 可选的已 process() OsuFileParser 实例
 * @returns {object} `star` 为数值难度的显示星数（3.4 + 0.38 * numeric，与 Azusa/Roxy 同口径），
 *   `estDiff` 只给 RC 标签（LN 半段由 pipeline 用 Sunny 结果拼接）；不支持的谱面返回
 *   `unsupported: true` 与 `estDiff: "Unsupported: ..."`，不切换到 Sunny。
 */
export function runXdc1EstimatorFromText(osuText, options = {}, parsed = null) {
    const speedRate = options.speedRate ?? 1.0;
    const withGraph = options.withGraph === true;

    const sourceParser = parsed && typeof parsed?.getParsedData === "function"
        ? parsed
        : new OsuFileParser(osuText);
    if (!parsed || !parsed?.process) sourceParser.process();

    // Reuse the pipeline's parsed instance; IN/HO conversion must run on a clone
    // (modIN/modHO mutate the parser in place) so the shared chart stays pristine.
    let parser = sourceParser;
    const cvtText = String(options.cvtFlag ?? "").toUpperCase();
    if (cvtText.includes("IN") || cvtText.includes("HO")) {
        parser = parsed ? cloneOsuParser(sourceParser) : sourceParser;
        try {
            if (cvtText.includes("IN")) parser.modIN();
            if (cvtText.includes("HO")) parser.modHO();
        } catch {
            // keep the unconverted clone on conversion failure
        }
    }
    const parsedData = parser.getParsedData();
    const columnCount = Number(parsedData.columnCount) || 0;
    const lnRatio = Number(parsedData.lnRatio) || 0;

    if (columnCount !== 4) {
        return unsupportedResult(columnCount, lnRatio, "4K only");
    }
    if (parsedData.status === "Fail" || parsedData.status === "NotMania") {
        return unsupportedResult(columnCount, lnRatio, "parse failed");
    }

    const notes = buildNotes(parsedData, speedRate);
    if (notes.length < 2) {
        return unsupportedResult(columnCount, lnRatio, "too few notes");
    }

    const analysis = analyse(notes);
    const rating = Number(analysis.rating);
    if (!Number.isFinite(rating)) {
        return unsupportedResult(columnCount, lnRatio, "calculation failed");
    }
    if (rating < XDC1_SCOPE_MIN_RATING) {
        return unsupportedResult(columnCount, lnRatio, "below Alpha Low");
    }

    const calibratedDan = xdc1RatingToDan(rating);
    const numericDifficulty = Number(calibratedDan.toFixed(2));
    // XDC1 只负责 RC 半段；LN 半段由 pipeline 拼接 Sunny 的 LN 区间表结果。
    const rcLabel = calibratedDan < 10.52 ? "Alpha low" : numericToRcLabel(calibratedDan);
    const estDiff = rcLabel;
    const graph = withGraph && analysis.grid.length > 0
        ? {
            times: analysis.grid,
            // 与 Sunny/Daniel 共用同一套图表平滑（100ms 重采样 + 800ms Gaussian），
            // 避免 25ms 原始 strain 在图表上呈现为毛刺。
            values: smoothDForGraph(
                analysis.grid,
                analysis.graph,
                notes.map((note) => [note.k, note.h, note.t]),
            ).map((value) => starFromDan(xdc1RatingToDan(value))),
        }
        : null;

    return {
        star: Number(starFromDan(calibratedDan).toFixed(4)),
        lnRatio,
        columnCount,
        graph,
        estDiff,
        numericDifficulty,
        numericDifficultyHint: "xdc1-v1",
        rawNumericDifficulty: Number(rating.toFixed(4)),
        actualEstimatorAlgorithm: "XDC1",
    };
}
