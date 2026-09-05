# CLAUDE.md
> 本文是写给 LLM 的项目说明和编写要求，LLM 在编写代码时请务必遵守本文档的要求。
> This document is written for LLMs, and LLMs must follow the requirements of this document when writing code.
> 注意：AGENTS.md和CLAUDE.md如果出现冲突，请以CLAUDE.md为准。除非用户明确要求。如果遇到不确定的情况，请向用户提问。

## 项目介绍
- 本项目仓库地址 https://github.com/LeoBlackMT/osumania_map_analyser。
- 本仓库是一个运行在内存Hook工具 [tosu](https://github.com/tosuapp/tosu) 环境下的游戏内叠加界面(ppcounter, 下称插件)，实时在音乐游戏 osu!mania（4/6/7K/Lazer/Stable）及其各种mod下，提供估算难度、分析RC/LN键型、自定义ett版本计算MSD、难度图表和暂停检测功能。详见 [README.md](README.md)。
- 本仓库还有附属项目：
    - desktop/，桌面壳子项目（Tauri v2 + Rust），用于在桌面环境下运行插件。详见 [desktop/README.md](desktop/README.md)。
    - bridges/, 游戏桥接项目，包含 Etterna/Malody 的桥接代码。详见 [bridges/README.md](bridges/README.md)。
    - backend/，后端项目（Go + SQLite），用于收集匿名使用统计（遥测）数据。详见 [backend/README.md](backend/README.md)。
    - tools/, 工具项目，包含一些辅助工具和脚本。详见 [tools/README.md](tools/README.md)。

## 项目结构
- docs/: 项目功能说明和指南。
    - docs/README.md: 目录说明和索引文档。
- img/: 为README.md提供图片资源。
- backup/(本地私有): 用于存放本地备份的文件夹，仅在本地使用，远程仓库不存在。
- temp/(本地私有): 用于存放本地临时文件的文件夹，仅在本地使用，远程仓库不存在。
- ManiaMapAnalyser by Leo_Black/: 插件的源代码文件夹，包含插件的所有源代码文件。其路径携带空格，为tosu的命名要求。请在使用时注意。
    - js/app/: 插件核心功能的js目录，包含插件的主要功能实现。
    - js/app/worker/: 插件的worker目录，包含插件的worker线程实现。主要用于分离UI和计算逻辑，避免UI卡顿。
    - js/debug/: 插件的调试目录，包含插件的调试工具和调试代码。应当保留。
    - js/estimator/: 难度估计模块的核心目录，包含难度估计算法的实现。
        - js/estimator/companella/: Companella难度估计算法的ONNX模型及其WASM运行时。
        - js/estimator/intervals/: Sunny 难度估计算法的星数映射表。
    - js/ett/: 插件集成的 Etterna MinaCalc 目录，包含Etterna MinaCalc的五个版本WASM运行时。
    - js/interlude/: 插件集成的 Interlude 星数计算模块。
    - js/parser/: 插件的解析模块，包含谱面解析器、键型解析器和设置解析器。
    - js/patterns/: 插件的键型分析模块，包含RC/LN键型分析器。其中RC键型分析器使用了Interlude的算法，并在其基础上新增了LN检测算法。
    - js/rework/: 插件集成的星数重算算法模块，主要包含 Sunny Rework 的算法实现。
    - styles/: 插件的样式目录，包含插件的CSS样式文件。
    - styles/fonts/: 插件的字体目录，包含插件的字体文件。
    - config.js: 插件内部的配置文件，包含一些全局定义和常数。注意其options部分为tosu侧配置文件的枚举。
    - debug.html: 插件的调试页面，主要用于调试插件的估计算法和tosu api。
    - index.html: 插件的主页面，显示插件的核心功能。
    - index.js: 插件的主入口文件，负责初始化插件和注册插件的功能。包含版本号（内部）。
    - metadata.txt: 用于 tosu 读取的插件元信息文件，包含插件的名称、版本（外部）、作者和描述等信息。
    - settings.json: 暴露给 tosu 的插件设置定义文件，包含插件的设置项和默认值。但是，这并不是设置文件。用户可以通过 tosu 的设置界面修改这里定义的内容，从而改变插件的行为。实际的设置文件位于 tosu 的 `settings` 目录下，文件名为 `<插件目录名>.json`
- 附属项目见上方项目介绍。

## 要求限制
- 在进行操作之前，请先阅读下方[行为准则](#行为准则)并遵守。
- 在对代码进行破坏性修改、对未被git跟踪的文件进行修改、对已有功能进行大幅度改动之前，请务必先征求用户意见，确保用户理解你的修改意图。随后，在 backup 目录下创建备份文件夹，并将你要修改的文件复制到备份文件夹中，以便在出现问题时可以快速恢复。请使用时间戳和修改内容命名目录。
- 在生成测试代码/保存临时文件/生成调试文件时，请将其放置在 temp 目录下，并确保不会被提交到远程仓库。随后在实现功能后，请删除 temp 目录下本次相关的文件，以避免占用磁盘空间。测试脚本等临时文件建议等用户完全验收后再删除，避免验收时无法复用。测试脚本一律不得提交到仓库。
- desktop/ 为可选桌面壳子项目（Tauri v2 + Rust）：改动涉及 `desktop/**` 时遵循其自身构建（cargo build）与契约（desktop/docs/CONTRACT.md，其版本号须与页面 `js/app/sources/bridgeClient.js` 的 CONTRACT_VERSION 一致）；壳构建 CI 仅监听 main 分支的 desktop/** 变动。
- 请按照用户的实际情况进行git操作，默认允许 commit，但是不允许 push。请在进行 push 之前征求用户意见。严禁直接 push 到 main 分支，除非用户明确要求。请在进行 push 之前确保代码已经过测试，并且不会破坏已有功能。请使用Pull Request的方式进行贡献，以便后续进行代码审查和测试。
- 文档编写的规范和要求详见 [docs/README.md](docs/README.md)，请务必遵守。在新增功能/修改功能/修改管线时，请务必修改对应的文档，确保文档内容与实际功能一致。在进行重大破坏性修改时，请务必编写文档并标注修改内容和修改原因，以便后续进行代码审查和测试。
- 插件的谱面数据源共三类：osu!（经 tosu）、Etterna 与 Malody V（经桌面壳 desktop/ 与游戏桥 bridges/ 接入，架构见 [docs/features/multi-source.md](docs/features/multi-source.md)）。浏览器本体仍只运行在纯浏览器环境下：功能编写时请确保代码的兼容性和性能，避免使用不兼容的API和过于复杂的算法；不得要求用户启用一个如node的额外运行时环境；浏览器模式（无壳）必须保持 osu! 单源完整可用，以确保插件的独立性和可移植性。
- 【豁免】匿名使用统计（遥测）是唯一允许的 tosu 之外数据去向：`js/app/telemetry.js` 向自建后端（`backend/`，Go + SQLite）匿名上报聚合属性（算法/键数/mod/模式/难度/耗时等）。约束：默认开启可关（`enableTelemetry` 设置，Network 分组）、endpoint 硬编码于 `index.js`、静默失败不影响插件、绝不采集用户名/玩家id/分数/谱面标识/IP。后端代码随仓库提交（`backend/`，其中 `backend/docs/`、`.env`、`telemetry.db` 为 gitignore 私有）。
- 在README和settings.json中，由于目标为普通用户，请使用直白的语言描述功能，不要使用过于专业，或内部使用的术语。
- settings.json中，请全程使用英文。设置描述应当简洁直白，以确保用户能够理解设置项的作用。checkbox类应当放在options类之前；Link部分应当放在最前面。
- 合理安排代码的结构和模块划分，确保代码的可维护性。避免过于复杂的嵌套和冗余的代码逻辑。减少代码的重复性，增加复用程度。减少代码的耦合性，确保模块之间的独立性。遵循单一职责原则，确保每个模块只负责一个功能。
- 在未得到用户允许的情况下，禁止添加co-author、license、copyright等信息。
- 禁止在代码中添加任何形式的广告或推广内容。

## 注意事项
- 在有必要的情况下，你可以根据下方[参考链接](#参考链接)下载涉及到的仓库到本地进行分析。
- 插件的数据来源共三类：osu! 经 tosu 的 Websocket API（谱面数据与游戏状态）；Etterna 与 Malody V 经桌面壳（desktop/，song/state/settings 帧）接入。浏览器模式下壳不可达时自动回落 osu! 单源。你无需关心数据是如何得来的。
- config.js文件是提供给js内部使用的配置文件，而 settings.json 文件是暴露给 tosu 的插件设置定义文件，包含插件的设置项和默认值,用户可以通过 tosu 的设置界面修改这里定义的内容，从而改变插件的行为。但是，这并不是设置文件。实际的设置文件位于 tosu 的 `settings` 目录下，文件名为 `<插件目录名>.json`。实际的设置是通过 Websocket 从 tosu 传递给插件的，你无需关心设置是如何被修改的。
- index.js中的版本号应当与metadata.txt中的版本号保持一致。metadata.txt中的版本号是暴露给 tosu 的插件版本号，用户可以在 tosu 的插件管理界面看到该版本号。index.js中的版本号是插件内部使用的版本号，用于判断插件是否需要更新。
- tosu 的默认端口为24050，其获取谱面数据的端点为http://{host:port}/files/beatmap/file。

## 对话要求
- 请确保对话内容与项目相关，避免无关话题。
- 部分专有名词你可能需要联网搜索获取资料。如果你对某个概念或术语不熟悉，请积极搜索相关资料或向用户提问。
- 在进行实施之前，务必对接需求，有必要时向用户提问，确保对需求的理解准确无误。
- 在提供代码建议时，请确保代码的正确性和可读性，遵循项目的设计模式和统一规范。
- 使用用户的语言进行对话，使用中文/英文编写注释和文档。当用户语言非中文时，使用英文编写。
- 当使用plan模式时，尽可能详细的描述你要做什么，为什么这么做，以及你是怎么做的。让用户能够清晰地理解你的思路和实现细节。

## 估计算法 Benchmark
- 如果你在制作/改进估计难度算法，你需要使用 Benchmark 来验证算法的准确性。
- Benchmark 目前位于仓库[VSRG-DanEstimation-Benchmark](https://github.com/LeoBlackMT/VSRG-DanEstimation-Benchmark)，请clone后阅读其README文件，了解如何使用Benchmark进行算法验证。
- 无论如何你都需要避免读取 samples 中各个分类文件夹中的谱面样本数据，否则你可能会在算法中对这些谱面进行硬编码，或形成严重的过拟合，导致算法在实际使用中失效。

## 参考链接
- 本项目仓库地址 https://github.com/LeoBlackMT/osumania_map_analyser。
- Osu!Lazer 源码仓库: https://github.com/ppy/osu 和 https://github.com/ppy/osu-framework。其中 Osu Stable的源码并不公开。
- [tosu](https://github.com/tosuapp/tosu): 本插件的运行环境和基础框架。
- [Etterna](https://github.com/etternagame/etterna): 使用了Etterna的MinaCalc进行难度估计和MSD计算。
- [Sunny Rework](https://github.com/sunnyxxy/Star-Rating-Rebirth): 使用了Sunny Rework的算法进行难度估计。
- [Interlude](https://github.com/YAVSRG/YAVSRG): 使用了Interlude的RC键型分析算法并在基础上新增LN检测算法。
- [Daniel](https://thebagelofman.github.io/Daniel/): 使用了Daniel的算法进行难度估计。
- [Companella](https://github.com/Leinadix/companella): 使用了Companella的算法进行难度估计。
- [VSRG-DanEstimation-Benchmark](https://github.com/LeoBlackMT/VSRG-DanEstimation-Benchmark)：估计算法基准测试仓库。

## 行为准则

Behavioral guidelines to reduce common LLM coding mistakes.

**Tradeoff:** These guidelines bias toward caution over speed. For trivial tasks, use judgment.

## 1. Think Before Coding

**Don't assume. Don't hide confusion. Surface tradeoffs.**

Before implementing:
- State your assumptions explicitly. If uncertain, ask.
- If multiple interpretations exist, present them - don't pick silently.
- If a simpler approach exists, say so. Push back when warranted.
- If something is unclear, stop. Name what's confusing. Ask.

## 2. Simplicity First

**Minimum code that solves the problem. Nothing speculative.**

- No features beyond what was asked.
- No abstractions for single-use code.
- No "flexibility" or "configurability" that wasn't requested.
- No error handling for impossible scenarios.
- If you write 200 lines and it could be 50, rewrite it.

Ask yourself: "Would a senior engineer say this is overcomplicated?" If yes, simplify.

## 3. Surgical Changes

**Touch only what you must. Clean up only your own mess.**

When editing existing code:
- Don't "improve" adjacent code, comments, or formatting.
- Don't refactor things that aren't broken.
- Match existing style, even if you'd do it differently.
- If you notice unrelated dead code, mention it - don't delete it.

When your changes create orphans:
- Remove imports/variables/functions that YOUR changes made unused.
- Don't remove pre-existing dead code unless asked.

The test: Every changed line should trace directly to the user's request.

## 4. Goal-Driven Execution

**Define success criteria. Loop until verified.**

Transform tasks into verifiable goals:
- "Add validation" → "Write tests for invalid inputs, then make them pass"
- "Fix the bug" → "Write a test that reproduces it, then make it pass"
- "Refactor X" → "Ensure tests pass before and after"

For multi-step tasks, state a brief plan:
```
1. [Step] → verify: [check]
2. [Step] → verify: [check]
3. [Step] → verify: [check]
```

Strong success criteria let you loop independently. Weak criteria ("make it work") require constant clarification.

---

**These guidelines are working if:** fewer unnecessary changes in diffs, fewer rewrites due to overcomplication, and clarifying questions come before implementation rather than after mistakes.
