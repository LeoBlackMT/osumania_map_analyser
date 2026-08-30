# Roxy 4K RC Difficulty Estimator

Roxy is a synchronous 4-key regular-chain difficulty estimator for osu!mania. It combines a structural strain model with a compact meta calibration head. The structural layer reads the chart directly; the reference layer uses Azusa and Daniel as numeric signals. Sunny is computed only for Azusa reuse and graph support, and is disabled as an independent meta input. Roxy does not call Mixed or Companella, and Azusa remains available as an independent selectable estimator.

## 1. Scope

Roxy is intended for 4K RC charts within the **high-difficulty band (numeric 11~17, Alpha to Emik Zeta high)**. It rejects maps that are outside its scope:

- empty or unparsable input
- non-mania beatmaps
- non-4K beatmaps
- LN ratio above `0.18`
- fewer than `80` tap notes
- non-finite or non-positive speed rate
- final numeric below `11` (returns `< Alpha Low`, numeric null)
- final numeric at or above `17` (returns `> Emik Zeta high`, numeric null)
- internal estimator errors

Scope-boundary results use the same estimator result shape as valid results, but return no numeric difficulty: `estDiff` is the boundary label (`< Alpha Low` / `> Emik Zeta high`) and `numericDifficulty` is `null`. Mixed treats these as unusable (via the numeric-null check in `canUseRcResult`) and routes the low band to Azusa. This mirrors Daniel, which also only emits a native numeric within its own band.

## 2. Pipeline

```text
Read .osu text
  -> canonicalize the time axis for speedRate
  -> parse canonicalized .osu text
  -> build 4K tap rows
  -> compute row, hand, column, rhythm, entropy, and NPS features
  -> update seven structural strain streams
  -> aggregate streams into structural numeric difficulty
  -> compute Sunny and Daniel once
  -> call Azusa with those precomputed references
  -> build meta features from Azusa/Daniel/Roxy; keep Sunny slot disabled
  -> evaluate ridge linear calibration head
  -> apply explicit OD override correction, high-reference structural floor, and reference-gap residual correction
  -> apply difficulty-gated Azusa fusion
  -> format RC label and optional Azusa graph
```

The reference order is deliberately fixed on the canonicalized, OD-neutral analysis text:

1. Compute Sunny once for Azusa reuse and optional graph support.
2. Reuse `precomputedDanielResult` when it is valid for the same analysis path; otherwise compute Daniel once.
3. Call Azusa with both precomputed results.

This keeps Roxy, Azusa, Daniel, and Sunny aligned without recomputing Daniel or Sunny inside Azusa. Mixed may still pass its Sunny baseline into Roxy, but the current public Roxy path clears that external Sunny before the meta reference call so the reference layer remains OD-neutral and canonicalized. Sunny is not treated as a separate voting signal in the meta head.

## 3. Basic Functions

Roxy uses small bounded primitives instead of unbounded raw ratios.

```text
clamp(x, lo, hi) = min(max(x, lo), hi)

g(x, a, b)  = clamp((x - a) / (b - a), 0, 1)
gi(x, a, b) = clamp((b - x) / (b - a), 0, 1)

r(dt, base, offset, power)
  = min(8, (base / max(16, dt + offset)) ^ power)

decay(state, input, dt, tau)
  = state * exp(-dt / tau) + input
```

`g` is a rising gate, `gi` is a falling gate, and `r` maps shorter intervals to larger strain with a hard cap.

## 4. Speed-Rate Canonicalization

`speedRate` is treated as a pure time-axis transform. Roxy does not apply a post-score rate bonus, monotonic projection, or rate-specific special case. Instead it rewrites the timing-dependent parts of the `.osu` text and then analyzes the result at `analysisSpeedRate = 1`.

Let `firstObjectTime` be the first hitobject start time and `canonicalFirstObjectMs = 1000`:

```text
canonicalTime(t) =
  floor(t / speedRate - firstObjectTime / speedRate + canonicalFirstObjectMs)
```

The transform is applied to:

- timing point timestamps
- positive timing point beat lengths
- break event start/end timestamps
- hitobject start times
- LN end times

After this step, using `speedRate = 1.3` on an original chart is intended to follow the same analysis path as an equivalent pre-speeded `1.3x` `.osu` file. Roxy uses `floor` for timestamp conversion because the benchmark pre-speeded `.osu` files follow floor-style integer conversion.

## 5. Row Model

Rows are built from the canonicalized text. In the normal analysis path this means:

```text
t = canonicalizedStartTime
```

Notes within `2 ms` are merged into one row. Each row stores:

- `mask`: 4-bit column mask
- `rowSize`: number of active columns
- `leftCount`, `rightCount`: notes on columns `0-1` and `2-3`
- `dtRow`: interval from previous row
- `dtSame[c]`: interval from previous note in column `c`
- `dtHand[h]`: interval from previous active row on hand `h`
- `handMask[h]`: active mask for each hand
- `nps250`, `nps500`, `nps1000`, `nps4000`: rolling density windows

Hand split is fixed as columns `0-1` for left hand and `2-3` for right hand.

Important row-level features:

```text
rotation[h] = 1
  when current hand mask and previous non-empty same-hand mask have no overlap

sameHandOverlap = (overlapLeft + overlapRight) / 2

rowChord = (rowSize - 1) / 3

sameHandChord =
  (max(0, leftCount - 1) + max(0, rightCount - 1)) / 2

rhythmChaos =
  min(2, abs(log2((dtRow + 24) / (prevDtRow + 24)))) / 2
```

Two entropy windows are maintained over `750 ms`:

- `entropy750`: frequency entropy of the 16 possible row masks
- `transitionEntropy750`: frequency entropy of 256 possible `prevMask -> mask` transitions

## 6. Structural Inputs

Roxy converts every row into seven input signals.

### Speed

```text
speedIn =
  0.55 * r(dtRow, 155, 30, 1.06)
+ 0.30 * max_h r(dtHand[h], 180, 40, 1.08)
+ 0.15 * mean_h r(dtHand[h], 180, 40, 1.08)
```

Speed is based on row interval and hand interval, not directly on NPS.

### Jack

```text
anchorRow = 1 if any dtSame[c] <= 220 ms else 0

jackIn =
  max_c r(dtSame[c], 185, 35, 1.18)
* (1 + 0.20 * rowChord + 0.15 * anchorRow)
```

### Hand Stream

```text
handIn =
  max_h (
    0.70 * r(dtHand[h], 180, 38, 1.10)
  + 0.30 * rotation[h] * r(dtHand[h], 205, 45, 1.05)
  )
```

### Chord

```text
chordIn =
  rowChord * (1 + 0.18 * speedIn)
+ 0.22 * sameHandChord
+ max(0, rowSize - 2) * r(dtRow, 150, 80, 0.85)
```

### Chordjack

```text
chordjackIn =
  rowChord * (
    0.55 * jackIn
  + 0.30 * sameHandOverlap
  + 0.15 * handIn
  )
```

### Tech

```text
techIn =
  0.32 * rhythmChaos
+ 0.24 * entropy750
+ 0.24 * transitionEntropy750
+ 0.20 * (mask != prevMask ? 1 : 0)
```

### Stamina

```text
handStamina[h] =
  decay(handStamina[h], r(dtHand[h], 180, 40, 1.08), dtHand[h], 8000)

staminaIn =
  0.40 * log1p(nps1000) / log(24)
+ 0.35 * log1p(nps4000) / log(24)
+ 0.25 * max(handStamina[0], handStamina[1])
```

## 7. Strain Streams

Each structural input updates a burst and sustain state. The stream output is a weighted mix of those states.

| Stream | Burst tau | Sustain tau | Burst weight | Sustain weight |
|---|---:|---:|---:|---:|
| speed | 220 | 1600 | 0.78 | 0.22 |
| hand | 260 | 2200 | 0.80 | 0.20 |
| jack | 300 | 1800 | 0.88 | 0.12 |
| chordjack | 260 | 2400 | 0.82 | 0.18 |
| tech | 450 | 3200 | 0.70 | 0.30 |
| stamina | 1200 | 10000 | 0.58 | 0.42 |
| course | 30000 | 120000 | 0.35 | 0.65 |

The local raw strain is the weighted sum of stream outputs:

```text
localRaw =
  0.22 * speed
+ 0.18 * hand
+ 0.16 * jack
+ 0.16 * chordjack
+ 0.12 * tech
+ 0.11 * stamina
+ 0.05 * course
```

## 8. Aggregation

For each stream:

```text
A =
  0.30 * q97
+ 0.22 * q90
+ 0.18 * tailMeanTop4%
+ 0.15 * q75
+ 0.10 * powerMean(p = 2.4)
+ 0.05 * q50
```

Roxy also computes a fixed `400 ms` section peak aggregation over `localRaw`:

```text
sectionAgg = sum(sortedPeak[i] * 0.9^i) / sum(0.9^i)
```

The raw structural score is:

```text
rawAgg = 0.80 * weightedAgg + 0.20 * sectionAgg
logRaw = ln(1 + max(0, rawAgg))
preNumeric = linearMap(logRaw, p02, p98, -2, 20)
```

The pre-numeric value is corrected structurally, passed through an isotonic mapping, then clamped to the RC numeric range.

## 9. Global Statistics

Roxy measures chart-wide shape to adjust local strain:

```text
activeDurationSec = (lastT - firstT - inactiveMs) / 1000
inactiveMs = sum(gap - 1000 for gap > 1000)
breakDensity = breakCount / max(activeDurationSec / 60, 1)
avgNps = tapCount / max(activeDurationSec, 1)
handBias = abs(leftLoad - rightLoad) / max(leftLoad, rightLoad, 1e-6)
```

Other statistics include chord rate, triple rate, same-hand overlap rate, rotation rate, same-hand interval Q10, fast jack rate, anchor rate, anchor imbalance, and peak-to-sustain gap.

## 10. Structural Corrections

The correction layer handles pattern families that are not well represented by a single strain sum:

- low-density chordjack lift
- high-speed stream lift
- very dense high-chord damping
- long course break damping
- sustained course lift
- dense jumpstream lift and damping
- anchor jack lift
- hand-bias lift

The total correction is clamped before the isotonic mapping.

## 11. Meta Calibration

The meta layer receives four numeric sources:

| Source | Meaning |
|---|---|
| Azusa | Azusa result using Roxy's precomputed Sunny and Daniel references |
| Sunny | Computed or supplied for Azusa reuse and graph support; disabled as an independent meta reference |
| Daniel | 4K RC reference, computed once or supplied by caller |
| Roxy | Roxy structural numeric before meta calibration |

Feature groups:

- `pred_*` and `has_*` for each source
- min, max, mean, median, and range over available predictions
- pairwise differences for `Azusa/Daniel`, `Azusa/Sunny`, `Azusa/Roxy`, `Daniel/Sunny`, `Daniel/Roxy`, and `Sunny/Roxy`
- structural numeric details
- correction terms
- stream summaries
- global statistics and small interaction features

Available reference numeric values are bucketed to `1.0` difficulty before they enter `pred_*`, aggregate prediction features, and pairwise difference features. Missing references are filled with the median of the available bucketed predictions, while the matching `has_*` feature remains `0`. This prevents reference availability changes, such as Daniel becoming valid at one adjacent rate, from injecting a `0 -> 11` discontinuity into pairwise features, and it keeps `speedRate` calls stable against the `+-1 ms` timestamp conversion commonly introduced by pre-speeded `.osu` files.

The generated meta head is a standardized ridge linear model. It is intentionally less sharp than the earlier tree model because split thresholds were too sensitive to tiny timestamp and reference changes. This is a deliberate tradeoff: the current model gives up a large amount of in-benchmark fit in exchange for stable `speedRate` equivalence, OD-neutral references, and less benchmark-distribution memorization.

The Sunny feature slots remain in the schema for compatibility with the generated feature list, but the live Sunny prediction is set unavailable before feature construction. Those slots therefore carry fallback values plus `has_Sunny = 0`, not a live Sunny vote.

After meta evaluation, a structural backstop prevents the calibrated value from falling slightly below Roxy's own structural score. The backstop is gated from structural numeric `12.25` to `14.0`, targets `structuralNumeric - 0.15`, and only applies when the gap is positive but no larger than `0.35`. This keeps it from acting as a broad high-difficulty special case.

After OD correction and the high-reference structural floor, Roxy applies a very small reference-gap residual correction only when no explicit OD override is present. The correction compares the current unguarded output with Azusa, Daniel, and the structural score:

```text
azusaGap      = Azusa - base
danielGap     = Daniel - base
structuralGap = structuralNumeric - base

features = [
  azusaGap,
  danielGap,
  structuralGap,
  abs(azusaGap),
  abs(danielGap),
  azusaGap * chordRate,
  azusaGap * rotationRate,
  azusaGap / (sameHandQ10 + 1),
  danielGap * chordRate,
  structuralGap * gate(avgNps, 12, 24)
]

referenceGapCorrection =
  clamp(ridge(features), -0.30, 0.30) * 0.33
```

The final contribution is therefore limited to about `+-0.10` numeric difficulty. This keeps the correction from becoming another benchmark selector while recovering a small amount of residual error where all references disagree with the current calibrated output in the same direction.

## 12. OD Override

Roxy accepts `odFlag`, `OD`, `od`, or `overallDifficulty` in the options object. OD only changes the estimate when one of those override values is explicitly supplied. Parsed map OD is retained for debug output and for deriving `HR`/`EZ`, but a normal no-override call has `odCorrection = 0`.

Supported override values are:

- `HR`
- `EZ`
- a numeric OD value, used for DA-style OD override

The OD transform follows Sunny's judgement-window logic:

```text
effectiveOD = baseOD                         if no override
effectiveOD = 6.462 + 0.715 * baseOD         if HR
effectiveOD = -20.761 + 2.566 * baseOD       if EZ
effectiveOD = numeric override               otherwise

rawWindow = 0.3 * sqrt((64.5 - ceil(od * 3)) / 500)
judgeWindow(od) = min(rawWindow, 0.6 * (rawWindow - 0.09) + 0.09)

neutralWindow = judgeWindow(9)
pressureRatio = clamp(neutralWindow / effectiveWindow, 0.55, 1.85)

odCorrection =
  0 if no explicit override
  else clamp(
    log(pressureRatio)
  * (3.20 + 1.90 * gate(numeric, 6, 18) + 0.60 * gate(numeric, 14, 18.4)),
    -2.20,
    2.20
  )
```

Explicit OD correction is applied after the meta model and before final output. The correction uses OD9 as the neutral judgement-window baseline, so two maps with the same arrangement and different file OD keep the same no-override reference estimate. Only an explicit `HR`, `EZ`, or numeric DA-style OD override adds judgement pressure. The meta reference layer is also kept OD-neutral because the training data does not contain reliable OD-varied samples; feeding OD-shifted reference predictions into that model can create unstable reversals.

## 13. High-Reference Structural Floor

The meta calibration layer can under-read high-density RC charts when only Azusa is available as a high reference and Daniel is invalid or weak. Roxy applies a gated floor after OD correction when all of these are true:

- neutral Azusa reference is at least `17.0`, with confidence gradually increasing until `20.0`
- `avgNps >= 25`
- `chordRate >= 0.70`
- `sameHandQ10 <= 95`
- combined density/chord/three-note/jack/chordjack/fast-hand/duration pressure has entered the activation gate; the pressure gate rises from `0.22` to `0.46`

The floor interpolates from a structural pressure target toward an Azusa-relative target, with extra activation when Sunny or Daniel is unavailable and a small negative OD damp:

```text
pressureGate = gate(pressure, 0.22, 0.46)
activation =
  clamp(pressureGate * gate(Azusa, 17.0, 18.0) + missingReferenceBoost, 0, 1)
confidence = pressureGate * gate(Azusa, 17.0, 20.0)
referenceFloor = Azusa - (0.45 - 0.25 * confidence)
structuralFloor = 16.65 + 1.55 * confidence + 0.35 * rawGate + 0.25 * highNpsGate
structuralTarget = structuralFloor + min(0, odCorrection) * 0.25
referenceTarget = max(referenceFloor, structuralFloor) + min(0, odCorrection) * 0.25
floor = structuralTarget + (referenceTarget - structuralTarget) * activation
floor = clamp(floor, 16.8, min(18.65, Azusa + 0.30))
```

This is a targeted structural-reference guard for dense RC outliers, not a general replacement for the meta model.

## 14. Azusa Fusion

After every post-processing correction (OD override, high-reference structural floor, reference-gap residual correction, Azusa high-gap lift), Roxy blends its final numeric with the independent Azusa prediction at a fixed weight:

```text
fused = finalNumeric + (Azusa - finalNumeric) * 0.4
```

The rationale is variance reduction: two approximately unbiased estimators, when averaged, shrink variance. The weight is biased toward Roxy (0.4 on Azusa) because Azusa has larger variance. On the benchmark this lifts Exact to ~54% in the 11~17 band (with the high-difficulty scope, §1). No difficulty gate is needed anymore: the low band (where Azusa over-estimates) is already routed to Azusa by Mixed before Roxy runs, so the fusion only ever applies inside the 11~17 scope. The fusion does not change Roxy's structural core, meta features, or any other reference; it only re-averages the final output against Azusa.

## 15. Ordinal Meta Calibration

The ridge meta head is fitted on the 0.5-grid quantized `expected` labels instead of raw continuous values. Training against the quantized target teaches the model to place predictions on the 0.5 tier grid, which matches how the benchmark grades difficulty (the numeric difficulty of dan tiers moves in 0.5 steps).

Fit details:
- fit subset: scope 11~17 **plus the >=17 band** (6 maps). Including the upper band prevents the model from under-shooting the 17 boundary — with 11~17-only fitting, Mixed's >=17 Exact dropped from 52.6% to 42.1%; with the upper band included it recovers to 52.6%.
- normalization: full-distribution MEAN/SCALE (all rows), so out-of-scope inference does not distort
- lambda: 2.0, ordinal target grid 0.5

The output `numericDifficulty` stays **continuous** (no quantization): it equals the fully post-processed `finalNumeric` rounded to 2 decimals, and therefore always maps to exactly the same `estDiff` the plugin displays. (A 0.5-grid output quantization was tried and reverted: it inflated benchmark Exact by ~1.9pp but made the benchmark's inferred tier label disagree with the plugin's `estDiff`, sometimes by a full tier.)

On the benchmark this lifts Roxy 11~17 Exact from ~55% to ~59% (MAE 0.229 -> 0.219) and Mixed 4K RC 11~17 Exact from ~54% to ~58% (MAE 0.240 -> 0.233), with no Azusa regression and no variant-label loss.

## 16. Label Soft Cap

Roxy keeps numeric difficulty internally, but its RC label display is capped above `CloverWisp Theta high`:

```text
if numericDifficulty > 18.4:
    estDiff = "> CloverWisp Theta high"
else:
    estDiff = numericToRcLabel(numericDifficulty)
```

This prevents Roxy from displaying `Iota` or higher labels while still allowing the numeric value and star value to reflect values above Theta high.

## 17. Graph Output

The numeric calculation uses Roxy's structural strain data. The returned `graph` field does not use Roxy's local strain series. When graph output is requested, Roxy returns the graph provided by the Azusa reference call, which currently resolves to Azusa/Sunny graph data.

## 19. Marathon Duration Correction (Estimator-Embedded)

Since v2.0.2, Roxy applies a **marathon duration correction** inside the estimator itself: `options.marathonCorrection = { durationS, ettValues }` (optional; absent or missing MSD skillsets → no correction, bit-identical to the legacy output). The corrected `finalNumeric` drives `numericDifficulty`, `estDiff` and `star` derivation (the `> 18.4` soft-cap label rule still applies), so the estimator output is the final difficulty value — no post-output pipeline patching. Direct calls without the option (e.g. the benchmark runner) yield the uncorrected baseline.

**Mechanism and inspiration.** Inspired by the marathon correction in [Dan-Overlay](https://github.com/acarranzao1a-png/Dan-Overlay) (`pipeline.py` `_merge_primary_and_mina`, calibrated against the 6th–10th Reform Marathon Packs), which lowers the estimate of long, evenly difficult charts where accumulated stamina strain inflates the difficulty beyond the peak sections' true demand. The mechanism is ported with the correction target changed from SR/DP to Roxy's `numericDifficulty` (dan-tier numeric) and the taper moved from the SR domain to the numeric domain. See [features/marathon-correction.md](features/marathon-correction.md).

**Trigger conditions** (all must hold):
- drain duration > 300 s (last − first note start, unscaled by rate);
- Etterna MSD skillsets available, and skillset balance holds: `max(jack, stream, stamina, tech)/total < 0.45` with `jack = max(JackSpeed, Chordjack)`, `stream = max(Stream, Jumpstream)`, `stamina = 0.7·Stamina + 0.3·Handstream`, `tech = Technical`;
- `numericDifficulty` is a finite number (scope-out results — `< Alpha Low` / `> Emik Zeta high` with null numeric — are never touched);
- numeric < 16 (taper, see below).

**Correction** (lower-only, never raises): `corr = min(0.50, 0.40 × ln(1 + excessMin)) × taper(numeric)`, where `excessMin = (durationS − 300)/60`. The duration penalty is **log-saturating** (sub-linear) so that adjacent dan-tier course charts never swap order (the linear penalty difference exceeded the base numeric gap on REFORM 2nd 8th/9th; verified zero new inversions across all course packs). `taper(numeric) = 1` for numeric ≤ 10, linear to 0 at numeric ≥ 16. `estDiff` is redriven from the corrected numeric via `numericToRcLabel` (the `> 18.4` soft-cap label rule still applies); the star field is untouched (already normalized to the Sunny raw star for Roxy display).

**Calibration note.** Parameters were calibrated on the benchmark's `course` subset (34 maps, all RC; user-authorized) with an order-constrained grid; `scale = 0.40`, `cap = 0.50` chosen (merged MAE 0.3167, zero new inversions vs the linear 0.20/min version's 0.3225 which failed the 8th/9th order check). Validation: course MAE 0.4036 → 0.2250 (Roxy 14 in-scope rows), Exact 21.4% → 50.0%; see the calibration table in [features/marathon-correction.md](features/marathon-correction.md) §8.

## 18. Complexity

Let `N` be the number of tap notes and `R` the number of merged rows.

- parsing and row construction: `O(N)`
- rolling NPS windows: `O(R)` with two pointers
- entropy windows: `O(R)` with bounded mask tables
- strain update and aggregation: `O(R)`
- meta feature build and ridge evaluation: bounded by fixed feature count
- memory: `O(R)` for rows and strain arrays, plus reference estimator memory

Roxy is heavier than a single estimator because it uses Sunny, Daniel, and Azusa references, but Sunny and Daniel are each computed at most once in the normal Roxy path. `speedRate` is handled by canonicalizing the input text once, so it no longer triggers recursive baseline probes or extra guard calls.
