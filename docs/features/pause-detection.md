# 暂停检测功能文档（pause-detection.md）

> 目标读者：AI。描述插件在游玩过程中检测暂停并在难度图表与卡片上展示的功能实现。文中 path:line 行号为编写时快照，代码演进后可能漂移；定位源码请以符号名（symbol）为准，必要时用 grep 复核。
> 相关文档：[graph-visualization.md](graph-visualization.md)（暂停标记的渲染细节）、[settings.md](../settings.md)（用户设置说明）。

## 1. 功能概述

暂停检测用于实时识别玩家在游玩过程中的**暂停**（来自 tosu api_v2 的原生 `game.paused` 标志），并在两处展示：

1. **难度图表**：在暂停发生的谱面时间位置绘制红色竖线标记（`star-graph-pause-marker`）。
2. **卡片右下角**：显示 `Pause Count: N` 计数（`index.html:103` 的 `<div id="pause-count">`，DOM 引用见 `ManiaMapAnalyser by Leo_Black/js/app/appContext.js:135` 的 `pauseCountEl`）。

暂停检测是浏览器专属功能，仅涉及 `ManiaMapAnalyser by Leo_Black/js/app/` 下的模块，与估算/解析管线（`js/estimator/`、`js/parser/` 等共享模块）完全解耦。

## 2. 检测逻辑

### 2.1 数据源

`ManiaMapAnalyser by Leo_Black/js/app/socketHandlers.js:51` 的 `updateSongTimeState(data)` 是暂停检测的唯一驱动入口。每个 tosu 数据帧到达时：

1. 记录 `state.songStartMs` / `state.songEndMs`（谱面第一个/最后一个物件的谱面时间，按 `state.speedRate` 缩放），作为判定时间轴边界。该步**不依赖 live 时间**，位于 `liveTimeMs` 的 early-return 之前（`socketHandlers.js:52-70`）；时间线变化还会触发 `refreshGraphTimeline()` 重建难度图表（见 [graph-visualization.md](graph-visualization.md) §4.5）。
2. 从 tosu payload 提取实时谱面时间，并按倍速缩放为**谱面时间**（除以 `state.speedRate`，见 `socketHandlers.js:72-76`）；无 live 时间的帧到此为止。
3. 读取 **双层暂停判定**（优先原生标志，缺省时自动 500ms 停滞兜底，`socketHandlers.js:110-132`）：
   - **优先层**：若数据源提供原生的 `typeof data?.game?.paused === "boolean"`（官方 tosu 或桌面壳 native 传输），优先采纳该标志，零延迟且完全不受掉帧卡顿影响。
   - **兜底层**：若数据源缺失 `game.paused`（`undefined`），当游玩中谱面时间连续停滞达 `PAUSE_FALLBACK_THRESHOLD_MS`（500ms）时，自动推断进入暂停；时间重新推进时自动解除。

### 2.2 状态转移（socketHandlers.js:107-132）

| 条件 | 行为 |
| --- | --- |
| `gamePaused && !state.isPaused`（进入暂停） | 若暂停点处于谱面时间线内（不在 `beforeStart`/`atTimelineEnd` 区域），`addPauseMarker(scaledLiveTimeMs)` 并设置 `pauseTimeMs`、`frozenInterpMs` 为暂停时刻的谱面时间；无论是否记录标记，`state.isPaused = true`（`socketHandlers.js:112-123`） |
| `!gamePaused && state.isPaused`（解除暂停） | `isPaused = false`、`pauseTimeMs = 0`（`socketHandlers.js:124-127`） |
| 非游玩状态或检测关闭 | `isPaused = false`、`pauseTimeMs = 0`、`frozenInterpMs = songTimeMs`（`socketHandlers.js:128-132`） |

标记只在**进入暂停的那一帧**记录一次（`!state.isPaused` 守卫），暂停持续期间不会重复计数。

时间线边界语义与旧版一致：

- **beforeStart**（`socketHandlers.js:117`）：`scaledLiveTimeMs < songStartMs`，时间线尚未到达谱面开头——不记录标记。
- **atTimelineEnd**（`socketHandlers.js:116`）：`scaledLiveTimeMs >= songEndMs - NOTE_END_MARGIN_MS`，已进入谱面末尾 500ms 缓冲带——不记录标记。

**兼容性与兜底**：若数据源无 `game.paused` 字段（`undefined`），自动启用 500ms 谱面时间停滞兜底，确保在缺少原生暂停标志的环境下暂停检测依然可用，无需用户额外配置。

### 2.3 时间线回退清理（socketHandlers.js:78-88）

若已收集暂停标记，且当前谱面时间倒退到**最早标记之前**（`scaledLiveTimeMs + PAUSE_DETECT_EPSILON_MS < earliestPauseTimeMs`），说明时间线被重置（如重开本图），调用 `resetPauseRuntime(true)` 清空全部标记与计数。

### 2.4 生命周期清理

- **进入游玩 / 离开游玩（非结算）**：`resetPauseRuntime(true)`（`socketHandlers.js:164-165`）。
- **离开游玩到结算**：`resetPauseRuntime(false)`（不清标记，`socketHandlers.js:167`）。
- **谱面/Mod 变更**：`resetPauseRuntime(true)`（`socketHandlers.js:320`）。
- **分析失败/清空**：`clearAllPauseMarkers()`（`ManiaMapAnalyser by Leo_Black/js/app/analysis.js:302`）。

### 2.5 scheduler.js

`ManiaMapAnalyser by Leo_Black/js/app/scheduler.js` **不包含**任何暂停相关逻辑；暂停检测完全由 `socketHandlers.js` 的 `updateSongTimeState` 驱动。

## 3. 相关 state 字段

定义于 `ManiaMapAnalyser by Leo_Black/js/app/appContext.js:221-226`（初始值）：

| 字段 | 初始值 | 语义 |
| --- | --- | --- |
| `state.pauseMarkerTimes` | `[]` | 已判定暂停的谱面时间戳列表（单位 ms），图表标记的数据源 |
| `state.pauseCount` | `0` | 暂停次数，恒等于 `pauseMarkerTimes.length`（`graph.js:344` 同步） |
| `state.isPaused` | `false` | 当前是否处于暂停态（`game.paused` 的直接反映） |
| `state.pauseTimeMs` | `0` | 最近一次暂停的谱面时间；解除暂停后归零（`socketHandlers.js:126`） |
| `state.pauseFreezeStartRealMs` | `0` | 兜底层时间停滞起算的真实时间戳（ms）；仅在数据源缺失 `game.paused` 时生效 |
| `state.frozenInterpMs` | `0` | 暂停期间图表光标使用的冻结谱面时间（等于暂停时刻的谱面时间） |

控制开关：`state.pauseDetectionEnabled`（`appContext.js:184`）。相关状态位：`state.hasSongTimeSample`（`appContext.js:226`，是否已有首帧时间样本）、`state.isInPlayState`（`appContext.js:228`，由 `socketHandlers.js:152-169` 依据 tosu 状态名维护）。

## 4. 与 graph 的集成

图表暂停标记的入口全部在 `ManiaMapAnalyser by Leo_Black/js/app/graph.js`：

| 函数 | 位置 | 作用 |
| --- | --- | --- |
| `clearPauseMarkersDom(view = null)` | `graph.js:266` | 清空 `view.pauseMarkersEl` 的 DOM 内容；不传 view 时遍历全部 graph view |
| `redrawPauseMarkers()` | `graph.js:325` | 对所有启用的 graph view 执行 `drawPauseMarkersForView` |
| `clearAllPauseMarkers()` | `graph.js:331` | `pauseMarkerTimes=[]` + `pauseCount=0` + `clearPauseMarkersDom()` + `updatePauseCountVisibility()` |
| `addPauseMarker(songTimeMs)` | `graph.js:338` | 校验 `state.pauseDetectionEnabled` 与有限时间后 push 时间戳、同步 `pauseCount`、刷新 HUD 并重画标记 |
| `resetPauseRuntime(clearMarkers = false)` | `graph.js:349` | 重置暂停运行时字段（`isPaused`/`pauseTimeMs`/`frozenInterpMs`/`hasSongTimeSample`）；`clearMarkers=true` 时额外调用 `clearAllPauseMarkers()` |

内部渲染函数为 `drawPauseMarkersForView(view)`（`graph.js:293`，未导出）：先 `clearPauseMarkersDom(view)`，若检测关闭、无 `graphSeries` 或 view 不可用则直接返回；否则遍历 `state.pauseMarkerTimes`，按 `graphSeries.minTime/maxTime` 归一化映射到 viewbox X 坐标，绘制垂直线 `<line class="star-graph-pause-marker">`，颜色/线宽来自 `APP_CONFIG.graph.pauseLineColor`（`#FF3B3B`）/`pauseLineWidth`（`2`，`config.js:67-68`）。

**调用关系**：`socketHandlers.js` 检测到进入暂停（`game.paused` 上升沿）→ `addPauseMarker` →（更新 HUD + `redrawPauseMarkers`）；时间线回退重置 → `resetPauseRuntime(true)` → `clearAllPauseMarkers`。图表动画帧中，暂停时光标时间取 `getInterpolatedPlaybackTime()`（`graph.js:70-73`），暂停态返回 `state.frozenInterpMs`，保证光标停在暂停位置；`updateGraphCursor` 的暂停传参参见 `socketHandlers.js:144-146`。

暂停标记的**渲染细节**（SVG 分层、样式、主题）不在本文档范围，见 [graph-visualization.md](graph-visualization.md)。

**HUD 计数**：`ManiaMapAnalyser by Leo_Black/js/app/hud.js:255` 的 `updatePauseCountVisibility()`：检测关闭 → 隐藏；`pauseCount > 0` → 显示 `Pause Count: N`（active）；否则显示 `Pause Detection Enabled`（idle）。

## 5. 设置项

定义于 `ManiaMapAnalyser by Leo_Black/settings.json`：

| uniqueID | 位置 | 类型 | 默认值 | 说明 |
| --- | --- | --- | --- | --- |
| `enablePauseDetection` | `settings.json:311` | checkbox | `true` | 总开关；"Count pauses and draw pause markers on the graph when playing." |

接线逻辑（`ManiaMapAnalyser by Leo_Black/js/app/settings.js`）：

- `applyPauseDetectionSetting(value)`（`settings.js:578`）：写 `state.pauseDetectionEnabled`；**禁用时清空全部暂停状态**——`isPaused`、`pauseTimeMs`、`frozenInterpMs`、`pauseMarkerTimes`、`pauseCount`（`settings.js:583-588`），随后 `updatePauseCountVisibility()` + `redrawPauseMarkers()`（`settings.js:593-594`）。
- 解析器：`parseEnablePauseDetectionValue`（`ManiaMapAnalyser by Leo_Black/js/parser/settingsParser.js:390`），按 uniqueID 命名约定由 `settings.js:736` 的 `SETTING_HANDLERS` 接线，初始值应用在 `settings.js:933`。
- 默认值：`config.js:87`（`pauseDetectionEnabled: true`）。
- 时序常量：`config.js:72`（`songTimeJumpThresholdMs: 2000`）、`config.js:73`（`noteEndMarginMs: 500`）、`config.js:76`（`pauseDetectEpsilonMs: 0`），经 `appContext.js:273-275` 导出为 `SONG_TIME_JUMP_THRESHOLD_MS` / `NOTE_END_MARGIN_MS` / `PAUSE_DETECT_EPSILON_MS`。

> 历史：v2.0.1 起移除了 `pauseDetectionThreshold` 设置项（原"最小冻结时长"判定）。暂停改为直接采用 tosu api_v2 的 `game.paused` 标志，**游戏卡顿导致的谱面时间停滞不再参与判定**，因此不再需要阈值；`js/app/pauseDetection.js`（`computePauseTransition` 冻结状态机）、`state.pauseFreezeStartRealMs` / `state.pauseFreezeSongTimeMs` 一并删除。

注意：`enablePauseDetection` **不属于**计算影响设置（不影响估计结果），因此不在 `clearResultCache()` 失效列表中，也不会污染结果缓存键。

## 6. 注意事项

1. **不受游戏卡顿影响**：暂停判定基于 tosu 上报的 `game.paused` 原生标志，谱面时间帧间冻结（掉帧/卡顿）不会产生误判；原"卡顿误判需调高阈值"的问题因阈值移除而自然消失。
2. **仅在游玩状态工作**：`socketHandlers.js:107` 要求 `state.isInPlayState` 为真，结算界面/选图界面不检测。
3. **时间线回退清标记**：谱面时间倒退到最早标记之前（重开本图等）触发 `resetPauseRuntime(true)` 清空标记与计数（`socketHandlers.js:78-88`）。
4. **末尾缓冲**：进入谱面最后 500ms 后不再记录暂停标记（`atTimelineEnd`）；首物件之前同样不记录（`beforeStart`），避免谱面开始/结束的收尾帧产生无效标记。
5. **旧版 tosu 兼容与兜底**：当数据源 api_v2 缺失 `game.paused` 字段时（undefined），自动启用内置的 500ms 谱面时间停滞兜底机制，避免暂停检测完全失效。
6. **倍速归一化**：标记时间基于倍速缩放后的谱面时间（`socketHandlers.js:72-76`），DT/HT 下标记位置与谱面时间轴一致。
7. **禁用即清理**：关闭 `enablePauseDetection` 会立即清空已收集的标记与计数（`settings.js:583-588`），此后暂停检测不再运行。