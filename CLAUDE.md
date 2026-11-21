# CLAUDE.md
> 本文是写给 LLM 的项目说明和编写要求，LLM 在编写代码时请务必遵守本文档的要求。
> This document is written for LLMs, and LLMs must follow the requirements of this document when writing code.
> 注意：AGENTS.md和CLAUDE.md如果出现冲突，请以CLAUDE.md为准。除非用户明确要求。如果遇到不确定的情况，请向用户提问。

## 项目介绍
- 本项目仓库地址 https://github.com/LeoBlackMT/osumania_map_analyser
- 本仓库是一个 VSRG 游戏内叠加界面（下称插件）：既可作 Osu! 内存 Hook 工具 [tosu](https://github.com/tosuapp/tosu) 的 ppcounter 运行，也可由自带桌面壳独立运行。实时在 osu!mania(4/6/7K/Lazer/Stable)、Etterna、Malody V（及其编辑器）、Malody 4.3.7下，提供估算难度、RC/LN键型分析、MSD计算、难度分布、区间分析和分轨准确度等多种实用功能。
- 仓库和项目结构详见 [docs/architecture.md](docs/architecture.md)

## 要求限制
- 在对代码进行破坏性修改、对未被git跟踪的文件进行修改、对已有功能进行大幅度改动之前，请务必先征求用户意见，确保用户理解你的修改意图。随后，在 backup 目录下创建备份文件夹，并将你要修改的文件复制到备份文件夹中，以便在出现问题时可以快速恢复。请使用时间戳和修改内容命名目录。
- 在生成测试代码/保存临时文件/生成调试文件时，请将其放置在 temp 目录下，并确保不会被提交到远程仓库。随后在实现功能后，请删除 temp 目录下本次相关的文件。测试脚本等临时文件建议等用户完全验收后再删除，避免验收时无法复用。测试脚本一律不得提交到仓库。
- 请按照用户的实际情况进行git操作，默认允许 commit，但是不允许 push。请在进行 push 之前征求用户意见。严禁直接 push 到 main 分支，除非用户明确要求。请在进行 push 之前确保代码已经过测试，并且不会破坏已有功能。请使用英文Pull Request的方式进行贡献。
- 文档编写的规范和要求详见 [docs/README.md](docs/README.md)，请务必遵守。在新增功能/修改功能/修改管线时，请务必修改对应的文档，确保文档内容与实际功能一致。在进行重大破坏性修改时，请务必编写文档并标注修改内容和修改原因。在编辑附属项目时，请先阅读对应的README和文档以获取信息。
- 由于扩充了数据源，功能编写时请确保代码的兼容性和性能，避免使用不兼容的API和过于复杂的算法；不得要求用户启用一个如node的额外运行时环境；浏览器模式（无壳）必须保持 osu! 单源完整可用。除唯一后端（<https://mma-stats.leoblack.top/>）交互和本GitHub在线仓库外，不允许任何在线网络连接。
- 发行相关：版本号三处必须同时改并写在同一次提交里——`index.js` 的 `_VERSION`、`metadata.txt` 的 `Version`、`backend/build.ps1` 的 `-Version` 默认值。打包用 `release.ps1`，产物落在 gitignore 的 `release/`；Ed25519 反馈签名 seed 存在于 `.secrets/` 以及 GitHub 仓库secret中，不得写入明文仓库、日志、文档或对话。发行版必须通过 GitHub Release + tag 发布。页面侧不匹配会进终态并停止重连。index.js中的版本号应当与metadata.txt中的版本号保持一致。metadata.txt中的版本号是暴露给 tosu 的插件版本号，用户可以在 tosu 的插件管理界面看到该版本号。index.js中的版本号是插件内部使用的版本号，用于判断插件是否需要更新。
- 在tosu使用的settings.json中，请全程使用英文。checkbox类应当放在options类之前；Link部分应当放在最前面。
- 设置描述和README应当简洁直白，以确保用户能够理解设置项的作用。
- 合理安排代码的结构和模块划分，确保代码的可维护性。避免过于复杂的嵌套和冗余的代码逻辑。减少代码的重复性，增加复用程度。减少代码的耦合性，确保模块之间的独立性。遵循单一职责原则，确保每个模块只负责一个功能。
- 在未得到用户允许的情况下，禁止添加co-author、license、copyright等信息。
- 禁止在代码中添加任何形式的广告或推广内容。
- 不要因为假想风险，主动加警告、免责声明、审批流程或安全/合规清单。
- 不要因为用户驳回某个特性就在文档或注释中添加额外说明。
- 删除文件优先移入系统回收站。不得删除与任务无关的用户文件，不得擅自扩大删除范围。删除不做多余的事，不额外添加防止已移除内容被重新引入的规则或记录。

## 注意事项
- 在有必要的情况下，你可以根据下方[参考链接](#参考链接)下载涉及到的仓库到本地进行分析。
- config.js文件是提供给js内部使用的配置文件，而 settings.json 文件是暴露给 tosu 的插件设置定义文件，包含插件的设置项和默认值,用户可以通过 tosu 的设置界面修改这里定义的内容，从而改变插件的行为。但是，这并不是设置文件。实际的设置文件位于 tosu 的 `settings` 目录下，文件名为 `<插件目录名>.json`。实际的设置是通过 Websocket 从 tosu 传递给插件的，你无需关心 tosu 的设置是如何被修改的。
- tosu 的默认端口为24050，其获取谱面数据的端点为http://{host:port}/files/beatmap/file。

## 对话要求
- 用户当前明确指令优先，与其他规范发生冲突需要说明。
- 用户建议不适合目标时直接说，不要迎合。
- 部分专有名词你可能需要联网搜索获取资料。如果你对某个概念或术语不熟悉，请积极搜索相关资料或向用户提问。
- 在进行实施之前，务必对接需求，有必要时向用户提问，确保对需求的理解准确无误。多种理解会导致实质不同的工作时用 ask 澄清，不自行选一种。
- 使用用户的语言进行对话，使用中文/英文编写注释和文档。当用户语言非中文时，使用英文编写。
- 当编写计划书时，尽可能详细的描述你要做什么，为什么这么做，以及你是怎么做的。让用户能够清晰地理解你的思路和实现细节。
- 交流时先给影响与结论，再给必要证据，最后给接下来的行动、待决策和建议，没有对应内容就省略。
- 优先使用具体、简单的词和通俗易懂的表达，避免无意义术语、套话、重复总结。
- 技术细节只保留对理解结论、判断风险或复现结果有帮助的部分；解释复杂内容时可使用可视化功能进行辅助理解。

## 估计算法 Benchmark
- 如果你在制作/改进估计难度算法，你需要使用 Benchmark 来验证算法的准确性。
- Benchmark 目前位于仓库[VSRG-DanEstimation-Benchmark](https://github.com/LeoBlackMT/VSRG-DanEstimation-Benchmark)，请clone后阅读其README文件，了解如何使用Benchmark进行算法验证。
- 无论如何你都需要避免读取 samples 中各个分类文件夹中的谱面样本数据，否则你可能会在算法中对这些谱面数据硬编码，或形成严重的过拟合，导致算法在实际使用中失效。

## 参考链接
- 本项目仓库地址 https://github.com/LeoBlackMT/osumania_map_analyser 。
- Osu!Lazer 源码仓库: https://github.com/ppy/osu 和 https://github.com/ppy/osu-framework 。其中 Osu Stable的源码并不公开。
- [tosu](https://github.com/tosuapp/tosu): 本插件的运行环境和基础框架。
- [Etterna](https://github.com/etternagame/etterna): 使用了Etterna的MinaCalc进行难度估计和MSD计算。
- [Sunny Rework](https://github.com/sunnyxxy/Star-Rating-Rebirth): 使用了Sunny Rework的算法进行难度估计。
- [Interlude](https://github.com/YAVSRG/YAVSRG): 使用了Interlude的RC键型分析算法并在基础上新增LN检测算法。
- [Daniel](https://thebagelofman.github.io/Daniel/): 使用了Daniel的算法进行难度估计。
- [Companella](https://github.com/Leinadix/companella): 使用了Companella的算法进行难度估计。
- [XDC1](https://github.com/TheBagelOfMan/xdc1): 使用了 XDC1 的算法进行 4K 难度估计。
- [Proj7k](https://github.com/ka1z07/proj7k):使用了 Proj7k 的算法进行 7K 难度估计。
- [VSRG-DanEstimation-Benchmark](https://github.com/LeoBlackMT/VSRG-DanEstimation-Benchmark)：估计算法基准测试仓库。
- [DanielEtterna](https://github.com/JoseMGS3/DanielEtterna)：参考了其获取Etterna数据的方法。