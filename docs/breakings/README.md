# docs/breakings/README.md

## 类别说明（中文）

- 本文档是 `docs/breakings/` 目录的说明和索引文档。
- 本目录存放**重大破坏性更改说明文档**（给人类和 AI 共同阅读），记录修改内容与修改原因，便于后续代码审查和测试。
- 命名约定：**时间戳 + 修改内容**（如 `2026-08-09-perf-analysis-pipeline.md`），与 `docs/README.md:17`（中文）/ `:59`（English）的"按时间戳和修改内容命名"约定一致。
- 写作要求：
  - 文档**必须双语**，人类与 AI 共同阅读（docs/README.md:17）。
  - 每项破坏性更改含五要素：**修改内容（What changed）/ 修改原因（Why，关联性能依据）/ 影响范围（Scope）/ 兼容策略（Compat）/ 验证方式（Verification）**。
  - 验证方式写为回归验证结论与实测摘要（不引用本地命令或证据路径）。
  - 所有内容描述**实际落地代码**，不写"将要在未来"的推测。
- 何时新增：对管线/共享模块/缓存语义做出重大破坏性更改时（docs/README.md:17）；同时更新对应的管线文档和指南文档（docs/README.md:16），并在 `docs/README.md` 登记索引（docs/README.md:18）。

## 文档索引

| 文档 | 日期 | 分支/主题 | 说明 |
| --- | --- | --- | --- |
| [2026-08-09-perf-analysis-pipeline.md](2026-08-09-perf-analysis-pipeline.md) | 2026-08-09 | perf/analysis-pipeline-optimization | 分析管线性能优化：worker 单次往返协议、runAnalysisPipeline 纯函数、共享模块纯度（worker 根因修复）、缓存失效收窄与命中重派生、vibro 修复披露、perf 验收口径修订（12 项） |
| [2026-08-16-preset-system-and-settings-schema.md](2026-08-16-preset-system-and-settings-schema.md) | 2026-08-16 | pr/45（preset system） | 预设系统（相对 main 净变更）：新增 presets 模块/presets.html/内置预设、设置监听器数据化（SETTING_HANDLERS）、settings.json 预设分组与按钮、presetStorage 单一权威存储、快照系统键剥离、写回幂等化 |
| [2026-08-30-marathon-correction-in-estimator.md](2026-08-30-marathon-correction-in-estimator.md) | 2026-08-30 | feat/marathon-correction（v2.0.2） | 马拉松时长修正架构重构：管线派生段 → 估算器内嵌（options.marathonCorrection 参数化），按需前置 Ett 复用，遵守 perf 约束（无重跑/无重复解析/复用 WASM），基准双口径显式化 |
| [2026-08-30-multi-source-data-sources-and-desktop-shell.md](2026-08-30-multi-source-data-sources-and-desktop-shell.md) | 2026-08-30 | feat/multi-source-shell（v2.1.0） | 多数据源与桌面壳：新增 Etterna/Malody V 外部源（转换器 + 页面 sources/ + 壳 + 桥）、fetchBeatmapFile 外部文本入口与 result 帧 finally 汇合、缓存身份/速率签名语义、`gameClient` 等设置与败方门控（缓冲回放）、遥测 `client` 维度、壳桥契约（版本 2） |
| [2026-09-20-ett-ux-and-companella-capsule.md](2026-09-20-ett-ux-and-companella-capsule.md) | 2026-09-20 | fix/ett-ux-and-companella-capsule | Etterna 错误体验与胶囊修复：MinaCalc abort → `minacalc-aborted` + 模块缓存回收 + 卡片 "Unsupported Chart"；junk-file 全零 → "MSD unavailable" 且 MSD 显示 `--`；Companella 融合/采用后胶囊更新（`Companella` / `Azusa+Companella`）；新增 `ettErrorCode`、`ettResult.junkFile`/`rowCount` |
| [2026-09-21-malody4-native-source.md](2026-09-21-malody4-native-source.md) | 2026-09-21 | feat/malody4-source | Malody 4.3.7 第四数据源（零注入只读观察）：锚点 md5 身份 `mdy4:`、桥契约版本 2→3 与八型帧（`malody4_selection`、`sources.malody4`、`requestId n{seq}`）、`malody4Root` 解析链与安装器第三项（零文件复制）、路由优先级与第四圆点色、插件版本冻结 2.1.0 导致陈旧页面契约不匹配 |
| [2026-09-21-malody4-dynamic-judge-od.md](2026-09-21-malody4-dynamic-judge-od.md) | 2026-09-21 | feat/malody4-source | Malody 4 动态判定 OD：写死 OD 9 → 判定档 × 速率等效 OD（PC 表 `-4.56~16.42`），`modSignature` 4 段扩 5 段（旧快照自动失效），仅 `malody4` 源受影响（osu/Etterna/Malody V 逐字节不变） |
| [2026-09-24-malody-v-selection-bridge-fork.md](2026-09-24-malody-v-selection-bridge-fork.md) | 2026-09-24 | feat/malodyv-bepinex-bridge | Malody V 选曲桥改为自建 fork：分发物由上游 DLL 换成 `MMAMalodySelection.dll`（GUID `local.mma.malody.selection`），叠加界面整块移除，载荷 8→11 字段（判定档/Pro/Turbo），壳契约 v4→v5（页面接受 `[3,5]`），Malody V 接入动态 OD（判定档 × Pro × 倍率），缓存键最多 7 段，安装器不再写任何 cfg 且仅报告上游 DLL |
| [2026-09-26-local-settings-window.md](2026-09-26-local-settings-window.md) | 2026-09-26 | feat/local-settings-window | 本地设置窗口与离线权威链：权威链收敛为"在线 tosu 设置文件 / 离线本地 `mma-settings.json`"（原"离线读 tosu 文件"一级删除，离线骨架不继承 tosu 旧值），新增壳内第二窗口承载 `settings.html`（`Ctrl+Shift+S` / `--settings` / `POST /open-settings` 三入口）、`GET/POST /shell-config` 与离线读改写+广播、实例探测零窗口退出、设置窗口几何独立文件；契约版本仍 5、帧型与版本号均不变 |

[返回 docs 索引](../README.md)

---

# English

## Category Description

- This document is the guide and index for the `docs/breakings/` directory.
- This directory stores **major breaking-changes documents** (for both humans and AI), recording what changed and why, to make future code review and testing easier.
- Naming convention: **timestamp + change description** (e.g. `2026-08-09-perf-analysis-pipeline.md`), consistent with `docs/README.md:17` (Chinese) / `:59` (English) "named with a timestamp and the change description".
- Writing requirements:
  - Documents **must be bilingual** since both humans and AI read them (docs/README.md:17).
  - Each breaking change carries five elements: **What changed / Why (with perf evidence) / Scope / Compatibility / Verification**.
  - Verification is written as regression-comparison conclusions and measurement summaries (no local commands or evidence paths).
  - All content describes **actual landed code**, never "will be implemented in the future" speculation.
- When to add: when making major breaking changes to the pipeline / shared modules / cache semantics (docs/README.md:17); also update the corresponding pipeline and guide documents (docs/README.md:16) and register the entry in `docs/README.md` (docs/README.md:18).

## Document Index

| Document | Date | Branch/Topic | Description |
| --- | --- | --- | --- |
| [2026-08-09-perf-analysis-pipeline.md](2026-08-09-perf-analysis-pipeline.md) | 2026-08-09 | perf/analysis-pipeline-optimization | Analysis pipeline optimization: worker single-round-trip protocol, runAnalysisPipeline pure function, shared-module purity (worker root-cause fix), cache invalidation narrowing + hit re-derivation, vibro fix disclosure, perf acceptance-criteria revision (12 items) |
| [2026-08-16-preset-system-and-settings-schema.md](2026-08-16-preset-system-and-settings-schema.md) | 2026-08-16 | pr/45 (preset system) | Preset system (net changes vs main): new presets modules/presets.html/built-in presets, data-driven settings listener (SETTING_HANDLERS), settings.json preset group & buttons, presetStorage single source of truth, snapshot system-key stripping, idempotent write-back |
| [2026-08-30-marathon-correction-in-estimator.md](2026-08-30-marathon-correction-in-estimator.md) | 2026-08-30 | feat/marathon-correction (v2.0.2) | Marathon correction architecture refactor: pipeline patch stage → estimator-embedded (options.marathonCorrection), on-demand pre-Ett reuse, perf constraints honored (no rerun/no duplicate parse/WASM reuse), explicit two-tier benchmark semantics |
| [2026-08-30-multi-source-data-sources-and-desktop-shell.md](2026-08-30-multi-source-data-sources-and-desktop-shell.md) | 2026-08-30 | feat/multi-source-shell (v2.1.0) | Multi-source and desktop shell: new Etterna/Malody V external sources (converters + page `sources/` + shell + bridges), external-text entry in `fetchBeatmapFile` with the result frame merged in `finally`, cache identity/rate signature semantics, `gameClient` and other settings plus the osu suppression gate (buffered replay), telemetry `client` dimension, shell bridge contract, version 2 at the time |
| [2026-09-20-ett-ux-and-companella-capsule.md](2026-09-20-ett-ux-and-companella-capsule.md) | 2026-09-20 | fix/ett-ux-and-companella-capsule | Etterna error UX and capsule fixes: MinaCalc abort → `minacalc-aborted` plus module-cache recycling and an "Unsupported Chart" card; all-zero junk files → "MSD unavailable" with `--` instead of `0.00`; capsule updated after Companella fusion/adoption (`Companella` / `Azusa+Companella`); new `ettErrorCode`, `ettResult.junkFile`/`rowCount` |
| [2026-09-21-malody4-native-source.md](2026-09-21-malody4-native-source.md) | 2026-09-21 | feat/malody4-source | Malody 4.3.7 fourth data source (zero-injection read-only observation): anchor-md5 identity `mdy4:`, bridge contract version 2→3 with eight frame types (`malody4_selection`, `sources.malody4`, `requestId n{seq}`), the `malody4Root` resolution chain and the installer's third option (zero files copied), routing priority and the fourth dot colour, and the stale-page contract mismatch caused by the frozen plugin version 2.1.0 |
| [2026-09-21-malody4-dynamic-judge-od.md](2026-09-21-malody4-dynamic-judge-od.md) | 2026-09-21 | feat/malody4-source | Malody 4 dynamic judge OD: hard-coded OD 9 → judge level × rate equivalent OD (PC table `-4.56~16.42`), `modSignature` growing from 4 to 5 segments (old snapshots invalidate automatically), only the `malody4` source affected (osu/Etterna/Malody V byte-identical) |

| [2026-09-24-malody-v-selection-bridge-fork.md](2026-09-24-malody-v-selection-bridge-fork.md) | 2026-09-24 | feat/malodyv-bepinex-bridge | Malody V selection bridge moved to a self-built fork: the shipped DLL is now `MMAMalodySelection.dll` (GUID `local.mma.malody.selection`), the overlay layer is removed, the payload grows 8→11 fields (judge level / Pro / Turbo), the shell contract goes v4→v5 (page accepts `[3,5]`), Malody V gains dynamic OD (judge × Pro × rate), the cache key grows to at most 7 segments, and the installer writes no cfg and only reports the upstream DLL |
| [2026-09-26-local-settings-window.md](2026-09-26-local-settings-window.md) | 2026-09-26 | feat/local-settings-window | Local settings window and the offline authority chain: the chain collapses to "online tosu settings file / offline local `mma-settings.json`" (the old "read the tosu file offline" level is deleted and the offline skeleton does not inherit stale tosu values), a second shell window hosts `settings.html` (three entry points: `Ctrl+Shift+S` / `--settings` / `POST /open-settings`), new `GET/POST /shell-config` plus offline read-modify-write with broadcast, zero-window instance probe exit, and a separate geometry file for the settings window; contract version stays 5 and frame types and version numbers are unchanged |

[Back to docs index](../README.md)
