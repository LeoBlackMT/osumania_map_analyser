# ManiaMapAnalyser desktop shell

Tauri v2 桌面壳（Windows / Linux）：加载同一插件页面（在线 = tosu 插件页 / 离线 = 本壳 24061 静态服务），并作为本地聚合桥。Linux 下支持 Etterna 数据源（0.75+ 官方 Linux 版）；Malody V 无 Linux 版（该源仅 Windows）；**Malody 4.3.7 源同样仅 Windows**（`ReadProcessMemory` + PE 校验属 Win32 语义，其他平台以 `platform-unsupported` 优雅不可用）。人类使用教程见 [docs/shell-guide.md](../docs/shell-guide.md)；技术说明见 [docs/features/desktop-shell.md](../docs/features/desktop-shell.md) 与 `docs/CONTRACT.md`（版本 5）。

| 能力 | 端口/路径 | 说明 |
|---|---|---|
| Malody 编辑器通道 | 24060 | 文件通道为主：编辑器 WriteFile `*_mma_request.json` → 壳扫描 → 谱面 = 同目录 `<base>.mc|.osu` → song 帧 → 页面分析 → 卡片展示（不回写 txt）。另保留 POST/GET resolve（诊断/备用） |
| 静态服务（离线） | 24061 `/` | 服务插件目录（exe 所在目录优先；兼容上溯探测） |
| WS | 24061 `/ws` | hello/state/song/malody4_selection/settings/result/control/ping 帧（见 CONTRACT.md） |
| 设置 | 24061 `/settings` | **权威链只看 tosu 是否在线**：在线 = tosu 设置文件（`settings/<插件目录名>.values.json`，只读）> 离线 = `mma-settings.json`（缺则按 settings.json 生成骨架，**不读** tosu 文件）。GET 全量；POST 请求键优先读-改-写并广播（在线 403 / 非法 body 400 / 写失败 500） |
| 壳配置 | 24061 `/shell-config` | GET 返回 `{config, resolved:{etternaRoot,malodyRoot,malody4Root}}`（resolved = 实际采纳路径）；POST 请求键优先读-改-写（400 = body 非法 / base 不可读 / 写失败，此时不写盘），成功后失效根目录探测缓存并广播 settings 帧 |
| 设置窗口 | 24061 `/open-settings` + `/settings.html` | POST 打开/聚焦设置窗口（异步发起建窗；无 app 句柄 → 503）；`settings.html` 是窗口承载的设置页（壳配置 + 插件设置 + 完整预设），由静态分支服务 |
| 封面 | 24061 `/cover/...` | 白名单具体文件（图片扩展名，同帧下发 URL） |
| Etterna 轮询 | — | 2Hz 轮询 `Save/MmaBridge.txt`/`Save/MmaGameplay.txt`（首读基线不推送） |
| Malody 轮询 | — | 1.5s 轮询 chart/（两级目录 mtime 快筛，仅 stat 目录层） |
| Malody 4.3.7 只读观察 | `malody4/` 模块 | 200ms 一拍：读锚点身份键 + tail 游戏日志取场景 + 轮询 `config.json` 取变速位/判定档；**零文件写入游戏目录**、不注入；谱面库索引（流式 md5）在后台线程构建 |
| tosu 探测 | - | `tosu.env` 逐级向上探测（2–3 层）→ `GET {ip}:{port}/` 健康检查（30s 周期） |

窗口：透明/置顶/点击穿透（`set_ignore_cursor_events`，快捷键切换）；`WebviewUrl::External` 指向 tosu 插件页（在线）或 `http://127.0.0.1:24061/`（离线）。全局快捷键与窗口状态（位置/尺寸/置顶/穿透）持久化到 `mma-shell-state.json`。**Wayland 会话**下全局快捷键不可用（XGrabKey 注册成功但不触发），窗口聚焦时由页面内快捷键经 control 帧兜底（键位一致）；Windows/X11 行为不变。

**设置窗口**（第二个 OS 窗口，label `settings`）：承载 `http://127.0.0.1:24061/settings.html`（壳配置 + 插件设置 + 完整预设）。三个入口 —— 全局快捷键 `hotkeys.settings`（默认 `Ctrl+Shift+S`）、CLI `--settings`（已有实例时转交 `POST /open-settings` 后 `exit(0)`，不建任何窗口）、`POST /open-settings`（浏览器/脚本；`settings.html` 缺失时只记 warn 不建窗）。窗口几何写入**独立文件** `mma-shell-settings-window.json`（与主窗 `mma-shell-state.json` 互不影响）；打开期间主窗置顶临时取消（**不写**主窗状态文件，关闭/失败时按该文件恢复）。在线只读、离线可写（详见 `docs/shell-guide.md` 的「设置窗口」与 `docs/features/desktop-shell.md` §4）。

壳配置（exe 旁，首启自动生成）：`mma-shell-config.json`（`gameClient`/`etternaRoot`/`malodyRoot`/`malody4Root`/`hotkeys`/`logLevel`；`malody4Root` 由桥安装器的 **Malody 4** 选项写入——该选项零文件复制）与 `mma-settings.json`（全量插件设置；离线权威，在线时由 tosu 设置文件取代且不生成/不使用本地文件）。

构建与运行（开发态）：`cargo run --release`；打包：`desktop/release.ps1`（Windows，zip）/ `desktop/build-linux.sh`（Linux，tar.gz，需 Tauri Linux 系统依赖，与 CI 一致）。

开发环境变量：

- `MMA_PLUGIN_DIR`：覆盖插件目录解析（缺省：exe 所在目录优先，兼容上溯探测 `ManiaMapAnalyser by Leo_Black`）
- `MMA_SKIP_TOSU_PROBE=1`：跳过 tosu.env 探测（强制离线模式）
- `MMA_ETTERNA_ROOT` / `MMA_MALODY_ROOT`：覆盖游戏根目录（优先于配置/自动探测）
- `MMA_MALODY4_ROOT`：覆盖 Malody 4.3.7 根目录（同上；任何非空值即被采纳，故指向不存在的路径可用于**关断**该源）

日志：`logs/mma-shell-YYYYMMDD.log`（按日轮转保留 7；`logLevel` 过滤；成功=debug、失败=error）。

---

# ManiaMapAnalyser desktop shell (English)

Tauri v2 desktop shell (Windows / Linux): loads the same plugin page (online = tosu plugin page / offline = shell's 24061 static server) and acts as a local aggregation bridge. On Linux the Etterna data source is supported (official 0.75+ Linux build); Malody V has no Linux version (that source is Windows-only), and the **Malody 4.3.7 source is Windows-only as well** (`ReadProcessMemory` plus the PE check are Win32 semantics; elsewhere it degrades gracefully to `platform-unsupported`). Human-facing guide: [docs/shell-guide.md](../docs/shell-guide.md); technical details: [docs/features/desktop-shell.md](../docs/features/desktop-shell.md) and `docs/CONTRACT.md` (contract v5).

| Capability | Port/Path | Notes |
|---|---|---|
| Malody editor channel | 24060 | File channel primary: editor WriteFile `*_mma_request.json` → shell scans → chart = same-dir `<base>.mc\|.osu` → song frame → page analysis → shown on the card (no txt writeback). POST/GET resolve kept for diagnostics/fallback |
| Static server (offline) | 24061 `/` | Serves the plugin folder (exe's own dir first; upward probe fallback) |
| WS | 24061 `/ws` | hello/state/song/malody4_selection/settings/result/control/ping frames (see CONTRACT.md) |
| Settings | 24061 `/settings` | **The authority chain only checks whether tosu is online**: online = tosu settings file (`settings/<plugin folder>.values.json`, read-only) > offline = `mma-settings.json` (skeleton generated from settings.json when missing; the tosu file is **not** read). GET returns everything; POST is a request-key-first read-modify-write that broadcasts (403 online / 400 unparsable body / 500 write failure) |
| Shell config | 24061 `/shell-config` | GET returns `{config, resolved:{etternaRoot,malodyRoot,malody4Root}}` (`resolved` = the paths actually adopted); POST is a request-key-first read-modify-write (400 = bad body / unreadable base / write failure, and nothing is written then); on success the root-detection caches are cleared and a settings frame is broadcast |
| Settings window | 24061 `/open-settings` + `/settings.html` | POST opens/focuses the settings window (window creation is dispatched asynchronously; 503 without an app handle); `settings.html` is the page it hosts (shell config + plugin settings + the full preset manager), served by the static branch |
| Cover | 24061 `/cover/...` | Whitelisted concrete files (image extensions, URL sent in same frame) |
| Etterna polling | — | 2Hz on `Save/MmaBridge.txt`/`Save/MmaGameplay.txt` (first read = baseline, no push) |
| Malody polling | — | 1.5s over chart/ (two-level dir mtime fast filter; only stats dir layers) |
| Malody 4.3.7 read-only observation | `malody4/` modules | One 200 ms tick: anchor identity key + game-log tail for the scene + `config.json` polling for speed mods/judge; **zero files written into the game folder**, no injection; the chart-library index (streamed md5) is built on a background thread |
| tosu probe | - | `tosu.env` upward search (2–3 levels) → `GET {ip}:{port}/` health check (30s cycle) |

Window: transparent / always-on-top / click-through (`set_ignore_cursor_events`, toggled by shortcuts); `WebviewUrl::External` points to the tosu plugin page (online) or `http://127.0.0.1:24061/` (offline). Global shortcuts and window state (pos/size/topmost/click-through) persist to `mma-shell-state.json`. On **Wayland sessions** global shortcuts register but never fire (XGrabKey is a no-op there) — while the shell window is focused, in-page shortcuts take over via control frames (same key bindings); Windows/X11 behavior is unchanged.

**Settings window** (a second OS window, label `settings`): hosts `http://127.0.0.1:24061/settings.html` (shell config + plugin settings + the full preset manager). Three entry points — the global shortcut `hotkeys.settings` (`Ctrl+Shift+S` by default), the CLI `--settings` (with an instance already running it forwards `POST /open-settings` and exits 0 without creating any window), and `POST /open-settings` (browser/script; a missing `settings.html` logs a warning and opens nothing). Its geometry goes to a **separate file** `mma-shell-settings-window.json` (never mixed with the main window's `mma-shell-state.json`); while it is open the main window's topmost is temporarily cancelled (**without** writing the main window's state file — restored from it on close/failure). Read-only online, writable offline (see the "Settings window" section of `docs/shell-guide.md` and §4 of `docs/features/desktop-shell.md`).

Shell config (next to the exe, auto-created on first run): `mma-shell-config.json` (`gameClient`/`etternaRoot`/`malodyRoot`/`malody4Root`/`hotkeys`/`logLevel`; `malody4Root` is written by the installer's **Malody 4** option, which copies zero files) and `mma-settings.json` (full plugin settings; the offline authority — while tosu is online the tosu settings file replaces it and no local file is created or used).

Build & run (dev): `cargo run --release`; packaging: `desktop/release.ps1` (Windows, zip) / `desktop/build-linux.sh` (Linux, tar.gz; needs the Tauri Linux system deps, same as CI).

Dev env vars:

- `MMA_PLUGIN_DIR`: override plugin folder resolution (default: exe's own dir first, upward probe for `ManiaMapAnalyser by Leo_Black` fallback)
- `MMA_SKIP_TOSU_PROBE=1`: skip tosu.env probe (force offline mode)
- `MMA_ETTERNA_ROOT` / `MMA_MALODY_ROOT`: override game roots (env > config > auto-detect)
- `MMA_MALODY4_ROOT`: override the Malody 4.3.7 root (same precedence; any non-empty value is accepted, so pointing it at a nonexistent path is how you disable that source)

Logs: `logs/mma-shell-YYYYMMDD.log` (daily rotation, 7 kept; filtered by `logLevel`; success=debug, failure=error).
