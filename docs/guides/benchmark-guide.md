# Benchmark 使用指南（benchmark-guide.md）

> 目标读者：AI / 开发者。本文说明如何对难度估计算法运行基准测试（Benchmark）：benchmark 仓库的位置与协作关系、esm-loader 机制、运行方式、输出、影响面与注意事项。所有引用均为 `path:line + symbol name` 格式，可据此定位源码。

## 1. 两个仓库的协作关系

Benchmark 使用**独立仓库** [VSRG-DanEstimation-Benchmark](https://github.com/LeoBlackMT/VSRG-DanEstimation-Benchmark)，不在本仓库内：

- **本仓库**（osumania_map_analyser）：算法与数据解析的实现方（`js/estimator/`、`js/parser/`、`js/ett/`、`js/patterns/` 等）。
- **benchmark 仓库**（本地路径 `C:\Users\Leo_BlackLT\Desktop\Dev\web\VSRG-DanEstimation-Benchmark\`）：验证方，其 `runner/` 目录通过绝对路径反向导入本仓库的 `js/` 代码运行算法。
- 本仓库**没有** runner 目录；`AGENTS.md:20`（"5 levels up from `runner/`"）记载的跨仓库相对路径为历史状态，实际以绝对路径 `C:\Users\Leo_BlackLT\Desktop\Dev\web\VSRG-DanEstimation-Benchmark\` 为准（见 [注意事项](#8-注意事项)）。
- benchmark 仓库是独立的 git 仓库（本仓库 `.gitignore` 之外），不在本仓库版本控制内。
- 两个运行时环境对应关系见 `AGENTS.md:27-28`：Node 环境（`--loader esm-loader.mjs`）与 Python 环境（`benchmark-danoverlay.py`）。

## 2. 何时必须使用 Benchmark

- 制作/改进估计难度算法时，**必须**使用 Benchmark 验证准确性（`CLAUDE.md:65-66`）。
- 修改下列**共享模块**后，benchmark 结果会随之变化，建议重跑相关算法的 benchmark（详见 [影响面](#6-影响面与重跑建议)）。
- 估算器入口与共享模块依赖关系见 [difficulty-estimation.md](../features/difficulty-estimation.md)（其中第 1 节列出估算器依赖的共享模块表）。

## 3. esm-loader.mjs 机制

`runner/esm-loader.mjs` 是 Node ESM `load` 钩子（机制说明见 `AGENTS.md:110`）：

- 强制将路径匹配 `/maniamapanalyser by leo_black/js/` 的每个 `.js` 文件按 `format: "module"` 加载。原因：插件文件夹（`ManiaMapAnalyser by Leo_Black/`）没有 `package.json "type": "module"`，Node 默认会把 `js/` 下的文件当作 CommonJS，而 `js/` 中的代码使用 `export` 语法，不强制 ESM 会直接报错。
- **不解析无扩展名的 specifier**：它只强制 ESM 格式，不修补模块解析。因此本仓库 `js/` 内的 import 必须使用显式 `.js` 扩展名（浏览器可将 `"./foo"` 解析为 `"./foo.js"`，Node 不会，见 `AGENTS.md:144` 注意事项 3）。新文件必须遵守此约定，详见 [module-conventions.md](module-conventions.md)。

## 4. 运行方式

所有命令在 benchmark 仓库根目录运行。以下为本仓库侧的视角（benchmark 仓库内部实现细节以其自身 README 为准）：

| 入口 | 命令 | 说明 |
| --- | --- | --- |
| `runner/run-benchmark.ps1` | `.\runner\run-benchmark.ps1` 或脚本内命令 `node --loader esm-loader.mjs benchmark-runner.mjs [-Algorithm X] [-ListAlgorithms] [-WriteSourceCsv]`（`AGENTS.md:106`） | 主 benchmark 入口，从 benchmark 仓库根运行；`-ListAlgorithms` 列出可选算法，`-WriteSourceCsv` 输出源数据 CSV |
| `runner/run-danoverlay-benchmark.ps1` | `python benchmark-danoverlay.py`（无需 esm-loader） | DanOverlay Python 引擎 benchmark：调用本仓库 `temp/Dan-Overlay-main`（`src/pipeline.py` 的 `analyze_map()`），DP→numeric 换算为 `numeric = DP - 0.5`，仅 4K RC、固定 NM；需要 Python ≥3.12 + numpy（`AGENTS.md:28`） |
| `runner/run-roxy-benchmark.ps1` | `benchmark-roxy.mjs` | Roxy 专用 benchmark 入口 |
| `runner/smoke-result-cache.mjs` | 纯 `node runner/smoke-result-cache.mjs`，**无需 esm-loader**（`AGENTS.md:109`） | 结果缓存冒烟测试：`resultCache.js` 无 import（`ManiaMapAnalyser by Leo_Black/js/app/resultCache.js:15 createResultCache` 所在模块为纯 DOM-free 模块），共 8 个用例，通过时打印 `SMOKE: 8/8 PASS` |

## 5. 输出

- 运行结果写入 benchmark 仓库内：`results/{Algorithm}.csv`（各算法结果 CSV）+ `results/index.json`（索引，`AGENTS.md:106`）。
- 结果也可在 [benchmark.leoblack.top](https://benchmark.leoblack.top/) 在线查看（本仓库 `README.md:34`）。

## 6. 影响面与重跑建议

以下共享模块同时被浏览器插件与 Node benchmark runner 使用（不含浏览器 API），修改它们会直接改变 benchmark 结果：

| 模块 | 作用 |
| --- | --- |
| `js/estimator/` | 全部 6 种难度估计算法（入口见 difficulty-estimation.md 第 2 节） |
| `js/parser/` | `.osu` 谱面解析（`OsuFileParser`） |
| `js/ett/` | Etterna MinaCalc WASM（MSD 计算） |
| `js/patterns/` | RC/LN 键型分析（Interlude 算法 + LN 检测） |
| `js/estimator/intervals/` | 段位区间表（`DAN_INDEX`） |

修改上述任一模块后，建议重跑受影响的算法 benchmark 验证结果（例如改动 Sunny 算法后重跑 Sunny/Daniel/Azusa/Mixed 等依赖方）。

## 7. 约束：禁止读取 samples 谱面数据

- 基准测试的谱面样本位于 benchmark 仓库的 `samples/`（含分类文件夹与 `samples.7z`，下载入口见本仓库 `README.md:36`）。
- **无论如何避免读取 `samples` 中各个分类文件夹内的谱面样本数据**（`CLAUDE.md:67`）：否则可能对这些谱面硬编码或形成严重过拟合，导致算法在实际使用中失效。
- 允许的操作仅限于：查看目录结构/文件名，或在基准测试运行时通过 runner 程序间接消费谱面数据。禁止直接打开、复制、粘贴、引用样本谱面内容到代码或 prompt 中。
- 如需自测，可在 `temp/` 下使用非样本谱面（`temp/` 为本地私有目录，gitignored，见 `CLAUDE.md:14`）。

## 8. 注意事项

1. **esm-loader 不修补 specifier**：`js/` 内 import 必须显式 `.js`（`AGENTS.md:144`），新增文件遗漏扩展名会在 Node 端（benchmark runner）报模块找不到，而浏览器端正常——不要用浏览器验证代替 Node 验证。
2. **`temp/` 目录**：本仓库的 `temp/Dan-Overlay-main` 是本地私有目录（gitignored，`AGENTS.md:28` 注明），benchmark 仓库的 Python runner 依赖它存在；它不属于本仓库版本控制。
3. **路径以绝对路径为准**：`AGENTS.md:20` 记载的相对路径（5 层 `../`）为历史状态，本指南写作时验证的本地路径为 `C:\Users\Leo_BlackLT\Desktop\Dev\web\VSRG-DanEstimation-Benchmark\runner\`；如需移植环境，请以实际目录为准。
4. **benchmark 结果仅供参考**：基准测试只提供算法表现的参考，实际使用中可能受谱面特征、mod 组合等因素影响（本仓库 `README.md:35`），最终以玩家实际游玩体验为准。
5. **本指南只描述本仓库侧的使用视角**：benchmark 仓库内部实现（runner 脚本细节、算法选取规则等）以其自身 README 为准。
