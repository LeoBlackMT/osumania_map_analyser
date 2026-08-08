# 模块编写约定（module-conventions.md）

> 面向 AI 的约定清单：新增/修改 `ManiaMapAnalyser by Leo_Black/js/` 下模块前的检查项。
> 所有引用为 `path:line + symbol` 格式，行号可能随版本漂移，动手前请用 grep 复核。
> 核心依据：`AGENTS.md`（Common pitfalls 段）、`CLAUDE.md`（要求限制段）。

## 适用范围

`ManiaMapAnalyser by Leo_Black/js/` 下的代码在**两个环境**中运行：

- **浏览器**：tosu overlay（`index.html` → `index.js`），ES modules + DOM + WebSocket + Worker + WASM
- **Node**：独立仓库 `VSRG-DanEstimation-Benchmark` 的 runner，经 `--loader esm-loader.mjs` 直接 import `js/` 下的共享模块

任何模块都可能在 Node 中加载。违反下述约定 = benchmark runner 无法加载（`ERR_MODULE_NOT_FOUND`）或浏览器运行时崩溃。

## 约定清单

### 1. import 扩展名必须显式 `.js`

浏览器会把 `"./foo"` 解析为 `"./foo.js"`，Node **不会**——`esm-loader.mjs` 只强制 ESM format，**不**解析 extensionless specifier。`js/` 内新增文件的 import specifier 必须带 `.js`。

- 依据：`AGENTS.md:110`（esm-loader.mjs 实际机制）、`AGENTS.md:144`（pitfall 3 "Import extensions"）
- 反例：`import { x } from "./foo"` → Node 下报 `ERR_MODULE_NOT_FOUND`

### 2. Node/browser 拆分

| 目录 | 环境 | 约束 |
|---|---|---|
| `js/app/` | 浏览器专属 | 可用 `window`/`document` |
| `js/estimator/`、`js/ett/`、`js/interlude/`、`js/parser/`、`js/patterns/`、`js/rework/` | 共享（Node benchmark 使用） | **禁止** `window`/`document` |

`appContext.js` 顶层就执行 `document.getElementById`（`js/app/appContext.js:23-62` `statusEl` 等 DOM refs）——共享模块**不得** import 它或任何 `js/app/` 模块。

- 依据：`AGENTS.md:30`、`AGENTS.md:79-86`（Key modules 表）

### 3. `import.meta.url` 模式

Worker 创建与资源路径解析必须用 `new URL(specifier, import.meta.url)`（相对**模块文件**解析），**勿**改成相对字符串——相对字符串会相对 `index.html` 解析而失效。

- Worker 创建：`manager.js:18-21` `ensureWorker()` 内 `new Worker(new URL("./compute.worker.js", import.meta.url), { type: "module" })`
- WASM 路径：`calc.js:56-59` `locateFile` 的 `new URL(\`./versions/${path}\`, import.meta.url)`；`calc.js:73` 同模式（Node 侧经 `toWasmPath` 转文件系统路径）
- ONNX 路径：`companellaEstimator.js:63`（`ort.env.wasm.wasmPaths`）、`companellaEstimator.js:67`（`dan_model.onnx` URL）
- 依据：`AGENTS.md:153`（pitfall 12 "import.meta.url patterns"）

### 4. 动态 `import()` 唯一场景

全项目唯一的动态 import 是 `companellaEstimator.js:46-55` `getOrtNamespace()`：按环境懒加载 ONNX Runtime（Node → `ort.node.min.mjs`，浏览器 → `ort.wasm.min.mjs`）。同类例外：`companellaEstimator.js:70` `import("node:url")`（仅 Node 分支）。

新增功能**不得**再引入其他 `import()`——需要条件加载时优先用顶层静态 import 加环境分支。

- 依据：`AGENTS.md:153`

### 5. WASM 处理

- **不直接 import / `fs.readFileSync` `.wasm` 文件**：经 `js/ett/versions/` 的 JS glue（`minaclac-*.js`，Emscripten 导出）实例化；`.wasm` 作为静态资源由浏览器 fetch（`calc.js:56-59` `locateFile`）。
- Node 下经 glue 的 `wasmBinary` 传入预读字节（`calc.js:65-74` `loadEtternaModule`），不要自己加载 `.wasm`。
- 新增 Etterna 版本：`.wasm` + `.js` glue 成对放入 `js/ett/versions/`，并在 `js/ett/versions/index.js` 注册 loader（`calc.js:47-53` `WASM_FILE_BY_VERSION` 同步登记文件名）。
- 依据：`AGENTS.md:14`、`AGENTS.md:71-73`、`AGENTS.md:143`（pitfall 2 "WASM in Node"）

### 6. 文件夹名精确

插件文件夹名为 `ManiaMapAnalyser by Leo_Black`（**带空格**）。所有路径引用（代码、文档、脚本）必须逐字符一致；**禁止**重命名或 kebab-case 化。benchmark runner 依赖该精确路径 import `js/`（`AGENTS.md:20`）。

- 依据：`AGENTS.md:18`（Folder naming quirk）

### 7. 双层 state 约定

`state` 定义于 `js/app/appContext.js:64`，关键字段在 `appContext.js:76-89`：

- **写** `user*` 字段（用户真实偏好，可为 `"Auto"`）：`userContentBar`/`userSrText`/`userDiffText`（`appContext.js:79-81`）
- **读** 解析后字段：`contentBar`/`srText`/`diffText`（`appContext.js:76-78, 87`）；`effectiveContentBar`（`appContext.js:77`）为谱面级覆盖
- **估计算法双层**：`estimatorAlgorithm`（用户选择，`appContext.js:88`）vs `actualEstimatorAlgorithm`（实际执行，`appContext.js:89`，如 Azusa 因 LN 过高降级为 Sunny）——分析后读后者，缓存命中时从快照恢复，勿重算
- 写入 `state.contentBar = "Auto"` 是 bug（`AGENTS.md:146` pitfall 5）
- 依据：`AGENTS.md:120-121`；设置全流程见 [../pipeline/settings-pipeline.md](../pipeline/settings-pipeline.md)

### 8. 新计算影响设置必须进失效列表

结果缓存 key 不包含计算类设置（`AGENTS.md:66`）→ **任何影响计算结果的设置**必须加入 `settings.js` 命令监听器的 `clearResultCache()` 失效列表（`AGENTS.md:122`）；纯展示设置**不得**加（由覆盖检查处理，`AGENTS.md:64`）——漏加 = 静默返回旧结果。

- 依据：`AGENTS.md:154`（pitfall 13 "Result cache correctness"）；详见 [cache-invalidation.md](cache-invalidation.md)

### 9. 版本号一致性

`index.js` 内部版本（`index.js:3` `_VERSION`）必须与 `metadata.txt` 外部版本（`metadata.txt:6` `Version`）一致（当前均为 `1.7.1`）。发布新版本时两处同步修改——`metadata.txt` 版本暴露给 tosu 插件管理界面，`_VERSION` 供插件内部判断更新。

- 依据：`CLAUDE.md:53`

### 10. 无 co-author/license/copyright；settings 全英文

- 未获用户允许**禁止**添加 co-author、license、copyright 等信息（`CLAUDE.md:46`）
- `settings.json` 全程英文；标题与描述简洁直白、面向普通用户，不用内部术语（`CLAUDE.md:43-44`）；checkbox 放 options 之前、Link 放最前（`CLAUDE.md:44`）

### 11. Benchmark 影响面

改动 `js/estimator/`、`js/parser/`、`js/ett/`、`js/patterns/` 会改变独立仓库 `VSRG-DanEstimation-Benchmark` 的 benchmark 结果（`AGENTS.md:112`）——算法相关改动必须按 benchmark 流程验证，且不得读取 samples 中的谱面样本（防过拟合，`CLAUDE.md:67`）。

- 详见 [benchmark-guide.md](benchmark-guide.md)

### 12. 性能与兼容

插件运行在纯浏览器（tosu overlay）环境：避免不兼容的 API 与过于复杂的算法；不应当使用除 tosu 之外的第三方工具获取数据或进行计算，不要求用户启用 Node 等环境，确保插件的独立性和可移植性（`CLAUDE.md:42`）。

### 13. AI 工作流程：不嵌套子代理

用户硬约束：本项目文档与代码任务由执行代理**直接完成**读写，禁止在任务内再派生子代理（子代理可能破坏共享状态或引入不一致）。

### 14. 文档同步义务

新增/修改功能或管线必须同步更新对应文档：新功能编写技术文档；修改功能更新文档；管线破坏性更改更新管线文档与指南（`docs/README.md:14-16`、`CLAUDE.md:41`）；新增/删除文档需同步 `docs/README.md` 索引（`docs/README.md:17`）。

## 快速检查清单

动手前逐项确认：

- [ ] import 全部带 `.js` 扩展名（§1）
- [ ] 共享模块无 `window`/`document`，未 import `js/app/`（§2）
- [ ] Worker/资源路径用 `new URL(..., import.meta.url)`（§3）
- [ ] 未新增动态 `import()`（§4）
- [ ] `.wasm` 走 JS glue，未直接加载（§5）
- [ ] 路径中的文件夹名精确带空格（§6）
- [ ] 写 `user*`、读解析字段；算法读 `actualEstimatorAlgorithm`（§7）
- [ ] 计算影响设置已加入缓存失效列表（§8）
- [ ] 版本号与 `metadata.txt` 一致（§9）
- [ ] 无 co-author/license/copyright；settings 为英文（§10）
- [ ] 改动共享模块时评估 benchmark 影响（§11）
- [ ] API 兼容与性能符合纯浏览器环境（§12）
- [ ] 本次未派生子代理（§13）
- [ ] 对应文档已同步更新（§14）
