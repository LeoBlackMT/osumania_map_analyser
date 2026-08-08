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
| [cache-invalidation.md](cache-invalidation.md) | AI | 结果缓存失效机制说明与新增计算相关设置时的注意事项 |
| [module-conventions.md](module-conventions.md) | AI | 模块编写约定（共享模块约束、导入规范、兼容性要求） |
| [benchmark-guide.md](benchmark-guide.md) | AI | Benchmark 使用指南（算法验证流程与注意事项） |

[返回 docs 索引](../README.md)
