# 仓库核心项目结构

> 本文档给出仓库顶层目录、插件模块与附属项目的地图。文档编写规范与完整索引见 [docs/README.md](README.md)。

## 核心文档索引

- 文档总索引与编写规范：[README.md](README.md)
- 功能技术文档：[features/README.md](features/README.md)（难度估计、Mixed 路由、键型分析、图表、多数据源、桌面壳、LaneAcc、难度反馈等）
- 分析管线：[pipeline/README.md](pipeline/README.md)（总览、Worker、结果缓存、设置管线、mod、转换器）
- 开发指南：[guides/README.md](guides/README.md)（新增设置、接入 Worker、缓存失效、模块约定）
- 经验与教训：[learnings/README.md](learnings/README.md)
- 破坏性变更记录：[breakings/README.md](breakings/README.md)
- 用户文档：[settings.md](settings.md)、[shell-guide.md](shell-guide.md)、[presets-guide.md](presets-guide.md)
- 算法专题：[azusa_algorithm.md](azusa_algorithm.md)、[roxy_algorithm.md](roxy_algorithm.md)、[polaris_algorithm.md](polaris_algorithm.md)
- 桌面壳：[desktop/README.md](../desktop/README.md)、[desktop/docs/architecture.md](../desktop/docs/architecture.md)、[desktop/docs/CONTRACT.md](../desktop/docs/CONTRACT.md)
- 游戏桥接：[bridges/README.md](../bridges/README.md)
- 后端：[backend/README.md](../backend/README.md)
- 辅助工具：[tools/README.md](../tools/README.md)

## 仓库目录

- .github/：壳与后端构建 CI。
- .secrets/（本地私有）：用于签名的密钥。远程仓库不存在。
- docs/：项目功能说明、管线文档、指南与索引。
    - docs/README.md：目录说明和索引文档。
- img/：图片资源目录。
- backup/（本地私有）：用于存放本地备份的文件夹，仅在本地使用，远程仓库不存在。
- temp/（本地私有）：用于存放本地临时文件的文件夹，仅在本地使用，远程仓库不存在。
- release.ps1：发行打包脚本。
- release/（本地私有）：运行发行脚本后生成的目录，发行包在此创建。
- desktop/：桌面壳子项目（Tauri v2 + Rust），用于在桌面环境下运行插件。详见 [desktop/README.md](../desktop/README.md)，项目结构 [desktop/docs/architecture.md](../desktop/docs/architecture.md)，壳-页面契约 [desktop/docs/CONTRACT.md](../desktop/docs/CONTRACT.md)。
- bridges/：游戏桥接项目，包含 Etterna Lua 桥、Malody 编辑器 Lua 桥与 Malody V BepInEx 选曲桥。详见 [bridges/README.md](../bridges/README.md)。
- backend/：后端项目（Go + SQLite），用于收集匿名使用统计和估计算法反馈数据。详见 [backend/README.md](../backend/README.md)。
- tools/：离线辅助工具，包括 `lazer-offsets-gen` 偏移表生成器、`offsets-tool` manifest 签名/校验、`malody-v-od-check`、`malody4-od-check`、`i18n-check` 等。详见 [tools/README.md](../tools/README.md)。

## 插件本体（ManiaMapAnalyser by Leo_Black/）

插件目录名携带空格，为 tosu 的命名要求；代码、脚本与文档引用时必须逐字符一致。

- js/app/：插件核心功能的 js 目录（仅浏览器端运行），已按职责划分为内聚子目录：
    - js/app/core/：核心流程控制（analysis、main、appContext、settings、scheduler、resultCache、modeLogic、beatmapIdentity）。
    - js/app/net/：网络连接、WebSocket 与遥测（socket、socketHandlers、telemetry、telemetrySpec、updateChecker、shellConfigClient、replayPauses、externalLink）。
    - js/app/ui/：DOM 呈现、HUD、主题与组件（display、hud、combinedMode、contentBarTabs、coverTheme、triangles、changeKind、windowAffordances 等），包含子目录 ui/laneAcc/ 交互视图。
    - js/app/live/：局内实时状态与计数（livePp、livePpCounts）。
    - js/app/graph/：难度走势图表与选区分析（index、math、section、sectionOverlay）。
    - js/app/worker/：worker 线程（compute.worker.js + manager.js），把计算移出主线程，避免 UI 卡顿。
    - js/app/sources/：四数据源路由（sourceManager、sourceActivity）、壳桥客户端（bridgeClient、shellState）、外部源注入（externalSource）、OD 解析与 osu 读取器恢复/扫描提示。
    - js/app/feedback/：难度反馈的门控、评分条、数学与协议。
    - js/app/presets/：预设系统（schema、存储、内置预设、tosu/壳两种传输）。
    - js/app/settingsPage/：壳内设置页（表单、链接、壳配置面板与传输）。
    - js/app/i18n/：壳内页面（设置页、预设管理）的词典与取词模块（t/tf）。settings.json 保持英文，翻译只作用于壳自己的页面显示。
- js/laneAcc/：独立通用数据与计算模型（无 DOM，按键回放帧解析与精度累加，accumulator、chart、format、rawFrames、random、stableV2、timeAxis）。
- js/pipeline/：runAnalysisPipeline.js——一次调用完成解析 → 估算 → 标准化 → 附属段的纯函数管线。
- js/debug/：插件的调试目录，包含插件的调试工具和调试代码。应当保留。
- js/estimator/：九个难度估算算法的实现及共享数学工具（estimatorMath），包括 Sunny、Daniel、Azusa、Roxy、Polaris、XDC1、Proj7k、Companella 与 Mixed。
    - js/estimator/companella/：Companella 难度估计算法的 ONNX 模型及其 WASM 运行时。
    - js/estimator/proj7k/：Proj7k 7K 难度引擎本体。
    - js/estimator/intervals/：Polaris 与 rework 估算链路共用的星数 → 段位映射表（按键数与 RC/LN 分类）。
- js/ett/：插件集成的 Etterna MinaCalc 目录，包含 Etterna MinaCalc 的六个版本 WASM 运行时。
- js/interlude/：插件集成的 Interlude 星数计算模块。
- js/parser/：谱面解析、Mod 解析（modData）、时间切片（osuTimeSlice）、设置解析、判定档 → OD 表、列重映射与 note 列处理，以及 sm/ssc/mc → .osu 转换器；第三方 simfile 解析器在 vendor/simfile-parser。
- js/patterns/：插件的键型分析模块，包含 RC/LN 键型分析器。其中 RC 键型分析器使用了 Interlude 的算法，并在其基础上新增了 LN 检测算法。
- js/rework/：插件集成的星数重算算法模块，主要包含 Sunny / SunnyWindow 算法核心及 sunnyCore 共享计算。
- styles/：插件的样式目录，包含插件的 CSS 样式文件。
- styles/fonts/：插件的字体目录，包含插件的字体文件。
- presets/：内置预设 JSON，index.json 为清单。
- presets.html：预设管理页；settings.html：壳内设置页入口。
- config.js：插件内部的配置文件，包含一些全局定义和常数。注意其 options 部分为 tosu 侧配置文件的枚举。
- debug.html：插件的调试页面，主要用于调试插件的估计算法和 tosu api。
- index.html：插件的主页面，显示插件的核心功能。
- index.js：插件的主入口文件，负责初始化插件和注册插件的功能。包含版本号（内部）。
- metadata.txt：用于 tosu 读取的插件元信息文件，包含插件的名称、版本（外部）、作者和描述等信息。
- settings.json：暴露给 tosu 的插件设置定义文件，包含插件的设置项和默认值。但是，这并不是设置文件。实际的设置文件位于 tosu 的 `settings` 目录下，文件名为 `<插件目录名>.json`。
