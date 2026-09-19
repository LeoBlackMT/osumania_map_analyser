# Mixed 路由技术文档（mixed-routing.md）

> 目标读者：AI。本文说明 `Mixed` 估算器**对每张谱面究竟路由到哪个算法**：完整的判定顺序、每个门的精确条件与常量、失败后的回退链、标签与"算法胶囊"的最终取值，以及主线程 `Companella` 阶段的参与方式。
> 所有引用为 `path:symbol` 形式（行号会漂移，定位请以符号名为准）。
> 相关文档：[difficulty-estimation.md](difficulty-estimation.md)（估算器总览）、[../pipeline/worker.md](../pipeline/worker.md)（管线）、[../pipeline/result-cache.md](../pipeline/result-cache.md)（缓存）。

---

## 1. 一句话结论

`Mixed` 先用**模式**把谱面分成两棵树（`RC` 树 / `LN·Mix` 树），在树内按固定优先级尝试子算法（RC 树：Roxy → Azusa（偏好覆盖）→ Azusa（兜底）→ Daniel；LN·Mix 树：Azusa → Daniel），产出一个 **RC 半 + LN 半**的复合标签和一个"算法胶囊"；`Companella` 不在树内，而是由 `Mixed` 留一张**计划**，交给主线程在 Etterna MSD 就绪后异步补算并融合。

---

## 2. 前置门（任何路由之前）

`js/estimator/mixedEstimator.js runMixedEstimatorFromText(osuText, options, parsed)`

| 序 | 判定 | 常量 / 表达式 | 命中后的结果 |
| --- | --- | --- | --- |
| P1 | Sunny 基线 | `options.precomputedSunnyResult \|\| runSunnyEstimatorFromText(...)`（pipeline 命中 `NORMALIZATION_ALGORITHMS` 时复用，不重复计算） | 后续一切以它为参照 |
| P2 | **键数门** | `MIXED_SUPPORTED_KEYS = new Set([4, 6, 7])`；`!Number.isFinite(columnCount) \|\| !MIXED_SUPPORTED_KEYS.has(columnCount)` | **直接返回 Sunny 基线**，`actualEstimatorAlgorithm = "Sunny"`，`mixedCompanellaPlan = null`（5K/8K/9K/10K/18K… 到此结束） |
| P3 | cvtFlag | `parseCvtFlags(options.cvtFlag)` → `{ inEnabled, hoEnabled }`；`hasExplicitOd = options.odFlag != null` | 影响 P4 与 RC 树的 Azusa 偏好覆盖 |
| P4 | **模式判定** | `mixedModeTag = hoEnabled ? "RC" : modeTagFromLnRatio(lnRatio)`；`modeTagFromLnRatio`：`lnRatio ≤ 0.15 → "RC"`、`≥ 0.90 → "LN"`、其余（含 NaN）`→ "Mix"`（`js/patterns/config.js`） | 决定进入第 3 节还是第 4 节 |
| P5 | 标签合成 | `composeDifficultyFromRcLn(rc, ln, lnRatio)`：当 `lnRatio < 0.15` **只返回 RC 段**；否则返回 `"RC \|\| LN"` | RC 图的 LN 半永远不会出现在 `estDiff` 里 |

`HO`（去长条）会**强制走 RC 树**——这是"同一张 LN 图在开 HO 后换算法"的唯一原因。

---

## 3. RC 树（`mixedModeTag === "RC"`，含 `HO` 强制）

**进入本树前还有一个早退**：`mixedModeTag === "RC" && columnCount !== 4` → 直接返回 Sunny 基线（6K/7K 的 RC 图不用 Roxy/Azusa/Daniel）。

### 3.1 判定顺序（4K RC）

| 序 | 条件 | 命中算法 | 说明 |
| --- | --- | --- | --- |
| R1 | `canUseRcResult(roxy)` 为真 | **Roxy** | `canUseRcResult` 要求：`columnCount === 4`、`estDiff` 非空且不以 `Invalid` 开头、`numericDifficulty` 非 `null`（Roxy 的 scope 边界 `< Alpha Low` / `> Emik Zeta high` 返回 null → 视为不可用） |
| R2 | R1 命中 **且** `!inEnabled && !hasExplicitOd && shouldEvaluateAzusaRcPreference(roxy)` 为真，**且** `shouldPreferAzusaRcResult(roxy, azusa)` 为真 | **Azusa**（覆盖 Roxy） | 先"筛选"再"确认"，两段各有独立阈值，见 3.2 |
| R3 | R1 命中但 R2 不成立 | **Roxy**（保持） | 注意：**Roxy 胜出时不产生 Companella 计划**（RC 树的融合计划只在 R4' 产生） |
| R4 | R1 不成立 **且** `inEnabled === false` **且** `canUseRcResult(azusa)` 为真 | **Azusa** | Azusa 以 `forceSunnyReferenceHo:false` + `precomputedSunnyResult` 运行 |
| R4' | R4 命中 **且** `Number(sunnyBaseline.star) < LOW_BAND_COMPANELLA_STAR_MAX (9)` | Azusa + **Companella 计划** | `buildLowBandCompanellaPlan(azusaResult, lnRatio, lnDifficulty, "azusa")`：`fuseRc = true`、`onDisagree = "azusa"`、基准 `rcNumeric` = Azusa 的数值 |
| R5 | R1 不成立 **且** `inEnabled === false` **且** Azusa 不可用 **且** Daniel 可用 | **Daniel** | 可用性：`columnCount === 4 && !isDanielTooLowDifficulty(daniel.estDiff)`（`isDanielTooLowDifficulty` = `/^<\s*alpha\b/i`，即 Daniel 自称低于 Alpha 时不用） |
| R6 | R1 不成立 **且** `inEnabled === true` | **Sunny 基线** | IN 转换下 Azusa 语义不适用，因此跳过 Azusa/Daniel 整条链 |
| R7 | 以上都不成立 | **Sunny 基线** | 例如 Roxy 不可用 + Azusa 不可用 + Daniel 过低 |

RC 树的 `estDiff` / `numericDifficulty` **直接取获胜子算法的自身输出**（不做 RC/LN 拼接），因此 RC 图的标签形态取决于获胜算法（Roxy/Azusa 的段位标签、Daniel 的 Alpha+ 标签等）。

### 3.2 Azusa 偏好覆盖的两段门（R2 的细节）

`roxyNumeric` = Roxy 的 `debug.finalNumeric`（未量化值，避免 2 位小数舍入抖动），缺失时退回 `numericDifficulty`；`azusaReference` = Roxy `debug.meta.references.Azusa`；`handBias` / `anchorRate` 来自 Roxy `debug.stats`。

**筛选（`shouldEvaluateAzusaRcPreference`，先决定"要不要跑一次 Azusa"）** —— `delta = azusaReference − roxyNumeric`：

| 语义 | 条件（任一成立） |
| --- | --- |
| 均衡手候选 | `handBias ≤ 0.006` 且 `delta ≥ 0.25` |
| 锚点重候选 | `anchorRate ≥ 0.72` 且 `delta ≤ −0.55` |
| 跨界候选 | `roxyNumeric ≥ 11` 且 `azusaReference < 11` |

**确认（`shouldPreferAzusaRcResult`，决定"跑完要不要换"）** —— `delta = azusaNumeric − roxyNumeric`：

| 语义 | 条件（任一成立） |
| --- | --- |
| 均衡手抬升 | `handBias ≤ 0.003` 且 `delta ≥ 0.4` |
| 锚点重压低 | `anchorRate ≥ 0.78` 且 `delta ≤ −0.7` |
| 跨界抬升 | `roxyNumeric ≥ 11` 且 `azusaNumeric < 11` |

常量集中在 `AZUSA_RC_PREFERENCE`（`balancedHandScreenMaxBias 0.006` / `balancedHandMaxBias 0.003` / `azusaHigherScreenMinDelta 0.25` / `azusaHigherMinDelta 0.4` / `anchorHeavyScreenMinRate 0.72` / `anchorHeavyMinRate 0.78` / `azusaLowerScreenMaxDelta −0.55` / `azusaLowerMaxDelta −0.7`）。筛选门比确认门宽，作用是"少跑一次 Azusa"。

---

## 4. LN·Mix 树（`mixedModeTag` 为 `"LN"` 或 `"Mix"`）

RC 半默认来自 Sunny 基线，**只有 4K 才会考虑 Azusa/Daniel**：

| 序 | 条件（按顺序） | RC 半来源 | LN 半来源 | `actualEstimatorAlgorithm` | Companella 计划 |
| --- | --- | --- | --- | --- | --- |
| L1 | `columnCount === 4` 且 `star < 9` 且 `canUseRcResult(azusa)` | **Azusa** | Sunny 的 LN 半（`LN·Mix` 树统一由 aleju03 接管，见 §8；aleju03 失效时回退该表值） | `"Azusa"` | `buildLowBandCompanellaPlan(azusa, lnRatio, ln, "companella")` → `fuseRc = true`、`onDisagree = "companella"` |
| L2 | `columnCount === 4` 且 `star < 9` 且 Azusa **不可用** | （交给 Companella） | Sunny 的 LN 半（`LN·Mix` 树统一由 aleju03 接管，见 §8；aleju03 失效时回退该表值） | `"Companella"` | `{ lnRatio, lnDifficulty }`（**无 `fuseRc`** → 主线程直接采用 Companella 的 RC 半与数值） |
| L3 | `columnCount === 4` 且 `star ≥ 9` 且 Daniel 可用 | **Daniel** | Sunny 的 LN 半（`LN·Mix` 树统一由 aleju03 接管，见 §8；aleju03 失效时回退该表值） | `"Daniel"` | 无 |
| L4 | `columnCount === 4` 且 `star ≥ 9` 且 Daniel 不可用（或过低） | Sunny | Sunny 的 LN 半（`LN·Mix` 树统一由 aleju03 接管，见 §8；aleju03 失效时回退该表值） | `"Sunny"` | 无 |
| L5 | `columnCount !== 4`（6K/7K） | Sunny | Sunny 的 LN 半（`LN·Mix` 树统一由 aleju03 接管，见 §8；aleju03 失效时回退该表值） | `"Sunny"` | 无 |

- Azusa 在 L1/L2 的调用参数：`forceSunnyReferenceHo: false` + `precomputedSunnyResult: sunnyBaseline`（星数口径与缓存复用）。
- 最终标签 `estDiff = composeDifficultyFromRcLn(rc, ln, lnRatio)`：`lnRatio ≥ 0.15` 才有 `"RC || LN"`，否则只有 RC 段（`LN` 树必然 ≥ 0.90，`Mix` 树在 0.15–0.90 之间，故两者都会显示 LN 半）。
- `numericDifficulty` / `numericDifficultyHint` 恒取 **RC 半**的数值（LN 估计不产出数值——与"数值难度只覆盖 RC 算法"的既有约定一致）。
- `lnRatio` 会按 HO 归零：`hoEnabled ? 0 : 归一化后的 lnRatio`。

---

## 5. 主线程 Companella 阶段（`js/app/analysis.js`）

`Mixed` 只留计划；推理在主线程、且依赖 Etterna MSD 与 Interlude 星数。

| 序 | 判定 | 条件 | 结果 |
| --- | --- | --- | --- |
| C1 | 是否需要跑 | `shouldRunCompanella = Number(rework.columnCount) === 4 && (pendingCompanellaEstimate \|\| pendingMixedCompanellaContext != null)` | `pendingCompanellaEstimate` 来自"用户直接选择 Companella"；`pendingMixedCompanellaContext` 来自 L1/R4' 的计划 |
| C2 | **高 LN 跳过** | `companellaLnRatio = Number(rework.lnRatio ?? parsedInfo.lnRatio) > 0.18` | 清空两个 pending；若胶囊当前是 `"Companella"` 则改回 `"Sunny"`。**这是 LN 主体图（含 vibro 包）在 Mixed 下显示 `[Sunny]` 的直接原因** |
| C3 | 输入 MSD | 默认用管道的 `ettResult.values`；`state.companellaEtternaVersion !== state.etternaVersion` 时用 `pipelineResult.companellaEttResult`（缺省则主线程补算，失败仅 `console.warn` 并回退主版本） | 传给 `classifyCompanellaDifficulty({ msdValues, interludeStar, sunnyStar: rework.star })` |
| C4 | 直接选择（`pendingCompanellaEstimate`） | 推理成功 | `resolvedEstDiff/numeric/hint` 全部换成 Companella 的结果（胶囊在管道里就已是 `"Companella"`） |
| C5 | 直接选择但推理失败 | `catch` | 胶囊改回 `"Sunny"`，保留已算出的 star/estDiff（不产生 No data） |
| C6 | 融合计划 + `plan.fuseRc` 且 `rcNumeric ≥ RC_FUSION_LOW_BAND_MAX (11)` | `onDisagree === "companella"` | 采用 Companella 的 RC 半：`estDiff = compose(companella.estDiff, plan.lnDifficulty, plan.lnRatio)`，数值随之 |
| C7 | 同上但 `onDisagree === "azusa"` | — | **保持 Mixed 原结果不变**（Azusa 胜出；Companella 不改变结论） |
| C8 | `plan.fuseRc` 且 `rcNumeric < 11` | — | RC 数值 = `rcNumeric × 0.5 + companellaNumeric × 0.5`（`RC_AZUSA_COMPANELLA_FUSION_WEIGHT = 0.5`），标签由 `numericToRcLabel(fused)` 重新派生，LN 半仍用 `plan.lnDifficulty` |
| C9 | 计划**无** `fuseRc`（L2 路径） | — | 直接采用 Companella 的 `estDiff` / 数值，并与 `plan.lnDifficulty` 拼接 |
| C10 | 胶囊 | — | **C6/C8/C9 均不更新 `state.actualEstimatorAlgorithm`**（已知显示问题，见第 7 节） |

融合门控的历史：曾有一条"`|Azusa − Companella| ≤ 1.0` 一致性门"，实测会误杀大量有益融合（净收益 −4.89 → −2.87 MAE 点），已移除（见 `mixedEstimator.js` 注释）。

---

## 6. 胶囊（`state.actualEstimatorAlgorithm`）取值总表

渲染位置：`js/app/graph.js formatEstimateDifficultyCaption()`（当前只在 `actualAlgorithm !== selectedAlgorithm` 时加 `[算法]` 前缀）。

| 命中路径 | 胶囊 |
| --- | --- |
| P2 键数门（非 4/6/7K） | `Sunny` |
| RC 树早退（RC 且非 4K） | `Sunny` |
| R1/R3 Roxy 胜 | `Roxy` |
| R2/R4/R4' Azusa 胜 | `Azusa` |
| R5 Daniel 兜底 | `Daniel` |
| R6/R7 | `Sunny` |
| L1 Azusa 可用 | `Azusa` |
| L2 Azusa 不可用 | `Companella` |
| L3 Daniel 可用 | `Daniel` |
| L4/L5 | `Sunny` |
| 用户直接选择 Companella 且 `lnRatio ≤ 0.18` | `Companella` |
| 用户直接选择 Companella 但 `lnRatio > 0.18` | `Sunny`（被 C2 改回） |
| C6/C8/C9 融合成功 | **不更新**（保持融合前的 `Azusa`/`Roxy`） |
| 缓存命中 | 由快照恢复，不重算 |

---

## 7. 常见误解速查

1. **"LN 主体图为什么永远不用 Companella？"** → C2：`lnRatio > 0.18` 主动跳过（Companella 是 RC 模型），且 LN·Mix 树中还要求 `star < 9`。
2. **"数值里有 Companella 但胶囊写着 Azusa？"** → C10：融合成功不更新胶囊（已知显示问题，非算法失效）。
3. **"选了 Mixed 却显示 `[Sunny]`？"** → 非 4/6/7K、RC 且非 4K、`star ≥ 9` 且 Daniel 不可用、或 R6/R7 都会落到 Sunny 基线，属设计行为。
4. **"同一张 LN 图开 HO 后换了算法？"** → P4：`HO` 强制走 RC 树。
5. **"6K/7K 为什么没有 Mixed 的效果？"** → P2 允许 6/7K，但 RC 树早退、LN·Mix 树的 L1–L4 都要求 4K，因此 6/7K 实际恒为 Sunny 基线。
6. **"低难 4K 的 RC 值为什么和 Azusa 不同？"** → R4'/L1 的低难融合（C8 的 0.5/0.5 加权）会改变 RC 数值与标签。

---

## 8. 与其它机制的交互

- **SunnyWindow**（`forceSunnyWindow`）：在管道之后由 `analysis.js` 用 SunnyWindow 的 LN 段覆盖最终标签的 LN 半，并把新值写回 `pendingMixedCompanellaContext.lnDifficulty`（同时把 `lnRatio` 置 4e65 以强制显示 LN 段）→ 融合仍使用被覆盖后的 LN 半。
- **马拉松时长修正**：pipeline 在 `durationS > 300 && 4K && 算法 ∈ {Azusa, Roxy, Mixed}` 时前置一次 Ett，并把 `marathonCorrection` 注入估算器；修正发生在 Mixed 路由**之前**，会改变 Roxy/Azusa 的数值，因此也可能改变 R1–R5 的胜负（详见 [marathon-correction.md](marathon-correction.md)）。
- **aleju03 LN 半接管**（分支 `feat/aleju03-ln-estimator`）：`LN·Mix` 树（第 4 节 L1–L5，HB 谱面也在其中）的 LN 半**整体**改用 aleju03 的 LN 标签，不再区分档位；aleju03 抛错、非 4K（`unsupported-keycount`）或不是 LN 候选时**回退原表值**。该接管只作用于 LN 半，RC 半、胶囊与第 3 节的 RC 树语义不变（`RC` 树也不接管）。实测 benchmark 102 张 LN 谱的 Mixed LN 标签与 aleju03 独立运行逐行一致。
- **结果缓存**：`actualEstimatorAlgorithm`、`isVibroMap` 等随快照落盘并在命中时恢复；缓存键第一段是用户的 `state.estimatorAlgorithm`（选 `Mixed` 就是 `Mixed`），因此**切换算法必然 miss**，但 Mixed 内部换算法不会让缓存失效——同一张图在设置不变时结果稳定。

---

## 9. 代码索引

| 符号 / 常量 | 位置 | 作用 |
| --- | --- | --- |
| `runMixedEstimatorFromText` | `js/estimator/mixedEstimator.js` | 路由主入口（P1–P5） |
| `MIXED_SUPPORTED_KEYS` | 同上 | 键数门（4/6/7） |
| `modeTagFromLnRatio` | `js/patterns/config.js` | 模式判定（0.15 / 0.90） |
| `composeDifficultyFromRcLn` | `mixedEstimator.js` | `RC \|\| LN` 标签合成（`lnRatio < 0.15` 只出 RC） |
| `canUseRcResult` | 同上 | RC 结果可用性（4K + 非 Invalid + numeric 非 null） |
| `shouldEvaluateAzusaRcPreference` / `shouldPreferAzusaRcResult` | 同上 | Roxy→Azusa 覆盖的筛选/确认双门（各三条语义） |
| `AZUSA_RC_PREFERENCE` | 同上 | 上述两门的 8 个阈值 |
| `isDanielTooLowDifficulty` | 同上 | Daniel 自称 `< Alpha` 时拒用 |
| `buildLowBandCompanellaPlan` | 同上 | 低难融合计划（`fuseRc` / `onDisagree` / `rcNumeric`） |
| `applyCompanellaToMixedResult` | 同上 | C6–C9 的采用/保持/加权融合 |
| `LOW_BAND_COMPANELLA_STAR_MAX` (9) / `RC_FUSION_LOW_BAND_MAX` (11) / `RC_AZUSA_COMPANELLA_FUSION_WEIGHT` (0.5) | 同上 | 融合阈值与权重 |
| `shouldRunCompanella` / `companellaLnRatio > 0.18` | `js/app/analysis.js` | C1/C2 执行门与高 LN 跳过 |
| `formatEstimateDifficultyCaption` | `js/app/graph.js` | 胶囊渲染（仅回退时加 `[算法]` 前缀） |
| `state.actualEstimatorAlgorithm` | `js/app/appContext.js` | 胶囊状态（缓存命中由快照恢复） |
