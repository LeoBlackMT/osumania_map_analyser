# docs/breakings/2026-08-16-preset-system-and-settings-schema.md

> English version [below](#english).

# 中文

> 重大破坏性更改说明（双语，人类与 AI 共同阅读）。
> 日期：2026-08-16 ｜ 分支：pr/45（预设系统）
> 基线：本文件描述 **相对 main 分支（403bae0）的净变更**。main 上不存在任何预设系统，以下全部为 PR 新增/改动。预设数据**直接写入 tosu 的 `presetStorage` 设置**——不存在任何"从 localStorage 迁移预设"的路径（main 的 localStorage 仅用于遥测/更新检查/调试面板，与预设无关）。
> 每项含修改内容/原因/影响范围/兼容策略/验证方式五要素；所有内容以**实际落地代码**为准，验证结论见文末。

---

## 目录

| # | 更改项 | 严重度 |
| --- | --- | --- |
| ① | 新增预设系统（presets/ 模块 + presets.html + 内置预设文件） | 高 |
| ② | 设置命令监听器数据化（SETTING_HANDLERS） | 中 |
| ③ | settings.json 新增预设分组与按钮（7 个 uniqueID，合并双语 Guide 按钮） | 低 |
| ④ | 预设库存于 tosu presetStorage 设置（跨 origin 单一权威，新增） | 高 |
| ⑤ | 预设快照系统键剥离（新增系统的内部约束，防递归膨胀） | 高 |
| ⑥ | 写回幂等化（新增系统的内部机制，杀广播→POST 循环） | 高 |

---

## ① 新增预设系统（presets/ 模块 + presets.html + 内置预设文件）

**修改内容（What changed）**：相对 main 新增完整预设系统：
- 新模块目录 `ManiaMapAnalyser by Leo_Black/js/app/presets/`（core.js / schema.js / storage.js / io.js / manager.js / index.js）。
- 新页面 `ManiaMapAnalyser by Leo_Black/presets.html` + 新样式 `styles/presets.css`（预设管理器 UI）。
- 新静态目录 `ManiaMapAnalyser by Leo_Black/presets/`（index.json 清单 + 10 个内置预设文件，每文件 = 元数据 + settings 覆盖）。
- `js/app/main.js` 新增副作用导入 `./presets/index.js`（恰好一次，注册预设设置流监听）。
- `js/app/socket.js` 的 `sendCommand` 增加重试上限（命令 socket 未就绪时最多 ~2s 放弃），缓解页面加载时的 getSettings 重试风暴。

**修改原因（Why）**：预设系统是 PR 的核心功能——一键应用/保存整套配置、部分快照、导入导出、自动跟随手动修改。tosu 原无此能力。

**影响范围（Scope）**：新增文件全部为增量（main 无同名文件）；`main.js`/`socket.js` 为既有文件改动。游戏内 overlay 页面（index.html）加载时也会执行预设模块的初始化（监听设置流 + 从 presetStorage 拉取库）。

**兼容策略（Compat）**：main 已有功能（分析、估算、缓存、设置）行为不变；预设系统默认不干扰——未使用预设的用户行为与 main 一致（除 `main.js` 多一次模块导入与 `socket.js` 重试上限，见 ⑥）。

**验证方式（Verification）**：sim 模拟套件 22 场景全过（含预设 CRUD、广播收敛、幂等、快照清洗）；浏览器冒烟 presets.html 零 console errors，36 个设置行渲染。

---

## ② 设置命令监听器数据化（SETTING_HANDLERS）

**修改内容（What changed）**：`js/app/settings.js` 运行时命令监听器（`setupSettingsCommandListener`）从 **main 的逐设置硬编码 applyIf 链**（main 版本：35 行 `const xxxChanged = applyIf("xxx", ...)` + 手工 `changed`/`recomputeNeeded` OR 聚合 + 手工 `clearResultCache()` 条件）重构为**数据表驱动**：新增 `SETTING_HANDLERS`（36 行 `{ key, parse, apply }`）、`SETTING_RECOMPUTE_KEYS`（Set）、`SETTING_CACHE_KEYS`（Set）。监听器循环遍历表：`changedMap[key] = applyIf(key, apply, parse(payload))`，再按集合判断 `changed`/`recomputeNeeded`/缓存失效。

**修改原因（Why）**：预设系统（schema.js / core.js）需要"哪些键触发重算 / 缓存失效"的单一来源——main 的硬编码链无法被外部模块复用。数据化后预设模块直接 import 两个集合，新增设置只改表 + parse/apply 对。

**影响范围（Scope）**：`js/app/settings.js`（监听器 + 两个导出集合）、`js/app/presets/{schema,core}.js`（import 集合）。`applySettingsFrom`（启动基线链）**保持手工逐行**——运行时已数据化、启动未动（不对称有意：启动基线无 hasKey 守卫需求）。

**兼容策略（Compat）**：对外行为（parse→apply、hasKey 守卫、recompute/cache 语义）与 main 逐项等价；`SETTING_RECOMPUTE_KEYS` 对应 main 的 recomputeNeeded 列表，`SETTING_CACHE_KEYS` 对应 main 的 clearResultCache 列表（含 wsEndpoint 只在 changed 的例外）。

**验证方式（Verification）**：浏览器冒烟 dashboard 保存行为与 main 一致；sim 套件断言键集一致；语法检查通过。

---

## ③ settings.json 新增预设分组与按钮（7 个 uniqueID，合并双语 Guide 按钮）

**修改内容（What changed）**：settings.json 相对 main 净增 7 个 uniqueID：新分组 `hPresets`、`preset`（预设下拉）、`presetStorage`（内部存储，Debug 区）、`GuideButton`、`PresetGuideButton`、`PresetButton`、`DebugButton`。同时将 main 的 `GuideButtonEN`/`GuideButtonCN` 两个按钮合并为单一 `GuideButton`。

**修改原因（Why）**：预设选择入口 + 存储可见性 + 文档/工具按钮入口；双语按钮合并简化维护。

**影响范围（Scope）**：settings.json UI 定义（46 → 51 uniqueID）。`preset`/`presetStorage` 为内部字段，普通用户无需修改。

**兼容策略（Compat）**：全英文、Link 部分置前、checkbox 在 options 前（CLAUDE.md 要求）。`GuideButtonEN/CN` 移除——旧配置若引用这两个键，tosu 设置界面会忽略未知键，无破坏性影响（用户可见的变化仅是"两个指南按钮变为一个"）。

**验证方式（Verification）**：tosu 设置面板渲染正常，预设下拉可用；`GuideButton` 指向 docs/presets-guide.md 可点击。

---

## ④ 预设库存于 tosu presetStorage 设置（跨 origin 单一权威，新增）

**修改内容（What changed）**：预设库、激活预设名、写回去重队列全部存入 tosu 的 `presetStorage` 文本设置（settings/`<插件目录名>`.values.json），store 结构 v2 `{ v: 2, lastWritten: [...], presets: [...] }`。所有页面（localhost 与 127.0.0.1 两个 origin、浏览器标签、游戏内 overlay）通过 tosu 的 getSettings 广播共享同一份库。读取侧兼容旧 v1 裸数组（内存迁移，仅用于分支早期版本遗留数据）。

**修改原因（Why）**：localhost 与 127.0.0.1 是不同 origin；若用浏览器 localStorage 则彼此隔离，自定义预设在不同入口不一致。tosu 设置随 values.json 走、跨 origin 广播一致，且随 tosu 配置目录迁移。**main 的 localStorage 仅用于遥测/更新检查/调试面板，与预设无关，本 PR 不读写这些键。**

**影响范围（Scope）**：`js/app/presets/storage.js`（store v2 读写 + v1 兼容读）、`core.js`（首包/广播从 presetStorage 同步 store）。

**兼容策略（Compat）**：读取侧兼容 v1 裸数组（内存迁移，针对分支早期版本）；写入侧只写 `presetStorage` 设置；不动 main 的任何 localStorage 键。

**验证方式（Verification）**：sim 套件覆盖 v2 格式、跨 origin HTTP 拉取一致性、写回携带 store；浏览器实测 localhost 与 127.0.0.1 看到同一库。

---

## ⑤ 预设快照系统键剥离（新增系统的内部约束，防递归膨胀）

**修改内容（What changed）**：`storage.js` 定义 `SYSTEM_SNAPSHOT_KEYS = { presetStorage, preset }`。所有快照构建点（`snapshotOf`、`stripSystemKeys`、`sanitizePresets`、`cleanLastWritten`、`writeBackToTosu` body）均剥离这两个系统键；加载时 `sanitizePresets`/`cleanLastWritten` 清洗历史污染。**这是新增预设系统的内部设计约束，不属于对 main 的破坏。**

**修改原因（Why）**：开发中曾出现把 `presetStorage`（整个库）当普通设置捕获进预设快照的问题，库内嵌预设、预设内嵌库，**递归膨胀**——实测 values.json 达 252MB，每次 getSettings 解析耗时数秒，谱面分析 100ms → 1-2s。剥离后 store 收敛（实测 126MB → 10KB）。此修复随预设系统首次发布，保证新功能不会把 values.json 撑爆。

**影响范围（Scope）**：`js/app/presets/{storage,core}.js`。已污染的 values.json 首次加载时由首包自愈逻辑**写回一次**收缩（仅影响已使用过分支早期版本的测试环境）。

**兼容策略（Compat）**：读取侧自动清洗历史污染（预设内容保留，仅删系统键）；已存在的超大 values.json 首次加载自动收缩；main 用户首次升级无此问题（main 无预设库）。

**验证方式（Verification）**：sim 套件新增 S22（自动保存快照无系统键、污染 store 自愈）；对真实 252MB values.json 干跑清洗：presetStorage 126MB → 10KB，文件 252MB → ~30KB，5 个预设全部保留。

---

## ⑥ 写回幂等化（新增系统的内部机制，杀广播→POST 循环）

**修改内容（What changed）**：`storage.js` 新增 `storeFingerprint`（剥离 lastWritten 时间戳的内容指纹）；`core.js` 的 `persistLibrary` 在指纹与上次实际写入一致时跳过 POST；首包/广播 store 同步后重对齐指纹；`isWriteBackEcho` 从按 preset 字段匹配改为按设置内容匹配。**这是新增预设系统的内部机制，不属于对 main 的破坏。**

**修改原因（Why）**：预设系统的多页面写回（游戏内 overlay iframe + 浏览器标签 + dashboard）若不加幂等会形成**广播→POST→广播无限循环**（开发中实测每 ~2.4s 一个 POST 成功，服务器排队 34s）。幂等后内容未变不 POST，循环根断。`socket.js` 重试上限同样为降低请求风暴（① 提及）。

**影响范围（Scope）**：`js/app/presets/{storage,core}.js`、`js/app/socket.js`。广播风暴与服务器过载消除。

**兼容策略（Compat）**：真实变更仍立即写回（指纹变化即 POST）；页面加载零 POST（库未变时）；缺槽位批量创建仅一次 POST。

**验证方式（Verification）**：sim 套件 S20/S21（双页面收敛、重复广播零 POST、完整 store 首包零 POST）；生产实测 getSettings 广播风暴消失、谱面分析恢复 ~100ms 级。

---

## 验证结论（Verification Summary）

- 回归：sim 模拟套件 **22 场景全部 PASS**（含预设 CRUD、S20 双页面收敛、S21 幂等、S22 系统键剥离）；所有 presets 模块 `node --check` 语法通过；index.js 与 metadata.txt 版本一致（1.7.4）。
- 实测：真实 252MB 污染 values.json 干跑清洗成功（126MB → 10KB）；用户环境部署后确认"已完全正常"（谱面分析恢复 ~100ms 级，日志风暴消失）。
- 浏览器冒烟：presets.html 加载零 console errors，36 个设置行渲染，Guide 按钮/自定义滚动条/checkbox 尺寸一致生效。

---

# English

> Breaking-changes document (bilingual, for humans and AI).
> Date: 2026-08-16 ｜ Branch: pr/45 (preset system)
> Baseline: this document describes the **net changes relative to main (403bae0)**. main has no preset system at all; everything below is added/modified by this PR. Preset data is **written directly into tosu's `presetStorage` setting** — there is no "migration from localStorage" path (main's localStorage is only used by telemetry/update-checker/debug panel, unrelated to presets).
> Each item carries the five elements: What changed / Why / Scope / Compat / Verification. All content reflects **actual landed code**; conclusions at the end.

---

## Table of Contents

| # | Change | Severity |
| --- | --- | --- |
| ① | New preset system (presets/ modules + presets.html + built-in preset files) | High |
| ② | Settings command listener data-driven (SETTING_HANDLERS) | Medium |
| ③ | settings.json gains preset group & buttons (7 uniqueIDs, merged bilingual Guide button) | Low |
| ④ | Preset library stored in tosu presetStorage setting (cross-origin single source of truth, new) | High |
| ⑤ | Preset snapshot system-key stripping (new-system internal constraint, recursive bloat fix) | High |
| ⑥ | Idempotent write-back (new-system internal mechanism, kills broadcast→POST loop) | High |

---

## ① New preset system (presets/ modules + presets.html + built-in preset files)

**What changed**: Relative to main, a complete preset system is added:
- New module directory `ManiaMapAnalyser by Leo_Black/js/app/presets/` (core.js / schema.js / storage.js / io.js / manager.js / index.js).
- New page `ManiaMapAnalyser by Leo_Black/presets.html` + new stylesheet `styles/presets.css` (preset manager UI).
- New static directory `ManiaMapAnalyser by Leo_Black/presets/` (index.json manifest + 10 built-in preset files, each = metadata + settings overrides).
- `js/app/main.js` gains a side-effect import `./presets/index.js` (exactly once, registers the preset settings-stream listener).
- `js/app/socket.js` `sendCommand` gains a retry cap (~2s when the command socket is not ready), reducing getSettings retry storms on page load.

**Why**: The preset system is the PR's core feature — one-click apply/save of full configurations, partial snapshots, import/export, auto-follow of manual edits. tosu had no such capability.

**Scope**: All new files are additive (no same-name files on main); `main.js`/`socket.js` are modifications to existing files. The in-game overlay page (index.html) also runs the preset module initialization (settings-stream listener + presetStorage pull).

**Compat**: Existing main functionality (analysis, estimation, cache, settings) behaves unchanged; the preset system does not interfere by default — users who never touch presets see main-equivalent behavior (aside from one extra module import in main.js and the socket retry cap, see ⑥).

**Verification**: Sim test suite — 22 scenarios all pass (preset CRUD, broadcast convergence, idempotency, snapshot cleaning); browser smoke of presets.html with zero console errors and 36 setting rows rendered.

---

## ② Settings command listener data-driven (SETTING_HANDLERS)

**What changed**: The runtime command listener in `js/app/settings.js` (`setupSettingsCommandListener`) was refactored from **main's hardcoded per-setting applyIf chain** (main version: 35 `const xxxChanged = applyIf(...)` lines + manual `changed`/`recomputeNeeded` OR aggregation + manual `clearResultCache()` conditions) to a data-table-driven approach: new `SETTING_HANDLERS` (36 `{ key, parse, apply }` rows), `SETTING_RECOMPUTE_KEYS` (Set), `SETTING_CACHE_KEYS` (Set). The listener loops the table: `changedMap[key] = applyIf(key, apply, parse(payload))`, then derives `changed`/`recomputeNeeded`/cache invalidation from the sets.

**Why**: The preset system (schema.js / core.js) needed a single source of truth for "which keys trigger recompute / cache invalidation" — main's hardcoded chain was not reusable by external modules. After the refactor, preset modules import the two sets directly; adding a setting only touches the table + parse/apply pair.

**Scope**: `js/app/settings.js` (listener + two exported sets), `js/app/presets/{schema,core}.js` (import the sets). `applySettingsFrom` (startup baseline chain) **stays manual** — the runtime path is data-driven, the startup path is not (asymmetry intentional: startup baseline needs no hasKey guard).

**Compat**: Per-setting behavior (parse→apply, hasKey guard, recompute/cache semantics) is equivalent to main; `SETTING_RECOMPUTE_KEYS` corresponds to main's recomputeNeeded list and `SETTING_CACHE_KEYS` to main's clearResultCache list (including the wsEndpoint changed-only exception).

**Verification**: Browser smoke — dashboard save behaves identically to main; sim suite asserts consistent key sets; syntax check passes.

---

## ③ settings.json gains preset group & buttons (7 uniqueIDs, merged bilingual Guide button)

**What changed**: settings.json adds 7 uniqueIDs relative to main: new group `hPresets`, `preset` (dropdown), `presetStorage` (internal storage, Debug section), `GuideButton`, `PresetGuideButton`, `PresetButton`, `DebugButton`. Also merges main's `GuideButtonEN`/`GuideButtonCN` into a single `GuideButton`.

**Why**: Preset selection entry point + storage visibility + documentation/tool button entries; merging the bilingual buttons simplifies maintenance.

**Scope**: settings.json UI definition (46 → 51 uniqueIDs). `preset`/`presetStorage` are internal fields ordinary users need not touch.

**Compat**: All English, Links section first, checkboxes before options (CLAUDE.md requirement). `GuideButtonEN/CN` are removed — tosu ignores unknown keys in existing configs, so no breaking impact (the only user-visible change is "two guide buttons become one").

**Verification**: tosu settings panel renders correctly; the preset dropdown works; the Guide button points to docs/presets-guide.md and is clickable.

---

## ④ Preset library stored in tosu presetStorage setting (cross-origin single source of truth, new)

**What changed**: The preset library, active preset name, and write-back dedup queue all live in tosu's `presetStorage` text setting (settings/`<folder>`.values.json), store shape v2 `{ v: 2, lastWritten: [...], presets: [...] }`. Every page (localhost and 127.0.0.1 origins, browser tabs, in-game overlay) shares the same library through tosu's getSettings broadcast. Read side accepts legacy v1 bare arrays (in-memory migration, only for early branch data).

**Why**: localhost and 127.0.0.1 are different origins; browser localStorage would be isolated between them, making custom presets diverge per entry point. tosu settings travel with values.json, broadcast consistently across origins, and move with the tosu config directory. **main's localStorage is only used by telemetry/update-checker/debug panel — unrelated to presets; this PR does not touch those keys.**

**Scope**: `js/app/presets/storage.js` (store v2 read/write + v1 compatible read), `core.js` (sync store from presetStorage on first batch/broadcast).

**Compat**: Read side accepts legacy v1 bare arrays (in-memory migration, for early branch versions); write side only writes the `presetStorage` setting; no main localStorage keys are touched.

**Verification**: Sim suite covers v2 format, cross-origin HTTP pull consistency, write-back carrying the store; browser test shows localhost and 127.0.0.1 see the same library.

---

## ⑤ Preset snapshot system-key stripping (new-system internal constraint, recursive bloat fix)

**What changed**: `storage.js` defines `SYSTEM_SNAPSHOT_KEYS = { presetStorage, preset }`. All snapshot construction points (`snapshotOf`, `stripSystemKeys`, `sanitizePresets`, `cleanLastWritten`, `writeBackToTosu` body) strip these two keys; `sanitizePresets`/`cleanLastWritten` also clean historical pollution on load. **This is an internal design constraint of the new preset system, not a breaking change to main.**

**Why**: During development, capturing `presetStorage` (the whole store) into preset snapshots caused the store to embed presets and presets to embed the store, **recursively inflating** — measured at 252MB for values.json, making every getSettings parse take seconds and beatmap analysis drop from 100ms to 1-2s. After stripping, the store converges (measured 126MB → 10KB). This fix ships with the preset system's first release so the new feature never bloats values.json.

**Scope**: `js/app/presets/{storage,core}.js`. A polluted values.json is written back exactly once by the first-batch self-heal on the next load (only affects test environments that used early branch versions).

**Compat**: Read side auto-cleans historical pollution (preset content kept, only system keys removed); an existing oversized values.json shrinks automatically on first load; main users upgrading for the first time are unaffected (main has no preset library).

**Verification**: Sim suite adds S22 (auto-save snapshot free of system keys, polluted store self-heals); dry-run on the real 252MB values.json: presetStorage 126MB → 10KB, file 252MB → ~30KB, all 5 presets kept.

---

## ⑥ Idempotent write-back (new-system internal mechanism, kills broadcast→POST loop)

**What changed**: `storage.js` adds `storeFingerprint` (content fingerprint with lastWritten timestamps stripped); `core.js` `persistLibrary` skips the POST when the fingerprint matches the last actually-written payload; the first batch and broadcast store sync re-align the fingerprint; `isWriteBackEcho` now matches on settings CONTENT instead of the preset field. **This is an internal mechanism of the new preset system, not a breaking change to main.**

**Why**: Multi-page write-back in the preset system (in-game overlay iframe + browser tab + dashboard), without idempotency, formed a **broadcast→POST→broadcast infinite loop** (measured during development: one POST succeeded every ~2.4s, with the server queueing requests for 34s). Idempotency means unchanged content never POSTs, breaking the loop at the root. The `socket.js` retry cap serves the same goal (reducing request storms, see ①).

**Scope**: `js/app/presets/{storage,core}.js`, `js/app/socket.js`. Broadcast storms and server overload are eliminated.

**Compat**: Real changes still write back immediately (fingerprint change → POST); page load issues zero POSTs when the library is unchanged; missing anchor slots are created with a single POST.

**Verification**: Sim suite S20/S21 (two-page convergence, repeated identical broadcasts zero POSTs, full-store hydration zero POSTs); production testing shows the getSettings broadcast storm gone and beatmap analysis back to ~100ms.

---

## Verification Summary

- Regression: sim test suite **22 scenarios all PASS** (preset CRUD, S20 two-page convergence, S21 idempotency, S22 system-key stripping); all presets modules pass `node --check`; index.js and metadata.txt versions match (1.7.4).
- Measured: real 252MB polluted values.json dry-run cleanup succeeded (126MB → 10KB); after production deployment the user confirmed "completely normal" (beatmap analysis back to ~100ms, log storm gone).
- Browser smoke: presets.html loads with zero console errors, 36 setting rows rendered, Guide button / custom scrollbars / checkbox sizing verified.
