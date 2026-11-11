# 暂停检测功能文档（pause-detection.md）

> 目标读者：AI。描述插件在游玩过程中检测暂停并在难度图表与卡片上展示的功能实现。文中 path:line 行号为编写时快照，代码演进后可能漂移；定位源码请以符号名（symbol）为准，必要时用 grep 复核。
> 相关文档：[graph-visualization.md](graph-visualization.md)（暂停标记的渲染细节）、[settings.md](../settings.md)（用户设置说明）。

## 1. 功能概述

暂停检测用于实时识别玩家在游玩过程中的**暂停**（优先来自 tosu api_v2 / 桌面壳原生帧的 `game.paused` 标志；Stable 的该标志是“两次 `playTime` 相同”的推导值），并在两处展示：

1. **难度图表**：在暂停发生的谱面时间位置绘制红色竖线标记（`star-graph-pause-marker`）。
2. **卡片右下角**：显示 `Pause Count: N` 计数（`index.html` 的 `<div id="pause-count">`，DOM 引用见 `ManiaMapAnalyser by Leo_Black/js/app/core/appContext.js` 的 `pauseCountEl`）。

此外，**桌面壳 + osu!lazer** 下还有第二条独立数据面（见 §7）：回放播放、回放结算、历史结算屏从回放文件的 `.osr` 尾部读取**历史暂停**，进入即全景静态显示全部标记；拿不到数据显示 `Pause Data Unavailable`。两条数据面共用 `enablePauseDetection` 开关，互不覆盖：回放中实时检测整体跳过。

暂停检测是浏览器专属功能，仅涉及 `ManiaMapAnalyser by Leo_Black/js/app/` 下的模块，与估算/解析管线（`js/estimator/`、`js/parser/` 等共享模块）完全解耦。

## 2. 检测逻辑

### 2.1 数据源

`ManiaMapAnalyser by Leo_Black/js/app/net/socketHandlers.js:51` 的 `updateSongTimeState(data)` 是暂停检测的唯一驱动入口。每个 tosu 数据帧到达时：

1. 记录 `state.songStartMs` / `state.songEndMs`（谱面第一个/最后一个物件的谱面时间，按 `state.speedRate` 缩放），作为判定时间轴边界。该步**不依赖 live 时间**，位于 `liveTimeMs` 的 early-return 之前（`socketHandlers.js:52-70`）；时间线变化还会触发 `refreshGraphTimeline()` 重建难度图表（见 [graph-visualization.md](graph-visualization.md) §4.5）。
2. 从 tosu payload 提取实时谱面时间，并按倍速缩放为**谱面时间**（除以 `state.speedRate`，见 `socketHandlers.js:72-76`）；无 live 时优先回退到选歌 `menu.bm.time.current`，所有有效时间都缺失的帧到此为止。
3. 读取 **双层暂停判定**（优先原生标志，缺省时自动 500ms 停滞兜底，`socketHandlers.js:107-154`）。**回放中（`state.isReplay` 为真）整个分支跳过**：lazer 的 `ReplayPlayer` 自身暂停也会让 `game.paused` 短暂为真，必须整体跳过才能避免假标记（D12）：
   - **优先层**：若数据源提供原生的 `typeof data?.game?.paused === "boolean"`（官方 tosu 或桌面壳 native 传输），同样要求本局谱面时钟已经真正前进过（`state.playbackAdvanced`），并持续为真达到 `PAUSE_FALLBACK_THRESHOLD_MS`（500ms）才认定为暂停。Stable 的 `paused` 是“两次读取时间相同”的推导值，开局加载阶段会连续出现相同时间，必须靠这两道门过滤，否则会误记 +1/+2 次暂停。
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

若已收集暂停标记，且当前谱面时间倒退到**最早标记之前**（`scaledLiveTimeMs + PAUSE_DETECT_EPSILON_MS < earliestPauseTimeMs`），说明时间线被重置（如重开本图），调用 `resetPauseRuntime(true)` 清空全部标记与计数。**该分支在回放中整体跳过**（`!state.isReplay`）：回放倒带/重开不应清掉历史暂停。

### 2.4 生命周期清理

- **进入游玩 / 离开游玩（非结算）**：`resetPauseRuntime(true)`（`socketHandlers.js:164-165`）。
- **离开游玩到结算**：`resetPauseRuntime(false)`（不清标记，`socketHandlers.js:167`）。
- **谱面/Mod 变更**：`resetPauseRuntime(true)`（`socketHandlers.js:320`）。
- **分析失败/清空**：`clearAllPauseMarkers()`（`ManiaMapAnalyser by Leo_Black/js/app/core/analysis.js:302`）。

### 2.5 scheduler.js

`ManiaMapAnalyser by Leo_Black/js/app/core/scheduler.js` **不包含**任何暂停相关逻辑；暂停检测完全由 `socketHandlers.js` 的 `updateSongTimeState` 驱动。

## 3. 相关 state 字段

定义于 `ManiaMapAnalyser by Leo_Black/js/app/core/appContext.js:221-226`（初始值）：

| 字段 | 初始值 | 语义 |
| --- | --- | --- |
| `state.pauseMarkerTimes` | `[]` | 已判定暂停的谱面时间戳列表（单位 ms），图表标记的数据源 |
| `state.pauseCount` | `0` | 暂停次数，恒等于 `pauseMarkerTimes.length`（`graph.js:344` 同步） |
| `state.isPaused` | `false` | 当前是否处于暂停态（`game.paused` 的直接反映） |
| `state.pauseTimeMs` | `0` | 最近一次暂停的谱面时间；解除暂停后归零（`socketHandlers.js:126`） |
| `state.pauseFreezeStartRealMs` | `0` | 暂停确认的起算真实时间戳（ms）；原生 `game.paused` 与无标志时的 500ms 停滞兜底共用 |
| `state.playbackAdvanced` | `false` | 本局谱面时钟是否已真正前进过；开局加载阶段为 false，暂停判定必须等它为 true |
| `state.frozenInterpMs` | `0` | 暂停期间图表光标使用的冻结谱面时间（等于暂停时刻的谱面时间） |

控制开关：`state.pauseDetectionEnabled`（`appContext.js:184`）。相关状态位：`state.hasSongTimeSample`（`appContext.js:226`，是否已有首帧时间样本）、`state.isInPlayState`（`appContext.js:228`，由 `socketHandlers.js:152-169` 依据 tosu 状态名维护）。

## 4. 与 graph 的集成

图表暂停标记的入口全部在 `ManiaMapAnalyser by Leo_Black/js/app/graph/index.js`：

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

**HUD 计数**：`ManiaMapAnalyser by Leo_Black/js/app/ui/hud.js:255` 的 `updatePauseCountVisibility()`：检测关闭 → 隐藏；`pauseCount > 0` → 显示 `Pause Count: N`（active）；否则显示 `Pause Detection Enabled`（idle）。

## 5. 设置项

定义于 `ManiaMapAnalyser by Leo_Black/settings.json`：

| uniqueID | 位置 | 类型 | 默认值 | 说明 |
| --- | --- | --- | --- | --- |
| `enablePauseDetection` | `settings.json:311` | checkbox | `true` | 总开关；"Count pauses and draw pause markers on the graph when playing." |

接线逻辑（`ManiaMapAnalyser by Leo_Black/js/app/core/settings.js`）：

- `applyPauseDetectionSetting(value)`（`settings.js:578`）：写 `state.pauseDetectionEnabled`；**禁用时清空全部暂停状态**——`isPaused`、`pauseTimeMs`、`frozenInterpMs`、`pauseMarkerTimes`、`pauseCount`（`settings.js:583-588`），随后 `updatePauseCountVisibility()` + `redrawPauseMarkers()`（`settings.js:593-594`）。
- 解析器：`parseEnablePauseDetectionValue`（`ManiaMapAnalyser by Leo_Black/js/parser/settingsParser.js:390`），按 uniqueID 命名约定由 `settings.js:736` 的 `SETTING_HANDLERS` 接线，初始值应用在 `settings.js:933`。
- 默认值：`config.js:87`（`pauseDetectionEnabled: true`）。
- 时序常量：`config.js:72`（`songTimeJumpThresholdMs: 2000`）、`config.js:73`（`noteEndMarginMs: 500`）、`config.js:76`（`pauseDetectEpsilonMs: 0`），经 `appContext.js:273-275` 导出为 `SONG_TIME_JUMP_THRESHOLD_MS` / `NOTE_END_MARGIN_MS` / `PAUSE_DETECT_EPSILON_MS`。

> 历史：v2.0.1 起移除了 `pauseDetectionThreshold` 设置项（原“最小冻结时长”判定），改为优先采用 tosu api_v2 的 `game.paused`；后续 Stable 原生读取把“两次 `playTime` 相同”推导成 `paused`，单帧相同值会在开局加载阶段误触发。当前实现因此保留 500ms 确认窗口，并用 `playbackAdvanced` 过滤谱面开始前的停滞；无 `game.paused` 时仍走同阈值的谱面时间停滞兜底。

注意：`enablePauseDetection` **不属于**计算影响设置（不影响估计结果），因此不在 `clearResultCache()` 失效列表中，也不会污染结果缓存键。

## 6. 注意事项

1. **原生标志也要防抖**：`game.paused` 为真时必须先等谱面时钟真正前进过（`state.playbackAdvanced`），并持续 500ms；这样既不会把开局加载阶段的单帧/多帧相同时间误判为暂停，也不会在正常掉帧时立刻记一次暂停。
2. **仅在游玩状态工作**：`socketHandlers.js:107` 要求 `state.isInPlayState` 为真，结算界面/选图界面不检测。
3. **时间线回退清标记**：谱面时间倒退到最早标记之前（重开本图等）触发 `resetPauseRuntime(true)` 清空标记与计数（`socketHandlers.js:78-88`）。
4. **末尾缓冲**：进入谱面最后 500ms 后不再记录暂停标记（`atTimelineEnd`）；首物件之前同样不记录（`beforeStart`），避免谱面开始/结束的收尾帧产生无效标记。
5. **旧版 tosu 兼容与兜底**：当数据源 api_v2 缺失 `game.paused` 字段时（undefined），自动启用内置的 500ms 谱面时间停滞兜底机制，避免暂停检测完全失效。
6. **倍速归一化**：标记时间基于倍速缩放后的谱面时间（`socketHandlers.js:72-76`），DT/HT 下标记位置与谱面时间轴一致。
7. **禁用即清理**：关闭 `enablePauseDetection` 会立即清空已收集的标记与计数（`settings.js:583-588`），此后暂停检测不再运行。

## 7. 回放/历史结算的静态暂停（桌面壳 + osu!lazer）

这是与 §2 实时检测**并列**的第二条数据面，只在桌面壳 + lazer 下生效；浏览器模式（无壳）永远走 `live`。

### 7.1 链路与契约

```text
ReplayPlayer / ResultsScreen
  → 偏移表读 ScoreInfo.<Hash>（64-hex store key）
  → <storage>\files\<h>\<hh>\<hash>（.osr）
  → 跳过 replayData；lzma-rs 解压尾部 scoreInfo JSON
  → 24061 replay_pauses {hash, pauses, available, speedRate}（契约 v11）
  → 页面按 /speedRate 换算 + 区间过滤 → 竖线 + HUD
```

- 壳侧实现：`desktop/src/osu/lazer/osr.rs`（解析）、`desktop/src/server/replay_pauses.rs`（100 ms 采集、同 hash 去重、失败按 hash 记忆）、`desktop/src/osu/lazer/frame.rs`（`Snapshot.replay_hash`）。
- 页面侧实现：`js/app/net/replayPauses.js`（纯函数）、`js/app/net/socketHandlers.js`（`applyReplayPauses` / `applyReplayPausesToMarkers`）。
- 刷新/重连：页面在 `hello` 后发 `replay_pauses_sync`，壳回当前快照（无状态回空帧）。

### 7.1.1 适用版本（重要）

`pauses` 数组由 lazer PR #34058（commit `d8900def`，2025-07-22，"Store pause timestamps (rounded to ms) instead of general count"）引入：它把原来的 `pause_count`（整数）换成 `pauses`（`int[]`，每项是暂停开始时的物理谱面毫秒），Realm schema 同步升到 51。

- 首个包含该提交的 **tachyon** 构建：`2025.724.1-tachyon`（2025-07-24）；
- 首个包含该提交的 **lazer 稳定流**构建：`2025.816.0-lazer`（2025-08-16）。

因此本功能只对**以下成绩**可用：

| 成绩来源 | 是否带 `pauses` |
| --- | --- |
| lazer 稳定流 ≥ `2025.816.0` 的原生成绩 | 是 |
| lazer tachyon ≥ `2025.724.1` 的原生成绩 | 是 |
| 更早的 lazer 成绩（含 2022.1227 起带 `pause_count` 的 `.osr`） | 否（只有次数，没有时间戳） |
| osu!stable 成绩 / 从 stable 导入的成绩 | 否（stable 不记录暂停） |

早期成绩在页面上显示 `Pause Data Unavailable`，这不是读取失败，而是该成绩本身没有时间戳。
### 7.2 `.osr` 尾部格式（只读 `pauses`）

```text
byte ruleset | int32 version | string beatmapMd5 | string playerName | string replayMd5
u16 × 6 | int32 totalScore | u16 maxCombo | u8 perfect | int32 mods | string hpGraph
int64 dateTicks | int32 replayDataLength | bytes replayData
int64 legacyOnlineId | int32 scoreInfoLength | bytes scoreInfo
```

- 字符串是 `0x0B | ULEB128(byte len) | UTF-8`（`0x0B` 是 `SerializationWriter.ObjType.StringType`）。
- `scoreInfo` = `[5B LZMA props][8B 解压后长度 LE][LZMA 流]`；解压后是 `LegacyReplaySoloScoreInfo` JSON。
- 只读 `pauses`（int 数组）与 `mods`（倍速）；**不解析按键帧**，按长度跳过。
- 上限：文件 64 MiB、`scoreInfo` 1 MiB、解压后 8 MiB（防解压炸弹）。
- `version < 30_000_000` 没有 `scoreInfo` ⇒ `available:false`。

### 7.3 时间轴口径（必须除以 speedRate）

`pauses` 是**物理谱面时间**（`.osu` 毫秒轴）；页面图表的 x 轴是**显示轴**（原始 / `speedRate`，见 [graph-section-analysis.md](graph-section-analysis.md) §3）：

```text
markerTime = pauseMs / rate
rate = replayPausesSpeedRate ?? state.speedRate ?? 1
```

过滤区间：`[songStartMs, songEndMs - NOTE_END_MARGIN_MS)`（与实时检测同一口径；`NOTE_END_MARGIN_MS = 500`）。边界缺失时只按已知一侧过滤。

### 7.4 页面状态机与 HUD

`state.pauseDataStatus` 三态：

| 取值 | 含义 | HUD |
| --- | --- | --- |
| `live` | 实时检测模式（默认） | 沿用 §4 的两态 |
| `available` | 已拿到该回放的完整 `pauses` | `Pause Count: N`（含 `N=0`） |
| `unavailable` | 该回放/成绩拿不到暂停数据 | `Pause Data Unavailable`（`idle` 样式） |

关键 state 字段（`appContext.js`）：

| 字段 | 初值 | 语义 |
| --- | --- | --- |
| `isReplay` | `false` | 24062 根字段 `replay` |
| `replayHash` | `""` | 已应用的 `replay_pauses.hash` |
| `nativeReplayHash` | `""` | 24062 根字段 `replayHash`（身份校验） |
| `replayPausesRaw` | `[]` | 原始物理时间 ms |
| `replayPausesLoaded` | `false` | 是否收到过本回放的帧 |
| `replayPausesAvailable` | `false` | 最近一帧的 `available` |
| `replayPausesSpeedRate` | `null` | 最近一帧的 `speedRate` |
| `pauseDataStatus` | `"live"` | 三态 |
| `replayPausesAppliedKey` | `""` | 去重键（避免每帧重画） |

身份校验：`nativeReplayHash` 未知时**先接受** 24061 帧（它常先于 24062 到达），已知且不等时丢弃；进入回放时若已知 hash 与已应用的 payload 不一致则清空缓存。

**回放结算屏**：从 ReplayPlayer 进结算屏时 `resultsFromPlay` 也是 `true`，但 `results::read_results` 会把 `<player>` 的运行时类型 `ReplayPlayer` 写进 `snapshot.replay`，所以该帧仍带 `replay:true`。判定“活动局”必须用 `resultsFromPlay && !state.isReplay`，否则回放结算会误判成刚打完的一局。

### 7.5 失败与降级

| 情形 | `available` | 页面 |
| --- | --- | --- |
| 表无 `score_hash` / `score_info` | `false` | `Pause Data Unavailable` |
| `.osr` 不存在 / 旧版 / 损坏 / 无 `pauses` | `false` | 同上 |
| `pauses: []` | `true` | `Pause Count: 0` |
| 2026.921（表未更新） | `false` | 同上（已知限制） |

### 7.6 设置与生命周期

- 复用 `enablePauseDetection`：关闭时 HUD 与标记全隐藏（壳仍照常采集，不为此加通道）；重新开启后由下一帧自动恢复（`settings.js` 清空 `replayPausesAppliedKey`）。
- 进入回放/历史结算、离开回放/结算、换图、刷新均由 `applySignalState` 的边沿与 `replay_pauses_sync` 处理；`clearAllPauseMarkers()` 会清空去重键，保证下一帧能重画。
