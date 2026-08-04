# Roxy 4K RC Difficulty Estimator

Roxy is a synchronous 4-key regular-chain difficulty estimator for osu!mania. It combines a structural strain model with a compact meta calibration head. The structural layer reads the chart directly; the reference layer uses Azusa and Daniel as numeric signals. Sunny is computed only for Azusa reuse and graph support, and is disabled as an independent meta input. Roxy does not call Mixed or Companella, and Azusa remains available as an independent selectable estimator.

## 1. Scope

Roxy is intended for 4K RC charts. It rejects maps that are outside its scope:

- empty or unparsable input
- non-mania beatmaps
- non-4K beatmaps
- LN ratio above `0.18`
- fewer than `80` tap notes
- non-finite or non-positive speed rate
- internal estimator errors

Invalid results use the same estimator result shape as valid results, but return no numeric difficulty.

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

## 14. Label Soft Cap

Roxy keeps numeric difficulty internally, but its RC label display is capped above `CloverWisp Theta high`:

```text
if numericDifficulty > 18.4:
    estDiff = "> CloverWisp Theta high"
else:
    estDiff = numericToRcLabel(numericDifficulty)
```

This prevents Roxy from displaying `Iota` or higher labels while still allowing the numeric value and star value to reflect values above Theta high.

## 15. Graph Output

The numeric calculation uses Roxy's structural strain data. The returned `graph` field does not use Roxy's local strain series. When graph output is requested, Roxy returns the graph provided by the Azusa reference call, which currently resolves to Azusa/Sunny graph data.

## 16. Complexity

Let `N` be the number of tap notes and `R` the number of merged rows.

- parsing and row construction: `O(N)`
- rolling NPS windows: `O(R)` with two pointers
- entropy windows: `O(R)` with bounded mask tables
- strain update and aggregation: `O(R)`
- meta feature build and ridge evaluation: bounded by fixed feature count
- memory: `O(R)` for rows and strain arrays, plus reference estimator memory

Roxy is heavier than a single estimator because it uses Sunny, Daniel, and Azusa references, but Sunny and Daniel are each computed at most once in the normal Roxy path. `speedRate` is handled by canonicalizing the input text once, so it no longer triggers recursive baseline probes or extra guard calls.
