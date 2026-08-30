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
| [pattern-analysis.md](pattern-analysis.md) | AI | 键型分析功能文档（RC/LN 键型分布、SV 检测、vibro 检测） |
| [graph-visualization.md](graph-visualization.md) | AI | 难度图表可视化功能文档（难度变化图、已玩/未玩着色） |
| [pause-detection.md](pause-detection.md) | AI | 暂停检测功能文档（暂停次数检测、图表暂停位置显示） |
| [mode-tagging.md](mode-tagging.md) | AI | 模式标签功能文档（HB/RC/LN/Mix/SV 模式判定） |
| [rework-pp.md](rework-pp.md) | AI | ReworkPP 难度表现面板功能文档（5 行柱状图、v2Acc/PP 公式、Classic 感知星数、Max/Live 切换） |
| [marathon-correction.md](marathon-correction.md) | AI | 马拉松时长修正功能文档（Roxy/Azusa numeric 只降不升修正、均衡条件、taper、缓存/设置链路） |
| [telemetry.md](telemetry.md) | AI | 匿名使用统计（遥测）功能文档（事件契约、字段白名单、心跳/在线语义、隐私边界） |
| [multi-source.md](multi-source.md) | AI | 多数据源功能文档（Etterna/Malody 接入、转换器、路由决策表、败方门控、能力边界） |
| [desktop-shell.md](desktop-shell.md) | 人类/AI | 桌面壳功能与安装文档（在线/离线模式、桥安装、平台限制、构建发布） |

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
| [pattern-analysis.md](pattern-analysis.md) | AI | Pattern analysis document (RC/LN pattern distribution, SV detection, vibro detection) |
| [graph-visualization.md](graph-visualization.md) | AI | Difficulty graph visualization document (difficulty graph, played/unplayed coloring) |
| [pause-detection.md](pause-detection.md) | AI | Pause detection document (pause count detection, pause position display on graph) |
| [mode-tagging.md](mode-tagging.md) | AI | Mode tagging document (HB/RC/LN/Mix/SV mode judgment) |
| [rework-pp.md](rework-pp.md) | AI | ReworkPP performance panel document (5-row bar chart, v2Acc/PP formulas, Classic-aware star rating, Max/Live switching) |
| [marathon-correction.md](marathon-correction.md) | AI | Marathon duration correction document (Roxy/Azusa numeric lower-only correction, balance gate, taper, cache/settings wiring) |
| [telemetry.md](telemetry.md) | AI | Anonymous usage statistics (telemetry) document (event contract, field whitelist, heartbeat/online semantics, privacy boundaries) |
| [multi-source.md](multi-source.md) | AI | Multi-source document (Etterna/Malody integration, converters, routing decision table, osu gate, capability boundaries) |
| [desktop-shell.md](desktop-shell.md) | Human/AI | Desktop shell feature & install document (online/offline modes, bridge installation, platform limits, build & release) |

[Back to docs index](../README.md)
