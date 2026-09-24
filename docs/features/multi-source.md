# 多数据源：Etterna、Malody V、Malody 4 接入

> 面向 AI 的技术文档。给人类的使用安装说明见 `docs/shell-guide.md` 与 `bridges/` 下的安装说明。Malody 4.3.7 原生客户端源的完整说明（只读观察通道、版本门、索引、帧与限制）见 [malody4-source.md](malody4-source.md)；其动态判定 OD 见 [malody-od.md](malody-od.md)。

## 功能说明

插件在 osu!mania（tosu）单数据源之外，增加 **Etterna**、**Malody V** 与 **Malody 4.3.7 原生客户端**三个游戏数据源（合共四源）。玩家切游戏时分析卡自动跟随。数据源层新增「文件桥/Web Post/零注入只读观察」接入，**算法层零改动**：`.sm/.ssc/.mc` 由转换器转成 `.osu` 文本后进入既有 `runAnalysisPipeline`。

四个源各有独立 identity 前缀：osu 沿用既有 id/hash/path、Etterna `ett:`、Malody V `mdy:`、Malody 4 `mdy4:{md5}`（`mdy4:` 与 `mdy:` 是两个不同源的独立前缀，互不命中）。

## 架构

```
Etterna（主题 Lua 桥写 Save/MmaBridge.txt + MmaGameplay.txt）
  → 壳（desktop，2Hz 轮询 → song 帧）
Malody V 有两条通道，游戏内选曲桥优先、编辑器通道回退：
  A. 游戏内选曲桥（`bridges/malody/bepinex/`，BepInEx 6 IL2CPP 插件
     `MMAMalodySelection.dll`，本仓库自建 fork）：插件在选曲/游玩/结算各挂点
     POST `127.0.0.1:17653/selection`（8→11 字段，含判定档/Pro/Turbo）→ 壳归一化 + 去重
     → song 帧；**选曲界面即跟随**，不需用户做任何操作
  B. 编辑器插件（`bridges/malody/mma_editor.lua`）WriteFile `<base>_mma_request.json`
     → 壳 ≤1Hz 扫 chart/ → 谱面 = 同目录 `<base>.mc|.osu` → song 帧；处理完删 request
Malody 4.3.7（原生客户端，零注入只读观察：ReadProcessMemory 读锚点身份键
  `<md5>_<slot>` + tail 游戏日志取场景 + 轮询 config.json 取变速位/判定档
  → 壳 200ms 轮询 → `malody4_selection` 帧 / song 帧；游戏侧零文件、零注入）
  → 壳 WS/HTTP → 页面 sources/（bridgeClient → externalSource → state 注入）
  → fetchBeatmapFile（外部文本绕过 tosu 抓取）→ 既有管线
  → 分析结果展示在壳窗口卡片（不回写 txt 到游戏内）
```

- 双宿主：浏览器版（无壳）自动降级 osu 单源；壳版在线（tosu 存活）双活数据面。
- **Malody V 游戏内选曲桥通道说明**：游戏侧是 BepInEx 6 IL2CPP 插件（`local.mma.malody.selection`），源码在 `bridges/malody/bepinex/plugin/`，安装走 `bridges/install-bridge.ps1 -Game MalodyBridge`。插件把选曲/游玩/结算的观察 POST 到 `127.0.0.1:17653/selection`（先 8 字段，现 11 字段），壳侧归一化、去重、按内容六元组（含 Pro 与 Turbo）判定真实事件，再广播 song/state 帧。**它不需要用户打开任何面板**：判定档与 Pro 都由插件主动读取（判定档来自播放设置记录，Pro 来自 `Malody.Play.boq` 的 `bcgh`），面板记录仅作交叉校验。**不得与上游 `MalodyInsightBridge` 共存**（双 Hook）。
- Malody 编辑器通道说明：Malody `WriteFile` 只能写当前谱面目录且强制加谱面名前缀（`<base>_mma_request.json`）；`DoRequest` 的 POST+body 被 Malody 网络层拒绝（`invalid url: {body}`）；壳用 request 文件名 base 锁定同目录 `<base>.mc|.osu`（与命名/格式无关，.osu 谱也可分析）。不装插件时这是 Malody V 的唯一通道。
- Malody 4.3.7 通道说明：**不向游戏目录写入任何文件**、不注入 DLL、不用 BepInEx/Lua、不修改游戏内存；只支持 4.3.7 这一个二进制（PE `TimeDateStamp = 0x5D79AC91` 且文件大小 `4,750,848`，不符则整条通道不可用并给出 `target-mismatch:*` 原因），且仅 Windows。
- 契约：`desktop/docs/CONTRACT.md`（**版本 5**：`song` 帧与 `sources.malody` 都新增 `pro`/`turbo`、`winScale` 可为 `null`；页面接受 `[3,5]`。帧型与领域清单见该文件）。

## 转换器

- `js/parser/smSscToOsuConverter.js`：vendor simfile-parser（MIT，见 `js/parser/vendor/simfile-parser/NOTICE.md`，含列宽补丁）→ STOPS/DELAYS 烘焙、WARPS 折叠、键数行宽推导、LN 尾冲突修复；OD9/HP8/AR5。
- `js/parser/mcToOsuConverter.js`：移植 mc_to_osu.py（SV 负红线、type128、尾微调）；HP8/AR5 固定，OD 默认 9（不传参时逐字节不变），`malody4` 源显式传入由判定档 × 速率算出的等效 OD（见 [malody-od.md](malody-od.md)）。
- 测试与 golden 摘要：`docs/pipeline/converters.md`（真实样本仅本机私有，仓库只存摘要与断言；测试脚本本地私有）。

## 多源路由（sourceManager）

- 设置 `gameClient`：`Auto`（默认）/ `Osu!` / `Etterna` / `Malody` / `Malody 4`。
- Auto 决策表（`js/app/sources/sourceManager.js`）：
  - L1 游玩态抢占（osu=isInPlayState 信号豁免照读；etterna=playing 外推；malody4=壳 state 帧的 `playing` 位（场景 3 且新鲜 ≤10s）；malody=无游玩态信号）；
  - L2 60s 新鲜事件窗口（osu=换谱/换 mod/改 rate；etterna=桥写入；malody=POST/song 帧；**malody4=非心跳且 `path` 非空的选曲帧——hidden 帧与心跳永不续约**，否则壳只要在跑就会把路由永久钉在 malody4）；
  - L3 hold 只作用于当前源，他源新鲜事件无游玩态可抢占；窗口过期按 osu > Etterna > Malody 4 > Malody 重选；
  - L3' 存活回窗（无窗口源时 tosu 在线 → osu）；
  - L4 全离线 → 无源（圆点灰空心）。
- 切源 debounce 200ms，旧结果保留；源圆点（状态行末端）：**四色**实心（osu! #ff66aa / Etterna #a855f7 / Malody 4 #22d3ee 亮青 / Malody V #3b82f6，各源品牌色）或灰空心（无源）。Malody 4 与 Malody V 是两个独立源，圆点颜色可区分。

## osu 败方门控

`activeSource≠osu` 时挂起 tosu 的 identity/mod 应用与 recompute/sendCommand（信号段照常），并缓冲最后一条 tosu 包；切回 osu 先回放再 recompute（缓存键对齐，命中即零额外分析）。实现：socketHandlers 拆 `applySignalState`/`applyBeatmapState`，注册点按 `isOsuSuppressed()` 包装。

## 各游戏能力边界（如实标注）

| 能力 | Etterna | Malody V | Malody 4.3.7 |
|---|---|---|---|
| 谱面跟随 | 精确（选歌桥 key 门控写一次） | 编辑器场景精确（文件通道：request base 锁定同目录谱面） | 精确（游戏锚点身份键 md5 → 索引查谱；只索引 `.mc`） |
| rate | speedRate=rate.toFixed(5) 入签名（同图不同 rate 缓存独立） | 无 rate 概念按 1.0 | 模组位 DASH 1.2 / RUSH 1.5 / SLOW 0.8（互斥；多位同时命中按 DASH>RUSH>SLOW 取值并记日志），**OD 随之动态化** |
| mod | 无（桥不提供） | 无（PlayMeta 字段未证实；真机验证项） | 模组位（只读 `config.json` 的 `user_mods`，不连乘；FAIR 判定位不建模、由壳记 warn） |
| 原生 MSD | 桥 msd×8（meta.devMsd8，**仅开发对照**，页面显示为 MinaCalc 自算） | 无 | 无 |
| 暂停检测/livePP | 不做（非 osu 砍） | 不做 | 不做 |
| 向游戏目录写入文件 | 不写（只写 `Save/`） | 不写（编辑器插件自行写 request） | **零写入**（纯只读观察；安装器只写配置文件） |
| 需要管理员权限 | 不需要 | 不需要 | 不需要（同用户读进程；被策略挡住如实报 `access-denied`） |

## 遥测

analyze 事件新增 `client` 字段（osu/etterna/malody/**malody4**，取值为当前活跃源 id）；后端 daily_agg 新增 `client` 维度，dashboard Client 饼图与 Version 并列一行（`docs/features/telemetry.md`）。本轮不为遥测扩判定档维度（判定档无法由现有字段还原，属已知限制）。

## 已知未完成 / 验证中

- 离线模式页面侧设置拉取与持久化（壳 `/settings` 双向已实现，页面接线待办）；
- 外部源封面（壳 cover 帧已下发 URL，页面 coverTheme 消费待办）；
- 真机验证项：Etterna 主题桥写文件与消息在真实游戏运行；Malody V DoRequest 签名/URL 限制、PlayMeta 字段（皮肤显示方案已废弃，不再涉及皮肤目录）；Malody 4 真机端到端跟随（成功标准逐项记录见 `.omo/evidence/malody4-source/`）；
- 浏览器端到端（壳+页面）验证需 tosu/MalodyV 运行环境；
- Malody 4 源本轮未做项：**screen 显示门控**（`screen` 只进 state 帧与诊断，卡片不会按场景隐藏/清空）、**`.osu` 索引**（只索引 `.mc`，库里的 `.osu` 谱不会被跟随）、**多库/多实例**（只支持单一 `malody4Root` 与单一 `malody.exe`；多实例整源不可用为 `multiple-instances`）、**Malody V 的判定窗口表**（PC 表不能外推到它）、**FAIR 判定模组建模**；
- 本轮翻案记录：该源此前被判定不做（`.omo/plans/malody-v-bepinex-selection-bridge.md:19`：原生 x86 C++、无 Unity/Lua，支持它"等于另开一个原生注入逆向项目"），本轮因**零注入只读通道**（进程外读锚点 + 日志 + 配置文件，游戏侧零安装）被实测验证可行而重新接入。

# Multi-source: Etterna, Malody V and Malody 4

Technical document for AI readers. Human installation guides: `docs/shell-guide.md` and per-bridge READMEs. The Malody 4.3.7 native-client source has its own full document ([malody4-source.md](malody4-source.md)) and its dynamic judge OD table lives in [malody-od.md](malody-od.md).

Adds Etterna, Malody V and the Malody 4.3.7 native client as live data sources beside osu!mania/tosu, with automatic follow on game switch. **Zero algorithm-layer changes**: `.sm/.ssc/.mc` are converted to `.osu` text and enter the existing pipeline. Each source has its own identity prefix: osu keeps id/hash/path, Etterna `ett:`, Malody V `mdy:`, Malody 4 `mdy4:{md5}` (independent prefixes that never collide).

- Converters: `js/parser/{smSscToOsuConverter,mcToOsuConverter}.js` (vendor simfile-parser MIT, STOPS/DELAYS baked, key-count from row width, LN tail fix; fixed HP8/AR5 — `.sm/.ssc` keep OD9 while the `malody4` source passes an explicit judge-and-rate equivalent OD, and the converter default stays 9 byte-identically; real samples and test scripts stay local-only).
- Malody 4.3.7 channel: zero-injection read-only observation (anchor `ReadProcessMemory`, game-log tail, `config.json` polling), **no file written into the game folder**, no injection and no Lua; only the 4.3.7 binary is supported (PE `TimeDateStamp = 0x5D79AC91` and size `4,750,848`, otherwise the whole channel reports `target-mismatch:*`), Windows only.
- Router: `js/app/sources/sourceManager.js` decision table L1–L4+L3' (play-state > 60s fresh-event window with hold/preempt > priority reselect > tosu-alive re-entry > none); priority `osu > Etterna > Malody 4 > Malody`; forced `gameClient` (also `Malody 4`); the malody4 L2 window is renewed only by a non-heartbeat selection frame with a non-empty `path`; source dot uses each game's brand color (osu! pink / Etterna purple / Malody 4 cyan `#22d3ee` / Malody V blue).
- osu gate: beatmap-state handler suspended while another source routes (signals exempt, buffered replay on return).
- Malody V has two channels: the in-game song-selection bridge (`bridges/malody/bepinex/`, a BepInEx 6 IL2CPP plugin built from this repo, GUID `local.mma.malody.selection`) which POSTs observations to `127.0.0.1:17653/selection` and follows selection, gameplay and results with **no user action required**, and the editor Lua plugin (`mma_request.json`) which remains as the fallback when the bridge is not installed. Never co-install the upstream `MalodyInsightBridge` (double hooking).
- Bridge contract: `desktop/docs/CONTRACT.md` (v5: `song` and `sources.malody` both carry `pro`/`turbo`, `winScale` may be `null`; the page accepts `[3,5]`).
- Telemetry: analyze `client` field (values include `malody4`), dashboard Client pie with Version on its own row.
- Boundaries documented: Malody V results only via editor POST (resolve by title/path); malody4 follows the game's own anchor md5 and indexes `.mc` only; rate→speedRate; devMsd8 dev-only; pause/livePP not implemented for non-osu; malody4 needs no admin rights and writes nothing into the game folder. Skin display removed (2026-09).
- Known gaps: offline page settings wiring, external cover consumption, live PoC items (DoRequest/ReadFileSelect/PlayMeta), malody4 screen-based display gating, `.osu` indexing, multi-library/multi-instance, the Malody V judge-window table and FAIR modelling, browser end-to-end pending environment. This source was previously ruled out (`.omo/plans/malody-v-bepinex-selection-bridge.md:19`) and was reinstated once the zero-injection read-only channel was verified in practice.