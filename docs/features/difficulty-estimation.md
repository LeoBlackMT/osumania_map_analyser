# 难度估算（Difficulty Estimation）技术文档

> 目标读者：AI。本文描述插件难度估算模块的架构、分派机制、区间表与标签格式，所有引用均为 `path:line + symbol` 格式，可据此定位源码。文中 path:line 行号为编写时快照，代码演进后可能漂移；定位源码请以符号名（symbol）为准，必要时用 grep 复核。

## 1. 概述

插件提供 **6 种难度估计算法**（Mixed、Azusa、Roxy、Sunny、Daniel、Companella），适配 4/6/7K 的 RC 与 LN 谱面。所有估算器均以 `.osu` 谱面文本为输入（入口函数名统一为 `runXxxEstimatorFromText`，唯一例外是 Companella 的 `classifyCompanellaDifficulty`，见 [注意事项](#9-注意事项)）。

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
| **Mixed** | main | `mixedEstimator.js:192 runMixedEstimatorFromText` | 通用自动选择：RC 谱面在 Roxy/Azusa/Daniel 间挑选，LN 谱面以 Sunny 为基线（低难 4K 用 Companella 覆盖，高难用 Daniel） | 依赖 Ett WASM 与模式分析结果，只能主线程运行；RC 仅 4K 生效，非 4K RC 直接回退 Sunny 基线（`mixedEstimator.js:206`） |
| **Azusa** | worker | `azusaEstimator.js:822 runAzusaEstimatorFromText` | 4K RC 为主 | 仅支持 4K（非 4K 返回 `UnsupportedKeys` 错误结果，`azusaEstimator.js:844`）；LN 上限配置 `rcLnRatioLimit: 0.18`（`azusaEstimator.js:7`）；结果无效时由分派层回退 Sunny |
| **Roxy** | worker | `roxyEstimator.js:1400 runRoxyEstimatorFromText` | 4K RC（GBDT 元模型） | 仅支持 4K；LN ratio > 0.18 返回 `UnsupportedLN`（`roxyEstimator.js:1439`）；结果无效时由分派层回退 Sunny |
| **Sunny** | worker | `sunnyEstimator.js:4 runSunnyEstimatorFromText` | 4/6/7K 的 RC + LN | 无（各键型均有区间表） |
| **Daniel** | worker | `danielEstimator.js:9 runDanielEstimatorFromText` | 4K（Reform 系列） | 仅支持 4K：`calculateDaniel` 返回 `-3` 时回退 Sunny（`danielEstimator.js:18`）；4K 下用自己的 `estimateDanielDan` 标签，非 4K 才走 `estDiff` 区间表（`danielEstimator.js:29-37`） |
| **Companella** | main | `companellaEstimator.js:181 classifyCompanellaDifficulty`（async） | 4K 低中难区间（Mixed 中仅当 `star < 9` 时启用，`mixedEstimator.js:277`） | 异步 ONNX 推理，非 `runXxx` 命名；输入为 MSD + Interlude SR + Sunny SR 特征向量（`companellaEstimator.js:190-201`）；仅 4K（`analysis.js:498` 以 `columnCount === 4` 决定是否触发） |
| **SunnyWindow** | main | `sunnyWindowEstimator.js:14 runSunnyWindowEstimatorFromText` | forceSunnyWindow 开启时的 LN 部分覆盖辅助器 | 不可作为独立算法选择；只替换最终标签的 LN 段（`analysis.js:521-533`） |

## 3. 估算器分派机制

### 3.1 Worker 分派（`js/app/worker/compute.worker.js`）

Worker 内支持 **4 个 worker 估算器**：Sunny、Daniel、Azusa、Roxy（常量表 `compute.worker.js:15 ESTIMATORS`）。分派逻辑：

- 根据 `options.estimatorAlgorithm` 选择实现（`compute.worker.js:24-51`）；
- **Azusa 回退**：结果不通过 `isValidResult` 校验（`compute.worker.js:62`，要求 `star`、`numericDifficulty` 有限且 `estDiff` 为字符串）时，改为执行 `runSunnyEstimatorFromText`，并将 `actualEstimatorAlgorithm` 置为 `"Sunny"`（`compute.worker.js:38-41`）；
- **Roxy 回退**：同样在结果无效时回退 Sunny（`compute.worker.js:44-47`）；
- 回退的 `actualEstimatorAlgorithm` 随结果一并回传（`compute.worker.js:53-55`）。

### 3.2 主线程估算器

Mixed 与 Companella 不经过 Worker：

- **Mixed**：在 `analysis.js:500 runMixedEstimatorFromText(rawText, estimatorOptions)` 直接同步执行；结果中的 `mixedCompanellaPlan`（`mixedEstimator.js:310`）触发后续 Companella 异步估算，完成后经 `mixedEstimator.js:314 applyCompanellaToMixedResult` 合并回结果。
- **Companella**：先同步跑 Sunny 作为兜底（`analysis.js:494`），置 `pendingCompanellaEstimate` 标志（`analysis.js:498`），随后异步执行 ONNX 推理（`companellaEstimator.js:209-222`：`getOrtNamespace()` + `getModelSession()` 并行加载）。

### 3.3 实际执行者追踪

- 用户选择存于 `state.estimatorAlgorithm`，**实际执行**的算法记录在 `state.actualEstimatorAlgorithm`（`analysis.js:514`）；
- 缓存命中时从快照恢复，不重新计算（见 `docs/features/` 下的结果缓存文档）。

## 4. Worker 回退（主线程同步执行）

`js/app/worker/manager.js` 管理 Worker 生命周期：

- `manager.js:41 runInWorker`：创建 Worker 失败（不支持或构造异常，`manager.js:15-27 ensureWorker`）时返回 `null`（`manager.js:43`），调用方回退到主线程同步执行；
- `manager.js:80 isWorkerAvailable`：查询 Worker 是否可用；
- `manager.js:49-74`：只接受最新请求的结果（`latestId` 匹配），并带 30 秒超时兜底。

`analysis.js` 中所有 worker 估算器的调用均采用 `wp ? await wp : runXxxEstimatorFromText(...)` 模式：

- Daniel：`analysis.js:466-467`
- Azusa：`analysis.js:472-473`
- Roxy：`analysis.js:483-484`
- Sunny：`analysis.js:506-507`

回退后走同一套结果校验与 `actualEstimatorAlgorithm` 更新逻辑（`analysis.js:474-478`、`485-489`）。

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

- 每个表是 `[lower, upper, name]` 三元组数组，由 `reworkEstimatorUtils.js:88 intervalLookup` 按 SR 区间查找；超出上下界返回 `< xxx` / `> xxx` 前缀标签（`reworkEstimatorUtils.js:92-93`）。
- 表文件：`4k-rc.js`、`4k-rc-ext.js`、`4k-ln.js`、`4k-ln-ext.js`、`6k-rc.js`、`6k-ln.js`、`7k-rc.js`、`7k-rc-ext.js`、`7k-ln.js`、`10k-rc.js`。
- **ext 扩展表仅存在于 4K RC/LN 与 7K RC**（`4k-rc-ext.js`、`4k-ln-ext.js`、`7k-rc-ext.js`）。
- **6K/7K-LN 静默回退默认表**：`estDiff` 中 `keys.LN[useExtended ? "extended" : "default"] ?? keys.LN.default` 的 `??` 兜底（`reworkEstimatorUtils.js:105`），RC 同理（`reworkEstimatorUtils.js:101`）。
- **注意事项：`7k-wild.js` 被导入但未接入 `DAN_INDEX`**——`js/estimator/intervals/index.js:11` 导入了 `wild7K`，但 `DAN_INDEX` 中 7K 只有 `rc7K`/`rcExt7K`/`ln7K`，该表实际不可达。修改时不要误以为它生效。
- 10K 仅存 RC 默认表（`js/estimator/intervals/index.js:26-28`）。

## 6. extendedEstimationRange 作用域

`extendedEstimationRange`（设置项，默认关闭）只影响 **Sunny 家族** 的标签输出：

- **Sunny**：`sunnyEstimator.js:15 estDiff(parsed.star, parsed.lnRatio, parsed.columnCount, options.extendedEstimationRange === true)`；
- **SunnyWindow**：`sunnyWindowEstimator.js:31 estDiff2(..., options.extendedEstimationRange === true)`；
- **Daniel 非 4K 分支**：`danielEstimator.js:37` 传入同一参数；4K 分支使用 `estimateDanielDan` 自有标签，**不受影响**（`danielEstimator.js:30-36`）；
- `useExtended` 在 `estDiff` 内决定选 `extended` 还是 `default` 表（`reworkEstimatorUtils.js:101`、`105`）。

**不受影响**：

- Azusa / Roxy 的最终段位标签（由各自算法内部产出，不经过 `estDiff` 的 extended 选择）；
- Mixed 仅经其 Sunny 基线间接受影响（`mixedEstimator.js:193` 的 `sunnyBaseline`）。

## 7. RC 标签格式（rcDifficultyFormat.js）

`js/estimator/rcDifficultyFormat.js` 提供希腊字母段位标签与数值的互转，用于 RC 类算法的数值难度显示：

| symbol | 位置 | 作用 |
| --- | --- | --- |
| `GREEK_BY_INDEX` | `rcDifficultyFormat.js:1` | 段位名表：Alpha 到 Kappa（含 `Emik Zeta`、`Thaumiel Eta`、`CloverWisp Theta` 等自定义段位名） |
| `formatRcBaseLabel` | `rcDifficultyFormat.js:39` | 数值基底 → 标签：≤0 为 `Intro 1~3`，1~10 为 `Reform N`，>10 为希腊字母名 |
| `numericToRcLabel` | `rcDifficultyFormat.js:53` | 数值 → 标签：在基底 ± tier（low/mid/low/mid/mid/high/high，`RC_TIER_CANDIDATES`，`rcDifficultyFormat.js:14`）中找最近组合 |
| `rcLabelToNumeric` | `rcDifficultyFormat.js:83` | 标签 → 数值：解析 `Intro`/`Reform`/`Finish`/`Stellium`/希腊字母名 + tier 修正（`GREEK_BASE_MAP` 于 `rcDifficultyFormat.js:22` 映射希腊名 → 11~20） |

互转约定：数值基底 = 段位序号（Reform 1~10 → 1~10，Alpha 起 → 11~20），tier 修正为 ±0.2/±0.4；`rcLabelToNumeric` 对 `||` 分割的 RC/LN 复合标签只取 RC 段（`rcDifficultyFormat.js:84-87`）。

## 8. 第三方算法链接

以下算法为第三方实现，本文档只链接不总结内部细节：

- **Azusa** → [docs/azusa_algorithm.md](../azusa_algorithm.md)（英文）
- **Roxy** → [docs/roxy_algorithm.md](../roxy_algorithm.md)（英文）
- **Sunny / Etterna / Daniel / Companella / Interlude** → [README.md 参考内容区](../../README.md#参考内容)（原文链接：Sunny Rework、Etterna、Daniel、Companella、Interlude 仓库）

## 9. 注意事项

1. **Companella 命名与异步性**：入口为 `companellaEstimator.js:181 classifyCompanellaDifficulty`（async），不是 `runXxxEstimatorFromText` 命名规范；通过 `dynamic import()` 加载 ONNX Runtime（项目内唯一的动态导入），模型会话按需懒加载并缓存。
2. **SunnyWindow 不可选**：它不是独立算法，仅当 `forceSunnyWindow` 开启时由 `analysis.js:521-533` 调用，用其 LN 部分（`estDiff` 的 `||` 第二段）覆盖主估算器的 LN 标签，并附带 `typePercentageData` / `lnStar` 数据（`sunnyWindowEstimator.js:34-35`）。
3. **LN 阈值**：`estDiff` 中 LN 标签仅在 `lnRatio >= 0.15` 或 `enableAlwaysShowLNDifficulty` 时展示，否则只返回 RC 标签（`reworkEstimatorUtils.js:103`）；SunnyWindow 用 `estDiff2` 以 LN 星数为准（`reworkEstimatorUtils.js:110`）。
4. **标签结构**：RC/LN 复合标签以 `"RC || LN"` 形式返回（`reworkEstimatorUtils.js:107`），解析时按 `||` 分割。
5. **缓存键**：`extendedEstimationRange`、`forceSunnyWindow` 等计算相关设置不在缓存键中，依赖设置变更时清缓存（见设置/缓存文档），新增此类设置必须同步加入失效列表。
