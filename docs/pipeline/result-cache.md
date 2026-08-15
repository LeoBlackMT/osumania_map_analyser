# docs/pipeline/result-cache.md — 结果缓存（LRU）

> 面向 AI 的管线技术文档。文中所有 `path:line symbol` 引用均相对本仓库根目录（插件文件夹名为 `ManiaMapAnalyser by Leo_Black`，含空格，路径引用必须精确）。文中 path:line 行号为编写时快照，代码演进后可能漂移；定位源码请以符号名（symbol）为准，必要时用 grep 复核。
> 相关文档：[settings-pipeline.md](settings-pipeline.md)（设置管线，含失效触发）、[analysis-pipeline.md](analysis-pipeline.md)（分析管线总览）、[guides/cache-invalidation.md](../guides/cache-invalidation.md)（决策指南：新设置该不该加入失效——本文是"怎么工作的"，那篇是"该不该加"）。

## 1. 定位与数据流位置

结果缓存位于 `analysis.js:248 fetchBeatmapFile(reason)` 内部，是**获取谱面文件之前**的拦截层（`analysis.js:282-317`）：

```
fetchBeatmapFile() → 查缓存（analysis.js:308-317）
  → 命中：跳过 fetch/parse/估算，直接用快照渲染（analysis.js:322-331、:429-438）
  → miss：fetch .osu → 解析 → Worker/主线程估算 → 写门（analysis.js:743-746）→ put（analysis.js:751）
```

- 命中时**完全跳过网络 fetch**（`analysis.js:322` cached 分支没有 fetch 调用），只做 `setEffectiveContentBarForMap` 等显示侧处理。
- 缓存只影响显示速度，**不影响计算结果**（命中快照与重算结果同源，见 §10）。
- 模块本体 `resultCache.js` 是纯模块：无 import、无 DOM（`resultCache.js:4` 头注释），Node benchmark runner 的 `smoke-result-cache.mjs` 直接加载它。

## 2. API 与 LRU 机制

`resultCache.js:15 createResultCache({ maxSize = 200 } = {})` 返回一个闭包对象，内部用 `Map` 实现 LRU：

| 方法 | 位置 | 语义 |
| --- | --- | --- |
| `get(key)` | resultCache.js:30-32 | `touch(key)` 命中则 `deepClone(map.get(key))` 返回，未命中返回 `undefined` |
| `put(key, value, {skip})` | resultCache.js:34-43 | 见 §3（skip）与下方驱逐逻辑；`deepClone(value)` 后存入 |
| `has(key)` | resultCache.js:45-47 | **同样 touch**——存在即提升为最近使用（注意：`has` 也有副作用） |
| `clear()` | resultCache.js:49-52 | `map.clear()` 且 `generation += 1`（代数 +1，见 §4/§7） |
| `get size` | resultCache.js:54-56 | 当前条目数 |
| `get generation` | resultCache.js:58-60 | 当前代数（每次 clear 递增） |

**LRU 语义**（关键）：`Map` 的插入序即最近使用序（`resultCache.js:16` 注释 "insertion order = recency order (oldest first)"）：

- **touch 提升**（`resultCache.js:19-27`）：`get`/`has` 命中时 `map.delete(key)` 再 `map.set(key, value)`，把该条目移到迭代序末尾（最新）。
- **put 驱逐**（`resultCache.js:36-41`）：key 已存在 → 先 delete 再 set（刷新位置，不占新容量）；key 不存在且 `map.size >= maxSize` → `map.delete(map.keys().next().value)` 驱逐迭代序第一个（最久未用），再 set 新条目。
- **get 与 put 均 deepClone**（`resultCache.js:6-13 deepClone`）：优先 `structuredClone`，不可用时回退 `JSON.parse(JSON.stringify(value))`（Node <17 兼容，`resultCache.js:10-11` ponytail 注释）。因此**值必须 JSON-safe**——不能含函数、Date、带方法的对象（cluster 对象见 §7 的 jsonSafe 处理）。

## 3. put({skip:true})

`resultCache.js:35`：`if (skip) return;`——**不写入、不占容量、不触发驱逐**，直接返回。

用于 meta 降级 identity（§8）：`analysis.js:776` `resultCache.put(cacheKey, {...}, { skip: isMetaDegraded })`。这类快照"只读不写"，永远不进缓存。

## 4. 单例与工具函数

`resultCache.js` 模块级导出三个符号，供 settings.js 与 analysis.js 操作**同一个**实例：

- `resultCache.js:66 resultCache`——`createResultCache()` 默认参数（maxSize 200）的单例。
- `resultCache.js:68 clearResultCache()`——`resultCache.clear()` 的封装。settings.js 失效列表（§9）和缓存关闭时（§11）都调它；每次调用代数 +1。
- `resultCache.js:72 resultCacheGeneration()`——返回当前代数。analysis.js 在**分析开始时**捕获（`analysis.js:252 genAtStart`），写门前比对（§7）。

调用方不持有实例、只持有这层工具函数，从而保证设置失效与分析读写作用于同一缓存。

## 5. 缓存键

`analysis.js:305`：

```js
const CACHE_KEY_STAR_UNIFIED_VERSION = "star-v2"; // 星数统一为 Sunny 原始 sr 后作废旧快照
const cacheKey = `${CACHE_KEY_STAR_UNIFIED_VERSION}|${state.estimatorAlgorithm}|${state.lastBeatmapIdentity}|${state.modSignature}`;
```

版本前缀 + 三段含义：

| 段 | 来源 | 说明 |
| --- | --- | --- |
| `star-v2` | 常量 `CACHE_KEY_STAR_UNIFIED_VERSION`（analysis.js:305） | 缓存语义版本。星数胶囊统一为 Sunny 原始 sr（Azusa/Roxy/Mixed 的 star 被归一化，见 difficulty-estimation.md §显示星数）后，旧快照里存的是算法自身映射 star，必须失效——用版本前缀一次性作废所有旧条目 |
| `estimatorAlgorithm` | `state.estimatorAlgorithm`（appContext.js:88） | 用户选择的算法。注意不是实际算法——Azusa 回退 Sunny 时 key 仍含 "Azusa"，快照内用 `actualEstimatorAlgorithm` 记录实况（§10） |
| `lastBeatmapIdentity` | `state.lastBeatmapIdentity` | 谱面身份，由 socketHandlers.js 构建（见下）。**含 beatmap 的 md5 hash → 谱面文件被替换（内容变化）后 hash 变、键变，天然免疫文件替换** |
| `modSignature` | `state.modSignature` | mod 签名，modData.js 构建（见下） |

**identity 构成**（`socketHandlers.js:236-250`）：按可用性拼接 `id:${beatmapId}`（:237-239）、`hash:${beatmapHash}`（:240-242）、`path:${beatmapPath}`（:243-245），三者皆无时回退 `meta:${beatmapTitleKey}`（:247-250，见 §8），最终 `identityParts.join("|")`（:252）。空 identity 直接 return 不触发分析（:253）。

**modSignature 构成**（`modData.js:218-228`）：

```js
const modSignature = [
    Number(speedRate).toFixed(5),      // 倍速，5 位小数
    odFlag == null ? "none" : String(odFlag),
    cvtFlag == null ? "none" : String(cvtFlag),
    classic ? "1" : "0",               // Classic 感知星数标志（第 4 段）
].join("|");
```

即 `speedRate|odFlag|cvtFlag|classic`，只含**计算相关维度**（`modData.js:216-217` 注释：避免 lazer mod payload 无关字段波动导致重算抖动）。第 4 段 `classic` 由 `modData.js:218-220` 判定（`client === "lazer" ? modCodes.has("CL") : !modCodes.has("SV2")`，详见 mod-handling.md §2）——classic 切换会改变星数密度，必须进键。返回对象同时含 `modSignature` 字段（`modData.js:236`）与 `modCodes`（:238）、`classic`（:237），由 `modData.js:62 getModData(data, {...})` 计算。

## 6. 命中判定：覆盖检查

查缓存的前提（`analysis.js:308`）：`state.enableResultCache && state.lastBeatmapIdentity`。随后 `resultCache.get(cacheKey)`（`analysis.js:309`），取到快照后做**覆盖检查**（`analysis.js:310-314`）：

```js
snapshot.computed.graph === needComputed.graph
&& snapshot.computed.pattern === needComputed.pattern
&& snapshot.computed.ett === needComputed.ett
&& snapshot.computed.interlude === needComputed.interlude
&& snapshot.computed.pp === needComputed.pp
```

**五项**全匹配才命中，任一不匹配视为 miss 走完整重算。

- `needComputed` 是**本次分析需要哪些计算产物**的布尔集（`analysis.js:325-343`）：pattern（键型）、ett（Etterna MSD）、graph（难度图）、interlude（Interlude 星数）、pp（ReworkPP 谱面侧指标，`contentBarShows("ReworkPP")`，`analysis.js:342`）。各项由当前显示需求与算法需求推导（如 `state.diffText === "Graph" || contentBarShows("Graph")` 需要 graph，`analysis.js:337`；Companella/Mixed 需要 ett 与 interlude，`analysis.js:336`、:340-341）。`contentBar` 为多选有序数组后 `contentBarShows` 输出布尔语义不变。
- `snapshot.computed` 在写入时保存：`analysis.js:775` `computed: needComputed`。
- **显示类设置（contentBar/srText/diffText 切换等）由覆盖检查处理，而不是缓存失效**——改了显示需求但没改计算需求时仍可命中（如从"显示难度"切到"显示 MSD"若 needComputed 不变）；改了计算需求（如切到 ReworkPP 主体，`needComputed.pp` 由 false 变 true）则检查自动判 miss 重算。
- `needComputed` 用 fetch 前的保守值（`analysis.js:284-286` 注释：尚未经过谱面级 `effectiveContentBar` override），仅用于覆盖检查；实际 shows*/need* 在执行块内 override 之后重新计算。

## 7. 写门

`analysis.js:743-746`，全部条件同时满足才 `resultCache.put`（`analysis.js:751`）：

```js
if (!cached && state.enableResultCache && state.lastBeatmapIdentity
    && errors.length === 0
    && rework && !isStaleRequest()
    && genAtStart === resultCacheGeneration()) {
```

| 条件 | 含义 |
| --- | --- |
| `!cached` | 必须是 miss（命中的快照本来就在缓存里，无需再写） |
| `state.enableResultCache` | 开关开启 |
| `state.lastBeatmapIdentity` | 存在谱面身份（否则连键都不完整） |
| `errors.length === 0` | 分析全程无错误（`errors` 数组定义于 `analysis.js:388`） |
| `rework` | 估算结果真实存在（`rework` 对象在缓存分支由快照恢复 `analysis.js:430`，否则由估算器产出） |
| `!isStaleRequest()` | 请求未过期（`analysis.js:251`：`requestSeq !== state.analysisRequestSeq` 即过期——换歌/新分析已覆盖本次请求） |
| `genAtStart === resultCacheGeneration()` | **代数守卫**：`genAtStart` 捕获于分析开始（`analysis.js:252`），若期间 settings.js 调了 `clearResultCache()`（代数 +1）则拒绝写回——防止 clear 之后的旧分析结果污染新缓存 |

写入内容（`analysis.js:751-776`）：`rework`（star/estDiff/numericDifficulty/numericDifficultyHint/graph/lnRatio/columnCount/lnStar/typePercentageData）、`patternReport`、`mergedClusters`、`ettResult`、`interludeStar`、`isVibroMap`、`sixKConst`、`actualEstimatorAlgorithm`（§10）、`parsedInfo`、`ppMetrics`（谱面侧 ReworkPP 指标 `{star, variety, accScalar, totalNotes, spikiness, switches}`，JSON-safe 纯数值对象，无需 jsonSafe）、`computed: needComputed`。

**jsonSafe 包装**（`analysis.js:747-750`）：clustering.js 的 cluster 对象带 `format()`/`Importance` 方法，`structuredClone` 无法拷贝（违反 resultCache 的 JSON-safe 契约），故 `mergedClusters` 等经 `jsonSafe(value) = value == null ? value : JSON.parse(JSON.stringify(value))`（`analysis.js:750`）只存渲染所需普通字段。

## 8. meta 降级（永不写入缓存）

当 tosu 缺少 beatmap id/hash/path 时，identity 降级为标题元数据（`socketHandlers.js:247-250`）：

```js
const hasMetadataIdentity = beatmapTitleKey.replace(/[:]/g, "").length > 0;
if (identityParts.length === 0 && hasMetadataIdentity) {
    identityParts.push(`meta:${beatmapTitleKey}`);
}
```

处理链：

1. `analysis.js:306 isMetaDegraded = String(state.lastBeatmapIdentity || "").startsWith("meta:")`——在 fetchBeatmapFile 开头判定。
2. 查询侧：`analysis.js:308` 只要 identity 存在就会尝试查缓存（`meta:` 键也可能命中——但如果从未写入过，实际永远 miss）。
3. 写入侧：`analysis.js:776` `{ skip: isMetaDegraded }` → `put({skip:true})`（§3），**meta 降级快照永不进入缓存**。

原因：meta 键只含标题，**碰撞风险**（同标题不同谱面共用一键）；且 md5 缺失意味着无法检测文件替换。因此 meta 身份下的分析结果只读不写，缓存对该类谱面实际上不生效。

## 9. 失效列表（settings.js:833-850）

settings.js 的命令监听回调在**任何计算相关设置变化**时调 `clearResultCache()`（`settings.js:849`）。完整 12 个条件（`settings.js:835-848`）：

| # | 条件变量 | 对应设置 |
| --- | --- | --- |
| 1 | `estimatorChanged` | estimatorAlgorithm |
| 2 | `azusaSunnyReferenceHoChanged` | azusaSunnyReferenceHo |
| 3 | `etternaVersionChanged` | etternaVersion |
| 4 | `companellaEtternaVersionChanged` | companellaEtternaVersion |
| 5 | `svChanged` | useSvDetection |
| 6 | `vibroChanged` | VibroDetection（uniqueID 大写 V，state 字段小写 `state.vibroDetection`） |
| 7 | `wsEndpointChanged` | wsEndpoint（仅在 `changed` 不在 `recomputeNeeded`，故显式列出——`settings.js:834` 注释） |
| 8 | `forceSunnyWindowChanged` | forceSunnyWindow |
| 9 | `enableLNDifficultyChanged` | enableLNDifficulty |
| 10 | `enableAnalyzeLNChanged` | enableAnalyzeLN |
| 11 | `enableAlwaysShowLNDifficultyChanged` | enableAlwaysShowLNDifficulty |
| 12 | `extendedEstimationRangeChanged` | extendedEstimationRange |

**已移出失效列表的显示派生设置**（toggle-diff 实证零输出契约差异，30 样本子集）：

- `debugUseAmount`（debugChanged）——命中时重放排序 + Category 覆盖（§10）。
- `display6kLevel`（display6kLevelChanged）——命中时按缓存 star 重算 sixKConst（§10）。

两者**仍保留在 recomputeNeeded**（`settings.js:814`、:830）：设置变化仍会触发一次调度重算，但重算走**缓存命中分支**（不 fetch、不跑 pipeline），只做命中恢复 + 重派生 + 重新渲染——这就是显示更新机制。`enableAlwaysShowLNDifficulty` **不能**移出（toggle-diff 显示 estDiff 在 lnRatio<0.15 谱面上有真实差异——它改变了计算产出的 estDiff 字符串）。

**规则**：

- 新增**计算影响**设置必须加入此列表——缓存键不含它（§5），漏加会静默提供过期结果。
- 纯**显示**设置**不得**加入——覆盖检查（§6）已处理；若该设置改变的是"写时快照内的显示派生值"，则需要 §10 的命中重派生而不是失效（否则白丢命中）。
- **不要与关闭清除混淆**：`settings.js:672-674` 是 `applyEnableResultCacheSetting`（`settings.js:666`）内"缓存从开启切到关闭时清一次缓存"，语义是停用前清理残留；`settings.js:833-850` 才是运行期失效列表。前者是设置本身的副作用（settings-pipeline.md §8），后者是任何计算设置变化的统一失效点。

## 10. 命中恢复（actualEstimatorAlgorithm + 显示派生重算）

命中时快照直接恢复全部结果（`analysis.js:429-438` cached 分支）：

- `analysis.js:431` `state.actualEstimatorAlgorithm = cached.actualEstimatorAlgorithm`——**从快照恢复，绝不重算**。Azusa/Roxy 因谱面被拒而回退 Sunny 的判定结果存在快照里（写入侧 `analysis.js:769` `actualEstimatorAlgorithm: state.actualEstimatorAlgorithm`），命中时直接还原。
- 其余恢复项：`rework`（:430）、`resolvedEstDiff`/`resolvedNumericDifficulty`/`resolvedNumericDifficultyHint`（:432-434）、`lnStar`/`state.lnStar`（:436-437）、`typePercentageData`（:438）。

**命中重派生**（task 13）：快照内有两个字段是"写时刻的显示派生值"——`sixKConst` 与 `mergedClusters`——它们依赖写时刻的 `display6kLevel`/`debugUseAmount`。这两个设置已移出失效列表（§9），命中时必须按当前设置重派生：

- **sixKConst**（`analysis.js:439-446`）：不再直接取 `cached.sixKConst`，而是按 `runAnalysisPipeline` §6 同公式从缓存数据重算——`state.display6kLevel && columnCount === 6 && star 有限且 > 0` 时 `Math.round((star * 200 / 81 + 7 / 6) * 100) / 100`，否则 `null`。公式与 pipeline 逐字一致（6K 下 rework.star 恒为 Sunny sr），已验证与 pipeline 输出位级相同；4K 恒 null。这样"写时开→命中时关"置 null、"写时关→命中时开"按 star 补算。
- **mergedClusters + debugUseAmount 后处理**（`analysis.js:604-632`）：缓存里的 `mergedClusters` 是写时刻（可能已排序/覆盖过 Category 的）结果。命中时从 `cached.patternReport?.Clusters`（与 miss 路径同一原始来源）重放 `mergeDuplicateClusters`（display.js 纯数据字段，JSON-safe），再对**当前** `debugUseAmount` 重放 `applyDebugUseAmountPostProcess(clusters, report)`（按 Amount 降序排序 + 用首位 cluster 的 SpecificTypes[0]/Pattern 覆盖 `patternReport.Category`）——hit/miss 共用同一辅助函数。

读展示层时始终用 `state.actualEstimatorAlgorithm`（用户选择仍留在 `state.estimatorAlgorithm`）。

## 11. 注意事项

- **缓存键不含** `display6kLevel`、`extendedEstimationRange`、`forceSunnyWindow`、etterna 版本、debug 标志等——正确性完全依赖 §9 的失效列表 + §10 的命中重派生。任何新计算影响设置漏加失效 = 静默过期结果；任何"显示派生值进了快照"的新设置，要么加失效、要么按 §10 模式实现命中重派生（toggle-diff 实证差异再决定）。
- **关闭清除**：`enableResultCache` 从开切关时立即 `clearResultCache()`（`settings.js:672-674`），防残留快照在重新开启后命中旧数据。
- **JSON-safe 契约**：get 与 put 双向 deepClone（§2），值含函数/Date/带方法对象会炸——cluster 等必须走 jsonSafe 剥壳（`analysis.js:750`）。
- **deepClone 成本**：每次 get/put 各克隆一次，命中快照较大时（含 graph 数据）有开销；这是"只读快照、防外部篡改"的取舍。
- **`has()` 也有 touch 副作用**（§2）：任何命中检查都会改变 LRU 顺序。
- **meta 降级谱面缓存不生效**（§8）：性能上可接受，正确性上必须——标题键无法保证唯一。
- **generation 是失效探测器**：`clearResultCache()`（代数 +1）是唯一让写门 `genAtStart === resultCacheGeneration()` 失败的路径（§7）；调 `clear()` 手动重置场景等同理。
- **Node 环境**：`resultCache.js` 无 import，benchmark runner 的 `smoke-result-cache.mjs` 直接 `import` 它跑 8 个冒烟用例，修改本模块后建议跑一遍该用例保持 Node 侧兼容。
