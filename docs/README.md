# docs/README.md
> English version [below](#english).

# 中文

## 概要与要求
- 本文档是docs/目录下的说明和索引文档。
- 本目录包含了项目功能/管线技术文档和指南文档（给AI），以及项目的说明文档和使用文档（给人类）。
- 对于给AI的文档，一般不考虑其语言，但是不应当使用除中文和英文之外的语言。
- 对于给人类的文档，使用中英双语。先使用中文，在全部结束之后下方使用英文。如果文档过长，则拆分到 `*_en.md` 和 `*.md` 两个文件中，前者为英文，后者为中文。
- 对于非技术文档，目标为普通用户，请勿使用过于专业的术语，尽量使用通俗易懂的语言，直白地描述功能。
- 对于技术文档，目标为开发者，请尽量使用专业术语，准确地描述功能和实现细节。
- 对于一个类别的文档，请放在子目录中，并在子目录中放置一个README.md文件，作为该类别的说明文档和索引文档。
- 当开发新功能时，请编写其对应的技术文档。该文档应当包含功能说明、使用方法、注意事项、算法说明等内容。
- 当修改现有功能时，请修改对应的技术文档，确保其内容与实际功能一致。
- 当对管线做出破坏性更改时，请修改对应的管线文档和指南文档。
- 当认为可以记录经验和教训时，请编写对应的知识与教训文档，记录在开发、调优与基准验证过程中积累的经验、失败记录与方法论。该文档主要给人类和AI阅读，因此需要双语。文档请按内容命名并放在 docs/learnings 目录下。
- 当做出重大破坏性更改时，请编写文档说明，标注修改内容和修改原因，以便后续进行代码审查和测试。该文档主要给人类和AI阅读，因此需要双语。文档请按时间戳和修改内容命名并放在 docs/breakings 目录下。
- 新增/删除文档需要在docs/README.md中添加/删除对应的索引条目。
- 正文段落不得手动换行（让编辑器软换行）

## 文档索引

请使用链接跳转到对应的文档。对于重名的文档，使用路径进行区分。

| 文档或路径 | 目标 | 说明 |
| --- | --- | --- |
| docs/README.md | 人类 | 本文档 |
| [settings.md](settings.md) | 人类 | 插件设置说明文档 |
| [shell-guide.md](shell-guide.md) | 人类 | 桌面壳使用教程（安装、窗口操作、桥接线、故障排查，中英双语） |
| [presets-guide.md](presets-guide.md) | 人类 | 预设系统新手教程（零基础双语教学，逐按钮讲解） |
| [azusa_algorithm.md](azusa_algorithm.md) | 人类/AI | Azusa算法说明文档(英文) |
| [roxy_algorithm.md](roxy_algorithm.md) | 人类/AI | Roxy算法说明文档(英文) |
| [features/README.md](features/README.md) | AI | 功能技术文档类别索引（难度估计、键型分析等） |
| [features/difficulty-estimation.md](features/difficulty-estimation.md) | AI | 难度估计功能文档（6 种估计算法、4/6/7K、LN/RC 段位） |
| [features/pattern-analysis.md](features/pattern-analysis.md) | AI | 键型分析功能文档（RC/LN 键型分布、SV 检测、vibro 检测） |
| [features/graph-visualization.md](features/graph-visualization.md) | AI | 难度图表可视化功能文档（难度变化图、已玩/未玩着色） |
| [features/pause-detection.md](features/pause-detection.md) | AI | 暂停检测功能文档（暂停次数检测、图表暂停位置显示） |
| [features/mode-tagging.md](features/mode-tagging.md) | AI | 模式标签功能文档（HB/RC/LN/Mix/SV 模式判定） |
| [features/rework-pp.md](features/rework-pp.md) | AI | ReworkPP 难度表现面板功能文档（5 行柱状图、v2Acc/PP 公式、Classic 感知星数、Max/Live 切换） |
| [features/marathon-correction.md](features/marathon-correction.md) | AI | 马拉松时长修正功能文档（Roxy/Azusa numeric 只降不升修正、均衡条件、taper、缓存/设置链路） |
| [features/telemetry.md](features/telemetry.md) | AI | 匿名使用统计（遥测）功能文档（事件契约、字段白名单、心跳/在线语义、隐私边界） |
| [features/multi-source.md](features/multi-source.md) | AI | 多数据源功能文档（Etterna/Malody 接入、转换器、路由决策表、败方门控、能力边界） |
| [features/desktop-shell.md](features/desktop-shell.md) | AI | 桌面壳功能技术文档（架构、目录检测、契约 v2、窗口操控、构建发布） |
| [features/presets.md](features/presets.md) | AI | 预设系统功能文档（自拓展 schema、presets.html 管理器、presetStorage、部分预设、导入导出） |
| [pipeline/README.md](pipeline/README.md) | AI | 管线技术文档类别索引（分析、缓存、设置、mod） |
| [pipeline/analysis-pipeline.md](pipeline/analysis-pipeline.md) | AI | 分析管线总览：tosu WebSocket -> 谱面获取 -> 解析 -> 估算 -> 显示 的完整数据流 |
| [pipeline/worker.md](pipeline/worker.md) | AI | Worker 与 runAnalysisPipeline 架构：worker 生命周期、消息协议、纯函数契约、共享解析、WASM-in-worker、stale 粒度 |
| [pipeline/result-cache.md](pipeline/result-cache.md) | AI | 结果缓存（LRU）机制：缓存键、命中覆盖检查、写入门槛与失效时机 |
| [pipeline/settings-pipeline.md](pipeline/settings-pipeline.md) | AI | 设置管线：settings.json -> 解析 -> 状态注入 -> 缓存失效与重算触发 |
| [pipeline/mod-handling.md](pipeline/mod-handling.md) | AI | mod 处理：mod 代码解析、倍速/OD 影响、modSignature 的构成与作用 |
| [pipeline/converters.md](pipeline/converters.md) | AI | 谱面转换器（sm/ssc/mc → osu）：时间轴烘焙语义、键数推导、测试与 golden 摘要 |
| [guides/README.md](guides/README.md) | AI | 指南文档类别索引（新增设置、缓存失效等） |
| [guides/adding-a-setting.md](guides/adding-a-setting.md) | AI | 新增设置项的完整流程指南（settings.json -> 解析器 -> state -> 缓存失效） |
| [guides/adding-to-worker.md](guides/adding-to-worker.md) | AI | 新增估算器/管线阶段到 worker 的完整流程指南（入口纯化 -> 注册 -> 消费接线 -> golden 扩展） |
| [guides/cache-invalidation.md](guides/cache-invalidation.md) | AI | 结果缓存失效机制说明与新增计算相关设置时的注意事项 |
| [guides/module-conventions.md](guides/module-conventions.md) | AI | 模块编写约定（共享模块约束、导入规范、兼容性要求） |
| [learnings/README.md](learnings/README.md) | AI | 知识与教训文档类别索引（难度估计算法调优经验、失败记录、方法论） |
| [learnings/difficulty-estimation.md](learnings/difficulty-estimation.md) | 人类/AI | 难度估计算法调优知识与教训（量化/序数校准/路由规则经验、历史探针结论、方法论） |
| [breakings/README.md](breakings/README.md) | 人类/AI | 重大破坏性更改说明类别索引（时间戳+内容命名，双语五要素） |
| [breakings/2026-08-30-marathon-correction-in-estimator.md](breakings/2026-08-30-marathon-correction-in-estimator.md) | 人类/AI | 马拉松时长修正架构重构破坏性说明（管线派生段 → 估算器内嵌、按需前置 Ett 复用、perf 约束遵守、基准双口径） |
| [breakings/2026-08-30-multi-source-data-sources-and-desktop-shell.md](breakings/2026-08-30-multi-source-data-sources-and-desktop-shell.md) | 人类/AI | 多数据源与桌面壳破坏性说明（外部文本入口、缓存身份/速率签名、设置管线、遥测 client 维度、fetch 语义） |

# English

## Summary & Requirements
- This document is the guide and index for the docs/ directory.
- The directory contains feature/pipeline technical documents and guide documents (for AI), as well as project description documents and usage documents (for humans).
- For AI documents, the language does not matter in general, but no language other than Chinese or English should be used.
- For human documents, use both Chinese and English. Use Chinese first, followed by the English translation. If a document is too long, split it into `*_en.md` and `*.md` files, the former in English and the latter in Chinese.
- For non-technical documents, the target is ordinary users; avoid overly professional terminology, use plain and easy-to-understand language, and describe features directly.
- For technical documents, the target is developers; use professional terminology and describe features and implementation details accurately.
- Documents of one category should be placed in a subdirectory, with a README.md inside that subdirectory serving as the category's guide and index document.
- When developing a new feature, write its corresponding technical document. The document should include feature description, usage, notes, algorithm description, and so on.
- When modifying an existing feature, update the corresponding technical document to keep it consistent with the actual feature.
- When making breaking changes to the pipeline, update the corresponding pipeline documents and guide documents.
- When you think it is appropriate to record experience and lessons learned, write a corresponding knowledge & lessons-learned document, recording the experience, failed attempts, and methodology accumulated during development, tuning, and benchmarking. The document is mainly for humans and AI to read, so it needs to be bilingual. Name the document according to its content and place it in the docs/learnings directory.
- When making major breaking changes, write a document explaining the changes, marking what was changed and why, to facilitate future code review and testing. The document should be named with a timestamp and the change description, and placed in the docs/breakings directory.
- When adding or removing documents, add or remove the corresponding index entries in docs/README.md.
- Do not manually insert line breaks in the main text paragraphs (let the editor soft wrap).

## Document Index

Use the links to jump to the corresponding document. For documents with the same name, use the path to distinguish them.

| Document or Path | Target | Description |
| --- | --- | --- |
| docs/README.md | Human | This document |
| [settings.md](settings.md) | Human | Plugin settings guide |
| [shell-guide.md](shell-guide.md) | Human | Desktop shell tutorial (install, window controls, bridges, troubleshooting; bilingual) |
| [presets-guide.md](presets-guide.md) | Human | Presets beginner guide (bilingual tutorial, every button explained) |
| [azusa_algorithm.md](azusa_algorithm.md) | Human/AI | Azusa algorithm document (English) |
| [roxy_algorithm.md](roxy_algorithm.md) | Human/AI | Roxy algorithm document (English) |
| [features/README.md](features/README.md) | AI | Index of feature technical documents (difficulty estimation, pattern analysis, etc.) |
| [features/difficulty-estimation.md](features/difficulty-estimation.md) | AI | Difficulty estimation document (6 algorithms, 4/6/7K, LN/RC dan tiers) |
| [features/pattern-analysis.md](features/pattern-analysis.md) | AI | Pattern analysis document (RC/LN pattern distribution, SV detection, vibro detection) |
| [features/graph-visualization.md](features/graph-visualization.md) | AI | Difficulty graph visualization document (difficulty graph, played/unplayed coloring) |
| [features/pause-detection.md](features/pause-detection.md) | AI | Pause detection document (pause count detection, pause position display on graph) |
| [features/mode-tagging.md](features/mode-tagging.md) | AI | Mode tagging document (HB/RC/LN/Mix/SV mode judgment) |
| [features/rework-pp.md](features/rework-pp.md) | AI | ReworkPP performance panel document (5-row bar chart, v2Acc/PP formulas, Classic-aware star rating, Max/Live switching) |
| [features/marathon-correction.md](features/marathon-correction.md) | AI | Marathon duration correction document (Roxy/Azusa numeric lower-only correction, balance gate, taper, cache/settings wiring) |
| [features/telemetry.md](features/telemetry.md) | AI | Anonymous usage statistics (telemetry) document (event contract, field whitelist, heartbeat/online semantics, privacy boundaries) |
| [features/multi-source.md](features/multi-source.md) | AI | Multi-source document (Etterna/Malody integration, converters, routing decision table, osu gate, capability boundaries) |
| [features/desktop-shell.md](features/desktop-shell.md) | AI | Desktop shell technical document (architecture, directory detection, contract v2, window controls, build & release) |
| [features/presets.md](features/presets.md) | AI | Preset system document (self-extending schema, presets.html manager, presetStorage, partial presets, export/import) |
| [pipeline/README.md](pipeline/README.md) | AI | Index of pipeline technical documents (analysis, cache, settings, mods) |
| [pipeline/analysis-pipeline.md](pipeline/analysis-pipeline.md) | AI | Analysis pipeline overview: tosu WebSocket -> beatmap fetch -> parse -> estimate -> display |
| [pipeline/worker.md](pipeline/worker.md) | AI | Worker and runAnalysisPipeline architecture: worker lifecycle, message protocol, pure-function contract, shared parsing, WASM-in-worker, stale granularity |
| [pipeline/result-cache.md](pipeline/result-cache.md) | AI | Result cache (LRU) mechanism: cache key, hit coverage check, write gate, invalidation timing |
| [pipeline/settings-pipeline.md](pipeline/settings-pipeline.md) | AI | Settings pipeline: settings.json -> parse -> state injection -> cache invalidation and recompute trigger |
| [pipeline/mod-handling.md](pipeline/mod-handling.md) | AI | Mod handling: mod code parsing, speed/OD effects, modSignature composition and purpose |
| [pipeline/converters.md](pipeline/converters.md) | AI | Chart converters (sm/ssc/mc → osu): timing baking semantics, key-count derivation, tests and golden summary |
| [guides/README.md](guides/README.md) | AI | Index of guide documents (adding a setting, cache invalidation, etc.) |
| [guides/adding-a-setting.md](guides/adding-a-setting.md) | AI | Complete guide for adding a setting (settings.json -> parser -> state -> cache invalidation) |
| [guides/adding-to-worker.md](guides/adding-to-worker.md) | AI | Complete guide for adding an estimator/pipeline stage to the worker (entry purification -> registration -> consumer wiring -> golden extension) |
| [guides/cache-invalidation.md](guides/cache-invalidation.md) | AI | Result cache invalidation notes and cautions when adding computation-affecting settings |
| [guides/module-conventions.md](guides/module-conventions.md) | AI | Module writing conventions (shared module constraints, import rules, compatibility requirements) |
| [learnings/README.md](learnings/README.md) | AI | Index of knowledge & lessons-learned documents (difficulty estimation tuning experience, failed attempts, methodology) |
| [learnings/difficulty-estimation.md](learnings/difficulty-estimation.md) | Human/AI | Difficulty estimation tuning knowledge & lessons (quantization/ordinal-calibration/routing-rule experiences, historical probe conclusions, methodology) |
| [breakings/README.md](breakings/README.md) | Human/AI | Index of major breaking-changes documents (timestamp+description naming, bilingual five elements) |
| [breakings/2026-08-30-marathon-correction-in-estimator.md](breakings/2026-08-30-marathon-correction-in-estimator.md) | Human/AI | Marathon correction architecture refactor breaking note (pipeline patch → estimator-embedded, on-demand pre-Ett reuse, perf constraints honored, two-tier benchmark semantics) |
| [breakings/2026-08-30-multi-source-data-sources-and-desktop-shell.md](breakings/2026-08-30-multi-source-data-sources-and-desktop-shell.md) | Human/AI | Multi-source & desktop shell breaking note (external text entry, cache identity/rate signature, settings pipeline, telemetry client dim, fetch semantics) |
