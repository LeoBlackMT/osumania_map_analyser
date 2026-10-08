# ManiaMapAnalyser desktop shell

Tauri v2 桌面壳（Windows / Linux）：加载同一插件页面（**缺省一律**壳自身 24061 静态服务——只有壳配置显式 `osuTransport: "tosu"` 才恢复旧策略"在线 = tosu 插件页 / 离线 = 24061"），并作为本地聚合桥。Linux 下支持 Etterna 数据源（0.75+ 官方 Linux 版）；Malody V 无 Linux 版（该源仅 Windows）；**Malody 4.3.7 源同样仅 Windows**（`ReadProcessMemory` + PE 校验属 Win32 语义，其他平台以 `platform-unsupported` 优雅不可用）；**osu 原生读取同样仅 Windows**（`ReadProcessMemory` / `VirtualQueryEx`，其他平台报 `platform-unsupported` 并回落 tosu）。人类使用教程见 [docs/shell-guide.md](../docs/shell-guide.md)；技术说明见 [docs/features/desktop-shell.md](../docs/features/desktop-shell.md)、osu 原生传输见 [docs/features/osu-native-source.md](../docs/features/osu-native-source.md) 与 `docs/CONTRACT.md`（本目录，契约版本 **6**）。

| 能力 | 端口/路径 | 说明 |
|---|---|---|
| Malody 编辑器通道 | 24060 | 文件通道为主：编辑器 WriteFile `*_mma_request.json` → 壳扫描 → 谱面 = 同目录 `<base>.mc|.osu` → song 帧 → 页面分析 → 卡片展示（不回写 txt）。另保留 POST/GET resolve（诊断/备用） |
| 静态服务 | 24061 `/` | 服务插件目录（exe 所在目录优先；兼容上溯探测）。主窗**缺省**导航到这里（见下"窗口"） |
| WS | 24061 `/ws` | hello/state/song/malody4_selection/settings/result/control/ping 帧（见 CONTRACT.md） |
| **osu 兼容 origin** | **24062** | 壳内 osu 原生读取器的 **tosu 兼容子集**：WS `/websocket/v2`（150 ms 一帧）、WS `/websocket/commands`（黑洞）、`GET /files/beatmap/{file,background}`；全部响应带 `Access-Control-Allow-Origin: *`，Host 门禁只放行本机 24062。**缺省实时载荷**；`MMA_OSU_COMPAT_REPLAY=1` 回到 B1 固定谱面回放（测试用）。绑定失败不 exit，只记日志 + 推 `state.errors` 并回落 tosu |
| **osu 原生读取** | `osu/` 模块 | 250 ms 一拍只读内存：按位数分派（stable 32 位签名扫描 / lazer 64 位偏移表驱动）；句柄权限恒 `PROCESS_VM_READ\|PROCESS_QUERY_INFORMATION`；L0–L3 四层门；不健康时**不出帧**（绝不发假数据）。详见 [docs/features/osu-native-source.md](../docs/features/osu-native-source.md) |
| 设置 | 24061 `/settings` | **权威链只看 tosu 是否在线**：在线 = tosu 设置文件（`settings/<插件目录名>.values.json`，只读）> 离线 = `mma-settings.json`（缺则按 settings.json 生成骨架，**不读** tosu 文件）。GET 全量；POST 请求键优先读-改-写并广播（在线 403 / 非法 body 400 / 写失败 500） |
| 壳配置 | 24061 `/shell-config` | GET 返回 `{config, resolved:{etternaRoot,malodyRoot,malody4Root}}`（resolved = 实际采纳路径）；POST 请求键优先读-改-写（400 = body 非法 / base 不可读 / 写失败，此时不写盘），成功后失效根目录探测缓存并广播 settings 帧。启动时对**已有**文件只补不改：`hotkeys` 缺失/空串的键按内置默认补齐（含设置窗口的 `Ctrl+Shift+S`），用户值原样保留 |
| 设置窗口 | 24061 `/open-settings` + `/settings.html` | POST 打开/聚焦设置窗口（异步发起建窗；无 app 句柄 → 503）；`settings.html` 是窗口承载的设置页（导航 **Shell → Card Settings → Presets**：壳配置 + 卡片设置 + 完整预设，由静态分支服务）。表单**由 `settings.json` schema 在运行时生成**（新增设置只改 settings.json + config.js defaults + 解析/应用函数，设置页零改动）；`settings.json` 的 button 条目中壳内可达的 4 个渲染成**页面顶部**的 **Links** 行（状态栏下方、导航栏上方，打开窗口第一眼可见；另两个 `PresetButton`/`DebugButton` 指向 tosu 自己的页面，由 `settingsLinks.js EXCLUDED_LINK_IDS` 排除，settings.json 不动），壳窗口内链接走 `window.open` → 剪贴板兜底提示 |
| 内存特征表管理 | 24061 `/offsets/status` + `/offsets/generate` + `/offsets/update` | GET 查询当前 stable/lazer 特征表版本与生成器就绪态；POST `/offsets/generate` 免 SDK 活体自校验生成当前游戏的偏移表；POST `/offsets/update` 校验 Ed25519 签名清单并同步远端表 |
| 影子对拍诊断 | 24061 `/shadow/status` + `/shadow/reset` | GET 查询当前 L3 影子比对状态、匹配率与逐字段差异；POST `/shadow/reset` 重置对拍计数 |
| 封面 | 24061 `/cover/...` | 白名单具体文件（图片扩展名，同帧下发 URL） |
| Etterna 轮询 | — | 2Hz 轮询 `Save/MmaBridge.txt`/`Save/MmaGameplay.txt`（首读基线不推送） |
| Malody 轮询 | — | 1.5s 轮询 chart/（两级目录 mtime 快筛，仅 stat 目录层） |
| Malody 4.3.7 只读观察 | `malody4/` 模块 | 200ms 一拍：读锚点身份键 + tail 游戏日志取场景 + 轮询 `config.json` 取变速位/判定档；**零文件写入游戏目录**、不注入；谱面库索引（流式 md5）在后台线程构建 |
| tosu 探测 | - | `tosu.env` 逐级向上探测（2–3 层）→ `GET {ip}:{port}/` 健康检查（30s 周期） |

窗口：透明/置顶/点击穿透（`set_ignore_cursor_events`，快捷键切换）；`WebviewUrl::External` **缺省一律**指向 `http://127.0.0.1:24061/`（壳配置 `osuTransport != "tosu"`，即缺省 `auto`）——主窗必须能跟壳说话，原生传输的端点下发与桥都只在壳自己的页面上生效；只有壳配置显式 `"osuTransport": "tosu"`（逃生开关）才恢复旧策略（在线 ⇒ tosu 插件页 / 离线 ⇒ 24061）。全局快捷键与窗口状态（位置/尺寸/置顶/穿透）持久化到 `mma-shell-state.json`。**Wayland 会话**下全局快捷键不可用（XGrabKey 注册成功但不触发），窗口聚焦时由页面内快捷键经 control 帧兜底（键位一致）；Windows/X11 行为不变。

**设置窗口**（第二个 OS 窗口，label `settings`）：承载 `http://127.0.0.1:24061/settings.html`（壳配置 + 卡片设置 Card Settings + 完整预设）。三个入口 —— 全局快捷键 `hotkeys.settings`（默认 `Ctrl+Shift+S`）、CLI `--settings`（已有实例时转交 `POST /open-settings` 后 `exit(0)`，不建任何窗口）、`POST /open-settings`（浏览器/脚本；`settings.html` 缺失时只记 warn 不建窗）。窗口几何写入 `mma-shell-state.json` 的 `settings` 字段（向前兼容读取遗留的 `mma-shell-settings-window.json`）；打开期间主窗置顶临时取消（**不写**主窗状态文件，关闭/失败时按该文件恢复）。在线只读、离线可写（详见 `docs/shell-guide.md` 的「设置窗口」与 `docs/features/desktop-shell.md` §4）。

壳配置（exe 旁，首启自动生成）：`mma-shell-config.json`（`gameClient`/`etternaRoot`/`malodyRoot`/`malody4Root`/`hotkeys`/`logLevel`；`malody4Root` 由桥安装器的 **Malody 4** 选项写入——该选项零文件复制）与 `mma-settings.json`（全量插件设置；离线权威，在线时由 tosu 设置文件取代且不生成/不使用本地文件）。

构建与运行（开发态）：`cargo run --release`；打包：`desktop/release.ps1`（Windows，zip）/ `desktop/build-linux.sh`（Linux，tar.gz，需 Tauri Linux 系统依赖，与 CI 一致）。

开发环境变量：

- `MMA_PLUGIN_DIR`：覆盖插件目录解析（缺省：exe 所在目录优先，兼容上溯探测 `ManiaMapAnalyser by Leo_Black`）
- `MMA_SKIP_TOSU_PROBE=1`：跳过 tosu.env 探测（强制离线模式）
- `MMA_ETTERNA_ROOT` / `MMA_MALODY_ROOT`：覆盖游戏根目录（优先于配置/自动探测）
- `MMA_MALODY4_ROOT`：覆盖 Malody 4.3.7 根目录（同上；任何非空值即被采纳，故指向不存在的路径可用于**关断**该源）
- `MMA_OSU_COMPAT_REPLAY=1`：把 24062 从**实时载荷**切回 B1 的固定谱面回放（测试用；缺省实时）
- `MMA_LAZER_OFFSETS=<表路径>`：显式指定 lazer 偏移表（优先于 `<exe 目录>\offsets\lazer\*.json`）
- `MMA_OSU_COMPARE=0` / `MMA_OSU_COMPARE_INTERVAL_MS` / `MMA_OSU_COMPARE_OUT` / `MMA_OSU_TOSU` / `MMA_OSU_COMPARE_FULL=1`：影子对拍装置的开关/节奏/落点/tosu 端点/整包落盘（dev-only，默认开启、100 ms 一拍）

**osu 原生传输的能力与限制**（完整清单见 [docs/features/osu-native-source.md](../docs/features/osu-native-source.md)）：

- **偏移表随壳分发与动态更新**：发布包（`release.ps1`）自动打包 `offsets/` 目录（含 `stable/` 与 `lazer/` 纯数据偏移表及签名 `manifest.json`）并随包编译分发 `gen.exe`。支持通过设置页一键活体生成，或通过内置 Ed25519 签名通道在线静默更新。本地缓存落地在 `%APPDATA%\ManiaMapAnalyser\offsets\`，若不可用则优雅回退至内嵌默认表。
- **lazer 的字段缺口**（7 条，逐条进 `sources.osu.degradedFields`）：mods 三槽、`play.hits`/`resultsScreen.hits`、`files.background`/`files.audio`；因此 lazer 上 `modSignature` 只在 NM 时与 tosu 逐字节相等，hits/livePP 面不可用。
- **受限会话下的运行方式（真机实测）**：在工作区上下文里 `Toolhelp32` 只能看到自己那一族进程（表现为"游戏没在跑"、读不到游戏）——**把 `mma-shell.exe` 复制到工作区外（如 `%TEMP%\mma-shell\`）再启动**即可正常附着（本机实测：从 `desktop\target\debug` 启动不可用，复制到 `%TEMP%` 后可用）。同一结论也适用于 `tools/lazer-offsets-gen`。
- **Linux**：无内存读取（`platform-unsupported`），osu 走 tosu；其余源行为不变。

日志：`logs/mma-shell-YYYYMMDD.log`（按日轮转保留 7；`logLevel` 过滤；成功=debug、失败=error）。

---

# ManiaMapAnalyser desktop shell (English)

Tauri v2 desktop shell (Windows / Linux): loads the same plugin page (by default **always** the shell's own 24061 static server — only an explicit shell-config `osuTransport: "tosu"` restores the old "online = tosu plugin page / offline = 24061" policy) and acts as a local aggregation bridge. On Linux the Etterna data source is supported (official 0.75+ Linux build); Malody V has no Linux version (that source is Windows-only), and the **Malody 4.3.7 source is Windows-only as well** (`ReadProcessMemory` plus the PE check are Win32 semantics; elsewhere it degrades gracefully to `platform-unsupported`); the **osu native reader is Windows-only too** (`ReadProcessMemory` / `VirtualQueryEx`; elsewhere it reports `platform-unsupported` and falls back to tosu). Human-facing guide: [docs/shell-guide.md](../docs/shell-guide.md); technical details: [docs/features/desktop-shell.md](../docs/features/desktop-shell.md), the osu native transport in [docs/features/osu-native-source.md](../docs/features/osu-native-source.md) and `docs/CONTRACT.md` (this folder, contract v**6**).

| Capability | Port/Path | Notes |
|---|---|---|
| Malody editor channel | 24060 | File channel primary: editor WriteFile `*_mma_request.json` → shell scans → chart = same-dir `<base>.mc\|.osu` → song frame → page analysis → shown on the card (no txt writeback). POST/GET resolve kept for diagnostics/fallback |
| Static server | 24061 `/` | Serves the plugin folder (exe's own dir first; upward probe fallback). The main window navigates here **by default** (see "Window") |
| WS | 24061 `/ws` | hello/state/song/malody4_selection/settings/result/control/ping frames (see CONTRACT.md) |
| **osu compatible origin** | **24062** | The **tosu-compatible subset** served by the in-shell osu reader: WS `/websocket/v2` (one frame per 150 ms), WS `/websocket/commands` (black hole), `GET /files/beatmap/{file,background}`; every response carries `Access-Control-Allow-Origin: *`, and the Host gate only admits local 24062. **Live payload by default**; `MMA_OSU_COMPAT_REPLAY=1` falls back to the B1 fixed-chart replay (testing). A bind failure does not exit: it logs, pushes into `state.errors` and falls back to tosu |
| **osu native reader** | `osu/` modules | One 250 ms tick of read-only memory: client dispatch by bitness (stable = 32-bit signature scan / lazer = 64-bit offset-table driven); handle access is always `PROCESS_VM_READ\|PROCESS_QUERY_INFORMATION`; four gates L0–L3; when unhealthy it **emits no frame** (never fabricates data). See [docs/features/osu-native-source.md](../docs/features/osu-native-source.md) |
| Settings | 24061 `/settings` | **The authority chain only checks whether tosu is online**: online = tosu settings file (`settings/<plugin folder>.values.json`, read-only) > offline = `mma-settings.json` (skeleton generated from settings.json when missing; the tosu file is **not** read). GET returns everything; POST is a request-key-first read-modify-write that broadcasts (403 online / 400 unparsable body / 500 write failure) |
| Shell config | 24061 `/shell-config` | GET returns `{config, resolved:{etternaRoot,malodyRoot,malody4Root}}` (`resolved` = the paths actually adopted); POST is a request-key-first read-modify-write (400 = bad body / unreadable base / write failure, and nothing is written then); on success the root-detection caches are cleared and a settings frame is broadcast. At startup an **existing** file is only backfilled, never rewritten: missing/empty `hotkeys` keys get their built-in default (including `Ctrl+Shift+S` for the settings window) while user values stay untouched |
| Settings window | 24061 `/open-settings` + `/settings.html` | POST opens/focuses the settings window (window creation is dispatched asynchronously; 503 without an app handle); `settings.html` is the page it hosts (nav **Shell → Card Settings → Presets**: shell config + card settings + the full preset manager, served by the static branch). Its form is **generated at runtime from the `settings.json` schema** (a new setting only touches settings.json + config.js defaults + the parse/apply pair — zero settings-page changes), and the four button entries reachable from the shell become the **Links** row at the top of the page (below the status bar, above the nav, visible the moment the window opens; the other two — `PresetButton` / `DebugButton` — point at tosu's own pages and are dropped by `settingsLinks.js EXCLUDED_LINK_IDS` without touching settings.json). Inside the shell window links go through `window.open` with a clipboard fallback notice |
| Memory Offsets | 24061 `/offsets/status` + `/offsets/generate` + `/offsets/update` | GET queries stable/lazer table versions and generator readiness; POST `/offsets/generate` extracts and verifies offsets live from running game; POST `/offsets/update` verifies Ed25519 signature manifest and updates remote tables |
| Shadow Diagnostics | 24061 `/shadow/status` + `/shadow/reset` | GET queries live L3 shadow comparison match rates and per-field diff details; POST `/shadow/reset` resets stats |
| Cover | 24061 `/cover/...` | Whitelisted concrete files (image extensions, URL sent in same frame) |
| Etterna polling | — | 2Hz on `Save/MmaBridge.txt`/`Save/MmaGameplay.txt` (first read = baseline, no push) |
| Malody polling | — | 1.5s over chart/ (two-level dir mtime fast filter; only stats dir layers) |
| Malody 4.3.7 read-only observation | `malody4/` modules | One 200 ms tick: anchor identity key + game-log tail for the scene + `config.json` polling for speed mods/judge; **zero files written into the game folder**, no injection; the chart-library index (streamed md5) is built on a background thread |
| tosu probe | - | `tosu.env` upward search (2–3 levels) → `GET {ip}:{port}/` health check (30s cycle) |

Window: transparent / always-on-top / click-through (`set_ignore_cursor_events`, toggled by shortcuts); `WebviewUrl::External` points **by default** to `http://127.0.0.1:24061/` (shell config `osuTransport != "tosu"`, i.e. the default `auto`) — the main window must be able to talk to the shell, since both the native transport's endpoint delivery and the bridge only work on the shell's own page; only an explicit `"osuTransport": "tosu"` (escape hatch) restores the old policy (online ⇒ tosu plugin page / offline ⇒ 24061). Global shortcuts and window state (pos/size/topmost/click-through) persist to `mma-shell-state.json`. On **Wayland sessions** global shortcuts register but never fire (XGrabKey is a no-op there) — while the shell window is focused, in-page shortcuts take over via control frames (same key bindings); Windows/X11 behavior is unchanged.

**Settings window** (a second OS window, label `settings`): hosts `http://127.0.0.1:24061/settings.html` (shell config + card settings + the full preset manager). Three entry points — the global shortcut `hotkeys.settings` (`Ctrl+Shift+S` by default), the CLI `--settings` (with an instance already running it forwards `POST /open-settings` and exits 0 without creating any window), and `POST /open-settings` (browser/script; a missing `settings.html` logs a warning and opens nothing). Its geometry is saved into `mma-shell-state.json`'s `settings` field (with backward compatibility for the legacy `mma-shell-settings-window.json`); while it is open the main window's topmost is temporarily cancelled (**without** writing the main window's state file — restored from it on close/failure). Read-only online, writable offline (see the "Settings window" section of `docs/shell-guide.md` and §4 of `docs/features/desktop-shell.md`).

Shell config (next to the exe, auto-created on first run): `mma-shell-config.json` (`gameClient`/`etternaRoot`/`malodyRoot`/`malody4Root`/`hotkeys`/`logLevel`; `malody4Root` is written by the installer's **Malody 4** option, which copies zero files) and `mma-settings.json` (full plugin settings; the offline authority — while tosu is online the tosu settings file replaces it and no local file is created or used).

Build & run (dev): `cargo run --release`; packaging: `desktop/release.ps1` (Windows, zip) / `desktop/build-linux.sh` (Linux, tar.gz; needs the Tauri Linux system deps, same as CI).

Dev env vars:

- `MMA_PLUGIN_DIR`: override plugin folder resolution (default: exe's own dir first, upward probe for `ManiaMapAnalyser by Leo_Black` fallback)
- `MMA_SKIP_TOSU_PROBE=1`: skip tosu.env probe (force offline mode)
- `MMA_ETTERNA_ROOT` / `MMA_MALODY_ROOT`: override game roots (env > config > auto-detect)
- `MMA_MALODY4_ROOT`: override the Malody 4.3.7 root (same precedence; any non-empty value is accepted, so pointing it at a nonexistent path is how you disable that source)
- `MMA_OSU_COMPAT_REPLAY=1`: switch 24062 from the **live payload** back to the B1 fixed-chart replay (testing; live is the default)
- `MMA_LAZER_OFFSETS=<table path>`: explicit lazer offset table (takes precedence over `<exe dir>\lazer-offsets\<lazer>__<runtime>__<arch>.json`)
- `MMA_OSU_COMPARE=0` / `MMA_OSU_COMPARE_INTERVAL_MS` / `MMA_OSU_COMPARE_OUT` / `MMA_OSU_TOSU` / `MMA_OSU_COMPARE_FULL=1`: the shadow-comparison rig's switch / cadence / output path / tosu endpoint / full-payload dumping (dev-only; enabled by default, one record per 100 ms)

**osu native transport: capability & limitations** (full list: [docs/features/osu-native-source.md](../docs/features/osu-native-source.md)):

- **Offset Tables Distribution & Dynamic Updates**: The release package (`release.ps1`) bundles the `offsets/` folder (pure data tables for stable and lazer + signed `manifest.json`) and compiles `gen.exe` beside `mma-shell.exe`. Supports 1-click live generation from the settings window, as well as Ed25519-signed online updates. Local cache is stored in `%APPDATA%\ManiaMapAnalyser\offsets\`, silently falling back to bundled/embedded tables when offline.
- **lazer field gaps** (7, each reported in `sources.osu.degradedFields`): the three mod slots, `play.hits`/`resultsScreen.hits`, and `files.background`/`files.audio`; hence on lazer `modSignature` is byte-identical to tosu only for NM, and the hits/livePP surface is unavailable.
- **Running inside a restricted session (measured)**: in the workspace context `Toolhelp32` only sees its own process family (the game looks "not running" and cannot be read) — **copy `mma-shell.exe` outside the workspace (e.g. `%TEMP%\mma-shell\`) and start it there** to attach normally (measured: starting from `desktop\target\debug` does not work, copying to `%TEMP%` does). The same applies to `tools/lazer-offsets-gen`.
- **Linux**: no memory reading (`platform-unsupported`), osu goes through tosu; the other sources are unchanged.

Logs: `logs/mma-shell-YYYYMMDD.log` (daily rotation, 7 kept; filtered by `logLevel`; success=debug, failure=error).
