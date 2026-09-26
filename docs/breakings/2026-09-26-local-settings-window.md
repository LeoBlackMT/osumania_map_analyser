# 2026-09-26 — 本地设置窗口与离线权威链（桌面壳）

> 破坏性变更记录。格式：改了什么 / 为什么 / 影响范围 / 兼容性 / 验证方式。

## 1. 改了什么

| 面 | 之前 | 现在 |
|---|---|---|
| 设置权威链 | ① tosu 非空对象 → ② **tosu 设置文件存在**（**离线也读**）→ ③ `mma-settings.json` → ④ 生成默认 | **① tosu 在线**（`tosu.is_some()` 且探测存活）→ tosu 设置文件；**② 离线** → 本地 `mma-settings.json`（缺则按插件 `settings.json` 生成骨架）。原第 2 级"离线读 tosu 文件"**已删除** |
| 图形化设置界面 | 无（只能手改 JSON） | 壳内**第二个 OS 窗口**（label `settings`，默认 1040×720 居中），承载壳 24061 上的新页面 `settings.html`：插件设置 + 壳配置 + 完整预设区 |
| 打开设置窗口 | — | 三入口：全局快捷键 `hotkeys.settings`（默认 `Ctrl+Shift+S`）、CLI `mma-shell.exe --settings`、`POST /open-settings` |
| 本机端点（24061） | `/settings`（GET/POST）、`/cover`、静态 | 新增 `GET/POST /shell-config`、`POST /open-settings`；`POST /settings` 改为请求键优先的读-改-写并广播 |
| 离线写入语义 | 整份覆盖写、不更新缓存、不广播 | 读-改-写（`merge_plugin_settings`，未知键保留）+ 更新缓存 + **广播 settings 帧**（overlay 立即生效，无需重启） |
| 来源切换 | 无（`tosu_online` 只用于推导 URL） | 抽成 `apply_tosu_online_transition`：定时器与 `POST /settings` 复探共用，切源同时刷缓存并全量推送 |
| 定时器门控 | 三条检测均无来源门控 | 本地 `mma-settings.json` 检测加 `if !online`（stat→read→stat），tosu 文件检测加 `if online`，壳配置检测**不加门控**（在线也要 ≤30s 生效） |
| 页面预设存储 | 只有 tosu 路径（依赖 `window.COUNTER_PATH`） | 新增 preset transport 注入点（`setPresetTransport`/`getPresetTransport`）：`presets/tosuTransport.js` 与原逻辑等价，`presets/shellTransport.js` 走 24061（全量基点 + 有界重试），设置页用薄适配器组合 |
| 实例探测 | 无（第二实例先建主窗口再退出） | `main()` 开头探测 24061：命中本壳 → 带 `--settings` 则转交 `POST /open-settings`，然后**一律 `exit(0)`（不建任何窗口）**；端口被外部进程占用 → `exit(2)` |
| 设置窗口几何 | — | 独立文件 `mma-shell-settings-window.json`（与主窗 `mma-shell-state.json` 完全分离） |

**不变的部分（同等重要）**：`CONTRACT_VERSION`（`desktop/src/frames.rs`）**仍为 5**，页面 `bridgeClient.js` 的常量同步保持 5，接受区间仍为 `[3,5]`；帧类型集合与各帧 schema 未增删（八型 + diag）；`server/ws.rs` 未改；`tauri.conf.json` 未改；**插件版本号未 bump**（`index.js` `_VERSION` 与 `metadata.txt` `Version` 仍为 `2.1.0`）；未加任何 Cargo 依赖。

## 2. 为什么

- **非 tosu 用户没有图形化入口**：手改 `mma-settings.json` 对普通用户门槛过高，且改完要等周期检测才生效；预设系统此前也只有 tosu 一条落盘路径（浏览器页面依赖 tosu 注入的 `window.COUNTER_PATH`），壳页里保存预设会静默 no-op。
- **旧的权威链判据是错的**：它看的是"tosu 设置文件存不存在"而不是"tosu 在不在线"。装着 tosu 但没运行的机器上会采纳 `values.json` 里**记忆中的旧值**，用户在设置窗口/本地文件里的修改被无视；同一条链的两个使用者（GET 与 POST）还可能各自给出不同来源。
- **离线写入缺少即时反馈**：旧 `POST /settings` 只写盘，不更新壳内缓存也不广播，overlay 要等下一次 30s 检测才变化。
- **第二实例体验差**：旧行为下第二次双击会先建出主窗口再因端口占用退出，屏幕会闪一下。

## 3. 影响范围

- **权威链切换后，离线配置的"来源"会变**：以前"装了 tosu 但没运行"会读 tosu 的 `values.json`；现在读本地 `mma-settings.json`。本地文件不存在时**按插件默认值生成骨架，不继承** tosu `values.json`（决策 D4a：不做一次性播种）——这类用户的卡片显示可能看起来"回到默认"，需要在设置窗口里重设或手改本地文件。
- **只在桌面壳（`desktop/`）**：插件页面本体、浏览器模式（无壳、osu! 单源）行为不变；`presets.html`（tosu 在线）行为不变；四个数据源（osu!/Etterna/Malody V/Malody 4.3.7）的管线与分析结果不变。
- **新增本机面**：`127.0.0.1:24061` 上多了 `/shell-config`（读写壳配置）、`/open-settings`，以及设置页静态资源（`settings.html`、`js/app/settingsPage/*`、`styles/settings-page.css`）。Host 白名单仍只放行本机三个写法；未列入白名单的路径 404/403。
- **新窗口与主窗口的关系**：设置窗口打开期间主窗置顶被临时取消（**不写** `mma-shell-state.json`），关闭/建窗失败按该文件恢复；关闭 overlay 会连带关闭设置窗口。
- **已知差异（计划 §11-Q9，刻意保留）**：置顶/穿透的**页面内兜底**路径（control 帧 `toggleTopmost`/`toggleClickThrough` → `server/ws.rs handle_control`）没有 `is_open()` 门控——设置窗口打开时按这两个兜底键仍会切换置顶并写 `mma-shell-state.json`，主窗可能重新盖住设置窗口。本轮选择"不动 `ws.rs`"。

## 4. 兼容性

- **帧契约**：不升版本、不改帧型——旧壳 + 新页面、新壳 + 旧页面在 §11.8 的 `[3,5]` 区间下都仍然握手成功；设置窗口是"壳自己的第二个窗口 + 本机 HTTP 面"，与页面契约正交。
- **旧壳 + 新页面**：新页面按端点可用性降级；壳不提供 `/shell-config`/`/open-settings` 时设置页只是拿不到壳配置面板、`--settings` 会记日志退出（不崩）。**注意**：页面写路径始终发全量 body，旧壳的整份覆盖写不会把文件截断。
- **旧页面 + 新壳**：`mma-shell.exe --settings` 遇到不带 `/open-settings` 的旧壳实例时静默退出并记日志；主窗口照常。
- **配置数据**：无破坏性迁移。`mma-settings.json` 结构不变（仍是全键对象，`presetStorage` 键承载离线预设库）；`mma-shell-state.json` 不动；新增的 `mma-shell-settings-window.json` 可以安全删除（下次打开回到默认几何）。
- **阅读顺序提示**：`desktop/docs/CONTRACT.md` §13 是本机 HTTP 面（非帧契约）的权威说明；旧文档里"离线也读 tosu 设置文件/没有图形化设置界面"的说法以本轮文档为准。

## 5. 验证方式

- **Rust 单测**：`cargo test` 全部通过（177 项，含本轮新增的 `merge_json` 五分支、`SettingsWindowState` 往返与缺失回落、`should_seed_local_settings` 四象限、权威链四象限）。
- **构建**：`cargo build` 与 `cargo build --release` 均成功（Windows release 为 GUI 子系统，无控制台窗口）。
- **本机端点（真壳 + 合成 tosu 替身）**："装了 tosu 但未运行" → `GET /settings` 返回本地内容、`POST` 200 且重启后仍在；"tosu 在线" → `POST /settings` **403** 且 `GET` 返回 tosu 内容，含"tosu 刚启动 ≤30s"的冷窗口用例（仍在毫秒级切源并全量推送）；运行中关掉 tosu → 探针在 30s 内回到本地内容；`POST /settings` 往返保留合成未知键并返回合并后全量对象。
- **进程行为（真机）**：既有实例在跑时，第二进程带与不带 `--settings` 都在 <200ms 内 `exit(0)` 且不建任何窗口；24061 被外部进程占用时 `exit(2)` 并记 error。
- **不变量**：`CONTRACT_VERSION`（壳）与 `bridgeClient.js` 常量都为 5；开/关设置窗口前后 `mma-shell-state.json` 字节摘要一致；`POST /settings` 后 `mma-settings.json` 键集合不变；插件版本号（`index.js` 与 `metadata.txt`）未变。
- **端到端与目视项**：本机冒烟脚本（15 项独立检查，含广播断言与字节快照回滚，另带 `--self-test`）与手工清单（全局快捷键打开/聚焦、设置窗口几何独立记忆、无挂起开关序列、关 overlay 连带关闭、离线预设 CRUD 与重启保持、在线只读 + 自定义端口 dashboard URL、`presets.html` 与浏览器模式回归）在验收步骤中执行；GUI 项为目视判据，自动化证据仅本机可复现（`desktop/scripts/`、`desktop/tests-local/`、`temp/` 均为本机 gitignored 脚手架，不入库）。验收所在会话环境不产生可枚举的 OS 窗口，故 GUI 目视项需在交互式会话复核；窗口的**代码路径**已由证据确认：日志出现 `settings window opened`（只在 `build()` 返回 Ok 时记录），且连续三次 `POST /open-settings` 只新增一行该日志（后两次走 show/focus 分支）。

# 2026-09-26 — Local settings window and the offline authority chain (desktop shell)

> Breaking-change record. Format: what changed / why / scope / compatibility / how it was verified.

## 1. What changed

| Area | Before | Now |
|---|---|---|
| Settings authority chain | ① non-empty tosu object → ② **the tosu settings file existing** (**read offline too**) → ③ `mma-settings.json` → ④ generated defaults | **① tosu online** (`tosu.is_some()` and the probe alive) → the tosu settings file; **② offline** → the local `mma-settings.json` (skeleton generated from the plugin's `settings.json` when missing). The old level 2 ("read the tosu file offline") is **gone** |
| Graphical settings UI | none (hand-edit JSON) | a **second OS window** inside the shell (label `settings`, 1040×720 centred by default) hosting the new `settings.html` on port 24061: plugin settings + shell config + the full preset area |
| Opening it | — | three entry points: the global shortcut `hotkeys.settings` (`Ctrl+Shift+S` by default), the CLI `mma-shell.exe --settings`, and `POST /open-settings` |
| Local endpoints (24061) | `/settings` (GET/POST), `/cover`, static files | new `GET/POST /shell-config` and `POST /open-settings`; `POST /settings` became a request-key-first read-modify-write that broadcasts |
| Offline write semantics | whole-file overwrite, no cache update, no broadcast | read-modify-write (`merge_plugin_settings`, unknown keys kept) + cache update + **settings-frame broadcast** (the overlay reacts immediately, no restart) |
| Source switching | none (`tosu_online` only fed the startup URL) | extracted into `apply_tosu_online_transition`, shared by the timer and the `POST /settings` re-probe: switching refreshes the cache and pushes the full settings |
| Timer gating | none of the three detectors was source-gated | local `mma-settings.json` gains `if !online` (stat→read→stat), the tosu file gains `if online`, the shell config stays **ungated** (online changes must still land within ~30s) |
| Page preset storage | tosu only (relied on `window.COUNTER_PATH`) | preset transport injection (`setPresetTransport`/`getPresetTransport`): `presets/tosuTransport.js` is the old logic verbatim, `presets/shellTransport.js` talks to 24061 (full-object base + bounded retry), and the settings page composes them through a thin adapter |
| Instance probe | none (a second instance built the main window, then exited) | a probe at the top of `main()`: our own shell → forward `POST /open-settings` when `--settings` was given, then **always `exit(0)` without creating any window**; a foreign process on the port → `exit(2)` |
| Settings-window geometry | — | its own file `mma-shell-settings-window.json` (fully separate from the main window's `mma-shell-state.json`) |

**What does not change (equally important)**: `CONTRACT_VERSION` (`desktop/src/frames.rs`) is **still 5**, the page's `bridgeClient.js` constant stays 5 and the accepted range stays `[3,5]`; the frame-type set and every frame schema are untouched (eight types + diag); `server/ws.rs` is untouched; `tauri.conf.json` is untouched; **the plugin version is not bumped** (`index.js` `_VERSION` and `metadata.txt` `Version` stay `2.1.0`); no Cargo dependency was added.

## 2. Why

Non-tosu users had no graphical entry point: hand-editing `mma-settings.json` is too much to ask and only took effect on the periodic re-read, while the preset system's only persistence path was tosu's (the page relied on the tosu-injected `window.COUNTER_PATH`, so saving a preset on a shell page silently did nothing). The old chain's criterion was also simply wrong — it checked whether the tosu settings file *exists* rather than whether tosu is *online*, so on a machine with tosu installed but not running it adopted the stale values in `values.json` and ignored what the user had written locally; two consumers of the same chain (GET and POST) could even disagree about the source. Offline writes had no immediate feedback either (the old `POST /settings` wrote the file but neither updated the shell cache nor broadcast, so the overlay waited for the next ~30s tick), and a second launch flashed a main window before exiting.

## 3. Scope

After the chain change the *source* of an offline configuration changes: "tosu installed but not running" used to read tosu's `values.json`, and now reads the local `mma-settings.json`. When that file does not exist the skeleton is generated from plugin defaults and does **not** inherit the tosu `values.json` (decision D4a: no one-time seeding), so such users may see the card display looking "back to defaults" and need to re-set it in the settings window or edit the local file. Everything here is inside the desktop shell: the plugin page itself, browser mode (no shell, osu!-only) and `presets.html` while tosu is online behave exactly as before, and none of the four data sources' pipelines or analysis results changed. The new local surface adds `/shell-config` (read/write shell config), `/open-settings` and the settings-page static assets (`settings.html`, `js/app/settingsPage/*`, `styles/settings-page.css`) on `127.0.0.1:24061`; the Host whitelist still only allows the three loopback spellings and everything else stays 403/404. While the settings window is open the main window's always-on-top is temporarily cancelled (**without** writing `mma-shell-state.json`) and restored from that file on close or build failure; closing the overlay closes the settings window with it. One **known difference is deliberately kept** (plan §11-Q9): the in-page fallback path for topmost/click-through (control frames `toggleTopmost`/`toggleClickThrough` → `server/ws.rs handle_control`) has no `is_open()` gate, so those two keys still toggle and write `mma-shell-state.json` while the settings window is open and the main window can cover it again; the "do not touch `ws.rs`" constraint was kept this round.

## 4. Compatibility

The frame contract is not version-bumped and no frame type changes: an old shell with a new page, or a new shell with an old page, still handshakes inside the `[3,5]` range of §11.8 — the settings window is the shell's own second window plus a local HTTP face, orthogonal to the page contract. A new page degrades per endpoint availability against an old shell (no shell-config panel, `--settings` logs and exits instead of crashing), and because the page always sends a full body, an old shell's whole-file overwrite cannot truncate the file. A new shell forwarding `--settings` to an old instance that lacks `/open-settings` exits silently with a log entry while the main window keeps running. There is no destructive data migration: `mma-settings.json` keeps its full-object shape (the `presetStorage` key carries the offline preset library), `mma-shell-state.json` is untouched, and the new `mma-shell-settings-window.json` can simply be deleted (the next open falls back to the default geometry). §13 of `desktop/docs/CONTRACT.md` is the authority for the local HTTP face; older text claiming "the tosu settings file is read offline too" or "no graphical settings UI" is superseded by this round's documentation.

## 5. How it was verified

Rust unit tests all pass (177, including the new `merge_json` branches, the `SettingsWindowState` round-trip and missing-file fallback, the four `should_seed_local_settings` quadrants and the four authority-chain quadrants); `cargo build` and `cargo build --release` both succeed (the Windows release is a GUI subsystem binary with no console window). Against a real shell plus a synthetic tosu stand-in, "installed but not running" returns the local content from `GET /settings` and accepts `POST` (still there after a restart), while "tosu online" answers `POST /settings` with **403** and `GET` with the tosu content — including the cold-window case of tosu having just started (≤30s), where the source still switches and the full settings are pushed within milliseconds; stopping the stand-in brings the probe back to local content within 30 seconds, and a `POST /settings` round-trip preserves synthesised unknown keys and returns the merged full object. Real-machine process behaviour: with an instance already running, a second process — with or without `--settings` — exits 0 in under 200 ms without creating any window, and a foreign process holding 24061 makes it exit 2 with an error log. Invariants checked: `CONTRACT_VERSION` (shell) and the `bridgeClient.js` constant are both 5; `mma-shell-state.json` has the same byte digest before and after opening/closing the settings window; `mma-settings.json`'s key set is unchanged after a `POST`; and the plugin version (`index.js` / `metadata.txt`) is unchanged. The end-to-end smoke script (15 independent checks with broadcast assertions and byte-snapshot rollback, plus `--self-test`) and the manual checklist (global shortcut open/focus, independent settings-window geometry, the no-hang open/close sequence, overlay close closing the settings window, offline preset CRUD surviving a restart, read-only online with a custom-port dashboard URL, `presets.html` and browser-mode regression) run in the acceptance step; the GUI items are visual judgements and the automated evidence is reproducible on this machine only (`desktop/scripts/`, `desktop/tests-local/` and `temp/` are local-only gitignored scaffolding and are not committed). The acceptance session's environment produced no enumerable OS windows, so the GUI items need a re-check in an interactive session; the window's **code path** is nevertheless evidenced by the log line `settings window opened` (only emitted when `build()` returns Ok) and by three consecutive `POST /open-settings` calls adding exactly one such line (the second and third take the show/focus branch).
