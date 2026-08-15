# 2026-08-15 contentBar 多选改造（commands 类型）

> 重大破坏性更改说明 / Major breaking-change note. Bilingual, five elements.

## 修改内容（What changed）

- `contentBar`（Card Body Content）设置从**单选项（options）**改为 tosu **`commands`** 类型：
  用户可添加多行，每行从 Pattern / Etterna / Graph / ReworkPP 中选择一个 section，
  **行顺序 = 前端显示顺序**；`uniqueCheck: "section"` 阻止重复。
- 删除 `"Full"`、`"None"`、`"Auto"` 三个旧选项值；新增独立 checkbox `autoContentBar`
  （默认 true）：勾选时忽略列表、按谱面 mode 自动选择单一主体（RC→Etterna，其余→Pattern）；
  关闭时使用列表，**空列表 = None（不显示主体）**。
- 内部数据模型：`state.contentBar` / `state.effectiveContentBar` / `state.userContentBar`
  由单字符串改为**有序数组 `string[]`**；`contentBarShows(section)` 改为
  `getActiveContentBar().includes(section)`。
- 显示顺序实现：DOM 不变，`updateContentBarVisibility` 按列表顺序为 8 个 grid item
  （4 个 separator + 4 个 block）设置 CSS `order`；多选（≥2 项）启用新布局类 `bars-multi`
  （复用原 `bars-full` 的高度自适应堆叠），单块保留 `bars-pattern/etterna/graph/pp`，
  空列表用 `bars-none`。
- 非 4/6/7K 谱面：主体 override 从"整体降级为 Pattern"改为"**从列表移除 Graph**（保序）"。
- `resolvedModeTag`：`activeContentBar === "None"` 特判改为 `activeContentBar.length === 0`。
- Auto 切换检测：`state.contentBar !== beforeContent` 引用比较改为 `JSON.stringify` 序列化比较。

## 修改原因（Why）

用户希望自由组合卡片主体内容（任意多个 section 同时显示）并自定义顺序，旧单选
+ Full 固定布局无法满足。tosu 的 `commands` 是唯一支持动态多行值的设置类型；
其 `options` 子字段可声明为 `type:"options"` 下拉，天然满足"每行选一个 section"。
顺序要求决定了数据模型必须保序（数组而非 Set）。

## 影响范围（Scope）

- 设置定义与解析：`settings.json`、`config.js`、`js/parser/settingsParser.js`。
- 状态与显示：`js/app/appContext.js`（`contentBarShows`/`isAutoContentBarEnabled`）、
  `js/app/settings.js`（apply/visibility/order/legacy autoMode）、`js/app/analysis.js`
  （override/resolvedModeTag/Auto 切换检测）。
- 样式：`styles/card-modes.css`、`theme.css`、`status.css`、`responsive.css`（`bars-multi`）。
- 缓存：`contentBar`/`autoContentBar` 属"特殊显示类"——只改变 `needComputed` 布尔集，
  由命中覆盖检查处理，**不加入 `clearResultCache()` 失效列表**；`autoContentBarChanged`
  加入 `recomputeNeeded` 触发按需重算。
- 共享模块（estimator/parser/ett/patterns/pipeline）**零改动**，Benchmark runner 不受影响。

## 兼容策略（Compat）

- 旧值迁移（`parseContentBarValue`/`normalizeContentBarList`）：
  `"Full"` → 四元素全选数组；`"None"` → `[]`；`"Auto"` → `[]`（由 `autoContentBar=true`
  默认恢复自动行为）；`"Pattern"/"Etterna"/"Graph"/"ReworkPP"` → 单元素数组。
- 旧 `enablePatternAnalysis` fallback：true → `["Pattern"]`，false → `[]`。
- 旧 `autoMode` 强制路径：写 `autoContentBar=true`（原写 `userContentBar="Auto"`）。
- `bars-full` CSS 类保留为迁移别名（与 `bars-multi` 共用规则）。

## 验证方式（Verification）

- 解析冒烟（temp/contentbar-smoke.mjs）：**24/24 PASS**——保序、去重、非法项丢弃、
  旧字符串迁移（Full/None/Auto/单值）、`enablePatternAnalysis` fallback、
  `autoContentBar` 默认值、`contentBarShows` 多选/空/单元素语义。
- 语法检查：`node --check` 全部通过（settings.js/analysis.js/appContext.js/settingsParser.js/config.js）。
- settings.json：`ConvertFrom-Json` 校验通过（47 条目，contentBar=commands、autoContentBar=checkbox）。
- 版本核对：`index.js` `_VERSION` == `metadata.txt` Version == **1.7.4**。
- 浏览器实测（实施时执行）：增删行、uniqueCheck 提示、Auto 切换、None 空态、
  多选 separator 与顺序断言（getBoundingClientRect）、非 4/6/7K Graph 移除、
  缓存命中/重算无陈旧。

---

# English

## What changed

- `contentBar` (Card Body Content) changed from single-choice `options` to tosu
  **`commands`** type: users add rows, each row picks one section from
  Pattern / Etterna / Graph / ReworkPP, **row order = display order**;
  `uniqueCheck: "section"` prevents duplicates.
- Removed legacy `"Full"`, `"None"`, `"Auto"` option values; added independent
  checkbox `autoContentBar` (default true): when on, the list is ignored and the
  body is auto-picked by map mode (RC→Etterna, else→Pattern); when off, the list
  is used, and **an empty list means None (no body)**.
- Internal model: `state.contentBar` / `state.effectiveContentBar` /
  `state.userContentBar` changed from single string to **ordered `string[]`**;
  `contentBarShows(section)` → `getActiveContentBar().includes(section)`.
- Display order: DOM unchanged; `updateContentBarVisibility` sets CSS `order` on
  the 8 grid items (4 separators + 4 blocks) by list position; multi-select
  (≥2) uses new `bars-multi` layout class (reuses `bars-full` adaptive stacking),
  single block keeps `bars-pattern/etterna/graph/pp`, empty list uses `bars-none`.
- Non-4/6/7K beatmaps: body override changed from "collapse whole body to Pattern"
  to "**remove Graph from the list** (order preserved)".
- `resolvedModeTag`: `activeContentBar === "None"` check → `activeContentBar.length === 0`.
- Auto-switch detection: `state.contentBar !== beforeContent` reference compare →
  `JSON.stringify` serialized compare.

## Why

Users want to freely combine multiple card-body sections with custom order; the
old single-choice + fixed Full layout cannot express that. tosu `commands` is
the only setting type supporting dynamic multi-row values, and its `options`
sub-field can be declared as a dropdown — naturally "one section per row". The
order requirement dictates an order-preserving array model (not a Set).

## Scope

- Settings definition/parsing: `settings.json`, `config.js`, `js/parser/settingsParser.js`.
- State & display: `js/app/appContext.js` (`contentBarShows`/`isAutoContentBarEnabled`),
  `js/app/settings.js` (apply/visibility/order/legacy autoMode), `js/app/analysis.js`
  (override/resolvedModeTag/Auto-switch detection).
- Styles: `styles/card-modes.css`, `theme.css`, `status.css`, `responsive.css` (`bars-multi`).
- Cache: `contentBar`/`autoContentBar` are "special display-class" settings — they
  only change the `needComputed` booleans, handled by the hit coverage check;
  **not added to `clearResultCache()`**; `autoContentBarChanged` joins
  `recomputeNeeded` for on-demand recompute.
- Shared modules (estimator/parser/ett/patterns/pipeline) untouched — Benchmark
  runner unaffected.

## Compatibility

- Legacy migration (`parseContentBarValue`/`normalizeContentBarList`):
  `"Full"` → 4-element array; `"None"` → `[]`; `"Auto"` → `[]` (default
  `autoContentBar=true` restores auto behavior); single values → 1-element array.
- Legacy `enablePatternAnalysis` fallback: true → `["Pattern"]`, false → `[]`.
- Legacy `autoMode` force path writes `autoContentBar=true` (was `userContentBar="Auto"`).
- `bars-full` CSS kept as migration alias (shared rules with `bars-multi`).

## Verification

- Parse smoke (temp/contentbar-smoke.mjs): **24/24 PASS** — order preserved,
  dedupe, invalid-item drop, legacy string migration (Full/None/Auto/single),
  `enablePatternAnalysis` fallback, `autoContentBar` default, `contentBarShows`
  multi/empty/single semantics.
- Syntax: `node --check` passes for all modified JS files.
- settings.json: `ConvertFrom-Json` OK (47 entries, contentBar=commands,
  autoContentBar=checkbox).
- Version: `index.js` `_VERSION` == `metadata.txt` Version == **1.7.4**.
- Browser checks (during implementation): add/remove rows, uniqueCheck prompt,
  Auto toggle, None empty state, multi separators and order assertion
  (getBoundingClientRect), non-4/6/7K Graph removal, cache hit/recompute freshness.
