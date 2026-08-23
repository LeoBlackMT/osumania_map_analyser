# 难度图表可视化功能文档（graph-visualization）

> 目标：AI（LLM）｜语言：中文
> 本文档描述插件「难度图表可视化」功能的实现细节：双图结构、图形数学、渲染流程、已玩/未玩双色填充、暂停标记与相关常量。
> 文中所有 `path:line symbol` 引用均相对本仓库根目录（插件文件夹名为 `ManiaMapAnalyser by Leo_Black`，含空格，路径引用必须精确）。文中 path:line 行号为编写时快照，代码演进后可能漂移；定位源码请以符号名（symbol）为准，必要时用 grep 复核。

## 1. 功能概述

难度图表（difficulty graph）将谱面难度随时间的变化渲染为 SVG 折线图。图表数据来自 `rework.graph`（由估算管线产出，见 [difficulty-estimation.md](difficulty-estimation.md)）。核心代码集中在浏览器专属模块 `ManiaMapAnalyser by Leo_Black/js/app/graph.js`（DOM 操作只发生在该模块，可安全使用 `document`）。

图表有**两处独立的显示位置**（双图结构），共用同一份数据与同一套渲染逻辑：

| 视图 | DOM | 启用条件 | 显示位置 |
|---|---|---|---|
| header（顶部胶囊图） | `rework-diff-graph` 系列元素 | `state.diffText === "Graph"` | 卡片右上角（顶栏区域） |
| body（主体图） | `body-graph` 系列元素 | `state.contentBar`（或 `effectiveContentBar`）包含 Graph | 卡片主体 |

- `state.diffText`（右上角内容）与 `state.contentBar`（主体内容）的设置语义见 [settings.md](../settings.md) 的 "Graph: 显示难度变化图"（`settings.md:13`）与右上角 Graph 选项说明（`settings.md:24`）。
- **非 4/6/7K 谱面图表不可用**：`ManiaMapAnalyser by Leo_Black/js/app/appContext.js:180 GRAPH_SUPPORTED_KEY_SET`（`new Set([4, 6, 7])`）。分析管线在 `analysis.js:328` / `analysis.js:354` 用其判断是否需要把主体回退为 Pattern；渲染入口 `analysis.js:567` 对不支持键数直接调用 `showDiffGraphError("Unsupported Keys")`。

## 2. DOM 引用与视图定义（GRAPH_VIEW_DEFS）

### 2.1 DOM refs

所有 SVG 元素引用在 `ManiaMapAnalyser by Leo_Black/js/app/appContext.js` 顶部通过 `getElementById` 一次性获取：

- header 图：`appContext.js:30 diffGraphSvgEl`（`rework-diff-graph`）、`appContext.js:31 diffGraphFillEl`、`appContext.js:32 diffGraphFillPlayEl`、`appContext.js:33 diffGraphPlayClipRectEl`、`appContext.js:34 diffGraphLineEl`、`appContext.js:35 diffGraphCursorEl`、`appContext.js:36 diffGraphCursorDotEl`、`appContext.js:37 diffGraphPauseMarkersEl`、`appContext.js:38 diffGraphErrorEl`，包裹层 `appContext.js:29 diffGraphWrapEl`。
- body 图：`appContext.js:40 bodyGraphSvgEl`（`body-graph`）、`appContext.js:41 bodyGraphFillEl`、`appContext.js:42 bodyGraphFillPlayEl`、`appContext.js:43 bodyGraphPlayClipRectEl`、`appContext.js:44 bodyGraphLineEl`、`appContext.js:45 bodyGraphCursorEl`、`appContext.js:46 bodyGraphCursorDotEl`、`appContext.js:47 bodyGraphPauseMarkersEl`、`appContext.js:48 bodyGraphErrorEl`，包裹层 `appContext.js:39 bodyGraphWrapEl`。

### 2.2 GRAPH_VIEW_DEFS 机制

`appContext.js:237 GRAPH_VIEW_DEFS` 是视图定义的统一描述数组，每个视图含 `key`、各 DOM ref 字段与 `isEnabled()` 判定：

- header 视图：`isEnabled: () => state.diffText === "Graph"`（`appContext.js:250`）。
- body 视图：`isEnabled: () => contentBarShows("Graph")`（`appContext.js:264`，内部经 `appContext.js:232 contentBarShows` → `appContext.js:228 getActiveContentBar` 读取 `effectiveContentBar || contentBar`）。

遍历工具：

- `appContext.js:268 hasAnyGraphModeEnabled()`：`diffText === "Graph" || contentBarShows("Graph")`，用于判断图表功能整体是否激活。
- `appContext.js:272 forEachGraphView(callback)`：无条件遍历两个视图（清空、重置等必须覆盖隐藏视图的场合）。
- `appContext.js:278 forEachEnabledGraphView(callback)`：只遍历 `isEnabled()` 为真的视图（渲染、游标更新等仅对可见视图生效的场合）。

graph.js 中通过 `view.svgEl` / `view.fillEl` / `view.fillPlayEl` / `view.playClipRectEl` / `view.lineEl` / `view.cursorEl` / `view.cursorDotEl` / `view.pauseMarkersEl` / `view.errorEl` / `view.wrapEl` 等字段统一访问各视图元素，视图代码完全不感知具体是 header 还是 body。

## 3. 图形数学（graphMath.js）

`ManiaMapAnalyser by Leo_Black/js/app/graphMath.js` 是纯函数模块（无 DOM），供 graph.js 与加载骨架共用：

- `graphMath.js:1 f2(v)`：数值转字符串，非有限值返回 `"0.00"`（两位小数，用于加载骨架等路径构建）。
- `graphMath.js:5 buildLinePath(points)`：由 `[[x, y], ...]` 点数组构建折线 SVG path 字符串（`M x0 y0 L x1 y1 ...`），空数组返回 `""`。
- `graphMath.js:22 buildFillPath(points, baseY)`：构建填充 path：先下到基线再沿点走再封底闭合（`M x0 baseY L x0 y0 ... L xN baseY Z`），形成封闭区域。
- `graphMath.js:47 normalizeGraphSeries(graphData, resampleIntervalMs)`：数据清洗与归一化——过滤非有限时间、缺失值继承前值、时间戳去重（`time <= lastTime` 时强制 `lastTime + 1`）、单遍统计 `minYValue`/`maxYValue`；点数不足 2 返回 `null`。**注意：该函数本身不重采样**（重采样在调用方决定采样点）；若 `times` 为空则用 `i * resampleIntervalMs` 合成时间轴。
- `graphMath.js:102 interpolateSeriesValue(times, values, targetTime)`：二分查找 + 线性插值；越界取端点值，重复时间点返回 `y0`。

## 4. 渲染流程

### 4.1 主渲染 renderDiffGraph

入口 `ManiaMapAnalyser by Leo_Black/js/app/graph.js:545 renderDiffGraph(graphData)`（被 `analysis.js:570` 调用）：

1. `hasAnyGraphModeEnabled()` 为假直接返回 `false`（`graph.js:546`）。
2. `normalizeGraphSeries(graphData, GRAPH_RESAMPLE_INTERVAL_MS)` 归一化（`graph.js:550`）；失败 → `graph.js:552 showDiffGraphError("Graph unavailable")`。
3. `graph.js:558 trimSeriesStartToFirstObject(normalizedSeries)`（定义于 `graph.js:144`）：谱面首个物件（`state.songStartMs`）之前的点被裁剪，起点值经 `interpolateSeriesValue` 插值补出，避免图线从谱面开始前就画出来。
4. 手工预分配 `lineParts`/`fillParts` 数组，逐点计算 viewBox 坐标：`x = xMin + (t - minTime) * tScale * xSpan`，`y = yMax - (v - minYValue) * vScale * ySpan`；坐标字符串用 `graph.js:541 f1`（一位小数，注释说明对 260px viewBox 足够且省 ~20% 字符串长度）；填充基线 `baseYs = yMax`（`graph.js:594`），闭合 `Z`（`graph.js:623`）。
5. `graph.js:629 setGraphLoading(false)` 先撤掉加载态，然后用 `requestAnimationFrame` 把 DOM 更新**推迟到下一帧**（`graph.js:632`），保证加载骨架能先绘制一帧、避免闪烁。
6. 下一帧回调内 `forEachEnabledGraphView`（`graph.js:633`）：
   - `lineEl`/`fillEl` 写入 path（`graph.js:634-635`）；
   - `fillPlayEl` 写入同一 fill path（`graph.js:636`）并 `playClipRectEl` 宽度置 0（`graph.js:637`）——已玩亮层初始不可见；
   - `fillEl` 加 `graph-unplayed` 类（`graph.js:638`，暗色底）；
   - 错误元素隐藏（`graph.js:639-642`）。
7. 写入 `state.graphSeries = { times, values, minTime, maxTime, minYValue, maxYValue }`（`graph.js:645`，后续游标/暂停标记读取它）。
8. `graph.js:647 triggerGraphScanEnter(view)`（定义 `graph.js:50`）：换歌时加 `scan-enter` 类做左→右扫描揭示，换难度/改设置加 `scan-enter-soft` 轻量淡入（由 `state.activeChangeKind` 决定，`graph.js:58`），`GRAPH_SCAN_ENTER_DURATION_MS = 400` 后移除（`graph.js:28`、`graph.js:61-67`）。
9. `graph.js:648 redrawPauseMarkers()`、`graph.js:649 updateGraphCursor()` 收尾。

返回 `true` 表示渲染成功（`graph.js:652`）。

### 4.2 加载态 setGraphLoading

`graph.js:388 setGraphLoading(isLoading)`：仅在 `hasAnyGraphModeEnabled()` 时工作，只作用于启用视图（`graph.js:393`）。

- 进入加载（`graph.js:398-418`）：`buildGraphLoadingPaths()`（`graph.js:92`，用 `APP_CONFIG.graph.loadingSampleCount` 个点 + `loadingBaseOffset` 画水平基线骨架）生成 line/fill path；`svgEl` 加 `loading` 类（触发 CSS 波浪动画）；`fillEl` **移除** `graph-unplayed`（`graph.js:404`，注释说明加载骨架不继承上一张图的暗化）；`resetPlayedFill(view)`；显示 "Graph loading..." 文本（`graph.js:406`，文本元素由 `graph.js:112 ensureGraphLoadingTextEl` 惰性创建，显示切换 `graph.js:136 setGraphLoadingTextVisible`）；隐藏游标与错误元素。
- 退出加载（`graph.js:421-422`）：移除 `loading` 类、隐藏加载文本。

### 4.3 错误态 showDiffGraphError

`graph.js:426 showDiffGraphError(message)`：先 `setGraphLoading(false)`，置 `state.graphSeries = null`（`graph.js:432`），对启用视图清空 fill/line path、`resetPlayedFill`、隐藏游标、清空暂停标记（`graph.js:433-449`），最后在 `errorEl` 显示错误消息（默认 "Graph unavailable"，`graph.js:452`）。调用方：`analysis.js:568`（Unsupported Keys）与 `analysis.js:572`（渲染失败）。

### 4.4 图整体清空 clearDiffGraph

`graph.js:341 clearDiffGraph()`：`state.graphSeries = null`；对**所有**视图（`forEachGraphView`）移除 `loading` 类、清除扫描动画、清空 fill/line path、`resetPlayedFill`、隐藏游标、隐藏错误、隐藏加载文本、清空暂停标记。`graph.js:704` 中当 `updateDiffTextVisibility` 发现无任何图表模式启用时也会调用它。

## 5. 已玩/未玩双色填充（重点）

### 5.1 机制

同一张 SVG 图内叠了两层填充：

- 底层 `fillEl`：完整 fill path，带 `graph-unplayed` 类（`graph.js:638`）——未玩部分（暗色、低亮）。
- 顶层 `fillPlayEl`：**同一份** fill path（`graph.js:636`），类名 `star-graph-fill-play`（亮色），被 `<clipPath>` 裁剪，clip rect 宽度决定可见范围——已玩部分（亮色）。

裁剪矩形 `playClipRectEl` 的宽度由游标 x 决定，每帧跟随：

- `graph.js:515-517`：`view.playClipRectEl.setAttribute("width", x.toFixed(2))`，其中 `x` 是当前播放时间插值出的 viewBox 横坐标（`graph.js:495`）。注释（`graph.js:514`）说明：clip 宽度 = 已玩边界，暂停时 x 冻结、回退时 x 收缩，自动跟随。
- 游标更新在 `graph.js:460 updateGraphCursor(explicitTimeMs)`：播放时间经 `graph.js:70 getInterpolatedPlaybackTime()` 获取（基于 WebSocket 时间戳 + `performance.now()` 插值，暂停时返回冻结值 `state.frozenInterpMs`）；时间限定在 `[songStartMs, songEndMs]`（`graph.js:482-484`）再 clamp 到系列范围；y 值经 `interpolateSeriesValue` + 归一化得出（`graph.js:496-498`）。
- 动画循环：`graph.js:537 syncGraphAnimationLoop()` 只在**存在任一图表模式**时用 `requestAnimationFrame` 每帧调用 `updateGraphCursor()`（`graph.js:530-536`）；无图表模式时 `cancelAnimationFrame` 停掉循环，避免常驻空转。`startGraphAnimationLoop()`（`graph.js:552`）在初始化时调用，`updateDiffTextVisibility`/`setRuntimeContentBar`/`setEffectiveContentBarForMap` 在模式切换时同步启停。

### 5.2 resetPlayedFill 调用点（共 6 处，清空/加载/错误路径）

`graph.js:261 resetPlayedFill(view)` 定义：clip rect 宽度置 `"0"`、`fillPlayEl` 的 `d` 置 `""`。**漏调用会导致亮色已玩层泄漏到已清空的图上**（clip rect 残留旧宽度 + 旧 path 仍在）。

全部调用点：

| 序号 | 位置 | 场景 |
|---|---|---|
| 1 | `graph.js:353` | `clearDiffGraph`（图整体清空，含无图表模式启用时的兜底清空） |
| 2 | `graph.js:405` | `setGraphLoading(true)`（进入加载骨架，亮层必须同时清掉） |
| 3 | `graph.js:445` | `showDiffGraphError`（错误路径） |
| 4 | `graph.js:462` | `updateGraphCursor` 且 `hasAnyGraphModeEnabled()` 为假（全视图） |
| 5 | `graph.js:469` | `updateGraphCursor` 且 `state.graphSeries` 为空（启用视图） |
| 6 | `graph.js:476` | `updateGraphCursor` 且播放时间不可得（启用视图） |

> 维护提醒：任何新增的"清空图表"路径都必须调用 `resetPlayedFill`（或复用 `clearDiffGraph`），否则会出现已玩亮层残留在空白图上的视觉 bug。

## 6. 暂停标记

暂停标记把暂停检测产出的暂停时间点画成竖直虚线。数据源与生命周期：

- `state.pauseMarkerTimes`（数组，`appContext.js:132`）与 `state.pauseCount`（`appContext.js:133`）；开关为 `state.pauseDetectionEnabled`（`appContext.js:98`）。检测逻辑见 [pause-detection.md](pause-detection.md)。
- 新增：`graph.js:318 addPauseMarker(songTimeMs)`——push 到 `pauseMarkerTimes`、更新 `pauseCount`、`updatePauseCountVisibility()`、`redrawPauseMarkers()`。
- 绘制：`graph.js:273 drawPauseMarkersForView(view)`——先 `clearPauseMarkersDom(view)`；若 `pauseDetectionEnabled` 且存在 `graphSeries` 且视图启用，则对每个标记时间 clamp 到 `[minTime, maxTime]` 后映射为 x 坐标，`document.createElementNS` 创建 `<line>`，类名 `star-graph-pause-marker`，描边色/宽度取 `APP_CONFIG.graph.pauseLineColor` / `pauseLineWidth`（`graph.js:299-300`），y 范围取视图纵向边界（`graph.js:284` 经 `graph.js:32 getGraphLineVerticalBounds`，body 视图上下各外扩 5px）。
- 重绘：`graph.js:305 redrawPauseMarkers()`——对启用视图逐视图重画。渲染完成（`graph.js:662`）、暂停开关变更（`settings.js:581`）、新增标记后（`graph.js:326`）都会触发。
- 清除：`graph.js:246 clearPauseMarkersDom(view = null)`（带 view 清单个，否则清全部）；`graph.js:311 clearAllPauseMarkers()`（同时清空 `pauseMarkerTimes`、`pauseCount` 并刷新 HUD）；`graph.js:329 resetPauseRuntime(clearMarkers)` 在 `clearMarkers` 时调用前者。
- 清空时机：`clearDiffGraph`（`graph.js:339`）、加载（`graph.js:388`）、错误（`graph.js:426`）、设置关闭（`settings.js:569-577` 清空数组）。
- 联动：`socketHandlers.js:62-72` 在回退到最早暂停点之前时 `resetPauseRuntime(true)`，实现"重绕即清除旧暂停标记"。

## 7. 常量（config.js graph 块）

`ManiaMapAnalyser by Leo_Black/config.js:52-65` 的 `graph` 配置块，经 `appContext.js:159-166` 导出为模块常量：

| config.js 字段 | appContext 导出 | 值 | 用途 |
|---|---|---|---|
| `viewboxWidth` | `appContext.js:159 GRAPH_VIEWBOX_WIDTH` | 260 | SVG viewBox 宽 |
| `viewboxHeight` | `appContext.js:160 GRAPH_VIEWBOX_HEIGHT` | 86 | SVG viewBox 高 |
| `paddingX` | `appContext.js:161 GRAPH_PADDING_X` | 6 | 水平内边距 |
| `paddingTop` | `appContext.js:162 GRAPH_PADDING_TOP` | 8 | 顶部内边距 |
| `paddingBottom` | `appContext.js:163 GRAPH_PADDING_BOTTOM` | 6 | 底部内边距 |
| `resampleIntervalMs` | `appContext.js:164 GRAPH_RESAMPLE_INTERVAL_MS` | 100 | 重采样间隔（100ms，`renderDiffGraph` 调用 `normalizeGraphSeries` 时传入，`graph.js:550`） |
| `loadingSampleCount` | —（经 `APP_CONFIG.graph`） | 20 | 加载骨架采样点数（`graph.js:97`） |
| `loadingWaveCycles` / `loadingWaveAmplitude` | — | 2.5 / 7 | 加载波浪动画参数（CSS 侧使用） |
| `loadingBaseOffset` | — | 22 | 加载骨架基线偏移（`graph.js:102`） |
| `pauseLineColor` | `appContext.js:165 PAUSE_LINE_COLOR` | `#FF3B3B` | 暂停标记线颜色（`graph.js:299`） |
| `pauseLineWidth` | `appContext.js:166 PAUSE_LINE_WIDTH` | 2 | 暂停标记线宽（`graph.js:300`） |

另有 `appContext.js:168 GRAPH_LOADING_BASELINE_Y`（`GRAPH_VIEWBOX_HEIGHT - GRAPH_PADDING_BOTTOM`）作为加载骨架 fill 的基线 y。

## 8. 其他入口与注意事项

- **显示切换**：`graph.js:655 updateDiffTextVisibility()` 统一按 `state.diffText` 切换右上区域（Difficulty 文本 / header 图 / MSD 等右胶囊）的可见性：`Graph` 显示 header 图（`graph.js:658`），`Difficulty` 显示估计难度（`graph.js:657`），`MSD/Pattern/ReworkSR/InterludeSR` 显示右胶囊（`graph.js:659-662`）；`None` 时隐藏 caption（`graph.js:680`）。无任何图表模式启用时调用 `clearDiffGraph()`（`graph.js:704-705`）。
- **数值难度**：`graph.js:720 setNumericDifficultyValue(value, hint)` 写入 `state.numericDifficulty` / `numericDifficultyHint`；`graph.js:737 setForceHideNumericDifficulty(value)` 强制隐藏。两者仅在 `diffText === "Difficulty"` 时触发 caption 重渲染（`graph.js:732`、`graph.js:744`）。caption 文本由 `graph.js:199 formatEstimateDifficultyCaption()` 生成（含 `[实际算法]` 前缀、`RCxx|LNxx*` 双值格式）。
- **游标可见性**：`graph.js:376 setGraphCursorVisible(visible)`——隐藏时对启用视图强制隐藏游标与游标点（`graph.js:378-385`），防止禁用视图残留游标。
- **图表是否启用**：由 `state.diffText` 与 `contentBar` 共同判定（`hasAnyGraphModeEnabled`，见 §2.2），与缓存快照的 coverage 检查相关（详见 [result-cache.md](../pipeline/result-cache.md)）。
- **样式**：`star-graph-fill.graph-unplayed`（暗底）与 `star-graph-fill-play`（亮层）、`star-graph-pause-marker`、`loading` 波浪动画、`scan-enter` 扫描动画等类名在 `styles/graph.css` 与 `styles/theme.css`（osu 主题变体 `html.ma-theme-osu` 使用 `--ma-accent`）中定义；本文档不展开 CSS 细节。
- **性能**：`renderDiffGraph` 手工预分配 path 段数组并复用 `f1` 减少字符串拼接（`graph.js:582-587`）；动画循环仅在 `hasAnyGraphModeEnabled()` 时更新游标（`graph.js:531`）。
