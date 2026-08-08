# pattern-analysis.md — 键型分析（RC/LN Pattern）功能文档

> 目标读者：AI。本文档描述 `js/patterns/` 键型分析模块的完整管线、各模块职责、核心模式体系、LN 检测来源、`debugUseAmount` 设置影响及其与模式标签（mode tag）的衔接。
> 路径约定：引用路径相对仓库根目录，插件目录名精确为 `ManiaMapAnalyser by Leo_Black`（含空格）；`js/patterns/` 内的文件常以短名 `config.js`/`summary.js`/`findPatterns.js` 出现，均指 `ManiaMapAnalyser by Leo_Black/js/patterns/` 下同名文件（模块表见 §3）。`js/patterns/` 是共享模块（浏览器与 Node benchmark runner 均使用），**不依赖任何 DOM API**，文档中所有模块均可在 Node 环境运行。

---

## 1. 概述

`js/patterns/` 模块负责把 `.osu` 谱面文本转换为**键型分析报告**（RC/LN 模式分布），是插件"键型分析"功能的计算核心。其 RC 键型分析算法移植自 [Interlude (YAVSRG)](https://github.com/YAVSRG/YAVSRG)，并在其基础上**新增了 LN 检测算法**（见 [第 5 节](#5-ln-检测说明)）。

对外入口为 `analyzePatternFromText`，被 `js/app/analysis.js` 调用；产出 `report`（含 `Clusters`、`Category`、`LNPercent`、`ModeTag`、`SVAmount`、`Duration`），由 `display.js` 渲染为界面上的 5 条模式进度条，`Category` 与 `ModeTag` 还会参与模式标签与自动显示档位的决策。

## 2. 数据流总览（完整管线）

```
.osu 谱面文本（rawText）
  │
  ▼
┌─ ① 解析 ───────────────────────────────────────────────────────────┐
│ patternOsuParser.parseOsuManiaFromText                               │
│   ManiaMapAnalyser by Leo_Black/js/parser/patternOsuParser.js:252    │
│   输入: .osu 文本  →  输出: chart（createChart, chart.js:17）         │
│   chart = { Keys, Notes, BPM, SV, FirstNote, LastNote }              │
│   Notes 为时间行数组，每行 Data 为长度 Keys 的 NoteType 数组          │
└──────────────────────────────────────────────────────────────────────┘
  │
  ▼
┌─ ② 汇总入口 ────────────────────────────────────────────────────────┐
│ service.analyzePatternFromText                                       │
│   ManiaMapAnalyser by Leo_Black/js/patterns/service.js:4             │
│   输入: osuText  →  输出: { report, topFiveClusters }                 │
│   内部委托 fromChart（summary.js:35）                                │
└──────────────────────────────────────────────────────────────────────┘
  │
  ▼
┌─ ③ 基础特征（primitives） ──────────────────────────────────────────┐
│ primitives.calculatePrimitives                                      │
│   ManiaMapAnalyser by Leo_Black/js/patterns/primitives.js:58         │
│   输入: chart → 输出: primitive 行数组（每行: Time/MsPerBeat/Notes/  │
│   Jacks/Direction/Roll/LNHeads/LNBodies/LNTails/NormalNotes...）     │
│ 附带: lnPercent（primitives.js:135，LN 比例）、                       │
│       svTime（primitives.js:152，SV 时间量）                         │
└──────────────────────────────────────────────────────────────────────┘
  │
  ▼
┌─ ④ 模式发现（findPatterns） ────────────────────────────────────────┐
│ findPatterns.find                                                   │
│   ManiaMapAnalyser by Leo_Black/js/patterns/findPatterns.js:109      │
│   输入: primitive 行 → 输出: patterns 段列表                         │
│   （滑动窗口按顺序匹配 6 个核心模式 + 按键数选择的具体模式集）        │
└──────────────────────────────────────────────────────────────────────┘
  │
  ▼
┌─ ⑤ 聚类（clustering） ──────────────────────────────────────────────┐
│ clustering.calculateClusteredPatterns                               │
│   ManiaMapAnalyser by Leo_Black/js/patterns/clustering.js:146        │
│   输入: patterns → 输出: clusters（按 BPM 聚合，统计具体子类型占比， │
│   计算 Amount / Importance / RatingMultiplier）                      │
└──────────────────────────────────────────────────────────────────────┘
  │
  ▼
┌─ ⑥ 分类（categorise） ──────────────────────────────────────────────┐
│ categorise.categoriseChart                                          │
│   ManiaMapAnalyser by Leo_Black/js/patterns/categorise.js:9          │
│   输入: clusters（按 Importance 排序）→ 输出: Category 名称字符串     │
│   （如 "Jumpstream Tech"、"Column Lock"、"Uncategorised"）           │
└──────────────────────────────────────────────────────────────────────┘
  │
  ▼
┌─ ⑦ 汇总（summary） ─────────────────────────────────────────────────┐
│ summary.fromChart                                                   │
│   ManiaMapAnalyser by Leo_Black/js/patterns/summary.js:35            │
│   输出: report { Clusters, Category, LNPercent, HBRowRatio,          │
│                  ModeTag, SVAmount, Duration, ImportantClusters }   │
└──────────────────────────────────────────────────────────────────────┘
  │
  ▼
消费端（浏览器侧，非共享模块）:
  • js/app/analysis.js:606  调用 analyzePatternFromText(rawText)
  • js/app/analysis.js:609  mergeDuplicateClusters（display.js:307）合并同 Pattern 簇
  • js/app/display.js:650   renderPatternClusters 渲染 5 条进度条
  • js/app/analysis.js:792 report.ModeTag 参与模式标签决策（见第 7 节）
```

## 3. 各模块职责表

| 文件（`ManiaMapAnalyser by Leo_Black/js/...`） | 导出符号 | 职责 |
| --- | --- | --- |
| `patterns/chart.js` | `NoteType`（:1）、`createTimeItem`（:9）、`createBPM`（:13）、`createChart`（:17） | 数据模型：音符类型枚举（NOTHING/NORMAL/HOLDHEAD/HOLDBODY/HOLDTAIL）与 chart 结构 |
| `parser/patternOsuParser.js` | `parseOsuManiaFromText`（:252） | .osu 文本 → chart：解析 Difficulty/TimingPoints/HitObjects，处理 Hold（typ & 128）与 SV，无 timing 时兜底 500ms/拍 |
| `patterns/primitives.js` | `calculatePrimitives`（:58）、`lnPercent`（:135）、`svTime`（:152）、`detectDirection`（:34） | 逐行基础特征提取：Note 数、Jacks 数、方向/滚动、LN 头/体/尾分列；LN 比例；SV 时间量（供 SV 谱检测） |
| `patterns/patternsDef.js` | `CorePattern`（:24）、`CORE_PATTERN_LIST`（:33）、6 个 `CORE_*` 核心匹配函数（:158-:202）、`SPECIFIC_4K`（:554）/`SPECIFIC_7K`（:582）/`SPECIFIC_OTHER`（:611）、`resolveRatingMultiplier`（:39） | 模式定义中心：核心模式集合、具体子类型匹配器（按键数分组注册）、评分倍率解析 |
| `patterns/findPatterns.js` | `find`（:109） | 模式发现：滑动窗口贪心匹配核心模式与具体子类型，产出带时间范围的 pattern 段 |
| `patterns/clustering.js` | `calculateClusteredPatterns`（:146） | 聚类：按 BPM 聚合同类段、统计具体子类型占比、计算 Amount（时间总量）/Importance（Amount×倍率×BPM）/RatingMultiplier |
| `patterns/categorise.js` | `categoriseChart`（:9） | 分类：按 Importance 选出主导簇，产出 Category 名称（含 Hybrid/Tech 后缀） |
| `patterns/summary.js` | `fromChart`（:35）、`resolveModeTag`（:28）、`hbRowRatio`（:11） | 汇总编排：调用 find→clustering→categorise，计算 LN 比例、HB 行比例与内部 ModeTag，裁剪/排序簇 |
| `patterns/service.js` | `analyzePatternFromText`（:4） | 对外入口：包一层 fromChart，附 `topFiveClusters`（前 5 簇） |
| `patterns/config.js` | `PATTERNS_CONFIG`（:35） | 全部可调阈值与倍率表（BPM 聚类阈值、SV 阈值、子类型倍率表等） |

## 4. 管线各步详解

### ① 解析 — `parser/patternOsuParser.js:252` `parseOsuManiaFromText`

- 按行切分 `.osu` 文本，分节解析 `[Difficulty]`（CircleSize → Keys，缺省 4）、`[TimingPoints]`、`[HitObjects]`。
- 命中 `[HitObjects]` 中 `type & 128` 的物件标记为 Hold，记录 `EndTime`（`patternOsuParser.js:236-247`）；其余为 HitCircle。
- 输出 `createChart(keys, snaps, bpm, sv)`（`chart.js:17`）：`Notes` 为时间行数组，每行 `Data` 是长度为 `Keys` 的 `NoteType` 数组——这是后续所有计算的统一输入。无 timing points 时兜底 `createBPM(4, 500.0)` 与速度 1.0（`patternOsuParser.js:279-282`）。

### ② 汇总入口 — `patterns/service.js:4` `analyzePatternFromText`

极薄封装：`const chart = parseOsuManiaFromText(osuText)` → `fromChart(chart)`，返回 `{ report, topFiveClusters: report.Clusters.slice(0, 5) }`（`service.js:9-12`）。`rate` 参数当前未使用（`service.js:5` 的 `void rate`）。

### ③ 基础特征 — `patterns/primitives.js:58` `calculatePrimitives`

把时间行数组转换成"每行一个 primitive 对象"的序列，是模式匹配的输入。每行包含：

- `Notes`：该行 NORMAL+HOLDHEAD 数量；`Jacks`：与上一行重合的列数（`primitives.js:106-107`）；
- `Direction` / `Roll`：由 `detectDirection`（`primitives.js:34`，基于左右边界列位移判断 Left/Right/Inwards/Outwards/None）得出；
- `LNHeads` / `LNBodies` / `LNTails` / `NormalNotes`：按列区分的 LN 与普通音符（`primitives.js:87-94`）——**LN 检测的数据基础**；
- `MsPerBeat`、`BeatLength`、`LeftHandKeys`（`keysOnLeftHand`，`primitives.js:12`）。

同一时间点无任何音符的行会被跳过（`primitives.js:96-98`）。同文件另有：

- `lnPercent`（`primitives.js:135`）：`lnotes / notes`，HOLDHEAD 计入总数与 LN 数；
- `svTime`（`primitives.js:152`）：累计速度 ≠1 的时间区间（`SV_SPEED_EPS` 容差），极端 BPM（`SV_EXTREME_BPM_*`）时强制超过 `SV_AMOUNT_THRESHOLD`——供 SV 谱检测（见第 8 节）。

### ④ 模式发现 — `patterns/findPatterns.js:109` `find`

- 按键数选择具体模式集：`chart.Keys === 4` → `SPECIFIC_4K()`，`=== 7` → `SPECIFIC_7K()`，其余 → `SPECIFIC_OTHER()`（`findPatterns.js:113-115`）。
- `matches`（`findPatterns.js:91`）对 primitive 序列做**滑动窗口贪心匹配**：从头部依次调用 6 个核心匹配函数 `CORE_STREAM`/`CORE_CHORDSTREAM`/`CORE_JACKS`/`CORE_COORDINATION`/`CORE_DENSITY`/`CORE_WILDCARD`（`findPatterns.js:96-101`），命中即消费对应数量的行（`remaining = remaining.slice(1)` 前进一格），未命中则只前进 1 行。
- 每个匹配段由 `appendFoundPattern`（`findPatterns.js:40`）产出 `{ Pattern, SpecificType, Mixed, Start, End, MsPerBeat }`；`Mixed` 表示段内 MsPerBeat 波动超过 `PATTERN_STABILITY_THRESHOLD`（`findPatterns.js:43`）。
- `ENABLE_MULTI_LABEL_SAME_WINDOW`（`config.js:110`，默认 true）控制同一窗口是否允许多个具体子类型标签同时记录（`appendCoreMatches`，`findPatterns.js:65-89`）。

### ⑤ 聚类 — `patterns/clustering.js:146` `calculateClusteredPatterns`

- `assignClusters`（`clustering.js:42`）：按 `BPM_CLUSTER_THRESHOLD`（5.0 BPM）把同型段聚为 BPM 簇；Mixed 段按 Pattern 单独聚类（`clustering.js:71`）。簇 BPM = 段内平均 MsPerBeat 反算（`createClusterBuilder.calculate`，`clustering.js:32-35`）。
- `specificClusters`（`clustering.js:81`）：按 `Pattern@@mixed@@BPM` 分组（`clustering.js:86`），统计各具体子类型占比（`SpecificTypes`，按占比降序），合并重叠时间区间得到 `Amount`（`patternAmount`，`clustering.js:4`）。
- 每个簇带 `RatingMultiplier`（`resolveRatingMultiplier`，见第 6 节）与 `Importance` getter = `Amount × RatingMultiplier × BPM`（`clustering.js:119-121`）；`format(rate)` 生成 `"185BPM Jumpstream"` / `"~180BPM Mixed Jacky WC"` 之类的显示文本（`clustering.js:122-130`）。
- 谱面同时存在 Density/Wildcard 簇且含 Release 子类型时，Release 簇倍率乘以 `RELEASE_WITH_DW_MULTIPLIER`（0.8，`clustering.js:134-141`）。

### ⑥ 分类 — `patterns/categorise.js:9` `categoriseChart`

- 取 `Importance` 降序中比值超过 `IMPORTANT_CLUSTER_RATIO`（0.5）的簇为重要簇（`categorise.js:17-25`）。
- 主导簇命名：首选占比 > 0.05 的具体子类型名（`categorise.js:34`）；特判 Jumpstream/Handstream 混合（`CATEGORY_JS_HS_SECONDARY_RATIO`，`categorise.js:36-45`）；否则用核心模式名。
- `isHybridChart` 目前恒为 false（`categorise.js:3-7`），故结果只会带 "Tech" 后缀（主导簇 `Mixed` 时）：`"Jumpstream Tech"` 等；无簇时返回 `"Uncategorised"`（`categorise.js:13-15`）。

### ⑦ 汇总 — `patterns/summary.js:35` `fromChart`

编排 + 后处理，产出最终 report：

1. `lnPercent` + `hbRowRatio`（`summary.js:11`，HB 行 = 同时含 HOLDHEAD 与 NORMAL 的行）→ `resolveModeTag`（`summary.js:28`）：LN 比例 ≤ 0.15 → `RC`，≥ 0.9 → `LN`，HB 行比例 ≥ 0.1 → `HB`，否则 `Mix`（阈值见 `config.js:95-97`）。
2. **RC 模式过滤**：`modeTag === "RC"` 时剔除 LN 核心模式（Coordination/Density/Wildcard）的段（`summary.js:41-43`）——避免 RC 谱面上 LN 模式污染结果。
3. `calculateClusteredPatterns(patterns, { modeTag })` → 过滤 BPM ≤ 25 的簇（`summary.js:46`）→ 按 `Amount` 降序。
4. 同 Pattern 剪枝（`canBePruned`，`summary.js:49-56`）：同型且时间量不足一半、BPM 更低者剔除；每 Pattern 保留至多 3 簇（`summary.js:61-63`），再按 `Importance` 降序。
5. `categoriseChart(chart.Keys, prunedClusters, svAmount)` 得 `Category`。

## 5. LN 检测说明

- `js/patterns/` 的 **RC 键型分析**移植自 [Interlude (YAVSRG)](https://github.com/YAVSRG/YAVSRG) 的 RC 分析算法；**LN 检测是本项目在 Interlude 基础上的新增**（见 CLAUDE.md 项目结构说明）。
- Interlude 本体（星数计算模块）位于 `ManiaMapAnalyser by Leo_Black/js/interlude/`，其内部算法本文档不展开，如需了解请直接阅读该目录源码。
- 本项目 LN 检测的数据基础在 `chart.js:1` `NoteType`（HOLDHEAD/HOLDBODY/HOLDTAIL 三类 LN 标记）→ `primitives.js:87-94` 的 `LNHeads/LNBodies/LNTails` 分列 → `patternsDef.js` 中以 LN 为条件的核心/具体模式（见下节）。`patternsDef.js:40` `resolveRatingMultiplier` 中 `lnCorePatterns`（Coordination/Density/Wildcard）与 `rcCorePatterns`（Stream/Chordstream/Jacks）的区分即 RC/LN 两大体系的分界。

## 6. CorePattern 体系（`patterns/patternsDef.js`）

### 核心模式（6 个，`patternsDef.js:24-31` `CorePattern`）

| 核心模式 | 匹配函数 | 判据要点（简化） | 体系 |
| --- | --- | --- | --- |
| `Stream` | `CORE_STREAM`（:158） | 连续 5 行单键、无 jack、首尾不同列 | RC |
| `Chordstream` | `CORE_CHORDSTREAM`（:179） | 4 行内多键且后续行存在多键、无 jack | RC |
| `Jacks` | `CORE_JACKS`（:173） | 当前行 jack ≥ 2 且 MsPerBeat < 2000 | RC |
| `Coordination` | `CORE_COORDINATION`（:188） | 当前行存在 LN 头/体/尾 | LN |
| `Density` | `CORE_DENSITY`（:194） | 当前行存在 LN 头（`isLnHeadContext`） | LN |
| `Wildcard` | `CORE_WILDCARD`（:199） | 当前行存在 LN 头（兜底） | LN |

### 具体子类型（按键数注册）

`SPECIFIC_4K`（:554）、`SPECIFIC_7K`（:582）、`SPECIFIC_OTHER`（:611）返回 `makeSpecificPatterns` 结构（:543），每个核心模式挂一组具体匹配器，按 `COORDINATION_SPECIFIC_ORDER`/`DENSITY_SPECIFIC_ORDER`/`WILDCARD_SPECIFIC_ORDER`（`config.js:111-113`）排序（`reorderSpecific`，:66）。例如：

- Stream（4K）：`STREAM_4K_ROLL`（:295）、`STREAM_4K_TRILL`（:306）、`STREAM_4K_MINITRILL`（:315）；
- Chordstream（4K）：Handstream（:250）、Jumpstream（:256）、Double/Triple Jumpstream（:265/:274）、Jumptrill（:283）、Split Trill（:289）；
- Jacks：Longjacks（:219）、Quadstream（:232）、Gluts（:238）、Chordjacks（:204）、Minijacks（:213）；
- Coordination（LN）：`COORDINATION_COLUMN_LOCK`（:367）、`COORDINATION_SHIELD`（:395）、`COORDINATION_RELEASE`（:411）；
- Density（LN）：`DENSITY_4K_JUMPSTREAM`（:468）、`DENSITY_4K_HANDSTREAM`（:473）、`DENSITY_4K_INVERSE`（:478）等（基于 `headRows` 提取 LN 头列再套用 RC 判据，:109）；
- Wildcard（LN）：`WILDCARD_JACK`（:503）、`WILDCARD_SPEED`（:520）。

7K/其他键数通过别名复用（如 `CHORDSTREAM_OTHER_DOUBLE_STREAMS = CHORDSTREAM_7K_DOUBLE_STREAMS`，:362-365）。

### `resolveRatingMultiplier`（`patternsDef.js:39`）的作用

把"簇的相对重要性"量化为一个倍率，参与 `Importance = Amount × RatingMultiplier × BPM`（`clustering.js:119-121`），进而影响聚类排序与分类。逻辑：

1. 基线 = 核心模式倍率 `CORE_RATING_MULTIPLIER`（`config.js:36-43`：Stream 1/3、Chordstream 0.65、Jacks 0.9、Coordination 0.75、Density 0.9、Wildcard 1.0）；
2. 有具体子类型时，用 `SUBTYPE_RATING_MULTIPLIER_BY_MODE[modeTag]`（`config.js:44-91`，RC/LN/HB/Mix 四套表，如 "Column Lock" 1.5、"Release" RC 0.73 / LN 1.0 / HB-Mix 0.3）覆盖基线；
3. 模式修正（`patternsDef.js:51-61`）：`modeTag === "RC"` 时 LN 核心模式改用 Mix 表 × `RC_LN_CORE_SCALE`（0.0 → 归零，LN 模式不参与 RC 谱）；`modeTag === "LN"` 时 RC 核心模式 × `RC_CORE_LN_SCALE`（0.3）。

## 7. `debugUseAmount` 设置的影响

- 定义：`settings.json:405` `debugUseAmount`（checkbox，"Use Amount For Category"，默认 false；属于 `settings.json:397` 的 Debug Options 分组），运行时状态见 `js/app/appContext.js:82`，解析器 `js/parser/settingsParser.js:289-290`。
- 作用位置：`js/app/analysis.js:611-621`（**浏览器侧消费逻辑，不在共享的 `js/patterns/` 内**）。在 `mergeDuplicateClusters`（`display.js:307`，按 `Pattern` 合并同型簇、Amount 求和、BPM 取最大、子类型占比按 Amount 加权归一）之后：
  - **默认（false）**：`Category` 保持 `categoriseChart` 基于 **Importance** 的判定结果；
  - **开启（true）**：`mergedClusters` 改为按 **Amount** 降序（`analysis.js:612`），并把 `report.Category` 强制改为 Amount 最大的簇——其 `SpecificTypes[0]` 占比 > 0.05 时取具体子类型名，否则取核心模式名（`analysis.js:613-620`）。
- 即：默认分类看"重要程度（Importance）"，开启后改看"时间总量（Amount）"。该设置**只改分类来源，不改簇本身的计算**；且它不参与结果缓存键，切换时会触发 `clearResultCache()`。

## 8. 与 modeLogic 的衔接（模式标签）

模式标签判定存在**两层**，需注意区分：

1. **`js/patterns/` 内部（共享层）**：`summary.js:28` `resolveModeTag` 基于 `lnPercent`/`hbRowRatio` 产出 `report.ModeTag`（RC/LN/HB/Mix，阈值 `config.js:95-97`），并据此过滤 LN 模式（`summary.js:41-43`）与选择倍率表（第 6 节）。
2. **`js/app/modeLogic.js`（浏览器层）**：`modeTagFromLnRatio`（`modeLogic.js:1`，仅按 LN 比例 0.15/0.9 二分 RC/Mix/LN，无 HB）作为**兜底**（`analysis.js:792` 用 `rework.lnRatio ?? parsedInfo.lnRatio`）；最终 `resolvedModeTag = (activeContentBar === "None") ? fallbackModeTag : (report.ModeTag || fallbackModeTag)`（`analysis.js:793-795`）——即只要显示 Pattern 内容，优先用 `js/patterns/` 内部更完整的判定。
3. 标签渲染：`setModeTag`（`js/app/hud.js:143`）/ `setModeTagAdvanced`（`hud.js:171`，基于 typePercentageData）；`resolveAutoDisplayProfile`（`modeLogic.js:31`）据此决定自动档位（RC → Etterna/MSD，其余 → Pattern/ReworkSR）。

详细模式标签功能见 [mode-tagging.md](mode-tagging.md)。

## 9. 注意事项

1. **非 4/6/7K 谱面主体回退 Pattern 显示**：`js/app/appContext.js:180` `GRAPH_SUPPORTED_KEY_SET = Set([4, 6, 7])`。当键数不在其中、且 `state.contentBar` 非 "None"/"Full" 时，`analysis.js:353-357` 通过 `setEffectiveContentBarForMap("Pattern")`（`js/app/settings.js:422`，per-map 覆盖，存于 `state.effectiveContentBar`）把主体内容强制回退为 Pattern 显示（Full 模式下由图表块自行显示 "Unsupported Keys" 提示，`analysis.js:351-352` 注释）。`js/patterns/` 本身对任意键数均能计算（`SPECIFIC_OTHER` 兜底）。
2. **SV 检测对分类的影响**：`useSvDetection` 开启时（`analysis.js:798-806`），若 `report.SVAmount ≥ SV_AMOUNT_THRESHOLD`（2000ms，`config.js:102`，由 `svTime` `primitives.js:152` 计算），`report.Category` 被覆盖为 `"SV"` 并显示 SV 标签；`svTime` 对极端 BPM 会强制超阈值（`primitives.js:217-219`）。注意 SV 覆盖发生在 app 层，`js/patterns/` 内的 `Category` 不受影响。
3. **vibro 检测的影响**：vibro 检测在 `js/app/vibro.js`——`detectVibro`（`vibro.js:16`，基于 Etterna 数值与 jack 速度比，`analysis.js:655-657`）与 `detectVibroFromLongjackPattern`（`vibro.js:27`，基于 pattern report 的 Longjacks 簇）。它**不直接改 Category**，命中时 `setForceHideNumericDifficulty(isVibroMap)`（`analysis.js:824`）隐藏 Numeric Difficulty 显示。`config.js:107-108` 的 `LONGJACK_VIBRO_*` 阈值供 vibro 判定使用。
4. **`needPatternAnalysis` 触发条件**（`analysis.js:410-415`）：Pattern 显示、srText/diffText 为 "Pattern"、`useSvDetection`、vibro 检测或自动档位启用任一满足即运行——模式分析可能被"顺带"执行以服务其他功能，即使界面上没显示 Pattern 条。
5. **共享模块约束**：`js/patterns/` 与 `js/parser/patternOsuParser.js` 在 Node benchmark runner 中也会加载，禁止引入 `window`/`document`；新增文件 import 必须带 `.js` 扩展名（浏览器解析习惯与 Node esm-loader 的要求）。
6. **`rate` 参数未使用**：`service.js:4` 的 `rate` 目前被忽略（`void rate`），倍速换算由调用侧（`analysis.js` 传入原始文本）或显示层 `format(rate)`（`clustering.js:122`）处理。

## 10. 相关文件

- 共享计算：`ManiaMapAnalyser by Leo_Black/js/patterns/`（service/summary/primitives/findPatterns/clustering/categorise/patternsDef/config/chart）、`ManiaMapAnalyser by Leo_Black/js/parser/patternOsuParser.js`
- 浏览器消费：`ManiaMapAnalyser by Leo_Black/js/app/analysis.js`、`js/app/display.js`、`js/app/hud.js`、`js/app/modeLogic.js`、`js/app/vibro.js`、`js/app/appContext.js`
- 设置定义：`ManiaMapAnalyser by Leo_Black/settings.json`（`debugUseAmount` :405）、`js/parser/settingsParser.js`
- 上游参考：Interlude (YAVSRG) RC 算法 → `ManiaMapAnalyser by Leo_Black/js/interlude/`
