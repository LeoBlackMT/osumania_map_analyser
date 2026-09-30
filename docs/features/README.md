# docs/features/README.md

## 概要与要求
- 本文档是 docs/features/ 目录下的说明和索引文档。
- 本目录存放插件的**功能技术文档**，目标为 AI（LLM），用于描述各功能模块的实现细节、算法说明、注意事项等。
- 文档面向 AI，不限制语言，但仅使用中文或英文；本目录默认使用中文。
- 当开发新功能时，请在本目录编写对应的功能技术文档；当修改现有功能时，请同步修改对应文档，确保内容与实际功能一致。
- 新增/删除文档时，请在下方索引表中添加/删除对应的条目。

## 文档索引

| 文档或路径 | 目标 | 说明 |
| --- | --- | --- |
| [difficulty-estimation.md](difficulty-estimation.md) | AI | 难度估计功能文档（6 种估计算法、4/6/7K、LN/RC 段位） |
| [mixed-routing.md](mixed-routing.md) | AI | Mixed 路由技术文档（逐条判定顺序：RC 树 R1–R7、LN/Mix 树 L1–L5、Companella 阶段 C1–C10；含精确阈值与常量、失败回退链、标签与算法胶囊取值、常见误解速查） |
| [pattern-analysis.md](pattern-analysis.md) | AI | 键型分析功能文档（RC/LN 键型分布、SV 检测、vibro 检测） |
| [graph-visualization.md](graph-visualization.md) | AI | 难度图表可视化功能文档（难度变化图、已玩/未玩着色） |
| [pause-detection.md](pause-detection.md) | AI | 暂停检测功能文档（暂停次数检测、图表暂停位置显示） |
| [mode-tagging.md](mode-tagging.md) | AI | 模式标签功能文档（HB/RC/LN/Mix/SV 模式判定） |
| [rework-pp.md](rework-pp.md) | AI | ReworkPP 难度表现面板功能文档（5 行柱状图、v2Acc/PP 公式、Classic 感知星数、Max/Live 切换） |
| [marathon-correction.md](marathon-correction.md) | AI | 马拉松时长修正功能文档（Roxy/Azusa numeric 只降不升修正、均衡条件、taper、缓存/设置链路） |
| [telemetry.md](telemetry.md) | AI | 匿名使用统计（遥测）功能文档（事件契约、字段白名单、心跳/在线语义、隐私边界） |
| [multi-source.md](multi-source.md) | AI | 多数据源功能文档（Etterna、Malody V、Malody 4 接入、转换器、路由决策表、败方门控、能力边界；含 osu 传输层：原生源 + tosu 兜底 + 浏览器模式不变式） |
| [desktop-shell.md](desktop-shell.md) | AI | 桌面壳功能技术文档（架构、目录检测、**契约 v6**、24062 osu 兼容端点与端点下发、Malody V 选曲桥端点 17653、窗口操控、构建发布；人类教程见 docs/shell-guide.md） |
| [osu-native-source.md](osu-native-source.md) | AI | osu! 原生源功能文档（壳内只读内存层 stable 签名扫描 / lazer 偏移表、24062 tosu 兼容子集 origin 与回放开关、契约 v6 `sources.osu` 端点下发与页面 socket 切流、能力边界、reason 闭集 + `degradedFields` + 冻结/保持/停帧语义、L0–L3 健康机与门常数、**锚点失效定位**与**lazer 偏移表重建**两手册、自检清单） |
| [malody4-source.md](malody4-source.md) | AI | Malody 4.3.7 原生客户端数据源功能文档（零注入只读观察三条信号、版本门、谱面库索引与 `mdy4:` 身份、`malody4_selection` 帧与 `reason` 闭集、根目录解析链、路由与已知限制） |
| [malody-od.md](malody-od.md) | 人类/AI | 判定档 → 等效 osu!mania OD：Malody 4.3.7 的 PC 表 20 格与逐格 σ\*（96% 等精度方法学、窗口值来源、上界 21.3、FAIR 局限）**以及 Malody V 的表**（判定档 × Pro × 倍率、Turbo 补偿、复算工装） |
| [presets.md](presets.md) | AI | 预设系统功能文档（自拓展 schema、presets.html 管理器、presetStorage、部分预设、导入导出） |

[返回 docs 索引](../README.md)

# English

## Summary & Requirements
- This document is the guide and index for the docs/features/ directory.
- This directory stores the plugin's feature technical documents, targeting AI (LLM), used to describe the implementation details, algorithm explanations, notes, and more for each feature module.
- The documents target AI and do not restrict language, but only Chinese or English should be used; this directory defaults to Chinese.
- When developing a new feature, write the corresponding feature technical document in this directory; when modifying an existing feature, update the corresponding document to keep it consistent with the actual feature.
- When adding or removing documents, add or remove the corresponding entries in the index table below.

## Document Index

| Document or Path | Target | Description |
| --- | --- | --- |
| [difficulty-estimation.md](difficulty-estimation.md) | AI | Difficulty estimation document (6 algorithms, 4/6/7K, LN/RC dan tiers) |
| [mixed-routing.md](mixed-routing.md) | AI | Mixed routing document (step-by-step decision order: RC tree R1-R7, LN/Mix tree L1-L5, Companella stage C1-C10, with exact thresholds and constants, fallback chains, label and capsule values, myth checklist) |
| [pattern-analysis.md](pattern-analysis.md) | AI | Pattern analysis document (RC/LN pattern distribution, SV detection, vibro detection) |
| [graph-visualization.md](graph-visualization.md) | AI | Difficulty graph visualization document (difficulty graph, played/unplayed coloring) |
| [pause-detection.md](pause-detection.md) | AI | Pause detection document (pause count detection, pause position display on graph) |
| [mode-tagging.md](mode-tagging.md) | AI | Mode tagging document (HB/RC/LN/Mix/SV mode judgment) |
| [rework-pp.md](rework-pp.md) | AI | ReworkPP performance panel document (5-row bar chart, v2Acc/PP formulas, Classic-aware star rating, Max/Live switching) |
| [marathon-correction.md](marathon-correction.md) | AI | Marathon duration correction document (Roxy/Azusa numeric lower-only correction, balance gate, taper, cache/settings wiring) |
| [telemetry.md](telemetry.md) | AI | Anonymous usage statistics (telemetry) document (event contract, field whitelist, heartbeat/online semantics, privacy boundaries) |
| [multi-source.md](multi-source.md) | AI | Multi-source document (Etterna, Malody V and Malody 4 integration, converters, routing decision table, osu gate, capability boundaries; includes the osu transport layer: native source + tosu fallback + the browser-mode invariant) |
| [desktop-shell.md](desktop-shell.md) | AI | Desktop shell technical document (architecture, directory detection, contract v6, the 24062 osu-compatible origin and endpoint delivery, the Malody V selection-bridge endpoint 17653, window controls, build & release; human tutorial: docs/shell-guide.md) |
| [osu-native-source.md](osu-native-source.md) | AI | osu! native source document (in-shell read-only memory layer: stable signature scan vs the lazer offset table, the 24062 tosu-compatible subset origin and its replay switch, contract v6 `sources.osu` endpoint delivery plus the page's socket-layer switch, capability boundaries, the `reason` closed set with `degradedFields` and freeze/hold/stop semantics, the L0–L3 health machine and gate constants, two manuals (locating an anchor failure / regenerating the lazer offset table) and the self-check list) |
| [malody4-source.md](malody4-source.md) | AI | Malody 4.3.7 native client data source document (zero-injection read-only observation, version gate, chart library index and `mdy4:` identity, `malody4_selection` frame and the `reason` closed set, root resolution chain, routing and known limitations) |
| [malody-od.md](malody-od.md) | Human/AI | Judge level → equivalent osu!mania OD for **both clients**: the Malody 4.3.7 PC table (20 cells with per-cell σ*, the 96%-accuracy equal-precision method, window-value provenance, why the bound is 21.3, the FAIR caveat) **and the Malody V table** (judge × Pro × rate, Turbo compensation, recompute tool) |
| [presets.md](presets.md) | AI | Preset system document (self-extending schema, presets.html manager, presetStorage, partial presets, export/import) |

[Back to docs index](../README.md)
