# 特定时间点谱面分析（graph-section-analysis）

> 目标：AI（LLM）｜语言：中文
> 本文描述「图表选区分析」功能：双击难度图表进入选区模式，拖出时间区间，把 `.osu` 文本按区间切片后重新喂给分析管线，让卡片指标刷新成该区间的结果。
> 文中 `path:line symbol` 为编写时快照，行号会漂移；定位以符号名为准。

## 1. 功能与范围

- **仅桌面壳**：入口是 `js/app/graphSection/index.js` 的 `initGraphSection()`，内部用 `isShellRuntime()`（`js/app/feedback/feedbackSpec.js`）判定「24061 端口 **或** 壳追加的 `#mma-shell`」。浏览器 / OBS 模式下函数直接返回：不挂监听、不改 DOM、保留全文（`rememberFullChartText` / `notifySectionAnalysis` 也各自早退）。
- **图表始终显示全谱波形**：区间跑批只刷新卡片指标，**不重画图表**（详见 §5.3）。
- **不改分析管线语义**：`runAnalysisPipeline` 签名/输出不变，区间能力完全由「输入文本被切片」实现。
- **不改缓存键、不新增设置项**：没有 `settings.json` 条目，也没有 `clearResultCache()` 变更。

## 2. 交互

| 动作 | 行为 |
|---|---|
| 双击任一图表（`.star-graph-wrap` / `.body-graph-wrap`） | `idle → armed`：两个图表容器加 `graph-section-armed`（呼吸描边 + crosshair） |
| 待命态拖拽（左键按下 → 移动 → 松开） | 实时画遮罩；松开派发 `scheduleRecompute("graph section", lazy 200ms)` |
| 待命态再次拖拽 | 直接覆盖上一次选区，不退出待命态 |
| 纯点击（无位移） | 还原上一次选区，不跑分析 |
| 再次双击 | 退出；若存在选区则派发一次全谱重算 |
| 换图（身份变化）/ 清卡 / 进入游玩 / 进入结算屏 | 复位选区（见 §8） |

**进入守卫**（`canArm`）：`isShellRuntime()`、非游玩态（`state.isInPlayState` / `malodyPlaying` / `malody4Playing`）、非结算屏（`state.clientStateName === "resultscreen"`）、`state.graphSeries` 可用（跨度 > 1ms）、至少一个图表视图可见。

**`clearSection(reason, { recompute })`**：未 armed 且无选区时为**纯 no-op**（返回 false），避免「页面启动就在结算屏」「平时进游玩态」白跑分析。

## 3. 时间轴与坐标映射

三条时间轴，不可混用：

| 轴 | 定义 | 出现位置 |
|---|---|---|
| 显示轴 | `原始时间 / speedRate` | `state.graphSeries`、`rework.graph.times`、`state.songTimeMs/songStartMs` |
| 物理轴（谱面轴） | `.osu` 文件毫秒 | 切片器、`[TimingPoints]`、`[HitObjects]` |
| 真实播放轴 | = 显示轴（DT/HT 已折算） | 状态行时间胶囊 |

**像素 → viewBox**：`js/app/graphMath.js clientXToViewBoxX()`。两图 `preserveAspectRatio` 不同（header=`none` 拉伸、body=`xMidYMid meet` 可能左右留黑边），所以按 `getBoundingClientRect` + viewBox + PAR 解算；用比例而非 `getScreenCTM`，CSS `zoom` 的缩放因子在比值里自然抵消。

**viewBox → 绘图带比例**：`u = clamp01((xVb - GRAPH_PADDING_X) / (GRAPH_VIEWBOX_WIDTH - 2 * GRAPH_PADDING_X))`，`GRAPH_PADDING_X = 6`、有效宽度 248。用户拖到图表边框附近也会被钳到绘图带两端（消除「边框与绘图区之间的间隙」）。

**选区数据**：`{ u0, u1, startMs, endMs, rateAtSelection }`。
- `u0/u1` 用于画遮罩（与设备像素、缩放、速率无关）。
- `startMs/endMs` 是**物理毫秒**，切片唯一真值来源：`物理 = 显示 × speedRate`。
- 速率变化时全谱曲线整体等比缩放、像素形状不变 ⇒ `u` 不变；物理时间与速率无关 ⇒ 切片区间不变；胶囊按 `物理 / 当前速率` 现算。

## 4. 切片器（`js/parser/osuTimeSlice.js`）

纯函数、零依赖、Node 可测：`sliceOsuTextByTimeRange(rawText, startMs, endMs) → { text, objectCount, timingPointCount }`。

| 段 | 处理 |
|---|---|
| `[General]` `[Editor]` `[Metadata]` `[Difficulty]` `[Colours]` | 原样保留（`Mode/CircleSize/OverallDifficulty` 一变，键数/OD 全歪） |
| `[Events]` | 非 Break 行原样；Break 仅保留与区间相交者（原样，不裁剪） |
| `[TimingPoints]` | 区间内全部（红/绿都留）+ 区间前最后一条红线（BPM 基准）+ 区间前最后一条绿线（SV 状态延续）；文件没有红线 ≤ start 时补**首条红线**（osu 语义：首红线向前生效）；按时间稳定升序 |
| `[HitObjects]` | `objStart <= endMs && objEnd >= startMs`（LN 用 `type & 128` 与第 6 段 `结束时间:...`）；**LN 完整保留、不截头不截尾** |
| 未知段 | 原样保留（前向兼容） |
| 换行 | `split(/\r?\n/)` → `join("\n")`，末尾补 `\n` |
| 空区间 | 结构完整、`objectCount=0`，不抛异常（由管线自然报错） |

**为什么保留原始时间戳**：所有估算器只吃相对差值；平移会引入 `canonicalizeOsuTiming`（Roxy）之外的二次偏移风险。

**为什么补前导绿线**：`sunnyWindowAlgorithm.js parseTimingsAndDetectSV()` 用绿线累计「非 1.0x 速度持续时长」判 `isSV`；区间开始正处于 SV 段内时，缺前一条绿线会丢失 `inSV` 起点。

## 5. 分析管线接线（`analysis.js`）

### 5.1 全谱原文缓存

- `rememberFullChartText(rawText)`：**只在非区间跑批**成功后写入 `{ identity, text }`（`js/app/graphSection/index.js`）。浏览器模式早退，不占内存。
- `getFullChartText()`：identity 与 `state.lastBeatmapIdentity` 不一致时返回 `null`。
- 为什么必须复用原文：外部源（Etterna / Malody V / Malody 4）没有可重抓的 osu 端点；osu 侧也免一次网络往返。

### 5.2 拦截点（H1–H12）

`const sectionRange = getSectionRange(); const isSectionRun = Boolean(sectionRange);` 只在 `fetchBeatmapFile` 顶部求值一次。

| # | 位置 | 行为 |
|---|---|---|
| H1 | 入口 | 快照选区 |
| H2 | 缓存读取 | 区间不读（短路在 `state.enableResultCache` 之前） |
| H3 | 图表加载态 | 区间**跳过** `setGraphLoading`（保留曲线与 `graphSeriesSource`）；曲线已为空时 `clearSection("graph-missing")` 后 return |
| H4 | 文本获取 + `pendingSource*` | 区间用 `getFullChartText()`；**不消费** `pendingSourceText/RequestId/Active`；取不到 ⇒ `clearSection("text-missing")` 后 return |
| H5 | 文本获取后 | 非区间才 `rememberFullChartText(rawText)` |
| H6 | 进管线前 | `rawText = sliceOsuTextByTimeRange(...).text`（唯一输入变换） |
| H7 | `needComputed.graph` | 区间为 `false`（不产图） |
| H8 | 渲染段 | 区间不调用 `renderDiffGraph` / `clearDiffGraph` |
| H9 | 写缓存 | 区间不写 |
| H10 | `feedbackContext` / 遥测 | 区间都不写/不构造 payload |
| H11 | `if (rework)` 末尾 | `notifySectionAnalysis(rework.star)` 刷新遮罩主题色与胶囊 |
| H12 | 两处 `resetReworkDisplay()` | 传 `{ keepGraph: isSectionRun }`，失败时保留全谱曲线/暂停标记/反馈上下文 |

### 5.3 图表为什么不能重画

图表要一直显示全谱波形（用户需要全局视野，选区只是叠加遮罩）。`renderDiffGraph` 会把 `state.graphSeries` 替换成传入序列；若把区间结果画上去，x 轴窗口、进度游标、暂停标记都会按「区间形状」错位。因此区间跑批同时做三件事：`withGraph=false`（不白算）、跳过加载态、跳过渲染。

## 6. 严格门控（缓存 / 遥测 / 反馈）

区间跑批是**临时派生计算**，六条硬规则：

1. 不读缓存（连 `state.enableResultCache` 都不读）；
2. 不写缓存（不 `resultCache.put`、不改变 generation、不清缓存）；
3. 不上报遥测（不构造 payload、不调用 `trackTelemetryAnalyze`）；
4. 不写 `state.feedbackContext`（成功失败都不写，保留上一轮全谱值）；
5. 不消费 `pendingSourceRequestId` ⇒ 不产生壳 result 帧；
6. 失败路径也不清空图表、不显示 "Graph unavailable"。

对应实现点见 §5.2 的 H2/H4/H9/H10/H12。

## 7. DOM 与样式

- 状态行时间胶囊：`index.html` `#section-time`（`.title-icon` 右侧），文本 `mm:ss - mm:ss`（不设上限，75 分钟显示 `75:03`），样式在 `styles/status.css .section-time`。
- 遮罩图层：两个 SVG 内各一组 `<g class="graph-section-overlay is-hidden">`（`rect.graph-section-mask` + 两条 `line.graph-section-edge-*`），位于暂停标记之后、游标之前；显隐用 **class**（SVG 上单写 `hidden` 不可靠）。
- 颜色：`--graph-section-accent` 由 `starColorFor(rework.star)` 写到卡片根，遮罩 `fill-opacity: .18`，边界线 1.4。
- 呼吸描边：`.star-graph-wrap.graph-section-armed` / `.body-graph-wrap.graph-section-armed`，`graph-section-breathe` 2.2s；`prefers-reduced-motion` 下关闭动画。

## 8. 生命周期

| 事件 | 位置 | 处理 |
|---|---|---|
| 换歌/换难度/换外部源 | `socketHandlers.applyBeatmapState`（身份变化） | `clearSection("beatmap", { recompute: false })`（随后自己的 recompute 生效） |
| 外部源 song 帧身份变化 | `sources/externalSource.handleSongFrame` | 同上 |
| 进入游玩 | `socketHandlers.applySignalState`（`enteredPlayState`） | `clearSection("play", { recompute: true })` |
| Malody 桥 / Malody 4 进入游玩 | `externalSource.handleSongFrame`（`screen=playing/play`） | `clearSection("play", { recompute: false })`（该帧自带跑批） |
| 进入结算屏 | `socketHandlers.applySignalState`（`nextIsResultScreen` 首次为真） | `clearSection("results", { recompute: true })` |
| 清卡 | `analysis.js invokeCardClear` | `clearSection("card-clear", { recompute: false })` |

> 仅改 mod（含 DT/HT/自定义速率）**不复位**：选区按物理时间保存，速率变化要保留选区并按新速率重算。

## 9. 已知限制

1. **区间独立重算**：结果是「这段谱面单独拿出来算」，不是「全谱中这段的难度贡献」；看数值时要理解这一点（遮罩 + 胶囊只提示范围，不改变语义）。
2. **SV 图**：`parseTimingsAndDetectSV` 需要 >1 个 SV 区间且 >2000ms 才判 `isSV`；超短区间可能判否，LN 子评分口径略偏。切片器已补前导绿线覆盖多数情况。
3. **跨界 LN 完整保留**：区间 LN% 会高于全谱（真实语义），模式标签可能从 RC 变 Mix/LN。
4. **缓存关闭 + 外部源**：退出选区需要重算；缓存关闭时无法命中全谱快照，外部源又无端点可抓，只能等下一次 song 帧。
5. **速率切换的 200ms 竞态**：拖拽瞬间若 mod 速率刚好变化，物理时间可能按旧速率折算一次；下一次分析/重拖会校正。
6. **空选区**：不设最小秒数/最小 Note 门槛，空结果走现有错误链路（状态行红字 / No data），不额外提示。

## 10. 验收要点

- 双击两图均能进入；一侧拖拽另一侧同步；边界贴合绘图带两端。
- 松开后指标全量刷新，图表仍是全谱形状；胶囊时间与拖拽位置一致。
- DT 开启时胶囊时间按 1.5× 折算、遮罩不动、星数按 DT 重算。
- 换图/换难度/清卡/游玩/结算五类事件均复位。
- 浏览器模式（无 `#mma-shell`）双击无反应、无监听。
- 区间跑批：无缓存读写、无遥测、无 result 帧、`feedbackContext` 不变。
