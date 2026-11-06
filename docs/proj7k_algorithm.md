# Proj7k Estimator Documentation

> Target audience: humans / AI. Proj7k is a JavaScript port of the [proj7k](https://github.com/ka1z07/proj7k) 7K difficulty engine (spec v0.2.3, upstream commit `915e5af`). This document describes its scope, design, output semantics and how its parity with the upstream engine is checked.
> Shared-module constraints apply (see docs/guides/module-conventions.md): the estimator is DOM-free, pure and runs identically under Node and the browser.

---

## 1. Overview

Proj7k is an **independent 7K physical model**. Unlike Polaris it does not start from Sunny's star: it reads the chart through a finger-level model of the two hands and solves for the player level at which the chart's expected misses reach a tolerance. Its own star rating is then placed on the JinJin Dan ladder.

```
Parse → notes (column, head, tail) → finger mapping → per-event physical demand
      → per-hand load → threshold solve (+ hardest-window solve) → stars
      → star → level (JinJin Dan stars) → label / numericDifficulty
                                                 └→ technique split (8 skills, debug only)
```

| Field | Type | Description |
|-------|------|-------------|
| `estDiff` | string | One Regular label from the engine's stars, e.g. `Regular 6 mid/high`; outside the ladder `< Regular 0 low` / `> Regular Stellium high` |
| `numericDifficulty` | number \| null | Continuous level, 0 = Regular 0 … 14 = Regular Stellium (Gamma 11, Azimuth 12, Zenith 13, same numbering as Polaris); `null` outside [−0.5, 14.5] |
| `star` | number | Sunny raw sr (unchanged; the star capsule always shows the Sunny sr) |
| `actualEstimatorAlgorithm` | string | `"Proj7k"` when applied; `"Sunny"` when the estimator fell back |
| `debug` | object | `{ rate, proj7kStars, proj7kD, level, thumbHand, dominantSkill, dominanceMargin, skills }`; `skills` holds stars, level, coverage and dominance for each of the 8 skills |

There is no separate LN half label. The engine has one dan scale shared by all of its technique pools, and it deliberately reads LN lower than RC by design, so an LN half label from the same stars would only repeat the RC label.

## 2. Scope & fallback

| Condition | Behaviour |
|---|---|
| 7K chart | Proj7k applies |
| Any other key count | Sunny result returned, `actualEstimatorAlgorithm = "Sunny"` |
| `cvtFlag` contains `IN` or `HO` | Sunny (the converted chart is not the chart the engine was calibrated on) |
| Empty chart or non-finite result | Sunny |

Speed mods divide every note time by `speedRate` before evaluation, so the engine sees the chart as played.

## 3. Design framework

The full specification is upstream (`osu-mania-7k-difficulty-spec.md`); this section only gives the framework.

1. **Fingers, not columns.** Each column is a specific finger (ring, middle, index, thumb, index, middle, ring); presses and releases are both events. The thumb's column is tried on each hand and the hand giving the lower total difficulty is kept.
2. **Physical demand per event.** An event's demand comes from how fast the same finger repeats, how much the other fingers of the same hand interfere (neighbouring fingers couple more than distant ones), the other hand's interference, and the constraint that held LNs put on the rest of the hand. Cost rises faster than speed near a finger's limit, so the same pattern played faster reads super-linearly harder.
3. **Hand load.** Each hand carries a decaying load; an event reads its hand's sustained load rather than an instantaneous peak.
4. **Difficulty as a threshold.** Each event has a miss probability that depends on the player level and the event's reading. The chart's difficulty is the level at which the expected misses equal a tolerance. Short windows are also solved on their own, so a short dense burst is not averaged away.
5. **Star scale.** A two-parameter curve maps that level to stars. It is fixed by a few human anchors on the JinJin Dan ladder; the rest of the engine is a physically motivated prior, not fitted on a large dataset.
6. **Technique split.** Each of the 8 skills (RC jack / tech / speed / stamina, LN general / tech / inverse / release) has its own cost series built from parts of the event demand: the finger's own repeat for jack, the rest of the hand's motion for speed, that motion sustained without rest and weighted by chord size for stamina, rhythm irregularity and snap diversity for tech, and presses under held columns / isolated precise releases for the LN specialties. Each series goes through the same hand accumulator and threshold solve, and is put on the skill's own star line, so "rc_tech 7 stars" reads as "requires 7th-tier tech". The total difficulty does not depend on this layer.
7. **Dominant skill.** A skill's level is its stars over what a chart of this total usually requires of it. The dominant skill is the one with the highest level within the chart's family (an LN chart when the mean LN context is at least one half); `ln_general` is the LN umbrella and holds unless a specialty is clearly above usual. Skills are exposed in `debug` only.

## 4. Star → dan label

`PROJ7K_DAN_STARS` in `proj7kEstimator.js` is the upstream `dan_table.json`: the star rating of each of the 15 JinJin Dan tiers, taken as the tier-level stars of the four RC technique pools.

- The continuous level interpolates linearly in `ln(star)` between adjacent tiers, and extends past the ends along the first and last segments.
- The label is the nearest tier plus a sub-tier from the offset to it, on the same offsets Polaris uses (low −0.4, mid/low −0.2, mid 0, mid/high +0.2, high +0.4).

## 5. Parity with the upstream engine

`js/estimator/proj7k/engine.js` is a line-by-line port of the Python engine (`src/proj7k/engine/`). It was checked on the upstream repository's 120-chart JinJin Dan benchmark (8 technique pools × 15 tiers). Across all 120 charts:

- total stars and all 8 skill stars match to within 4e-15, skill levels to within 1e-15 (floating-point rounding);
- the chosen thumb hand and the dominant skill are identical on every chart.

Evaluation takes about 35 ms per chart under Node 22.

## 6. Updating

When the upstream engine changes, update three things together and re-run the parity check against the new upstream reference:

- the parameters in `engine.js` (`PARAMS`);
- the star scale constants `STAR_A` and `STAR_B`, plus `SKILL_SCALE` and `SKILL_EXPECT`;
- `PROJ7K_DAN_STARS`.
