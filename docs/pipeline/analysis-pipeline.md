# docs/pipeline/analysis-pipeline.md — 分析管线总览

> 面向 AI 的管线技术文档。文中所有 `path:line symbol` 引用均相对本仓库根目录（插件文件夹名为 `ManiaMapAnalyser by Leo_Black`，含空格，路径引用必须精确）。
> 相关文档：[result-cache.md](result-cache.md)（结果缓存：缓存键/覆盖检查/写门/失效，本文只概述并交叉引用）、[settings-pipeline.md](settings-pipeline.md)（设置管线：设置注入、失效触发、getSettings 命令）、[mod-handling.md](mod-handling.md)（mod 解析与 modSignature）、[../features/difficulty-estimation.md](../features/difficulty-estimation.md)（6 个估算算法各自的详细说明）。

## 1. 全链路数据流图

```
tosu（osu!mania 进程内）
  │  WebSocket v2 数据包（游戏状态/谱面/mod/时间）
  ▼
socket.js:93 WebSocketManager ── appContext.js:178 socket（api_v2 / commands 双通道）
  │  socket.js:60 api_v2(callback)  →  /websocket/v2    ← 数据（谱面/mod/状态）
  │  socket.js:64 commands(callback) →  /websocket/commands ← 命令（getSettings 等）
  ▼
socketHandlers.js:145 setupSocketListener（api_v2 回调，socketHandlers.js:146）
  ├─ socketHandlers.js:47 updateSongTimeState —— 歌曲时间/暂停检测/图表光标（graph.js:460 updateGraphCursor）
  ├─ socketHandlers.js:195-253 身份构建
  │    ├─ beatmapIdentity（id:/hash:/path: 组合，缺全部时降级 meta:，:236-250）
  │    ├─ songKey（set:/dir:/meta:，:224-230，用于区分换歌 vs 换难度）
  │    └─ modSignature（socketHandlers.js:257-261 应用 modData.js:62 getModData 的结果）
  ├─ socketHandlers.js:278-285 changeKind 判定（song / difficulty / mod）
  ├─ socketHandlers.js:296-298 applyCoverThemeForBeatmap（coverTheme.js:46，异步取色，失败不阻塞）
  ├─ socketHandlers.js:303 socket.sendCommand("getSettings", ...)（仅首次，settings.js:865-868）
  └─ socketHandlers.js:305 scheduleRecompute("beatmap/mod changed", true)
       ▼
scheduler.js:9 scheduleRecompute —— 200ms 防抖（config.js:70 socketRecalcLazyDelayMs，scheduler.js:22-23）
       ▼
main.js:13 setRecomputeHandler(fetchBeatmapFile) → analysis.js:248 fetchBeatmapFile(reason)
       │
       ├─ [1] 请求序号：analysis.js:249-251 requestSeq / isStaleRequest
       │        （appContext.js:149 analysisRequestSeq 自增，过期请求一律 return）
       ├─ [2] 缓存查找：analysis.js:308-317 —— 命中走快照（:322-331/:429-438），详见 result-cache.md
       ├─ [3] miss → fetch .osu：analysis.js:333-336 fetch(getEndpoint())（config.js:2 endpoint）
       ├─ [4] 解析：analysis.js:349 parseMetadataFromBeatmap → osuFileParser.js:19 OsuFileParser
       ├─ [5] 估算分派：analysis.js:465-511
       │     ├─ Worker：analysis.js:466 runInWorker（manager.js:41）→ compute.worker.js:17 分派
       │     │         （Sunny/Daniel/Azusa/Roxy；Azusa/Roxy 无效结果回退 Sunny）
       │     ├─ 主线程：Mixed（mixedEstimator.js:192）/ Companella（companellaEstimator.js:181，ONNX）
       │     └─ 回退：worker 不可用时 manager.js:43 返回 null → 主线程同步执行
       ├─ [6] 附属计算（各自独立 try/catch，错误入 errors[]，analysis.js:388）
       │     ├─ Interlude：analysis.js:590 calculateInterludeStar（interlude/index.js:14）
       │     ├─ Pattern：analysis.js:606 analyzePatternFromText（patterns/service.js:4）
       │     ├─ Etterna：analysis.js:647 analyzeEtternaFromText（ett/index.js:36）
       │     └─ Companella：analysis.js:710 classifyCompanellaDifficulty（4K 追加，见 §7.5）
       ├─ [7] 写缓存：analysis.js:743-746 写门（5 条件）→ analysis.js:751 resultCache.put
       ├─ [8] 渲染出口（§10）
       │     ├─ display.js：renderPatternClusters :650 / renderEtternaSkillBars :683 / showNumericStarValue :421 ...
       │     ├─ graph.js：renderDiffGraph :545 / updateGraphCursor :460 / setNumericDifficultyValue :720 ...
       │     ├─ hud.js：setModeTag :143 / setModeTagAdvanced :171 / setStatus :126 / showOverlay :304 ...
       │     └─ analysis.js:887 renderRightCapsule / :896 renderFullModeSeparators
       └─ [9] 收尾：analysis.js:902-917 状态行（formatMetadataStatus display.js:776）；finally 移除 loading
```

## 2. 入口装配（main.js）

`main.js:15 initialize()` 是唯一启动入口（被 `index.html` 的模块脚本调用）：

1. `main.js:17 loadSettings()`——先读 settings.json 基线再注册命令监听（详见 settings-pipeline.md §2）。
2. `main.js:13 setRecomputeHandler(fetchBeatmapFile)`——把 `analysis.js:248 fetchBeatmapFile` 挂为调度器的重算处理器，这是**所有分析触发的唯一入口**（换歌/换难度/改 mod/改设置都经 `scheduler.js:9 scheduleRecompute` 到达这里）。
3. `main.js:22 setupSocketListener()`——注册 api_v2 数据回调。
4. `main.js:23 scheduleRecompute("initial load", false)`——启动后立即触发首次分析（不防抖，`useLazyDelay=false` 立即执行）。

## 3. WebSocket 数据接入（socket.js）

`socket.js:1 class WebSocketManager`（`socket.js:93` 默认导出，实例为 `appContext.js:178 socket`）维护两条连接：

| 通道 | 方法 | 端点 | 用途 |
| --- | --- | --- | --- |
| 数据 | `socket.js:60 api_v2(callback)` | `/websocket/v2` | 游戏状态/谱面/mod/时间，socketHandlers.js 的输入 |
| 命令 | `socket.js:64 commands(callback)` | `/websocket/commands` | 发 `getSettings` 等命令（`socket.js:68 sendCommand`） |

细节：`socket.js:28 createConnection` 建立连接并带 1s 重连（:43）；`socket.js:46-54 onmessage` 解析 JSON 后调 callback，`data.error` 直接丢弃；`socket.js:32` 连接 URL 带 `?l=` 计数器路径参数；`sendCommand` 在命令通道未就绪时 100ms 重试、失败 1s 重试至多 3 次（:68-90）。

## 4. 身份与 mod 构建（socketHandlers.js）

`socketHandlers.js:146 socket.api_v2((data) => ...)` 的回调体 `socketHandlers.js:145 setupSocketListener` 里，先处理游玩状态切换（:147-164）、mod 数据（:166-169）、歌曲时间（:171 → `socketHandlers.js:47 updateSongTimeState`），然后才进入身份构建（:173 起，`data?.beatmap` 缺失直接 return）。

### 4.1 beatmapIdentity：id:/hash:/path: 组合 + meta: 降级

`socketHandlers.js:236-250` 按可用性拼接三段（每段缺失则跳过）：

```js
const identityParts = [];
if (beatmapId)   identityParts.push(`id:${beatmapId}`);          // :237-239
if (beatmapHash) identityParts.push(`hash:${beatmapHash}`);      // :240-242
if (beatmapPath) identityParts.push(`path:${beatmapPath}`);      // :243-245
// 三段全缺才降级 meta:
if (identityParts.length === 0 && hasMetadataIdentity) {
    identityParts.push(`meta:${beatmapTitleKey}`);               // :247-250
}
const nextBeatmapIdentity = identityParts.join("|");             // :252
if (!nextBeatmapIdentity) return;                                // :253 空身份直接放弃
```

- **md5 免疫文件替换**：`hash:` 段来自 `beatmap?.md5 || beatmap?.checksum`（:196），谱面文件内容变化 → md5 变 → identity 变 → 缓存键变，天然免疫文件替换（这是缓存键安全性的基石，见 result-cache.md §5）。
- **meta: 降级**：仅当 tosu 同时缺少 id/hash/path 时发生（:248）。此时身份只有标题元数据（artist::title::version::mapper 小写拼接，:198-203）——**更弱**：同标题不同谱面会共用同一键（碰撞风险），且无 md5 无法检测文件替换。对应处理：`analysis.js:306 isMetaDegraded` 判定，缓存侧永不写入（result-cache.md §8）。
- 归一化：id 取正整数（`normalizeNumberText` :187-193，`Math.trunc` 去小数）；path 反斜杠转正斜杠、折叠重复斜杠、小写（:181-185）；hash 小写（:196）。
- `socketHandlers.js:290-292 lastBeatmapIdentitySource`：记录身份是 composite（≥2 段）还是单段来源，供展示层区分。

### 4.2 songKey：区分"换歌" vs "换难度"

`socketHandlers.js:224-230`：

```js
const songKeyParts = [];
if (beatmapSetId)     songKeyParts.push(`set:${beatmapSetId}`);      // :225
if (beatmapFolderPath) songKeyParts.push(`dir:${beatmapFolderPath}`); // :226
if (songKeyParts.length === 0 && songMetaKey...) songKeyParts.push(`meta:${songMetaKey}`); // :227-229
const nextSongKey = songKeyParts.join("|");                          // :230
```

- **set:** 用 mapset id（:208 `beatmap?.set || beatmap?.setId || beatmap?.beatmapSetId`）。
- **dir:** 用谱面目录（:209-218 优先 directPath 的背景/音频文件路径，退而求其次取谱面文件所在目录）。
- **meta:** 是 songKey 的最后回退，只含 artist::title::mapper（:219-223，**不含 version**，所以同一歌的不同难度共享一个 songKey）。
- 关键性质（:205-207 注释）：songKey 不含 version/md5/id/path——**同一 mapset 内切难度 songKey 不变**，可据此区分"换歌"（mapset 变了）与"换难度"（mapset 没变）。

### 4.3 changeKind：song / difficulty / mod

`socketHandlers.js:278-285`：

```js
const identityChanged = nextBeatmapIdentity !== previousBeatmapIdentity;
let changeKind = "mod";
if (identityChanged) {
    const songChanged = !previousSongKey || !nextSongKey || nextSongKey !== previousSongKey;
    changeKind = songChanged ? "song" : "difficulty";
}
state.pendingChangeKind = changeKind;   // :286
```

判定逻辑：

- identity 没变（谱面难度与 mod 都没变）→ 什么都不做，提前 return（:265 `hasStateMismatch` 检查，:263-265）。
- identity 变了 + songKey 变了 → `song`（换歌，或首次加载：任一 songKey 为空即视为换歌）。
- identity 变了 + songKey 没变 → `difficulty`（同 mapset 内切难度）。
- identity 没变但 mod 变了 → `mod`（identityChanged=false 时 changeKind 保持初值 "mod"）。

消费端在 `analysis.js:258-263`：取 `state.pendingChangeKind` 并清空（避免后续纯改设置的 recompute 误用上一次的换歌动画）；没拿到种类时按 reason 推断（"initial load" → song，其余 → difficulty）；结果写入 `state.activeChangeKind` 供 `display.js:408 playStarBlockEntrance(changeKind)` 选择入场动画（换歌整块入场，换难度轻量过渡）。

### 4.4 mod 状态应用与 modSignature

- api_v2 包可能不完整（partial），因此 mod 状态**只在 mod payload 显式出现时应用**：`socketHandlers.js:257-261 shouldApplyModState = !previousModSignature || (modData.hasModPayload && (modData.hasModInfo || modData.hasExplicitNoMod))`；不满足时沿用旧 modSignature。
- 应用侧 `socketHandlers.js:267-272`：写入 `state.speedRate / state.odFlag / state.cvtFlag / state.modSignature`（来源 `modData.js:62 getModData`，解析细节见 mod-handling.md）。
- **modSignature 不参与换歌判定**，只进缓存键：`analysis.js:305` 缓存键 = `estimatorAlgorithm|beatmapIdentity|modSignature`（构成见 modData.js:218-222，`speedRate|odFlag|cvtFlag`，详见 result-cache.md §5 与 mod-handling.md）。

## 5. 请求调度（scheduler.js）

`scheduler.js:9 scheduleRecompute(reason, useLazyDelay)`：

- 防抖：`state.recalcTimerId`（appContext.js:144）非空则 `clearTimeout` 并重置（:10-13）——连发的 socket 事件只保留最后一次。
- `useLazyDelay=true` 时 `setTimeout(run, SOCKET_RECALC_LAZY_DELAY_MS)`（:22-23），即 **200ms**（config.js:70 `socketRecalcLazyDelayMs`，经 appContext.js:175 `SOCKET_RECALC_LAZY_DELAY_MS` 导入）。
- 延迟结束后调 `recomputeHandler(reason)`（:15-20），即 `analysis.js:248 fetchBeatmapFile`（main.js:13 注册）。
- 调用方：换歌/换难度/改 mod → `useLazyDelay=true`（socketHandlers.js:305）；改设置 → 同样走懒延迟（settings.js:859 `scheduleRecompute("settings changed", true)`）；启动首分析 → `false` 立即执行（main.js:23）。

## 6. 请求序号：analysisRequestSeq / isStaleRequest

`analysis.js:249-251`：

```js
const requestSeq = (state.analysisRequestSeq || 0) + 1;
state.analysisRequestSeq = requestSeq;
const isStaleRequest = () => requestSeq !== state.analysisRequestSeq;
```

- 每次 `fetchBeatmapFile` 开始时自增全局序号（appContext.js:149 `analysisRequestSeq`），闭包捕获本次的 `requestSeq`。
- 之后任何时刻（每个 await 之后）调用 `isStaleRequest()` 比对：一旦期间又发起了新分析（序号被推进），本次请求即过期，**立即 return 放弃渲染**，防止过期请求的结果覆盖新结果。
- 检查点遍布异步边界：fetch 响应后（:337、:344）、估算后（:515）、Interlude（:591）、Etterna（:651）、Companella（:701、:715）、异常路径（:919）、finally（:935）。
- worker 内部还有**同等的过期丢弃**：`manager.js:47 latestId` 只接受最新请求的响应（:54），30s 超时保护（:68-73）。

## 7. fetchBeatmapFile 主流程

`analysis.js:248 fetchBeatmapFile(reason)` 是全部逻辑的宿主（938 行）。开场动作：`analysis.js:273 setStatus("Loading beatmap file...")` + `hideOverlay()`；图模式时 `analysis.js:276-280 setGraphLoading(true)` 否则 `clearDiffGraph()`。

### 7.1 缓存查找与覆盖检查（简述，详见 result-cache.md §6）

- `analysis.js:287-304 needComputed`：本次需要的计算产物布尔集 `{pattern, ett, graph, interlude}`，由显示需求与算法需求推导（例如 `state.diffText === "Graph" || contentBarShows("Graph")` 需要 graph，:299；Companella/Mixed 需要 ett 与 interlude，:298、:302-303）。
- `analysis.js:305 cacheKey`：`${state.estimatorAlgorithm}|${state.lastBeatmapIdentity}|${state.modSignature}`。
- `analysis.js:306 isMetaDegraded`：identity 以 `meta:` 开头。
- `analysis.js:308-317`：`state.enableResultCache && state.lastBeatmapIdentity` 时查 `resultCache.get(cacheKey)`，取到后比对快照 `computed` 四项与 needComputed——全等才命中（`cached = snapshot`），任一不等视为 miss 走完整重算。

### 7.2 fetch .osu

`analysis.js:333-336`：

```js
const response = await fetch(getEndpoint(), { method: "GET", cache: "no-store" });
```

- 端点来自 `config.js:2 endpoint`（`http://localhost:24050/files/beatmap/file`），运行时 host 由设置覆盖：`appContext.js:16 getEndpoint()` 用 `state.wsEndpoint`（appContext.js:12 getSocketHost 回退 `SOCKET_HOST`）。
- `cache: "no-store"` 保证每次都拿最新文件；响应非 ok（:339-341）、内容为空（:345-347）都抛错。
- 响应后立即查过期（:337、:344）。

### 7.3 解析

`analysis.js:349 parseMetadataFromBeatmap(rawText)`（定义 :81-90）：`OsuFileParser`（osuFileParser.js:19）`process()` 后取 `metaData / lnRatio / columnCount`——只解析**元信息**，供 display6kLevel 判定（:537）、LN% 显示（:579-581）、fallback mode tag（:792）使用；完整谱面数据不进内存，估算器各自内部解析。

随后 `analysis.js:350-357`：keycount 不在 `GRAPH_SUPPORTED_KEY_SET`（appContext.js:180，即 {4,6,7}）且非 None/Full 模式时，`setEffectiveContentBarForMap("Pattern")` 把主体降级为 Pattern（谱面级 override，存 `state.effectiveContentBar`）。

### 7.4 估算分派（worker vs 主线程）

算法判定 `analysis.js:405 currentEstimatorAlgorithm()`（settings.js:999，即 `state.estimatorAlgorithm`）。分派结构 `analysis.js:465-511`：

| 算法 | 路径 | 回退 |
| --- | --- | --- |
| Daniel | `analysis.js:466-467` worker（`runInWorker`）；null 时主线程 `runDanielEstimatorFromText`（danielEstimator.js:9） | 无（Daniel 无 LN 支持） |
| Azusa | `analysis.js:472-473` worker 或主线程 `runAzusaEstimatorFromText`（azusaEstimator.js:822） | **无效结果**（`isValidEstimatorResult` :460-464）→ 主线程 `runSunnyEstimatorFromText` 并置 `actualEstimatorAlgorithm = "Sunny"`（:475-478） |
| Roxy | `analysis.js:483-484` worker 或主线程 `runRoxyEstimatorFromText`（roxyEstimator.js:1400） | 同上回退 Sunny（:486-489） |
| Companella | `analysis.js:494` **主线程** `runSunnyEstimatorFromText` 打底，4K 时置 `pendingCompanellaEstimate`（:498），Companella 本体在 §7.5 异步追加 | Companella 失败仅 console.warn，保留 Sunny 底（:736-738） |
| Mixed | `analysis.js:500` **主线程** `runMixedEstimatorFromText`（mixedEstimator.js:192） | 4K 时 Companella 追加（:723-735） |
| Sunny / 其他 | `analysis.js:506-507` worker 或主线程 `runSunnyEstimatorFromText`（sunnyEstimator.js:4） | worker null 时主线程同步执行 |

- **worker 回退链**：`manager.js:41 runInWorker` 在 Worker 构造失败时返回 null（manager.js:43），所有分支统一 `wp ? await wp : 同步函数` 模式——worker 是纯加速，行为等价。
- worker 内部分派：`compute.worker.js:17 self.onmessage`，`compute.worker.js:15 ESTIMATORS` 白名单，Azusa/Roxy 的无效回退在 worker 内也有（:38-41、:44-47），结果带 `actualEstimatorAlgorithm` 字段回传（:54）。
- `shouldForceSunnyWindow`（:427、:521-533）：SunnyWindow 结果替换 LN 段难度（详见 difficulty-estimation.md 对应章节）。
- 6K 定数 `sixKConst`：:536-546，`state.display6kLevel && columnCount === 6` 时用 Sunny SR 换算（`*200/81 + 7/6`）。
- **估算失败**：整个 try（:440-556）catch → `resetReworkDisplay()`（:551）+ `errors.push("Rework failed: ...")`（:555）。

### 7.5 附属计算（Companella 追加）

`analysis.js:685-739`：4K 且（pendingCompanellaEstimate 或 pendingMixedCompanellaContext）时，在 Etterna 结果就绪后调 `classifyCompanellaDifficulty`（companellaEstimator.js:181，async ONNX）：Companella 直接覆盖最终难度（:717-721），Mixed 经 `applyCompanellaToMixedResult`（mixedEstimator.js 导出，analysis.js:6-9 导入）融合（:723-735）。Companella 有独立的 etterna 版本设置（`companellaEtternaVersion`，:691-707，与主版本不同时单独重算 MSD）。

### 7.6 错误收集 errors[]

`analysis.js:388 const errors = []`。各附属计算块独立 try/catch，把失败原因 `errors.push(...)`（:555 Rework、:593 Interlude、:624 Pattern、:662 Etterna）。Etterna 的 keycount 不支持错误不算入 errors（:660-663 `shouldReportEtternaError && !isKeycountError` 过滤）。最终 `analysis.js:902-917`：errors 非空 → `setStatus("[Error] ...", "error")`（buildMetaError 拼接，:101-114）；否则 `setStatus(formatMetadataStatus(metadata), "ok")`（display.js:776）。

## 8. 写门（5 条件，简述）

`analysis.js:743-746`，全部满足才 `resultCache.put`（:751）：

```js
!cached && state.enableResultCache && state.lastBeatmapIdentity
&& errors.length === 0
&& rework && !isStaleRequest()
&& genAtStart === resultCacheGeneration()
```

即：miss + 开关开 + 身份存在 + 全程无错 + 有估算结果且请求未过期 + 代数未变（`genAtStart` 捕获于 :252，settings 侧 clear 会 +1 代数，防 clear 后旧分析污染新缓存）。写入内容与 `jsonSafe` 包装见 **result-cache.md §7**——本文不展开。

## 9. actualEstimatorAlgorithm 的时机

`state.actualEstimatorAlgorithm`（appContext.js:89，初值 = estimatorAlgorithm）记录**实际执行**的算法，与用户选择 `state.estimatorAlgorithm` 分离：

- **分析后设置**：`analysis.js:514 state.actualEstimatorAlgorithm = actualEstimatorAlgorithm`——Azusa/Roxy 因结果无效回退 Sunny 时由回退分支改写（:477、:488）；worker 路径的回退已通过返回值的 `actualEstimatorAlgorithm` 字段带回（analysis.js:474、:485，源头 compute.worker.js:54）。
- **缓存命中恢复，绝不重算**：`analysis.js:431 state.actualEstimatorAlgorithm = cached.actualEstimatorAlgorithm`（命中分支 :429-438 整体从快照恢复，见 result-cache.md §10）。
- **重置**：`analysis.js:216 resetReworkDisplay` 开头 `state.actualEstimatorAlgorithm = state.estimatorAlgorithm`（失败/清屏时回到用户选择）。
- 展示层一律读 `state.actualEstimatorAlgorithm`。

## 10. 渲染出口

分析完成后各渲染入口的触发点（均在 analysis.js 内）：

### display.js（卡片主体）

| 触发点（analysis.js） | 渲染函数（display.js） | 内容 |
| --- | --- | --- |
| :562 `showNumericStarValue(rework.star)` | display.js:421 `showNumericStarValue` | 左侧 star 数值 |
| :783/:789 `setEstimateDifficultyText(...)` | display.js:386 `setEstimateDifficultyText` | 难度名（≥18.5 时截断为 "> Cloverwisp Theta high"，:779-789） |
| :633 `renderPatternClusters(mergedClusters)` | display.js:650 `renderPatternClusters` | 键型 cluster 列表 |
| :676 `renderEtternaSkillBars(ettResult.values, columnCount)` | display.js:683 `renderEtternaSkillBars` | Etterna 技能条 |
| :855/:859/:863/:872/:879 `show6KConstValue` / `showCategoryValue` / `showInterludeValue` / `showMsdValue` / `showNumericStarValue` | display.js:434/:484/:522/:508/:421 | 左侧胶囊按 `state.srText` 分支（:854-885） |
| :887 `renderRightCapsule(...)` | display.js:576 `renderRightCapsule` | 右侧胶囊 |
| :896 `renderFullModeSeparators(overallValue)` | display.js:765 `renderFullModeSeparators` | Full 模式分隔线 |
| :562 `playStarBlockEntranceOnce()`（:265-271 封装） | display.js:408 `playStarBlockEntrance(changeKind)` | 入场动画（用 §4.3 的 activeChangeKind） |
| :370 `renderContentSkeleton()` | display.js:381 `renderContentSkeleton` | 展开动画期间的骨架屏 |
| :609 `mergeDuplicateClusters(allClusters)` | display.js:307 `mergeDuplicateClusters` | cluster 合并（数据转换，非渲染） |

### graph.js（SVG 图）

| 触发点 | 渲染函数（graph.js） | 内容 |
| --- | --- | --- |
| analysis.js:570 `renderDiffGraph(rework.graph)`（keycount 不支持走 :567-568 报错） | graph.js:545 `renderDiffGraph` | 难度图绘制 |
| socketHandlers.js:86/:141 `updateGraphCursor(...)` | graph.js:460 `updateGraphCursor` | 播放光标（时间线驱动，见 §4 的 updateSongTimeState） |
| analysis.js:821 `setNumericDifficultyValue(cappedDiff, cappedHint)` | graph.js:720 `setNumericDifficultyValue` | 图内数值显示 |
| analysis.js:824 `setForceHideNumericDifficulty(isVibroMap)` | graph.js:737 `setForceHideNumericDifficulty` | vibro 图隐藏数值 |
| analysis.js:564 `updateDiffTextVisibility()` | graph.js:655 `updateDiffTextVisibility` | 按 diffText 显隐 |
| analysis.js:277-279/:230/:576 `setGraphLoading` / `clearDiffGraph` | graph.js:388/:341 | 加载/清除态 |
| analysis.js:234/:553/:568/:572 `showDiffGraphError(...)` | graph.js:426 `showDiffGraphError` | 错误提示 |
| socketHandlers.js:115/:111 `addPauseMarker` / `clearAllPauseMarkers` | graph.js:318/:311 | 暂停标记（暂停检测驱动） |
| socketHandlers.js:300/:70 `resetPauseRuntime(...)` | graph.js:329 `resetPauseRuntime` | 暂停运行时重置 |

### hud.js（HUD 状态）

| 触发点 | 渲染函数（hud.js） | 内容 |
| --- | --- | --- |
| analysis.js:812 `setModeTag(resolvedModeTag)` / :810 `setModeTagAdvanced(typePercentageData, lnRatio)` | hud.js:143 `setModeTag` / hud.js:171 `setModeTagAdvanced` | 模式标签（RC/LN/HB/Mix，:792-813 判定） |
| analysis.js:814 `setSvTagVisible(shouldShowSvTag)` | hud.js:237 `setSvTagVisible` | SV 标签（:798-806 SVAmount 阈值判定） |
| analysis.js:273/:912/:915/:920 `setStatus(...)` | hud.js:126 `setStatus` | 状态行（loading/ok/error） |
| analysis.js:274/:913/:916/:928 `hideOverlay` / `showOverlay(...)` | hud.js:317/:304 | 遮罩层 |
| socketHandlers.js:157 `updateCardPlayVisibility()` | hud.js:282 `updateCardPlayVisibility` | 游玩状态显隐 |

## 11. 时序

| 参数 | 值 | 定义 | 消费 |
| --- | --- | --- | --- |
| `socketRecalcLazyDelayMs` | 200ms | config.js:70 `APP_CONFIG.timing.socketRecalcLazyDelayMs` → appContext.js:175 `SOCKET_RECALC_LAZY_DELAY_MS` | scheduler.js:22-23 `scheduleRecompute` 防抖窗口（socketHandlers.js:305、settings.js:859 传入 `useLazyDelay=true`） |
| `settingsCommandTimeoutMs` | 1500ms | config.js:71 `APP_CONFIG.timing.settingsCommandTimeoutMs` → appContext.js:176 `SETTINGS_COMMAND_TIMEOUT_MS`（settings.js:50 导入） | settings.js:871 `waitForInitialSettingsFromCommand(timeoutMs)` 的 getSettings 响应超时（超时 reject "getSettings timeout"），详见 settings-pipeline.md §9 |
| worker 安全超时 | 30s | manager.js:68-73 硬编码 | `runInWorker` 防挂起，超时 reject "Worker timeout" |
| 重连间隔 | 1s | socket.js:43 | WebSocket 断开重连 |
| `sendCommand` 重试 | 100ms/1s，至多 3 次 | socket.js:71-76/:82-87 | 命令通道未就绪/发送失败 |

## 12. 注意事项

- **stale 请求不写缓存**：写门含 `!isStaleRequest()`（analysis.js:745）与代数守卫 `genAtStart === resultCacheGeneration()`（:746）双保险——前者防"过期分析结果覆盖新结果"（含缓存），后者防"clear 之后旧分析写回"（result-cache.md §7）。
- **meta 降级 identity 不缓存**：`analysis.js:776` `put(cacheKey, {...}, { skip: isMetaDegraded })`——meta: 身份只读不写，碰撞风险下宁可每张图重算（result-cache.md §8）。
- **分析失败路径**：估算块 catch → `analysis.js:551 resetReworkDisplay()`（定义 :215-246）——重置 actualEstimatorAlgorithm、数值/胶囊/图/meta 全清、mode tag 回 "Mix"、SV 标签隐藏、必要时显示 "Graph unavailable"；外层 catch（:918-933，fetch/解析失败）同样调用并在 overlay 显示 "Load failed"。
- **worker 失败不阻塞主线程**：`runInWorker` 返回 null 即同步回退（analysis.js:467 等），Worker 构造失败（manager.js:23-25）与 30s 超时（manager.js:68-73）都不会让分析中断——代价是主线程计算可能卡顿，这是"可用性优先"的取舍。
- **缓存键不含显示类设置**：`display6kLevel`、`forceSunnyWindow`、etterna 版本等不进键，正确性依赖 settings.js 失效列表——任何新计算影响设置漏加失效 = 静默过期结果（result-cache.md §9）。
- **needComputed 是保守值**：fetch 前用上一张图的 effectiveContentBar 推导（analysis.js:284-286 注释），仅供覆盖检查；实际 shows*/need* 在 override 后重算（:362-365）——修改 needComputed 推导时注意保持两处一致。
- **changeKind 只消费一次**：`analysis.js:262` 取用后即清空 `state.pendingChangeKind`——后续纯设置 recompute 拿到的都是 undefined，按 difficulty 轻量过渡处理（:258-261），避免换歌动画重复播放。
- **身份为空的包直接丢弃**：`socketHandlers.js:253` 空 identity return——tosu 在谱面信息未就绪时发的包不会触发分析（mod 变化也进不来，此时 mod 状态保留旧值）。
