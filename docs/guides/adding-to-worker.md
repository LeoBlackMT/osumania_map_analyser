# docs/guides/adding-to-worker.md：新增估算器/管线阶段到 worker 的操作指南

> 面向 AI 的操作指南。文中所有 `path:line symbol` 引用均相对本仓库根目录（插件文件夹名为 `ManiaMapAnalyser by Leo_Black`，含空格，路径引用必须精确）。**照抄步骤不保证正确，每步引用的行号是编写时核实的，动手前请重新打开对应文件确认。**文中 path:line 行号为编写时快照，代码演进后可能漂移；定位源码请以符号名（symbol）为准，必要时用 grep 复核。
> 配套文档：先读 [pipeline/worker.md](../pipeline/worker.md)（worker 与 runAnalysisPipeline"怎么工作"）与 [pipeline/analysis-pipeline.md](../pipeline/analysis-pipeline.md)（管线总览）；本文是"怎么动手"。共享模块纯度约束见 [module-conventions.md](module-conventions.md) §2/§2.1。新估算器算法本身的文档义务见 [docs/README.md](../README.md) 概要与 [features/](../features/README.md)。

## 0. 速览：新增一个管线阶段要动 5 处

```
ManiaMapAnalyser by Leo_Black/js/
├── estimator/xxxEstimator.js        ← ① 入口纯化（本指南核心）
├── pipeline/runAnalysisPipeline.js  ← ② 注册到管线（或 compute.worker.js 白名单）
├── app/analysis.js                  ← ③ 消费侧接线 + 展示（浏览器专属，多为旧路径改造）
└── （可选）test/pipeline-runner.mjs ← ④ golden matrix 扩展（Node 回归）
```

同步回退与 Node harness **不需要额外代码**：它们与 worker 共用 `runAnalysisPipeline`（同一函数）。本文以"新增一个估算器"为主线，管线阶段（pattern/ett/interlude 类附属段）的差异用 ⚠️ 标注。

---

## 步骤 1：入口纯化（reference 任务 2/9 模式）

**目标**：新估算器入口 `runXxxEstimatorFromText(osuText, options = {}, parsed = null)` 在 Node 与浏览器行为一致。

### 1.1 禁止 state/window/document

参考现状（任务 2 清零后的形态）：`reworkEstimatorUtils.js:98 estDiff` 第 5 参 `enableAlwaysShowLNDifficulty` 替代读 state；`sunnyWindowEstimator.js:22/:25` 从 options 读 `enableAnalyzeLN`/`enableAlwaysShowLNDifficulty`；`sunnyWindowAlgorithm.js` 不再 import appContext。

- **禁止** `import ... from "../app/appContext.js"` 或任何 `js/app/` 模块（appContext 顶层执行 `document.getElementById`，appContext.js:23-62，import 即崩，worker/Node 双挂）。
- **禁止** 顶层或函数内 `window`/`document` 引用。
- 自查 grep（应无匹配，注释除外）：

```
grep -rn "state\.\|window\.\|document\." ManiaMapAnalyser\ by\ Leo_Black/js/estimator/xxxEstimator.js
```

### 1.2 显式选项注入

所有影响计算的输入走 `options` 透传（现有模式：`{...options}` spread 在 azusa/roxy/mixed 的每次嵌套 runX 调用处，任务 10 已验证自动透传）。新选项键加进 `analysis.js:350-372` 的 `estimatorOptions`/`pipelineOptions` 构造处，**这是浏览器路径的唯一输入源**。

- ⚠️ 若新选项影响计算结果，必须同时加入 `settings.js:844-857` 的 `clearResultCache()` 失效链（见 cache-invalidation.md 与 breaking-changes ⑦⑧）。
- ⚠️ `estimatorDebugPanel.js`（浏览器专属）是隐式全局状态读取者，若它的 `runOptionsFromPayload` 直接调 runX，也要显式传新选项（任务 2 learnings）。

### 1.3 可注入 parsed

第三参 `parsed = null`：已 `process()` 的 `OsuFileParser` 实例（**不是** `getParsedData()` 结果，不含 `timingPoints`，`modIN`/`modHO` 实例方法会挂，见 worker.md §5.1）。为空时内部 `new OsuFileParser(osuText)` + `process()`。

- ⚠️ **命名陷阱**：入口内若已有 `const parsed = normalizeReworkResult(rawResult)` 这类局部变量，会遮蔽新参数，把局部改名为 `parsedResult`（三个估算器文件都踩过，见 task-9 learnings）。
- ⚠️ 若算法需要 cvtFlag 转换（IN/HO）：在**clone** 上转换（`cloneOsuParser` 文件局部拷贝，sunnyAlgorithm.js:55-71 为模板），共享实例保持 pristine；无转换需求直接复用。

### 1.4 JSON-safe 输出

输出对象**不得含方法/getter**。worker 回传经 structuredClone，方法抛 DataCloneError。参考 pattern 的 `sanitizePatternResult`（runAnalysisPipeline.js:45-70）：getter 求值结果**不输出**，只输出消费侧读的纯数据字段。

### 1.5 软失败通道（附属段专属）

管线阶段若可能失败且旧行为是"记错误但继续"，用 `xxxError` 文本 + 字段置空（NaN/null）模式，**不并入 `errors[]`**。旧 errors.push 带展示条件（`shouldReportEtternaError`/`isKeycountError`/need*），由 analysis.js 按旧条件决定并入（worker.md §4.4）。

---

## 步骤 2：注册到 runAnalysisPipeline（或 compute.worker.js 白名单）

- **估算器**：`runAnalysisPipeline.js:129-167` 的分派块加分支（`estimatorAlgorithm === "Xxx"` → `runXxxEstimatorFromText(rawText, options, parser)`）。若算法属于归一化集合（star 口径需统一为 Sunny sr），把名字加进 `NORMALIZATION_ALGORITHMS`（:28）并核对归一化复用决策表（:109-127，见 worker.md §4.5）。
- **管线阶段**：在 `runAnalysisPipeline.js` 内按顺序插入（当前顺序：Interlude :210-219 → Pattern :221-233 → Ett :235-249 → Companella 二次 Ett :251-272），附属段开关（`withXxx`）加入 options 并默认 false。
- **⚠️ 不建议**往 `compute.worker.js:39-81` 旧 4 估算器白名单加分支，该分支已无调用方（analysis.js 全走 pipeline），保留仅为兼容。新增逻辑一律进 pipeline。

**同步回退与 Node harness 自动获得**：`analysis.js:375` 与 `test/pipeline-runner.mjs computeOutput` 都调同一个 `runAnalysisPipeline`，注册即三端生效，不需要额外接线。

---

## 步骤 3：analysis.js 消费侧接线（浏览器专属）

参考现有消费模式：

- 主结果：`pipelineResult.rework` / `.actualEstimatorAlgorithm`（analysis.js:491-492）。
- 附属段：`pipelineResult.xxxError != null` → 按旧条件 `errors.push(...)`；否则用 `pipelineResult.xxxResult`（analysis.js:561-580 interlude、:618-636 pattern、:655-688 ett 的既有模式）。
- ⚠️ **主线程回退分支**：pipeline 估算失败或保守开关未覆盖（override 后 need* 变真）时，保留主线程直接计算旧路径（如 analysis.js:629-635 pattern 回退、:670-687 ett 回退），新阶段必须同样补回退分支，否则 5K override 等边界会白屏。
- 渲染/缓存写门逻辑留在 analysis.js，不进 pipeline。

---

## 步骤 4：golden matrix 扩展（harness）

`test/pipeline-runner.mjs` 的 `computeOutput` 与 `SETTING_COMBOS`（任务 13 后为 options-only）：

- 新估算器：在 computeOutput 加阶段（keycount 分布见 `--matrix keys`；输出契约格式见 test/README.md"Per-sample output contract"）。
- 新选项/设置：加进 `SETTING_COMBOS` 后 `node test/capture-golden.mjs --matrix settings` 重新捕获（60 files，5 combos × 12）+ `node test/compare-golden.mjs --matrix settings` 比对。
- **全量门**：`node test/compare-golden.mjs` → `748 files compared, 0 diffs` + exit 0。
- ⚠️ golden 覆盖盲区：748 样本 100% 4K（`{"4": 748}`），6K/7K 路径零覆盖，6K/7K 改动用合成样本（temp/ 内）或浏览器冒烟补验（test/README.md"Keycount coverage gap"）。
- ⚠️ 修改估算器会改变独立仓库 `VSRG-DanEstimation-Benchmark` 的 benchmark 结果，需按 benchmark 流程验证；**不得读取 benchmark repo 的 samples/ 谱面数据**（防过拟合，CLAUDE.md:67）。

---

## 步骤 5：消息体量实测要求

新阶段若产生大体积输出（数组/对象）进入 worker 往返消息，**先实测再定去留**（reference 任务 12 graph 决策）：

1. 写 temp 脚本用 `structuredClone`（Node 模拟 postMessage）分别测"含新字段/不含"的克隆耗时。
2. 对照主线程替代方案成本（如重跑估算器 30~400ms）。
3. 结论写入文档：**bytes 占比 ≠ 耗时**。graph 占 99.3% 载荷但 clone 仅 0.6~3.2ms，留在 pipeline；若实测克隆耗时接近或超过主线程重算，才考虑移出（记录于 worker.md §7.2）。

---

## 步骤 6：文档同步清单

| 文档 | 何时更新 |
| --- | --- |
| [pipeline/worker.md](../pipeline/worker.md) | 管线阶段/选项/契约变化 |
| [pipeline/analysis-pipeline.md](../pipeline/analysis-pipeline.md) | 全链路数据流/接线变化 |
| [features/difficulty-estimation.md](../features/difficulty-estimation.md) | 新估计算法说明 |
| [features/pattern-analysis.md](../features/pattern-analysis.md) | pattern 相关阶段 |
| [guides/module-conventions.md](module-conventions.md) | 共享模块约束变化 |
| [guides/cache-invalidation.md](cache-invalidation.md) | 新计算影响设置 |
| [docs/README.md](../README.md) | 新增/删除文档时登记索引 |

破坏性更改（消息协议/输出契约/纯函数语义变化）另写 [breakings/](../breakings/README.md) 双语说明。

---

## 检查清单（动手前逐项确认）

- [ ] 纯度 grep 通过：`state\.|window\.|document\.` 无匹配（注释除外），未 import `js/app/`（§1.1）
- [ ] 入口签名 `(osuText, options = {}, parsed = null)`；局部变量未遮蔽 `parsed` 参数（§1.3）
- [ ] 选项全部显式透传，浏览器侧在 analysis.js:350-372 构造（§1.2）
- [ ] 新计算影响设置已加入 settings.js:844-857 失效链（§1.2）
- [ ] cvtFlag 转换在 clone 上进行，共享实例保持 pristine（§1.3）
- [ ] 输出 JSON-safe：无方法/getter（§1.4）
- [ ] 已注册到 runAnalysisPipeline 分派/阶段（§2）
- [ ] analysis.js 消费侧含主线程回退分支（§3）
- [ ] golden matrix 扩展 + 全量门 748/0（§4）
- [ ] 大体积输出已做消息体量实测（§5）
- [ ] 文档已同步 + breakings 双语说明（§6）
- [ ] 修改前已备份（CLAUDE.md 备份规则：破坏性编辑先拷到 `backup/<时间戳>-<描述>/`）
- [ ] 未修改 js 代码前用 lsp 引用检查确认改动面（`lsp_find_references`）

## 禁止模式

- **数学改写**：任何数值/公式/求值顺序改动都必须过 `node test/compare-golden.mjs` 全量门；共享数学函数只进 `reworkMathCore.js`（逐字抽取，见 module-conventions.md §2.2）。
- **共享对象变异**：不得原地修改传入的 parsed 实例/options/上游对象（worker 内无第二份拷贝，变异会静默改变其他消费段结果）。
- **f32 位置移动**：`Math.fround` 相关代码移动/重排会改变浮点舍入位（interlude 的 `f32(jackBpm(delta))` 模式），保持原位。
- **向 compute.worker.js 旧白名单加分支**：已无调用方，新增逻辑一律进 pipeline（§2）。
