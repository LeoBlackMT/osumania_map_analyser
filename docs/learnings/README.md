# docs/learnings/README.md

## 类别说明

- 本文档是 `docs/learnings/` 目录下的说明和索引文档。
- 本目录存放**知识与教训文档**（给 AI 和人类），记录在开发、调优与基准验证过程中积累的经验、失败记录与方法论。
- 这些文档与 `docs/features/`（功能技术文档）的区别：features 描述「系统当前如何工作」，learnings 描述「我们尝试过什么、什么有效、什么无效、为什么」——面向后续继续改进算法的 LLM 与开发者，避免重复踩坑。
- 撰写时以「结论 + 证据 + 原因」为导向，引用相关代码位置或数据作为依据。
- 新增/删除 learnings 文档需要在本文档中添加/删除对应的索引条目，并在 `docs/README.md` 中同步更新。

## 文档索引

请使用链接跳转到对应的文档。

| 文档或路径 | 目标 | 说明 |
| --- | --- | --- |
| [difficulty-estimation.md](difficulty-estimation.md) | 人类/AI | 难度估计算法调优知识与教训（Roxy/Azusa/Mixed 实验史、量化/序数校准/路由规则的经验与陷阱） |

[返回 docs 索引](../README.md)

# English

## Category Description

- This document is the guide and index for the `docs/learnings/` directory.
- This directory stores knowledge & lessons-learned documents (for AI and humans), recording experience, failed attempts and methodology accumulated while developing, tuning and benchmarking.
- Difference from `docs/features/` (feature technical docs): features describe "how the system currently works", learnings describe "what we tried, what worked, what failed and why" — for future LLMs and developers who continue improving the algorithms, to avoid repeating mistakes.
- Write with a "conclusion + evidence + reason" orientation, citing relevant code locations or data.
- When adding or removing learnings documents, update the index entries in this document and sync the updates in `docs/README.md`.

## Document Index

Use the links to jump to the corresponding document.

| Document or Path | Target | Description |
| --- | --- | --- |
| [difficulty-estimation.md](difficulty-estimation.md) | Human/AI | Difficulty estimation tuning knowledge & lessons (Roxy/Azusa/Mixed experiment history, quantization/ordinal-calibration/routing-rule experiences and pitfalls) |

[Back to docs index](../README.md)
