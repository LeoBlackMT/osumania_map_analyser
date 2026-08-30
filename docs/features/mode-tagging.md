# 模式标签功能文档（mode-tagging）

> 面向 AI 的功能技术文档。描述插件的模式判定（HB/RC/LN/Mix）、SV 检测、vibro 检测、LN 成分分析（Analyze LN Parts）以及左下角标签胶囊 UI 的完整实现。文中 path:line 行号为编写时快照，代码演进后可能漂移；定位源码请以符号名（symbol）为准，必要时用 grep 复核。
>
> 相关文档：[settings.md](../settings.md)（设置说明，人类）、[pattern-analysis.md](pattern-analysis.md)（键型分析）、[difficulty-estimation.md](difficulty-estimation.md)（难度估计）。

## 1. 功能总览

插件在卡片左下角显示一个"模式标签胶囊"（mode tag capsule），用于指示当前谱面的类型。标签体系包含四种模式标签 **HB / RC / LN / Mix**，以及一个独立的 **SV 徽章**（`sv-tag`）。此外还有可选的**成分百分比模式**（Analyze LN Parts），将标签胶囊扩展为多个百分比标签（All/RC/Mix/HB/LN）。

关键数据流：

```
模式标签（HB/RC/LN/Mix）:
  patternReport.ModeTag (js/patterns/summary.js resolveModeTag)
  或回退 modeTagFromLnRatio(rework.lnRatio) (js/app/modeLogic.js)
  → analysis.js 决定 resolvedModeTag
  → setModeTag / setModeTagAdvanced 渲染

SV 徽章:
  patternReport.SVAmount (svTime) >= SV_AMOUNT_THRESHOLD → setSvTagVisible(true)

Vibro:
  detectVibro(ettResult.values) >= 0.95 → isVibroMap → 难度显示 "VIBRO"
```

## 2. 模式标签判定（HB/RC/LN/Mix）

### 2.1 标签判定函数

模式标签存在**两套独立的判定实现**，分别服务于不同场景：

**a) `ManiaMapAnalyser by Leo_Black/js/patterns/summary.js:28 resolveModeTag(lnRatio, hbRatio)`** — 键型分析报告的 ModeTag 来源：

- `js/patterns/summary.js:29` — `lnRatio <= PATTERNS_CONFIG.LN_MODE_LOW_THRESHOLD (0.15)` → `"RC"`
- `js/patterns/summary.js:30` — `lnRatio >= PATTERNS_CONFIG.LN_MODE_HIGH_THRESHOLD (0.9)` → `"LN"`
- `js/patterns/summary.js:31` — 否则若 `hbRatio >= PATTERNS_CONFIG.HB_ROW_RATIO_THRESHOLD (0.1)` → `"HB"`
- `js/patterns/summary.js:32` — 其余情况 → `"Mix"`

其中 `hbRatio` 由 `summary.js:11 hbRowRatio(chart)` 计算：同一行内同时存在 HOLDHEAD 与 NORMAL 记为 HB 行，`hbRows / 总行数` 即为 HB 行占比。阈值常量位于 `ManiaMapAnalyser by Leo_Black/js/patterns/config.js:95-97`（`LN_MODE_LOW_THRESHOLD: 0.15`、`LN_MODE_HIGH_THRESHOLD: 0.9`、`HB_ROW_RATIO_THRESHOLD: 0.1`）。

**b) `ManiaMapAnalyser by Leo_Black/js/app/modeLogic.js:1 modeTagFromLnRatio(lnRatio)`** — 简易回退判定（无 HB 分支）：

- `modeLogic.js:2-4` — `lnRatio` 非有限数 → `"Mix"`
- `modeLogic.js:5-7` — `lnRatio <= 0.15` → `"RC"`
- `modeLogic.js:8-10` — `lnRatio >= 0.9` → `"LN"`
- `modeLogic.js:11` — 其余 → `"Mix"`

> ⚠️ 注意：`modeTagFromLnRatio` 只输出 RC/LN/Mix 三种标签，**不会输出 HB**。HB 标签只能由键型分析路径（`resolveModeTag` + `hbRowRatio`）产生。修改任一实现时需保持两者对 RC/LN 阈值的一致性。

### 2.2 最终标签的解析（analysis.js）

`ManiaMapAnalyser by Leo_Black/js/app/analysis.js:792-795`：

```js
const fallbackModeTag = modeTagFromLnRatio(Number(rework?.lnRatio ?? parsedInfo.lnRatio));
let resolvedModeTag = (activeContentBar === "None")
    ? fallbackModeTag
    : (patternResult?.report?.ModeTag || fallbackModeTag);
```

- `analysis.js:792` — 回退标签来自 `modeTagFromLnRatio`，输入优先取 rework 结果的 `lnRatio`，取不到再用解析器 `parsedInfo.lnRatio`。
- `analysis.js:793-795` — 当 contentBar 为 `"None"` 时只用回退标签（此时不展示键型分析区，避免依赖 pattern 报告）；否则优先取 `patternResult.report.ModeTag`（`summary.js:74 ModeTag: modeTag`），缺失时回退。

### 2.3 Mixed 估算器内的标签使用

`ManiaMapAnalyser by Leo_Black/js/estimator/mixedEstimator.js:204` 也调用 `modeTagFromLnRatio`（本地副本，`mixedEstimator.js:18`），用于判断是否走 RC 分支。注意这是**估算器内部逻辑**，与 UI 标签无关，修改阈值时需同步评估。

## 3. modeLogic.js 职责

`ManiaMapAnalyser by Leo_Black/js/app/modeLogic.js`（全文 43 行）包含：

| 行号 | 符号 | 职责 |
|---|---|---|
| `:1` | `modeTagFromLnRatio(lnRatio)` | LN 占比 → RC/LN/Mix 标签（见 §2.1b） |
| `:14` | `normalizeClientStateName(value)` | tosu 客户端状态名归一化（小写、去非字母），用于 play/resultscreen 判断 |
| `:21` | `isPlayStateName(normalizedStateName)` | 判断是否为 play/gameplay/playing 状态 |
| `:27` | `isResultScreenStateName(normalizedStateName)` | 判断是否为 resultscreen 状态 |
| `:31` | `resolveAutoDisplayProfile(modeTag)` | Auto 显示档位解析（见 §3.1） |

### 3.1 resolveAutoDisplayProfile — Auto 档位自动选择

`modeLogic.js:31 resolveAutoDisplayProfile(modeTag)` 根据模式标签返回 contentBar/srText 的 Auto 默认值：

- `modeLogic.js:32-37` — `modeTag === "RC"` → `{ contentBar: "Etterna", srText: "MSD" }`（RC 谱面自动展示 Etterna 技能条 + MSD）
- `modeLogic.js:39-42` — 其他（LN/HB/Mix）→ `{ contentBar: "Pattern", srText: "ReworkSR" }`

调用链（仅当用户将 `contentBar` 或 `srText` 设为 `"Auto"` 时生效）：

- `ManiaMapAnalyser by Leo_Black/js/app/settings.js:84 isAutoDisplayEnabled()` — 任一 user 字段为 "Auto" 即开启。
- `settings.js:88 resolveRuntimeDisplayProfile(modeTag)` — 将 Auto 字段替换为 `resolveAutoDisplayProfile` 的输出，非 Auto 字段保持用户选择。
- `settings.js:473 refreshAutoDisplayProfile(modeTag = state.currentModeTag || "Mix")` — 运行时刷新入口；在 `analysis.js:829` 每次分析完成后调用（`refreshAutoDisplayProfile(resolvedModeTag)`），以及在 `settings.js:484`/`:498` 用户设置变更时调用。
- 注意：`refreshAutoDisplayProfile` 的默认参数取 `state.currentModeTag`（`ManiaMapAnalyser by Leo_Black/js/app/appContext.js:124 currentModeTag: "Mix"`），而分析路径显式传入 `resolvedModeTag` 以保证与实际判定一致。

## 4. Vibro 检测

### 4.1 检测逻辑（vibro.js）

`ManiaMapAnalyser by Leo_Black/js/app/vibro.js` 提供两个纯函数：

**a) `vibro.js:16 detectVibro(values, threshold)`** — 主流程使用的检测（基于 Etterna MSD 技能值）：

- `vibro.js:17` — 从 `values` 中取 `Overall`（兼容 `overall` 小写）。
- `vibro.js:18` — 取 `JackSpeed`（兼容 `Jackspeed`/`jackSpeed`/`jackspeed` 大小写变体）。
- `vibro.js:20-22` — `Overall <= 0` 或 JackSpeed 非有限数 → 非 vibro。
- `vibro.js:24` — 判定条件：`jackSpeed / overall >= threshold`。

**b) `vibro.js:27 detectVibroFromLongjackPattern(patternReport, threshold, minBpm)`** — 基于键型聚类报告的备选检测：

- 遍历 `patternReport.Clusters`，跳过 BPM 低于 `minBpm` 的聚类（`vibro.js:38-41`）。
- 在聚类 `SpecificTypes` 中查找 `"Longjacks"` 且占比 `ratio >= threshold`（`vibro.js:42-45`）。

> ⚠️ 当前主流程只调用 `detectVibro`（见 §4.2）；`detectVibroFromLongjackPattern` 仅有定义、未被任何调用点引用。接入或改造时注意其输入是 pattern 报告而非 Etterna 值。

### 4.2 主流程接入（analysis.js）

`ManiaMapAnalyser by Leo_Black/js/app/analysis.js:653-657`：

```js
const reworkStarValue = Number(rework?.star);
const vibroEligible = Number.isFinite(reworkStarValue) && reworkStarValue > 5.0;
// MSD 基准固定 0.72.3：resolveVibroMsdValues 在主结果版本 != 0.72.3 时补算
const vibroValues = await resolveVibroMsdValues(rawText, ettResult);
isVibroMap = state.vibroDetection
    && vibroEligible
    && detectVibro(vibroValues, VIBRO_JACKSPEED_RATIO_THRESHOLD);
```

三个条件缺一不可：

1. `analysis.js:655` — 设置开关 `state.vibroDetection`（对应 settings.json 的 `VibroDetection`）。
2. `analysis.js:654` — rework 星数必须 `> 5.0`（vibro 谱面通常是高密度高星谱，低星谱不做检测）。
3. `analysis.js:657` — `detectVibro(vibroValues, VIBRO_JACKSPEED_RATIO_THRESHOLD)` 命中，`vibroValues` 为 **0.72.3 版本**的 Etterna MSD（`resolveVibroMsdValues`：主结果 `etternaVersion === "0.72.3"` 时直接复用，否则用 0.72.3 单独补算；补算失败回退主结果）。非 4K 谱面下 0.72.3 输出全 0，vibro 自然判定不命中。

阈值来源链：`ManiaMapAnalyser by Leo_Black/config.js:49 vibroJackspeedRatioThreshold: 0.95`（小驼峰命名）→ `ManiaMapAnalyser by Leo_Black/js/app/appContext.js:157 VIBRO_JACKSPEED_RATIO_THRESHOLD = APP_CONFIG.etterna.vibroJackspeedRatioThreshold` → `analysis.js:657` 使用。

依赖关系：`analysis.js:409 needVibroDetection = state.vibroDetection` 会使键型分析（`:410-415`）与 Etterna 分析（`:420-423`）按需开启，保证 `ettResult.values` 与 `patternReport` 可用。

### 4.3 Vibro 对显示的影响

检测为 vibro 谱面后（`isVibroMap = true`）：

- `analysis.js:824 setForceHideNumericDifficulty(isVibroMap)` — 隐藏数值难度（实现见 `ManiaMapAnalyser by Leo_Black/js/app/graph.js:737 setForceHideNumericDifficulty(value)`）。vibro 谱面的数值难度会被极度拉高，无参考价值。
- `analysis.js:898-900` — 当 `diffText === "Difficulty"` 时，难度文本直接显示 `"VIBRO"`。

> 关联设置说明见 `docs/settings.md:74-75`：不启用 vibro 检测时，"您将看到被极度拉高的难度估计"。这与 [difficulty-estimation.md](difficulty-estimation.md) 中数值难度（Numeric Difficulty）显示逻辑相互影响。

## 5. SV 检测

### 5.1 svTime — SV 量统计

`ManiaMapAnalyser by Leo_Black/js/patterns/primitives.js:152 svTime(chart)` 计算谱面的"变速总时长"（毫秒），用作 SV 强度指标：

- `primitives.js:153` — 无 SV 事件直接返回 0。
- `primitives.js:161-178` — 遍历 SV 事件，累计速度偏离 1 的时间段（判定基准 `PATTERNS_CONFIG.SV_SPEED_EPS`，`js/patterns/config.js:103 SV_SPEED_EPS: 0.05`）；同时统计非 1 速度区间段数 `nonOneIntervals`。
- `primitives.js:184-186` — 非 1 区间段数 `<= 1` → 返回 0（单段变速不构成 SV 谱）。
- `primitives.js:188-215` — 极端 BPM 检查：BPM 超出 `SV_EXTREME_BPM_MIN/MAX`（`js/patterns/config.js:104-105`，20/450）或相邻 BPM 比值 `>= SV_EXTREME_BPM_RATIO`（`js/patterns/config.js:106`，4.0）视为极端变速谱。
- `primitives.js:217-219` — 极端时返回 `Math.max(total, SV_AMOUNT_THRESHOLD + 1.0)`，确保必然超过判定阈值。

输出通过 `ManiaMapAnalyser by Leo_Black/js/patterns/summary.js:66 svAmount = svTime(chart)` 写入报告 `summary.js:75 SVAmount: svAmount`。

### 5.2 useSvDetection 触发 SV 标签

`ManiaMapAnalyser by Leo_Black/js/app/analysis.js:798-806`：

```js
if (state.useSvDetection) {
    const svAmount = Number(patternReport?.SVAmount);
    if (Number.isFinite(svAmount) && svAmount >= PATTERNS_CONFIG.SV_AMOUNT_THRESHOLD) {
        shouldShowSvTag = true;
        if (patternReport && typeof patternReport === "object") {
            patternReport.Category = "SV";
        }
    }
}
```

- `analysis.js:798` — 开关 `state.useSvDetection`（settings.json `useSvDetection`）。
- `analysis.js:800` — `SVAmount >= PATTERNS_CONFIG.SV_AMOUNT_THRESHOLD`（`js/patterns/config.js:102 SV_AMOUNT_THRESHOLD: 2000.0`，即 2 秒）。
- `analysis.js:803` — 命中时**覆盖** `patternReport.Category = "SV"`，影响右胶囊的键型分类显示（`analysis.js:887-893 renderRightCapsule` 的 `patternReport?.Category` 参数）。

> ⚠️ 澄清：SV 检测**不会替换**模式标签（HB/RC/LN/Mix 仍按 §2 显示在标签胶囊），而是以独立 SV 徽章形式展示（见 §6.3）。`MODE_TAG_OPTIONS` 中虽有 `"SV"`（`config.js:16 options.modeTag`），但当前流程从未通过 `setModeTag("SV")` 渲染它——SV 一律走 `svTagEl` 徽章路径。

### 5.3 SV 标签显示入口

- `analysis.js:814 setSvTagVisible(shouldShowSvTag)` — 每次分析完成后调用。
- `analysis.js:237-238` — 清空/加载路径调用 `setModeTag("Mix")` + `setSvTagVisible(false)` 复位。

## 6. Analyze LN Parts（成分百分比标签）

### 6.1 数据来源

`ManiaMapAnalyser by Leo_Black/js/rework/sunnyWindowAlgorithm.js:335 shouldCalcData = state.enableAnalyzeLN` 控制 SunnyWindow 是否计算成分百分比数据：

- `sunnyWindowAlgorithm.js:333` — `getLNParts` 将谱面按 LN 段裁剪。
- `sunnyWindowAlgorithm.js:349` — 无 LN 段时 `typePercentageData` 仍按开关计算（`shouldCalcData ? getTypePercentageData(...) : null`）。
- `sunnyWindowAlgorithm.js:324-330` — 百分比列表格式：`[["All", 总列数], ["RC", n], ["Mix", n], ["HB", n], ["LN", n]]`，按类型列数计数。

分析侧：`ManiaMapAnalyser by Leo_Black/js/app/analysis.js:520-533`（`shouldForceSunnyWindow` 分支）调用 `runSunnyWindowEstimatorFromText` 并在 `analysis.js:524 typePercentageData = sunnyWindowRework.typePercentageData` 取出。命中缓存时 `analysis.js:438` 从快照恢复。

> ⚠️ **前置依赖**：设置 `enableAnalyzeLN` 依赖 `forceSunnyWindow`（"Improve Sunny LN Estimation"）。`settings.json:317-320` 的 description 明确标注 `[Require Improve Sunny LN Estimation]`。若 `forceSunnyWindow` 关闭，`analysis.js:521` 的分支不执行，`typePercentageData` 保持 `null`（`analysis.js:400`），标签胶囊退化为单一模式标签。

### 6.2 渲染切换

`ManiaMapAnalyser by Leo_Black/js/app/analysis.js:808-814`：

```js
if (typePercentageData) {
    const lnRatio = Number(rework?.lnRatio ?? parsedInfo.lnRatio)
    setModeTagAdvanced(typePercentageData, lnRatio);
} else {
    setModeTag(resolvedModeTag);
}
setSvTagVisible(shouldShowSvTag);
```

有百分比数据 → `setModeTagAdvanced` 多标签渲染；否则 → `setModeTag` 单标签。

## 7. 标签 UI（hud.js）

### 7.1 DOM 元素

- `ManiaMapAnalyser by Leo_Black/js/app/appContext.js:60 modeTagSubGroupEl = document.getElementById("mode-tag-subgroup")` — 模式标签容器（内含多个 `span.mode-tag`）。
- `appContext.js:61 svTagEl = document.getElementById("sv-tag")` — 独立 SV 徽章。
- `appContext.js:120 showModeTagCapsule` — 胶囊显示开关（settings.json `showModeTagCapsule`）。
- `appContext.js:121 showSvTag` — SV 徽章显示状态（运行时标志）。
- `appContext.js:124 currentModeTag` — 当前模式标签（供 graph 使用）。
- `appContext.js:153 MODE_TAG_OPTIONS = APP_CONFIG.options.modeTag` — `["RC", "LN", "HB", "Mix", "SV"]`（`config.js:16`）。

### 7.2 setModeTag — 单标签渲染

`ManiaMapAnalyser by Leo_Black/js/app/hud.js:143 setModeTag(tag)`：

1. `hud.js:144` — 写入 `state.currentModeTag = tag`。
2. `hud.js:149` — `modeTagSubGroupEl.hidden = !state.showModeTagCapsule`（胶囊关闭时整个容器隐藏）。
3. `hud.js:151-154` — 容器无子元素时补建一个 `span`。
4. `hud.js:156-161` — 渲染文本与类名 `mode-tag mode-{tag.toLowerCase()}`（如 `mode-rc`、`mode-ln`），仅在内容变化时更新。
5. `hud.js:162-164` — 内容变化且胶囊开启时重放 `capsule-switch` 动画（`hud.js:18 restartAnimationClass`）。
6. `hud.js:166-168` — 多余子元素加 `hidden-tag` 类隐藏（从多标签态退化时清场）。

### 7.3 setModeTagAdvanced — 多标签百分比渲染

`ManiaMapAnalyser by Leo_Black/js/app/hud.js:171 setModeTagAdvanced(tag, lnRatio)`：

1. `hud.js:172-176` — 兼容旧调用：传入普通字符串时构造 `[["All", 1], [realTag, 1]]`，非法 tag 回退 `"Mix"`。
2. `hud.js:177` — `shift()` 取出总计数 `allCount`。
3. `hud.js:179-182` — 按计数降序排序，相等时按 `MODE_TAG_OPTIONS` 索引序。
4. `hud.js:183` — 各类型计数换算为百分比：`count * 100 / allCount`。
5. `hud.js:184` — 过滤掉 0% 的类型。
6. `hud.js:186-189` — **currentModeTag 更新**（用于 graph 着色，见 `ManiaMapAnalyser by Leo_Black/js/app/graph.js:217` 中 `state.currentModeTag === "RC"` 判断）：`lnRatio > 0.15` 时取占比列表中第一个 LN/HB 标签，否则 `"RC"`。
7. `hud.js:194` — 胶囊显隐同步。
8. `hud.js:196-210` — 逐项渲染 `span`，文本为 `"TAG"`（100%）或 `"TAG 45%"`（`hud.js:202`），类名 `mode-tag mode-{tag.toLowerCase()}`，变化时重放 `capsule-switch`。
9. `hud.js:211-213` — 多余的旧子元素加 `hidden-tag`。

### 7.4 SV 徽章显示/隐藏

- `hud.js:237 setSvTagVisible(visible)` — 写入 `state.showSvTag`（`:238`）；胶囊关闭时立即隐藏（`:244-247`），否则按状态动画显示/隐藏。
- `hud.js:45 showSvTagAnimated()` — 显示并播放 `sv-enter` 入场动画（`:57-59`）。
- `hud.js:62 hideSvTagAnimated()` — 播放 `sv-exit` 出场动画，`SV_TAG_EXIT_DURATION_MS`（`hud.js:15`，220ms）后置 `hidden`（`:76-80`）。
- `hud.js:35 hideSvTagImmediately()` — 清除计时器并立即隐藏。

### 7.5 updateModeTagVisibility — 统一显隐刷新

`ManiaMapAnalyser by Leo_Black/js/app/hud.js:216 updateModeTagVisibility()`：

1. `hud.js:217-219` — 模式标签容器跟随 `showModeTagCapsule`。
2. `hud.js:225-228` — 胶囊关闭时 SV 徽章立即隐藏（**SV 标签依赖胶囊开关**）。
3. `hud.js:230-234` — 胶囊开启时按 `state.showSvTag` 决定动画显示/隐藏。

调用点：`settings.js:604`（设置变更后）与 `ManiaMapAnalyser by Leo_Black/js/app/main.js:18`（初始化）。

## 8. 相关设置（settings.json）

| 行号 | uniqueID | 类型 | 作用 |
|---|---|---|---|
| `:96` | `showModeTagCapsule` | checkbox | "Map Tag Capsule"：显示左下角标签胶囊（HB/RC/LN/Mix/SV） |
| `:277` | `VibroDetection` | checkbox | **注意大写 V**：检测 vibro 谱面并应用 fallback 处理 |
| `:285` | `useSvDetection` | checkbox | 基于 SV 的模式/类别检测，检测到显著变速时标为 SV |
| `:309` | `forceSunnyWindow` | checkbox | "Improve Sunny LN Estimation"：分析前裁剪 rice 部分（Analyze LN Parts 的前置依赖） |
| `:317` | `enableAnalyzeLN` | checkbox | "Analyze LN Parts"：分析谱面 LN/HB/Mix/RC 成分百分比 |

状态字段对照（注意 uniqueID 与 state 字段的大小写差异）：`VibroDetection`（settings.json:277）→ `state.vibroDetection`（`appContext.js:111`）；其余同小驼峰。设置默认值见 `config.js` defaults：`vibroDetection: true`（`:88`）、`useSvDetection: true`（`:90`）、`showModeTagCapsule: true`（`:93`）、`enableAnalyzeLN: false`（`:113`）。

`forceSunnyWindow` 默认值已同步：`config.js:111` 与 `settings.json:314` 均为 `true`（历史不一致已修复）。

## 9. 注意事项

1. **SV 标签依赖胶囊开关**：`showModeTagCapsule` 关闭时，`updateModeTagVisibility`（`hud.js:225-228`）与 `setSvTagVisible`（`hud.js:244-247`）都会立即隐藏 SV 徽章（见 `docs/settings.md:78` 与 `:204` 的说明）。不要期望单独控制 SV 徽章。
2. **`modeTagFromLnRatio` 无 HB**：HB 标签仅来自键型分析（`summary.js:28 resolveModeTag`）。若 pattern 分析被禁用（contentBar 为 None），HB 永远不会出现，此时只有 RC/LN/Mix 回退。
3. **SV 检测不替换模式标签**：SV 命中只置 `patternReport.Category = "SV"`（`analysis.js:803`）并显示独立徽章；`resolvedModeTag` 保持 HB/RC/LN/Mix。`MODE_TAG_OPTIONS` 中的 `"SV"` 仅为枚举完整性，当前渲染路径不使用。
4. **`enableAnalyzeLN` 前置依赖**：`forceSunnyWindow` 关闭时 `typePercentageData` 恒为 `null`（`analysis.js:521-524`），标签胶囊退回单标签模式。改动 `analysis.js:521` 分支时需同步考虑该依赖。
5. **currentModeTag 是 graph 的输入**：`hud.js:186-189` 注释明确"currentModeTag用于graph的显示"（`graph.js:217` 的 LN 难度门控）。修改标签判定时不要破坏 `state.currentModeTag` 的语义。
6. **Vibro 判定门槛**：`reworkStar > 5.0`（`analysis.js:654`）与 `jackSpeed/overall >= 0.95`（`appContext.js:157 VIBRO_JACKSPEED_RATIO_THRESHOLD`）双重条件。改阈值只改 `config.js:49 vibroJackspeedRatioThreshold`，不要改 `appContext.js:157` 的引用关系。
7. **缓存一致性**：`isVibroMap`、`typePercentageData`、`currentModeTag` 均参与结果快照（`analysis.js:751-776`），缓存命中时从快照恢复（`:438`、`:644`）。新增影响这些值的设置必须加入 `settings.js` 的 `clearResultCache()` 失效清单。

## 10. 引用索引（已验证）

| 路径:行 | 符号 |
|---|---|
| `ManiaMapAnalyser by Leo_Black/js/app/modeLogic.js:1` | `modeTagFromLnRatio` |
| `ManiaMapAnalyser by Leo_Black/js/app/modeLogic.js:31` | `resolveAutoDisplayProfile` |
| `ManiaMapAnalyser by Leo_Black/js/patterns/summary.js:28` | `resolveModeTag` |
| `ManiaMapAnalyser by Leo_Black/js/patterns/summary.js:11` | `hbRowRatio` |
| `ManiaMapAnalyser by Leo_Black/js/patterns/summary.js:66` | `svAmount = svTime(chart)` |
| `ManiaMapAnalyser by Leo_Black/js/patterns/summary.js:74` | `ModeTag: modeTag` |
| `ManiaMapAnalyser by Leo_Black/js/patterns/summary.js:75` | `SVAmount: svAmount` |
| `ManiaMapAnalyser by Leo_Black/js/patterns/primitives.js:152` | `svTime` |
| `ManiaMapAnalyser by Leo_Black/js/patterns/config.js:95` | `LN_MODE_LOW_THRESHOLD: 0.15` |
| `ManiaMapAnalyser by Leo_Black/js/patterns/config.js:96` | `LN_MODE_HIGH_THRESHOLD: 0.9` |
| `ManiaMapAnalyser by Leo_Black/js/patterns/config.js:97` | `HB_ROW_RATIO_THRESHOLD: 0.1` |
| `ManiaMapAnalyser by Leo_Black/js/patterns/config.js:102` | `SV_AMOUNT_THRESHOLD: 2000.0` |
| `ManiaMapAnalyser by Leo_Black/js/patterns/config.js:103` | `SV_SPEED_EPS: 0.05` |
| `ManiaMapAnalyser by Leo_Black/config.js:49` | `vibroJackspeedRatioThreshold: 0.95` |
| `ManiaMapAnalyser by Leo_Black/config.js:16` | `options.modeTag` |
| `ManiaMapAnalyser by Leo_Black/js/app/appContext.js:157` | `VIBRO_JACKSPEED_RATIO_THRESHOLD` |
| `ManiaMapAnalyser by Leo_Black/js/app/appContext.js:121` | `showSvTag` |
| `ManiaMapAnalyser by Leo_Black/js/app/appContext.js:124` | `currentModeTag` |
| `ManiaMapAnalyser by Leo_Black/js/app/appContext.js:60` | `modeTagSubGroupEl` |
| `ManiaMapAnalyser by Leo_Black/js/app/appContext.js:61` | `svTagEl` |
| `ManiaMapAnalyser by Leo_Black/js/app/appContext.js:153` | `MODE_TAG_OPTIONS` |
| `ManiaMapAnalyser by Leo_Black/js/app/vibro.js:16` | `detectVibro` |
| `ManiaMapAnalyser by Leo_Black/js/app/vibro.js:27` | `detectVibroFromLongjackPattern` |
| `ManiaMapAnalyser by Leo_Black/js/app/analysis.js:792` | `fallbackModeTag` |
| `ManiaMapAnalyser by Leo_Black/js/app/analysis.js:798-806` | SV 检测块 |
| `ManiaMapAnalyser by Leo_Black/js/app/analysis.js:653-657` | `isVibroMap` 判定 |
| `ManiaMapAnalyser by Leo_Black/js/app/analysis.js:824` | `setForceHideNumericDifficulty(isVibroMap)` |
| `ManiaMapAnalyser by Leo_Black/js/app/analysis.js:898-900` | `setEstimateDifficultyText("VIBRO")` |
| `ManiaMapAnalyser by Leo_Black/js/app/analysis.js:524` | `typePercentageData = sunnyWindowRework.typePercentageData` |
| `ManiaMapAnalyser by Leo_Black/js/app/analysis.js:808-814` | 标签渲染分派 |
| `ManiaMapAnalyser by Leo_Black/js/app/hud.js:143` | `setModeTag` |
| `ManiaMapAnalyser by Leo_Black/js/app/hud.js:171` | `setModeTagAdvanced` |
| `ManiaMapAnalyser by Leo_Black/js/app/hud.js:216` | `updateModeTagVisibility` |
| `ManiaMapAnalyser by Leo_Black/js/app/hud.js:237` | `setSvTagVisible` |
| `ManiaMapAnalyser by Leo_Black/js/app/hud.js:15` | `SV_TAG_EXIT_DURATION_MS` |
| `ManiaMapAnalyser by Leo_Black/js/app/hud.js:35/45/62` | `hideSvTagImmediately`/`showSvTagAnimated`/`hideSvTagAnimated` |
| `ManiaMapAnalyser by Leo_Black/js/app/settings.js:84` | `isAutoDisplayEnabled` |
| `ManiaMapAnalyser by Leo_Black/js/app/settings.js:88` | `resolveRuntimeDisplayProfile` |
| `ManiaMapAnalyser by Leo_Black/js/app/settings.js:473` | `refreshAutoDisplayProfile` |
| `ManiaMapAnalyser by Leo_Black/js/app/graph.js:217` | `state.currentModeTag === "RC"` 门控 |
| `ManiaMapAnalyser by Leo_Black/js/app/graph.js:737` | `setForceHideNumericDifficulty` |
| `ManiaMapAnalyser by Leo_Black/js/rework/sunnyWindowAlgorithm.js:335` | `shouldCalcData = state.enableAnalyzeLN` |
| `ManiaMapAnalyser by Leo_Black/js/rework/sunnyWindowAlgorithm.js:324-330` | `getTypePercentageData` 输出格式 |
| `ManiaMapAnalyser by Leo_Black/settings.json:96` | `showModeTagCapsule` |
| `ManiaMapAnalyser by Leo_Black/settings.json:277` | `VibroDetection` |
| `ManiaMapAnalyser by Leo_Black/settings.json:285` | `useSvDetection` |
| `ManiaMapAnalyser by Leo_Black/settings.json:309` | `forceSunnyWindow` |
| `ManiaMapAnalyser by Leo_Black/settings.json:317` | `enableAnalyzeLN` |
