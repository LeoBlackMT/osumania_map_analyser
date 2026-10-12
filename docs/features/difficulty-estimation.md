# 难度估算（Difficulty Estimation）技术文档

> 目标读者：AI。本文描述插件难度估算模块的架构、分派机制、区间表与标签格式，所有引用均为 `path:line + symbol` 格式，可据此定位源码。文中 path:line 行号为编写时快照，代码演进后可能漂移；定位源码请以符号名（symbol）为准，必要时用 grep 复核。

## 1. 概述

插件提供 **7 种难度估计算法**（Mixed、Azusa、Roxy、Sunny、Polaris、Daniel、Companella），适配 4/6/7K 的 RC 与 LN 谱面（核心算法键数无关：5K/8K/10K 等无区间表键数仍可计算星数与难度图，段位标签回退 "Unknown difficulty"）。所有估算器均以 `.osu` 谱面文本为输入（入口函数名统一为 `runXxxEstimatorFromText`，唯一例外是 Companella 的 `classifyCompanellaDifficulty`，见 [注意事项](#9-注意事项)）。

估算器内部依赖以下共享模块（这些模块同时被 Node benchmark runner 使用，不含任何浏览器 API）：

| 模块 | 作用 |
| --- | --- |
| `js/rework/` | Sunny Rework / Daniel / SunnyWindow 算法核心（`sunnyAlgorithm.js`、`danielAlgorithm.js`、`sunnyWindowAlgorithm.js`） |
| `js/parser/` | `.osu` 谱面解析（`OsuFileParser`） |
| `js/ett/` | Etterna MinaCalc WASM（MSD 计算，Mixed/Companella 依赖） |
| `js/patterns/` | RC/LN 键型分析（Interlude 算法 + LN 检测） |
| `js/estimator/intervals/` | 段位区间表（`DAN_INDEX`） |

## 2. 估计算法总览

| 算法 | 线程 | 入口函数 | 适用键型/场景 | 限制 |
| --- | --- | --- | --- | --- |
| **Mixed** | main | `mixedEstimator.js:236 runMixedEstimatorFromText` | 通用自动选择：RC 谱面在 Roxy/Azusa/Daniel 间挑选，LN 谱面以 Sunny 为基线（低难 4K 的 RC 段由 Azusa⊕Companella 融合覆盖，见 §3.6；高难用 Daniel） | 依赖 Ett WASM 与模式分析结果，只能主线程运行；RC 仅 4K 生效，非 4K RC 直接回退 Sunny 基线（`mixedEstimator.js:242` 键数门 / `:257` 早退） |
| **Azusa** | worker | `azusaEstimator.js:832 runAzusaEstimatorFromText` | 4K RC 为主 | 仅支持 4K（非 4K 返回 `UnsupportedKeys` 错误结果）；LN ratio > 0.18 在入口返回 `UnsupportedLN`（`rcLnRatioLimit`，结果无效时由分派层回退 Sunny） |
| **Roxy** | worker | `roxyEstimator.js:1468 runRoxyEstimatorFromText` | 4K RC（GBDT 元模型） | 仅支持 4K；LN ratio > 0.18 返回 `UnsupportedLN`（`roxyEstimator.js:1524`）；结果无效时由分派层回退 Sunny |
| **Sunny** | worker | `sunnyEstimator.js:4 runSunnyEstimatorFromText` | 4/6/7K 的 RC + LN | 无（各键型均有区间表） |
| **Polaris** | worker | `polarisEstimator.js runPolarisEstimatorFromText` | 7K 表修正：以 Sunny 为基线，对 star→段位表映射叠加行为数据拟合的平滑修正（DT 速率响应 / LN 侧 lnRatio / 层级形状） | 仅 7K（其他键数与 IN/HO 转换图回退 Sunny，`actualEstimatorAlgorithm = "Sunny"`）；star 恒为 Sunny sr（不入 §4.5 归一化集合）；算法文档见 [../polaris_algorithm.md](../polaris_algorithm.md) |
| **Daniel** | worker | `danielEstimator.js:9 runDanielEstimatorFromText` | 4K（Reform 系列） | 仅支持 4K：`calculateDaniel` 返回 `-3` 时回退 Sunny（`danielEstimator.js:18`）；4K 下用自己的 `estimateDanielDan` 标签，非 4K 才走 `estDiff` 区间表（`danielEstimator.js:30-38`） |
| **Companella** | main | `companellaEstimator.js:181 classifyCompanellaDifficulty`（async） | 4K 低中难区间（Mixed 低难 RC 融合与 LN/Mix 分支中 `star < 9` 时启用，`mixedEstimator.js` LOW_BAND_COMPANELLA_STAR_MAX） | 异步 ONNX 推理，非 `runXxx` 命名；输入为 MSD + Interlude SR + Sunny SR 特征向量（`companellaEstimator.js:190-201`）；仅 4K（`analysis.js:758` 置 `pendingCompanellaEstimate`、`:1004` 的 `shouldRunCompanella` 门以 `columnCount === 4` 决定是否触发） |
| **SunnyWindow** | main | `sunnyWindowEstimator.js:13 runSunnyWindowEstimatorFromText` | forceSunnyWindow 开启时的 LN 部分覆盖辅助器 | 不可作为独立算法选择；只替换最终标签的 LN 段（`analysis.js:764-776`） |

## 3. 估算器分派机制

> `Mixed` 的内部路由（模式判定、RC/LN 分支选择顺序、Companella 触发与融合、算法胶囊语义、为什么某张图没有用 Companella）另见 [mixed-routing.md](mixed-routing.md)。

### 3.1 Worker 分派（`js/app/worker/compute.worker.js`）

Worker 内支持 **4 个 worker 估算器**：Sunny、Daniel、Azusa、Roxy（常量表 `compute.worker.js:16 ESTIMATORS`）。分派逻辑：

- 根据 `options.estimatorAlgorithm` 选择实现（`compute.worker.js:52-73`）；
- **Azusa 回退**：结果不通过 `isValidResult` 校验（`compute.worker.js:60` 调用，`:84` 定义，要求 `star`、`numericDifficulty` 有限且 `estDiff` 为字符串）时，改为执行 `runSunnyEstimatorFromText`，并将 `actualEstimatorAlgorithm` 置为 `"Sunny"`（`compute.worker.js:60-63`）；
- **Roxy 回退**：同样在结果无效时回退 Sunny（`compute.worker.js:66-69`）；
- 回退的 `actualEstimatorAlgorithm` 随结果一并回传（`compute.worker.js:75-77`）。

### 3.2 主线程估算器

Mixed 与 Companella 不经过 Worker：

- **Mixed**：经 pipeline 分派（`runAnalysisPipeline.js:198 runMixedEstimatorFromText(rawText, mixedOpts, parser)`）执行；结果中的 `mixedCompanellaPlan`（`mixedEstimator.js:386`）触发后续 Companella 异步估算，完成后经 `mixedEstimator.js:391 applyCompanellaToMixedResult` 合并回结果。
- **Companella**：先同步跑 Sunny 作为兜底（`runAnalysisPipeline.js:204`），置 `pendingCompanellaEstimate` 标志（`analysis.js:758`），随后异步执行 ONNX 推理（`companellaEstimator.js:209-222`：`getOrtNamespace()` + `getModelSession()` 并行加载）。
- **Companella 的 LN 相关门控已于 2026-09-20 移除**：历史上（2026-09-01 `48256a0`）这里有一条 `lnRatio > 0.18` 的跳过规则（与 Azusa/Roxy 的 RC 作用域阈值同值），导致 LN 主体谱先被设成 `Companella`、计划随即丢弃、胶囊又改回 `"Sunny"`——用户完全看不到 Companella 结果，与 v2.0.0 时期行为不一致。该门属于把算法作用域约束误用到 Mixed 路径，现已删除；Azusa/Roxy **自身**的入口门控（`rcLnRatioLimit: 0.18`）不受影响。
- ONNX 推理抛错时同样回退 Sunny 基线（`pendingCompanellaEstimate` 复位），不产生 No data；pipeline 本身也失败时走主错误路径（`errors` 已含 Rework failed）。

### 3.3 实际执行者追踪

- 用户选择存于 `state.estimatorAlgorithm`，**实际执行**的算法记录在 `state.actualEstimatorAlgorithm`（`analysis.js:747` 从 `pipelineResult` 恢复；重置点 `:284`）；
- 缓存命中时从快照恢复，不重新计算（见 `docs/features/` 下的结果缓存文档）。

### 3.5 Classic 感知星数（Sunny 密度切换）

Classic 语义只影响 **Sunny 家族的星数密度**（`sunnyAlgorithm.js:942-944`）：

```js
const effectiveWeights = (options?.classicMod === true ? CArr : CArrV2).map((c, i) => c * gaps[i]);
```

- `CArr`（Classic 密度）与 `CArrV2` 均由 `computeCAndKs` 计算（`sunnyAlgorithm.js:682` 定义 / `:921` 调用），effectiveWeights 是**唯一切换行**——Classic 开时用 C_arr，否则用 C_arrV2（对标 C# MACalculator.cs ContainsCL 分支与 genirx dart `containsCL ? cArr : cArrV2`）。
- **classicMod 选项**：`options.classicMod === true` 时切换；`undefined/false`（缺省）时输出与改动前**逐位一致**（回归锚点，基准 runner 不传 classicMod → 预期零差异）。classicMod 沿 options 链透传：analysis.js `classicMod: state.classicMod === true`（pipeline options）→ sunnyEstimator → calculate。
- **classic 判定**：`!modCodes.has("SV2") && (client !== "lazer" || modCodes.has("CL"))`（modData.js:229-230，见 mod-handling.md §2.1），经 modSignature 第 4 段进缓存键——classic 状态切换自动触发重算。

### 3.6 Mixed 低难段 RC 融合（Azusa⊕Companella）

低难 4K 谱面（RC 分支中 Roxy scope-out，或 LN/Mix 分支中 `sunnyBaseline.star < 9`）的 RC 数值不再交给单一估算器，而是以 **Azusa 为基准与 Companella 做 0.5/0.5 加权融合**（`RC_AZUSA_COMPANELLA_FUSION_WEIGHT`）：

- **机制**：两个近似独立的低难估计器取平均以降低方差（与 Roxy 内部 Azusa 融合同一机制族）；离线探针显示权重在 0.4~0.7 间结果平坦，取对称 0.5 避免拟合基准分布。
- **计划传递**：`mixedCompanellaPlan` 携带 `{fuseRc: true, rcNumeric, rcEstDiff, onDisagree, lnRatio, lnDifficulty}`，异步 Companella 结果到达后在 `applyCompanellaToMixedResult` 内融合；`estDiff` 由融合数值经 `numericToRcLabel` 重新派生（numeric 与 estDiff 保持同源）。
- **作用域门控**：仅当 Azusa 数值低于 Alpha（`RC_FUSION_LOW_BAND_MAX = 11`，低难主张成立）时融合；未通过时回落 `onDisagree` 指定的分支原赢家（RC 分支 = Azusa，LN/Mix 分支 = Companella），保证 11~17/LN 段行为与融合引入前逐位一致。一致性门控（|Azusa−Companella| ≤ 1.0）经真实运行验证会误杀大量有益融合（净收益 −4.89 → −2.87 MAE 点），已移除。
- **回退**：Companella ONNX 失败/数值无效时不动结果，base 的纯 Azusa RC 段即为兜底显示。
- **行内标签**：融合后 `actualEstimatorAlgorithm` 为 `"Azusa"`（基准侧）；`mixedCompanellaPlan` 不再残留（置 null）。
- **效果**（benchmark harness，main 基线 → 融合，746 行全量）：RC <11 Exact 20.3% → 21.9%、MAE 0.734 → 0.715；RC ALL Exact 50.5% → 50.9%、MAE 0.302 → 0.294；ALL Exact 44.9% → 45.3%、MAE 0.428 → 0.422；11~17 微升（MAE 0.318 → 0.316），≥17 / LN 与基线逐位一致（无回归）。改动行净效应 −4.89 MAE 点（73 行改善 / 52 行回退）。
- 只改星数密度，**不影响** v2Acc/PP 公式与 Azusa/Roxy/Daniel 等非 Sunny 主算法的自身口径（Azusa/Roxy 内部参考 Sunny 调用同参透传）。

### 3.7 4K LN 数值后修正（lnCorrection）

4K 且 Sunny `lnRatio` 主导的谱面在 Mixed 上的最终 numeric（来源为 Companella 或其与 Azusa 的融合）系统性低估：benchmark ln 行（102 条）bias +1.02（MAE 1.228），行为数据集 4K LN 侧 +0.2~0.5。依据「移植算法（Daniel / Companella）不直接修改、必要时在输出结果后修正」的约束，在 `applyCompanellaToMixedResult` 的三个采纳路径上对 numeric 叠加 lnRatio 门控的修正（`lnCorrection.js`：全量 +0.5、ramp 0.375 → 0.55），并重派生 RC 半段标签（numeric 与 estDiff 保持同源）；LN 半段标签不动。未触发时逐位保持原行为；`forceSunnyWindow` 的 4e65 lnRatio 哨兵不触发。详见 [ln-correction.md](ln-correction.md)。

### 3.4 显示星数统一为 Sunny 原始 sr

左上角星数胶囊（`#rework-star`）显示的是 `rework.star`（`analysis.js:795` `showNumericStarValue`；其余调用点 :1230/:1241/:1264/:1268）；归一化逻辑已迁入 `runAnalysisPipeline.js:225-229`。**无论选择哪种算法，只要显示的是星数，就一定是 Sunny 算法原始输出的 sr**：

- Azusa / Roxy / Mixed 的 `star` 字段是自身 numeric 难度的线性映射（`3.4 + 0.38 * numericDifficulty`，`azusaEstimator.js:992`、`roxyEstimator.js:1634`），**不是** Sunny sr；Mixed 的 star 还可能继承被选中的 Roxy/Azusa/Daniel 子算法（`mixedEstimator.js:380-388`）；
- `runAnalysisPipeline.js:225-229`：当 `actualEstimatorAlgorithm` 为 Azusa/Roxy/Mixed（即未回退 Sunny）时，用与"Sunny 算法选择"完全相同的调用 `runSunnyEstimatorFromText(rawText, estimatorOptions)` 重算 sr 并覆盖 `rework.star`（同一函数、同一 options，逐位一致）；
- **Daniel 排除**：使用独立改进算法（自己的 `sr`），不做归一化；
- **Polaris 排除**：star 本就是 Sunny sr（7K 生效时与 Sunny 选择逐位一致；非 7K/cvt 回退路径同 Sunny），无需归一化覆盖（`runAnalysisPipeline.js` 分派注释）；
- **Companella / SunnyWindow**：star 本就是 Sunny sr（Companella 直接跑 Sunny，`runAnalysisPipeline.js:204`；SunnyWindow 只替换 estDiff 的 LN 段，不碰 star），无需处理；
- **vibro 检测**仍用算法自身 star 判定（`runAnalysisPipeline.js:213` `Number(selectedRework?.star)`），不随归一化变化，保持既有行为；
- 缓存快照存储的就是归一化后的 `rework.star`（`analysis.js:1096`），命中恢复一致；缓存键带 `star-v9` 版本前缀使旧快照失效（沿革见 [result-cache.md](../pipeline/result-cache.md) §5）。

## 4. Worker 回退（主线程同步执行）

`js/app/worker/manager.js` 管理 Worker 生命周期：

- `manager.js:89 runInWorker`：创建 Worker 失败（不支持或构造异常，`manager.js:17-49 ensureWorker`）时返回 `null`（`manager.js:91`），调用方回退到主线程同步执行；
- `manager.js:135 isWorkerAvailable`：查询 Worker 是否可用；
- `manager.js:89-131 runInWorker`：新请求先取消全部在途请求（`pendingRequests` Map，`rejectAllPending` :56-62 / :95），共享 `message` 监听按 id 匹配响应，并带 30 秒超时兜底（`manager.js:101-106`）。

worker 估算器的调用统一经 `runAnalysisPipeline` 内部分派（pipeline 自身在 worker 不可用时回退主线程；各估算器调用点）：

- Daniel：`runAnalysisPipeline.js:173`
- Azusa：`runAnalysisPipeline.js:180`
- Roxy：`runAnalysisPipeline.js:188`
- Sunny：`runAnalysisPipeline.js:204` / `:207`

回退后走同一套结果校验与 `actualEstimatorAlgorithm` 更新逻辑（`runAnalysisPipeline.js:182-186` Azusa / `:190-193` Roxy）。

## 5. 段位区间表（DAN_INDEX）

`js/estimator/intervals/index.js:13 DAN_INDEX` 按键数组织区间表：

```js
export const DAN_INDEX = {
    4: { RC: { default: rc4K, extended: rcExt4K }, LN: { default: ln4K, extended: lnExt4K } },
    6: { RC: { default: rc6K },               LN: { default: ln6K } },
    7: { RC: { default: rc7K, extended: rcExt7K }, LN: { default: ln7K } },
    10: { RC: { default: rc10K } },
};
```

- 每个表是 `[lower, upper, name]` 三元组数组，由 `reworkEstimatorUtils.js:87 intervalLookup` 按 SR 区间查找；超出上下界返回 `< xxx` / `> xxx` 前缀标签（`reworkEstimatorUtils.js:91-92`）。
- 表文件：`4k-rc.js`、`4k-rc-ext.js`、`4k-ln.js`、`4k-ln-ext.js`、`6k-rc.js`、`6k-ln.js`、`7k-rc.js`、`7k-rc-ext.js`、`7k-ln.js`、`10k-rc.js`。
- **ext 扩展表仅存在于 4K RC/LN 与 7K RC**（`4k-rc-ext.js`、`4k-ln-ext.js`、`7k-rc-ext.js`）。
- **6K/7K-LN 静默回退默认表**：`estDiff` 中 `keys.LN[useExtended ? "extended" : "default"] ?? keys.LN?.default` 的 `??` 兜底（`reworkEstimatorUtils.js:107`），RC 同理（`reworkEstimatorUtils.js:102`）。
- **LN 表缺失回退**（如 10K 仅有 RC 表）：`keys.LN` 可选链守卫——LN 表不存在时 `estDiff`/`estDiff2` 直接返回 RC 难度，不再访问 undefined 崩溃（`reworkEstimatorUtils.js:107-108` / `:122-123`）。
- **注意事项：`7k-wild.js` 被导入但未接入 `DAN_INDEX`**——`js/estimator/intervals/index.js:11` 导入了 `wild7K`，但 `DAN_INDEX` 中 7K 只有 `rc7K`/`rcExt7K`/`ln7K`，该表实际不可达。修改时不要误以为它生效。
- 10K 仅存 RC 默认表（`js/estimator/intervals/index.js:26-28`）；**5K/8K 等无表键数的星数仍正常计算**（Sunny 核心键数无关），仅难度标签显示 "Unknown difficulty"。

## 6. extendedEstimationRange 作用域

`extendedEstimationRange`（设置项，默认关闭）只影响 **Sunny 家族** 的标签输出：

- **Sunny**：`sunnyEstimator.js:16 estDiff(parsed.star, parsed.lnRatio, parsed.columnCount, options.extendedEstimationRange === true)`；
- **SunnyWindow**：`sunnyWindowEstimator.js:30 estDiff2(..., options.extendedEstimationRange === true)`；
- **Daniel 非 4K 分支**：`danielEstimator.js:38` 传入同一参数；4K 分支使用 `estimateDanielDan` 自有标签，**不受影响**（`danielEstimator.js:30-37`）；
- `useExtended` 在 `estDiff` 内决定选 `extended` 还是 `default` 表（`reworkEstimatorUtils.js:102`、`107`）。

**不受影响**：

- Azusa / Roxy 的最终段位标签（由各自算法内部产出，不经过 `estDiff` 的 extended 选择）；
- Mixed 仅经其 Sunny 基线间接受影响（`mixedEstimator.js:237` 的 `sunnyBaseline`）。

## 7. RC 标签格式（rcDifficultyFormat.js）

`js/estimator/rcDifficultyFormat.js` 提供希腊字母段位标签与数值的互转，用于 RC 类算法的数值难度显示：

| symbol | 位置 | 作用 |
| --- | --- | --- |
| `GREEK_BY_INDEX` | `rcDifficultyFormat.js:1` | 段位名表：Alpha 到 Kappa（含 `Emik Zeta`、`Thaumiel Eta`、`CloverWisp Theta` 等自定义段位名） |
| `formatRcBaseLabel` | `rcDifficultyFormat.js:39` | 数值基底 → 标签：≤0 为 `Intro 1~3`，1~10 为 `Reform N`，>10 为希腊字母名 |
| `numericToRcLabel` | `rcDifficultyFormat.js:53` | 数值 → 标签：在基底 ± tier（low / mid/low / mid / mid/high / high，偏移 −0.4/−0.2/0/+0.2/+0.4，`RC_TIER_CANDIDATES`，`rcDifficultyFormat.js:14-20`）中找最近组合 |
| `rcLabelToNumeric` | `rcDifficultyFormat.js:83` | 标签 → 数值：解析 `Intro`/`Reform`/`Finish`/`Stellium`/希腊字母名 + tier 修正（`GREEK_BASE_MAP` 于 `rcDifficultyFormat.js:22` 映射希腊名 → 11~20） |

互转约定：数值基底 = 段位序号（Reform 1~10 → 1~10，Alpha 起 → 11~20），tier 修正为 ±0.2/±0.4；`rcLabelToNumeric` 对 `||` 分割的 RC/LN 复合标签只取 RC 段（`rcDifficultyFormat.js:84-87`）。

## 8. 第三方算法链接

以下算法为第三方实现，本文档只链接不总结内部细节：

- **Azusa** → [docs/azusa_algorithm.md](../azusa_algorithm.md)（英文）
- **Roxy** → [docs/roxy_algorithm.md](../roxy_algorithm.md)（英文）
- **Polaris** → [docs/polaris_algorithm.md](../polaris_algorithm.md)（英文，7K 表修正估算器）
- **Sunny / Etterna / Daniel / Companella / Interlude** → [README.md 参考内容区](../../README.md#参考内容)（原文链接：Sunny Rework、Etterna、Daniel、Companella、Interlude 仓库）

## 9. 注意事项

1. **Companella 命名与异步性**：入口为 `companellaEstimator.js:181 classifyCompanellaDifficulty`（async），不是 `runXxxEstimatorFromText` 命名规范；通过 `dynamic import()` 加载 ONNX Runtime（项目内唯一的动态导入），模型会话按需懒加载并缓存。
2. **SunnyWindow 不可选**：它不是独立算法，仅当 `forceSunnyWindow` 开启时由 `analysis.js:764-776` 调用，用其 LN 部分（`estDiff` 的 `||` 第二段）覆盖主估算器的 LN 标签，并附带 `typePercentageData` / `lnStar` 数据（`sunnyWindowEstimator.js:33-34`）。
3. **LN 阈值**：`estDiff` 中 LN 标签仅在 `lnRatio >= 0.15` 或 `enableAlwaysShowLNDifficulty` 时展示，否则只返回 RC 标签（`reworkEstimatorUtils.js:104`）；SunnyWindow 用 `estDiff2` 以 LN 星数为准（`reworkEstimatorUtils.js:113` 的 `estDiff2`）。
4. **标签结构**：RC/LN 复合标签以 `"RC || LN"` 形式返回（`reworkEstimatorUtils.js:110`），解析时按 `||` 分割。
5. **缓存键**：`extendedEstimationRange`、`forceSunnyWindow` 等计算相关设置不在缓存键中，依赖设置变更时清缓存（见设置/缓存文档），新增此类设置必须同步加入失效列表。
6. **显示星数恒为 Sunny sr**：星数胶囊只显示 Sunny 原始 sr（§3.4），Azusa/Roxy/Mixed 的 star 仅是内部口径，不作为显示值。
7. **Etterna MSD 上限突破**：MinaCalc 内置 SSR 技能值上限 40.0 已通过 wasm 二进制等长替换（`f32.const 40.0` → `f32.const 100.0`）提升至 100.0（工具见 `tools/patch-minaclac-msd-cap.mjs`，`tools/README.md`）；显示刻度 `config.js:52 etterna.maxSkillValue = 45.0`（技能条宽度与 MSD 胶囊颜色映射按 45 标定）。浏览器加载 wasm 带 `?v=` cache-bust（`js/ett/constants.js WASM_ASSET_VERSION`，bump 时机：wasm 字节变更）。实测效果：技能值低于饱和区（Overall ≲ 37）的谱面输出逐位不变；处于饱和区（~37–40）的谱面数值随上限平滑上升；触顶谱面（技能值被钳在 40.000）突破 40/42 上限。`minaclac-68.0-unofficial.wasm` 未 patch（其 40.0 常量非技能上限，patch 会改变普通谱面输出）；0.70.0 的 Stream 技能值存在旧版独立的饱和行为（不在本 patch 常量内），其 Overall 与其余技能值正常突破。
