# docs/pipeline/worker.md：Worker 与 runAnalysisPipeline 架构

> 面向 AI 的技术文档。文中所有 `path:line symbol` 引用均相对本仓库根目录（插件文件夹名为 `ManiaMapAnalyser by Leo_Black`，含空格，路径引用必须精确）。文中 path:line 行号为编写时快照，代码演进后可能漂移；定位源码请以符号名（symbol）为准，必要时用 grep 复核。
> 相关文档：[analysis-pipeline.md](analysis-pipeline.md)（全链路总览，本文是 §7.4 的展开）、[../guides/adding-to-worker.md](../guides/adding-to-worker.md)（新增管线内容到 worker 的操作指南）、[../guides/module-conventions.md](../guides/module-conventions.md)（共享模块约束）、[../breakings/2026-08-09-perf-analysis-pipeline.md](../breakings/2026-08-09-perf-analysis-pipeline.md)（本次破坏性更改记录）。

## 1. 总览：一个 worker，一条管线，一次往返

性能优化分支落地后的 worker 架构：**估算 + 全部附属段收敛为一次 `pipeline` 消息往返**，worker 内跑共享纯函数 `runAnalysisPipeline`，主线程只剩渲染、缓存与浏览器专属逻辑（vibro 判定、Companella ONNX、SV/auto-profile）。

```
analysis.js:374 runInWorker(pipelineInput)         ← manager.js:40 runInWorker
  │  成功 → Promise<{ id, result }>                ← compute.worker.js:31（一次往返）
  │  Worker 构造失败 → manager.js:42 返回 null
  ▼
analysis.js:375  wp ? await wp : await runAnalysisPipeline(pipelineInput)   ← 主线程同步回退，同一函数
  ▼
Node 回归脚本（computeOutput）直接 await runAnalysisPipeline（第三端，无 worker）
```

三端共用同一份 `runAnalysisPipeline`（`js/pipeline/runAnalysisPipeline.js`），保证 worker 路径、同步回退路径、Node 回归路径输出逐位一致（748 golden 全绿的基础）。

## 2. Worker 生命周期（manager.js）

`js/app/worker/manager.js` 是 worker 的唯一持有者，模块级闭包变量 `worker / nextId / pendingRequests / messageHandlerAttached`（manager.js:11-16）。

### 2.1 创建与复用

- `ensureWorker()`（manager.js:16-43）：`worker` 已存在直接返回（**单例复用**）；不存在则 `new Worker(new URL("./compute.worker.js", import.meta.url), { type: "module" })`（manager.js:19-22，`import.meta.url` 相对**模块文件**解析：`new URL` 模式，勿改相对字符串，见 module-conventions.md §3）。
- **构造失败**（不支持 Worker 的环境，如部分 WebView/禁用 worker 的浏览器）：`catch` 置 `worker = null`（manager.js:38-41），**不抛错**，由调用方走同步回退。
- 崩溃恢复：`ensureWorker()` 为新建 worker 注册 `error` 监听；worker 运行时崩溃会**拒绝全部在途请求、`terminate()` 已死实例并置 `worker = null`**，下一次请求自动重建。这同时避免崩溃后 pending promise/listener 永久滞留。

### 2.2 请求取消与响应匹配

`runInWorker(input)`（manager.js:83-124）：

- `generateId()`（manager.js:45-48）自增 `nextId` 生成 `req-<n>-<ts>`。
- 每次新请求先 `rejectAllPending(new Error("Worker request superseded"))`（manager.js:50-56）：**旧的在途请求立即 settle，不再保留 listener/promise**，避免快速换歌/改设置时累积泄漏。
- 响应匹配改为**单一共享 `message` 监听**（`attachMessageHandler`，manager.js:58-75）：按 `event.data.id` 查 `pendingRequests` Map；查得到才 resolve/reject 并清理超时，查不到（已过期/已取消）直接忽略。因此旧请求的迟到响应永远不会污染新请求，也不需要逐请求 `removeEventListener`。
- 正常完成或错误时 `pendingRequests.delete(id)` + `clearTimeout(timeoutId)`；错误 reject `new Error(error)`，成功 resolve result。

### 2.3 30s 超时

- 每个请求的 `setTimeout`（manager.js:95-100）：30s 内未收到响应 → 从 `pendingRequests` 删除并 `reject(new Error("Worker timeout"))`。
- `postMessage` 同步抛错时也会清理该请求并 reject。
- **stale 粒度差异**（详见 §7）：pipeline 单次往返消息在 worker 内**不可中断**，30s 超时是唯一的最坏情况保护；旧 4 估算器消息时代可以在每个 runX 边界丢弃过期请求，粒度更细。

### 2.4 同步回退链

```
runInWorker 返回 null（Worker 构造失败，manager.js:38-41）
  → analysis.js:375 走 await runAnalysisPipeline(pipelineInput)
  → 同一函数、同一输入，逐位相同输出
  → 仅有的代价：估算在主线程同步执行（可能卡顿）
```

`isWorkerAvailable()`（manager.js:129-131）暴露可用性查询（当前仅 debug 面板使用）。

## 3. 消息协议

### 3.1 pipeline 消息（现行主路径）

| 方向 | 载荷 | 位置 |
| --- | --- | --- |
| 主线程 → worker | `{ id, type: "pipeline", input }`，`input = { rawText, estimatorAlgorithm, options }` | manager.js:64 |
| worker → 主线程 | `{ id, result }`（成功，经 structuredClone，`[]` 转移列表）或 `{ id, error }`（失败） | compute.worker.js:31/:34 |

worker 端处理（compute.worker.js:23-37）：

- `data.type === "pipeline"` 分支：校验 `id / input / input.rawText`，缺失回 `{ id, error: "Missing pipeline input" }`（:26）。
- `runAnalysisPipeline(input)` 是 **async**（ett WASM + interlude）→ **必须经 `.then/.catch` 回传**（:29-35），不能同步 `postMessage`。
- 失败（估算器/SunnyWindow 抛错）回 `{ id, error: <message> }`。注意这属于**硬失败**（传播给 analysis.js 外层 catch 报 "Rework failed"）；附属段软失败不在此列（见 §4.4）。

### 3.2 旧 4 估算器消息（保留不动）

`{ id, osuText, options }` → 白名单分派（compute.worker.js:39-81）：Daniel/Azusa/Roxy/Sunny 各自 `runXEstimatorFromText(osuText, options)`，Azusa/Roxy 无效结果回退 Sunny 并改写 `actualEstimatorAlgorithm`（:60-69，`isValidResult` :84-88）。**该分支无调用方**（analysis.js 已全部走 pipeline），保留是兼容旧代码路径的防御（worker 是独立入口，防外部复用/调试）。

### 3.3 返回契约

成功：`{ id, result }` 的 `result` 即 `runAnalysisPipeline` 的输出（§4.3 完整字段）。失败：`{ id, error: string }`。响应经 structuredClone（postMessage 默认序列化），**输出必须 JSON-safe**（无方法、无函数、无循环引用。pattern 的 cluster 已 sanitize，§4.4）。

## 4. runAnalysisPipeline 纯函数（三端共用）

### 4.1 定位与约束

`js/pipeline/runAnalysisPipeline.js:94` `export async function runAnalysisPipeline({ rawText, estimatorAlgorithm, options = {}, parsed = null })`。

- **DOM-free / state-free / JSON-safe**：所有输入显式传入，**禁止读 `state`/`window`/`document`**（含 appContext）；共享模块纯度约束见 module-conventions.md §2。
- **异步**：ett WASM（`analyzeEtternaFromText`）与 interlude（`calculateInterludeStar`）均 async。
- 逐段顺序与旧 analysis.js 完全一致：解析 → 估算分派 → vibro 输入 → 归一化 → SunnyWindow → 派生（sixKConst）→ Interlude → Pattern → Ett → Companella 二次 Ett。
- **错误通道**：估算器/SunnyWindow 抛错**向上传播**（调用方处理）；附属段各自 try/catch 软失败（§4.4）；`errors[]` 恒为空（硬错误预留字段，逐字一致承诺见实测记录）。

### 4.2 输入选项清单（全部显式传参）

`options` 完整清单（runAnalysisPipeline.js:78-81 + analysis.js:350-372 构造处）：

| 选项 | 类型 | 说明 |
| --- | --- | --- |
| `speedRate` | number | 倍速（mod DT/NC/HT 或自定义），默认 1 |
| `odFlag` | number \| null | OD 变化标记 |
| `cvtFlag` | string \| null | 转换标记（IN/HO，影响转换路径与 roxy 共享边界） |
| `withGraph` | boolean | 是否需要难度图数组（`diffText=Graph` 或主体显示 Graph） |
| `forceSunnyReferenceHo` | boolean | Azusa 是否强制 HO 参考 |
| `forceSunnyWindow` | boolean | 是否计算 SunnyWindow（LN 段覆盖） |
| `enableAnalyzeLN` | boolean | LN 分析（typePercentageData） |
| `enableAlwaysShowLNDifficulty` | boolean | 恒显 LN 难度段（estDiff 第 5 参来源） |
| `display6kLevel` | boolean | 6K 定数（sixKConst gate） |
| `extendedEstimationRange` | boolean | 扩展区间表 |
| `withPattern` | boolean | 附属段开关（needComputed 保守值） |
| `withEtterna` | boolean | 附属段开关 |
| `withInterlude` | boolean | 附属段开关 |
| `etternaVersion` | string | 主 Ett 版本 |
| `companellaEtternaVersion` | string | Companella 二次 Ett 版本 |

全部由 analysis.js 从 `state` 取值显式构造（analysis.js:350-372），pipeline 内不读 state。附属段开关默认 false，**仅请求需要的段**，避免 worker 白算（5K 等非支持键数谱面被 override 强制 Pattern 的边界由 analysis.js 主线程回退分支兜底，见 analysis-pipeline.md §7.4）。

### 4.3 输出契约

`runAnalysisPipeline.js:274-292` 返回对象（16 个业务字段 + errors）：

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `rework` | object | 估算结果（star/estDiff/numericDifficulty/graph/lnRatio/columnCount/...），star 已归一化（§4.5） |
| `actualEstimatorAlgorithm` | string | 实际执行算法（Azusa/Roxy 无效回退 Sunny 时改写） |
| `sunnyStar` | number \| null | 归一化用的 Sunny 原始 sr（非归一化算法为 null） |
| `sunnyWindow` | object \| null | `forceSunnyWindow` 时 calculateSunny + calculateLN 结果 |
| `sixKConst` | number \| null | `display6kLevel && columnCount===6` 时 `star*200/81+7/6` 2dp |
| `vibro` | `{ star: number, eligible: boolean }` | 归一化前 star 与 `> 5.0` 判定（§4.5） |
| `parsedSummary` | `{ metadata, lnRatio, columnCount }` | 解析摘要（主线程不再二次解析） |
| `patternReport` | object \| null | 纯数据子集（§4.4） |
| `patternTopFiveClusters` | array \| null | 前 5 cluster |
| `patternError` | string \| null | 软失败文本 |
| `ettResult` | object \| null | `{ values, keycount, ... }` |
| `ettError` | string \| null | 软失败文本 |
| `interludeStar` | number | 软失败时为 NaN |
| `interludeError` | string \| null | 软失败文本 |
| `companellaEttResult` | object \| null | 二次 Ett（Companella/Mixed && 4K && 版本不同） |
| `companellaEttError` | string \| null | 软失败文本 |
| `errors` | string[] | **恒为空**（硬错误预留；估算器抛错走传播通道） |

### 4.4 软失败通道与 pattern sanitize

- **附属段各自 try/catch**（runAnalysisPipeline.js:213-272）：失败置空字段/NaN + 独立 `xxxError` 文本，**不并入 `errors[]`**。原因：旧代码的 errors.push 带展示条件（`shouldReportEtternaError`/`isKeycountError`/need* 门控）依赖主线程 override 后的状态，由 analysis.js 按旧条件决定是否并入（Pattern :618-621、Interlude :564-567、Etterna :655-661），保证逐字一致。
- **pattern sanitize**（`sanitizePatternResult` runAnalysisPipeline.js:45-70）：cluster 对象带 `format()`/`Importance` getter（patterns/clustering.js:112-131），structuredClone/postMessage 会抛 DataCloneError。pipeline 只输出 analysis.js 消费的纯数据字段（`Pattern/SpecificTypes/RatingMultiplier/BPM/Mixed/Amount` + report 的 `Category/LNPercent/HBRowRatio/ModeTag/SVAmount/Duration`）。**getter 求值结果不输出**（Importance），消费侧只读数据字段。

### 4.5 归一化与 vibro 顺序约束

**归一化**（runAnalysisPipeline.js:177-185）：`actualEstimatorAlgorithm ∈ {Azusa, Roxy, Mixed}` 且未回退时，`rework.star` 覆盖为 `runSunnyEstimatorFromText(rawText, options, parser).star`（星数胶囊恒显 Sunny 口径；Daniel 排除；Companella/Sunny 本就是 Sunny sr）。

**vibro 顺序约束（关键）**：

1. **pipeline 内 vibro 判定用归一化前 star**（runAnalysisPipeline.js:169-175）：`vibroStar = Number(selectedRework?.star)`，`eligible = Number.isFinite(vibroStar) && vibroStar > 5.0`，与旧 analysis.js `selectedRework?.star` 顺序一致（算法自身 star，非归一化后口径）。
2. **实际 isVibroMap 判定留在主线程**（analysis.js:664-667）：`isVibroMap = state.vibroDetection && vibroEligible && detectVibro(ettResult?.values, VIBRO_JACKSPEED_RATIO_THRESHOLD)`。`detectVibro`（js/app/vibro.js:16）是浏览器专属（`JackSpeed/Overall >= 0.95`），**等 ettResult 就绪后**在主线程执行，pipeline 只负责把 `vibro.eligible` 算好带出。

**归一化星数复用决策**（runAnalysisPipeline.js:109-127，仅性能优化，不改数值）：

| 算法 | 内部 Sunny 调用 | 可复用？ | 处理 |
| --- | --- | --- | --- |
| Mixed | `sunnyBaseline = options.precomputedSunnyResult || runSunnyEstimatorFromText(osuText, options, parsed)`（mixedEstimator.js:181） | 是（同 options 同文本） | 预计算一份 Sunny 经 `precomputedSunnyResult` 喂入并复用其 star |
| Azusa, `forceSunnyReferenceHo=false` | `sunnyOptions = options` | 是 | 同上 |
| Azusa, `forceSunnyReferenceHo=true` | `sunnyOptions = {...options, cvtFlag:"HO"}` | **否**（cvtFlag 不一致） | 独立计算；**不可**传 `precomputedSunnyResult`（会改变数值语义） |
| Roxy | canonicalizeOsuTiming 改写文本 + `precomputedSunnyResult: null` 硬编码 | **否**（文本被改写） | 独立计算 |
| Sunny/Daniel/Companella | 无归一化 | 不适用 | 无 |

决策表证据：task-11 实测记录 §2。

## 5. 共享解析（parse-once）

### 5.1 parsed 实例只读契约

`runAnalysisPipeline.js:96-101`：外部传入 `parsed`（已 `process()` 的 `OsuFileParser` 实例）则复用，否则内部 `new OsuFileParser(rawText)` + `process()`。估算器/归一化/SunnyWindow/Interlude 共享同一实例。

- **必须是实例，不是 `getParsedData()` 的结果**：`getParsedData()`（osuFileParser.js:40-55）不含 `timingPoints`/`noteTimes`，而 `modIN`/`modHO` 是实例方法（osuFileParser.js:280/:327），内部调 `getBeatLengthAt` → `this.timingPoints`，纯数据对象会挂掉转换路径。见 task-9 learnings。
- `getParsedData()` 只读契约（估算器读取侧）：`columnCount / columns / noteStarts / noteEnds / noteTypes / od / gameMode / status / lnRatio / metaData / breaks / objectIntervals`（osuFileParser.js:41-54）。
- sunnyWindow 的 `getLNParts` 仍需要 osuText 做 `parseTimingsAndDetectSV`（SV 检测子集不在 parsed 契约内）。parsed 只替换 note 子集。

### 5.2 cloneOsuParser 隔离（modIN/modHO）

`modIN`（osuFileParser.js:280）/`modHO`（:327）**原地变异**实例（noteTypes 元素写入 / 数组重赋值 + lnRatio 重算）。共享实例必须保持 pristine：

- 各核心在 `parsed` 提供 **且 cvtFlag ∈ {IN, HO}** 时，先字段拷贝 clone（`cloneOsuParser`，sunnyAlgorithm.js:55-71、sunnyWindowAlgorithm.js:200、chartBuilder.js:71-87，三处文件局部重复）再在 clone 上转换，共享实例不被污染，等价于各自 fresh-parse 行为。
- 无转换需求时直接复用实例（`needsConvert ? cloneOsuParser(parsed) : parsed`，sunnyAlgorithm.js:79-82 模式）。

### 5.3 roxy canonicalize 共享边界

**`speedRate === 1 && cvtFlag 不含 IN/HO` 时才共享 parsed**，否则 Roxy 独立解析（runAnalysisPipeline.js 内 Roxy 分支 + roxyEstimator.js:1400 入口）：

- `canonicalizeOsuTiming` 在**任意** speedRate 下都改写文本（`raw - firstTime + ROXY_CANONICAL_FIRST_OBJECT_MS(1000)`，speedRate=1 是常数平移，≠1 还缩放时间差）→ 改写文本必须重新解析，不能吃共享实例。
- cvtFlag ∈ {HO, IN} 排除：`applyConversionFlag` 原地变异 parser（§5.2），会弄脏调用方实例。
- 速度 1 时平移不变性已被实证（roxy NM 20/20 identical，task-10 实测）。

### 5.4 各段的解析归属（能共享则共享，必须独立则独立）

| 段 | 解析方式 | 原因 |
| --- | --- | --- |
| 估算器/归一化/SunnyWindow/Interlude | 共享同一 parsed 实例 | parse-once 主路径 |
| Pattern | `analyzePatternFromText(rawText)` 独立解析 | patternOsuParser 语义独立（module-conventions.md 守则），且 sanitize 只出纯数据 |
| Ett | `analyzeEtternaFromText(rawText, ...)` 自身解析 | WASM 行构建需要；loader 调用方式不改 |
| Companella 二次 Ett | 同上，独立解析 | 同 Ett |

## 6. WASM-in-worker（ett）

`js/ett/calc.js` 的 loader 在 worker 内**无需改动**即可工作（task 12 实测结论）：

- `locateFile`（calc.js:62-65）用 `new URL(\`./versions/${path}\`, import.meta.url)`。`import.meta.url` 按**模块文件**解析（calc.js 自身的 URL），worker 中与主线程一致 → 生成的 wasm URL 指向**同源静态资源**，`fetch` 实例化正常。
- Node 侧 `IS_NODE` 分支（calc.js:34/:67-80）经 `fs.readFile` 预读 `wasmBinary`，与 worker 无关。
- 已验证路径：worker 内 `analyzeEtternaFromText`（pipeline §9 段）产出真实 MSD（task-12 冒烟 Overall 25.74 等），无 CORS/路径问题。

## 7. Companella 主线程接线与 graph 去留决策

### 7.1 Companella（异步 ONNX，输入来自 pipeline）

`classifyCompanellaDifficulty`（companellaEstimator.js:181，async ONNX，唯一动态 `import()` 场景，companellaEstimator.js:48-55 按环境懒加载 ort）**留在主线程**，analysis.js:742 调用：

- 触发条件：4K（`rework.columnCount === 4`）且 `pendingCompanellaEstimate`（算法=Companella）或 `pendingMixedCompanellaContext`（算法=Mixed，来自 `rework.mixedCompanellaPlan`，analysis.js:504-506）。
- **数据来源全部来自 pipeline 结果**：`companellaMsdValues` 取 `pipelineResult.companellaEttResult?.values ?? pipelineResult.ettResult?.values`（二次 Ett 已在 pipeline 内完成，一次往返）；`interludeStar`、`sunnyStar`（= rework.star）同源。pipeline 未计算二次 Ett（估算失败等边界）时回退主线程直接计算（analysis.js:726-738，旧路径）。
- Companella 直接覆盖最终难度（:749-753）；Mixed 经 `applyCompanellaToMixedResult`（mixedEstimator.js:302）融合（:755-767）。
- **为什么留主线程**：ONNX 推理需要 ort 命名空间 + `ort.env.wasm.wasmPaths`（companellaEstimator.js:62-64）动态 import 与 worker 环境的兼容性未被验证，且输入已由 pipeline 备齐，主线程跑不增加往返。

### 7.2 graph 留 pipeline（实测决策）

graph 数组占 withGraph 消息体 ~99%，但 **structuredClone 耗时 0.6~3.2ms**（3.6 万点马拉松谱面 = 1.76MB/3.2ms）。搬主线程需重跑估算器 withGraph（Sunny 30~100ms / Mixed 100~400ms），是 10~100x 回归，且与 worker offload 目标直接冲突。**决策：graph 留在 pipeline（worker）内**。测量证据：task-12 实测记录 §2。

约束：`normalizeGraphSeries` 不降采样（只补缺刻时间），全量点参与渲染。worker 端任何摘要/重采样都会改变图形（渲染逐字约束）。

## 8. stale 粒度说明

| 层 | 粒度 | 机制 |
| --- | --- | --- |
| 主线程请求序号 | 每次 `fetchBeatmapFile`（analysis.js:231-233 `analysisRequestSeq`） | 每个 await 后 `isStaleRequest()` 检查（:325/:332/:495/:576/:676/:732/:747），过期立即 return |
| worker pendingRequests | 每次 `runInWorker` | 新请求先 `rejectAllPending`；共享 `message` 监听按 id 匹配 `pendingRequests`，过期/已取消响应直接忽略 |
| pipeline 消息内部 | **一次往返，不可中断** | worker 内 `runAnalysisPipeline` 一旦开始就跑到结束；30s 超时（manager.js:95-100）是最坏情况保护 |
| 旧 4 估算器消息 | **每次 runX 可中止** | 旧消息时代主线程可在估算器之间丢弃过期响应（粒度更细，现已无调用方） |

**pipeline 单次往返的取舍**：一次性带回全部产物（含二次 Ett、pattern、graph），代价是 worker 内无法在中途放弃，30s 超时兜底。对典型谱面（总耗时 <1s）无实际影响。

## 9. 与 analysis.js 的接线要点

- `analysis.js:345-400` 构造 `errors`/`pipelineInput` 并调用；`pipelineResult = wp ? await wp : await runAnalysisPipeline(pipelineInput)`（:375）。
- 成功后 `parsedInfo = pipelineResult.parsedSummary`（:376）→ `applyContentBarOverride(parsedInfo.columnCount)`（:377）。
- `vibroEligible` 提升为主函数级变量（analysis.js:447），ETT 段消费（:666）。
- SunnyWindow 合并留在 analysis.js（:509-521）：LN 段替换 `resolvedEstDiff`、`typePercentageData`、`lnStar` 等展示/缓存逻辑逐行未变。
- 缓存命中分支（:472-489）不走 pipeline：从快照恢复 + 命中重派生（sixKConst :482-486 / debugUseAmount :609-617，详见 result-cache.md 与 breaking-changes ⑦⑧）。
- 估算失败路径：pipeline 抛错 → 外层 catch（:378-400）`resetReworkDisplay()` + `errors.push("Rework failed: ...")` + 最小 OsuFileParser 补齐 parsedInfo（失败路径回退元信息解析，与旧 parseMetadataFromBeatmap 行为一致）。

## 10. 变更记录

本架构由 perf/analysis-pipeline-optimization 分支引入（任务 11/12），破坏性变更逐项记录于 [../breakings/2026-08-09-perf-analysis-pipeline.md](../breakings/2026-08-09-perf-analysis-pipeline.md)。
