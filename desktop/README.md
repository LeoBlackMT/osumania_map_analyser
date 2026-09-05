# ManiaMapAnalyser desktop shell

Tauri v2 桌面壳（Windows）：加载同一插件页面（在线 = tosu 插件页 / 离线 = 本壳 24061 静态服务），并作为本地聚合桥。人类使用教程见 [docs/shell-guide.md](../docs/shell-guide.md)；技术说明见 [docs/features/desktop-shell.md](../docs/features/desktop-shell.md) 与 `docs/CONTRACT.md`（版本 2）。

| 能力 | 端口/路径 | 说明 |
|---|---|---|
| Malody 编辑器通道 | 24060 | 文件通道为主：编辑器 WriteFile `*_mma_request.json` → 壳扫描 → 谱面 = 同目录 `<base>.mc|.osu` → song 帧 → 页面分析 → 卡片展示（不回写 txt）。另保留 POST/GET resolve（诊断/备用） |
| 静态服务（离线） | 24061 `/` | 服务插件目录（exe 所在目录优先；兼容上溯探测） |
| WS | 24061 `/ws` | hello/state/song/settings/result/control/ping 帧（见 CONTRACT.md） |
| 设置 | 24061 `/settings` | 优先级：tosu 设置文件（`settings/<插件目录名>.values.json`）> `mma-settings.json` > settings.json 生成默认 |
| 封面 | 24061 `/cover/...` | 白名单具体文件（图片扩展名，同帧下发 URL） |
| Etterna 轮询 | — | 2Hz 轮询 `Save/MmaBridge.txt`/`Save/MmaGameplay.txt`（首读基线不推送） |
| Malody 轮询 | — | 1.5s 轮询 chart/（两级目录 mtime 快筛，仅 stat 目录层） |
| tosu 探测 | - | `tosu.env` 逐级向上探测（2–3 层）→ `GET {ip}:{port}/` 健康检查（30s 周期） |

窗口：透明/置顶/点击穿透（`set_ignore_cursor_events`，快捷键切换）；`WebviewUrl::External` 指向 tosu 插件页（在线）或 `http://127.0.0.1:24061/`（离线）。全局快捷键与窗口状态（位置/尺寸/置顶/穿透）持久化到 `mma-shell-state.json`。

壳配置（exe 旁，首启自动生成）：`mma-shell-config.json`（`gameClient`/`etternaRoot`/`malodyRoot`/`hotkeys`/`logLevel`）与 `mma-settings.json`（全量插件设置；离线模式；tosu 设置文件存在时优先 tosu）。

构建与运行（开发态）：`cargo run --release`；或 clone 整个仓库并运行 `desktop/release.ps1`。

开发环境变量：

- `MMA_PLUGIN_DIR`：覆盖插件目录解析（缺省：exe 所在目录优先，兼容上溯探测 `ManiaMapAnalyser by Leo_Black`）
- `MMA_SKIP_TOSU_PROBE=1`：跳过 tosu.env 探测（强制离线模式）
- `MMA_ETTERNA_ROOT` / `MMA_MALODY_ROOT`：覆盖游戏根目录（优先于配置/自动探测）

日志：`logs/mma-shell-YYYYMMDD.log`（按日轮转保留 7；`logLevel` 过滤；成功=debug、失败=error）。

---

# ManiaMapAnalyser desktop shell (English)

Tauri v2 desktop shell (Windows): loads the same plugin page (online = tosu plugin page / offline = shell's 24061 static server) and acts as a local aggregation bridge. Human-facing guide: [docs/shell-guide.md](../docs/shell-guide.md); technical details: [docs/features/desktop-shell.md](../docs/features/desktop-shell.md) and `docs/CONTRACT.md` (contract v2).

| Capability | Port/Path | Notes |
|---|---|---|
| Malody editor channel | 24060 | File channel primary: editor WriteFile `*_mma_request.json` → shell scans → chart = same-dir `<base>.mc\|.osu` → song frame → page analysis → shown on the card (no txt writeback). POST/GET resolve kept for diagnostics/fallback |
| Static server (offline) | 24061 `/` | Serves the plugin folder (exe's own dir first; upward probe fallback) |
| WS | 24061 `/ws` | hello/state/song/settings/result/control/ping frames (see CONTRACT.md) |
| Settings | 24061 `/settings` | Priority: tosu settings file (`settings/<plugin folder>.values.json`) > `mma-settings.json` > settings.json defaults |
| Cover | 24061 `/cover/...` | Whitelisted concrete files (image extensions, URL sent in same frame) |
| Etterna polling | — | 2Hz on `Save/MmaBridge.txt`/`Save/MmaGameplay.txt` (first read = baseline, no push) |
| Malody polling | — | 1.5s over chart/ (two-level dir mtime fast filter; only stats dir layers) |
| tosu probe | - | `tosu.env` upward search (2–3 levels) → `GET {ip}:{port}/` health check (30s cycle) |

Window: transparent / always-on-top / click-through (`set_ignore_cursor_events`, toggled by shortcuts); `WebviewUrl::External` points to the tosu plugin page (online) or `http://127.0.0.1:24061/` (offline). Global shortcuts and window state (pos/size/topmost/click-through) persist to `mma-shell-state.json`.

Shell config (next to the exe, auto-created on first run): `mma-shell-config.json` (`gameClient`/`etternaRoot`/`malodyRoot`/`hotkeys`/`logLevel`) and `mma-settings.json` (full plugin settings; offline mode; tosu settings file wins when present).

Build & run (dev): `cargo run --release`; or clone the repo and run `desktop/release.ps1`.

Dev env vars:

- `MMA_PLUGIN_DIR`: override plugin folder resolution (default: exe's own dir first, upward probe for `ManiaMapAnalyser by Leo_Black` fallback)
- `MMA_SKIP_TOSU_PROBE=1`: skip tosu.env probe (force offline mode)
- `MMA_ETTERNA_ROOT` / `MMA_MALODY_ROOT`: override game roots (env > config > auto-detect)

Logs: `logs/mma-shell-YYYYMMDD.log` (daily rotation, 7 kept; filtered by `logLevel`; success=debug, failure=error).
