# Mixed 路由技术文档（mixed-routing.md）

> 目标读者：AI。本文说明 `Mixed` 估算器**如何为一张谱面选择子算法**、`Companella` 在什么条件下参与、以及前端算法胶囊（`state.actualEstimatorAlgorithm`）在各条路径上显示什么。
> 所有引用为 `path:line + symbol` 形式；行号为编写时快照，定位请以符号名为准。
> 相关文档：[difficulty-estimation.md](difficulty-estimation.md)（估算器总览与分派）、[../pipeline/worker.md](../pipeline/worker.md)（管线与归一化）、[../pipeline/result-cache.md](../pipeline/result-cache.md)（缓存与 `actualEstimatorAlgorithm` 的落盘语义）。

## 1. 入口与前置

`Mixed` 的唯一入口是 `js/estimator/mixedEstimator.js runMixedEstimatorFromText(osuText, options, parsed)`。它先取 Sunny 基线（`sunnyBaseline = options.precomputedSunnyResult || runSunnyEstimatorFromText(...)`，pipeline 在 `NORMALIZATION_ALGORITHMS` 命中时已算好并复用，故只跑一次 Sunny）。

- **键数门**：`MIXED_SUPPORTED_KEYS = new Set([4, 6, 7])`。不在集合内（5K/8K/9K/10K/12K/18K…）直接返回 Sunny 基线，`actualEstimatorAlgorithm = "Sunny"`。
- **cvtFlag 解析**：`parseCvtFlags(options.cvtFlag)` → `{ inEnabled, hoEnabled }`。`HO`（去长条）会把模式强制按 `RC` 处理（`mixedModeTag = hoEnabled ? "RC" : modeTagFromLnRatio(lnRatio)`）。`IN`（转长条）不强制模式，但会禁用 Roxy→Azusa 的偏好覆盖（见 §3）。
- **模式判定**：`js/patterns/config.js modeTagFromLnRatio(lnRatio)`：`lnRatio ≤ 0.15` → `RC`；`≥ 0.90` → `LN`；中间 → `Mix`。`RC` 与 `LN/Mix` 走两条完全不同的分支。

## 2. RC 分支（`mixedModeTag === "RC"`）

RC 分支**只在 4K 上生效**：`columnCount !== 4` 时直接返回 Sunny 基线（`mixedEstimator.js`，RC 且非 4K 的早退）。4K 的选择顺序是 Roxy → Azusa（偏好覆盖）→ Azusa（兜底）→ Daniel（再兜底），并在低难时挂一个 Companella 融合计划。

1. **Roxy 优先**：`tryRunRoxyFallback(...)`，可用性由 `canUseRcResult` 判定——要求 `columnCount === 4`、`estDiff` 非空且不以 `Invalid` 开头、`numericDifficulty` 非 `null`（Roxy 的 scope 边界 `< Alpha Low` / `> Emik Zeta high` 返回 null → 视为不可用）。可用则 `actualEstimatorAlgorithm = "Roxy"`。
2. **Azusa 偏好覆盖**：仅当 `!inEnabled && !hasExplicitOd && shouldEvaluateAzusaRcPreference(roxyResult)` 时才额外跑一次 Azusa（`forceSunnyReferenceHo: false`，复用 `precomputedSunnyResult`）；若 `shouldPreferAzusaRcResult(roxyResult, azusaResult)` 成立则改用 Azusa。两函数都基于 `AZUSA_RC_PREFERENCE` 常量与三个语义：
   - **均衡手候选**：Roxy 的 `debug.handBias` 很小（手很均衡）且 Azusa 数值明显更高 → Azusa 更可信；
   - **锚点重候选**：Roxy 的 `debug.anchorRate` 高（锚点密集）且 Azusa 数值更低 → 用 Azusa 压低；
   - **跨界规则**：Roxy 数值已 ≥ 11（Alpha 上界）而 Azusa 仍 < 11 —— 结构难但段位低的图 Roxy 系统性高估，改判 Azusa。
3. **Roxy 不可用**：仅当 `!inEnabled`（IN 转换下 Azusa 语义不适用）时才跑 Azusa；可用则 `actualEstimatorAlgorithm = "Azusa"`，并在 `sunnyBaseline.star < LOW_BAND_COMPANELLA_STAR_MAX(9)` 时构造**低难融合计划**（`buildLowBandCompanellaPlan(..., "azusa")`，见 §4）。Azusa 也不可用时跑 Daniel，条件为 4K 且 `!isDanielTooLowDifficulty(danielResult.estDiff)`（即 `estDiff` 不以 `< Alpha…` 开头）→ `actualEstimatorAlgorithm = "Daniel"`。
4. **标签合成**：`estDiff = composeDifficultyFromRcLn(rcDifficulty, lnDifficulty, lnRatio)`。因为此分支 `lnRatio ≤ 0.15`，`composeDifficultyFromRcLn` 只返回 RC 段——**RC 图永远不显示 LN 段**。

## 3. LN / Mix 分支（`mixedModeTag` 为 `LN` 或 `Mix`）

RC 半沿用 Sunny 基线的 RC 段（`sunnyParts.rc`），只有 4K 且 `star < 9` 时才用 Azusa 的 RC 半：

| 条件 | RC 半来源 | `actualEstimatorAlgorithm` | Companella 计划 |
|---|---|---|---|
| 非 4K，或 `star ≥ 9` | Sunny | `"Sunny"` | 无 |
| 4K 且 `star < 9` 且 Azusa 可用 | Azusa | `"Azusa"` | `buildLowBandCompanellaPlan(..., "companella")` |
| 4K 且 `star < 9` 且 Azusa 不可用 | Sunny | `"Companella"` | `{ lnRatio, lnDifficulty }`（不融合 RC，直接采用 Companella 结果） |

标签同样是 `RC || LN`（`lnRatio ≥ 0.15` 时 LN 段才出现）。

**结论（常见误解）**：**LN 主体或 Mix 图默认不会走 Roxy/Daniel，也不一定走 Companella**——只有当它是 4K、Sunny 星数 < 9，并且 Azusa 可用时才会挂上 Companella 计划；星数 ≥ 9 或非 4K 的谱面胶囊显示 `[Sunny]` 是**设计行为**，不是 Companella 失效。

## 4. Companella 的异步应用（主线程，`js/app/analysis.js`）

`Mixed` 只产出**计划**（`mixedCompanellaPlan`），真正的推理在主线程：

1. **是否执行**：`shouldRunCompanella = Number(rework.columnCount) === 4 && (pendingCompanellaEstimate || pendingMixedCompanellaContext != null)`（`analysis.js`）。`pendingCompanellaEstimate` 表示用户**直接选择了 Companella**（pipeline 用 Sunny 打底，主线程补算）。
2. **高 LN 跳过（关键门）**：`lnRatio > 0.18` 时**直接跳过**，并把 `pendingCompanellaEstimate/Context` 清空；若此时胶囊是 `"Companella"` 则改回 `"Sunny"`（`analysis.js`，注释明确"Companella 是 RC 模型，高 LN 谱面不适用，与 Azusa/Roxy 同门控"）。**这就是 vibro / LN 包在 Mixed 下几乎总是显示 `[Sunny]` 的直接原因**：这类谱面 `lnRatio` 常在 18% 以上。
3. **输入**：`classifyCompanellaDifficulty({ msdValues, interludeStar, sunnyStar: rework.star })`。MSD 取值优先用管道里已算好的 `ettResult.values`；当 `state.companellaEtternaVersion !== state.etternaVersion` 时用 pipeline 的二次 Ett 结果（`companellaEttResult`），缺失则主线程补算，失败只 `console.warn` 并回退主 Ett 版本。
4. **结果落点**：
   - 直接选择 Companella（`pendingCompanellaEstimate`）→ 覆盖 `resolvedEstDiff/numeric/hint`；失败则把胶囊改回 `"Sunny"`（保持 star/estDiff 可用，不产生 No data）。
   - 融合计划（`pendingMixedCompanellaContext`）→ `applyCompanellaToMixedResult({estDiff, numeric, hint, plan}, companellaResult)`：
     - `plan.fuseRc === true`（RC 分支的低难计划）且 `rcNumeric ≥ RC_FUSION_LOW_BAND_MAX(11)`：`onDisagree === "companella"` 时采用 Companella 的 RC 半，否则**保持 Azusa 结果不变**；
     - `plan.fuseRc === true` 且 `rcNumeric < 11`：RC 数值按 `RC_AZUSA_COMPANELLA_FUSION_WEIGHT(0.5)` 与 Companella 取平均，再由 `numericToRcLabel(fused)` 重新派生标签；
     - 无 `fuseRc`（4K/star<9/Azusa 不可用路径）：直接采用 Companella 的 RC 半与数值；
     - 三种情况都用 `composeDifficultyFromRcLn(newRcLabel, plan.lnDifficulty, plan.lnRatio)` 重新拼标签——**LN 半始终来自计划里保存的原值**。

## 5. 算法胶囊（`state.actualEstimatorAlgorithm`）语义

胶囊由 `js/app/graph.js` 渲染为 `[算法名]`，取值规则：

| 场景 | 胶囊显示 |
|---|---|
| 键数不在 {4,6,7}，或 RC 且非 4K | `Sunny` |
| RC 分支：Roxy 胜 | `Roxy` |
| RC 分支：Azusa 胜（偏好覆盖或兜底） | `Azusa` |
| RC 分支：Daniel 兜底 | `Daniel` |
| LN/Mix 分支：`star ≥ 9` 或非 4K | `Sunny` |
| LN/Mix 分支：Azusa 可用 | `Azusa` |
| LN/Mix 分支：Azusa 不可用 | `Companella` |
| 用户直接选择 Companella（4K 且 `lnRatio ≤ 0.18`） | `Companella` |
| 用户直接选择 Companella 但 `lnRatio > 0.18` | `Sunny`（被高 LN 门改回） |

**已知问题（待决策）**：Companella 通过**融合**参与时（§4 的 `fuseRc` 路径或直接采用路径），`state.actualEstimatorAlgorithm` **不会被更新**——胶囊仍显示融合前的赢家（`Azusa`/`Roxy`）。也就是说"这张图的数值里含 Companella 的贡献"与"胶囊写着 Azusa"可以同时成立。这属于显示语义问题，不影响 `estDiff`/`numericDifficulty` 的正确性；是否改为显示融合后的真实来源（例如 `Azusa+Companella`，或让 Companella 在采用路径上接管胶囊）需要产品决策。

## 6. 与其它机制的交互

- **SunnyWindow**（`forceSunnyWindow` 开启）：在 pipeline 之后由 `analysis.js` 用 SunnyWindow 的 LN 段覆盖最终标签的 LN 半，并把新的 LN 难度写回 `pendingMixedCompanellaContext.lnDifficulty`（同时把 `lnRatio` 置 4e65 以强制显示 LN 段）——因此 Companionella 融合仍会使用被覆盖后的 LN 半。
- **马拉松时长修正**：由 Azusa/Roxy 估算器本体消费 `options.marathonCorrection`（pipeline 在 `>300s 且 4K 且算法 ∈ {Azusa, Roxy, Mixed}` 时前置一次 Ett）。修正发生在 Mixed 路由**之前**，因此可能改变 Mixed 的路由结果（详见 [marathon-correction.md](marathon-correction.md)）。
- **aleju03 低段 LN 接管**（分支 `feat/aleju03-ln-estimator`）：当 Sunny 的 `estDiff` 含 LN 半且该半以 `<` 开头（`intervalLookup` 的表下限，如 `< LN 5 mid`）时，LN 半改用 aleju03 的判决；`plan.lnDifficulty` 随之被替换，Companella 融合仍按新值拼接。该改动不触及 RC 分支与胶囊语义。
- **结果缓存**：`actualEstimatorAlgorithm` 与 `isVibroMap` 等一起写入快照，命中时原样恢复（`analysis.js` 写门 + `resultCache.js`）；缓存键第一段是 `state.estimatorAlgorithm`（用户选择），因此切换算法必然 miss。

## 7. 速查：为什么我这张图没有用 Companella

1. 键数不是 4（Roxy/Azusa/Companella 全是 4K 模型）→ `[Sunny]`。
2. `lnRatio > 0.18`（LN 主体或大片 LN 的 Mix 图）→ 主线程**主动跳过** Companella。
3. `lnRatio > 0.18` 之外的 LN/Mix 图但 Sunny `star ≥ 9` → 不构造 Companella 计划。
4. RC 图但 Roxy 可用且未触发 Azusa 偏好覆盖 → 走 Roxy，Companella 只在 `star < 9` 的低难计划里出现。
5. 计划确实存在且执行成功 → 数值里有 Companella 贡献，但**胶囊可能仍显示 `Azusa`/`Roxy`**（§5 已知问题，容易被误判为"没生效"）。

## 8. 代码索引

| 符号 | 位置 | 作用 |
| --- | --- | --- |
| `runMixedEstimatorFromText` | `js/estimator/mixedEstimator.js` | 路由主入口 |
| `MIXED_SUPPORTED_KEYS` | `mixedEstimator.js` | 键数门（4/6/7） |
| `modeTagFromLnRatio` | `js/patterns/config.js` | 0.15 / 0.90 模式阈值 |
| `canUseRcResult` | `mixedEstimator.js` | RC 结果可用性（4K + 非 Invalid + numeric 非 null） |
| `shouldEvaluateAzusaRcPreference` / `shouldPreferAzusaRcResult` | `mixedEstimator.js` | Roxy→Azusa 偏好覆盖三条规则 |
| `buildLowBandCompanellaPlan` | `mixedEstimator.js` | 低难融合计划（`onDisagree` = `azusa` / `companella`） |
| `composeDifficultyFromRcLn` | `mixedEstimator.js` | `RC \|\| LN` 标签合成（`lnRatio < 0.15` 只出 RC） |
| `applyCompanellaToMixedResult` | `mixedEstimator.js` | 融合/采用/回落三种处理 |
| `LOW_BAND_COMPANELLA_STAR_MAX`(9) / `RC_FUSION_LOW_BAND_MAX`(11) / `RC_AZUSA_COMPANELLA_FUSION_WEIGHT`(0.5) | `mixedEstimator.js` | 融合阈值与权重 |
| `shouldRunCompanella` / `companellaLnRatio > 0.18` | `js/app/analysis.js` | 主线程执行门与高 LN 跳过 |
| `state.actualEstimatorAlgorithm` | `js/app/appContext.js` | 胶囊取值来源（命中缓存时由快照恢复） |
