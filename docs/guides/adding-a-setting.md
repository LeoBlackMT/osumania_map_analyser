# docs/guides/adding-a-setting.md — 新增设置项操作指南

> 面向 AI 的操作指南。文中所有 `path:line symbol` 引用均相对本仓库根目录（插件文件夹名为 `ManiaMapAnalyser by Leo_Black`，含空格，路径引用必须精确）。**照抄步骤不保证正确——每步引用的行号是编写时核实的，动手前请重新打开对应文件确认。**文中 path:line 行号为编写时快照，代码演进后可能漂移；定位源码请以符号名（symbol）为准，必要时用 grep 复核。
> 配套文档：先读 [pipeline/settings-pipeline.md](../pipeline/settings-pipeline.md)（设置管线"怎么工作"）与 [pipeline/result-cache.md](../pipeline/result-cache.md)（缓存"怎么工作"）；本文是"怎么动手"。缓存失效的**该不该加**判断见 [cache-invalidation.md](cache-invalidation.md)。

## 0. 速览：新增一个设置要动 5 个文件

```
ManiaMapAnalyser by Leo_Black/
├── settings.json        ← ① tosu 设置定义（用户看到的样子）
├── config.js            ← ② JS 默认值 + options 枚举白名单
├── js/parser/settingsParser.js  ← ③ parse{uniqueID}Value 解析器（工厂内定义 + 返回表导出）
├── js/app/appContext.js ← ④ state 字段 + parser 解构导出
└── js/app/settings.js   ← ⑤ 三处接线：applyXxxSetting 函数 + applySettingsFrom + applyIf 链/聚合
```

管线总览见 settings-pipeline.md §2（启动流程）与 §5（运行时变更流程）。**最容易漏的是第 5 步——settings.js 的三处接线，以及第 6 步缓存失效**。下面以虚构设置 `enableExample`（checkbox）为主线走完全程，每步附真实代码作为模板。

---

## 步骤 1 — settings.json 添加条目

文件：`ManiaMapAnalyser by Leo_Black/settings.json`（tosu 设置定义，顶层是数组）。

**条目格式**（字段固定为 `uniqueID` / `type` / `title` / `description` / `options` / `value`）：

- `uniqueID`：PascalCase，**命令通道键名与它完全一致**（settings-pipeline.md §3）。state 字段则是小驼峰（步骤 4）。
- `type`：tosu 支持 `header` / `checkbox` / `options` / `text` / `button` / `color`（CLAUDE.md:44 附近说明仅限这些值）。
- `title` / `description`：**必须全英文**，tosu 不支持中文（CLAUDE.md:44）；描述要简洁直白、面向普通用户，不用内部术语（CLAUDE.md:43）。
- `options`：checkbox 类为 `[]`，options 类为字符串数组。
- `value`：默认值，checkbox 为布尔、options 为字符串（字符串数组中的一项）。

**checkbox 模板**（对照真实条目 `enablePauseDetection` settings.json:269）：

```json
{
	"uniqueID": "enableExample",
	"type": "checkbox",
	"title": "Enable Example Feature",
	"description": "Recommended: Enable the example feature. What it does for the player in plain words.",
	"options": [],
	"value": false
}
```

**options 模板**（对照真实条目 `estimatorAlgorithm` settings.json:338）：

```json
{
	"uniqueID": "exampleMode",
	"type": "options",
	"title": "Example Mode",
	"description": "Select the example mode behavior.",
	"options": [
		"Off",
		"Light",
		"Full"
	],
	"value": "Light"
}
```

**摆放位置规则**（CLAUDE.md:44）：Link 部分（header `hLinks` settings.json:3）放在最前；**checkbox 类放在 options 类之前**；按现有 header 分组归类（如功能开关放进 `hFunctions` settings.json:245 分组内），新 header 用 `type: "header"`。

> ⚠️ 陷阱：tosu 不支持中文标题/描述，全英文；uniqueID 一旦定下就贯穿后续所有步骤（命令通道键名、parser 名、state 字段名都从它派生）；options 的枚举值列表必须与步骤 2 的 `APP_CONFIG.options` 一致（漏掉任一值会在 parser 白名单校验时被回退到默认值）。

---

## 步骤 2 — config.js 添加默认值

文件：`ManiaMapAnalyser by Leo_Black/config.js`。

- `APP_CONFIG.defaults`（config.js:76-115）：加键 `enableExample: false`。键名用 **state 字段名（小驼峰）**，不是 uniqueID（注意 `VibroDetection` settings.json:277 ↔ `vibroDetection` config.js:88 的差异）。
- **options 型设置**：还要加 `APP_CONFIG.options` 枚举（config.js:5-17），如 `exampleMode: ["Off", "Light", "Full"]`——`createSettingsParsers` 用它构造 `createSet` 白名单校验解析结果（settingsParser.js:234-244）。

```js
// config.js defaults 块内（:76-115）
enableExample: false,
// config.js options 块内（:5-17），options 型设置专属
exampleMode: ["Off", "Light", "Full"],
```

> ⚠️ 陷阱：**defaults 的默认值必须与 settings.json 的 `value` 同步**。历史上 `forceSunnyWindow`、`enableAlwaysShowLNDifficulty` 曾两处方向相反（见 settings-pipeline.md §7，已修复），只在 settings.json 拉取失败时暴露——新设置不要重蹈覆辙。

---

## 步骤 3 — settingsParser.js 添加 parser

文件：`ManiaMapAnalyser by Leo_Black/js/parser/settingsParser.js`。

在工厂 `createSettingsParsers(appConfig)`（settingsParser.js:233）内部添加 `parse{uniqueID}Value`，**同时在返回表（settingsParser.js:574-614）导出**。

**命名约定**：`parse{uniqueID}Value`（如 `parseEnablePauseDetectionValue` settingsParser.js:372）。settings.js 按此约定引用（settings-pipeline.md §3）。

**取原始值**：用 `extractSettingValue(settingsPayload, key)`（settingsParser.js:211）——它兼容三种 payload 形状（数组 `[{uniqueID, value}]`、对象键值、嵌套 `{settings: {...}}`），新 parser 一律经它取值，不要自己解析。

**checkbox parser 模板**（对照 `parseEnablePauseDetectionValue` settingsParser.js:372）：

```js
function parseEnableExampleValue(settingsPayload) {
    const value = extractSettingValue(settingsPayload, "enableExample");
    return normalizeBooleanSetting(value, appConfig.defaults.enableExample);
}
```

`normalizeBooleanSetting`（settingsParser.js:179）接受布尔/数字/字符串（"true"/"1" 等），无效值回退默认。**已有 normalize 辅助可复用，不要自己写**：`normalizeBooleanSetting`、`normalizeContentBarValue`（:5）、`normalizeEstimatorAlgorithmValue`（:34）等。

**options parser 模板**（对照 `parseContentBarValue` settingsParser.js:268，白名单校验 + 回退链）：

```js
function parseExampleModeValue(settingsPayload) {
    const value = extractSettingValue(settingsPayload, "exampleMode");
    const normalized = normalizeExampleModeValue(value); // 若需要自定义 normalize
    if (normalized && exampleModeSet.has(normalized.toLowerCase())) {
        return normalized;
    }
    return appConfig.defaults.exampleMode;
}
```

options 型设置需在工厂顶部建 `createSet`（settingsParser.js:234-244 同款写法：`const exampleModeSet = createSet(appConfig?.options?.exampleMode);`）。

**返回表**（settingsParser.js:574-614）末尾加：

```js
return {
    // ... 现有 39 个
    parseEnableExampleValue,
    parseExampleModeValue,
};
```

> ⚠️ 陷阱：
> - **parser 名与键名必须精确匹配**：命令通道键名 = uniqueID = PascalCase（`parseVibroDetectionValue` 内部取键 `"VibroDetection"` settingsParser.js:387，大写 V）。漏改大小写 = 设置永远读不到。
> - 返回表漏导出 = appContext.js 解构（步骤 4）与 settings.js import（步骤 5）直接报 undefined。
> - 不要为布尔设置发明新 normalize——`normalizeBooleanSetting` 已覆盖所有输入形态。
> - 数值型设置参考 `parsePauseDetectionThresholdValue` settingsParser.js:512（`Number(value)` + 有限性校验 + 非法回退默认）。

---

## 步骤 4 — appContext.js 添加 state 字段

文件：`ManiaMapAnalyser by Leo_Black/js/app/appContext.js`。

**state 区域**（appContext.js:64-151）加字段，用 `APP_CONFIG.defaults` 初始化：

```js
enableExample: APP_CONFIG.defaults.enableExample,
exampleMode: APP_CONFIG.defaults.exampleMode,
```

**parser 解构**（appContext.js:187-226）加导出——settings.js 从 appContext.js import parser，不直接 import settingsParser.js：

```js
parseEnableExampleValue,
parseExampleModeValue,
```

**显示相关设置注意双层 state**（settings-pipeline.md §4）：`state.userContentBar`（有序数组，2026-08-15 起）/ `state.userSrText` / `state.userDiffText`（appContext.js:79-81）是用户意图层（`srText` 可为 `"Auto"`），`state.contentBar`（有序数组）/ `state.srText` / `state.diffText`（appContext.js:76-78）是解析值层。规则：**写 `user*` 字段、读解析值字段**（展示代码读解析值层；`contentBar` 之上还有谱面级覆盖，正确读法是 `getActiveContentBar()` appContext.js:234 / `contentBarShows(section)` appContext.js:238）。若新设置是这类显示选项，需要同时建 `user*` 与解析值两层并仿 `applyContentBarSetting` settings.js:540 写 apply 函数（`contentBar` 的 Auto 由独立 `autoContentBar` 布尔开关控制，见 `applyAutoContentBarSetting` settings.js:555）。

> ⚠️ 陷阱：
> - state 字段名小驼峰，uniqueID PascalCase——命名时对照 `state.vibroDetection`（appContext.js:111）↔ `"VibroDetection"`。
> - 全部用 `APP_CONFIG.defaults.xxx` 初始化，禁止硬编码字面量（唯一例外 `wsEndpoint` appContext.js:150 带 `|| SOCKET_HOST` 回退）。
> - 解构列表与 createSettingsParsers 返回表顺序无关但必须一一对应，漏一个 = settings.js 运行时 TypeError。

---

## 步骤 5 — settings.js 三处接线（最容易漏）

文件：`ManiaMapAnalyser by Leo_Black/js/app/settings.js`。**启动路径**（`applySettingsFrom` settings.js:907-943）与**运行时路径**（applyIf 链 settings.js:733-767）必须同步添加，否则新设置在启动/运行时有一条路径不生效（settings-pipeline.md §9）。

### 5a. 添加 apply 函数

仿 `applyPauseDetectionSetting` settings.js:544 的统一模式：`parse→compare(与 state 现值比较)→mutate state→side effects→return changed`（settings-pipeline.md §9）：

```js
export function applyEnableExampleSetting(value) {
    const next = normalizeBooleanSetting(value, APP_CONFIG.defaults.enableExample);
    const changed = state.enableExample !== next;
    state.enableExample = next;
    // 副作用：视觉刷新 / 缓存清理等（没有则省略）
    return changed;
}
```

**import 也要加**：settings.js:1-52 的 import 块加入 `parseEnableExampleValue`（从 `./appContext.js`）。options 型若用了自定义 normalize，还需在 settings.js:53-64 的 `../parser/settingsParser.js` import 中补充。

### 5b. 加入 applySettingsFrom（启动基线链）

`applySettingsFrom`（settings.js:907-943）加一行：`applyEnableExampleSetting(parseEnableExampleValue(source));`。它由两条路径调用：settings.json 文件基线（settings.js:946-947）与无文件时的 defaults 回退对象（settings.js:950-986，该对象手工拼装 35 键，全部来自 `APP_CONFIG.defaults`）。**回退对象建议同步加键**（如 `enableExample: APP_CONFIG.defaults.enableExample`）——虽然 parser 缺键时会回退 defaults（少了也能跑），但保持 35 键全量模式与现有代码一致。

### 5c. 加入 applyIf 链 + 聚合

运行时监听 `setupSettingsCommandListener`（settings.js:707）内：

- **applyIf 链**（settings.js:733-767）加一行，链上每个 `xxxChanged` 都走 **hasKey 守卫**（`applyIf` settings.js:729-730 = `hasKey(key) ? applyFn(parseResult) : false`；`hasKey` 定义于 settings.js:723-728，数组按 `uniqueID` 查找、对象用 `hasOwnProperty`）。**hasKey 守卫的作用**：tosu 命令可能不发全部设置（用户没碰过的分组），直接 apply 会让 parser 内部回退的 config defaults 覆盖 settings.json 基线——守卫让未发送的键保持基线不动（settings-pipeline.md §3）。loadSettings 的文件基线阶段不走此守卫。

```js
const enableExampleChanged = applyIf("enableExample", applyEnableExampleSetting, parseEnableExampleValue(payload));
```

- **聚合**：`changed`（settings.js:776-810）与 `recomputeNeeded`（settings.js:812-831）两个 OR 链都加 `|| enableExampleChanged`。区别：纯显示类设置只在 `changed`（立即应用视觉变更，不重算），计算/显示相关设置进 `recomputeNeeded`（触发 `scheduleRecompute` settings.js:858-859）。吃不准就两边都加（代价只是多余重算，不会错）。

> ⚠️ 陷阱（本步密集区）：
> - **两处必须同步**：applyIf 链（settings.js:733-767）与 applySettingsFrom（settings.js:907-943）漏一处 = 启动或运行时一条路径失效。
> - 键名字符串必须是 uniqueID 原样（`"VibroDetection"` settings.js:746 大写 V 的前车之鉴），parser 名 `parse{uniqueID}Value`、函数名 `apply{uniqueID}Setting` 三处命名要一致。
> - changed 标志命名约定 `xxxChanged`，聚合时漏加 = 设置生效了但 UI 不刷新/不重算。
> - apply 函数必须返回 changed 布尔，聚合依赖它；函数内 compare 用 `state.xxx !== next` 而非 `!== value`（value 可能未归一化）。

---

## 步骤 6 — 若计算影响设置：加入缓存失效列表

**先判断该不该加**：判断标准见 [cache-invalidation.md](cache-invalidation.md)（决策指南）。粗判：设置影响**估算结果/解析/分析产物**（算法选择、倍速、OD、键型分析、SV 检测、etterna 版本等）→ 计算影响；只影响**外观/显示**（颜色、透明度、文案、布局）→ 纯显示。显示类**不得**加入失效列表——覆盖检查（result-cache.md §6）已处理，加入只会白白丢命中。

**若是计算影响设置**：在失效列表（settings.js:833-850）加条件。缓存键不含任何设置（`analysis.js:305` 的 key 只有 estimator|identity|modSignature，见 result-cache.md §5），漏加会**静默提供陈旧结果**：

```js
if (estimatorChanged
    || /* ... 现有 14 个 ... */
    || enableExampleChanged) {
    clearResultCache();
}
```

> ⚠️ 陷阱：
> - 失效列表的条件变量必须用聚合时定义的 `xxxChanged` 标志（settings.js:835-848），与 recomputeNeeded 是两套判断——`wsEndpointChanged` 只在 `changed` 里（不在 recomputeNeeded），所以在失效列表被显式列出（settings.js:834 注释）。
> - 不要与"关闭缓存时清一次"混淆：`applyEnableResultCacheSetting` settings.js:666 内 settings.js:672-674 是停用前清理残留，不是运行期失效（result-cache.md §9）。
> - 新计算影响设置若加入了失效列表，记得同时把它的 changed 标志加进 recomputeNeeded（步骤 5c），否则重算不会被触发，只有缓存被清空。

---

## 步骤 7 — 更新文档

- **docs/settings.md**（人类设置说明，中英双语，直白语言，CLAUDE.md:43）：新增该设置的说明条目，保持与现有条目的编号/格式一致。
- **对应功能/管线文档**：若设置接通了功能逻辑（如分析、显示、估计算法），更新对应技术文档（docs/README.md:14-16 的要求）。
- **docs/README.md 索引**：新增文档（不是新设置）才需要加条目；类别子目录 README 同理。本文档的索引条目已存在于 `docs/guides/README.md:18`（新增指南时需自行添加）。
- 若新设置改变了管线语义（如缓存键、失效语义、双层 state 规则），同步修改 [pipeline/settings-pipeline.md](../pipeline/settings-pipeline.md) 与 [pipeline/result-cache.md](../pipeline/result-cache.md)。

> ⚠️ 陷阱：CLAUDE.md:41 要求"新增/修改功能时务必修改对应文档，确保内容与实际功能一致"；只改代码不改文档 = 文档漂移，后续 LLM 会按旧文档工作。

---

## 完成检查清单

动手前、后各过一遍（路径含空格，PowerShell 用 `-LiteralPath`）：

```powershell
# ① settings.json 条目存在
Select-String -LiteralPath "ManiaMapAnalyser by Leo_Black\settings.json" -Pattern '"enableExample"'
# ② config.js defaults 键存在（options 型再查 options 枚举）
Select-String -LiteralPath "ManiaMapAnalyser by Leo_Black\config.js" -Pattern 'enableExample'
# ③ parser 定义 + 返回表导出
Select-String -LiteralPath "ManiaMapAnalyser by Leo_Black\js\parser\settingsParser.js" -Pattern 'parseEnableExampleValue'
# ④ state 字段 + 解构导出
Select-String -LiteralPath "ManiaMapAnalyser by Leo_Black\js\app\appContext.js" -Pattern 'enableExample'
# ⑤ settings.js：import / apply 函数 / applySettingsFrom / applyIf / 聚合（changed+recomputeNeeded）/ 失效列表（若适用）
Select-String -LiteralPath "ManiaMapAnalyser by Leo_Black\js\app\settings.js" -Pattern 'enableExample'
```

**核对点**：

- [ ] settings.json 条目：英文、直白描述、checkbox 在 options 前、Link 在最前、uniqueID PascalCase
- [ ] config.js：defaults 键与 settings.json `value` 一致；options 型有 `APP_CONFIG.options` 枚举且与 settings.json `options` 一致
- [ ] settingsParser.js：`parse{uniqueID}Value` 定义 + 返回表导出，键名精确匹配 uniqueID
- [ ] appContext.js：state 字段（`APP_CONFIG.defaults` 初始化）+ parser 解构导出
- [ ] settings.js：import + `apply{uniqueID}Setting` + `applySettingsFrom` + applyIf 链 + changed/recomputeNeeded 聚合（**三处接线全部到位**）
- [ ] 计算影响设置：失效列表（settings.js:833-850）已加条件；纯显示设置未加
- [ ] 文档：docs/settings.md、功能/管线文档、docs/README.md 与类别 README 已同步
- [ ] 用 `Test-Path` 确认新设置名在 5 个文件中都出现，行号与本文引用一致（引用行号以实际读取为准）
