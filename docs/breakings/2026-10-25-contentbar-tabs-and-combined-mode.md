# 2026-10-25 卡片 Tab 胶囊栏与组合模式（Combined）

> 类型：用户可见设置语义变更 + 桌面壳新增功能
> 插件版本：保持 **2.1.0**（未 bump）｜壳契约：保持 **7**（未改帧结构）
> 相关文档：[docs/features/content-bar-tabs.md](../features/content-bar-tabs.md)、[docs/settings.md](../settings.md)

## 中文

### 修改内容（What changed）

- `contentBar` 删除 `Full` 选项，收敛为 `None / Auto / Pattern / Etterna / Graph / ReworkPP`。
- 旧配置中的 `contentBar: "Full"` 解析为 `"Auto"`，不写回用户文件。
- 桌面壳新增 **组合模式（Combined）**：`mma-shell-config.json` 的 `combinedMode` 保存总开关与有序模块列表；设置页 `Shell Configuration` 提供开关、模块复选框、上/下移动。
- 桌面壳紧凑卡片新增 **Tab 胶囊栏**（Pattern / Etterna / Graph / PP）：
  - 仅壳窗口显示；`contentBar=None` 或 Combined 开启时隐藏；
  - 仅左键单击切换；只计算当前 Tab 的一个展示模块；
  - 点击已激活 Tab 触发一次强制重算（跳过结果缓存一次）；
  - 换歌/换难度/IN·HO 自动复位到 `contentBar` 设置。
- 原内部命名 `Full` 更名为 `Combined`：`fullMode` → `combinedMode`、`bars-full` → `bars-combined`、`full-separator` → `combined-separator`、`renderFullModeSeparators` → `renderCombinedModeSeparators`。
- 内置预设 `FullOverview` 的 `contentBar` 改为 `Auto`（tosu 侧不再支持多模块），描述改为“Auto 主体 + 顶右图表”；桌面壳的多模块需求由组合模式承担。

### 修改原因（Why）

- 卡片固定 475px，原 Full 在单卡内堆叠 Pattern + Etterna + Graph + PP，16:9 画布/OBS 下总高度可超过 650px；缩放又会牺牲可读性。
- tosu 叠加层必须保持极简、无交互；多模块看板只属于桌面壳。
- Tab 提供“单曲临时巡检”，不需要改全局 `contentBar`；Combined 提供可配置的广播/谱师审查看板。
- `Full` 更名 `Combined` 让“多个模块组合”与旧的全量语义解耦；新键从未发布，改名没有历史迁移成本。

### 影响范围（Scope）

- `ManiaMapAnalyser by Leo_Black/settings.json` / `config.js` / `js/parser/settingsParser.js`：枚举与旧值解析。
- `index.html` / `styles/*` / `js/app/settings.js` / `analysis.js` / `socketHandlers.js` / `sources/externalSource.js`：卡片布局、Tab、Combined、复位。
- `js/app/settingsPage/shellConfigPanel.js` / i18n：设置页 Combined 编辑区。
- `desktop/src/config/json.rs`：壳配置骨架 + 回填；`desktop/src/server/http.rs`：`GET /shell-config` CORS。
- **不影响**：估算器、结果缓存键、遥测白名单、难度反馈协议、桥协议、`CONTRACT_VERSION`。

### 兼容策略（Compat）

- 旧 tosu `contentBar: "Full"` → `"Auto"`（静默，不写回）；
- 旧 `mma-shell-config.json` 无 `combinedMode` → 启动时补齐默认 `{enabled:false, modules:[四个模块]}`，用户已有值不覆盖；
- 旧壳 + 新页：无 `combinedMode` → Combined 默认关闭，Tab 正常；
- 新壳 + 旧页：旧页忽略 `combinedMode`，行为不变；
- `combinedMode` 为新键，改名 `Full → Combined` 不需要迁移旧壳配置；本地残留的 `fullMode` 键会被忽略；
- 浏览器/OBS：`isShellRuntime()` 为 false，不建 Tab、不请求 `/shell-config`、`combinedModeEnabled` 恒 false；
- 版本/契约：插件 2.1.0、契约 7 均不变。

### 验证方式（Verification）

- `normalizeCombinedMode()` 单测：缺失/空数组/大小写/重复/非法项/`enabled` 非 bool；空数组回落默认四模块。
- `normalizeContentBarValue("Full"/"FULL"/" full ")` → `"Auto"`；非法值 → `null`。
- Rust `backfill_shell_combined_mode`：缺键、非法 `enabled`、非法/重复/空 `modules`、幂等。
- `cargo build` / `cargo test`；`GET /shell-config` 的 CORS curl 断言。
- 浏览器验收：24061 壳页 Tab 可见、`None` 隐藏、Combined 排序/隐藏、24050 无 Tab、设置页最后一个复选框禁用。
- 真实壳验收：换歌/换难度/IN·HO 复位、点击已激活 Tab 强制重算、`mma-shell-config.json` 重启保留。

## English

### What changed

- Removed `Full` from the `contentBar` option set; the enum is now `None / Auto / Pattern / Etterna / Graph / ReworkPP`.
- Legacy `contentBar: "Full"` is parsed as `"Auto"` (silent; the user file is not rewritten).
- Added desktop-shell **Combined mode**: `mma-shell-config.json` stores `combinedMode` (master switch + ordered module list); the Shell Configuration page provides the switch, module checkboxes, and up/down reordering.
- Added the shell-only **Tab capsule bar** (Pattern / Etterna / Graph / PP):
  - visible only in the shell window; hidden when `contentBar=None` or Combined is enabled;
  - left-click only; computes only the selected display module;
  - clicking the already-active tab forces one recompute (bypasses the result cache once);
  - resets to the `contentBar` setting on song/difficulty change or IN·HO (`cvtFlag`) change.
- Renamed the internal Full identifiers to Combined: `fullMode` → `combinedMode`, `bars-full` → `bars-combined`, `full-separator` → `combined-separator`, `renderFullModeSeparators` → `renderCombinedModeSeparators`.
- The built-in `FullOverview` preset now uses `contentBar: "Auto"` (tosu no longer supports multi-section bodies); its description is now “Auto body + top-right graph”. Desktop-shell multi-section use cases move to Combined mode.

### Why

- The card is fixed at 475px; the old Full stacked Pattern + Etterna + Graph + PP and could exceed 650px on a 16:9 canvas/OBS, while zooming hurts readability.
- The tosu overlay must stay minimal and interaction-free; multi-section boards belong to the desktop shell.
- Tabs provide single-map inspection without changing the global `contentBar`; Combined provides a configurable broadcast/authoring board.
- Renaming Full to Combined separates “multiple sections combined” from the old full-content meaning; the new key was never released, so no migration cost.

### Scope

- `ManiaMapAnalyser by Leo_Black/settings.json` / `config.js` / `js/parser/settingsParser.js`: enum and legacy parsing.
- `index.html` / `styles/*` / `js/app/settings.js` / `analysis.js` / `socketHandlers.js` / `sources/externalSource.js`: card layout, tabs, Combined, resets.
- `js/app/settingsPage/shellConfigPanel.js` / i18n: Combined editor.
- `desktop/src/config/json.rs`: shell-config skeleton/backfill; `desktop/src/server/http.rs`: `GET /shell-config` CORS.
- **Not affected**: estimators, result-cache keys, telemetry whitelist, difficulty-feedback protocol, bridge protocol, `CONTRACT_VERSION`.

### Compatibility

- Legacy tosu `contentBar: "Full"` → `"Auto"` (silent, no file rewrite).
- Existing `mma-shell-config.json` without `combinedMode` gets the default skeleton on startup; user values are preserved.
- Old shell + new page: missing `combinedMode` → Combined stays off, tabs work.
- New shell + old page: the old page ignores `combinedMode`.
- `combinedMode` is a new key; the Full → Combined rename needs no shell-config migration. A local leftover `fullMode` key is ignored.
- Browser/OBS: `isShellRuntime()` is false; no tabs, no `/shell-config` request, `combinedModeEnabled` stays false.
- Version/contract: plugin 2.1.0 and contract 7 are unchanged.

### Verification

- Unit tests for `normalizeCombinedMode()` (missing/empty/case/duplicates/invalid/non-bool) and empty-array fallback.
- `normalizeContentBarValue("Full"/"FULL"/" full ")` → `"Auto"`; invalid → `null`.
- Rust `backfill_shell_combined_mode`: missing key, invalid `enabled`, invalid/duplicate/empty `modules`, idempotence.
- `cargo build` / `cargo test`; CORS curl check for `GET /shell-config`.
- Browser acceptance: shell tabs visible, `None` hides them, Combined order/visibility, no tabs at 24050, last-checkbox disabled in settings.
- Real-shell acceptance: song/difficulty/IN·HO reset, active-tab forced recompute, `mma-shell-config.json` survives restart.
