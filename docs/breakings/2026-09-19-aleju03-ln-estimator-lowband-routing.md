# 2026-09-19 aleju03 LN 估算器与低段 LN 路由（aleju03 LN estimator and low-band LN routing）

## 中文

### 修改内容（What changed）

- 新增估算器 **aleju03**：移植 mania-hub 自研 4K LN 参考邻域估计器（其 `dan/dan-estimator/ln.ts` 的 `estimateLnDan` 与 `features.ts` 的 `extractDanFeatures`）。计算层位于 `js/estimator/aleju03/`：`math.js`（辅助数学）、`features.js`（结构特征）、`lnReference.js`（压力距离 + 参考邻域 + 12 个结构 floor 与 2 个 compression + 课程分段 + 回归兜底）、`lnReferenceCharts.js`（127 行参考数据，逐字提取）；入口为 `js/estimator/aleju03Estimator.js` 的 `runAleju03EstimatorFromText`。
- 该算法作为**可选算法**暴露：`settings.json` 的 `estimatorAlgorithm` options 与 `config.js` 的 `APP_CONFIG.options.estimatorAlgorithm` 均加入 `"aleju03"`。
- 管线接入：`runAnalysisPipeline.js` 增加 `aleju03` 分派分支、把 `aleju03` 加入 `NORMALIZATION_ALGORITHMS`（星数口径恒为 Sunny 原始 sr）、并允许复用 `sharedSunnyResult`（不额外跑 Sunny）。
- **Mixed 低段 LN 路由**（`mixedEstimator.js`）：当 4K 谱面的 LN 区间表读到下限（`intervalLookup` 返回以 `<` 开头的标签，例如 `< LN 5 mid`）时，LN 半改用 aleju03 的判决；aleju03 未产出 LN 判决时保留原表结果。RC 半、`numericDifficulty` 与其它档位完全不变。

### 修改原因（Why）

- 我们的 4K LN 区间表下限为 "LN 5 mid"，其下所有谱面被钳到同一个读数：在 benchmark 的 102 张 LN 谱上，`expected ≤ 5` 的 20 张系统性低估 **1.645 dan**（17/20 落入 `< LN 5` 边界），该区间 MAE 高达 **1.845**。
- mania-hub 的参考邻域方案在同一批谱上表现显著更好（该区间 MAE **0.120**，且在其自研 LN 的独立验证中同样占优）；把它的判决接在表下限以下，是"只补盲区、不动既有档位"的最小改动。
- 这不是主观取舍：改动前后的实测对比见"验证方式"。

### 影响范围（Scope）

- 仅 4K：aleju03 对非 4K 返回 `unsupported-keycount` 并整体回退 Sunny（含 `actualEstimatorAlgorithm = "Sunny"`）。
- 仅"表读到下限"的谱面会被 Mixed 替换 LN 半（benchmark 的 102 张 LN 谱中 19 张）；其余谱面的 `estDiff`、`star`、`numericDifficulty` 与改动前逐字一致。
- 新算法选项影响结果缓存：算法名本就在缓存键第一段，切换算法即 miss；无需新增缓存失效项，也不需要 bump `CACHE_KEY_STAR_UNIFIED_VERSION`（星数口径与输出时间轴均未变化）。
- 未 bump 插件版本（`index.js` `_VERSION` 与 `metadata.txt` `Version` 仍为 2.1.0）。

### 兼容策略（Compat）

- aleju03 无 LN 判决时**整体回退 Sunny**（RC 谱面、非 4K、结构候选门未通过），`actualEstimatorAlgorithm` 记为 `"Sunny"`，与 Azusa/Roxy 的既有回退语义一致；诊断字段 `aleju03Ln = { applied, reason, rawDan, displayName, confidence }` 供调试与遥测观察。
- LN 标签格式沿用 `"RC || LN n…"`（`composeDifficultyFromRcLn` / `estDiff` 的既有约定），aleju03 的显示名（`LN 7+` 等）与区间表的 `LN 7 mid` 同构，下游 `||` 分割消费方无需改动。
- 计算层为共享纯函数（无 `window`/`document`、未 import `js/app/`），浏览器、worker、Node benchmark runner 三端可用；`cvtFlag ∈ {IN, HO}` 时在 `cloneOsuParser` 拷贝上转换，共享 `parsed` 实例保持 pristine。

### 验证方式（Verification）

- 合成冒烟（4K LN / 4K RC / 7K 三种输入，6 项断言）全部通过：LN 谱得到 aleju03 判决且 `actualEstimatorAlgorithm = "aleju03"`；RC 谱回退 Sunny 且 `estDiff` 与 Sunny 逐字相同；非 4K 以 `unsupported-keycount` 拒绝。
- LN 实测（benchmark 的 `osu.csv` 中 `pattern=ln` 的 102 张，倍速 1.0，Δ = expected − got）：

| 方案 | MAE | RMSE | bias | ≤0.5 | ≤1.0 |
| --- | --- | --- | --- | --- | --- |
| aleju03（独立算法） | **0.399** | 0.678 | +0.234 | 74.5% | 85.3% |
| Sunny（现有 LN 区间表） | 0.861 | 1.229 | +0.020 | 55.9% | 72.5% |
| Mixed（改动后） | **0.528** | 0.802 | +0.238 | 67.6% | 84.3% |

  低段子集 `expected ≤ 5`（20 张）：aleju03 MAE **0.120** / Sunny MAE **1.845**（bias −1.645）/ Mixed MAE **0.350**。
- 路由安全性：19 张触发低段接管；**未触发的谱面 Mixed 与 Sunny 逐字一致（0 处不一致）**。
- 移植保真性旁证：同一批谱上，本仓库移植实现的误差（独立算法 0.399、生产路由 0.528）与 mania-hub 原实现实测（0.418 / 0.530）一致。

---

## English

### What changed

- New estimator **aleju03**: a port of mania-hub's in-house 4K LN reference-neighbour estimator (`estimateLnDan` plus `extractDanFeatures`). The calculation layer lives in `js/estimator/aleju03/`: `math.js` (helpers), `features.js` (structural features), `lnReference.js` (pressure distance, reference neighbourhood, 12 structural floors and 2 compressions, course segmentation, regression fallback), `lnReferenceCharts.js` (127 reference rows, extracted verbatim). The entry point is `runAleju03EstimatorFromText` in `js/estimator/aleju03Estimator.js`.
- Exposed as a **selectable algorithm**: `"aleju03"` was added to `settings.json`'s `estimatorAlgorithm` options and to `APP_CONFIG.options.estimatorAlgorithm` in `config.js`.
- Pipeline wiring: `runAnalysisPipeline.js` gains the `aleju03` dispatch branch, adds `aleju03` to `NORMALIZATION_ALGORITHMS` (star is always the raw Sunny SR) and allows reusing `sharedSunnyResult` (no extra Sunny pass).
- **Mixed low-band LN routing** (`mixedEstimator.js`): when a 4K chart's LN interval table reads below its floor (`intervalLookup` returns a label starting with `<`, e.g. `< LN 5 mid`), the LN half is taken from aleju03 instead; if aleju03 yields no LN verdict the table result is kept. The RC half, `numericDifficulty` and every other tier are untouched.

### Why

- Our 4K LN interval table starts at "LN 5 mid", so everything below clamps onto one reading: across the benchmark's 102 LN charts, the 20 charts with `expected ≤ 5` are systematically **under-rated by 1.645 dan** (17/20 land on the `< LN 5` boundary) for an MAE of **1.845**.
- mania-hub's reference-neighbour approach is markedly better on the same set (MAE **0.120** in that band, and it also leads in their own LN validation). Splicing it in below the table floor is the minimal change that repairs the blind spot without touching existing tiers.
- This is measured, not asserted: see Verification.

### Scope

- 4K only: for other key counts aleju03 returns `unsupported-keycount` and the result falls back to Sunny wholesale (`actualEstimatorAlgorithm = "Sunny"`).
- Only charts whose table reading is below the floor get their LN half replaced by Mixed (19 of the benchmark's 102 LN charts); every other chart's `estDiff`, `star` and `numericDifficulty` are byte-identical to before.
- Result caching: the algorithm name already forms the first cache-key segment, so switching algorithms is a miss; no new invalidation entry and no `CACHE_KEY_STAR_UNIFIED_VERSION` bump is needed (star semantics and output timeline are unchanged).
- No version bump (`index.js` `_VERSION` and `metadata.txt` `Version` remain 2.1.0).

### Compatibility

- When aleju03 has no LN verdict it **falls back to Sunny entirely** (RC charts, non-4K, structural candidate gate failed) and records `actualEstimatorAlgorithm = "Sunny"`, matching the existing Azusa/Roxy fallback semantics; the diagnostic field `aleju03Ln = { applied, reason, rawDan, displayName, confidence }` is available for debugging and telemetry.
- LN labels keep the `"RC || LN n…"` shape used by `composeDifficultyFromRcLn`/`estDiff`; aleju03's display names (`LN 7+` etc.) are isomorphic to the table's `LN 7 mid`, so downstream `||` consumers need no change.
- The calculation layer is shared and pure (no `window`/`document`, no `js/app/` imports) and works in the browser, the worker and the Node benchmark runner; with `cvtFlag ∈ {IN, HO}` conversion runs on a `cloneOsuParser` copy so the shared `parsed` instance stays pristine.

### Verification

- Synthetic smoke (4K LN / 4K RC / 7K inputs, 6 assertions) passes: LN charts get an aleju03 verdict with `actualEstimatorAlgorithm = "aleju03"`; RC charts fall back to Sunny with a byte-identical `estDiff`; non-4K is rejected as `unsupported-keycount`.
- LN measurement (benchmark `osu.csv`, `pattern=ln`, 102 charts, rate 1.0, Δ = expected − got):

| Configuration | MAE | RMSE | bias | ≤0.5 | ≤1.0 |
| --- | --- | --- | --- | --- | --- |
| aleju03 (standalone) | **0.399** | 0.678 | +0.234 | 74.5% | 85.3% |
| Sunny (existing LN table) | 0.861 | 1.229 | +0.020 | 55.9% | 72.5% |
| Mixed (after the change) | **0.528** | 0.802 | +0.238 | 67.6% | 84.3% |

  Low band `expected ≤ 5` (20 charts): aleju03 MAE **0.120**, Sunny MAE **1.845** (bias −1.645), Mixed MAE **0.350**.
- Routing safety: 19 charts triggered the low-band takeover, and **non-triggering charts are byte-identical between Mixed and Sunny (0 mismatches)**.
- Fidelity cross-check: on the same charts, this repository's port scores 0.399 (standalone) and 0.528 (production routing) against mania-hub's own measurements of 0.418 and 0.530.
