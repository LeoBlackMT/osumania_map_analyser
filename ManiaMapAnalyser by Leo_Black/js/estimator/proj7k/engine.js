// js/estimator/proj7k/engine.js
// proj7k 7K difficulty engine (spec v0.2.3) — DOM-free port of src/proj7k/engine/ (Python reference).
//
// Pipeline: notes → finger mapping → per-event physical demand v → per-hand load d
//   → threshold solve (total D, max'd with the hardest 8 s window) → stars = A·D^B
//   → per-skill cost series → per-skill D_k, stars and level (ADR-0056/0059)
//   → memberships → coverage, shares; dominant skill by level.
// Upstream: https://github.com/ka1z07/proj7k. Parameters below mirror engine/params.py;
// numeric parity with the Python engine (upstream 915e5af) is checked on the 120-chart Jinjin 7K benchmark.

export const PARAMS = Object.freeze({
    eps_rc: 0.04,
    eps_ln: 0.05,
    eps_total_ln_slope: 0.01,
    beta: 6.0,
    N0: 150.0,
    theta_min: 0.01,
    theta_max: 1000.0,
    theta_tol: 1e-9,
    l_min: 0.1,
    row_tol: 0.001,
    ln_context_holds: 2.0,
    delta_h: 0.05,
    k_same: 1.75,
    v_lim: 30.0,
    v_pow: 4.0,
    v_gain: 1.5,
    jack_hz: 13.0,
    jack_pow: 8.0,
    jack_gain: 1.0,
    j_clip: 16.0,
    delta_floor: 0.02,
    delta_0: 0.04,
    delta_0_rel: 0.5,
    delta_0_window: 2.0,
    k_ring_mid: 1.0,
    k_mid_idx: 0.7,
    k_ring_idx: 0.5,
    k_thumb_idx: 0.5,
    k_thumb_other: 0.3,
    k_scale: 1.0,
    k_cross: 0.2,
    chi_0: 0.15,
    w_rel: 0.7,
    v_cap: Infinity,
    T_c: 4.0,
    U_max: 4.0,
    alpha_r: 1.0,
    b: 0.15,
    rhythm_horizon: 12.0,
    tau_h: 4.0,
    gamma: 2.0,
    burst_kappa: 0.6,
    burst_window: 8.0,
    burst_tau: 1.0,
    burst_N0: 60.0,
    // §8.3 skill costs (ADR-0056); only the defaults of the Python engine are ported
    speed_tau: 1.5,
    stamina_sustain: 20.0,
    stamina_rest: 0.6,
    umbrella_level: 1.05,
    stamina_chord_floor: 0.4,
    stamina_chord_span: 1.5,
    tech_floor: 0.25,
    tech_hand_irr: 0.7,
    tech_ratio_tol: 0.12,
    tech_max_gap: 1.0,
    tech_div_lo: 1.0,
    tech_div_span: 2.0,
    tech_div_w: 1.0,
    inverse_h_ref: 2.0,
    inverse_pow: 2.0,
    release_floor: 0.2,
    release_ell: 0.3,
    r_lo: 0.26,
    r_hi: 0.42,
    k_hi: 2.5,
    u_lo: 0.5,
    u_hi: 2.5,
    h_ref: 3.0,
});

// Star scale (engine/scale.py): stars = STAR_A · D ^ STAR_B
export const STAR_A = 0.35061692407081574;
export const STAR_B = 0.7563906685450623;

// Skill star scales (ADR-0056): stars_k = a_k · D_k ^ b_k
export const SKILL_SCALE = Object.freeze({
    rc_jack: [0.4966972323519079, 0.8033975307437226],
    rc_tech: [0.7582764177294805, 0.5949991636969195],
    rc_speed: [0.39654452985789646, 0.7933308739078072],
    rc_stamina: [0.7164813615327359, 0.6921153938852171],
    ln_general: [0.30861008527796946, 0.8089341834249038],
    ln_tech: [0.7537352649078002, 0.6855909798750277],
    ln_inverse: [0.5936706615058907, 0.6461171119947136],
    ln_release: [1.5026260041888677, 0.5664022723462133],
});

// Skill expectation lines (ADR-0059): ln(expected stars_k) = ln a_k + b_k · ln(total stars)
export const SKILL_EXPECT = Object.freeze({
    rc_jack: [-0.13263861254207368, 1.010469691127703],
    rc_tech: [0.31230412834018784, 0.7619881572052272],
    rc_speed: [-0.23491667482045256, 1.0534694661923347],
    rc_stamina: [-0.17196096898845814, 1.0250211109356844],
    ln_general: [-0.1632286412979236, 1.0647392277607566],
    ln_tech: [0.031199583234101574, 0.9715814009191315],
    ln_inverse: [-0.23790125524209876, 0.9983487819102551],
    ln_release: [0.28313587947298663, 0.729949832564748],
});

export const SKILLS = Object.freeze([
    "rc_jack", "rc_tech", "rc_speed", "rc_stamina",
    "ln_general", "ln_tech", "ln_inverse", "ln_release",
]);
const RC_COUNT = 4;

const FINGER = ["ring", "mid", "idx", "thumb", "idx", "mid", "ring"];
const SIDE = [0, 0, 0, null, 1, 1, 1];

export function starsOf(D) {
    return D > 0 ? STAR_A * Math.pow(D, STAR_B) : 0;
}

export function skillStarsOf(skill, D) {
    const [a, b] = SKILL_SCALE[skill];
    return D > 0 ? a * Math.pow(D, b) : 0;
}

/** A skill's stars over what a chart of this total usually requires of it (1 = usual). */
export function skillLevel(skill, stars, totalStars) {
    const [la, b] = SKILL_EXPECT[skill];
    const e = totalStars > 0 ? Math.exp(la + b * Math.log(totalStars)) : 0;
    return e > 0 ? stars / e : 0;
}

function handOf(thumb) {
    return SIDE.map((s) => (s === null ? thumb : s));
}

function pairCoupling(p, f, g) {
    const key = [FINGER[f], FINGER[g]].sort().join("+");
    switch (key) {
        case "mid+ring": return p.k_ring_mid;
        case "idx+mid": return p.k_mid_idx;
        case "idx+ring": return p.k_ring_idx;
        case "idx+thumb": return p.k_thumb_idx;
        case "mid+thumb":
        case "ring+thumb": return p.k_thumb_other;
        default: return 0;
    }
}

function kappaMatrix(p, thumb) {
    const hand = handOf(thumb);
    const k = [];
    for (let f = 0; f < 7; f += 1) {
        const row = new Float64Array(7);
        for (let g = 0; g < 7; g += 1) {
            if (f !== g && hand[f] === hand[g]) {
                row[g] = p.k_scale * pairCoupling(p, f, g);
            }
        }
        k.push(row);
    }
    return k;
}

function median(values) {
    const a = Float64Array.from(values).sort();
    const n = a.length;
    const m = n >> 1;
    return n % 2 ? a[m] : 0.5 * (a[m - 1] + a[m]);
}

function lowerBound(arr, x) {
    let lo = 0;
    let hi = arr.length;
    while (lo < hi) {
        const mid = (lo + hi) >> 1;
        if (arr[mid] < x) lo = mid + 1;
        else hi = mid;
    }
    return lo;
}

function smoothstep(z) {
    const c = z < 0 ? 0 : z > 1 ? 1 : z;
    return c * c * (3 - 2 * c);
}

// ── §2 preprocessing ──────────────────────────────────────────────────────────

/**
 * @param {Array<[number, number, number|null]>} notes  (column 0–6, head s, tail s | null)
 */
export function preprocess(notes, p = PARAMS) {
    const objs = notes.map(([c, t, e]) => [c, t, e !== null && e !== undefined && e - t >= p.l_min ? e : null]);
    objs.sort((a, b) => (a[1] - b[1]) || (a[0] - b[0]));

    const raw = [];
    objs.forEach(([c, t, e], k) => {
        raw.push([t, c, 0, k]);
        if (e !== null) raw.push([e, c, 1, k]);
    });
    raw.sort((a, b) => (a[0] - b[0]) || (a[2] - b[2]) || (a[1] - b[1]));

    const T = [];
    const n = raw.length;
    const row = new Int32Array(n);
    for (let i = 0; i < n; i += 1) {
        const t = raw[i][0];
        if (T.length === 0 || t - T[T.length - 1] > p.row_tol + 1e-12) T.push(t);
        row[i] = T.length - 1;
    }
    const nRows = T.length;
    const col = new Int32Array(n);
    const rel = new Uint8Array(n);
    const obj = new Int32Array(n);
    const isLn = new Uint8Array(n);
    for (let i = 0; i < n; i += 1) {
        col[i] = raw[i][1];
        rel[i] = raw[i][2];
        obj[i] = raw[i][3];
        isLn[i] = objs[obj[i]][2] !== null ? 1 : 0;
    }

    const headRow = new Int32Array(objs.length).fill(-1);
    const tailRow = new Int32Array(objs.length).fill(-1);
    for (let i = 0; i < n; i += 1) {
        (rel[i] ? tailRow : headRow)[obj[i]] = row[i];
    }

    // held[r][c]: an LN on c with head row < r < tail row
    const held = [];
    {
        const diff = [];
        for (let r = 0; r <= nRows; r += 1) diff.push(new Int32Array(7));
        objs.forEach(([c, , e], k) => {
            if (e !== null && tailRow[k] - headRow[k] > 1) {
                diff[headRow[k] + 1][c] += 1;
                diff[tailRow[k]][c] -= 1;
            }
        });
        const acc = new Int32Array(7);
        for (let r = 0; r < nRows; r += 1) {
            const hr = new Uint8Array(7);
            for (let c = 0; c < 7; c += 1) {
                acc[c] += diff[r][c];
                hr[c] = acc[c] > 0 ? 1 : 0;
            }
            held.push(hr);
        }
    }
    const h = new Float64Array(n);
    for (let i = 0; i < n; i += 1) {
        const hr = held[row[i]];
        let s = 0;
        for (let c = 0; c < 7; c += 1) s += hr[c];
        h[i] = s - hr[col[i]];
    }

    // §4.1 j
    const j = new Float64Array(n);
    const lastObj = new Array(7).fill(null);
    for (let i = 0; i < n; i += 1) {
        const k = obj[i];
        if (rel[i]) {
            j[i] = 1.0 / (T[tailRow[k]] - T[headRow[k]]);
            continue;
        }
        const c = col[i];
        const prev = lastObj[c];
        if (prev !== null) {
            const ePrime = objs[prev][2] !== null ? T[tailRow[prev]] : T[headRow[prev]] + p.delta_h;
            j[i] = 1.0 / Math.max(p.delta_floor, T[row[i]] - ePrime + p.delta_h);
        }
        lastObj[c] = k;
    }

    // last[r][c]: time of the latest event on column c at or before row r
    const last = [];
    {
        const has = [];
        for (let r = 0; r < nRows; r += 1) has.push(new Uint8Array(7));
        for (let i = 0; i < n; i += 1) has[row[i]][col[i]] = 1;
        const cur = new Float64Array(7).fill(-Infinity);
        for (let r = 0; r < nRows; r += 1) {
            for (let c = 0; c < 7; c += 1) if (has[r][c]) cur[c] = T[r];
            last.push(Float64Array.from(cur));
        }
    }

    return {
        n, nRows, T, row, col, rel, obj, tailRow, isLn, held, h, j, last,
        delta0: localDelta0(T, p),
        U: rhythmSurprise(T, p),
    };
}

function localDelta0(T, p) {
    const n = T.length;
    const iv = new Float64Array(n);
    for (let r = 0; r < n; r += 1) iv[r] = r === 0 ? 1.0 : T[r] - T[r - 1];
    const out = new Float64Array(n);
    for (let r = 0; r < n; r += 1) {
        if (r === 0) {
            out[r] = p.delta_0;
            continue;
        }
        const lo = Math.max(lowerBound(T, T[r] - p.delta_0_window), 1);
        out[r] = p.delta_0_rel * median(iv.subarray(lo, r + 1));
    }
    return out;
}

function rhythmSurprise(T, p) {
    const n = T.length;
    const U = new Float64Array(n);
    if (n < 2) return U;
    const ell = new Float64Array(n);
    for (let r = 1; r < n; r += 1) ell[r] = Math.log2(T[r] - T[r - 1]);
    const floor = p.alpha_r * Math.pow(2.0, -p.U_max);
    const twoB2 = 2 * p.b * p.b;
    let lo = 1;
    for (let r = 1; r < n; r += 1) {
        while (lo < r && T[r] - T[lo] > p.rhythm_horizon * p.T_c) lo += 1;
        let rho;
        if (lo < r) {
            let num = 0;
            let ws = 0;
            for (let q = lo; q < r; q += 1) {
                const w = Math.exp(-(T[r] - T[q]) / p.T_c);
                const dl = ell[r] - ell[q];
                num += w * Math.exp(-(dl * dl) / twoB2);
                ws += w;
            }
            rho = (num + floor) / (ws + p.alpha_r);
        } else {
            rho = floor / p.alpha_r;
        }
        U[r] = Math.min(p.U_max, -Math.log2(rho));
    }
    return U;
}

// ── §4, §6–7 demand and hand accumulator ─────────────────────────────────────

function psi(dt, d0) {
    return Number.isFinite(dt) ? dt / (dt * dt + d0 * d0) : 0;
}

function rise(x, lim, power, gain) {
    const z = Math.pow(x / lim, power);
    return 1.0 + gain * (Number.isFinite(z) ? z / (1.0 + z) : 1.0);
}

function accumulate(ev, hand, v, p, tau) {
    const vg = v.map((x) => Math.pow(x, p.gamma));
    const Eh = [0, 0];
    const th = [NaN, NaN];
    const out = new Float64Array(ev.n);
    let i0 = 0;
    // events are sorted by time, so each row is a contiguous run of events
    while (i0 < ev.n) {
        const r = ev.row[i0];
        let i1 = i0;
        while (i1 < ev.n && ev.row[i1] === r) i1 += 1;
        const Tr = ev.T[r];
        const handIn = [0, 0];
        const hit = [false, false];
        for (let i = i0; i < i1; i += 1) {
            const hh = hand[ev.col[i]];
            handIn[hh] += vg[i];
            hit[hh] = true;
        }
        for (let hh = 0; hh < 2; hh += 1) {
            if (hit[hh]) {
                const phi = Number.isNaN(th[hh]) ? 0.0 : Math.exp(-(Tr - th[hh]) / tau);
                Eh[hh] = Eh[hh] * phi + (1 - phi) * handIn[hh];
                th[hh] = Tr;
            }
        }
        for (let i = i0; i < i1; i += 1) out[i] = Eh[hand[ev.col[i]]];
        i0 = i1;
    }
    return out;
}

function handDemand(ev, thumb, p) {
    const hand = handOf(thumb);
    const K = kappaMatrix(p, thumb);
    const { n, nRows, T, row, col, rel, j, held, delta0 } = ev;

    // only events strictly before the row count
    const lastPrev = [new Float64Array(7).fill(-Infinity)];
    for (let r = 1; r < nRows; r += 1) lastPrev.push(ev.last[r - 1]);
    const Psi = [];
    const lastHand = [];
    for (let r = 0; r < nRows; r += 1) {
        const ps = new Float64Array(7);
        const lh = [-Infinity, -Infinity];
        for (let c = 0; c < 7; c += 1) {
            ps[c] = psi(T[r] - lastPrev[r][c], delta0[r]);
            if (lastPrev[r][c] > lh[hand[c]]) lh[hand[c]] = lastPrev[r][c];
        }
        Psi.push(ps);
        lastHand.push(lh);
    }

    const v = new Float64Array(n);
    const vSame = new Float64Array(n);
    const vRate = new Float64Array(n);
    const rr = new Float64Array(n);
    for (let i = 0; i < n; i += 1) {
        const ri = row[i];
        const Kc = K[col[i]];
        let x = 0;
        let hc = 0;
        for (let c = 0; c < 7; c += 1) {
            x += Kc[c] * Psi[ri][c];
            hc += Kc[c] * held[ri][c];
        }
        const o = p.k_cross * psi(T[ri] - lastHand[ri][1 - hand[col[i]]], delta0[ri]);
        const cc = 1.0 + p.chi_0 * hc;
        const isRel = rel[i] === 1;
        const omega = isRel ? p.w_rel : 1.0;
        const kj = isRel ? 1.0 : p.k_same;
        const jj = isRel ? j[i] : Math.min(j[i], p.j_clip);
        const v0 = omega * (kj * jj + x + o) * cc;
        const rs = rise(v0, p.v_lim, p.v_pow, p.v_gain);
        const mini = isRel ? 0.0 : p.k_same * jj * (rise(jj, p.jack_hz, p.jack_pow, p.jack_gain) - 1.0) * cc;
        v[i] = Math.min(v0 * rs, p.v_cap) + mini;
        // ADR-0056: v split into the finger's own repeat (rc_jack) and the rest of the hand's motion (rc_speed)
        vSame[i] = omega * kj * jj * cc * rs + mini;
        vRate[i] = omega * (x + o) * cc * rs;
        const jxo = j[i] + x + o;
        rr[i] = jxo > 0 ? j[i] / jxo : 0;
    }

    const d = accumulate(ev, hand, v, p, p.tau_h).map((e) => Math.pow(e, 1.0 / p.gamma));
    const dBurst = p.burst_kappa > 0
        ? accumulate(ev, hand, v, p, p.burst_tau).map((e) => Math.pow(e, 1.0 / p.gamma))
        : d;
    return { thumb, hand, v, d, dBurst, r: rr, vSame, vRate };
}

// ── §3 threshold solve ───────────────────────────────────────────────────────

function lossAt(lnd, lnTheta, beta) {
    // expit(beta · (ln d − ln θ)); d = 0 (ln d = −∞) gives 0
    return 1.0 / (1.0 + Math.exp(-beta * (lnd - lnTheta)));
}

/**
 * Σ w_i p_i(θ) = eps (W + N0), bisection on ln θ; returns 0 when nothing can be lost.
 * @param {Float64Array} lnd  ln of the readings (−Infinity where d = 0)
 * @param {Float64Array|null} w  weights (null = all ones)
 */
function solve(lnd, w, eps, N0, p, from = 0, to = lnd.length) {
    let W = 0;
    let Wpos = 0;
    for (let i = from; i < to; i += 1) {
        const wi = w ? w[i] : 1;
        W += wi;
        if (lnd[i] > -Infinity) Wpos += wi;
    }
    const target = eps * (W + N0);
    if (Wpos <= target) return 0.0;
    let lo = Math.log(p.theta_min);
    let hi = Math.log(p.theta_max);
    const g = (lt) => {
        let s = 0;
        for (let i = from; i < to; i += 1) {
            const wi = w ? w[i] : 1;
            if (wi !== 0) s += wi * lossAt(lnd[i], lt, p.beta);
        }
        return s - target;
    };
    if (g(lo) <= 0) return p.theta_min;
    if (g(hi) >= 0) return p.theta_max;
    while (hi - lo > p.theta_tol) {
        const mid = 0.5 * (lo + hi);
        if (g(mid) > 0) lo = mid;
        else hi = mid;
    }
    return Math.exp(0.5 * (lo + hi));
}

function burst(ev, dem, D, eps, p) {
    if (p.burst_kappa <= 0 || D <= 0) return 0.0;
    const lnd = dem.dBurst.map((x) => Math.log(x));
    const tEv = new Float64Array(ev.n);
    for (let i = 0; i < ev.n; i += 1) tEv[i] = ev.T[ev.row[i]];
    const T0 = ev.T[0];
    const step = p.burst_window / 4;
    const count = Math.max(0, Math.ceil((ev.T[ev.nRows - 1] + 1e-9 - T0) / step));
    let best = 0.0;
    for (let s = 0; s < count; s += 1) {
        const t0 = T0 + s * step;
        const from = lowerBound(tEv, t0);
        const to = lowerBound(tEv, t0 + p.burst_window);
        if (to > from) best = Math.max(best, solve(lnd, null, eps, p.burst_N0, p, from, to));
    }
    return p.burst_kappa * best;
}

// ── §8–9 memberships, attribution, dominance ─────────────────────────────────

function lnContext(ev, p) {
    const out = new Float64Array(ev.n);
    for (let i = 0; i < ev.n; i += 1) {
        out[i] = Math.max(ev.isLn[i], Math.min(1.0, ev.h[i] / p.ln_context_holds));
    }
    return out;
}

function memberships(ev, dem, p) {
    const { n, nRows, row, col, rel } = ev;
    const lam = lnContext(ev, p);
    const hand = dem.hand;
    const size = new Float64Array(2 * nRows);
    for (let i = 0; i < n; i += 1) size[row[i] * 2 + hand[col[i]]] += 1;
    const pressRow = new Uint8Array(nRows);
    for (let i = 0; i < n; i += 1) if (!rel[i]) pressRow[row[i]] = 1;

    const w = [];
    const a = [];
    for (let i = 0; i < n; i += 1) {
        const qJack = smoothstep((dem.r[i] - p.r_lo) / (p.r_hi - p.r_lo));
        const qChord = smoothstep((size[row[i] * 2 + hand[col[i]]] - 1.0) / (p.k_hi - 1.0));
        const qRhythm = smoothstep((ev.U[row[i]] - p.u_lo) / (p.u_hi - p.u_lo));
        const qInverse = rel[i] ? 0 : Math.min(1.0, ev.h[i] / p.h_ref);
        const tr = ev.tailRow[ev.obj[i]];
        const qRelease = ev.isLn[i] && tr >= 0 && !pressRow[tr] ? 1 : 0;
        const unit = [
            qJack, qRhythm, (1 - qJack) * (1 - qChord), (1 - qJack) * qChord,
            1 - Math.max(qRhythm, qInverse, qRelease), qRhythm, qInverse, qRelease,
        ];
        const rcSum = unit[0] + unit[1] + unit[2] + unit[3];
        const lnSum = unit[4] + unit[5] + unit[6] + unit[7];
        const wi = new Float64Array(8);
        const ai = new Float64Array(8);
        for (let k = 0; k < 8; k += 1) {
            const fam = k < RC_COUNT ? 1 - lam[i] : lam[i];
            wi[k] = unit[k] * fam;
            ai[k] = wi[k] / (k < RC_COUNT ? rcSum : lnSum);
        }
        w.push(wi);
        a.push(ai);
    }
    return { w, a };
}

function dominance(a, lnd, D, p) {
    const n = lnd.length;
    const s = new Float64Array(n);
    let total = 0;
    const lt = D > 0 ? Math.log(D) : -Infinity;
    for (let i = 0; i < n; i += 1) {
        const pl = D > 0 ? lossAt(lnd[i], lt, p.beta) : (lnd[i] > -Infinity ? 1 : 0);
        s[i] = pl * (1 - pl);
        total += s[i];
    }
    const pi = new Float64Array(8);
    for (let i = 0; i < n; i += 1) {
        const si = total > 0 ? s[i] / total : 1 / n;
        for (let k = 0; k < 8; k += 1) pi[k] += a[i][k] * si;
    }
    return pi;
}

// ── §8.3 skill costs (ADR-0056) ──────────────────────────────────────────────

const CARRIED_RATIOS = [1.0, 2.0, 0.5, 3.0, 1.0 / 3.0, 4.0, 0.25, 1.5, 2.0 / 3.0];
const GAP_CLASSES = [1.0, 2.0, 0.5, 1.5, 1.0 / 3.0, 2.0 / 3.0, 0.75, 4.0 / 3.0, 4.0, 0.25];
const GAP_CLASS_SHARE = 0.1;
const DIVERSITY_WINDOW_S = 1.0;

function upperBound(arr, x) {
    let lo = 0;
    let hi = arr.length;
    while (lo < hi) {
        const mid = (lo + hi) >> 1;
        if (arr[mid] <= x) lo = mid + 1;
        else hi = mid;
    }
    return lo;
}

function pressRows(ev, keep = () => true) {
    const rows = [];
    for (let i = 0; i < ev.n; i += 1) {
        // events are in row order, so the press rows come out sorted and unique
        if (!ev.rel[i] && keep(i) && (rows.length === 0 || ev.row[i] > rows[rows.length - 1])) rows.push(ev.row[i]);
    }
    return rows;
}

function nearClass(ratio, classes, tol) {
    for (let c = 0; c < classes.length; c += 1) {
        if (Math.abs(ratio - classes[c]) <= tol * classes[c]) return c;
    }
    return -1;
}

// per event: gap classes holding ≥ 10% of the press-row gaps within ±1 s of its row
function snapDiversity(ev, p) {
    const out = new Float64Array(ev.n);
    const rows = pressRows(ev);
    if (rows.length < 3) return out;
    const t = rows.map((r) => ev.T[r]);
    const gaps = [];
    for (let k = 1; k < t.length; k += 1) gaps.push(t[k] - t[k - 1]);
    const div = new Float64Array(ev.nRows);
    for (let k = 0; k < rows.length; k += 1) {
        const lo = lowerBound(t, t[k] - DIVERSITY_WINDOW_S);
        const hi = upperBound(t, t[k] + DIVERSITY_WINDOW_S);
        const g = gaps.slice(Math.max(lo, 0), Math.max(hi - 1, 0)).filter((x) => x > 0);
        if (g.length < 4) continue;
        const med = median(g);
        const counts = new Map();
        for (const x of g) {
            const cls = nearClass(x / med, GAP_CLASSES, p.tech_ratio_tol);
            counts.set(cls, (counts.get(cls) || 0) + 1);
        }
        let d = 0;
        for (const c of counts.values()) if (c / g.length >= GAP_CLASS_SHARE) d += 1;
        div[rows[k]] = d;
    }
    for (let i = 0; i < ev.n; i += 1) out[i] = div[ev.row[i]];
    return out;
}

// 1 on a press whose hand's interval is not a carried ratio of the one before it
function handIrregular(ev, hand, p) {
    const out = new Float64Array(ev.n);
    for (let hh = 0; hh < 2; hh += 1) {
        const rows = pressRows(ev, (i) => hand[ev.col[i]] === hh);
        if (rows.length < 3) continue;
        const badOfRow = new Uint8Array(ev.nRows);
        for (let k = 2; k < rows.length; k += 1) {
            const g0 = ev.T[rows[k - 1]] - ev.T[rows[k - 2]];
            const g1 = ev.T[rows[k]] - ev.T[rows[k - 1]];
            const ok = g0 > 0 && g1 > 0 && g1 <= p.tech_max_gap;
            if (ok && nearClass(g1 / g0, CARRIED_RATIOS, p.tech_ratio_tol) < 0) badOfRow[rows[k]] = 1;
        }
        for (let i = 0; i < ev.n; i += 1) {
            if (!ev.rel[i] && hand[ev.col[i]] === hh) out[i] = badOfRow[ev.row[i]];
        }
    }
    return out;
}

// seconds each event's hand has been playing since its last rest
function runSinceRest(ev, hand, p) {
    const out = new Float64Array(ev.n);
    const start = [NaN, NaN];
    const last = [NaN, NaN];
    for (let i = 0; i < ev.n; i += 1) {
        const hh = hand[ev.col[i]];
        const t = ev.T[ev.row[i]];
        if (Number.isNaN(last[hh]) || t - last[hh] > p.stamina_rest) start[hh] = t;
        last[hh] = t;
        out[i] = t - start[hh];
    }
    return out;
}

function skillCosts(ev, dem, p) {
    const { n, nRows, row, rel } = ev;
    const lam = lnContext(ev, p);
    const qHand = handIrregular(ev, dem.hand, p);
    const div = snapDiversity(ev, p);
    const run = runSinceRest(ev, dem.hand, p);
    const pressCount = new Float64Array(nRows);
    for (let i = 0; i < n; i += 1) if (!rel[i]) pressCount[row[i]] += 1;
    const headRow = new Int32Array(ev.tailRow.length).fill(-1);
    for (let i = 0; i < n; i += 1) if (!rel[i]) headRow[ev.obj[i]] = row[i];

    const costs = {};
    for (const k of SKILLS) costs[k] = new Float64Array(n);
    for (let i = 0; i < n; i += 1) {
        const rc = 1.0 - lam[i];
        const v = dem.v[i];
        const qU = smoothstep((ev.U[row[i]] - p.u_lo) / (p.u_hi - p.u_lo));
        const qH = p.tech_hand_irr * qHand[i];
        const qDiv = Math.min(1, Math.max(0, (div[i] - p.tech_div_lo) / p.tech_div_span));
        const irregular = p.tech_floor + (1.0 - p.tech_floor) * Math.max(qU, qH, p.tech_div_w * qDiv);
        const irregularLn = p.tech_floor + (1.0 - p.tech_floor) * Math.max(qU, qH);
        const sustain = Math.min(1.0, run[i] / p.stamina_sustain);
        const chord = p.stamina_chord_floor
            + (1.0 - p.stamina_chord_floor) * Math.min(1.0, (pressCount[row[i]] - 1.0) / p.stamina_chord_span);
        const qInv = rel[i] ? 0 : Math.pow(Math.min(1.0, ev.h[i] / p.inverse_h_ref), p.inverse_pow);
        const tr = ev.tailRow[ev.obj[i]];
        const hasTail = ev.isLn[i] && tr >= 0;
        const isolated = hasTail && !pressCount[tr] ? 1 : 0;
        let qRel = 0;
        if (isolated) {
            const hr = headRow[ev.obj[i]];
            const ell = ev.T[tr] - ev.T[hr >= 0 ? hr : 0];
            const precise = rel[i] ? Math.min(1.0, p.release_ell / Math.max(ell, 1e-3)) : 0;
            qRel = p.release_floor + (1.0 - p.release_floor) * precise;
        }
        costs.rc_jack[i] = dem.vSame[i] * rc;
        costs.rc_tech[i] = v * irregular * rc;
        costs.rc_speed[i] = dem.vRate[i] * rc;
        costs.rc_stamina[i] = dem.vRate[i] * sustain * chord * rc;
        costs.ln_general[i] = v * lam[i];
        costs.ln_tech[i] = v * irregularLn * lam[i];
        costs.ln_inverse[i] = v * qInv * lam[i];
        costs.ln_release[i] = v * qRel * lam[i];
    }
    return costs;
}

function skillDemands(ev, dem, p) {
    const costs = skillCosts(ev, dem, p);
    const out = {};
    SKILLS.forEach((k, idx) => {
        const tau = k === "rc_speed" ? p.speed_tau : p.tau_h;
        const lnd = accumulate(ev, dem.hand, costs[k], p, tau).map((e) => Math.log(Math.pow(e, 1.0 / p.gamma)));
        out[k] = solve(lnd, null, idx < RC_COUNT ? p.eps_rc : p.eps_ln, p.N0, p);
    });
    return out;
}

// ── public entry ─────────────────────────────────────────────────────────────

/**
 * Difficulty profile of a 7K chart.
 * @param {Array<[number, number, number|null]>} notes  (column 0–6, head s, tail s | null)
 * @returns {{thumbHand: string, totalD: number, totalStars: number,
 *   skills: Object<string, {D: number, stars: number, coverage: number, dominance: number}>,
 *   dominantSkill: string, dominanceRank: string[], dominanceMargin: number, nEvents: number}}
 */
export function evaluateNotes(notes, p = PARAMS) {
    if (!notes || notes.length === 0) throw new Error("empty chart");
    const ev = preprocess(notes, p);
    const lam = lnContext(ev, p);
    let lamMean = 0;
    for (let i = 0; i < ev.n; i += 1) lamMean += lam[i];
    lamMean /= ev.n;
    const epsTotal = p.eps_rc + p.eps_total_ln_slope * lamMean;

    let best = null;
    for (const thumb of [0, 1]) {
        const dem = handDemand(ev, thumb, p);
        const lnd = dem.d.map((x) => Math.log(x));
        const D = solve(lnd, null, epsTotal, p.N0, p);
        if (best === null || D < best.D) best = { D, dem, lnd };
    }
    const { dem, lnd } = best;
    const D = Math.max(best.D, burst(ev, dem, best.D, epsTotal, p));

    const totalStars = starsOf(D);

    // ADR-0056: each skill's own cost → D_k on its own star scale; memberships still give coverage and shares
    const demands = skillDemands(ev, dem, p);
    const { w, a } = memberships(ev, dem, p);
    const pi = dominance(a, lnd, D, p);
    // ADR-0059: rank by level, the chart's family first (LN chart when mean LN context ≥ 1/2);
    // ln_general is the LN umbrella and ranks at umbrella_level
    const lnChart = lamMean >= 0.5;
    const skills = {};
    const key = new Float64Array(SKILLS.length);
    SKILLS.forEach((k, idx) => {
        let cov = 0;
        for (let i = 0; i < ev.n; i += 1) cov += w[i][idx];
        const stars = skillStarsOf(k, demands[k]);
        const level = skillLevel(k, stars, totalStars);
        skills[k] = { D: demands[k], stars, level, coverage: cov / ev.n, dominance: pi[idx] };
        const kk = k === "ln_general" ? p.umbrella_level : level;
        key[idx] = (idx < RC_COUNT) !== lnChart ? kk : kk - 1e3;
    });
    const order = SKILLS.map((k, idx) => idx).sort((x, y) => (key[y] - key[x]) || (x - y));
    return {
        thumbHand: dem.thumb === 0 ? "L" : "R",
        totalD: D,
        totalStars,
        skills,
        dominantSkill: SKILLS[order[0]],
        dominanceRank: order.map((idx) => SKILLS[idx]),
        dominanceMargin: key[order[0]] - key[order[1]],
        nEvents: ev.n,
    };
}
