# 2026-11-11 Mixed 4K RC 路由对称融合与 XDC1 分歧兜底（Mixed 4K RC symmetric fusion & XDC1 routing）

## 中文

### 修改内容（What changed）

- **4K RC 树接入对称融合与双向 XDC1 兜底路由**（`ManiaMapAnalyser by Leo_Black/js/estimator/mixedEstimator.js`）：
  - 新增常量 `XDC1_RC_DISCREPANCY_MIN_DELTA = 0.5`、`XDC1_RC_OVERRIDE_MIN_DELTA = 0.5` 与 `XDC1_RC_FUSION_WEIGHT = 0.5`，新增 `tryRunXdc1Fallback`、`canUseXdc1RcResult`、`hasActiveMarathonCorrection`、`isDeltaAtLeast`、`fuseSymmetricRcValues`、`buildFusedRcOutcome`、`resolveRoxyXdc1RcOutcome` 与 `resolveHighAzusaXdc1RcOutcome`。
  - **R3'（Roxy 基底分支）**：当 `mixedModeTag === "RC"`、`columnCount === 4`、R2 未触发 `Azusa` 压高估保护（`actualAlgorithm === "Roxy"`）、`!inEnabled && !hasExplicitOd` 且未命中马拉松修正（`computeMarathonCorrection <= 0`）时，关闭图表试算 XDC1：
    - 若 `XDC1` 无效（如 `< Alpha Low`）→ 保持 `"Roxy"`；
    - 若双向差值 `|xdc1Numeric − roxyNumeric| >= 0.50`（量化到 `1e-4` 规避浮点尾差）→ 切换为 `"XDC1"`（若开启图表则补算 XDC1 曲线）；
    - 若 `|xdc1Numeric − roxyNumeric| < 0.50` → 输出对称均值 `fused = 0.5 * XDC1 + 0.5 * Roxy`（保留 2 位小数），`actualEstimatorAlgorithm = "Roxy+XDC1"`，`estDiff = numericToRcLabel(fused)`，`numericDifficultyHint = null`，并保留已算出的 `Roxy` 曲线。
  - **R4' / C8'（低难 Azusa⊕Companella 融合分支）**：当 `Roxy` 越界退入 R4 且 `sunnyBaseline.star < 9`、`azusaNumeric < 11`（`RC_FUSION_LOW_BAND_MAX`）、`!hasExplicitOd` 且未命中马拉松修正时，将轻量 `xdc1Candidate` 随 `mixedCompanellaPlan` 传入主线程；`applyCompanellaToMixedResult` 计算 0.5/0.5 融合值 `fused` 后，若 `xdc1Numeric − fused >= 0.50`，切向 XDC1 并将 `actualEstimatorAlgorithm` 与 `companellaCapsule` 置为 `"XDC1"`。
  - **R4-Hi（高段 Roxy 越界分支）**：当 `Roxy` 越界退入 R4 且 `sunnyBaseline.star >= 9`、`azusaNumeric >= 11`、`!hasExplicitOd` 且未命中马拉松修正时，试算 XDC1；若有效则输出对称均值 `0.5 * XDC1 + 0.5 * Azusa`（2 位小数），`actualEstimatorAlgorithm = "XDC1+Azusa"`，`estDiff = numericToRcLabel(fused)`，`numericDifficultyHint = null`，保留 `Azusa`（Sunny）曲线；否则保持纯 `Azusa`。
- **保护边界**：R2 `shouldPreferAzusaRcResult` 命中分支、已触发马拉松时长修正的谱面、显式 OD（`odFlag`）谱面、`IN` 转谱 mod、非 4K 谱面（6K/7K/其余键数）与 LN·Mix 树（`lnRatio > 0.15` 且未开 `HO`）均维持原有路由，不受影响。
- **遥测、反馈口径与结果缓存升级**：
  - `ManiaMapAnalyser by Leo_Black/js/app/net/telemetrySpec.js` 与 `backend/internal/spec/spec.go` 对称扩展 `CAPSULE_ALIASES` / `CapsuleAliases`：`Roxy+XDC1 -> Roxy`、`XDC1+Azusa -> Azusa`（同步扩充 `backend/internal/spec/spec_test.go` 用例）。
  - `ManiaMapAnalyser by Leo_Black/js/app/core/analysis.js` 的 `rcNative` 纳入 `"Roxy+XDC1"` 与 `"XDC1+Azusa"`，使遥测直接上报融合后数值；`CACHE_KEY_STAR_UNIFIED_VERSION` 由 `"star-v14"` 升至 `"star-v15"`，使受影响 4K RC 谱面的旧快照自动失效。

### 修改原因（Why）

- 基于 `VSRG-DanEstimation-Benchmark` 非 LN 样本（n = 1,262）拆解：在 `|XDC1 − Roxy| < 0.50` 主区间内（占共同有效图的 96.9%），物理应变模型 `XDC1` 与 GBDT 元模型 `Roxy` 误差方向高度互补，取自然对称权重 0.5/0.5 无需数据集拟合即可同时压低 MAE 与方差；当双向分歧达到 `>= 0.50` 时 `XDC1` 在低估（`>= +0.50`）与高估（`<= -0.50`）两侧均显著胜出 `Roxy`；高段 `Roxy` 越界（`17.0+ Eta–Theta`）后 `XDC1` 与 `Azusa` 亦存在反向偏差互补，0.5/0.5 融合可进一步提升超高段精度与稳定性。
- 全量实跑对比（1,364 张谱面，`main` 基线 → 本分支）：
  - **ALL (n = 1,364)**：MAE `0.3123 → 0.2838`（-0.0285）、RMSE `0.5313 → 0.5074`、Exact `757 (55.50%) → 831 (60.92%)`（+74 张）、Miss `80 → 69`、Trend Fit R² `96.6284% → 96.9252%`。
  - **RC (n = 1,262)**：MAE `0.2383 → 0.2075`（-0.0308）、RMSE `0.3436 → 0.3020`、Bias `+0.0135 → -0.0062`、P90 绝对误差 `0.530 → 0.460`、最大低估 `1.720 → 1.530`、Exact `747 (59.19%) → 821 (65.06%)`（+74 张）、Miss `27 → 16`（-40.7%）、Trend Fit R² `98.2403% → 98.6407%`；其中 1,228 张单图 RC 的 MAE 从 `0.2318` 降至 **`0.1999`**（Exact `59.93% → 66.29%`）。
  - **LN (n = 102)**：0 行变动（MAE `1.2277` 逐位不变）。

### 影响范围（Scope）

- 仅影响选择 `Mixed` 估算器时的 **4K RC** 谱面（`lnRatio <= 0.15` 或启用 `HO` mod）；6K/7K/其余键数、LN/Mix 谱面、独立选择其他估算器与插件对外版本号均不受影响。
- `actualEstimatorAlgorithm` 新增可能值 `"Roxy+XDC1"`、`"XDC1"` 与 `"XDC1+Azusa"`（前两者在遥测与反馈端自动归一化为 `"Roxy"` 与 `"Azusa"`，`star` 仍由分析管线统一归一化为 Sunny 原始 SR）。

### 兼容策略（Compat）

- `mixedCompanellaPlan.xdc1Candidate` 仅携带纯 JSON 字段，保持 Web Worker 结构化克隆与结果缓存深拷贝契约。
- XDC1 返回 `unsupported: true`（如 `< Alpha Low`）时静默跳过，保持原 `Roxy` / `Azusa` / `Companella` 行为。
- 遥测与难度反馈沿用既有白名单映射（`CAPSULE_ALIASES` / `CapsuleAliases`），不新增存储维度。

### 验证方式（Verification）

- 全量官方 Benchmark 实跑（1,364 张谱面全部成功，0 Invalid / 0 Missing / 0 Failed；LN 子集 102 张零差异）。
- 合成谱回归验证覆盖 `Roxy+XDC1` 对称融合、`R3'` 双向 `>= 0.50` 切换 `XDC1`（含 XDC1 图表输出）、`R4-Hi` `XDC1+Azusa` 融合、C8' IEEE-754 边界 `11.15 - 10.65 = 0.50`、显式 OD / `IN` / 马拉松修正保护、LN 与非 4K（6K/7K/5K）隔离、遥测别名归一化与 `star-v15` 缓存版本（全部通过）；后端 Go 测试（`feedback` / `spec` / `store`）与转换器冒烟测试（108/108）均全部通过。

---

## English

### What changed

- **Symmetric fusion and two-way XDC1 fallback routing in the 4K RC tree** (`ManiaMapAnalyser by Leo_Black/js/estimator/mixedEstimator.js`):
  - Added constants `XDC1_RC_DISCREPANCY_MIN_DELTA = 0.5`, `XDC1_RC_OVERRIDE_MIN_DELTA = 0.5`, and `XDC1_RC_FUSION_WEIGHT = 0.5`, plus helpers `tryRunXdc1Fallback`, `canUseXdc1RcResult`, `hasActiveMarathonCorrection`, `isDeltaAtLeast`, `fuseSymmetricRcValues`, `buildFusedRcOutcome`, `resolveRoxyXdc1RcOutcome`, and `resolveHighAzusaXdc1RcOutcome`.
  - **R3' (Roxy winner path)**: When `mixedModeTag === "RC"`, `columnCount === 4`, R2 did not switch to `Azusa` (`actualAlgorithm === "Roxy"`), `!inEnabled && !hasExplicitOd`, and `computeMarathonCorrection <= 0`, XDC1 is previewed without computing its graph:
    - If XDC1 is unsupported (`< Alpha Low`), keep `"Roxy"`.
    - If bidirectional discrepancy `|xdc1Numeric - roxyNumeric| >= 0.50` (quantized to `1e-4`), switch to `"XDC1"` (recomputing XDC1's graph only when requested).
    - Otherwise (`|xdc1Numeric - roxyNumeric| < 0.50`), emit the symmetric 0.5/0.5 average (rounded to 2 decimals) as `actualEstimatorAlgorithm = "Roxy+XDC1"`, `estDiff = numericToRcLabel(fused)`, `numericDifficultyHint = null`, retaining `Roxy`'s graph.
  - **R4' / C8' (low-band Azusa⊕Companella path)**: When Roxy scopes out into R4' with `sunnyBaseline.star < 9`, `azusaNumeric < 11`, `!hasExplicitOd`, and no active marathon correction, a lightweight `xdc1Candidate` rides on `mixedCompanellaPlan`; if `xdc1Numeric - fused >= 0.50` after `applyCompanellaToMixedResult`, switch to `"XDC1"` (`companellaCapsule = "XDC1"`).
  - **R4-Hi (high-band Roxy out-of-scope path)**: When Roxy scopes out into R4 with `sunnyBaseline.star >= 9`, `azusaNumeric >= 11`, `!hasExplicitOd`, and no active marathon correction, if XDC1 is valid, emit the symmetric 0.5/0.5 average of `XDC1` and `Azusa` as `actualEstimatorAlgorithm = "XDC1+Azusa"`, `estDiff = numericToRcLabel(fused)`, `numericDifficultyHint = null`, keeping `Azusa`'s Sunny graph.
- **Guards**: R2 `shouldPreferAzusaRcResult` overrides, active marathon duration correction charts, explicit OD (`odFlag`), `IN` convert mod, non-4K charts, and the LN/Mix tree (`lnRatio > 0.15` without `HO`) remain untouched.
- **Telemetry, feedback, and cache version alignment**:
  - Updated `CAPSULE_ALIASES` / `CapsuleAliases` in both `ManiaMapAnalyser by Leo_Black/js/app/net/telemetrySpec.js` and `backend/internal/spec/spec.go` (`Roxy+XDC1 -> Roxy`, `XDC1+Azusa -> Azusa`), plus unit assertions in `backend/internal/spec/spec_test.go`.
  - Added `"Roxy+XDC1"` and `"XDC1+Azusa"` to `rcNative` in `ManiaMapAnalyser by Leo_Black/js/app/core/analysis.js`, and bumped `CACHE_KEY_STAR_UNIFIED_VERSION` from `"star-v14"` to `"star-v15"`.

### Why

- Error decomposition across non-LN benchmark charts (`n = 1,262`) showed that within `|XDC1 - Roxy| < 0.50` (96.9% of shared valid charts), physical-strain `XDC1` and GBDT meta-model `Roxy` have opposite-sign biases and cancel cleanly under parameter-free 0.5/0.5 averaging; when disagreement reaches `>= 0.50` in either direction, `XDC1` strongly outperforms `Roxy` on both underrating (`>= +0.50`) and overrating (`<= -0.50`) tails; above Roxy's Zeta ceiling (`17.0+`), `XDC1` and `Azusa` likewise cancel opposite biases via 0.5/0.5 fusion.
- Full benchmark run (`1,364` charts, `main` baseline -> branch):
  - **ALL (`1,364`)**: MAE `0.3123 -> 0.2838` (-0.0285), RMSE `0.5313 -> 0.5074`, Exact `757 (55.50%) -> 831 (60.92%)` (+74 maps), Miss `80 -> 69`, Trend Fit R² `96.6284% -> 96.9252%`.
  - **RC (`1,262`)**: MAE `0.2383 -> 0.2075` (-0.0308), RMSE `0.3436 -> 0.3020`, Bias `+0.0135 -> -0.0062`, P90 abs error `0.530 -> 0.460`, max underrate `1.720 -> 1.530`, Exact `747 (59.19%) -> 821 (65.06%)` (+74 maps), Miss `27 -> 16` (-40.7%), Trend Fit R² `98.2403% -> 98.6407%`; single-chart RC (`1,228`) drops to MAE **`0.1999`** (Exact `59.93% -> 66.29%`).
  - **LN (`102`)**: 0 diffs (`MAE 1.2277` unchanged).

### Scope

- Affects only **4K RC** charts under the `Mixed` estimator (`lnRatio <= 0.15` or `HO` mod). Non-4K charts, LN/Mix charts, standalone estimators, and plugin version numbers are unchanged.
- `actualEstimatorAlgorithm` may now report `"Roxy+XDC1"`, `"XDC1"`, or `"XDC1+Azusa"` (the composite capsules normalize to `"Roxy"` and `"Azusa"` for telemetry/feedback; displayed `star` stays normalized to Sunny raw SR).

### Compatibility

- `mixedCompanellaPlan.xdc1Candidate` holds plain JSON fields only, preserving Web Worker structured-clone and result-cache invariants.
- Unsupported XDC1 results (`< Alpha Low`) fall through cleanly with no error.
- Composite capsules map cleanly onto existing backend `AllowedActualAlgorithms` via `CapsuleAliases`.

### Verification

- Full benchmark run (`1,364/1,364` OK, `0` invalid/missing/failed; `0` diffs across all `102` LN rows).
- Synthetic verification tests covering `Roxy+XDC1` fusion, bidirectional `>= 0.50` `XDC1` switching (with XDC1 graph), `R4-Hi` `XDC1+Azusa` fusion, C8' IEEE-754 boundary (`11.15 - 10.65 = 0.50`), explicit OD / `IN` / marathon guards, LN and 6K/7K/5K isolation, telemetry aliases, and `star-v15` cache version all pass; backend Go tests (`feedback`, `spec`, `store`) and converter smoke tests (`108/108`) pass.
