# 暂停检测功能文档（pause-detection.md）

> 目标读者：AI。描述插件在游玩过程中检测暂停并在难度图表与卡片上展示的功能实现。文中 path:line 行号为编写时快照，代码演进后可能漂移；定位源码请以符号名（symbol）为准，必要时用 grep 复核。
> 相关文档：[graph-visualization.md](graph-visualization.md)（暂停标记的渲染细节）、[settings.md](../settings.md)（用户设置说明）。

## 1. 功能概述

暂停检测用于实时识别玩家在游玩过程中的**暂停**（游戏时间冻结），并在两处展示：

1. **难度图表**：在暂停发生的谱面时间位置绘制红色竖线标记（`star-graph-pause-marker`）。
2. **卡片右下角**：显示 `Pause Count: N` 计数（`index.html:94` 的 `<div id="pause-count">`，DOM 引用见 `ManiaMapAnalyser by Leo_Black/js/app/appContext.js:52` 的 `pauseCountEl`）。

暂停检测是浏览器专属功能，仅涉及 `ManiaMapAnalyser by Leo_Black/js/app/` 下的模块，与估算/解析管线（`js/estimator/`、`js/parser/` 等共享模块）完全解耦。

## 2. 检测逻辑

### 2.1 数据源

`ManiaMapAnalyser by Leo_Black/js/app/socketHandlers.js:47` 的 `updateSongTimeState(data)` 是暂停检测的唯一驱动入口。每个 tosu 数据帧到达时：

1. 从 tosu payload 提取实时谱面时间，并按倍速缩放为**谱面时间**（除以 `state.speedRate`，见 `socketHandlers.js:54-55`）。
2. 记录 `state.songStartMs` / `state.songEndMs`（谱面第一个/最后一个物件的谱面时间，`socketHandlers.js:57-60`），作为判定时间轴边界。
3. 仅在 `state.pauseDetectionEnabled && state.isInPlayState` 时执行暂停状态机（`socketHandlers.js:91`）。

### 2.2 状态机：computePauseTransition

核心纯函数为 `ManiaMapAnalyser by Leo_Black/js/app/pauseDetection.js:1` 的 `computePauseTransition(...)`，无 DOM 依赖，入参：

- `previousTimeMs` / `currentTimeMs`：上一帧与本帧的谱面时间。
- `isPaused`：当前是否处于暂停冻结态。
- `jumpThresholdMs`：跳变阈值（`SONG_TIME_JUMP_THRESHOLD_MS`，默认 2000ms）。
- `noteEndMarginMs`：时间轴末尾缓冲（`NOTE_END_MARGIN_MS`，默认 500ms）。
- `timelineStartMs` / `timelineEndMs`：谱面时间轴起止（`songStartMs` / `songEndMs`）。
- `epsilonMs`：时间冻结判定的容差（`PAUSE_DETECT_EPSILON_MS`，默认 0）。
- `freezeStartRealMs` / `freezeSongTimeMs`：当前冻结起点的墙钟时间 / 谱面时间（跨帧记忆）。
- `pauseThresholdMs`：判定为暂停的最小冻结时长（`state.pauseDetectionThresholdMs`，默认 500ms）。
- `nowRealMs`：当前墙钟时间（`performance.now()`）。

返回 `{ jumped, atEnd, sameTime, nextPaused, shouldAddMarker, shouldClearMarkers, frozenInterpMs, pauseTimeMs, freezeStartRealMs, freezeSongTimeMs }`。

### 2.3 判定公式（pauseDetection.js:40-81）

- **atEnd**（`pauseDetection.js:41`）：`now >= timelineEndMs - noteEndMarginMs`，即当前时间已进入谱面末尾 500ms 缓冲带。
- **beforeStart**（`pauseDetection.js:43`）：`now < timelineStartMs`，即时间线尚未到达谱面开头。
- **timeDelta**（`pauseDetection.js:45`）：`now - prev`，两帧谱面时间差。
- **jumped**（`pauseDetection.js:46`）：`|timeDelta| > jumpThresholdMs`，且**不是**"向前的小跳变"（`timeDelta > 0 && timeDelta < threshold` 的情况不算 jumped）。即：向后的跳变、或超过 2000ms 的大向前跳变都视为跳变。
- **sameTime**（`pauseDetection.js:47`）：`|timeDelta| <= epsilonMs`，两帧谱面时间未前进。`PAUSE_DETECT_EPSILON_MS` 默认 0（`config.js:72`），即只有谱面时间**完全冻结**才算。

状态转移（`pauseDetection.js:57-81`）：

| 条件 | 行为 |
| --- | --- |
| `jumped && !atEnd && !beforeStart` | 视为时间线跳变（seek/重载），退出暂停、`shouldClearMarkers=true`、清冻结起点 |
| `sameTime && !atEnd && !beforeStart` | 冻结检测：无冻结起点则记录 `freezeStartRealMs=nowReal`、`freezeSongTimeMs=now`；已有起点且 `nowReal - freezeStartRealMs >= pauseThresholdMs`（默认 500ms）时判定暂停：`nextPaused=true`、`shouldAddMarker=true`、`frozenInterpMs = pauseTimeMs = freezeSongTimeMs`（冻结时的谱面时间） |
| `nextPaused`（之前暂停，时间恢复前进） | 解除暂停，清冻结起点 |
| 其他 | 清冻结起点（时间正常前进，未到阈值） |

输入非法（非有限数值）时返回全空态，`nextPaused` 保持原值（`pauseDetection.js:25-38`）。

### 2.4 调用接线（socketHandlers.js:91-128）

```js
const pauseTransition = computePauseTransition({ ... });   // socketHandlers.js:92-105
state.pauseFreezeStartRealMs = pauseTransition.freezeStartRealMs;   // :107
state.pauseFreezeSongTimeMs = pauseTransition.freezeSongTimeMs;     // :108
if (pauseTransition.shouldClearMarkers) clearAllPauseMarkers();     // :110-112
if (pauseTransition.shouldAddMarker) {                              // :114-118
    addPauseMarker(pauseTransition.pauseTimeMs);
    state.pauseTimeMs = pauseTransition.pauseTimeMs;
    state.frozenInterpMs = pauseTransition.frozenInterpMs;
}
state.isPaused = pauseTransition.nextPaused;                        // :120
if (!state.isPaused) state.pauseTimeMs = 0;                         // :121-123
```

非游玩状态或检测关闭时：`isPaused=false`、`pauseTimeMs=0`、`frozenInterpMs=songTimeMs`（`socketHandlers.js:124-128`）。

### 2.5 时间线回退清理（socketHandlers.js:62-72）

若已收集暂停标记，且当前谱面时间倒退到**最早标记之前**（`scaledLiveTimeMs + PAUSE_DETECT_EPSILON_MS < earliestPauseTimeMs`），说明时间线被重置（如重开本图），调用 `resetPauseRuntime(true)` 清空全部标记与计数。

### 2.6 生命周期清理

- **进入游玩 / 离开游玩（非结算）**：`resetPauseRuntime(true)`（`socketHandlers.js:159-163`）。
- **离开游玩到结算**：`resetPauseRuntime(false)`（不清标记，`socketHandlers.js:161-162`）。
- **谱面/Mod 变更**：`resetPauseRuntime(true)`（`socketHandlers.js:300`）。
- **分析失败/清空**：`clearAllPauseMarkers()`（`ManiaMapAnalyser by Leo_Black/js/app/analysis.js:231`）。

### 2.7 scheduler.js

`ManiaMapAnalyser by Leo_Black/js/app/scheduler.js` **不包含**任何暂停相关逻辑；暂停检测完全由 `socketHandlers.js` 的 `updateSongTimeState` 驱动。

## 3. 相关 state 字段

定义于 `ManiaMapAnalyser by Leo_Black/js/app/appContext.js:127-133`（初始值）：

| 字段 | 初始值 | 语义 |
| --- | --- | --- |
| `state.pauseMarkerTimes` | `[]` | 已判定暂停的谱面时间戳列表（单位 ms），图表标记的数据源 |
| `state.pauseCount` | `0` | 暂停次数，恒等于 `pauseMarkerTimes.length`（`graph.js:324` 同步） |
| `state.isPaused` | `false` | 当前是否处于暂停冻结态 |
| `state.pauseTimeMs` | `0` | 最近一次暂停的谱面时间；解除暂停后归零（`socketHandlers.js:121-123`） |
| `state.frozenInterpMs` | `0` | 暂停期间图表光标使用的冻结谱面时间（等于冻结起点谱面时间） |
| `state.pauseFreezeStartRealMs` | `0` | 当前冻结起点的墙钟时间（`performance.now()`），用于累计冻结时长 |
| `state.pauseFreezeSongTimeMs` | `0` | 当前冻结起点的谱面时间 |

控制开关：`state.pauseDetectionEnabled`（`appContext.js:93`）、`state.pauseDetectionThresholdMs`（`appContext.js:94`）。相关状态位：`state.hasSongTimeSample`（`appContext.js:134`，是否已有首帧时间样本）、`state.isInPlayState`（`appContext.js:136`，由 `socketHandlers.js:150-156` 依据 tosu 状态名维护）。

## 4. 与 graph 的集成

图表暂停标记的入口全部在 `ManiaMapAnalyser by Leo_Black/js/app/graph.js`：

| 函数 | 位置 | 作用 |
| --- | --- | --- |
| `clearPauseMarkersDom(view = null)` | `graph.js:246` | 清空 `view.pauseMarkersEl` 的 DOM 内容；不传 view 时遍历全部 graph view |
| `redrawPauseMarkers()` | `graph.js:305` | 对所有启用的 graph view 执行 `drawPauseMarkersForView` |
| `clearAllPauseMarkers()` | `graph.js:311` | `pauseMarkerTimes=[]` + `pauseCount=0` + `clearPauseMarkersDom()` + `updatePauseCountVisibility()` |
| `addPauseMarker(songTimeMs)` | `graph.js:318` | 校验 `state.pauseDetectionEnabled` 与有限时间后 push 时间戳、同步 `pauseCount`、刷新 HUD 并重画标记 |
| `resetPauseRuntime(clearMarkers = false)` | `graph.js:329` | 重置全部暂停运行时字段（`isPaused`/`pauseTimeMs`/`frozenInterpMs`/两个 freeze 字段/`hasSongTimeSample`）；`clearMarkers=true` 时额外调用 `clearAllPauseMarkers()` |

内部渲染函数为 `drawPauseMarkersForView(view)`（`graph.js:273`，未导出）：先 `clearPauseMarkersDom(view)`，若检测关闭、无 `graphSeries` 或 view 不可用则直接返回；否则遍历 `state.pauseMarkerTimes`，按 `graphSeries.minTime/maxTime` 归一化映射到 viewbox X 坐标，绘制垂直线 `<line class="star-graph-pause-marker">`，颜色/线宽来自 `APP_CONFIG.graph.pauseLineColor`（`#FF3B3B`）/`pauseLineWidth`（`2`，`config.js:63-64`）。

**调用关系**：`socketHandlers.js` 判定 `shouldAddMarker` → `addPauseMarker` →（更新 HUD + `redrawPauseMarkers`）；判定 `shouldClearMarkers` → `clearAllPauseMarkers`。图表动画帧中，暂停时光标时间取 `getInterpolatedPlaybackTime()`（`graph.js:70-73`），暂停态返回 `state.frozenInterpMs`，保证光标停在冻结位置；`updateGraphCursor` 的暂停传参见 `socketHandlers.js:140-142`。

暂停标记的**渲染细节**（SVG 分层、样式、主题）不在本文档范围，见 [graph-visualization.md](graph-visualization.md)。

**HUD 计数**：`ManiaMapAnalyser by Leo_Black/js/app/hud.js:256` 的 `updatePauseCountVisibility()`：检测关闭 → 隐藏；`pauseCount > 0` → 显示 `Pause Count: N`（active）；否则显示 `Pause Detection Enabled`（idle）。

## 5. 设置项

定义于 `ManiaMapAnalyser by Leo_Black/settings.json`：

| uniqueID | 位置 | 类型 | 默认值 | 说明 |
| --- | --- | --- | --- | --- |
| `enablePauseDetection` | `settings.json:269` | checkbox | `true` | 总开关；"Count pauses and draw pause markers on the graph when playing." |
| `pauseDetectionThreshold` | `settings.json:325` | options | `"500"` | 判定为暂停的最小冻结时长，可选 `100` / `200` / `500` / `1000`（ms）；"Higher values reduce false positives from game lag." |

接线逻辑（`ManiaMapAnalyser by Leo_Black/js/app/settings.js`）：

- `applyPauseDetectionSetting(value)`（`settings.js:544`）：写 `state.pauseDetectionEnabled`；**禁用时清空全部暂停状态**——`isPaused`、`pauseTimeMs`、`frozenInterpMs`、`pauseFreezeStartRealMs`、`pauseFreezeSongTimeMs`、`pauseMarkerTimes`、`pauseCount`（`settings.js:549-556`），随后 `updatePauseCountVisibility()` + `redrawPauseMarkers()`（`settings.js:561-562`）。
- `applyPauseDetectionThresholdSetting(value)`（`settings.js:566`）：数值校验，非法/非正数回退到 `APP_CONFIG.defaults.pauseDetectionThresholdMs`（500）。
- 解析器：`parseEnablePauseDetectionValue`（`ManiaMapAnalyser by Leo_Black/js/parser/settingsParser.js:372`）、`parsePauseDetectionThresholdValue`（`settingsParser.js:512`），按 uniqueID 命名约定由 `settings.js:742-743` 的 `applyIf` 接线，初始值应用在 `settings.js:917-918`。
- 默认值：`config.js:84`（`pauseDetectionEnabled: true`）、`config.js:85`（`pauseDetectionThresholdMs: 500`）。
- 时序常量：`config.js:68`（`songTimeJumpThresholdMs: 2000`）、`config.js:69`（`noteEndMarginMs: 500`）、`config.js:72`（`pauseDetectEpsilonMs: 0`）、`config.js:73`（`pauseDetectionThresholdMs: 500`），经 `appContext.js:170-173` 导出为 `SONG_TIME_JUMP_THRESHOLD_MS` / `NOTE_END_MARGIN_MS` / `PAUSE_DETECT_EPSILON_MS` / `PAUSE_DETECTION_THRESHOLD_MS`。

注意：`pauseDetectionThreshold` 与 `enablePauseDetection` 均**不属于**计算影响设置（不影响估计结果），因此不在 `clearResultCache()` 失效列表中，也不会污染结果缓存键。

## 6. 注意事项

1. **游戏卡顿误判**：若游戏掉帧/卡顿导致谱面时间在多帧内冻结超过阈值，会被计为一次暂停。遇到此情况应调高 `pauseDetectionThreshold`（如 1000ms）。
2. **仅在游玩状态工作**：`socketHandlers.js:91` 要求 `state.isInPlayState` 为真，结算界面/选图界面不检测。
3. **跳变不计数**：谱面时间跳动超过 2000ms（seek、重载、切难度）触发 `shouldClearMarkers` 清空标记，而不是记为暂停；从末尾向前的小回退（<2000ms）同理。
4. **末尾缓冲**：进入谱面最后 500ms 后不再判定暂停（`atEnd`），避免结算前收尾帧被误判。
5. **Epsilon 容差默认 0**：`PAUSE_DETECT_EPSILON_MS` 为 0（`config.js:72`）意味着只有谱面时间**完全冻结**才进入冻结累积；若游戏在卡顿时仍上报微小变化的时间，可调大该常量（目前无对应设置项，需改代码）。
6. **倍速归一化**：所有时间判定基于倍速缩放后的谱面时间（`socketHandlers.js:54-55`），DT/HT 下暂停检测阈值语义保持一致（冻结的是谱面时间）。
7. **禁用即清理**：关闭 `enablePauseDetection` 会立即清空已收集的标记与计数（`settings.js:549-556`），且 `computePauseTransition` 不再运行。
