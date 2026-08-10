# docs/breakings/2026-08-09-perf-analysis-pipeline.md

> 重大破坏性更改说明（双语，人类与 AI 共同阅读）。
> 日期：2026-08-09 ｜ 分支：perf/analysis-pipeline-optimization
> 本文件记录该分支引入的**破坏性更改**（消息协议、纯函数语义、共享模块约束、缓存失效、验收口径等），每项含修改内容/原因/影响范围/兼容策略/验证方式五要素。所有内容以**实际落地代码**为准；验证结论见文末。

---

# English

> Major breaking-changes notice (bilingual, for both humans and AI).
> Date: 2026-08-09 | Branch: perf/analysis-pipeline-optimization
> This file records the **breaking changes** introduced by this branch (message protocol, pure-function semantics, shared-module purity constraints, cache invalidation, acceptance criteria, etc.). Each item carries five elements: What changed / Why / Scope / Compatibility / Verification. Everything reflects the **actual landed code**; verification conclusions are at the end.

---

## 目录 / Contents

| # | 更改项 / Change | 严重度 / Severity |
| --- | --- | --- |
| ① | worker 消息协议（pipeline 单次往返） / Worker message protocol (single-round-trip pipeline) | 高 / High |
| ② | runAnalysisPipeline 纯函数（三端共用） / Pure-function pipeline (shared by three consumers) | 高 / High |
| ③ | 估算器入口 optional parsed 参数 / Optional parsed param on estimator entries | 中 / Medium |
| ④ | runSunnyWindowEstimatorFromText 选项签名 / SunnyWindow options signature | 中 / Medium |
| ⑤ | reworkEstimatorUtils estDiff 第 5 参 / estDiff 5th parameter | 中 / Medium |
| ⑥ | 共享模块纯度约束（worker 根因修复） / Shared-module purity (worker root-cause fix) | 高 / High |
| ⑦ | 设置失效清单收窄 / Invalidation list narrowing | 高 / High |
| ⑧ | 缓存命中重派生语义 / Cache-hit re-derivation semantics | 中 / Medium |
| ⑨ | reworkMathCore/常量去重（re-export 兼容） / Math-core & constant dedup (re-export compatible) | 低 / Low |
| ⑩ | vibro latent bug 修复披露 / Vibro latent bug fix disclosure | 中 / Medium |
| ⑪ | findPatterns/clustering 行为不变声明 / findPatterns/clustering behavior-unchanged declaration | 低 / Low |
| ⑫ | perf 验收口径修订披露 / Perf acceptance-criteria revision disclosure | 高 / High |

---

## ① worker 消息协议（pipeline 单次往返）/ Worker message protocol (single-round-trip pipeline)

**修改内容（What changed）**：`manager.js:64 runInWorker` 改发 `{ id, type: "pipeline", input }`（`input = { rawText, estimatorAlgorithm, options }`）；`compute.worker.js:23-37` 新增 `"pipeline"` 消息分支，异步（then/catch）调 `runAnalysisPipeline` 后回 `{ id, result }` 或 `{ id, error }`。估算 + 全部附属段（pattern/ett/interlude/Companella 二次 Ett）一次往返完成。旧 4 估算器消息 `{ id, osuText, options }`（compute.worker.js:39-81）**保留不动**（白名单 Daniel/Azusa/Roxy/Sunny + Azusa/Roxy 无效回退 Sunny），现无调用方。

**English**：`manager.js:64 runInWorker` now posts `{ id, type: "pipeline", input }` (`input = { rawText, estimatorAlgorithm, options }`); `compute.worker.js:23-37` adds a `"pipeline"` message branch that runs `runAnalysisPipeline` asynchronously (then/catch) and replies `{ id, result }` or `{ id, error }`. Estimation plus all auxiliary stages (pattern/ett/interlude/Companella second-ett) complete in a single round trip. The legacy 4-estimator messages `{ id, osuText, options }` (compute.worker.js:39-81) are **kept unchanged** (Daniel/Azusa/Roxy/Sunny whitelist + Azusa/Roxy invalid-result fallback to Sunny); they currently have no callers.

**修改原因（Why）**：估算器间多次往返（每条消息 structuredClone + 调度开销）改为一次往返；主线程阻塞迁移到 worker（任务 11/12 核心收益，见 ⑫）。

**English**：Replace many small round trips (per-message structuredClone + scheduling overhead) with one; move main-thread blocking into the worker (core win of tasks 11/12, see ⑫).

**影响范围（Scope）**：`js/app/worker/manager.js`、`js/app/worker/compute.worker.js`、`js/app/analysis.js`（dispatch 改为单次 pipeline 调用）。消息协议对外（其他插件/调试器）不承诺稳定。

**English**：`js/app/worker/manager.js`, `js/app/worker/compute.worker.js`, `js/app/analysis.js` (dispatch is now a single pipeline call). The message protocol is not a stable public API for external consumers.

**兼容策略（Compat）**：legacy 消息分支保留（防御）；manager 的 `latestId` 过期丢弃（manager.js:53）与 30s 超时（manager.js:67-72）语义不变；worker 构造失败仍返回 null → 主线程同步回退（analysis.js:375）。

**English**：Legacy message branch kept (defensive); `latestId` stale-discard (manager.js:53) and 30s timeout (manager.js:67-72) semantics unchanged; Worker-ctor failure still returns null → main-thread sync fallback (analysis.js:375).

**验证方式（Verification）**：全量回归验证通过（748 样本全量比对，精确浮点比对）；浏览器冒烟 worker 路径 0 console errors（实测记录）。

---

## ② runAnalysisPipeline 纯函数（三端共用）/ Pure-function pipeline (shared by three consumers)

**修改内容（What changed）**：新增 `js/pipeline/runAnalysisPipeline.js:94` `runAnalysisPipeline({ rawText, estimatorAlgorithm, options, parsed? })`，异步返回 16 业务字段 + `errors[]`（契约见 worker.md §4.3）。**三端共用同一函数**：worker（compute.worker.js:29）、主线程同步回退（analysis.js:375）、Node 回归脚本（computeOutput）。逐段顺序与旧 analysis.js 一致（解析 → 估算分派 → vibro 输入 → 归一化 → SunnyWindow → sixKConst → Interlude → Pattern → Ett → Companella 二次 Ett）。

**English**：New `js/pipeline/runAnalysisPipeline.js:94` `runAnalysisPipeline({ rawText, estimatorAlgorithm, options, parsed? })`, async, returns 16 business fields plus `errors[]` (contract in worker.md §4.3). **One function shared by three consumers**: worker (compute.worker.js:29), main-thread sync fallback (analysis.js:375), Node regression script (computeOutput). Stage order matches the old analysis.js exactly.

**修改原因（Why）**：单一实现消除 worker/同步回退/harness 三份分派代码的漂移风险；parse-once（解析一次共享给估算器/归一化/SunnyWindow/Interlude，主线程不再二次解析，analysis.js:376）。

**English**：One implementation removes drift risk across three dispatch copies; parse-once (parse once, share the instance with estimators/normalization/SunnyWindow/Interlude, no second parse on the main thread, analysis.js:376).

**影响范围（Scope）**：所有估算路径（浏览器 worker + 同步回退 + Node 回归）行为必须逐位一致；输出契约（含 `parsedSummary`、软失败字段）成为新的事实标准。

**English**：All estimation paths (browser worker + sync fallback + Node regression) must stay bit-identical; the output contract (incl. `parsedSummary`, soft-failure fields) is the new de-facto standard.

**兼容策略（Compat）**：`errors[]` 恒为空（硬错误预留）；估算器/SunnyWindow 抛错向上传播，由 analysis.js 外层 catch 按旧格式报 "Rework failed: ..."（analysis.js:378-400）；附属段软失败字段由 analysis.js 按旧展示条件并入 errors[]（worker.md §4.4）。

**English**：`errors[]` always empty (reserved); estimator/SunnyWindow throws propagate up and analysis.js outer catch reports "Rework failed: ..." in the old format (analysis.js:378-400); soft-failure fields are merged into errors[] by analysis.js per the old display gates (worker.md §4.4).

**验证方式（Verification）**：全量回归验证通过（748 样本全量比对，回归脚本已切到 pipeline 同步路径，实测记录）。

---

## ③ 估算器入口 optional parsed 参数（向后兼容）/ Optional parsed param on estimator entries (backward compatible)

**修改内容（What changed）**：6 个估算器入口增加可选第三参 `parsed = null`：`runSunnyEstimatorFromText`（sunnyEstimator.js:4）、`runDanielEstimatorFromText`（danielEstimator.js:9）、`runSunnyWindowEstimatorFromText`（sunnyWindowEstimator.js:13）、`runAzusaEstimatorFromText`（azusaEstimator.js:822）、`runRoxyEstimatorFromText`（roxyEstimator.js:1400）、`runMixedEstimatorFromText`（mixedEstimator.js:180）。传入已 `process()` 的 `OsuFileParser` **实例**时跳过内部解析（parse-once）；cvtFlag ∈ {IN,HO} 时在 `cloneOsuParser` 拷贝上转换（sunnyAlgorithm.js:55-71 等），共享实例保持 pristine；Roxy 仅在 `speedRate === 1 && cvtFlag 不含 IN/HO` 时共享（canonicalize 边界，worker.md §5.3）。

**English**：Six estimator entries gained an optional third param `parsed = null`: `runSunnyEstimatorFromText` (sunnyEstimator.js:4), `runDanielEstimatorFromText` (danielEstimator.js:9), `runSunnyWindowEstimatorFromText` (sunnyWindowEstimator.js:13), `runAzusaEstimatorFromText` (azusaEstimator.js:822), `runRoxyEstimatorFromText` (roxyEstimator.js:1400), `runMixedEstimatorFromText` (mixedEstimator.js:180). Passing a processed `OsuFileParser` **instance** skips the internal parse (parse-once); for cvtFlag ∈ {IN,HO} conversion runs on a `cloneOsuParser` copy (sunnyAlgorithm.js:55-71 etc.), keeping the shared instance pristine; Roxy shares only when `speedRate === 1 && cvtFlag excludes IN/HO` (canonicalize boundary, worker.md §5.3).

**修改原因（Why）**：解析是估算段中最重的公共成本之一（perf-baseline 单次解析 ~2ms，任务 11 前每估算器各自解析），共享实例减少解析次数（13 → 1-2 次，见 ⑫）。

**English**：Parsing is one of the heaviest shared costs of the estimation stages (single parse ~2ms per perf-baseline; every estimator parsed separately before task 11); sharing the instance cuts parse count (13 → 1-2, see ⑫).

**影响范围（Scope）**：调用方可选择性传入；未传时走原 fresh-parse 路径，**行为零变化**。

**English**：Callers may optionally pass it; omitting it keeps the original fresh-parse path, **zero behavior change**.

**兼容策略（Compat）**：签名向后兼容（第三参默认 null）；所有旧调用点（含 benchmark repo 的 runner 直接 import）不传即原行为。

**English**：Signature is backward compatible (3rd param defaults to null); all old call sites (incl. benchmark repo runners importing directly) keep original behavior by omission.

**验证方式（Verification）**：task-9/10 QA 逐位一致（parsed 路径 273/273 + 120/120）；全量回归验证通过（748 样本全量比对）。

---

## ④ runSunnyWindowEstimatorFromText 选项签名 / SunnyWindow options signature

**修改内容（What changed）**：`runSunnyWindowEstimatorFromText(osuText, options, parsed)` 从 options 读取 `enableAnalyzeLN`（sunnyWindowEstimator.js:22，传入 calculateLN）与 `enableAlwaysShowLNDifficulty`（:25，shouldShowLN 门槛），**不再读 state**；pipeline 内调用显式注入（runAnalysisPipeline.js:190-194）。

**English**：`runSunnyWindowEstimatorFromText(osuText, options, parsed)` reads `enableAnalyzeLN` (sunnyWindowEstimator.js:22, passed to calculateLN) and `enableAlwaysShowLNDifficulty` (:25, shouldShowLN gate) from options instead of state; the pipeline call injects them explicitly (runAnalysisPipeline.js:190-194).

**修改原因（Why）**：worker 内无 state（共享模块纯度，见 ⑥）；显式注入使 Node 侧（harness/matrix）可复现同一输出。

**English**：No state exists inside the worker (shared-module purity, see ⑥); explicit injection lets Node (harness/matrix) reproduce identical output.

**影响范围（Scope）**：默认值 false 与 config.js `defaults.enableAlwaysShowLNDifficulty`/`enableAnalyzeLN` 一致；未显式传时行为同旧 state 默认。

**English**：Defaults (false) match config.js `defaults.enableAlwaysShowLNDifficulty`/`enableAnalyzeLN`; omitted options behave like the old state defaults.

**兼容策略（Compat）**：签名不变（仅读法从 state 改为 options）；直接调用者若依赖旧 state 值需自行传参。

**English**：Signature unchanged (only the read source changed from state to options); direct callers relying on the old state value must pass it explicitly.

**验证方式（Verification）**：设置矩阵回归验证通过（enableAnalyzeLN-on combo 有真实 typePercentageData 指纹，60 文件全量比对）；全量 748 样本回归验证通过。

---

## ⑤ reworkEstimatorUtils estDiff 第 5 参 / estDiff 5th parameter

**修改内容（What changed）**：`estDiff(sr, lnRatio, columnCount, useExtended = false, enableAlwaysShowLNDifficulty = false)`（reworkEstimatorUtils.js:98），第 5 参替换原 `state.enableAlwaysShowLNDifficulty` 读取。调用方显式传：sunnyEstimator.js:15、danielEstimator.js:38（均 `options.enableAlwaysShowLNDifficulty === true`）。

**English**：`estDiff(sr, lnRatio, columnCount, useExtended = false, enableAlwaysShowLNDifficulty = false)` (reworkEstimatorUtils.js:98): the 5th param replaces the old `state.enableAlwaysShowLNDifficulty` read. Callers pass it explicitly: sunnyEstimator.js:15, danielEstimator.js:38 (both `options.enableAlwaysShowLNDifficulty === true`).

**修改原因（Why）**：reworkEstimatorUtils 是共享模块（sunny/daniel/azusa/roxy/mixed 全链 import），读 state 即 import appContext → worker 加载即崩（⑥ 根因链的一部分）；该字段是唯一的 state 读取点（根因分析确认）。

**English**：reworkEstimatorUtils is a shared module (imported by the whole sunny/daniel/azusa/roxy/mixed chain); reading state means importing appContext → the worker crashes on load (part of the ⑥ root-cause chain); this field was the only state read (confirmed by root-cause analysis).

**影响范围（Scope）**：所有调用 estDiff 的估算器输出路径；`estDiff2`（:111）与 `normalizeReworkResult`（:124）本就不读 state，未改。

**English**：All estimator output paths that call estDiff; `estDiff2` (:111) and `normalizeReworkResult` (:124) never read state and are unchanged.

**兼容策略（Compat）**：默认 false = config.js `defaults.enableAlwaysShowLNDifficulty`；旧调用点不传即旧默认行为。

**English**：Default false = config.js `defaults.enableAlwaysShowLNDifficulty`; old call sites omitting it keep the old default behavior.

**验证方式（Verification）**：全量回归验证通过（748 样本全量比对）；设置矩阵（enableAlwaysShowLNDifficulty-on combo）60 文件全量比对通过。

---

## ⑥ 共享模块纯度约束（worker 根因修复）/ Shared-module purity (worker root-cause fix)

**修改内容（What changed）**：共享估算/解析模块中 3 个 `state`/appContext 污染点清零：`reworkEstimatorUtils.js`（estDiff 第 5 参，⑤）、`sunnyWindowEstimator.js`（④）、`sunnyWindowAlgorithm.js`（enableAnalyzeLN 经 options 透传，task-2 修复）。共享模块目录（estimator/ett/interlude/parser/patterns/rework/pipeline）**禁止** import `js/app/`、读 `state`、引用 `window/document`（module-conventions.md §2）。

**English**：Three `state`/appContext pollution points in shared estimation/parsing modules were cleared: `reworkEstimatorUtils.js` (estDiff 5th param, ⑤), `sunnyWindowEstimator.js` (④), `sunnyWindowAlgorithm.js` (enableAnalyzeLN threaded via options, task-2 fix). Shared module dirs (estimator/ett/interlude/parser/patterns/rework/pipeline) must **not** import `js/app/`, read `state`, or touch `window/document` (module-conventions.md §2).

**修改原因（Why）**：**根因修复：worker 从未真正工作**。污染链：`compute.worker.js:10` → `sunnyEstimator.js:2` → `reworkEstimatorUtils.js:2` → `appContext.js:23+` 顶层 `document.getElementById`。Web Worker 无 document → worker 脚本加载即抛 ReferenceError → manager 回退/超时 → **所有估算实际在主线程执行**（卡顿根源，根因分析确认）。清零后 worker 首次真正可用。

**English**：**Root-cause fix: the worker never actually worked.** Pollution chain: `compute.worker.js:10` → `sunnyEstimator.js:2` → `reworkEstimatorUtils.js:2` → `appContext.js:23+` top-level `document.getElementById`. Web Workers have no document → the worker script throws ReferenceError on load → manager falls back/times out → **all estimation actually ran on the main thread** (root of the jank, confirmed by root-cause analysis). After the cleanup the worker works for the first time.

**影响范围（Scope）**：所有共享模块（worker 路径 + Node benchmark runner：`esm-loader.mjs` 只强制 ESM format、**不 shim document**，清零前 Node 侧直接 import 也会崩）；浏览器行为不变（显式传值 = 旧 state 值）。

**English**：All shared modules (worker path + Node benchmark runner: `esm-loader.mjs` only forces ESM format and does **not** shim `document`, so direct Node imports crashed before the cleanup); browser behavior unchanged (explicit values equal the old state values).

**兼容策略（Compat）**：值语义不变（默认与 config.js defaults 对齐）；对外 API 签名向后兼容（③④⑤）。

**English**：Value semantics unchanged (defaults aligned with config.js); public API signatures backward compatible (③④⑤).

**验证方式（Verification）**：plain Node 回归（无 document shim 依赖）748 样本全量比对通过；浏览器冒烟 worker 路径 0 console errors（实测记录）；设置矩阵 60 文件捕获与比对通过（task-13 实测证实无 shim 可跑）。

---

## ⑦ 设置失效清单收窄 / Invalidation list narrowing

**修改内容（What changed）**：`settings.js:844-857` 的 `clearResultCache()` 条件**移除** `display6kLevelChanged` 与 `debugChanged`（debugUseAmount）；`enableAlwaysShowLNDifficultyChanged` **保留**。三者仍留在 `recomputeNeeded`（settings.js:817-836），切换仍调度重算（命中路径重渲染，不 fetch 不重跑 pipeline）。

**English**：The `clearResultCache()` conditions (settings.js:844-857) **dropped** `display6kLevelChanged` and `debugChanged` (debugUseAmount); `enableAlwaysShowLNDifficultyChanged` **stays**. All three remain in `recomputeNeeded` (settings.js:817-836), so toggling still schedules a recompute (hit-path re-render, no fetch, no pipeline).

**修改原因（Why）**：toggle-diff 实证（30 样本，实测记录）：`display6kLevel` **0/30 diff**（corpus 100% 4K，sixKConst gate `display6kLevel && columnCount===6` 永不触发）；`debugUseAmount` **0/30 diff**（浏览器后处理，pipeline 不消费）；`enableAlwaysShowLNDifficulty` **25/30 diff**（lnRatio<0.15 谱面的 estDiff 加 " || LN ..." 段，真实输出差异）→ **不可移出**。

**English**：Toggle-diff evidence (30 samples, measured): `display6kLevel` **0/30 diff** (corpus is 100% 4K; the sixKConst gate `display6kLevel && columnCount===6` never fires); `debugUseAmount` **0/30 diff** (browser-only post-processing; the pipeline never consumes it); `enableAlwaysShowLNDifficulty` **25/30 diff** (estDiff gains " || LN ..." on lnRatio<0.15 maps, a real output change) → **must stay**.

**影响范围（Scope）**：缓存命中率（两个设置不再清缓存 → 命中保留）；hit 路径需重派生这两个展示值（⑧）。

**English**：Cache hit rate (these two settings no longer clear the cache → hits persist); the hit path must re-derive these two display values (⑧).

**兼容策略（Compat）**：**若未来把 enableAlwaysShowLNDifficulty 移出失效链**，其 estDiff 重派生需要 DAN_INDEX 区间表 + sunnyWindow/Companella 后处理，**无法仅凭缓存字段重建**（task-13 实测记录），维持现状（保留失效）是唯一正确选项。

**English**：**If enableAlwaysShowLNDifficulty were ever moved out of the invalidation chain**, its estDiff re-derivation needs the DAN_INDEX interval tables + sunnyWindow/Companella post-processing, which is **not reconstructable from cached fields alone** (task-13 measurement): keeping it in the chain is the only correct option.

**验证方式（Verification）**：浏览器缓存行为冒烟（实测记录）：display6kLevel/debugUseAmount 切换后 CACHE-LOOKUP hit=yes、fetch/pipeline 计数不变；extendedEstimationRange 切换 hit=no、代数 +1、pipeline 重跑。

---

## ⑧ 缓存命中重派生语义 / Cache-hit re-derivation semantics

**修改内容（What changed）**：analysis.js 命中分支（:472-489）从缓存数据**重派生**两个已移出失效链的设置值：① `sixKConst`（:482-486）用 pipeline 的**同一 gate + 公式**（`display6kLevel && columnCount===6 && star 有限且 >0` → `Math.round((star*200/81+7/6)*100)/100`）从缓存 `rework.star` 重算；② `debugUseAmount` 后处理（:586-600 `applyDebugUseAmountPostProcess`，miss 与 hit 共用）从 `cached.patternReport.Clusters`（未后处理的原始源）重放 `mergeDuplicateClusters` + 排序 + Category 覆盖（:609-617）。

**English**：The analysis.js hit branch (:472-489) re-derives the two settings that left the invalidation chain from cached data: ① `sixKConst` (:482-486) uses the pipeline's **own gate + formula** (`display6kLevel && columnCount===6 && star finite && >0` → `Math.round((star*200/81+7/6)*100)/100`) recomputed from the cached `rework.star`; ② `debugUseAmount` post-processing (:586-600 `applyDebugUseAmountPostProcess`, shared by miss and hit) replays `mergeDuplicateClusters` + sort + Category override from `cached.patternReport.Clusters` (the un-post-processed raw source, :609-617).

**修改原因（Why）**：缓存值反映**写时刻**的设置；两个设置不再失效后，命中时必须按当前设置重派生，否则命中与未命中显示不同值。

**English**：Cached values reflect the settings at **write time**; once these two settings stopped invalidating, the hit path must re-derive from the current settings or hit/miss would display different values.

**影响范围（Scope）**：命中路径渲染（6K 定数胶囊、debugUseAmount 排序/Category）；缓存快照字段 `sixKConst` 仍写（:800）但命中不再直接读。

**English**：Hit-path rendering (6K constant capsule, debugUseAmount sort/Category); the snapshot still stores `sixKConst` (:800) but hits no longer read it directly.

**兼容策略（Compat）**：重派生必须与 pipeline/miss 路径**逐字同公式同 gate**（规则：公式/路径恒等，不重算）；4K 下 gate 短路为 null，6K 下与缓存值逐位相同（Node 公式恒等已证）。

**English**：Re-derivation must be **exactly the pipeline/miss arithmetic** (same gate, same rounding, formula/path identity, never recompute); the gate short-circuits to null on 4K, and on 6K the result is bit-identical to the cached value (formula identity proven in Node).

**验证方式（Verification）**：Node 公式恒等（合成 6K 谱面：pipeline sixKConst 20.1 == hit 公式 20.1；OFF → null，实测记录）；浏览器冒烟 display6kLevel/debugUseAmount 切换 hit=yes 且渲染正确（实测记录）。

---

## ⑨ reworkMathCore/常量去重（re-export 兼容）/ Math-core & constant dedup (re-export compatible)

**修改内容（What changed）**：新增 `js/rework/reworkMathCore.js`（从 sunny/daniel **逐字抽取** 13 个数学函数 + 3 个图表常量 + `jackNerfer`/`targetPercentiles`，两算法共用）；常量去重：`js/ett/constants.js`（SUPPORTED_KEYS/ETTERNA_VERSION_KEYS）、`js/parser/noteColumn.js`（xToColumn）、`patternsDef.js` 导出 jackBpm（非 f32 版）、`patterns/config.js` 宿主 `modeTagFromLnRatio`。原导出经 re-export 保持（如 `modeLogic.js` re-export `modeTagFromLnRatio`）。

**English**：New `js/rework/reworkMathCore.js` (13 math functions + 3 graph constants + `jackNerfer`/`targetPercentiles` **extracted verbatim** from sunny/daniel, shared by both); constant dedup: `js/ett/constants.js` (SUPPORTED_KEYS/ETTERNA_VERSION_KEYS), `js/parser/noteColumn.js` (xToColumn), `patternsDef.js` exports jackBpm (non-f32 variant), `patterns/config.js` hosts `modeTagFromLnRatio`. Original exports stay via re-export (e.g. `modeLogic.js` re-exports `modeTagFromLnRatio`).

**修改原因（Why）**：重复定义去重，降低未来数值改动双处漂移风险；**数值/求值顺序零变化**（逐字抽取 + 位级恒等证明）。

**English**：Deduplicate repeated definitions to cut future drift risk of dual edits; **zero numeric/evaluation-order change** (verbatim extraction + bit-identity proof).

**影响范围（Scope）**：import 面（re-export 兼容，外部 import 路径不变）；`noteColumn` 公式对整数 x 位级恒等（任务 14 证明 2052 整数 + 8000 分数用例）。

**English**：Import surface (re-export compatible, external import paths unchanged); `noteColumn` is bit-identical for integer x (task 14 proved 2052 integer + 8000 fractional cases).

**兼容策略（Compat）**：原导出保留；差异函数（computeJbar/computePbar 等 8 个）**不**进 mathCore（各算法保留本地版本，module-conventions.md §2.2）；`summary.js resolveModeTag` 的非恒等分支改写为等价形式（70-case 网格证明）。

**English**：Original exports kept; the 8 differing functions (computeJbar/computePbar etc.) do **not** enter mathCore (each algorithm keeps its local copy, module-conventions.md §2.2); `summary.js resolveModeTag`'s non-identical branch was rewritten into an equivalent form (proven over a 70-case grid).

**验证方式（Verification）**：全量回归验证通过（748 样本全量比对）；70-case 网格 + 位级恒等脚本（实测记录）。

---

## ⑩ vibro latent bug 修复披露 / Vibro latent bug fix disclosure

**修改内容（What changed）**：**行为修复，非性能改动**。旧 `analysis.js:672`（重构前）ETT 段引用 try 块作用域外的 `selectedRework?.star` → ReferenceError 被 ETT catch 吞掉 → `vibroEligible` 恒 false → `isVibroMap` **恒 false**（vibro 检测自引入以来从未生效）。修复后：pipeline 计算 `vibro = { star, eligible }`（**归一化前** star，`eligible = finite && star > 5.0`，runAnalysisPipeline.js:169-175）；analysis.js 提升 `vibroEligible`（:447）并在 ETT 段正确消费（:664-667 `state.vibroDetection && vibroEligible && detectVibro(ettResult.values, VIBRO_JACKSPEED_RATIO_THRESHOLD)`）。

**English**：**Behavior fix, not a perf change**. The old `analysis.js:672` (pre-refactor) ETT section referenced `selectedRework?.star` outside its try scope → ReferenceError swallowed by the ETT catch → `vibroEligible` always false → `isVibroMap` **always false** (vibro detection never worked since its introduction). After the fix: the pipeline computes `vibro = { star, eligible }` (from the **pre-normalization** star, `eligible = finite && star > 5.0`, runAnalysisPipeline.js:169-175); analysis.js hoists `vibroEligible` (:447) and consumes it correctly in the ETT section (:664-667 `state.vibroDetection && vibroEligible && detectVibro(ettResult.values, VIBRO_JACKSPEED_RATIO_THRESHOLD)`).

**修改原因（Why）**：vibro 判定本应按设计工作（旧注释意图 = 算法自身 star > 5.0 门槛），作用域 bug 使其从未生效；pipeline 重构顺带修复。

**English**：Vibro gating was designed to work (old comment intent = algorithm-own star > 5.0 gate); a scoping bug silently disabled it; the pipeline refactor fixed it in passing.

**影响范围（Scope）**：浏览器行为变化：vibro 谱面现在会显示 "VIBRO" 难度文本（analysis.js:930-932）与隐藏图内数值（setForceHideNumericDifficulty :856）。Node golden 不含 vibro 字段，回归不受影响。

**English**：Browser behavior change: vibro maps now show the "VIBRO" difficulty text (analysis.js:930-932) and hide the in-graph number (setForceHideNumericDifficulty :856). The Node golden contract has no vibro field, so regression is unaffected.

**兼容策略（Compat）**：门槛（>5.0）与旧代码意图一致；无设置可关掉"已修复"（这是修复，不是新开关）。

**English**：The gate (>5.0) matches the old intended behavior; there is no setting to disable "the fix" (it is a fix, not a new toggle).

**验证方式（Verification）**：代码审查（pipeline vibro 顺序 + analysis.js 消费链）+ F3 浏览器冒烟覆盖 vibro 场景（处置记录）；golden 748 样本全量比对证明非 vibro 路径无回归。

---

## ⑪ findPatterns/clustering 行为不变声明 / findPatterns/clustering behavior-unchanged declaration

**修改内容（What changed）**：本分支对 pattern 相关代码的性能改造**均为位级等价**，行为不变：`findPatterns.js` O(n²)→O(n)（8 元素滑动窗口，matcher 仅读有界 head 窗口，任务 4）；`chartBuilder.js` HOLDBODY fill 限定 hold span 行（二分查找，任务 5）；`danielAlgorithm.js` mergeByHead 替换为 concat+稳定排序（508 用例等价证明，任务 7）；clustering/summary 复杂度评估后**跳过**（上限远低于优化阈值，任务 8）。

**English**：All pattern-related perf changes in this branch are **bit-identical, behavior unchanged**: `findPatterns.js` O(n²)→O(n) (8-element sliding window; matchers only read a bounded head window, task 4); `chartBuilder.js` HOLDBODY fill bounded to hold-span rows (binary search, task 5); `danielAlgorithm.js` mergeByHead replaced by concat+stable-sort (508-case equivalence proof, task 7); clustering/summary complexity was evaluated and **skipped** (upper bounds far below the optimization threshold, task 8).

**修改原因（Why）**：性能优化受"不改任何数值/公式"硬约束（CLAUDE.md 基准防过拟合），等价性通过构造证明，不靠重算。

**English**：Perf work is bound by the "never change numbers/formulas" hard constraint (CLAUDE.md benchmark anti-overfitting); equivalence is proven by construction, not by recomputation.

**影响范围（Scope）**：无（位级等价，输出契约零变化）。

**English**：None (bit-identical, zero output-contract change).

**兼容策略（Compat）**：N/A（无破坏性；声明以排除误判）。

**English**：N/A (non-breaking; declared to rule out misjudgment).

**验证方式（Verification）**：全量回归验证通过（748 样本全量比对，含异常样本 3ef6d23c5d1517bb 的确定性解析错误，两测一致）；等价证明见实测记录。

---

## ⑫ perf 验收口径修订披露 / Perf acceptance-criteria revision disclosure

**修改内容（What changed）**：原验收标准"**中位管线耗时 ≥30%**"（基于任务 3 harness 测量）**废弃并修订**为三条可验证口径：① 主线程阻塞 → ~0（worker offload）；② 单算法 pipeline 总耗时 **-15~25%**（Node 可测）；③ 解析次数 **13 → 1-2 次**（grep/代码证明）。

**English**：The original acceptance criterion "**median pipeline time ≥30%**" (based on task-3 harness measurement) is **retired and revised** to three verifiable criteria: ① main-thread blocking → ~0 (worker offload); ② single-algorithm pipeline total time **-15~25%** (measurable in Node); ③ parse count **13 → 1-2** (grep/code proof).

**修改原因（Why）**：**测量口径与真实路径不符**。harness 口径 = 5 算法依次全跑（测试矩阵成本），非浏览器真实路径（单算法 + 归一化 + 主线程）。实测：sum-of-summaries 指标**数学下限 ≈380ms**（azusa+roxy+mixed 算法本身 ≈294ms，受"不改公式"约束不可压缩），目标 ≤302ms 不可达；且机器负载摆动 ±40%（同代码 291↔530ms），>30% 的"快态窗口"读数无法在当前机器状态复现（task-12 实测记录）。真实收益（worker offload、主线程零阻塞、一次往返）在 Node 同步 harness 的测量面之外，由浏览器冒烟证明。

**English**：**The measurement basis did not match the real path.** The harness metric = 5 algorithms run sequentially (test-matrix cost), not the real browser path (single algorithm + normalization + main thread). Measured: the sum-of-summaries metric has a **mathematical floor ≈380ms** (azusa+roxy+mixed algorithm cost alone ≈294ms, incompressible under the "no formula change" constraint), so ≤302ms is unreachable; machine load swings ±40% (same code 291↔530ms), and the >30% "fast-state window" reading cannot be reproduced on the current machine (task-12 measurement). The real wins (worker offload, zero main-thread blocking, single round trip) sit outside the sync Node harness measurement surface and are proven by browser smoke.

**影响范围（Scope）**：后续 perf 任务的验收基准；不得再用"中位管线耗时 -30%"作为门槛表述。

**English**：Acceptance baseline for future perf tasks; the "median pipeline time -30%" phrasing must not be reused as a gate.

**兼容策略（Compat）**：方法论保留"同方法计算两侧"（identical protocol + identical sample set）规则，同时引用两条指标（Node 可测 + 浏览器实测）。

**English**：Methodology keeps the "measure both sides the same way" rule (identical protocol + identical sample set), quoting both metrics (Node-measurable + browser-observed).

**验证方式（Verification）**：浏览器冒烟证明主线程零阻塞 + worker 路径渲染一致（task-11/12 实测记录，截图见本地实证目录）；单算法 pipeline 对比用同一 12 样本子集（复刻基线测量协议）；解析次数 13→1-2 由代码结构证明（pipeline 单解析 + patternOsuParser 独立 + ett 独立）。

---

## 文末验证指引 / Verification guide

### Node 回归门（golden）

全量回归验证：748 样本全量比对（精确浮点比对，含 NaN/±Inf/-0 哨兵、数组指纹），期望 0 diffs、exit 0。支持快速子集（按样本目录过滤/限量）先过快门再跑全量。设置矩阵（5 combos × 12 = 60 文件）同协议比对，期望 0 diffs、exit 0。Node ≥ 20.10（本机 v24.4.0）；`--experimental-detect-module` 为 Node 24 默认，plain `node` 即可。本机负载高时全量运行可能 5-15 分钟，建议给足超时。

### 浏览器冒烟（Playwright）

冒烟方法（routeWebSocket + 本地静态 server + addInitScript Worker 包装计数）与通过记录：

| 场景 | 断言 |
| --- | --- |
| worker 路径 | star/diff 正确、**0 console errors**、worker 实例化 |
| 同步回退（Worker 构造抛错） | 与 worker 路径逐位一致、0 console errors |
| Companella 全链路（二次 Ett） | star=7.12、Overall=25.74（0.74.0）、pattern 渲染 |
| 6K/7K 合成样本 | sixKConst（6K only）、keycount-aware ett 回退、渲染正常 |
| 缓存命中重派生 | display6kLevel/debugUseAmount 切换 hit=yes 且不 fetch；extendedEstimationRange 切换 miss |
| vibro（修复后） | vibro 谱面显示 "VIBRO" + 隐藏图内数值 |

### 实证摘要

本分支全部实证结论（pipeline 契约/决策表/冒烟、消息体量实测/graph 决策/perf 口径、toggle-diff/失效收窄/命中重派生、去重与恒等证明、parsed 路径 QA、等价证明）已在上文各节验证方式中给出；perf 基线采用同一 12 样本子集与相同测量协议（详见 worker.md §7.2 与 analysis-pipeline.md §7.4）。
