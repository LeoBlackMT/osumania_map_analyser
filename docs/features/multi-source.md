# 多数据源：Etterna / Malody 接入

> 面向 AI 的技术文档。给人类的使用安装说明见 `docs/shell-guide.md` 与 `bridges/` 下的安装说明。

## 功能说明

插件在 osu!mania（tosu）单数据源之外，增加 **Etterna** 与 **Malody V** 两个游戏数据源。玩家切游戏时分析卡自动跟随。数据源层新增「文件桥/Web Post」接入，**算法层零改动**：`.sm/.ssc/.mc` 由转换器转成 `.osu` 文本后进入既有 `runAnalysisPipeline`。

## 架构

```
Etterna（主题 Lua 桥写 Save/MmaBridge.txt + MmaGameplay.txt）
  → 壳（desktop，2Hz 轮询 → song 帧）
Malody（编辑器插件 WriteFile `<base>_mma_request.json` → 壳 ≤1Hz 扫 chart/
  → 谱面 = 同目录 `<base>.mc|.osu` → song 帧；处理完删 request）
  → 壳 WS/HTTP → 页面 sources/（bridgeClient → externalSource → state 注入）
  → fetchBeatmapFile（外部文本绕过 tosu 抓取）→ 既有管线
  → 分析结果展示在壳窗口卡片（不回写 txt 到游戏内）
```

- 双宿主：浏览器版（无壳）自动降级 osu 单源；壳版在线（tosu 存活）双活数据面。
- Malody 编辑器通道说明：Malody `WriteFile` 只能写当前谱面目录且强制加谱面名前缀（`<base>_mma_request.json`）；`DoRequest` 的 POST+body 被 Malody 网络层拒绝（`invalid url: {body}`）；壳用 request 文件名 base 锁定同目录 `<base>.mc|.osu`（与命名/格式无关，.osu 谱也可分析）。
- 契约：`desktop/docs/CONTRACT.md`（版本 2，九领域：帧七型/身份/requestId 状态机/应答两来源/settings 在线只读离线双向/封面白名单/state 周期/contract 终态；皮肤状态文件章节已标注废弃）。

## 转换器

- `js/parser/smSscToOsuConverter.js`：vendor simfile-parser（MIT，见 `js/parser/vendor/simfile-parser/NOTICE.md`，含列宽补丁）→ STOPS/DELAYS 烘焙、WARPS 折叠、键数行宽推导、LN 尾冲突修复；OD9/HP8/AR5。
- `js/parser/mcToOsuConverter.js`：移植 mc_to_osu.py（SV 负红线、type128、尾微调）。
- 测试与 golden 摘要：`docs/pipeline/converters.md`（真实样本仅本机私有，仓库只存摘要与断言；测试脚本本地私有）。

## 多源路由（sourceManager）

- 设置 `gameClient`：`Auto`（默认）/ `Osu!` / `Etterna` / `Malody`。
- Auto 决策表（`js/app/sources/sourceManager.js`）：
  - L1 游玩态抢占（osu=isInPlayState 信号豁免照读；etterna=playing 外推）；
  - L2 60s 新鲜事件窗口（osu=换谱/换 mod/改 rate；etterna=桥写入；malody=POST/song 帧）；
  - L3 hold 只作用于当前源，他源新鲜事件无游玩态可抢占；窗口过期按 osu>Etterna>Malody 重选；
  - L3' 存活回窗（无窗口源时 tosu 在线 → osu）；
  - L4 全离线 → 无源（圆点灰空心）。
- 切源 debounce 200ms，旧结果保留；源圆点（状态行末端）：三色实心（osu! #635bff / Etterna #0d9c5f / Malody #f5a623，与遥测 Client 饼图一致）或灰空心（无源）。

## osu 败方门控

`activeSource≠osu` 时挂起 tosu 的 identity/mod 应用与 recompute/sendCommand（信号段照常），并缓冲最后一条 tosu 包；切回 osu 先回放再 recompute（缓存键对齐，命中即零额外分析）。实现：socketHandlers 拆 `applySignalState`/`applyBeatmapState`，注册点按 `isOsuSuppressed()` 包装。

## 各游戏能力边界（如实标注）

| 能力 | Etterna | Malody |
|---|---|---|
| 谱面跟随 | 精确（选歌桥 key 门控写一次） | 编辑器场景精确（文件通道：request base 锁定同目录谱面） |
| rate | speedRate=rate.toFixed(5) 入签名（同图不同 rate 缓存独立） | 无 rate 概念按 1.0 |
| mod | 无（桥不提供） | 无（PlayMeta 字段未证实；真机验证项） |
| 原生 MSD | 桥 msd×8（meta.devMsd8，**仅开发对照**，页面显示为 MinaCalc 自算） | 无 |
| 暂停检测/livePP | 不做（非 osu 砍） | 不做 |

## 遥测

analyze 事件新增 `client` 字段（osu/etterna/malody）；后端 daily_agg 新增 `client` 维度，dashboard Client 饼图与 Version 并列一行（`docs/features/telemetry.md`）。

## 已知未完成 / 验证中

- 离线模式页面侧设置拉取与持久化（壳 `/settings` 双向已实现，页面接线待办）；
- 外部源封面（壳 cover 帧已下发 URL，页面 coverTheme 消费待办）；
- 真机验证项：Etterna 主题桥写文件与消息在真实游戏运行；Malody DoRequest 签名/URL 限制、PlayMeta 字段（皮肤显示方案已废弃，不再涉及皮肤目录）；
- 浏览器端到端（壳+页面）验证需 tosu/MalodyV 运行环境。

# Multi-source: Etterna / Malody

Technical document for AI readers. Human installation guides: `docs/shell-guide.md` and per-bridge READMEs.

Adds Etterna and Malody V as live data sources beside osu!mania/tosu, with automatic follow on game switch. **Zero algorithm-layer changes**: `.sm/.ssc/.mc` are converted to `.osu` text and enter the existing pipeline.

- Converters: `js/parser/{smSscToOsuConverter,mcToOsuConverter}.js` (vendor simfile-parser MIT, STOPS/DELAYS baked, key-count from row width, LN tail fix; OD9/HP8/AR5; real samples and test scripts stay local-only).
- Router: `js/app/sources/sourceManager.js` decision table L1–L4+L3' (play-state > 60s fresh-event window with hold/preempt > priority reselect > tosu-alive re-entry > none); forced `gameClient`; source dot colors match telemetry pie.
- osu gate: beatmap-state handler suspended while another source routes (signals exempt, buffered replay on return).
- Bridge contract: `desktop/docs/CONTRACT.md` (v2).
- Telemetry: analyze `client` field, dashboard Client pie with Version on its own row.
- Boundaries documented: Malody results only via editor POST (resolve by title/path); rate→speedRate; devMsd8 dev-only; pause/livePP not implemented for non-osu. Skin display removed (2026-09).
- Known gaps: offline page settings wiring, external cover consumption, live PoC items (DoRequest/ReadFileSelect/PlayMeta), browser end-to-end pending environment.