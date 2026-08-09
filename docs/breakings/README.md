# docs/breakings/README.md

## 类别说明（中文）

- 本文档是 `docs/breakings/` 目录的说明和索引文档。
- 本目录存放**重大破坏性更改说明文档**（给人类和 AI 共同阅读），记录修改内容与修改原因，便于后续代码审查和测试。
- 命名约定：**时间戳 + 修改内容**（如 `2026-08-09-perf-analysis-pipeline.md`），与 `docs/README.md:17`（中文）/ `:59`（English）的"按时间戳和修改内容命名"约定一致。
- 写作要求：
  - 文档**必须双语**（每节中英并列），人类与 AI 共同阅读（docs/README.md:17）。
  - 每项破坏性更改含五要素：**修改内容（What changed）/ 修改原因（Why，关联性能依据）/ 影响范围（Scope）/ 兼容策略（Compat）/ 验证方式（Verification）**。
  - 验证方式指向 `test/compare-golden.mjs` 命令与 `.omo/evidence/` 证据路径。
  - 所有内容描述**实际落地代码**，不写"将要在未来"的推测。
- 何时新增：对管线/共享模块/缓存语义做出重大破坏性更改时（docs/README.md:17）；同时更新对应的管线文档和指南文档（docs/README.md:16），并在 `docs/README.md` 登记索引（docs/README.md:18）。

## 文档索引

| 文档 | 日期 | 分支/主题 | 说明 |
| --- | --- | --- | --- |
| [2026-08-09-perf-analysis-pipeline.md](2026-08-09-perf-analysis-pipeline.md) | 2026-08-09 | perf/analysis-pipeline-optimization | 分析管线性能优化：worker 单次往返协议、runAnalysisPipeline 纯函数、共享模块纯度（worker 根因修复）、缓存失效收窄与命中重派生、vibro 修复披露、perf 验收口径修订（12 项） |

[返回 docs 索引](../README.md)

---

# English

## Category Description

- This document is the guide and index for the `docs/breakings/` directory.
- This directory stores **major breaking-changes documents** (for both humans and AI), recording what changed and why, to make future code review and testing easier.
- Naming convention: **timestamp + change description** (e.g. `2026-08-09-perf-analysis-pipeline.md`), consistent with `docs/README.md:17` (Chinese) / `:59` (English) "named with a timestamp and the change description".
- Writing requirements:
  - Documents **must be bilingual** (Chinese and English per section) since both humans and AI read them (docs/README.md:17).
  - Each breaking change carries five elements: **What changed / Why (with perf evidence) / Scope / Compatibility / Verification**.
  - Verification points to the `test/compare-golden.mjs` commands and `.omo/evidence/` evidence paths.
  - All content describes **actual landed code**, never "will be implemented in the future" speculation.
- When to add: when making major breaking changes to the pipeline / shared modules / cache semantics (docs/README.md:17); also update the corresponding pipeline and guide documents (docs/README.md:16) and register the entry in `docs/README.md` (docs/README.md:18).

## Document Index

| Document | Date | Branch/Topic | Description |
| --- | --- | --- | --- |
| [2026-08-09-perf-analysis-pipeline.md](2026-08-09-perf-analysis-pipeline.md) | 2026-08-09 | perf/analysis-pipeline-optimization | Analysis pipeline optimization: worker single-round-trip protocol, runAnalysisPipeline pure function, shared-module purity (worker root-cause fix), cache invalidation narrowing + hit re-derivation, vibro fix disclosure, perf acceptance-criteria revision (12 items) |

[Back to docs index](../README.md)
