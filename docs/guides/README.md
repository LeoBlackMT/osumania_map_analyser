# docs/guides/README.md

## 类别说明

- 本文档是 `docs/guides/` 目录下的说明和索引文档。
- 本目录存放**指南文档**（给 AI），用于指导 LLM 在新增功能、修改管线或维护本项目时遵循的规范与流程。
- 指南文档面向 AI 阅读，语言可使用中文或英文，但不得使用其他语言。
- 撰写指南文档时，应以"如何操作"为导向，给出具体的步骤、约束和注意事项，并引用相关源代码位置作为依据。
- 当对管线做出破坏性更改时，请同时修改对应的管线文档和本目录下的指南文档。
- 新增/删除指南文档需要在本文档中添加/删除对应的索引条目，并在 `docs/README.md` 中同步更新。

## 文档索引

请使用链接跳转到对应的文档。

| 文档或路径 | 目标 | 说明 |
| --- | --- | --- |
| [adding-a-setting.md](adding-a-setting.md) | AI | 新增设置项的完整流程指南（settings.json → 解析器 → state → 缓存失效） |
| [adding-to-worker.md](adding-to-worker.md) | AI | 新增估算器/管线阶段到 worker 的完整流程指南（入口纯化 → 注册 → 消费接线 → golden 扩展） |
| [cache-invalidation.md](cache-invalidation.md) | AI | 结果缓存失效机制说明与新增计算相关设置时的注意事项 |
| [module-conventions.md](module-conventions.md) | AI | 模块编写约定（共享模块约束、导入规范、兼容性要求） |

[返回 docs 索引](../README.md)

# English

## Category Description

- This document is the guide and index for the `docs/guides/` directory.
- This directory stores guide documents (for AI), used to guide LLMs on the conventions and workflows to follow when adding features, modifying the pipeline, or maintaining this project.
- Guide documents target AI readers; Chinese or English may be used, but no other language.
- When writing guide documents, orient them around "how to operate", giving concrete steps, constraints, and notes, and cite relevant source code locations as the basis.
- When making breaking changes to the pipeline, also update the corresponding pipeline documents and the guide documents in this directory.
- When adding or removing guide documents, add or remove the corresponding index entries in this document and sync the updates in `docs/README.md`.

## Document Index

Use the links to jump to the corresponding document.

| Document or Path | Target | Description |
| --- | --- | --- |
| [adding-a-setting.md](adding-a-setting.md) | AI | Complete guide for adding a setting (settings.json -> parser -> state -> cache invalidation) |
| [adding-to-worker.md](adding-to-worker.md) | AI | Complete guide for adding an estimator/pipeline stage to the worker (entry purification -> registration -> consumer wiring -> golden extension) |
| [cache-invalidation.md](cache-invalidation.md) | AI | Result cache invalidation notes and cautions when adding computation-affecting settings |
| [module-conventions.md](module-conventions.md) | AI | Module writing conventions (shared module constraints, import rules, compatibility requirements) |

[Back to docs index](../README.md)
