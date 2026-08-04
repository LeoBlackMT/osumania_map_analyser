# Azusa Estimator Documentation

---

## 1. Overview

Azusa is a **multi-stage continuous estimator** that takes an osu!mania 4K `.osu` beatmap file as input and outputs:

| Field | Type | Description |
|-------|------|-------------|
| `estDiff` | string | Human-readable RC tier label (e.g. "Alpha mid") |
| `numericDifficulty` | number | Continuous RC value (range -2 ~ 20) |
| `star` | number | Linear-mapped star rating |
| `rawNumericDifficulty` | number | Primary numeric before blend and calibration |
| `debug` | object | Full intermediate values for analysis |

The pipeline consists of **6 stages** executed sequentially:

```
Parse → Difficulty Curve → Primary Numeric → Blend → Calibration → Correction
```

---

## 2. Configuration Constants

All tunable parameters are centralized in `AZUSA_CONFIG`.

### 2.1 Input Constraints

| Constant | Value | Description |
|----------|-------|-------------|
| `rcLnRatioLimit` | 0.18 | Maps with LN ratio > 18% are rejected |
| `minNotes` | 80 | Minimum note count for stable estimation |
| `rowToleranceMs` | 2 | Time window (ms) for chord detection |

### 2.2 Skill Weights

| Skill | Weight | Rationale |
|-------|--------|-----------|
| `speed` | 0.36 | Overall note rate — strongest predictor |
| `stamina` | 0.24 | Sustained density over time |
| `chord` | 0.12 | Multi-note complexity (reduced: chordRate has low correlation with RC) |
| `tech` | 0.16 | Rhythm irregularity and column movement |
| `jack` | 0.12 | Same-column repetition (separated from speed) |

### 2.3 Decay Model

| Parameter | Value |
|-----------|-------|
| `decayWindowsMs` | [140, 280, 560, 980] ms |
| `decayWeights` | [0.34, 0.30, 0.22, 0.14] |
| `localPower` | 2.15 (power-mean exponent for per-note skill fusion) |

Four exponentially-decaying windows simulate short/medium/long-term memory of strain. The weighted sum of all four windows gives the instantaneous skill value.

### 2.4 Length Bonus

| Parameter | Value |
|-----------|-------|
| `lengthRefNotes` | 600 |
| `lengthExponent` | 0.22 |
| `lengthCap` | 3.5 |

Formula: `min(3.5, pow(noteCount / 600, 0.22))`

Uses a power function with slow sublinear growth to prevent over-estimation of marathon/course maps.

---

## 3. RC Label System

### 3.1 Tier Constants

**`GREEK_BY_INDEX`**: Greek letter names for tiers 11–20.
```
Alpha, Beta, Gamma, Delta, Epsilon, Emik Zeta, Thaumiel Eta, CloverWisp Theta, Iota, Kappa
```

**`RC_TIER_CANDIDATES`**: Five sub-tier offsets for fine-grained labeling.
```
low (-0.4), mid/low (-0.2), mid (0), mid/high (+0.2), high (+0.4)
```

### 3.2 Mapping Rules

| Numeric Range | Label Format |
|---------------|-------------|
| ≤ 0 | Intro 1 ~ 3 |
| 1 ~ 10 | Reform 1 ~ 10 |
| 11 ~ 20 | Alpha ~ Kappa |

Each integer base is further divided into 5 sub-tiers. The final label is determined by nearest-neighbor search across all (base, offset) combinations.

---

## 4. Utility Functions

### 4.1 `clamp(value, min, max)`
Standard numeric clamping.

### 4.2 `safeDiv(a, b, fallback)`
Division with NaN/Inf/zero protection.

### 4.3 `fmt4(value)`
Formats a number to 4 decimal places for debug output. Returns `null` for non-finite values.

### 4.4 `piecewiseLinear(x, knots, valueCol)`
Generic piecewise linear interpolation over a sorted knot table.

- `knots` is an array of `[x, y1, y2, ...]` tuples
- `valueCol` selects which column to interpolate (default 1 for `[x, y]` format)
- Used by: `calibrateAzusaOutputNumeric`

### 4.5 `piecewiseBlock(x, blocks)`
Piecewise constant + linear transition interpolation for block calibration.

- `blocks` is an array of `[xMin, xMax, y]` tuples
- Within a block range, returns the constant `y`
- Between blocks, linearly interpolates
- Used by: `calibrateAzusaNumeric`

### 4.6 `quantileFromSorted(sortedValues, q)`
Linear-interpolated quantile from a pre-sorted array.

### 4.7 `powerMean(values, p)`
Generalized power mean (Hölder mean) of order `p`.

### 4.8 `expDecayFactor(dtMs, tauMs)`
Exponential decay factor: `exp(-dt / tau)`. Returns 1 when `dt ≤ 0`.

### 4.9 `skillFromStates(states)`
Weighted sum of the 4 decay-window states using `decayWeights`.

---

## 5. Stage 1: Parsing & Preprocessing

### 5.1 `buildErrorResult(code, message, extras)`
Constructs a standardized error/invalid result object with null difficulty and a diagnostic message.

### 5.2 `buildTapNotes(parsed)`
Extracts tap notes from the parser output. Each note has:
- `t`: time (ms)
- `c`: column index (0-3)
- `hand`: 0 for left (cols 0,1), 1 for right (cols 2,3)
- `rowSize`: initially 1, updated by `annotateRows`

Returns notes sorted by `(time, column)`.

### 5.3 `annotateRows(taps, toleranceMs)`
Merges simultaneous notes (within `toleranceMs`) into rows. Updates each note's `rowSize` to the count of notes in that row. This is the foundation for chord detection.

---

## 6. Stage 2: Difficulty Curve (`buildDifficultyCurve`)

**Function**: `buildDifficultyCurve(taps)`

This is the **core feature extraction** stage. For each note in sequence, it:

1. Computes time intervals:
   - `dtGlobal` — time since previous note (any column)
   - `dtSame` — time since previous note in the same column
   - `dtHand` — time since previous note on the same hand
   - `dtAny` — time since the 2nd-most-recent note

2. Computes sliding-window density:
   - `d250` — NPS in a 250ms window
   - `d500` — NPS in a 500ms window

3. Computes per-note feature inputs:

| Input | Formula | Purpose |
|-------|---------|---------|
| `stream` | `(170 / (dtAny + 30))^1.07` | Overall note rate |
| `handStream` | `(185 / (dtHand + 42))^1.08` | Per-hand note rate |
| `jack` | `(190 / (dtSame + 35))^1.16` | Same-column repetition speed |
| `speedInput` | `0.60·stream + 0.30·handStream + 0.10·jack` | Combined speed signal |
| `jackInput` | `jack × (1 + 0.15·chord)` | Jack amplified by chords |
| `staminaInput` | `0.48·(d500/11) + 0.27·(d250/15) + 0.25·stream` | Sustained density |
| `chordInput` | `chord × (1 + 0.10·min(1.5, stream))` | Chord complexity |
| `techInput` | `0.45·rhythmChaos + 0.30·movement + 0.25·chordRowPenalty` | Technical complexity |

4. Updates 5 × 4 decay-state accumulators (5 skills × 4 windows):
   ```
   state[j] = state[j] × exp(-dtGlobal / tau[j]) + input
   ```

5. Computes instantaneous skill values via weighted sum of 4 windows.

6. Fuses 5 skills into a single `local` difficulty value using power-mean with exponent `localPower = 2.15`.

**Return value** — a curve object containing all per-note series, density arrays, jack raw series, column counts, and chord counts.

---

## 7. Stage 3: Primary Numeric (`computeAzusaNumericFromCurve`)

**Function**: `computeAzusaNumericFromCurve(curve, noteCount)`

Aggregates the difficulty curve into a single primary numeric value.

### 7.1 Summary Statistics

For each of the 5 skill series, compute:

| Statistic | Percentile |
|-----------|-----------|
| `q97` | 97th percentile (peak difficulty) |
| `q90` | 90th percentile |
| `q75` | 75th percentile (sustained difficulty) |
| `q50` | 50th percentile (median difficulty) |
| `tailMean` | Mean of top 4% (extreme peak) |
| `pm` | Power mean (p=2.6) |

### 7.2 Blended Scores

| Blend | Formula | Weight |
|-------|---------|--------|
| `peakBlend` | Weighted sum of q97 + q90 across 5 skills | 0.52 |
| `sustainBlend` | Weighted sum of q75 + tailMean across 5 skills | 0.26 |
| `densityBlend` | `0.14·log1p(density250) + 0.22·log1p(density500)` | 0.10 |
| `midBlend` | Weighted sum of q50 across 5 skills | 0.08 |
| `lengthBoost` | `min(3.5, (notes/600)^0.22)` | 0.04 |

```javascript
raw = 0.52·peak + 0.26·sustain + 0.10·density + 0.08·mid + 0.04·length
scaled = 0.82 + 0.43·raw
```

### 7.3 Structural Adjustments

**`chordjackBoost`**: Multiplicative interaction bonus for maps with simultaneous high chord-rate and high jack density. Formula:

```
chordjackBoost = clamp(2.5 × g_chord × g_jack × g_anchor, 0, 2.2)

where:
  g_chord = clamp((chordRate - 0.40) × 3.5, 0, 1)
  g_jack  = clamp((jackQ95 - 1.25) × 2.8, 0, 1)
  g_anchor = clamp(1 - anchorImbalance × 8, 0, 1)
```

This specifically addresses the under-estimation of low chordjack patterns by activating when chordRate > 0.40 and jackQ95 > 1.25, while being suppressed for anchor-heavy maps (high anchorImbalance).

**`midSpeedBonus`**: Small bonus for maps with moderate NPS (9–19 range), where the strain model has the least discriminative power.

```
midSpeedBonus = clamp((avgNPS - 9) × 0.04, 0, 0.35) × clamp((19 - avgNPS) × 0.25, 0, 1)
```

**Final primary**:
```javascript
corrected = scaled + chordjackBoost + midSpeedBonus
return clamp(corrected, -2, 20)
```

---

## 8. Stage 4: Reference Blend (`resolveRcBlendComponents`)

**Function**: `resolveRcBlendComponents(primary, daniel, sunny, curveHints)`

Fuses Azusa's primary numeric with two reference algorithms (Daniel and Sunny) to improve accuracy, particularly in ranges where Azusa's features have less discriminative power.

### 8.1 Gate Mechanism

```
lowGateSource = daniel ?? sunny ?? primary
lowGate  = clamp((9.61 - lowGateSource) / 4.94, 0, 1)
highGate = 1 - lowGate
```

- When the map is **easy** (daniel < 9.61): `lowGate > 0`, use sunny-dominant lowBase
- When the map is **hard** (daniel ≥ 9.61): `highGate > 0`, use daniel-dominant highBase

### 8.2 Low Base (sunny-dominant)

```
lowBase = -8.317 + 1.536·sunny + 0.011·primary + 0.049·daniel
        + lowGate × [0.442·max(0, sunny - 9.84)
                    + 0.016·max(0, primary - 10.4)
                    + 0.235·(max(0, 7.935 - sunny))²]
```

### 8.3 High Base (daniel-dominant)

```
highBase = 0.809·daniel + 0.057·primary + 0.165·sunny + 0.183
         + highMask × [-0.154·max(0, primary - daniel)
                      + 0.081·max(0, sunny - daniel)]
         + anchorLift
```

Where `highMask = clamp((lowGateSource - 14.83) / 2.667, 0, 1)` provides additional correction for very high-difficulty maps.

**`anchorLift`**: A small residual correction (max ±0.25) for maps with extreme jack density and very low chord rate.

### 8.4 Final Blend

```
lowLift = max(0, 9.889 - lowGateSource) × 0.257
value   = lowBase × lowGate + (highBase + lowLift) × highGate
```

---

## 9. Stage 5: Two-Level Calibration

### 9.1 Block Calibration (`calibrateAzusaNumeric`)

**Function**: `calibrateAzusaNumeric(value, lowGate, highGate)`

Uses `piecewiseBlock` to interpolate through two block tables:
- **`AZUSA_CALIBRATION_LOW_BLOCKS`**: 17 blocks covering numeric range [1.92, 9.83] → RC [1.0, 10.3]
- **`AZUSA_CALIBRATION_HIGH_BLOCKS`**: 39 blocks covering numeric range [11.43, 19.35] → RC [10.4, 17.95]

The low and high calibrations are blended by gate weight, providing a smooth transition between the two calibration regimes.

### 9.2 Curve Gap Residual (`computeCurveGapResidualCorrection`)

**Function**: `computeCurveGapResidualCorrection(...)`

A fitted linear model using 19 interaction terms between `x` (the calibrated value), `highGate`, `ds` (daniel - sunny), `sp` (sunny - primary), and curve statistics (`anchorImbalance`, `chordRate`, `jackQ95`). Output is clamped to [-1.2, 1.2].

### 9.3 Isotonic Output Calibration (`calibrateAzusaOutputNumeric`)

**Function**: `calibrateAzusaOutputNumeric(value)`

Uses `piecewiseLinear` over **`AZUSA_ISOTONIC_POINTS`**: a 79-point isotonic regression table mapping pre-output values to final RC values. This is the primary calibration table, fitted from the full 526-sample benchmark dataset using the Pool Adjacent Violators Algorithm (PAVA).

---

## 10. Stage 6: Reference Correction (`computeReferenceCorrection`)

**Function**: `computeReferenceCorrection(azusaEst, daniel, sunny)`

A gated correction applied to the calibrated output in the mid-to-high range [10.0, 17.5]:

| Sub-range | Gate | daniel coeff | sunny coeff |
|-----------|------|-------------|-------------|
| [10.0, 11.5) | Linear ramp | 0.10 | 0.06 |
| [11.5, 12.5) | Full (1.0) | 0.20 | 0.13 |
| [12.5, 16.0) | Full (1.0) | 0.40 | 0.25 |
| [16.0, 17.5) | Linear decay | 0.28 | 0.17 |

Formula: `correction = gate × (coeffD × (daniel - x) + coeffS × (sunny - x))`, clamped to [-1.2, 1.2].

This leverages the observation that Daniel and Sunny have higher discriminative power than Azusa in the Alpha–Eta range (RC 11–17), while Azusa is more reliable at the extremes.

---

## 11. Calibration Tables

### 11.1 Block Calibration Tables

**`AZUSA_CALIBRATION_LOW_BLOCKS`** (17 entries): Maps blend values in [1.92, 9.83] to RC [1.0, 10.3]. Each entry is `[xMin, xMax, rcValue]`. The block structure allows for piecewise-constant calibration with linear transitions between blocks.

**`AZUSA_CALIBRATION_HIGH_BLOCKS`** (39 entries): Maps blend values in [11.43, 19.35] to RC [10.4, 17.95]. Same structure as low blocks but with finer granularity in the upper range.

### 11.2 Isotonic Calibration Table

**`AZUSA_ISOTONIC_POINTS`** (79 entries): Each entry is `[preOutputValue, rcValue]`. Fitted using the Pool Adjacent Violators Algorithm (PAVA) on 526 benchmark samples. This is a monotonic non-decreasing function that provides the final RC mapping.

---

## 12. Algorithm Complexity

- **Time**: O(n) where n = number of tap notes. The main loop in `buildDifficultyCurve` processes each note once with constant work per note (4 decay windows × 5 skills = 20 exponential decays). The `summarize` step sorts 5 arrays of size n: O(n log n). Overall complexity is dominated by O(n log n).

- **Space**: O(n) for storing per-note series (5 skill series + local + times + density arrays ≈ 8n values).

- **Browser compatibility**: Pure JavaScript with no external runtime dependencies beyond the OsuFileParser, Daniel, and Sunny estimators (all part of the same project). No Node.js-specific APIs are used.