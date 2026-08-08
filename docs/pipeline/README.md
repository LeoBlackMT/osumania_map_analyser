# docs/pipeline/README.md

## 类别说明

本文档是 `docs/pipeline/` 目录的说明和索引文档。

本目录包含插件内部数据管线的技术文档，面向 AI 阅读，用于理解数据流的各个阶段（谱面获取 → 解析 → 分析 → 结果缓存 → 显示），以及在修改管线时保持文档与实际功能一致。

文档索引：

| 文档或路径 | 目标 | 说明 |
| --- | --- | --- |
| [analysis-pipeline.md](analysis-pipeline.md) | AI | 插件分析管线总览：tosu WebSocket → 谱面获取 → 解析 → 估算 → 显示 的完整数据流 |
| [result-cache.md](result-cache.md) | AI | 结果缓存（LRU）机制：缓存键、命中覆盖检查、写入门槛与失效时机 |
| [settings-pipeline.md](settings-pipeline.md) | AI | 设置管线：settings.json → 解析 → 状态注入 → 缓存失效与重算触发 |
| [mod-handling.md](mod-handling.md) | AI | mod 处理：mod 代码解析、倍速/OD 影响、modSignature 的构成与作用 |

[返回 docs 索引](../README.md)

# English

## Category Description

This document is the guide and index for the `docs/pipeline/` directory.

This directory contains technical documents for the plugin's internal data pipeline, intended for AI readers, to understand each stage of the data flow (beatmap fetch -> parse -> analyze -> result cache -> display), and to keep the documents consistent with the actual functionality when modifying the pipeline.

## Document Index

| Document or Path | Target | Description |
| --- | --- | --- |
| [analysis-pipeline.md](analysis-pipeline.md) | AI | Analysis pipeline overview: tosu WebSocket -> beatmap fetch -> parse -> estimate -> display |
| [result-cache.md](result-cache.md) | AI | Result cache (LRU) mechanism: cache key, hit coverage check, write gate, invalidation timing |
| [settings-pipeline.md](settings-pipeline.md) | AI | Settings pipeline: settings.json -> parse -> state injection -> cache invalidation and recompute trigger |
| [mod-handling.md](mod-handling.md) | AI | Mod handling: mod code parsing, speed/OD effects, modSignature composition and purpose |

[Back to docs index](../README.md)
