# ReworkPP 难度表现面板技术文档

> 目标读者：AI。本文描述 ReworkPP 功能（contentBar 选项值 `"ReworkPP"`）的实现细节、算法说明、缓存与实时机制、性能约束与注意事项。所有引用均为 `path:line + symbol` 格式，行号为编写时快照，代码演进后可能漂移；定位源码请以符号名（symbol）为准，必要时用 grep 复核。
> 相关文档：[../pipeline/analysis-pipeline.md](../pipeline/analysis-pipeline.md)（管线：withPpMetrics 与 Daniel 专用 pass）、[../pipeline/result-cache.md](../pipeline/result-cache.md)（缓存：ppMetrics 快照与覆盖检查第 5 项）、[../pipeline/mod-handling.md](../pipeline/mod-handling.md)（CL/SV2 识别与 classic 判定）。

## 1. 功能说明

ReworkPP 是 **Card Body Content（contentBar）** 的一个选项值（**无空格**，settings.json:62、config.js:6、settingsParser.js `normalizeContentBarValue` 的 `"reworkpp" → "ReworkPP"` 分支三处一致）。选中后卡片主体（`#pp-bars`，index.html）显示 **5 行柱状图**，每行由标签 + 轨道 + 值胶囊（pill）组成：

| 行 | key | 取值范围（min~max） | 中心锚定 |
| --- | --- | --- | --- |
| Max PP / Live PP | `pp` | 0 ~ 1200 | 否（clamp 到 1200） |
| Proportion | `proportion` | 0 ~ 1 | 否 |
| Acc Multiplier | `acc` | 0.87 ~ 1.13 | 是（锚定 1.0） |
| Variety Multiplier | `variety` | 0.945 ~ 1.055 | 是（锚定 1.0） |
| Length Multiplier | `length` | 0.9 ~ 1.1 | 是（锚定 1.0） |

- **第一行标签随模式切换**：Max PP ↔ Live PP（单柱标签切换，非双值并列）。
- **3 个 Multiplier 行中心锚定**：中心线 = 值 1.0，值 ≥ 1.0 向右延伸、< 1.0 向左延伸，fill 宽度 = `|value−1.0|/(max−min)·100`%。
- **值胶囊 3 位小数**（`value.toFixed(3)`，含 PP 行）。
- **彩虹条联动**：`enableEtternaRainbowBars` 开启时 PP 柱状图 fill 注入 `--ett-fill-bg:${ETT_FULL_TRACK_RAINBOW_GRADIENT}` + `--ett-fill-bg-size`（与 Etterna 技能条同一机制，display.js `renderReworkPpBars`）；否则仅设置 `--bar-width`，非彩虹 fallback accent 在 theme.css。
- **原地更新**：复用 `canUpdateBarsInPlace(ppBarsEl, 5, ".pp-fill")` 判断 → `bars-live` 类（CSS 420ms 宽度过渡）原地更新 label/值/style，否则全量重建（stagger）。
- **游玩/结算时实时更新**：api_v2 state name 为 play/gameplay/playing/resultscreen（normalized）时显示 **Live PP**（v2Acc 驱动，判定计数逐帧变化），其余状态（menu/selectplay 等）显示 **Max PP**（100% acc）。

## 2. 算法说明

### 2.1 v2Acc（305 权重准确率）

`js/rework/reworkPerformance.js:14 calculateCustomAccuracy`（共享 DOM-free 模块，逐字移植自 C# osu-author-port rework 分支 ManiaPerformanceCalculator.cs 与 genirx dart rework_performance.dart）：

```
v2Acc = (perfect·305 + great·300 + good·200 + ok·100 + meh·50) / ((perfect+great+good+ok+meh+miss)·305)
```

- totalHits（含 miss）为 0 → 返回 0（**无 NaN**）；结果 clamp 到 [0,1]。
- tosu 判定计数映射：`play.hits.geki → perfect(305)`、`'300' → great`、`katu → good(200)`、`'100' → ok`、`'50' → meh`、`'0' → miss`。
- **v2Acc 不受 Classic 影响**：Classic 只切换星数密度（§3），准确率公式恒为 305 权重。

### 2.2 PP 公式链

`reworkPerformance.js:74 calculateReworkPp`，返回 `{pp, v2Acc, proportion, accMultiplier, varietyMultiplier, lengthMultiplier}`：

```
pp = difficultyValue · modMultiplier · varietyMultiplier · accMultiplier · lengthMultiplier
difficultyValue = 9.8 · max(star − 0.15, 0.05)^2.2 · proportion
modMultiplier = (noFail ? 0.75 : 1) · (easy ? 0.90 : 1)
```

各组件（全部逐字移植，禁止"修正"）：

| 组件 | 函数 | 公式 |
| --- | --- | --- |
| proportion | `calculatePerformanceProportion`（:25） | `acc > 0.8 ? 4.5·(acc−0.8) / (100·(1−acc) + 0.9^20)^0.05 : 0`（**acc ≤ 0.8 恒为 0**） |
| varietyMultiplier | `varietyMultiplier`（:37） | `0.945 + (1.055−0.945) / (1 + e^(−3·(variety−3.25)))` |
| accMultiplier | `accMultiplier`（:44） | `s·(2·acc^20 − 1) + 2 − 2·acc^20`，`s = 0.87 + 0.26 / (1 + e^(−20·(accScalar−1)))` |
| lengthMultiplier | `lengthMultiplier`（:54） | `1.1 / (1 + sqrt(star / (2·totalNotes)))`（totalNotes ≤ 0 或 star 非有限 → null） |

- **无效输入守卫** `inputsValid`（:61）：starRating ≤ 0 / totalNotes ≤ 0 / 任一计数 < 0 / star·variety·accScalar 非有限 → `calculateReworkPp` 返回 null。
- `Math.pow(0.9, 20)` 直接计算，不预取近似值（与 C# 一致）。
- 谱面侧输入（star/variety/accScalar/totalNotes）来自 ppMetrics（§2.3）；判定计数来自 tosu 实时数据（§4）。

### 2.3 谱面侧指标 ppMetrics

由 `js/rework/sunnyAlgorithm.js` 在 `options.withPpMetrics === true` 时计算（genirx dart sunny_algorithm.dart :1550-1778 移植），随估算结果返回：

```
ppMetrics = { star, variety, accScalar, totalNotes, spikiness, switches }
```

- **star**：Sunny 原始 sr（归一化口径，与星数胶囊一致）。
- **spikiness**：`weightedVariance = (Σ((D⁸ − weightedMean⁸)²·w)/Σw)^(1/8)`，`spikiness = sqrt(weightedVariance) / weightedMean`（den ≤ 0 或 weightedMean ≤ 0 → 0）。
- **switches**：head/tail gap 签名 + ±50 滑动平均 + Ks^0.25 + D 权重（dart `_switches`；tail 分支仅当 `tails.last > tails.first` 时运行）。
- **variety**：`0.5·headVariety + 0.11·tailVariety + 0.45·colVariety`，各用 Rao 二次熵（`ln(1+|x−y|)` 距离）。
- **accScalar** = `0.5·spikiness + 0.5·switches`。
- **totalNotes**：**转换前**的原始 HitObjects 数（hold=1），在 preprocessFile 的 modIN/modHO 转换之前捕获。
- 指标**不影响 star 值**，且仅在 withPpMetrics 时计算（避免无条件 O(n²) 成本）。

## 3. Classic 语义（全局 Classic 感知星数）

Classic 判定在 `js/app/modData.js:218-220 getModData`：

```js
const classic = client === "lazer" ? modCodes.has("CL") : !modCodes.has("SV2");
```

- **lazer**：带 `CL`（Classic）mod → classic=true；否则 false。
- **stable**：未开 `SV2`（ScoreV2）→ classic=true；开了 SV2 → false（"stable 开 sv2 = lazer 什么都不开"）。
- unknown/空 client 按非 lazer 处理 → 除非带 SV2 否则 classic=true。

Classic 只影响**星数密度**（sunnyAlgorithm.js:938-940）：

```js
const effectiveWeights = (options?.classicMod === true ? CArr : CArrV2).map((c, i) => c * gaps[i]);
```

- `CArr`（Classic 密度）与 `CArrV2` 均由 computeCAndKs 计算（:918-919），**唯一切换行**是 effectiveWeights。
- `options.classicMod` 沿 options 链透传：analysis.js `classicMod: state.classicMod === true` → pipeline → sunnyEstimator → calculate（sunnyEstimator.js 透传 `classicMod: options.classicMod === true`）。
- **v2Acc 不受 Classic 影响**（§2.1）；PP 公式本身无 Classic 乘子。
- **classicMod 缺省（undefined/false）时输出与改动前逐位一致**（回归锚点，基准 runner 不传 classicMod → 预期零差异）。

## 4. Max vs Live 切换

`js/app/livePp.js`（浏览器专属，每消息更新器）：

- **模式判定** `resolveLiveMode`（:40）：`isPlayStateName(state.clientStateName) || isResultScreenStateName(state.clientStateName)` → **live**；其他（menu/selectplay 等）→ **max**。注意 resultscreen 归一化为 "resultscreen"（勿硬编码 "resultScreen"）。
- **Max PP**：`calculateReworkPp({...ppMetrics, perfect: totalNotes, 其余 0, noFail: modCodes.includes("NF"), easy: modCodes.includes("EZ")})`——100% acc 上限值，数据组装在 analysis.js `buildReworkPpDisplay`（:135）。
- **Live PP**：实时判定计数 `{perfect: hits.geki, great: hits['300'], good: hits.katu, ok: hits['100'], meh: hits['50'], miss: hits['0']}`（缺字段按 0），传入 `calculateReworkPp`。
- **cheap guard**：计数与 live/max 标志均未变化 → 跳过渲染（`countsEqual` :45）；暂停期间计数不变 → 自然冻结。
- **resultScreen 保留计数**：`lastCounts` 模块级保留上一把判定计数（不写 state），play 结束后/结算屏上用保留值兜底。
- **play 首帧 0 判定**：totalHits == 0 → v2Acc = 0 → proportion = 0 → **PP = 0.000，无 NaN**（公式守卫）。
- **负值保护**：proportion 在 acc ≤ 0.8 时恒 0；渲染层对数值做 `Math.max(0, ·)` 防护。

## 5. 缓存与实时机制

- **ppMetrics 进快照**：写门 put 对象含 `ppMetrics: pipelineResult.ppMetrics || null`（analysis.js:868，JSON-safe 纯数值对象，无需 jsonSafe）；命中恢复 `state.ppMetrics = cached.ppMetrics || null`（analysis.js:533）。
- **computed 第 5 项**：`needComputed.pp = contentBarShows("ReworkPP")`（analysis.js:339），覆盖检查比对 `snapshot.computed.pp === needComputed.pp`（analysis.js:353，5 项全等）——contentBar 切到 ReworkPP/Full 会触发按需重算，而非全缓存失效。
- **live 值不缓存**：只缓存谱面侧 ppMetrics；Live PP 是实时渲染值，每次由当前计数重算。
- **pipeline options**：`withPpMetrics: needComputed.pp`、`classicMod: state.classicMod === true`（analysis.js:425-426）。
- **每消息更新 + 计数守卫**：socketHandlers.js 在 `updateSongTimeState(data)` 之后、beatmap 守卫之前调 `updateLivePp(data)`（:180），livePp 内部自带守卫，成本极低；不建立节流/RAF（每消息 + CSS 过渡即满足平滑需求）。

## 6. 性能

- **withPpMetrics 门控**：指标计算（O(n²) 级）仅当 `needComputed.pp`（contentBar 显示 ReworkPP）时开启，**不上主线程**——指标在 worker 内的 pipeline 中计算。
- **Daniel 专用 Sunny pass**：Daniel 估算器自身不产出 Sunny 指标；withPpMetrics 时 pipeline 在 worker 内额外跑一次 `runSunnyEstimatorFromText(rawText, {...options, withPpMetrics: true}, parser)`（runAnalysisPipeline.js:204，同 options 保证一致性：speedRate/odFlag/cvtFlag/classicMod/extendedEstimationRange），取 ppMetrics + star。同步回退路径（worker 不可用）同函数自动覆盖。
- **软失败**：ppMetrics 组装失败（无 Sunny 结果/抛错）→ `ppMetrics: null`，不进 errors[]（与附属段语义一致），渲染错误空态。
- withPpMetrics=false 时输出契约与改动前完全一致（无 ppMetrics 字段、零额外计算）。

## 7. 注意事项

- **play 首帧 0 判定 → PP 0**：totalHits == 0 时 v2Acc=0、proportion=0，显示 PP=0.000（公式守卫，无 NaN、无负数）。
- **resultScreen 保留计数**：结算屏无新判定计数时用 `lastCounts` 保留值渲染 Live PP。
- **负值保护**：proportion acc≤0.8 恒 0；渲染 Math.max(0, ·)。
- **星数胶囊 Classic 感知变化**：classic 标志经 modSignature 第 4 段进缓存键（mod-handling.md），classic 状态切换（stable 开/关 SV2、lazer 开/关 CL）→ 签名变化 → 缓存 miss → 重算，星数胶囊/难度/PP 全部反映当前 Classic 语义。
- **改动约束**：公式数值逐字移植，不做任何"改进/修正"；Classic 只改 effectiveWeights 一行；`reworkPerformance.js` 是共享 DOM-free 模块（无 import、无 window/document），Node benchmark runner 可直接加载。
