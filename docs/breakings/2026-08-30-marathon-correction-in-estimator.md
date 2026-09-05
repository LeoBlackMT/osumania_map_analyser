# docs/breakings/2026-08-30-marathon-correction-in-estimator.md

> 日期：2026-08-30 ｜ 分支：feat/marathon-correction（v2.0.2）
> 类别：管线/估算器破坏性更改（马拉松时长修正架构重构：管线派生段 → 估算器内嵌）

## 修改内容（What changed）

马拉松时长修正从 `runAnalysisPipeline` 的派生段（§11，估算器输出后、返回前应用）**移动到估算器本体**：

- `runAzusaEstimatorFromText` / `runRoxyEstimatorFromText` 新增可选参数 `options.marathonCorrection = { durationS, ettValues }`：内部在 finalNumeric 计算后应用只降不升修正（对数饱和 `min(0.50, 0.40×ln(1+excessMin))` + numeric taper 10~16），`estDiff`/`star`/`numericDifficulty` 统一由修正后值派生。缺省/无 MSD 时不触发（与旧输出逐位一致）。
- `runAnalysisPipeline`：删除派生段；新增**按需前置 Ett**（`durationS > 300` 且 4K 且算法 ∈ {Azusa, Roxy, Mixed} 时先算 Ett，注入 `options.marathonCorrection`，并复用于段 9/10）。
- 模块 `js/estimator/marathonCorrection.js`：`computeMarathonCorrection` 保持不变（估算器内部调用）；`applyMarathonCorrectionToRcResult` 保留为测试/工具 API（管线不再使用）。

## 修改原因（Why，性能依据）

用户架构决策：**修正应当应用在估算器上，估算器最后输出的数值化难度即给到前端**——管线层"输出后修正"使修正游离于算法本体之外，且暗示对所有算法生效。重构同时遵守 perf 约束（docs/breakings/2026-08-09-perf-analysis-pipeline.md）：① 解析仍共享一次（13→1-2 不变，无重复解析）；② **无估算器重跑**（修正发生在首次估算内部，不引入双倍计算）；③ 前置 Ett 仅命中长图 RC 候选（>300s 4K），普通谱面零额外开销，且该 Ett 复用于既有段 9/10（与展示共用一次 WASM 调用，总计算量不增）。

## 影响范围（Scope）

- 行为：Azusa/Roxy（含 Mixed 路由命中）在 >300s 且 MSD 均衡条件下的输出降低（修正后数值化难度），`estDiff` 相应重派生；其他算法/短图/无 MSD 场景输出不变。
- 管线段序：解析后新增按需前置 Ett（估算之前）；段 9 在 `withEtterna=true` 且已有前置结果时复用，不再二次调用 WASM。
- 基准口径：本仓库 `results/` 的 Azusa/Roxy/Mixed 数据按"插件等价调用"重跑（候选行注入 `marathonCorrection`）→ `got` 为修正后数值（course 行变化：Azusa 30/34、Roxy 13/34、Mixed 30/34）；benchmark 参考 runner 缺省不传该参数 → 算法本体（无修正）口径；两者差异即修正本身（显式双口径）。
- 性能：>300s 的 4K 谱面在 `withEtterna=false` 场景会多一次前置 WASM 调用（修正数据源所需）；<300s 与离线场景零变化。

## 兼容策略（Compat）

- 估算器缺省行为逐位兼容（golden/基准回归不传参即可验证）；`marathonCorrection` 为纯增量可选参数。
- 前置 Ett 仅作为段 9 的**复用优先**路径：`withEtterna=true` 时输出与独立计算逐位一致（smoke 验证 ettResult 复用一致）；失败时复用错误状态，不重复尝试。
- 结果缓存键（star-v2｜算法｜identity｜modSignature）不含修正参数——修正恒定应用（无开关），键语义稳定。

## 验证方式（Verification）

- 冒烟：本地冒烟脚本（12/12 PASS，不入库）——估算器参数化（with/without 参数对照、estDiff 重派生）、短图不受影响、pipeline 前置+复用（`withEtterna=false` 也生效）、Sunny 不受影响、ett 复用与独立计算逐位一致、无参回归逐位一致。
- 基准回归：osu.csv（746 行）Azusa/Roxy/Mixed 全量按修正口径重跑——course 行体现修正（Azusa 30/34、Roxy 13/34、Mixed 30/34，8th 9.28<9th 9.40 排序保持），非候选项与旧版逐字一致（缺省行为未被破坏，修正严格走参数通道）。
- course 子集修正效果（参数校准记录）：Roxy MAE 0.4036→0.2250、Azusa MAE 0.4782→0.3544（Bias −0.085）；8th/9th 相邻段位排序修复（对数饱和）；全 pack 相邻段位零新增倒挂（详细见 docs/features/marathon-correction.md §8）。

---

# docs/breakings/2026-08-30-marathon-correction-in-estimator.md (English)

> Date: 2026-08-30 | Branch: feat/marathon-correction (v2.0.2)
> Category: Pipeline/estimator breaking change (marathon correction relocated: pipeline patch → estimator-embedded)

## What changed

The marathon duration correction moved from the `runAnalysisPipeline` post-output patch stage (§11) **into the estimators themselves**:

- `runAzusaEstimatorFromText` / `runRoxyEstimatorFromText` gain an optional `options.marathonCorrection = { durationS, ettValues }`: applied right after finalNumeric (lower-only, log-saturating `min(0.50, 0.40×ln(1+excessMin))` + numeric taper 10~16); `estDiff`/`star`/`numericDifficulty` are derived from the corrected value. Absent or missing MSD → no correction, bit-identical legacy output.
- `runAnalysisPipeline`: the patch stage is removed; an **on-demand pre-Ett** stage is added (`durationS > 300` && 4K && algorithm ∈ {Azusa, Roxy, Mixed}), injecting `options.marathonCorrection` and being reused by stages 9/10.
- `js/estimator/marathonCorrection.js`: `computeMarathonCorrection` unchanged (now called inside the estimators); `applyMarathonCorrectionToRcResult` kept as a test/utility API.

## Why (with perf evidence)

User architecture decision: the correction belongs to the estimator, whose output numeric difficulty goes straight to the frontend — post-output patching in the pipeline detaches the correction from the algorithm and implies it applies to all algorithms. The refactor also honors the perf constraints (docs/breakings/2026-08-09-perf-analysis-pipeline.md): ① parsing stays shared (13→1-2, no duplicate parse); ② **no estimator rerun** (correction happens inside the first pass, no double compute); ③ the pre-Ett only fires for long RC candidates (>300 s, 4K) and is reused by the existing Ett stages (one WASM call, no net increase).

## Scope

- Behavior: Azusa/Roxy (incl. Mixed routing) outputs lower on >300 s balanced charts; `estDiff` redriven; all other cases unchanged.
- Pipeline order: post-parse on-demand pre-Ett before estimation; stage 9 reuses it when `withEtterna=true`, no second WASM call.
- Benchmark: reference runner calls estimators without the option → baseline (uncorrected); plugin pipeline injects it → corrected values shown (two-tier semantics explicit).
- Perf: >300 s 4K maps pay one extra pre-Ett WASM call when `withEtterna=false` (needed as the correction data source); <300 s and offline paths unchanged.

## Compat

- Estimator default behavior is bit-compatible (golden/benchmark without the option). The option is purely additive.
- Pre-Ett is a reuse-first path for stage 9: output identical to an independent computation (smoke-verified); failures reuse the error state without retry.
- Result-cache key (`star-v2|algorithm|identity|modSignature`) excludes the correction parameter — correction is unconditionally applied (no switch), key semantics stable.

## Verification

- Smoke: local-only smoke script (not in the repo), 12/12 PASS (parameterized estimator, short-map invariance, pipeline pre-Ett + reuse, Sunny untouched, ett reuse bit-identical, no-param regression bit-identical).
- Benchmark regression: osu.csv (746 rows) fully rerun with correction injection — course rows moved (Azusa 30/34, Roxy 13/34, Mixed 30/34; 8th 9.28 < 9th 9.40 ordering kept), non-candidate rows byte-identical to the previous HEAD results (default behavior intact, correction strictly parameter-gated).
- Course-subset effect (calibration record): Roxy MAE 0.4036→0.2250, Azusa 0.4782→0.3544 (Bias −0.085); 8th/9th adjacent-tier ordering fixed (log-saturation); zero new inversions across course packs (see docs/features/marathon-correction.md §8).