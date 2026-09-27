# 2026-09-20 Etterna 错误体验与 Companella 胶囊（Etterna error UX and the Companella capsule）

## 中文

### 修改内容（What changed）

- **MinaCalc 中止（abort）友好化**（`js/ett/calc.js`）：捕获 Emscripten abort（"Aborted(). Build with -sASSERTIONS for more info."），把该版本的 wasm 模块从缓存中丢弃（abort 之后实例已不可用，否则后续谱面会连续失败），并抛出具错误码 `code = "minacalc-aborted"` 与可读文案的错误；原始文本保留在 `error.cause`。`runAnalysisPipeline.js` 新增契约字段 `ettErrorCode`，`analysis.js` 据此把卡片渲染为 **"Unsupported Chart"**（与既有 "Unsupported Keycount" 同一约定），`errors[]` 文案改为 `Etterna analyze failed: MinaCalc aborted on this chart (structure or density out of range)`。
- **junk file 显式化**：MinaCalc 对"荒唐密度"谱面（例如 1 秒 150+ 行）不报错，而是打印 `skipping junk file` 并返回**全 0 技能值**。`calc.js` 现在输出 `junkFile`（要求 ≥32 行且所有展示技能值**四舍五入到 0.00**，即绝对值 < 0.005——显示层只保留两位小数，0.004 与 0 对用户没有区别）与 `rowCount`。展示层的四个位置全部覆盖：① Etterna 段渲染为 `MSD unavailable (MinaCalc junk file)`；② 右侧数值胶囊与分隔符显示 `-`/`--`；③ **左上胶囊的 MSD 模式**不再显示 `0.00 MSD`，回退为星数胶囊（与"无 Ett 结果"一致）；④ **metadata 状态行给出红色提示**（junk 谱往 `errors[]` 推入 `Etterna MSD unavailable (MinaCalc junk file)`，走既有的 metadata 红字通道）——因为卡片主体未必是 Etterna 段，只看主体用户不知道发生了什么。
- **结果缓存键版本 bump**：`CACHE_KEY_STAR_UNIFIED_VERSION` 由 `star-v6` 提到 `star-v7`。原因：快照里保存的 `ettResult` 来自写入时的插件构建，旧快照没有 `junkFile` 字段 → 命中缓存就会退回 `0.00` 且无任何提示（这正是"更新后第一次复测看起来完全没生效"的原因）。bump 后旧快照整体失效。另：junk 谱因 `errors` 非空而不满足缓存写门，因此**不会把降级快照落盘**。
- **Companella 胶囊跟随真实来源**：`applyCompanellaToMixedResult` 新增 `companellaCapsule` 标记；`analysis.js` 在融合后更新 `state.actualEstimatorAlgorithm`——采用 Companella 结果（C6/C9）→ `"Companella"`；低难 0.5/0.5 混合（C8）→ `"Azusa+Companella"`；门控未通过、数值未变（C7）→ 保持原值。

### 修改原因（Why）

- 用户报告：anime vibro 包中一张 18K 谱 Etterna 分析失败，界面直接显示 Emscripten 原文（"Aborted(). Build with -sASSERTIONS for more info."），对用户没有意义；同包中 Ganbare 的 MSD 显示 `0.00`，实际是 MinaCalc 的 junk-file 守卫返回全 0，而非"难度为零"。
- 另有一处显示不一致：Companella 通过融合参与时数值已改变，但胶囊仍显示融合前的赢家（`Azusa`/`Roxy`），看起来像"没生效"。

### 影响范围（Scope）

- 只改**展示层与错误语义**：交给下游的技能值/星数/数值难度输入输出不变（junk 谱依旧把全 0 值传给 vibro 与 Companella，与改动前一致）。
- pipeline 契约**新增**字段：`ettErrorCode`（string|null）、`ettResult.junkFile`（boolean）、`ettResult.rowCount`（number）；既有字段语义未变。
- `actualEstimatorAlgorithm` **新增取值** `"Azusa+Companella"`，但它是**胶囊专有**的：遥测的 `actualAlgorithm` 维度不会出现该值——载荷边界（`analysis.js` 的 `toTelemetryActualAlgorithm`）把它映射回真实算法名 `"Azusa"`（后端按字符串分桶），白名单之外的标签则整个字段都不发送。
- 未 bump 插件版本（`index.js` `_VERSION` 与 `metadata.txt` `Version` 仍为 2.1.0）；缓存快照会保存新的胶囊值，命中时原样恢复。

### 兼容策略（Compat）

- abort 分支只影响 MinaCalc 主动中止的谱面（此前这些谱面必然失败，现在也失败，但错误可读且**不再污染 wasm 模块缓存**）。
- junk 分支不隐藏数据：`ettResult.values` 仍是全 0，只是展示层明确标注不可用；设置 `Numeric Difficulty`/MSD 相关显示逻辑之外的路径不变。
- 胶囊新增值不影响任何估算数值；下游按 `actualEstimatorAlgorithm` 分支的逻辑（如 `rcNative` 判定）只匹配既有的 `Azusa`/`Roxy`，不受影响。

### 验证方式（Verification）

- `temp/ett-capsule/tests.mjs`：**14/14 通过**（合成谱，不读样本数据）。
  - abort 复现形态（18K、≈37nps、偶发满宽 18 键和弦、3% 长条）抛出 `code = "minacalc-aborted"`、文案不含 `ASSERTIONS`，原始文本在 `cause`；abort 之后同版本 4K 频谱面仍可正常分析（模块缓存回收生效）。
  - 极端密度 4K 谱（每行 4 键、6.6ms 间隔）`junkFile === true` 且技能值全 0；普通 4K 谱 `junkFile === false`（不误判）。
  - `applyCompanellaToMixedResult` 四条路径的胶囊标记与数值：采用 → `Companella` + Companella 数值；混合 → `Azusa+Companella` + 0.5/0.5；保留 → 无标记且数值不变；无计划 → 原样返回。

---

## English

### What changed

- **MinaCalc abort made readable** (`js/ett/calc.js`): Emscripten aborts ("Aborted(). Build with -sASSERTIONS for more info.") are caught, the affected version's wasm module is dropped from the cache (an aborted instance is unusable and would fail every later chart), and a readable error with `code = "minacalc-aborted"` is thrown; the original text is kept in `error.cause`. `runAnalysisPipeline.js` gains the `ettErrorCode` contract field, and `analysis.js` renders the card as **"Unsupported Chart"** (same convention as the existing "Unsupported Keycount") while `errors[]` reads `Etterna analyze failed: MinaCalc aborted on this chart (structure or density out of range)`.
- **Junk files made explicit**: for absurd-density charts (e.g. 150+ rows per second) MinaCalc does not error out — it prints `skipping junk file` and returns **all-zero skillsets**. `calc.js` now reports `junkFile` (requires >= 32 rows and every displayed skill value to **round to 0.00**, i.e. an absolute value below 0.005 — the display keeps two decimals, so 0.004 and 0 look identical to a user) plus `rowCount`. All four display surfaces are covered: (1) the Etterna section reads `MSD unavailable (MinaCalc junk file)`; (2) the right capsule and separator show `-`/`--`; (3) the **left capsule in MSD mode** no longer shows `0.00 MSD` and falls back to the star capsule (same as "no Ett result"); (4) the **metadata status line shows a red notice** (junk charts push `Etterna MSD unavailable (MinaCalc junk file)` into `errors[]`, reusing the existing metadata error channel) — the card body is not necessarily the Etterna section, so a user looking at the body alone would not know what happened.
- **Result cache key bumped**: `CACHE_KEY_STAR_UNIFIED_VERSION` moves from `star-v6` to `star-v7`. Reason: snapshots store the `ettResult` produced by the build that wrote them, and older snapshots have no `junkFile` field, so a cache hit silently restored `0.00` with no notice (exactly why the first retest looked like the fix did nothing). The bump invalidates every older snapshot. Junk charts also fail the cache write gate (`errors` non-empty), so a degraded snapshot is never persisted.
- **Companella capsule follows the real source**: `applyCompanellaToMixedResult` returns a `companellaCapsule` marker, and `analysis.js` updates `state.actualEstimatorAlgorithm` after the merge — adopting Companella's value (C6/C9) gives `"Companella"`, the low-band 0.5/0.5 blend (C8) gives `"Azusa+Companella"`, and a gated-out plan that leaves the number untouched (C7) keeps the previous value.

### Why

- Reported: an 18K chart in the anime vibro pack failed Etterna analysis and the UI showed the raw Emscripten text, which means nothing to a user; in the same pack Ganbare's MSD showed `0.00` even though MinaCalc's junk-file guard had returned zeros — not a "zero difficulty".
- A second inconsistency: when Companella participates through fusion the number changes but the capsule kept the pre-fusion winner (`Azusa`/`Roxy`), which looks like Companella never ran.

### Scope

- Display and error semantics only: the skillset/star/numeric inputs handed downstream are unchanged (junk charts still pass all-zero values to vibro and Companella exactly as before).
- Additive pipeline contract fields: `ettErrorCode` (string|null), `ettResult.junkFile` (boolean), `ettResult.rowCount` (number); existing field semantics are untouched.
- `actualEstimatorAlgorithm` gains one value, `"Azusa+Companella"`, but it is **capsule-only**: the telemetry `actualAlgorithm` dimension never carries it, because the payload boundary (`toTelemetryActualAlgorithm` in `analysis.js`) maps it back to the real algorithm name `"Azusa"` (the backend buckets by raw string), and a label outside the whitelist is not sent at all.
- No version bump (`index.js` `_VERSION` and `metadata.txt` `Version` stay 2.1.0); cached snapshots store the new capsule value and restore it on a hit.

### Compatibility

- The abort branch only affects charts MinaCalc aborts on (they already failed before); now the message is readable and the wasm module cache is no longer poisoned.
- The junk branch hides nothing: `ettResult.values` stays all-zero, only the display marks it unavailable.
- The new capsule value does not affect any estimate; consumers that branch on `actualEstimatorAlgorithm` (e.g. the `rcNative` numeric check) only match the existing `Azusa`/`Roxy` values.

### Verification

- `temp/ett-capsule/tests.mjs`: **14/14 pass** (synthetic charts, no sample data).
  - The abort shape (18K, ~37 nps, occasional full-width 18-key chords, 3% holds) throws `code = "minacalc-aborted"` with no `ASSERTIONS` text in the message and the raw text in `cause`; a normal 4K chart still analyses afterwards (the module cache is recycled).
  - An extreme-density 4K chart (4 keys per row every 6.6 ms) reports `junkFile === true` with all-zero values, while a normal 4K chart reports `false` (no false positives).
  - All four `applyCompanellaToMixedResult` routes are asserted for both the capsule marker and the number: adopt -> `Companella`; blend -> `Azusa+Companella` with the 0.5/0.5 value; gated-out keep -> no marker, number unchanged; no plan -> returned as-is.
