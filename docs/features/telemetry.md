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
  `algorithm`、`actualAlgorithm`、`keycount`、`mods`、`speedRate`、`mode`、`star`、`lnRatio`、`typeBreakdown`、`durationMs`。
  - `actualAlgorithm` 语义：Mixed 是自动路由算法，`analysis.js` 上报的是**路由后实际命中的子算法**（Roxy/Azusa/Daniel/Companella/Sunny，见 `js/estimator/mixedEstimator.js` 的 `actualEstimatorAlgorithm` 返回值与 `runAnalysisPipeline.js` Mixed 分支），不再是字面 "Mixed"；其余算法（Azusa/Roxy 无效回退）同样上报实际结果。
- **开关**：设置项 `enableTelemetry`（Network 分组，默认开），用户可关；endpoint 为空时完全不发送（默认开启但未配置 = no-op）。

## 3. 模块设计

`js/app/telemetry.js` 是**纯浏览器**模块（仿 `updateChecker.js` 的 localStorage try/catch 模式），不 import 任何 estimator/parser 共享模块，不影响 benchmark。对外 API：

- `initTelemetry()` — 读配置，条件满足则发 `boot`。
- `setTelemetryConfig()` — settings.js 运行时调用；开关由关→开且 endpoint 非空时补发 `boot`。
- `noteTelemetryActivity()` — socketHandlers 每个 api_v2 包调用，更新 `lastActivityAt`。
- `startTelemetryHeartbeat()` — `setInterval(10min)`；仅当 `enabled && endpoint && (now-lastActivityAt < 30s)` 发 `heartbeat`。
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
