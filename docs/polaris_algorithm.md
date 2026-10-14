# Polaris Estimator Documentation

> Target audience: humans / AI. Polaris is a first-party estimator of this project (like Azusa / Roxy). This document describes its scope, correction model, output semantics, validation and retune procedure.
> Shared-module constraints apply (see docs/guides/module-conventions.md): the estimator is DOM-free, pure and runs identically under Node (benchmark runner) and the browser.

---

## 1. Overview

Polaris is a **table-calibration estimator**. It takes an osu!mania `.osu` beatmap as input, uses **Sunny Rework** as its baseline, and outputs difficulty labels whose `star → dan` mapping is corrected with behaviourally-fitted, smooth adjustment curves. Each table gets calibrated on the half (or halves) that actually needs it:

- **7K**: both the RC and LN sides. 7K's `Mixed` is always the Sunny baseline, so both halves are table-mapping products.
- **4K**: the **LN half only**. A 4K chart's RC half is decided by `Mixed`'s routing chain (Roxy/Azusa/Daniel/Companella), not by the table, so calibrating it would have no evidence behind it — while the LN half is always Sunny's LN table. LN-side 4K charts reach `Mixed`'s L2 path (pure Companella) 98.7 % of the time, because Azusa's own `UnsupportedLN` gate (`lnRatio > 0.18`) squeezes the L1 fusion branch into a 0.03-wide window (`lnRatio ∈ [0.15, 0.18]`). The displayed LN label was therefore never touched by any correction before Polaris.

```
Parse → Sunny baseline (star/lnRatio) → scope gate → star → level (per side, from interval tables)
      → Polaris corrections (B + R·f(rate) + H) → labels / numericDifficulty
```

| Field | Type | Description |
|-------|------|-------------|
| `estDiff` | string | Corrected RC/LN tier labels, e.g. `Regular 6 mid/high \|\| LN 6 mid/low`. On 4K the RC half is carried through from Sunny unchanged |
| `numericDifficulty` | number \| null | 7K: corrected continuous RC-side level (Reform scale), `null` when the star is outside the RC table range. 4K: Sunny's value unchanged (the RC half is not calibrated) |
| `star` | number | Sunny raw sr (unchanged; the star capsule always shows the Sunny sr) |
| `actualEstimatorAlgorithm` | string | `"Polaris"` when applied; `"Sunny"` when the estimator fell back |
| `debug` | object | `{ rate, keyCount, rcBase, rcLevel, lnBase, lnLevel }` intermediate values (`rcBase`/`rcLevel` are `null` on 4K) |

Rationale: the `js/estimator/intervals/*` tables are a direct `Sunny star → dan` mapping, which carries systematic deviations. A behavioural audit against ~4.8M real plays (Mania Tracker dataset, 2026-10 snapshot) quantified them per level, per rate and per LN ratio; Polaris applies the fitted corrections instead of rewriting the tables.

## 2. Scope & fallback

| Condition | Behaviour |
|---|---|
| `columnCount === 7` | Polaris applies to **both** the RC and LN sides |
| `columnCount === 4` | Polaris applies to the **LN side only**; the RC half is carried through from Sunny and `numericDifficulty` stays Sunny's |
| Other key counts (6K/10K/…) | Fall back to Sunny (`actualEstimatorAlgorithm = "Sunny"`) |
| `cvtFlag` contains `IN` / `HO` (chart-converting mods) | Fall back to Sunny — no fitted evidence for converted charts |
| non-finite `star` / `lnRatio` | Fall back to Sunny |

Scope note: Polaris calibrates the label surface the user reads. Two consequences worth stating:

- **The 4K LN half is calibrated in `Mixed` as well**, not only when Polaris is selected: `mixedEstimator` calls the same `calibrateLnLevel` helper, so a user on the default `Mixed` algorithm gets the calibrated 4K LN label too, and the two paths can never disagree. This is deliberate — on 4K, Polaris is not a substitute for `Mixed` (Polaris's RC half is Sunny's table value, while `Mixed`'s RC half comes from the routing chain, which is better), so the LN calibration had to reach the `Mixed` path to be useful at all.
- **On 7K, `Mixed` routes to Polaris outright.** `Mixed` exists to pick the best available algorithm, and on 7K the 4K-only candidates (Azusa/Roxy/Daniel/Companella) are all unavailable, so Polaris is the best remaining option — it is Sunny's table plus a fitted correction on both halves. `Mixed`'s 7K RC and LN halves are therefore the calibrated ones, and its capsule reads `Polaris`. Other non-4K key counts (6K/10K) have no curve set and stay on Sunny.

The result object is the full Sunny result (`star`, `lnRatio`, `columnCount`, `graph`, …) with `estDiff` / `numericDifficulty` replaced. **`star` is Sunny's raw sr by construction, so Polaris is not part of `NORMALIZATION_ALGORITHMS`** in `runAnalysisPipeline`.

## 3. What is corrected (measured deviations)

Measured on the Mania Tracker behavioural dataset (in-table charts with ≥8 players, n = 6899 chart×rate; `delta = play-implied dan − table mapping`):

1. **DT rate response (largest)**: at 1.5x, RC charts sit ~+0.7 to +1.4 dan above their table mapping across levels 5–11 (paired same-chart difference +0.95; LN +0.75). The table ladder rises less than actual difficulty when the chart is sped up.
2. **LN side**: full-LN charts (`lnRatio ≥ 0.97`) run ~+1.0 dan above their LN mapping; mid-`lnRatio` LN charts ~+0.2; the level shape adds a low-mid elevation.
3. **Level shape at 1.0x**: small (±0.3 dan), e.g. a local elevation around level 3 and a slight negative tail at the bottom edge. HT (rate < 1.0) showed conflicting small effects across measurement designs and is **not corrected**.
4. **4K LN half**: measured on the same dataset's 4K LN side (n = 2657). The LN label under-rates by **+0.20** on average, and the deviation is structured rather than flat — it grows with rate (1.0x +0.12 vs 1.5x +0.48), grows with hold density (`lnRatio ≥ 0.95`: +0.48), and rises at the top of the ladder ((13,99] +0.51). This is a table-mapping error, and it was previously uncorrected: `lnCorrection` adjusts `numericDifficulty` (the RC half), never the LN label.

## 4. Correction model

Implemented in `js/estimator/polarisCorrection.js`. Three curve sets share one model:

```
corr(side, level, rate, lnRatio) = B_side(level) + R_side(level) · f(rate) + H(lnRatio)

f(rate) = clamp((rate − 1) / 0.5, 0, 1)     # rate ≤ 1.0 → 0 (no correction); rate ≥ 1.5 → 1 (capped)
```

| Curve set | Selected by | Covers |
|---|---|---|
| `7K:RC` | `keyCount === 7`, `side === "RC"` | RC half on 7K |
| `7K:LN` | `keyCount === 7`, `side === "LN"` | LN half on 7K |
| `4K:LN` | `keyCount === 4`, `side === "LN"` | LN half on 4K |

There is no `4K:RC` set: `computePolarisCorrection` returns 0 for that combination.

- **`B_side(level)`** — 1.0x level baseline. 29 knots at 0.5 level spacing, linear interpolation, clamped outside.
- **`R_side(level)`** — DT increment at 1.5x for the same level (0 for levels < ≈3, peaking ≈1.1–1.2 around level 8–9, decaying to ≈0.2–0.4 at ≥12). Negative values clamped to 0.
- **`H(lnRatio)`** — LN-side lnRatio term (6 knots; ≈0 below 0.5, ≈+0.25 in 0.55–0.85, up to **+1.0 at lnRatio ≥ 0.97**).

Fitting (7K, n = 6899): additive (backfitting) joint fit on the Mania Tracker snapshot; per-table knots smoothed with a 3-point kernel. Validation: group split by `beatmap_id` (70/30) — held-out `mean|delta|` improves **6.8 %** (0.680 → 0.634), full-fit bias +0.170 → −0.002; on the flagged subset (≥1 dan, |z| ≥ 3) the mean absolute miss improves **17 %** (1.64 → 1.36). The full-fit → CV gap is ≈0, i.e. the curve family is low-capacity and not overfit.

Fitting (4K LN, n = 2657 chart×rate; level range 5.00–16.60): same model and knot grid, fitted on the 4K LN side of the same snapshot. Unlike the 7K fit this one applies the **R ≥ 0 constraint inside the fit**, so `∂level/∂rate ≥ 0` is a property of the fitted coefficients rather than something the runtime guard has to clamp. Validation: group CV by `beatmap_id % 5` — MAE **0.7285 → 0.6854 (−5.91 %)**, signed bias **+0.191 → −0.013**, 57.2 % of rows improved. A constant-offset control on the same population only reaches −3.21 %, so the level/rate/lnRatio shape carries real signal. The raw bias by stratum: level (3,5] −0.45, (7,9] +0.29, (13,99] +0.51; rate 1.0x +0.12 vs 1.5x +0.48; `lnRatio ≥ 0.95` +0.48.

### Monotonicity invariant

Because Polaris emits `level + correction`, both monotonicity properties are properties of the **composite curve**, not of the correction alone:

```
g(l) = l + B(l) + R(l)·f        f = clamp((rate − 1) / 0.5, 0, 1) ∈ [0, 1]

1) ∂level/∂star > 0   ⇔  g strictly increasing in l
2) ∂level/∂rate ≥ 0   ⇔  R(l) ≥ 0   (g is monotone in f, f is monotone in rate)
```

Both are guaranteed **by construction** at module load rather than checked after the fact:

- each curve is linear interpolation between knots, so piecewise-linear — adjacent-knot increments `Δg > 0` are sufficient for the whole curve;
- `Δg` is **linear in `f`**, so the endpoints `f = 0` and `f = 1` decide the entire range (no grid search needed);
- therefore raising `B` at the worst endpoint and clamping negative `R` to zero fixes any inversion in a single pass, with no iteration.

`POLARIS_MIN_LEVEL_STEP` (0.05 per 0.5-level knot) is the enforced floor — a real margin rather than a bare `> 0`, so float noise or an aggressive refit cannot land within rounding distance of an inversion.

Outside the knot range, interpolation is clamped to a constant, so `g` has slope exactly 1 there — **extrapolation at the extreme boundary is monotone by construction**, which is the case the guard exists for (at Stellium/Wild levels the sample is thin and the fitted spline is free to wobble).

Runtime repair changes numbers only when the **fitted coefficients are themselves inverted**. Whether that happened is exported separately as `POLARIS_MONOTONICITY.rawViolations`; a non-empty list means the spline is inverted somewhere and the fit script should be revisited — the runtime guard is a safety net, not a licence to ship an inverted curve. For the current three curve sets the list is empty, so the invariant holds with room to spare (the 7K LN set's tightest raw step is +0.269 at level 6.25→6.75; the 4K LN set was fitted under an explicit `R ≥ 0` constraint, so it cannot violate the rate half at all). The effective curves can be re-checked at any time with `verifyPolarisMonotonicity()` (always expected to return `[]`).

### Table mapping helpers (inside the estimator)

- `star → level`: piecewise-linear with the exact structure used by the fitting scripts (bands contribute `(lo, n−0.1)` and `(hi, n+0.1)`; level `n` is derived from the label: numeric token with `Intro` shift, non-numeric base = previous level + 1). Linear extrapolation outside the table using the first/last 5-knot slope.
- `level → label`: nearest band centre; stars outside the table keep the `<` / `>` bound labels, and `numericDifficulty` is `null` there.
- **Extended range** (`extendedEstimationRange`): the RC side uses the extended table (`7k-rc-ext.js`) like Sunny does. Note: the `Wild N` bases of the extended table have no numeric semantics elsewhere in the project; Polaris continues the sequence (`Stellium` = 14 → `Wild 8` = 15 → `Wild 9` = 16) so that the mapping stays monotone. Corrections are fitted on the default range and extend flat beyond it. LN has no extended table.

## 5. Output semantics

- `estDiff` = corrected `RC label || LN label`; the LN half is omitted when `lnRatio < 0.15` and `enableAlwaysShowLNDifficulty` is off (same rule as Sunny's `estDiff`). On 4K the RC half is Sunny's RC half, carried through verbatim.
- `numericDifficulty` is the corrected **RC-side** level rounded to 2 dp on 7K (the LN side is a label-only surface, consistent with other estimators' numeric semantics), and **Sunny's value unchanged on 4K** — the 4K RC half is not calibrated, so there is nothing to re-derive. Telemetry treats Polaris as a native-numeric algorithm (`rcNative` in analysis.js) — the value is the standard Reform continuous scale in both cases.
- The star capsule, vibro gate, graph and cache keys are unchanged: `star` is the Sunny raw sr and the cache key includes the algorithm name segment.

## 6. Validation & regression

- **Unit / smoke** (temp scripts, not committed): JS↔fit port equivalence (`maxDiff < 1e-9`), synthetic 7K RC/full-LN charts at 1.0x/1.5x/0.75x, synthetic 4K LN (LN half corrected, RC half and `numericDifficulty` carried through from Sunny), 6K and `HO` fallback, pipeline integration. Monotonicity: `verifyPolarisMonotonicity()` empty, `POLARIS_MONOTONICITY.rawViolations` empty, and a dense level × rate grid over all three curve sets (step 0.05) showing a strictly increasing composite in the star direction and a non-decreasing one in the rate direction.
- **Golden regression** (`test/compare-golden.mjs`): existing algorithms must stay bit-identical. The harness drives Sunny/Daniel/Azusa/Roxy/Mixed directly and does not import Polaris, so widening Polaris's scope cannot move the goldens — the 0-diff compare therefore remains a valid gate for "nothing else changed" rather than exercising the new branch. The new branch is covered by the synthetic smoke cases instead.
- **Benchmark**: `VSRG-DanEstimation-Benchmark` currently contains no 7K samples, so the benchmark cannot score Polaris's 7K side; the behavioural dataset re-scoring above is the acceptance metric for both fits. Do not add per-chart special cases from either source.

## 7. Retune procedure

1. Re-run the fit scripts (temp, gitignored) against a fresh Mania Tracker snapshot; regenerate knots. The 7K and 4K LN sets are fitted on their own populations (`algo`-conditioned rows of the respective side) — do not pool them, and keep the 4K fit's `R ≥ 0` constraint so the rate half of the invariant is a property of the fit.
2. Re-run the group-CV check — do not accept a full-fit improvement without a held-out improvement. Compare against a constant-offset control on the same population: if the shaped curve does not beat the constant out of sample, ship the constant instead.
3. Update `polarisCorrection.js` constants; re-run the port test and smoke tests; regenerate the port vectors from the new knots. **Check `POLARIS_MONOTONICITY.rawViolations` is still empty**: non-empty means the new knots invert the spline and the runtime guard is silently repairing them, which is a fit defect to fix at the source, not to absorb.
4. Re-run the golden compare (0 diffs) and, if the change can affect 4K LN rows, the Mixed benchmark harness before/after.
5. Judge with **signed bias and improved-row share, not MAE** when the within-group spread is much larger than the correction: on the 4K LN population the sd is 0.96 against a +0.19 mean shift, which is exactly the regime where MAE cannot see a mean-shift correction.

---

*Polaris v1 (2026-10). Fitted against the Mania Tracker behavioural dataset snapshot; see also `docs/features/difficulty-estimation.md` and `docs/features/ln-correction.md`.*
