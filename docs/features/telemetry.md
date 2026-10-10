# docs/features/telemetry.md — 匿名使用统计（遥测）

> 面向 AI（LLM）的功能技术文档。目标：描述插件侧遥测功能 `js/app/telemetry.js` 的实现与隐私边界。后端实现（Go + SQLite + 看板 + OBS 备份）位于仓库内 `backend/` 子项目，其公开说明见 `backend/README.md`。

## 1. 功能说明

插件在浏览器（tosu overlay）内匿名上报三类事件到硬编码在 `index.js` 的遥测端点（`window.__MMA_TELEMETRY_ENDPOINT`，当前为 `https://mma-stats.leoblack.top`），供项目维护者统计：活跃用户数、在线分布、使用行为（算法/键数/mod/模式/难度）。

| 事件 | 时机 | 载荷 |
|---|---|---|
| `boot` | 插件启动、且遥测开启且 endpoint 非空 | 仅 id/kind/version |
| `heartbeat` | 每 **10 分钟**一次，且最近 30s 内收到 api_v2（游戏已连接） | 仅 id/kind/version |
| `analyze` | 每次成功分析后 | id/kind/version + `data` 字段 |

## 2. 隐私边界（硬约束）

- **匿名标识**：`crypto.randomUUID()`（或回退随机 hex）生成的随机 UUID，存 `localStorage` 键 `mma.telemetry.installId.v1`（失败回退 sessionStorage → 内存）。同一机器同一 tosu 数据目录多实例共享 → 天然去重；换机器/清数据才变。
- **明确不采**：用户名、玩家 id、分数/acc、谱面 md5/标题、IP（后端不存，连哈希都不存）、UA/OS、时区。
- **采集字段白名单**（`analyze.data`，与后端 `backend/internal/telemetry/handler.go` 的 `allowedDataKeys` 严格一致）：
  `algorithm`、`actualAlgorithm`、`keycount`、`mods`、`speedRate`、`mode`、`star`、`lnRatio`、`typeBreakdown`、`durationMs`、`numericDifficulty`、`client`。
  - `client` 语义：本次分析实际生效的**数据源**，取值 = **`lazer` / `stable`（壳内 native 内存读取）/ `lazer(tosu)` / `stable(tosu)`（tosu 供数）/ `malody` / `malody4` / `etterna`**（`js/app/analysis.js` 在请求开始时快照）。Malody / Etterna 源直接取 `state.activeSource`（`malody` 覆盖 BepInEx 通道与编辑器插件两类）；osu 源先定**游戏客户端**（`state.client`，native 下取自我们自己的载荷、tosu 下取自 tosu 的载荷，两处都是 `stable`/`lazer`，非 lazer 一律计 stable），再定**谁在供数** —— 壳的 native 端点生效 ⇒ 裸值，否则（无壳 / 浏览器 tosu 页 / 旧壳 ≤v5 / native 已回落）⇒ 加 `(tosu)` 后缀。**后端不为此新增字段或维度**：`daily_agg` 仍以 `client` 为维度聚合，dashboard 的 Client 饼图与 Version 并列一行。
  - `actualAlgorithm` 与 `algorithm` 值域守卫：统一由 `js/app/telemetrySpec.js` 约束并与后端 `backend/internal/spec/spec.go` 保持镜像对齐。主算法 `algorithm` 仅放行 `Mixed/Sunny/Azusa/Daniel/Roxy/Companella`，其余草稿/魔改名不上报；实际算法 `actualAlgorithm` 仅放行 `Sunny/Daniel/Azusa/Roxy/Companella`，胶囊标签 `"Azusa+Companella"` 自动映射归入 `"Azusa"`。不在白名单的字段不予发送。
  - `version` 防污染守卫：仅正式 SemVer 版本（`^v?[0-9]+\.[0-9]+(?:\.[0-9]+)?$`）允许激活遥测；任何带 `-dev`, `-test`, `debug`, `dirty`, `local` 等后缀的非正式版本客户端一律不发送遥测，服务端若收到也静默丢弃（204），防止本地开发与魔改数据污染大盘。
  - `durationMs` 异常防污染：上限阈值设为 30000ms（30 秒）。超过 30 秒（例如断点调试或休眠恢复）的分析耗时直接整项丢弃，不截断、不上报、不参与耗时与极值统计，避免产生假峰或极值失真。
  - `star` 星数保持真实：不设置星数极值过滤，完整记录真实星数。
  - `numericDifficulty` 语义：标准数值化难度（Reform 段位体系，**.0 = mid**）。Azusa/Roxy（含 Mixed 路由到它们）用原生连续值；其余算法（Sunny/Companella/Daniel）用 `rcLabelToNumeric(estDiff)` 从估计难度字符串**反向换算**——Daniel 原生值是 DP 尺度（比标准约高 0.5），故 Daniel 一律走反解。边界标签（`< Alpha Low`、`> Emik Zeta high`、`Unknown` 等）反解为 null → **不发送该字段**。
  - **已知限制（本轮不扩维）**：Malody 4 源的等效 OD 由"判定档 × 速率"决定，而遥测只有 `speedRate`、没有判定档，故**无法从遥测还原该源实际使用的 OD**（判定档不进任何帧之外的字段，页面也不上报它）。本轮明确**不为遥测扩维**，此限制如实记录于此。
- **开关**：设置项 `enableTelemetry`（Network 分组，默认开），用户可关；endpoint 为空时完全不发送（默认开启但未配置 = no-op）。

## 3. 模块设计

`js/app/telemetry.js` 是**纯浏览器**模块（仿 `updateChecker.js` 的 localStorage try/catch 模式），不 import 任何 estimator/parser 共享模块，不影响 benchmark。对外 API：

- `initTelemetry()` — 读配置，条件满足则发 `boot`。
- `setTelemetryConfig()` — settings.js 运行时调用；开关由关→开且 endpoint 非空时补发 `boot`。
- `noteTelemetryActivity()` — socketHandlers 每个 api_v2 包调用，更新 `lastActivityAt`。
- `startTelemetryHeartbeat()` / `stopTelemetryHeartbeat()` — 遥测激活时才启动 `setInterval(10min)`；`setTelemetryConfig()` 在关闭遥测时同步停止 interval，避免禁用后定时器常驻。心跳仅在 `enabled && endpoint && (now-lastActivityAt < 30s)` 时发送。
- `trackTelemetryAnalyze(data)` — 发 `analyze`（fire-and-forget）。

发送用 `fetch(..., {keepalive:true})` + 5s `AbortController` 超时，`.catch` 静默——**遥测绝不阻塞/破坏插件功能**。

## 4. 埋点位置

- `index.js`：`const TELEMETRY_ENDPOINT = "https://mma-stats.leoblack.top"` + `window.__MMA_TELEMETRY_ENDPOINT`。
- `main.js` `initialize()`：`loadSettings()` 之后 `initTelemetry()` + `startTelemetryHeartbeat()`。
- `socketHandlers.js`：`setupSocketListener()` 的 `api_v2` 回调顶部 `noteTelemetryActivity()`。
- `analysis.js` `fetchBeatmapFile()`：顶部记录 `analysisStartedAt`；成功路径末尾（`rework && metadataErrors.length===0 && !isStaleRequest()`）调 `trackTelemetryAnalyze(...)`。缓存命中/未命中都上报（命中 `durationMs≈0` 体现缓存效果）；失败、stale、auto-profile 提前 return 均不上报。

## 5. 与 CLAUDE.md 的关系

CLAUDE.md 原规定「不应当使用除 tosu 之外的第三方工具获取数据/计算」。本项目作者已明确授权**豁免这一条**用于遥测：插件向自建后端上报匿名统计是唯一允许的 tosu 之外数据去向。已同步在 CLAUDE.md 记录该豁免。

## 6. 设置项

唯一新增设置 `enableTelemetry`（checkbox，Network 分组，默认 `true`）。它是**纯遥测开关**：不进 `recomputeNeeded`，不进 `clearResultCache()` 失效列表，只进 `changed` 聚合（与 `enableNumericDifficulty` 同类「立即应用、不重算」）。
