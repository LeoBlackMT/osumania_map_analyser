# docs/pipeline/mod-handling.md — mod 处理管线

> 面向 AI 的管线技术文档。文中所有 `path:line symbol` 引用均相对本仓库根目录（插件文件夹名为 `ManiaMapAnalyser by Leo_Black`，含空格，路径引用必须精确）。文中 path:line 行号为编写时快照，代码演进后可能漂移；定位源码请以符号名（symbol）为准，必要时用 grep 复核。
> 相关文档：[result-cache.md](result-cache.md)（缓存键与失效）、[analysis-pipeline.md](analysis-pipeline.md)（分析管线总览）、[../features/difficulty-estimation.md](../features/difficulty-estimation.md)（Etterna WASM 入口与各估算器）。

## 1. mod 数据来源

mod 状态随 tosu **api_v2** WebSocket 包逐帧送达，入口 `js/app/socketHandlers.js:145 setupSocketListener()` → `socket.api_v2`（socketHandlers.js:146）。每个包到达时先做 mod 提取：

`js/app/modData.js:62 getModData(data, { sortedKnownModCodes, modBitFlagEntries, fallbackClient, preferPlayMods })` 是唯一的数据提取点，浏览器专属（js/app/ 下，无 DOM 依赖但被 socketHandlers 直接调用）。提取流程：

1. **client 判定**：`data.client` 小写化（modData.js:68-69），缺省回退 `fallbackClient`（socketHandlers.js:38 传入 `state.client`）。**lazer 与 stable 的解析分支不同**（§4/§5）。
2. **候选收集**（按优先级排列，全部尝试）：
   - 游玩候选 `collectPlayModsCandidates`（modData.js:11）：`data.play.mods` 优先，其次 `data.tourney.clients[].play.mods`、`data.tourney.ipcClients[].gameplay.mods`。
   - 非游玩候选 `collectNonPlayModsCandidates`（modData.js:24）：`data.menu.mods`、`data.resultsScreen.mods`。
   - `preferPlayMods`（socketHandlers.js:39 传 `state.isInPlayState`）为真时只取游玩候选，否则两者合并（modData.js:73-75）——游玩中只信 play 字段，避免菜单残留 mod 干扰。
3. **归一化提取 mod 代码**：候选 payload 可能是**对象**（`{name, str, acronym, number, num, array}`）、**字符串**或**数字**：
   - 字符串 → `addCodesFromString`（modData.js:28）：大写化、去非字母后按 `sortedKnownModCodes` **最长匹配优先**逐段扫描。
   - 数字 → `addCodesFromNumber`（modData.js:50）：按 `modBitFlagEntries` 逐位与运算（stable 的 osu bitflag）。
   - 数组（`mods.array` 或裸数组）→ 逐个元素走字符串/`acronym` 提取（modData.js:118-147）。
4. **mod 状态信号量**（modData.js:78、:206-214）：
   - `hasModPayload`：至少一个候选字段非 undefined/null。
   - `hasModInfo`（= `hasRelevantModInfo`）：提取到任何计算相关信息（mod 代码、speed_change、DA OD、odFlag/cvtFlag、非 1.0 速率）。
   - `hasExplicitNoMod`：显式无 mod 信号（`NM`/`NOMOD`/`NONE`、数字 0、空数组）且无相关 mod 信息。

## 2. 已知 mod 代码表

白名单定义于 `js/../config.js:117-126`：

| 代码 | 含义（插件视角） | 参与计算 | 说明 |
| --- | --- | --- | --- |
| `DA` | lazer Difficulty Adjust | 是（speed_change / overall_difficulty，仅 lazer） | 通过 mod settings 携带自定义倍速与 OD（modData.js:167-177） |
| `NC` | Nightcore | 是（速率 1.5×） | 与 DT 等效加速 |
| `DT` | Double Time | 是（速率 1.5×） | 对应 stable bitflag 64 |
| `HT` | Half Time | 是（速率 0.75×） | 对应 stable bitflag 256 |
| `HR` | Hard Rock | 是（odFlag="HR"） | 对应 stable bitflag 16 |
| `EZ` | Easy | 是（odFlag="EZ"） | 对应 stable bitflag 2 |
| `DC` | Daycore | 是（速率 0.75×，仅 lazer 出现） | 与 HT 同样减速处理 |
| `IN` | Inverse | 是（cvtFlag="IN"，仅 lazer） | LN 反转转换（modIN） |
| `HO` | Hidden 类（lazer mania） | 是（cvtFlag="HO"，仅 lazer） | LN→RC 转换（modHO） |
| `MR` | Mirror | 否 | 被识别但**无任何计算分支使用**，仅影响 `hasModInfo` |
| `CL` | Classic（仅 lazer） | 是（classic 判定） | 参与 §2.1 classic 判定：无 SV2 且 lazer 带 CL → classic=true |
| `SV2` | ScoreV2（计分方式） | 是（classic 判定） | 对应 stable bitflag 536870912（1<<29）；**只要存在 SV2（无论 client）→ classic=false，优先级最高** |

- `APP_CONFIG.mods.knownCodes`（config.js:118）共 **12** 个代码；`APP_CONFIG.mods.bitFlags`（config.js:119-125）覆盖 stable 位标志对应项：`EZ:2, HR:16, DT:64, HT:256, NC:512, SV2:536870912`（EZ/HR/DT/HT/NC/SV2 与 osu bitflag 一一对应；DA/DC/IN/HO/MR/CL 无 bitflag，是 lazer 专属代码，走字符串路径。`CL` 在字符串/acronym 路径被收集，`SV2` 在 stable 走 bitflag 路径，lazer 下也可能以 acronym 出现）。
- **派生导出**（`js/app/appContext.js:182-185`）：
  - `SORTED_KNOWN_MOD_CODES`（appContext.js:184）= knownCodes 按**代码长度降序**排序——`addCodesFromString` 用它实现**最长匹配优先**，避免 "DT" 提前截断 "DTX" 之类粘连串。`SV2` 含数字，normalization 保留数字（modData.js:32 注释）。
  - `MOD_BIT_FLAG_ENTRIES`（appContext.js:185）= `Object.entries(bitFlags)`，供 `addCodesFromNumber` 遍历。

### 2.1 classic 判定（Classic 感知星数）

`getModData` 返回对象新增 `classic` 布尔与 `modCodes` 数组（modData.js:237-238）：

```js
const classic = !modCodes.has("SV2") && (client !== "lazer" || modCodes.has("CL"));
```

| client | mods | classic |
| --- | --- | --- |
| lazer | 带 CL、无 SV2 | true |
| lazer | 带 CL **+ SV2**（stable 导入） | false |
| lazer | 无 CL、无 SV2 | false |
| stable | 带 SV2 | false |
| stable | 无 SV2 | true |
| unknown/空 | 无 SV2 | true（按非 lazer 处理） |

语义：**计分方式优先**——只要带 SV2（ScoreV2 计分）即非 Classic，无论 client/CL；反之（ScoreV1/Classic 计分）无 SV2 时，lazer 需 CL 才为 Classic，stable/unknown 恒为 Classic。stable 导入 lazer 的成绩若用 ScoreV2 计分会同时带 CL+SV2 → 判非 Classic。classic 进 modSignature 第 4 段（§3），并透传至估算器 options（`classicMod`）切换 Sunny 星数密度（C_arr vs C_arrV2，见 features/rework-pp.md §3）。**只影响星数密度，不影响准确率（v2Acc）**。`modCodes` 是排序后的数组（JSON-safe），供 ReworkPP 的 NF/EZ mod 修正使用。

## 3. modSignature 构成与何时应用

**modSignature 格式 = `speedRate|odFlag|cvtFlag|classic`（四段）**，在 `getModData` 内构建（modData.js:224-228）：

```js
const modSignature = [
    Number(speedRate).toFixed(5),   // 速率保留 5 位小数，防浮点抖动
    odFlag == null ? "none" : String(odFlag),
    cvtFlag == null ? "none" : String(cvtFlag),
    classic ? "1" : "0",            // Classic 感知星数标志（第 4 段，§2.1）
].join("|");
```

- 只含**计算相关维度**（modData.js:216-217 注释明确）：lazer mod payload 中与计算无关的字段（如隐藏外观类设置）波动不会改变签名，避免无谓重算。
- 空值统一序列化为 `"none"`，如 `1.00000|none|none|0` 表示无 mod 且非 Classic。
- `classic` 段（0/1）反映当前 Classic 语义——stable 开/关 SV2、lazer 开/关 CL 都会改变该段，从而改变缓存键（§7），使星数/难度/PP 全部重算为当前 Classic 语义。

**应用时机**（socketHandlers.js:255-272）：api_v2 包可能是**部分包**（不一定带 mod 字段），因此不能每个包都覆盖：

```js
// socketHandlers.js:257-258
const shouldApplyModState = !previousModSignature
    || (modData.hasModPayload && (modData.hasModInfo || modData.hasExplicitNoMod));
const nextModSignature = shouldApplyModState ? modData.modSignature : previousModSignature;
```

- 条件：**首次收到 mod 状态**（`state.modSignature` 为空），或**本包确实携带显式 mod 载荷**（hasModPayload 且有信息或显式无 mod）。
- 应用时写入 `state.speedRate / state.odFlag / state.cvtFlag / state.modSignature`（socketHandlers.js:267-272；state 初始值见 `appContext.js:72-75`：`speedRate: 1.0`、`odFlag: null`、`cvtFlag: null`、`modSignature: ""`），并同步应用 `state.modCodes`/`state.classicMod`（socketHandlers.js:172-173，在 beatmap 守卫早退之前）。
- 签名变化才触发重算：`nextModSignature !== previousModSignature` 构成 `hasStateMismatch`（socketHandlers.js:263-265）→ `scheduleRecompute("beatmap/mod changed", true)`（socketHandlers.js:305）。

## 4. 速率处理（speedRate → musicRate）

`getModData` 内速率判定优先级（modData.js:182-188）：

1. **lazer 自定义倍速**：mod 数组中出现带 `settings.speed_change`（>0 且有限）的 mod（通常是 `DA`）→ `speedRate = speed_change`（modData.js:167-170、:182-183）。lazer 的 `mods.rate` 即由此承载，**优先级高于任何固定倍速 mod**。
2. **NC/DT** → `speedRate = 1.5`（modData.js:184-185）。
3. **HT/DC** → `speedRate = 0.75`（modData.js:186-187）。
4. 否则保持 `1.0`（modData.js:149 初始化）。

**消费方**（speedRate 一旦进 state 即全链路生效）：

| 消费方 | 位置 | 用法 |
| --- | --- | --- |
| Etterna WASM | `js/app/analysis.js:152-159 buildEtternaAnalyzeOptions` | `musicRate: state.speedRate`（analysis.js:154） |
| 估算器（Worker/主线程） | analysis.js:441-447 `estimatorOptions` | `speedRate: state.speedRate`（analysis.js:442），经 `runInWorker`/`runXxxEstimatorFromText` 透传 |
| Interlude | analysis.js:590 `calculateInterludeStar(rawText, state.speedRate, state.cvtFlag)` → `js/interlude/index.js:14 calculateInterludeStar(source, rate, cvtFlag)` | 第二参数 rate |
| 歌曲时间换算 | socketHandlers.js:54-60 | `liveTimeMs / speedRate` 得到谱面时间轴（beatmap time 是原速时间，需除速率还原） |

## 5. odFlag / cvtFlag 语义

### odFlag（OD 变化，modData.js:190-196）

取值域：`null`（无变化）| `"HR"` | `"EZ"` | **数字**（lazer DA 的自定义 OD）。

- lazer：DA mod 携带 `settings.overall_difficulty`（数字）→ `odFlag = 该数字`（modData.js:172-177、:190-191）。
- 否则 HR → `"HR"`（OD 上升）、EZ → `"EZ"`（OD 下降）（modData.js:192-196）。
- 传递给估算器（`estimatorOptions.odFlag` analysis.js:443），用于 OD 相关的判定与调整（具体数学不在本文范围）。

### cvtFlag（convert 转换标记，modData.js:198-204）

**仅 lazer**：`IN` → `"IN"`、`HO` → `"HO"`，否则 `null`。stable 恒为 null（stable 无 IN/HO 概念）。

- 语义是**把谱面按某 mod 的效果"转换"后再计算**，取值域 `null | "IN" | "HO"`：
  - `"IN"`：IN（Inverse）转换——`js/parser/osuFileParser.js:279 modIN()` 将同一列相邻 note 反转为 LN（modData 提取后由估算器内部触发）。
  - `"HO"`：HO（Hidden 类）转换——`osuFileParser.js:326 modHO()` 将 LN 一律拆为普通 note（LN→RC）。
- 消费方：
  - **Sunny 系估算器**：`js/rework/sunnyAlgorithm.js:196 preprocessFile(osuText, speedRate, odFlag, cvtFlag)` 内 `String(cvtFlag).includes("IN")` → `pObj.modIN()`、`.includes("HO")` → `pObj.modHO()`（sunnyAlgorithm.js:202-218）。`sunnyWindowAlgorithm.js:204-212` 同构。注意用 `includes` 判断，未来若出现组合值（如 `"IN|HO"`）也能命中。
  - **Interlude**：`cvtFlag` 作为第三参数传入（analysis.js:590）。
  - **Etterna**：`buildEtternaAnalyzeOptions` 的 `cvtFlag: state.cvtFlag`（analysis.js:156）。
- **azusaSunnyReferenceHo 关联**（调试设置）：Azusa 内部跑 Sunny 参考值时，`js/estimator/azusaEstimator.js:825 forceSunnyReferenceHo`（`options.forceSunnyReferenceHo !== false`）为真时强制给 Sunny 参考传入 `cvtFlag: "HO"`（azusaEstimator.js:901-902）——即参考值统一按"纯 RC 化"处理。设置值来自 `state.azusaSunnyReferenceHo`（appContext.js:90），经 `azusaOptions`（analysis.js:449-452）与 worker 透传（`js/app/worker/compute.worker.js:35` 默认 true）。**注意：这是独立于实际 mod 的强制转换，与 cvtFlag 本身是两回事**。

## 6. Etterna 版本回退

版本注册表 `js/ett/versions/index.js`（6 个 WASM 版本，见 `ETTERNA_VERSION_REGISTRY` index.js:10-36）：

| 常量/函数 | 位置 | 说明 |
| --- | --- | --- |
| `DEFAULT_ETTERNA_VERSION` | index.js:38 | `"0.72.3"`，首选版本未命中注册表时的兜底 |
| `NON_4K_ETTERNA_FALLBACK_VERSION` | index.js:8 | `"0.74.0"`，**所有非 4K**（5K–18K）的固定版本：0.74.0 是首个带真 n-key 管线的 MinaCalc（内部 515），更早构建的 FFI 只放行 4/6/7 且 4K 算法拒收更宽掩码 |
| `normalizeEtternaVersion` | index.js:73-80 | 归一化（`"0.68.0"` → `"0.68.0-Unofficial"`），非法值回 DEFAULT |
| `resolveEtternaVersionLoader(value)` | index.js:82-107 | 版本 → loader；不可用则回退到注册表第一个可用版本并带 `fallbackReason` |
| `supportsEtternaKeycount(version, keycount)` | index.js:59-71 | 版本是否支持某键数（注册表每项 `supportedKeycounts` 均为 `[4, 5, ..., 18]`，来自 `SUPPORTED_KEYS`，index.js:7；0.74.0/0.75.0 的 WASM 亦在 FFI 层放行 4..18） |
| `resolveEtternaVersionLoaderForKeycount(value, keycount)` | index.js:109-156 | 最终入口：先 `resolveEtternaVersionLoader`，再按键数回退 |

回退链（`resolveEtternaVersionLoaderForKeycount`）：

1. 解析用户所选版本（index.js:110）。
2. **非 4K 偏好**：`parsedKeycount !== 4` 且当前版本不是 0.74.0 时，直接改用 `NON_4K_ETTERNA_FALLBACK_VERSION`（index.js:113-127），原因记为 `Using 0.74.0 for non-4K stability`。
3. 当前版本支持该键数 → 原样返回（index.js:129-131）。
4. 不支持 → 优先 0.74.0，再遍历注册表找第一个支持的版本；都没有则原样返回（index.js:133-155）。

**0.75.0 说明**：MinaCalc 内部 527（0.74.0 的后续调参版），作为 4K 可选版本加入设置；非 4K 恒定走 0.74.0（0.75.0 的 n-key 同为实验质量，锁定 0.74.0 保证一致性）。

**调用方**：`js/ett/calc.js:147`（`js/ett/calc.js:4` 导入）。WASM 内部机制见 [../features/difficulty-estimation.md](../features/difficulty-estimation.md)，不在本文范围。

**版本设置**（独立两路）：

- `state.etternaVersion`（appContext.js:91，默认 `config.js:82` 的 `"0.72.3"`）→ settings.json:382 `etternaVersion`，驱动普通 Etterna 计算。
- `state.companellaEtternaVersion`（appContext.js:92，默认 `config.js:83` 的 `"0.74.0"`）→ settings.json:397 `companellaEtternaVersion`，**Companella 估算器专用**，与主版本互不影响。

## 7. modSignature 在缓存键中的作用

缓存键 = `star-v2|state.estimatorAlgorithm|state.lastBeatmapIdentity|state.modSignature`（`js/app/analysis.js:305`，`star-v2` 为星数统一语义的版本前缀）：

- mod 变化 → `modSignature` 变化 → 缓存键变化 → 旧快照 miss → 重新计算。同一谱面开 DT 与不开 DT 是**两个缓存条目**，互不污染。
- 键的第三段就是 §3 的四元组签名（速率/OD/cvt/classic 任一变化即换键）。
- 完整命中覆盖检查与写门见 [result-cache.md](result-cache.md)。

## 8. 注意事项

- **api_v2 部分包**：部分包不含 mod 字段。若无条件应用会造成 `state.modSignature` 被空载荷覆盖（或抖动），因此 socketHandlers.js:255-256 的注释明确"仅在 mod payload 显式存在时应用"；无载荷时保持 `previousModSignature`（socketHandlers.js:257-261）。
- **changeKind = "mod"**：谱面身份（identity）不变而仅签名变化时，`changeKind` 置为 `"mod"`（socketHandlers.js:278-286，`:279` 默认值），渲染层据此选择入场动画（换歌/换难度/仅改 mod 三态，注释见 socketHandlers.js:274-277）。
- **lazer vs stable 差异**：cvtFlag 仅 lazer 有（modData.js:198）；lazer 有 speed_change/DA-OD 数字通道（modData.js:167-177），stable 只能走 bitflag（`addCodesFromNumber`）。混合场景（tourney/ipcClients）也会被收集（modData.js:14-19）。
- **MR 是识别但无用的代码**：在 knownCodes 中（config.js:118）故能被提取，但全库无任何计算分支引用它，仅影响 `hasModInfo`（有 MR 时不算"无 mod"）。
- **速率优先级**：lazer 自定义倍速 > NC/DT > HT/DC > 1.0（modData.js:182-188）。NC/DT 与 HT/DC 不可能同时出现，若同时命中先判加速后判减速。
- **签名精度**：速率保留 5 位小数（modData.js:225），lazer 自定义速率如 1.05/0.9 等非整数值也能稳定区分；`1.00000|none|none|0` 为无 mod 且非 Classic 的基准签名。
- **新增计算相关 mod 代码**：需同时加入 `knownCodes`（config.js:118）与（若需 stable bitflag）`bitFlags`（config.js:119-125）；`SORTED_KNOWN_MOD_CODES`/`MOD_BIT_FLAG_ENTRIES` 是派生导出，无需手动改。若新代码参与计算（速率/OD/cvt），必须在 `getModData` 的判定分支（modData.js:182-204）与签名构建处同步处理，否则签名不反映其影响（缓存会按旧语义命中）。
## 多数据源（外部源）补充

外部源（Etterna/Malody）**不使用 modData 派生**：`externalSource.js` 直构

```
{ speedRate: rate.toFixed(5), odFlag: "none", cvtFlag: "none", classic: 0 }
```

写入 `state.modSignature`（4 段格式与 osu 一致，classic 位即 0，与 client 值无关），
保证跨在线/离线模式签名稳定、缓存键不抖。Etterna 的 rate（如 1.5x）进入
speedRate 段并联接 `state.speedRate`（分析消耗点 analysis.js `musicRate`/
estimator `speedRate`），同图不同 rate 输出值不同且缓存独立。
