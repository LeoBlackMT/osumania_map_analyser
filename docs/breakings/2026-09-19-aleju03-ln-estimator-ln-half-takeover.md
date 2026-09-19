# 2026-09-19 aleju03 LN 估算器与 Mixed 的 LN 半接管（aleju03 LN estimator and the Mixed LN-half takeover）

## 中文

### 修改内容（What changed）

- 新增估算器 **aleju03**：移植 mania-hub 自研 4K LN 参考邻域估计器（其 `dan/dan-estimator/ln.ts` 的 `estimateLnDan` 与 `features.ts` 的 `extractDanFeatures`）。计算层位于 `js/estimator/aleju03/`：`math.js`（辅助数学）、`features.js`（结构特征）、`lnReference.js`（压力距离 + 参考邻域 + 12 个结构 floor 与 2 个 compression + 课程分段 + 回归兜底）、`lnReferenceCharts.js`（127 行参考数据，逐字提取）；入口为 `js/estimator/aleju03Estimator.js` 的 `runAleju03EstimatorFromText`。
- 该算法作为**可选算法**暴露：`settings.json` 的 `estimatorAlgorithm` options 与 `config.js` 的 `APP_CONFIG.options.estimatorAlgorithm` 均加入 `"aleju03"`。
- 管线接入：`runAnalysisPipeline.js` 增加 `aleju03` 分派分支、把 `aleju03` 加入 `NORMALIZATION_ALGORITHMS`（星数口径恒为 Sunny 原始 sr）、并允许复用 `sharedSunnyResult`（不额外跑 Sunny）。
- **Mixed 的 LN 半接管**（`mixedEstimator.js`）：`LN·Mix` 树（`modeTagFromLnRatio` 判为 `LN` 或 `Mix` 的谱面，HB 谱面也落在其中）的 LN 半**整体**改用 aleju03 的判决，不再区分档位；`RC` 树不接管（RC 图的 LN 半因 `lnRatio ≤ 0.15` 本就不参与标签合成）。**失效回退**：aleju03 抛错、非 4K（`unsupported-keycount`）或不是 LN 候选（返回 null）时保留原表值。RC 半、`numericDifficulty`、胶囊与其它档位完全不变。
- **独立选择时的输出契约**：显式选择 `aleju03` 时，`estDiff` **只含 LN 难度**且使用区间表同款 tier 词表（`LN 7 mid/high`），不再拼接 RC 半；`numericDifficulty` / `numericDifficultyHint` 置 null（该算法不产出 RC 数值）。候选门语义与源实现一致，没有旁路开关。
- **非 LN 候选一律显示 Unknown**：保留源的 LN 候选门（`metadataLnSignal || chartLnSignal`），不是候选的谱面**不再**走 `ln-pressure` 回归兜底，而是返回 `estDiff = "Unknown difficulty"`（`numericDifficulty` 为 null、胶囊仍为 `aleju03`、诊断 `reason` 为 `no-ln-content`（完全无长条）或 `not-an-ln-candidate`）。原因：该回归式只由 Sunny sr 与 NPS 驱动，对近零 LN 的高星 RC 图会给出离谱值——实测一张 `star 12.84 / LN% ≈ 0.05%` 的谱面（4800 音符 3 根长条）曾被算成 **LN 17**（raw 21.54），而候选门正确判定其为"非候选"。回归兜底现在只服务于"通过候选门但最近参考行压力距离 > 2.6"的 LN 谱。
- **选择器别名（选择不生效的根因）**：`js/parser/settingsParser.js normalizeEstimatorAlgorithmValue` 是硬编码白名单——不认识的算法名返回 `null`，`parseEstimatorAlgorithmValue` 随即回退到默认的 `Mixed`，表现为"界面选了 aleju03 但实际仍在跑 Mixed（胶囊显示 Sunny/Azusa）"。现已补上 `aleju03` / `aleju` / `aleju-03`（大小写不敏感）。

### 修改原因（Why）

- 我们的 4K LN 区间表下限为 "LN 5 mid"：其下所有谱面被钳到同一个读数，`expected ≤ 5` 的 20 张系统性低估 **1.645 dan**（17/20 落入 `< LN 5` 边界），该区间 MAE 高达 **1.845**。
- 但缺陷不止在下限：同一批 102 张 LN 谱上，参考邻域方案整体也明显优于区间表（MAE **0.399** 对 **0.861**，≤1.0 命中率 85.3% 对 72.5%），且低段 MAE **0.120** 对 1.845。因此按用户决定**整体替换 LN·Mix 树的 LN 半**，而不是只补下限以下。
- 这不是主观取舍：改动前后的实测对比见"验证方式"。

### 影响范围（Scope）

- 仅 4K：aleju03 对非 4K 返回 `unsupported-keycount` 并整体回退 Sunny（含 `actualEstimatorAlgorithm = "Sunny"`）。
- `LN·Mix` 树的 LN 半全部来自 aleju03（benchmark 的 102 张 LN 谱 102/102 命中，标签与 aleju03 独立运行逐行一致）；**RC 树、`star`、`numericDifficulty` 与胶囊语义与改动前逐字一致**——只有 RC 半来源仍按原路由（Sunny/Azusa/Daniel）。
- 中间态说明：低段专用判定 `isBelowLnTableFloor` 已删除（不再需要"是否读表下限"的判断）。
- 新算法选项影响结果缓存：算法名本就在缓存键第一段，切换算法即 miss；无需新增缓存失效项，也不需要 bump `CACHE_KEY_STAR_UNIFIED_VERSION`（星数口径与输出时间轴均未变化）。
- 未 bump 插件版本（`index.js` `_VERSION` 与 `metadata.txt` `Version` 仍为 2.1.0）。

### 兼容策略（Compat）

- aleju03 无 LN 判决时**整体回退 Sunny**（非 4K、解析失败、特征提取失败），`actualEstimatorAlgorithm` 记为 `"Sunny"`，与 Azusa/Roxy 的既有回退语义一致；诊断字段 `aleju03Ln = { applied, reason, rawDan, displayName, variant, confidence }` 供调试与遥测观察。
- **标签格式与区间表一致**：aleju03 的变体后缀（`++`/`+`/无/`-`/`--`）在入口换算成 tier 词（`high`/`mid-high`/`mid`/`mid-low`/`low`），因此标签形如 `LN 7 mid/high`——与 `intervalLookup` 产出的 `LN 7 mid` 完全同构，下游（按 `||` 与 tier 词解析）不需要任何特例分支；`rcLabelToNumeric` 对 LN-only 标签返回 null，与 `numericDifficulty = null` 一致（Numeric Difficulty 只覆盖 RC 算法，这一行为既有文档已说明）。
- 计算层为共享纯函数（无 `window`/`document`、未 import `js/app/`），浏览器、worker、Node benchmark runner 三端可用；`cvtFlag ∈ {IN, HO}` 时在 `cloneOsuParser` 拷贝上转换，共享 `parsed` 实例保持 pristine。

### 验证方式（Verification）

- 合成冒烟（4K LN / 4K RC / 近零 LN / 7K / 设置解析器，15 项断言）全部通过：LN 谱得到 aleju03 判决且 `actualEstimatorAlgorithm = "aleju03"`；标签匹配既有 LN 格式（`^LN \d+ (low|mid/low|mid|mid/high|high)$`）且不含 RC 半；4K RC（LN% = 0）显示 `Unknown difficulty`、`reason = "no-ln-content"`；近零 LN（2000 音符 2 根长条）同样 `Unknown difficulty`、`reason = "not-an-ln-candidate"`；两者胶囊均为 `aleju03`、`numericDifficulty === null`；`normalizeEstimatorAlgorithmValue("aleju03") === "aleju03"` 且既有算法名不受影响；非 4K 以 `unsupported-keycount` 拒绝并保留 Sunny 标签。
- 接管与回退断言（`temp/aleju03-port/verify-ln-takeover.mjs`，10/10 通过，全部为合成谱、不读样本）：4K LN 与 4K Mix 谱的 `Mixed.estDiff` LN 半等于 aleju03 标签且不等于原表值；4K 纯 RC 谱不产生 LN 半；非 4K LN 谱的 LN 半与 RC 半都保持原表值（aleju03 的 `unsupported-keycount` 回退）。
- LN 实测（benchmark 的 `osu.csv` 中 `pattern=ln` 的 102 张，倍速 1.0，Δ = expected − got；Mixed 的数值取自官方 runner 日志中的 LN 标签，因为 runner 对同时返回 `numericDifficulty` 的算法优先取该数值）：

| 方案 | MAE | RMSE | bias | ≤0.5 | ≤1.0 |
| --- | --- | --- | --- | --- | --- |
| aleju03（独立算法） | **0.399** | 0.678 | +0.234 | 74.5% | 85.3% |
| Mixed（改动后，LN 标签口径） | **0.399** | 0.678 | +0.234 | 74.5% | 85.3% |
| Sunny（现有 LN 区间表） | 0.861 | 1.229 | +0.020 | 55.9% | 72.5% |

  低段子集 `expected ≤ 5`（20 张）：aleju03 MAE **0.120** / Sunny MAE **1.845**（bias −1.645）。
- 逐行一致性：**Mixed 的 LN 标签与 aleju03 独立运行 102/102 相同**（逐行比对 runner 日志的 `estDiff` LN 段与 `results/aleju03.csv`）。
- 注意（避免后续误读 benchmark 结果）：runner 在算法返回有限 `numericDifficulty` 时优先用它当作 `got`，而 Mixed 的 `numericDifficulty` 是 **RC** 数值，因此 `results/Mixed.csv` 的 LN 行数值不会因本次改动而变化——要观察 LN 半必须看 `estDiff`。
- 移植保真性旁证：本仓库移植的独立算法实测 0.399 与 mania-hub 原实现实测 0.418 一致。

---

## English

### What changed

- New estimator **aleju03**: a port of mania-hub's in-house 4K LN reference-neighbour estimator (`estimateLnDan` plus `extractDanFeatures`). The calculation layer lives in `js/estimator/aleju03/`: `math.js` (helpers), `features.js` (structural features), `lnReference.js` (pressure distance, reference neighbourhood, 12 structural floors and 2 compressions, course segmentation, regression fallback), `lnReferenceCharts.js` (127 reference rows, extracted verbatim). The entry point is `runAleju03EstimatorFromText` in `js/estimator/aleju03Estimator.js`.
- Exposed as a **selectable algorithm**: `"aleju03"` was added to `settings.json`'s `estimatorAlgorithm` options and to `APP_CONFIG.options.estimatorAlgorithm` in `config.js`.
- Pipeline wiring: `runAnalysisPipeline.js` gains the `aleju03` dispatch branch, adds `aleju03` to `NORMALIZATION_ALGORITHMS` (star is always the raw Sunny SR) and allows reusing `sharedSunnyResult` (no extra Sunny pass).
- **Mixed LN-half takeover** (`mixedEstimator.js`): the LN half of the `LN·Mix` tree (charts that `modeTagFromLnRatio` classifies as `LN` or `Mix`, which includes HB charts) is taken from aleju03 **as a whole**, with no tier-dependent condition; the `RC` tree is not touched (an RC chart's LN half never reaches the label because `lnRatio <= 0.15`). **Fallback**: when aleju03 throws, is not 4K (`unsupported-keycount`) or is not an LN candidate (returns null), the interval table's value is kept. The RC half, `numericDifficulty`, the capsule and every other tier are unchanged.
- **Output contract when selected standalone**: `estDiff` carries **LN difficulty only**, in the interval tables' own tier vocabulary (`LN 7 mid/high`), with no RC half; `numericDifficulty` / `numericDifficultyHint` are set to null (this estimator produces no RC numeric). Candidate-gate semantics match upstream, with no bypass switch.
- **Anything that is not an LN candidate shows Unknown**: the upstream candidate gate (`metadataLnSignal || chartLnSignal`) is kept, and charts that fail it are **no longer** pushed through the `ln-pressure` regression; they return `estDiff = "Unknown difficulty"` (`numericDifficulty` null, capsule stays `aleju03`, diagnostic `reason` is `no-ln-content` when there are no holds at all, otherwise `not-an-ln-candidate`). Rationale: that regression is driven purely by Sunny SR and NPS, so on high-star near-zero-LN RC charts it produced absurd values — a measured `star 12.84 / LN% ~ 0.05%` chart (4800 notes, 3 holds) came out as **LN 17** (raw 21.54) while the gate correctly called it a non-candidate. The regression now only serves LN candidates whose nearest reference row is farther than 2.6 in pressure distance.
- **Selector alias (the root cause of "selection does nothing")**: `js/parser/settingsParser.js normalizeEstimatorAlgorithmValue` is a hardcoded whitelist — an unrecognised algorithm name returns `null`, after which `parseEstimatorAlgorithmValue` falls back to the default `Mixed`, so the UI shows aleju03 selected while Mixed keeps running (capsule shows Sunny/Azusa). The aliases `aleju03` / `aleju` / `aleju-03` (case-insensitive) are now accepted.

### Why

- Our 4K LN interval table starts at "LN 5 mid": everything below clamps onto one reading, and the 20 charts with `expected <= 5` are systematically **under-rated by 1.645 dan** (17/20 land on the `< LN 5` boundary) for an MAE of **1.845** in that band.
- The defect is not limited to the floor: on the same 102 LN charts the reference-neighbour approach is clearly better overall (MAE **0.399** against **0.861**, `<= 1.0` hit rate 85.3% against 72.5%), and in the low band **0.120** against 1.845. The LN half of the `LN·Mix` tree is therefore replaced **entirely**, by explicit decision, rather than only below the floor.
- This is measured, not asserted: see Verification.

### Scope

- 4K only: for other key counts aleju03 returns `unsupported-keycount` and the result falls back to Sunny wholesale (`actualEstimatorAlgorithm = "Sunny"`).
- The whole `LN·Mix` tree now takes its LN half from aleju03 (all 102 benchmark LN charts, with labels matching aleju03's standalone run row by row); the `RC` tree, `star`, `numericDifficulty` and capsule semantics are byte-identical to before, with the RC half still coming from the original routing (Sunny/Azusa/Daniel).
- Intermediate state: the low-band-only helper `isBelowLnTableFloor` has been removed (the "is the table below its floor" question is no longer asked).
- Result caching: the algorithm name already forms the first cache-key segment, so switching algorithms is a miss; no new invalidation entry and no `CACHE_KEY_STAR_UNIFIED_VERSION` bump is needed (star semantics and output timeline are unchanged).
- No version bump (`index.js` `_VERSION` and `metadata.txt` `Version` remain 2.1.0).

### Compatibility

- When aleju03 has no LN verdict it **falls back to Sunny entirely** (non-4K, parse failure, feature-extraction failure) and records `actualEstimatorAlgorithm = "Sunny"`, matching the existing Azusa/Roxy fallback semantics; the diagnostic field `aleju03Ln = { applied, reason, rawDan, displayName, variant, confidence }` is available for debugging and telemetry.
- **Labels match the interval tables**: aleju03's variant suffix (`++`/`+`/none/`-`/`--`) is converted at the entry point into tier words (`high`/`mid-high`/`mid`/`mid-low`/`low`), so labels read `LN 7 mid/high` and are isomorphic to `intervalLookup`'s `LN 7 mid`; downstream consumers that split on `||` and read tier words need no special case. `rcLabelToNumeric` returns null for an LN-only label, consistent with `numericDifficulty = null` (Numeric Difficulty only covers RC algorithms, as the existing docs already state).
- The calculation layer is shared and pure (no `window`/`document`, no `js/app/` imports) and works in the browser, the worker and the Node benchmark runner; with `cvtFlag ∈ {IN, HO}` conversion runs on a `cloneOsuParser` copy so the shared `parsed` instance stays pristine.

### Verification

- Synthetic smoke (4K LN / 4K RC / near-zero LN / 7K / settings parser, 15 assertions) passes: LN charts get an aleju03 verdict with `actualEstimatorAlgorithm = "aleju03"`; labels match the existing LN label format (`^LN \d+ (low|mid/low|mid|mid/high|high)$`) and carry no RC half; a 4K RC chart (LN% = 0) shows `Unknown difficulty` with `reason = "no-ln-content"`; a near-zero LN chart (2000 notes, 2 holds) also shows `Unknown difficulty` with `reason = "not-an-ln-candidate"`; both keep the `aleju03` capsule and `numericDifficulty === null`; `normalizeEstimatorAlgorithmValue("aleju03") === "aleju03"` while existing names keep working; non-4K is rejected as `unsupported-keycount` and keeps the Sunny label.
- Takeover and fallback assertions (`temp/aleju03-port/verify-ln-takeover.mjs`, 10/10, all synthetic, no sample data): for a 4K LN and a 4K Mix chart the `Mixed.estDiff` LN half equals aleju03's label and differs from the table value; a 4K pure-RC chart produces no LN half; a 6K LN chart keeps both halves from the table (aleju03's `unsupported-keycount` fallback).
- LN measurement (benchmark `osu.csv`, `pattern=ln`, 102 charts, rate 1.0, delta = expected - got; Mixed's numbers are read from the LN labels in the official runner log because the runner prefers a finite `numericDifficulty` when the algorithm returns one):

| Configuration | MAE | RMSE | bias | <=0.5 | <=1.0 |
| --- | --- | --- | --- | --- | --- |
| aleju03 (standalone) | **0.399** | 0.678 | +0.234 | 74.5% | 85.3% |
| Mixed (after the change, LN-label basis) | **0.399** | 0.678 | +0.234 | 74.5% | 85.3% |
| Sunny (existing LN table) | 0.861 | 1.229 | +0.020 | 55.9% | 72.5% |

  Low band `expected <= 5` (20 charts): aleju03 MAE **0.120**, Sunny MAE **1.845** (bias -1.645).
- Row-by-row agreement: **Mixed's LN label equals aleju03's standalone label on 102/102 charts** (runner log `estDiff` LN segments compared against `results/aleju03.csv`).
- Note, to avoid misreading the benchmark later: the runner uses a finite `numericDifficulty` as `got` when the algorithm returns one, and Mixed's numeric is the **RC** value, so `results/Mixed.csv` LN rows do not change with this commit — the LN half is only visible in `estDiff`.
- Fidelity cross-check: this repository's port scores 0.399 standalone against mania-hub's own measurement of 0.418.
