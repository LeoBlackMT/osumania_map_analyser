# docs/guides/cache-invalidation.md — 结果缓存失效决策指南

> 面向 AI 的指南文档。文中所有 `path:line symbol` 引用均相对本仓库根目录（插件文件夹名为 `ManiaMapAnalyser by Leo_Black`，含空格，路径引用必须精确）。文中 path:line 行号为编写时快照，代码演进后可能漂移；定位源码请以符号名（symbol）为准，必要时用 grep 复核。
> 相关文档：[pipeline/result-cache.md](../pipeline/result-cache.md)（缓存机制详解——那篇是"怎么工作的"，本文是"该不该加失效"的决策指南）、[adding-a-setting.md](adding-a-setting.md)（新增设置项完整流程）、[pipeline/settings-pipeline.md](../pipeline/settings-pipeline.md)（设置管线，含失效触发）。
> 使用场景：**新增或修改任何设置项时**，先读本文 §3 判定清单，再决定是否加入失效列表；**需要把新的分析产物写入缓存时**，读本文 §10 操作步骤。

## 1. 核心规则（一句话）

新增**计算影响**设置 → 必须加入失效列表（`settings.js:835-849` 的 if 块，最终 `clearResultCache()` 调用在 `settings.js:849`）；
新增**显示类**设置 → 禁止加入（显示需求差异由覆盖检查兜住，`analysis.js:311-314`）。

```
新增设置
 ├─ 计算影响 → 加入失效列表（settings.js:835-849）＋ 挂进 changed/recomputeNeeded 链（settings.js:776-831）
 └─ 显示类   → 什么都不做（覆盖检查已兜住）
```

## 2. 为什么：缓存键不含这些设置

缓存键是版本前缀 + 三段（`analysis.js:305`）：

```js
const cacheKey = `${CACHE_KEY_STAR_UNIFIED_VERSION}|${state.estimatorAlgorithm}|${state.lastBeatmapIdentity}|${state.modSignature}`;
```

即 `star-v2|算法|谱面身份|mod 签名`（`CACHE_KEY_STAR_UNIFIED_VERSION` 常量是星数统一语义的缓存版本前缀，改星数口径时 bump 它以作废旧快照）。**不含** `display6kLevel`、`extendedEstimationRange`、`forceSunnyWindow`、etterna 版本、debug 标志等一切其余设置（详见 result-cache.md §5、§11 注意事项）。

键设计的取舍：键保持最小 → 无关设置切换不会误伤命中；代价是正确性完全委托给失效列表。漏加失效 = 设置变了但键没变 → 静默命中旧快照：

```js
// 用户把 display6kLevel 从 4 切到 3（state.display6kLevel 已更新，键不含它）
const cacheKey = "Azusa|hash:1a2b…|1.00000000|none|none"; // 与切换前完全相同！

const snapshot = resultCache.get(cacheKey); // 命中！
// 覆盖检查（analysis.js:311-314）也通过——计算需求四项没变
// → 直接渲染快照：快照里还是 display6kLevel=4 时代算出的 6K 结果
// → 用户看到的是旧设置下的分析，无任何报错（静默陈旧）
```

失效流程（设置变化时）：

```
tosu 下发设置 → settings.js 命令监听回调（settings.js:714）
 ├─ 命中 14 项失效条件之一 → clearResultCache()（settings.js:849）
 │    → 代数 +1（resultCache.js:51）→ 下次分析 get() miss → 全量重算
 │    → 写门前校验代数（analysis.js:746 genAtStart === resultCacheGeneration()），拒绝过期分析写回
 └─ 显示类设置 → 不动缓存 → 下次分析 get() 命中
      → 覆盖检查（analysis.js:311-314）比较 needComputed 四项
      ├─ 计算需求变了 → miss → 重算
      └─ 计算需求没变 → 命中 → 渲染时从 state 即时读新值
```

## 3. 判定清单：计算影响 vs 显示类

判定的本质：**该设置的效果在什么时候被解析**。

- **固化进快照**（分析时写入，`analysis.js:751-776` 的任一字段）→ 计算影响 → 必须失效。
- **渲染时从 state 即时读取**（不进快照）→ 显示类 → 禁止失效。

| 判定问题 | 是 → | 否 → |
| --- | --- | --- |
| 改变估算结果（算法选择/参数/版本/扩展范围）？ | 计算影响 | 显示类 |
| 改变解析/键型分析/模式判定（RC/LN/SV）内容？ | 计算影响 | 显示类 |
| 改变 Etterna MSD 计算（版本/调用方式）？ | 计算影响 | 显示类 |
| 检测开关（SV/vibro/LN）——产物是否固化进快照？ | 计算影响（见 §5 第 6、7、11 项） | 显示类 |
| 只改渲染/布局/颜色/可见性/文字（cardOpacity/主题/字体/卡片方向等）？ | — | 显示类 |
| 其效果是否完全在渲染阶段由当前 state 解析、快照里没有对应字段？ | — | 显示类 |
| 设置的值被写进快照（如 `isVibroMap` `analysis.js:767`、`sixKConst` `analysis.js:768`）？ | 计算影响 | — |

### 边界案例

- **enableNumericDifficulty（显示类，不加失效）**：解析在 `settings.js:748`，只挂在 `changed` 链（`settings.js:791`），不在 `recomputeNeeded`（`settings.js:812-831`），也不在失效列表。原因：数值难度总随 rework 一并计算并存进快照（`analysis.js:754-756` `resolvedNumericDifficulty*`），该设置只决定字幕是否渲染它（且只在 `diffText=Difficulty` 时可见）。渲染时读 state 即可，快照内容不受影响 → 归显示类（settings.js:861-862 注释 "Caption-only changes ... are applied immediately"）。
- **vibroDetection（名义检测，但必须失效）**：看起来像"检测开关"，但 `isVibroMap` 是**逐谱面分析结果**，被固化进快照（`analysis.js:767` `isVibroMap`），渲染段读的是快照值。关掉开关后快照里仍是旧 `isVibroMap=true` → 必须失效重算。教训：**"开关"不等于"显示类"——看产物是否进快照，不看名字**。

## 4. 覆盖检查如何兜住显示类差异

`needComputed` 是"本次分析需要哪些计算产物"的布尔集（`analysis.js:287-304`），四项：`pattern`（键型）、`ett`（Etterna MSD）、`graph`（难度图）、`interlude`（Interlude 星数）。写入时随快照保存（`analysis.js:775` `computed: needComputed`），命中时逐项比对（`analysis.js:311-314`）：

```js
snapshot.computed.graph === needComputed.graph
&& snapshot.computed.pattern === needComputed.pattern
&& snapshot.computed.ett === needComputed.ett
&& snapshot.computed.interlude === needComputed.interlude
```

- contentBar/srText/diffText 是**特殊显示类**：它们被织入 needComputed（`analysis.js:288-303`，如 `state.diffText === "Graph"` → graph、`state.srText === "MSD"` → ett）。改显示需求但没改计算需求 → 命中（如 diffText 在 "Difficulty" 类选项间切换）；改计算需求（如切到 "Graph"）→ 检查自动判 miss 重算。因此它们不在失效列表。
- 纯显示设置（cardOpacity、主题、字体……）根本不进 needComputed，也不进快照 → 命中后渲染时从 state 取新值，天然正确。
- 注意 `needComputed` 用 fetch 前的保守值（`analysis.js:284-286` 注释：未经谱面级 `effectiveContentBar` override），仅用于覆盖检查；实际 shows*/need* 在执行块内 override 后重算（`analysis.js:362-365`）。

## 5. 完整失效列表（14 项）与每条为何失效

if 块条件在 `settings.js:835-848`，`clearResultCache()` 调用在 `settings.js:849`。以下 14 项任一为真 → 全缓存清空（`resultCache.js:49-52 clear`，代数 +1）。

| # | 条件变量（settings.js:835-848） | 设置 | 为何影响计算结果 |
| --- | --- | --- | --- |
| 1 | `estimatorChanged` | estimatorAlgorithm | 算法选择。注：算法名已在缓存键中（`analysis.js:305` 第一段），失效属冗余保险（防未来键改动的 belt-and-suspenders） |
| 2 | `azusaSunnyReferenceHoChanged` | azusaSunnyReferenceHo | 改变 Azusa 的 Sunny 参考阈值 → 改变谱面被接受/回退的判定 → 改变实际结果与 `actualEstimatorAlgorithm` |
| 3 | `etternaVersionChanged` | etternaVersion | 不同 MinaCalc 版本算出的 MSD 不同（快照含 `ettResult`） |
| 4 | `companellaEtternaVersionChanged` | companellaEtternaVersion | Companella 自带 Ett 版本，同上影响 MSD |
| 5 | `debugChanged` | debugUseAmount | 调试分类标志，改变分析明细的分类结果（固化进快照） |
| 6 | `svChanged` | useSvDetection | SV 检测覆盖模式判定为 SV → 改变 `needComputed.pattern`（`analysis.js:291`）与存储的模式标签 |
| 7 | `vibroChanged` | VibroDetection（uniqueID 大写 V，state 字段 `state.vibroDetection`） | `isVibroMap` 固化进快照（`analysis.js:767`）；且影响 `needComputed.pattern/ett`（`analysis.js:292`、:297） |
| 8 | `wsEndpointChanged` | wsEndpoint | 数据源变化（lazer/stable 切换、端口变更）→ 同一身份下谱面内容/计算上下文可能不同。它只在 `changed` 不在 `recomputeNeeded`，故显式列出（`settings.js:834` 注释） |
| 9 | `forceSunnyWindowChanged` | forceSunnyWindow | 强制 SunnyWindow LN 覆盖 → 改变实际执行与结果 |
| 10 | `enableLNDifficultyChanged` | enableLNDifficulty | 控制 LN 难度计算 → 改变 `lnStar`/estDiff（快照字段 `analysis.js:760`、:436-437） |
| 11 | `enableAnalyzeLNChanged` | enableAnalyzeLN | LN 分析开关 → 改变键型分析内容与 `needComputed` |
| 12 | `enableAlwaysShowLNDifficultyChanged` | enableAlwaysShowLNDifficulty | 控制 LN 难度是否常算/常显 → 改变快照内 LN 结果 |
| 13 | `display6kLevelChanged` | display6kLevel | 改变 6K 恒定等级计算 → `sixKConst` 固化进快照（`analysis.js:768`、:435），且影响 6K estDiff |
| 14 | `extendedEstimationRangeChanged` | extendedEstimationRange | Sunny 家族使用扩展星数表 → 改变 estDiff（选项传入估算器 `analysis.js:446`） |

**共同点**：除第 1 项外全部**不在缓存键中**，且都改变快照 `analysis.js:751-776` 里存储的字段。第 6、7、11 项同时改变"算不算"（needComputed）与"存什么"（快照字段）。

## 6. 反例警示：显示类设置加入失效列表的代价

假设把 `cardOpacity` 加进失效列表：用户在 tosu 设置里拖动透明度 → 每次触发 `clearResultCache()`（`settings.js:849`）→ 全缓存清空 → 下一张谱面分析全量重算（fetch .osu + 解析 + 估算器 + WASM）。

- **性能退化**：调一下透明度，缓存里所有谱面的分析作废；每张谱面重算耗时数百毫秒到数秒，切换谱面/切歌时体验劣化。
- **浪费计算**：`clear()` 代数 +1（`resultCache.js:51`）后，正在进行的分析会在写门被拒（`analysis.js:746`），一次完整的计算白跑。
- **毫无收益**：cardOpacity 渲染时从 state 即时读取，快照里没有对应字段——失效不会让显示更正确，只是白白丢命中。

判定口诀：**加失效的唯一理由是该设置改变快照内容；只改变渲染输出的设置，失效是纯损耗。**

## 7. 命中恢复陷阱：actualEstimatorAlgorithm 勿重算

命中快照时直接恢复全部结果（`analysis.js:429-438` cached 分支）。其中：

`analysis.js:431`：`state.actualEstimatorAlgorithm = cached.actualEstimatorAlgorithm`——**从快照恢复，绝不重算**。

- Azusa/Roxy 因谱面特征被拒而回退 Sunny 的判定结果存在快照里（写入侧 `analysis.js:769`），命中时必须原样还原。
- 用户选择仍在 `state.estimatorAlgorithm`（缓存键第一段），读展示层始终用 `state.actualEstimatorAlgorithm`。
- 在 cached 分支里重新跑"当前算法是否接受该谱面"的逻辑 = 覆盖检查做过的活重做一遍，且结果可能与快照矛盾（快照来自当时的算法设置）。

## 8. 与"关闭清除"区分

| | 关闭清除（settings.js:672-674） | 失效列表（settings.js:835-849） |
| --- | --- | --- |
| 位置 | `applyEnableResultCacheSetting`（`settings.js:666`）内 | 命令监听回调内 |
| 触发条件 | `enableResultCache` 从**开切关**（`changed && wasEnabled && !next`） | 任一计算影响设置变化 |
| 语义 | 停用前清理残留，防重新开启时命中旧数据 | 运行期统一失效点 |
| 频率 | 仅开关切换那一次 | 每次计算相关设置变化 |

两者都调 `clearResultCache()`（`settings.js:673` 与 `settings.js:849`，函数定义 `resultCache.js:68`），但**不要**把关闭清除当成失效列表的替代品，反之亦然。

## 9. 新增设置工作流回顾

新增/修改设置项的完整流程（settings.json → 解析器 → state → 挂监听链 → 失效决策）见 [adding-a-setting.md](adding-a-setting.md) 第 6 步。本指南只负责其中一环：**判断新设置是否计算影响，决定加入失效列表或交给覆盖检查**。落入失效列表时，还需同时挂进 `changed`/`recomputeNeeded` 链（`settings.js:776-831`），否则监听回调根本感知不到该设置变化——先有感知，才有失效。

## 10. 如何将数据写入缓存（操作指南）

> 本文其余章节回答"该不该失效"，本节回答"怎么写进缓存"。机制细节（LRU 语义、deepClone、写门逐条解释）见 [pipeline/result-cache.md](../pipeline/result-cache.md) §2/§3/§7——本节只给操作步骤，不重复机制。
> 写入的**唯一入口**是 `analysis.js:743-776` 的写门块：位于 `fetchBeatmapFile`（`analysis.js:248`）内、companella 完成后、SV/auto-profile 段之前（`analysis.js:741-742` 注释）。新产物一律在该块内加字段，不要另起 `resultCache.put` 调用。

### 10.1 何时写入：写门条件（`analysis.js:743-746`）

```js
if (!cached && state.enableResultCache && state.lastBeatmapIdentity
    && errors.length === 0
    && rework && !isStaleRequest()
    && genAtStart === resultCacheGeneration()) {
```

| 条件 | 含义 |
| --- | --- |
| `!cached` | 必须是 miss——命中快照本来就在缓存里，无需再写 |
| `state.enableResultCache` | 开关开启 |
| `state.lastBeatmapIdentity` | 谱面身份存在（否则缓存键不完整，§10.5） |
| `errors.length === 0` | 分析全程无错误（`errors` 数组定义于 `analysis.js:388`） |
| `rework && !isStaleRequest()` | 估算结果真实存在，且请求未过期（`isStaleRequest` 定义于 `analysis.js:251`） |
| `genAtStart === resultCacheGeneration()` | **代数守卫**：`genAtStart` 捕获于分析开始（`analysis.js:252`），期间 settings.js 调了 `clearResultCache()`（代数 +1，`resultCache.js:51`）则拒绝写回，防 clear 后旧分析污染新缓存 |

这个 if 块就是最小模板：**在块内添加的新字段自动继承全部守卫**，不需要重复判断。

### 10.2 写入什么：快照字段清单（`analysis.js:751-776`）

| 字段 | 位置 | 说明 |
| --- | --- | --- |
| `rework` | `analysis.js:752-762` | 估算结果子对象：star/estDiff/numericDifficulty/numericDifficultyHint/graph/lnRatio/columnCount/lnStar/typePercentageData |
| `patternReport` | `analysis.js:763` | 键型分析报告，jsonSafe 包装（§10.3） |
| `mergedClusters` | `analysis.js:764` | 聚类结果，jsonSafe 包装（§10.3） |
| `ettResult` | `analysis.js:765` | Etterna MSD 结果 |
| `interludeStar` | `analysis.js:766` | Interlude 星数 |
| `isVibroMap` | `analysis.js:767` | vibro 检测结果——固化进快照，故 vibroDetection 设置必须失效（本文 §5 第 7 项） |
| `sixKConst` | `analysis.js:768` | 6K 恒定等级——同理 display6kLevel 必须失效（§5 第 13 项） |
| `actualEstimatorAlgorithm` | `analysis.js:769` | 实际执行的算法（含 Azusa/Roxy 回退 Sunny 的记录），命中时原样恢复（§10.7） |
| `parsedInfo` | `analysis.js:770-774` | 只存 metadata/lnRatio/columnCount 三个普通字段 |
| `computed` | `analysis.js:775` | `needComputed` 快照，供命中覆盖检查（§10.6） |

### 10.3 jsonSafe 剥壳（`analysis.js:750`）——最容易踩的坑

clustering.js 的 cluster 对象带 `format()`/`Importance` 方法，`structuredClone` 无法拷贝；而 resultCache 的 get/put 双向 deepClone 要求值必须 JSON-safe（`resultCache.js:6-13`，详见 result-cache.md §2/§7）。因此：

```js
const jsonSafe = (value) => (value == null ? value : JSON.parse(JSON.stringify(value)));
```

需要 jsonSafe 的：`typePercentageData`（`analysis.js:761`）、`patternReport`（:763）、`mergedClusters`（:764）。纯值字段（ettResult/interludeStar/isVibroMap/sixKConst 等）不需要。**规则：字段来源是类实例或带方法的对象 → 先 jsonSafe；拿不准 → 包一层无害。**

### 10.4 skip:isMetaDegraded（`analysis.js:776`）

```js
}, { skip: isMetaDegraded });
```

`isMetaDegraded` 在 `analysis.js:306` 判定（identity 以 `meta:` 开头）。meta 降级身份的快照**永不写入**：标题键碰撞风险、无 md5 无法检测文件替换（见 result-cache.md §8）。`skip:true` 时 `resultCache.put` 直接返回（`resultCache.js:35`），不写入、不占容量、不驱逐。**新写缓存代码必须带上这个 skip 参数**，漏了会让 meta 身份的快照污染 LRU。

### 10.5 缓存键构造（`analysis.js:305`）

```js
const cacheKey = `${CACHE_KEY_STAR_UNIFIED_VERSION}|${state.estimatorAlgorithm}|${state.lastBeatmapIdentity}|${state.modSignature}`;
```

版本前缀 + 三段：缓存语义版本（`star-v2`，星数统一后作废旧快照）| 用户选择的算法 | 谱面身份（含 md5） | mod 签名（`speedRate|odFlag|cvtFlag`）。**写前确认三段都在**——写门已校验 `state.lastBeatmapIdentity` 存在；直接用 fetchBeatmapFile 开头构造好的 `cacheKey` 变量，不要自己重造键（键不含任何其他设置，正确性依赖失效列表，见 §2）。

### 10.6 needComputed 推导与随快照保存（`analysis.js:775`、:287-304、:310-314）

- 写入时：`computed: needComputed` 保存"本次分析需要哪些计算产物"（pattern/ett/graph/interlude 四项布尔，推导见 `analysis.js:287-304`）。
- 命中时：逐项比对 `snapshot.computed` 与当前 `needComputed`（`analysis.js:310-314`），四项全等才命中。
- **新计算产物要接入覆盖检查** → 三处同步改：`needComputed` 加项（`analysis.js:287-304`）＋命中比对加项（:310-314）＋写门块 `computed` 一起存（:775）。

### 10.7 命中恢复：写入与恢复成对（`analysis.js:429-438`）

命中分支从快照恢复全部结果：`rework`（:430）、`actualEstimatorAlgorithm`（:431，**从快照恢复，绝不重算**——写入侧在 `analysis.js:769`）、`sixKConst`（:435）、`lnStar`（:436-437）、`typePercentageData`（:438）。**新增字段必须同时出现在写门块（:751-776）与命中恢复分支（:429-438）**，否则命中后该字段缺失/undefined——这是最常见的成对遗漏。

### 10.8 常见遗漏检查清单

写缓存前逐条自查：

- [ ] **忘 jsonSafe**：带方法/类实例的对象（cluster 等）直接写入 → deepClone 抛错（§10.3）
- [ ] **忘 skip:isMetaDegraded**：meta 身份快照写进 LRU → 标题碰撞污染（§10.4）
- [ ] **写入非 JSON-safe 值**：函数、Date、`undefined` 顶层字段
- [ ] **在 stale 请求中写入**：generation 守卫（`analysis.js:746`）只保护写门块内——块外另起 put 会绕过守卫
- [ ] **新字段只写不恢复**：命中分支（`analysis.js:429-438`）没恢复该字段（§10.7）
- [ ] **新计算产物没进 needComputed**：覆盖检查（:310-314）覆盖不到 → 需求变化仍命中旧快照（§10.6）
- [ ] **新计算影响设置没进失效列表**：键不含该设置 → 静默陈旧，见 §5（`settings.js:835-848` 条件、`settings.js:849` 调用）

### 10.9 新增功能时的操作步骤（checklist）

面向 LLM 的流程：

1. **新计算结果要进缓存** → 在写门块（`analysis.js:743-776`）put 对象内加字段（带 jsonSafe 与 skip 处理）＋ 命中分支（`analysis.js:429-438`）恢复该字段；若该产物参与显示需求判定，还需同步 §10.6 的三处覆盖检查位置。
2. **新计算影响设置** → 加入失效列表（`settings.js:835-848`，`clearResultCache()` 在 `settings.js:849`）＋ 挂进 `changed`/`recomputeNeeded` 链（`settings.js:776-831`），见本文 §5、§9。
3. **新显示类设置** → 不失效、不进缓存，由覆盖检查兜住（本文 §4）。

> 验证口诀：写入块字段数 = 命中恢复块字段数；jsonSafe 覆盖所有对象字段；skip 参数永远带着。
