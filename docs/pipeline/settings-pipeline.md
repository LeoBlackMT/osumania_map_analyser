# docs/pipeline/settings-pipeline.md — 设置管线

> 面向 AI 的管线技术文档。文中所有 `path:line symbol` 引用均相对本仓库根目录（插件文件夹名为 `ManiaMapAnalyser by Leo_Black`，含空格，路径引用必须精确）。
> 相关文档：[result-cache.md](result-cache.md)（缓存写门/失效）、[analysis-pipeline.md](analysis-pipeline.md)（分析管线总览）、[guides/adding-a-setting.md](../guides/adding-a-setting.md)（新增设置 7 步清单，依赖本文流程）。

## 1. 设置来源三处

插件设置不是单一来源，最终生效值由三处叠加：

| 来源 | 性质 | 位置 | 说明 |
| --- | --- | --- | --- |
| `ManiaMapAnalyser by Leo_Black/settings.json` | tosu 设置定义（基线） | 全文 45 个 uniqueID | 暴露给 tosu 设置界面的定义文件，含默认值。**这不是设置文件本身** |
| `ManiaMapAnalyser by Leo_Black/config.js` `APP_CONFIG.defaults` | JS 内部默认值 | config.js:76-115 | 解析器无值可读时的回退；`APP_CONFIG.options`（config.js:5-17）提供枚举白名单 |
| tosu 运行时 `getSettings` 命令 | 用户实际设置 | WebSocket 命令通道 | 实际设置文件位于 tosu 的 `settings` 目录（文件名 `<插件目录名>.json`），通过 `getSettings` 命令推送（见 CLAUDE.md:34、:52） |

**settings.json 的 45 个 uniqueID 构成**：6 header + 4 button + 35 实际设置。

- **Links（header `hLinks` settings.json:3）**：button `GuideButtonEN` settings.json:11、`GuideButtonCN` settings.json:19、`IssueButton` settings.json:27、`BenchmarkButton` settings.json:35
- **Modules Customization（header `hModules` settings.json:43）**：`contentBar` settings.json:51、`srText` settings.json:66、`diffText` settings.json:80、`showModeTagCapsule` settings.json:96
- **Theme & Effects（header `hTheme` settings.json:104）**：`enableOsuTheme` settings.json:112、`useOsuFont` settings.json:120、`enableFloatingTriangles` settings.json:128、`enableCoverArt` settings.json:136、`customBackgroundColor` settings.json:144、`enableEtternaRainbowBars` settings.json:152、`enableStatusMarquee` settings.json:160、`enableNumericDifficulty` settings.json:168、`enableLNDifficulty` settings.json:176、`reverseCardExtendDirection` settings.json:184、`cardVisibility` settings.json:192、`cardOpacity` settings.json:204、`cardBgBlur` settings.json:218、`cardRadius` settings.json:233
- **Functionality Options（header `hFunctions` settings.json:245）**：`enableUpdateCheck` settings.json:253、`enableResultCache` settings.json:261、`enablePauseDetection` settings.json:269、`VibroDetection` settings.json:277（注意大写 V，见 §9）、`useSvDetection` settings.json:285、`display6kLevel` settings.json:293、`extendedEstimationRange` settings.json:301、`forceSunnyWindow` settings.json:309、`enableAnalyzeLN` settings.json:317、`pauseDetectionThreshold` settings.json:325、`estimatorAlgorithm` settings.json:338、`etternaVersion` settings.json:353、`companellaEtternaVersion` settings.json:367
- **Network Configuration（header `hNetwork` settings.json:381）**：`wsEndpoint` settings.json:389
- **Debug Options（header `hDebug` settings.json:397）**：`debugUseAmount` settings.json:405、`azusaSunnyReferenceHo` settings.json:413、`enableAlwaysShowLNDifficulty` settings.json:420

config.js 的 `APP_CONFIG.defaults`（config.js:76-115）与 settings.json 字段一一对应，但存在已知不匹配（见 §7）。`APP_CONFIG.options`（config.js:5-17）是各 options 型设置的枚举白名单，`createSettingsParsers` 用它构造 `createSet` 校验解析结果（settingsParser.js:234-244）。

## 2. 启动流程

入口 `settings.js:891 loadSettings()`（async），时序如下：

1. **fetch settings.json 作基线**：`settings.js:896` `fetch("./settings.json", { cache: "no-store" })`，成功则 `fileSettings = await response.json()`，失败（无文件/网络错误）则 `fileSettings = null`（settings.js:894-905）。
2. **applySettingsFrom(fileSettings)**：settings.js:946-947。`applySettingsFrom`（settings.js:907-943）对 35 个设置逐个 `applyXxxSetting(parseXxxValue(source))`。此阶段**无 hasKey 守卫**——settings.json 文件内容本身即完整定义，字段缺失时 parser 落回 config defaults。
3. **无文件时 config defaults 回退**：settings.js:950-986。构造一个手工拼装的 source 对象（35 个键全部来自 `APP_CONFIG.defaults`），同样走 `applySettingsFrom`。注意此处字段名用的是**命令通道键名**（如 `VibroDetection` settings.js:964、`enablePauseDetection` settings.js:960），与 uniqueID 一致。
4. **注册运行时监听**：settings.js:992 `setupSettingsCommandListener()`。注释明确：监听必须在文件基线之后注册，否则命令回调可能用 config defaults 覆盖文件值（settings.js:989-991）。
5. **请求 tosu 实际设置**：settings.js:865-868 `socket.sendCommand("getSettings", getCounterPathForCommand())`。`getCounterPathForCommand`（settings.js:318-325）优先用 `window.COUNTER_PATH`，回退到当前页面 pathname+search。

命令响应路径上的等待机制 `waitForInitialSettingsFromCommand`（settings.js:871-889）——若 `state.settingsReceivedFromCommand` 已为 true 直接 resolve，否则挂起并设置 `state.initialSettingsResolver`（settings.js:884-887），由命令回调在收到首个 payload 后触发（settings.js:852-856）；超时（默认 1500ms，见 §9）则 reject。

## 3. 解析约定

- **`parse{uniqueID}Value(payload)` 命名约定**：每个设置一个 parser，由 `settingsParser.js:233 createSettingsParsers(appConfig)` 工厂批量创建并返回。当前共返回 **39 个 parser**（settingsParser.js:574-614 返回表）：35 个对应实际设置 + 4 个遗留 parser（`parseAutoModeValue` settingsParser.js:307、`parseUseDanielAlgorithmValue` settingsParser.js:312、`parseDisableVibroDetectionValue` settingsParser.js:382、`parseEnablePatternValue` settingsParser.js:246，均无对应 uniqueID，见 §6）。
- **payload 形状自适应**：`settingsParser.js:211 extractSettingValue` 兼容三种形状——数组（`[{uniqueID, value}]`，settings.json 文件格式与 tosu 命令格式）、对象键值（`{key: value}`）、嵌套对象（`{settings: {...}}`）。所有 parser 内部都通过它取值。
- **回退链**：取到值 → normalize → 校验是否在 `createSet` 白名单内（options 型，如 `parseContentBarValue` settingsParser.js:268-277）→ 不通过/缺失则落回 `appConfig.defaults`。
- **hasKey 守卫（命令通道专用）**：`settings.js:723-728 hasKey` 检查键是否真实存在于命令 payload（数组按 `uniqueID` 查找，对象用 `hasOwnProperty`）。`applyIf`（settings.js:729-730）= `hasKey(key) ? applyFn(parseResult) : false`。**作用**：tosu 命令可能不发全部设置（例如用户从未碰过的分组），若直接 apply，parser 内部落回的 config defaults 会覆盖 settings.json 基线——守卫让未发送的键保持文件基线不动。loadSettings 的文件基线阶段不走此守卫（§2 第 2 步）。

## 4. 双层 state（重点）

`appContext.js:64-151 state` 是全部设置的运行时持有者。其中显示相关设置存在**用户意图层与解析值层**的分层：

| 层 | 字段 | 语义 |
| --- | --- | --- |
| 用户意图层 | `state.userContentBar` / `state.userSrText` / `state.userDiffText`（appContext.js:79-81） | 用户实际选择，`contentBar`/`srText` 可为 `"Auto"` |
| 解析值层 | `state.contentBar` / `state.srText` / `state.diffText`（appContext.js:76-78、:87） | 运行时实际使用的具体值，永不为 `"Auto"` |
| 谱面级覆盖 | `state.effectiveContentBar`（appContext.js:77） | 按谱面覆盖 contentBar，由 `settings.js:422 setEffectiveContentBarForMap(contentBarOrNull)` 写入（null 表示无覆盖） |

**规则：写 `user*` 字段、读解析值字段。** 例如 `applyContentBarSetting`（settings.js:478-490）：写 `state.userContentBar`，若为 `"Auto"` 则 `refreshAutoDisplayProfile()` 解析出具体值，否则 `setRuntimeContentBar(state.userContentBar)` 写入解析值层。

- **Auto 解析**：`settings.js:84 isAutoDisplayEnabled()`（`userSrText === "Auto" || userContentBar === "Auto"`）→ `settings.js:88 resolveRuntimeDisplayProfile(modeTag)` 调 `resolveAutoDisplayProfile`（modeLogic.js）得出 `{contentBar, srText}` 映射，再经 `setRuntimeDisplayProfile`（settings.js:466-471）→ `setRuntimeContentBar`（settings.js:393）/`setRuntimeSrText`（settings.js:447）/`setRuntimeDiffText`（settings.js:458）写入解析值层。`diffText` 无 Auto 选项，直接透传。
- **读取约定**：展示代码一律读解析值层；`state.contentBar` 之上还有谱面级覆盖，正确读法是 `appContext.js:228 getActiveContentBar()`（`state.effectiveContentBar || state.contentBar`）与 `appContext.js:232 contentBarShows(section)`（active === section 或 Full）。

**估计算法层**：`state.estimatorAlgorithm`（appContext.js:88）是用户选择，`state.actualEstimatorAlgorithm`（appContext.js:89）记录实际执行的算法（如 Azusa 因 LN 比例过高回退 Sunny）。分析完成后读取后者；缓存命中时从快照恢复，不得重算（详见 [result-cache.md](result-cache.md)）。

## 5. 运行时变更流程（用户改设置）

用户修改 → tosu 推送 `getSettings` 命令 → 回调链：

1. **监听注册**：`settings.js:707 setupSettingsCommandListener()`（幂等，`state.settingsCommandSubscribed` 守卫 settings.js:708-710）→ `socket.commands((packet) => {...})` settings.js:714。
2. **解包**：`settings.js:695 extractSettingsPayloadFromCommandPacket(packet)`——数组直接返回；`{command: "getSettings", message}` 取 `message`；其余返回 null 丢弃。
3. **逐个应用**：`settings.js:733-767` 共 **35 个 applyIf**（与 35 个实际设置一一对应），每个产生一个 `xxxChanged` 布尔。全部走 hasKey 守卫 + `applyXxxSetting(parseXxxValue(payload))`。
4. **遗留 autoMode 检查**：settings.js:769-774，见 §6。
5. **聚合**：`changed`（settings.js:776-810，35 个标志 OR）与 `recomputeNeeded`（settings.js:812-831，20 个计算/显示相关标志 OR）。区别：纯显示类（如 `customColorChanged`、`cardOpacityChanged`）只在 `changed` 中，不触发重算。
6. **缓存失效**：settings.js:833-850——`clearResultCache()` 仅当计算相关标志变化时触发（estimator/azusaSunnyReferenceHo/etternaVersion/companellaEtternaVersion/debug/sv/vibro/wsEndpoint/forceSunnyWindow/enableLNDifficulty/enableAnalyzeLN/enableAlwaysShowLNDifficulty/display6kLevel/extendedEstimationRange）。`wsEndpointChanged` 只在 `changed` 不在 `recomputeNeeded`，故在此显式列出（settings.js:834 注释）。完整写门与失效语义见 [result-cache.md](result-cache.md)。
7. **首包解析**：settings.js:852-856 消费 `state.initialSettingsResolver`（§2 第 5 步挂起的等待者）。
8. **重算调度**：settings.js:858-859 `recomputeNeeded` 时 `scheduleRecompute("settings changed", true)`（scheduler.js，防抖）；仅 `changed` 时立即应用视觉变更（如数字难度开关），不重算。

## 6. 遗留逻辑（注意事项）

- **autoMode 强制 Auto**：settings.js:769-774——`parseAutoModeValue(payload)` 为真且 `isAutoDisplayEnabled()` 为假（即用户当前不是 Auto）时，**直接写** `state.userSrText = "Auto"`、`state.userContentBar = "Auto"` 并 `refreshAutoDisplayProfile()`。这是对所有设置的独立检查之后运行的全局覆盖，新设置项迁移自旧 `autoMode` 配置时需留意。
- **4 个遗留 parser（无对应 uniqueID）**：
  - `parseAutoModeValue` settingsParser.js:307（键 `autoMode`，回退 `defaults.autoMode`=false）
  - `parseUseDanielAlgorithmValue` settingsParser.js:312（键 `useDanielAlgorithm`——实际上它转调 `parseEstimatorAlgorithmValue` 判断结果是否为 "Daniel"，不再读独立键）
  - `parseDisableVibroDetectionValue` settingsParser.js:382（= `!parseVibroDetectionValue()`）
  - `parseEnablePatternValue` settingsParser.js:246（键 `enablePatternAnalysis`，供 `parseContentBarValue` 回退使用）
- **普通 parser 内部的遗留键回退**（迁移路径，非独立 parser）：
  - `parseContentBarValue` settingsParser.js:275-276：无 `contentBar` 键时读 `enablePatternAnalysis`，为真 → `"Pattern"`，否则 → `"None"`（`parseEnablePatternValue` 的回退）。
  - `parseDiffTextValue` settingsParser.js:300-304：遗留键 `enableEstDiff` → 真 `"Difficulty"` / 假 `"None"`。
  - `parseEstimatorAlgorithmValue` settingsParser.js:324-327：遗留键 `useDanielAlgorithm` → 真 `"Daniel"` / 假 `"Sunny"`。
  - `parseVibroDetectionValue` settingsParser.js:392-395：遗留键 `disableVibroDetection` → 取反。
  - `parseEnableUpdateCheckValue` settingsParser.js:498-499：遗留键 `showTitleIcon`。
  - `parseWsEndpointValue` settingsParser.js:540-549：遗留键 `wsHost`。

## 7. settings↔config 不匹配（已知注意事项，不修复）

config.js `defaults` 与 settings.json 存在两处**方向相反**的默认值不匹配。正常运行时无感（启动基线来自 settings.json 或命令通道），差异仅在**首次启动且 settings.json 拉取失败**时暴露——此时走 config defaults（settings.js:950-986）：

| 设置 | settings.json | config.js | 无文件基线时的行为差异 |
| --- | --- | --- | --- |
| `forceSunnyWindow` | settings.json:314 `true` | config.js:111 `false` | config 基线会关闭 Sunny LN 优化（移除 rice 部分后再分析），LN 估计可能偏高 |
| `enableAlwaysShowLNDifficulty` | settings.json:425 `false` | config.js:114 `true` | config 基线会强制显示 LN 难度，即使 LN% 过低或 LN 过简 |

修改任何一处默认值时，必须同步另一处，否则上述差异会被重新引入。

## 8. applyEnableResultCacheSetting

`settings.js:666 applyEnableResultCacheSetting(value)`：
1. `normalizeBooleanSetting(value, APP_CONFIG.defaults.enableResultCache)` 归一化；
2. 比较 `state.enableResultCache` 得 `changed`，记录旧值 `wasEnabled`；
3. mutate `state.enableResultCache`；
4. **副作用**：`changed && wasEnabled && !next`（即从开启 → 关闭）时调用 `clearResultCache()`（settings.js:672-674）——关闭缓存时立即清空已缓存快照，防止残留；
5. 返回 `changed`。

## 9. 注意事项

- **applyXxxSetting 统一模式**：`parse→compare(与 state 现值比较)→mutate state→side effects(视觉刷新/缓存清理等)→return changed`。所有 apply 函数返回 changed 布尔，供 §5 第 5 步聚合。新增设置必须遵循此模式（见 [guides/adding-a-setting.md](../guides/adding-a-setting.md)）。
- **VibroDetection 大小写**：settings.json 的 uniqueID 是 `VibroDetection`（大写 V，settings.json:277），命令通道键名与之相同（settings.js:746、settings.js:964 回退对象）；state 字段是小写 `state.vibroDetection`（appContext.js:111）。按 uniqueID 查找/新增时必须精确匹配 PascalCase。
- **settingsCommandTimeoutMs = 1500**：定义于 `config.js:71 APP_CONFIG.timing.settingsCommandTimeoutMs`，经 `appContext.js:176 SETTINGS_COMMAND_TIMEOUT_MS` 导出，由 `waitForInitialSettingsFromCommand`（settings.js:871-889）用作超时（超时 reject "getSettings timeout"）。
- **命令通道 ≠ 文件基线**：命令 payload 可能缺键（hasKey 守卫兜底），文件基线不缺键（settings.json 结构完整）。给 settings.json 加新设置时，`applySettingsFrom`（settings.js:907-943）与 applyIf 链（settings.js:733-767）两处都必须同步添加，否则新设置在启动/运行时有一条路径不生效。
- **新增计算相关设置必须加入缓存失效**：§5 第 6 步的失效列表（settings.js:833-850）之外的新设置不会自动失效缓存——缓存键不含该设置（见 [result-cache.md](result-cache.md)），漏加会静默提供过期结果。纯显示设置则**不得**加入失效列表（覆盖检查会处理）。
