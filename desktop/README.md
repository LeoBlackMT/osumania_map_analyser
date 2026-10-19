# ManiaMapAnalyser 桌面壳

ManiaMapAnalyser 桌面壳（mma-shell）基于 Tauri v2 开发，支持 Windows 与 Linux。桌面壳承载插件的网页前端，在置顶透明窗口中显示分析卡片，并负责聚合与分发来自多个音游客户端的数据。

用户操作指引参见 [docs/shell-guide.md](../docs/shell-guide.md)。技术架构说明参见 [docs/features/desktop-shell.md](../docs/features/desktop-shell.md) 及 [docs/CONTRACT.md](docs/CONTRACT.md)。

## 服务端口与核心模块

| 端口/模块 | 用途 | 说明 |
| --- | --- | --- |
| 24060 | Malody 编辑器通道 | 接收 Malody V 编辑器生成的分析请求并提供解析接口 |
| 24061 `/` | 静态资源服务 | 提供插件目录下的静态页面，包括主卡片、设置页与预设管理器 |
| 24061 `/ws` | 主 WebSocket 通道 | 推送状态、谱面数据、设置和控制命令等帧数据 |
| 24061 `/settings` | 插件设置接口 | 读取与写入卡片设置，tosu 在线时读取 tosu 配置，离线时读写 `mma-settings.json` |
| 24061 `/shell-config` | 壳配置接口 | 读取与修改 `mma-shell-config.json`，包括游戏路径、快捷键与日志等级 |
| 24061 `/open-settings` | 设置窗口唤起 | 触发打开独立的设置窗口 |
| 24061 `/offsets/*` | 偏移表管理 | 提供偏移表状态查询、活体生成及远端同步接口 |
| 24061 `/cover/*` | 封面代理 | 转发谱面背景图片资源 |
| 24062 | osu! 兼容服务 | 模拟 tosu 的 `/websocket/v2` 与谱面文件服务接口，供页面按原生方式读取数据 |
| `osu/` | osu! 内存读取模块 | 在 Windows 下通过读取内存直接获取 osu!stable 和 osu!lazer 的游戏状态 |
| `malody4/` | Malody 4.3.7 读取模块 | 在 Windows 下通过内存与日志观察 Malody 4.3.7 的选歌和游玩状态 |
| Etterna 轮询 | 外部文件读取 | 轮询 Etterna 桥接脚本写入的数据文件 |

## RC 难度反馈（默认开启，可关闭）

- 只有插件设置 `enableDifficultyFeedback` 打开、且页面由壳承载（24061 或壳追加的 `#mma-shell` 标记）时，结算屏底部才出现五档评分条。
- 页面只提交评分与显示值；壳从 native 快照重算客观字段（谱面 ID/checksum、mod、命中数、ScoreV1 权重）并用 Ed25519 签名。签名种子由构建期 `MMA_FEEDBACK_SIGNING_SEED` / `MMA_FEEDBACK_KEY_ID` 注入，未设置时使用 dev seed（后端默认拒绝）。
- 本机端点：`POST /feedback`（24061）。壳不直接访问外网，签名信封由页面转发到后端。

## 窗口与界面交互

- 主窗口：支持透明背景、无边框拖拽、窗口缩放、窗口置顶与点击穿透。相关状态（位置、尺寸、置顶、穿透）自动记录在 `mma-shell-state.json` 中。
- 设置窗口：独立的配置窗口，承载 `settings.html`。可通过全局快捷键 `Ctrl+Shift+S`、命令行参数 `--settings` 或接口调用打开。
- 全局快捷键：默认提供置顶（`Ctrl+Shift+T`）、点击穿透（`Ctrl+Shift+C`）、打开设置（`Ctrl+Shift+S`）以及退出（`Ctrl+Q`）。快捷键可在 `mma-shell-config.json` 中自定义。在 Linux Wayland 环境下，全局快捷键受限时由窗口聚焦后的内部快捷键处理。

## 平台支持情况

- Windows：支持全部功能，包括 osu! 原生内存读取、Malody 4.3.7 只读观察、Malody V 桥接以及 Etterna 桥接。
- Linux：支持静态界面展示与 Etterna 桥接。osu! 内存读取与 Malody 4.3.7 依赖 Windows API，在 Linux 下不可用，osu! 数据将回落至 tosu。

## 配置文件说明

配置文件位于可执行文件同级目录下：
- `mma-shell-config.json`：壳程序运行配置，包括数据源偏好、各游戏根目录、快捷键映射与日志等级。
- `mma-settings.json`：卡片显示设置。仅在 tosu 离线时由桌面壳负责维护；tosu 在线时直接采用 tosu 的设置文件。
- `mma-shell-state.json`：窗口运行状态（主窗口/设置窗口几何）+ 反馈用的匿名 `install_id`。旧版 `mma-shell-feedback.json` 会自动并入并删除。

## 构建与运行

开发态运行：
```bash
cargo run --release
```

打包发布：
- Windows：运行根目录下的 `release.ps1`。
- Linux：运行 `desktop/build-linux.sh`。

完整构建说明（seed 存放、Docker、CI、验证）见 [BUILD.md](BUILD.md)。

两个发布脚本都要求 RC 反馈签名 seed（缺失会直接失败，避免发出 dev 签名包）；默认从仓库内 `.secrets\feedback-seed.txt` 读取，不需要环境变量：

```bash
# Windows PowerShell
$env:MMA_FEEDBACK_SIGNING_SEED = "<64 hex>"
$env:MMA_FEEDBACK_KEY_ID = "release-2026-10"
.\release.ps1

# Linux（原生）
export MMA_FEEDBACK_SIGNING_SEED="<64 hex>"
export MMA_FEEDBACK_KEY_ID="release-2026-10"
desktop/build-linux.sh

# Linux（在 Windows 上用 Docker，无需本机 Linux 工具链）
.\release.ps1 -Linux          # 只出 Linux 包
.\release.ps1 -All            # Windows + Linux
```

Docker 镜像由 `desktop/docker/Dockerfile` 定义（`rust:1-bookworm` + Tauri 系统库），首次运行自动构建；seed 通过 `-e` 在运行时注入，不写进镜像。

- 只检查签名配置：`release.ps1 -SigningCheckOnly` / `build-linux.sh --check-signing`。
- 本地测试构建：`release.ps1 -AllowDevSigning` / `MMA_FEEDBACK_ALLOW_DEV=1 build-linux.sh`（服务端需 `FEEDBACK_ALLOW_DEV_KEY=true`）。
- seed 绝不入库；公钥填服务端 `FEEDBACK_PUBLIC_KEYS`。详见 `backend/docs/deployment.md` §14。

常用开发环境变量：
- `MMA_PLUGIN_DIR`：覆盖插件静态资源目录路径。
- `MMA_SKIP_TOSU_PROBE=1`：跳过 tosu 探测，强制运行在离线模式。
- `MMA_ETTERNA_ROOT` / `MMA_MALODY_ROOT` / `MMA_MALODY4_ROOT`：覆盖对应游戏的根目录路径。
- `MMA_LAZER_OFFSETS`：显式指定 lazer 偏移表文件路径。
- `MMA_OSU_COMPAT_REPLAY=1`：将 24062 端口切换为回放固定测试数据。

---

# ManiaMapAnalyser Desktop Shell

The ManiaMapAnalyser desktop shell (mma-shell) is developed with Tauri v2 and supports both Windows and Linux. It hosts the overlay web frontend, displaying the analysis card in an always-on-top transparent window, and aggregates data from multiple rhythm game clients.

For end-user operation guides, see [docs/shell-guide.md](../docs/shell-guide.md). For technical architecture details, refer to [docs/features/desktop-shell.md](../docs/features/desktop-shell.md) and [docs/CONTRACT.md](docs/CONTRACT.md).

## Ports and Core Modules

| Port / Module | Purpose | Description |
| --- | --- | --- |
| 24060 | Malody editor channel | Receives analysis requests from the Malody V editor and exposes resolution endpoints |
| 24061 `/` | Static file server | Serves frontend assets, including the overlay card, settings page, and presets manager |
| 24061 `/ws` | Main WebSocket stream | Broadcasts frames for state, beatmap data, settings, and controls |
| 24061 `/settings` | Plugin settings API | Reads and writes card display settings; uses tosu values when tosu is online, and `mma-settings.json` when offline |
| 24061 `/shell-config` | Shell configuration API | Reads and updates `mma-shell-config.json` for paths, hotkeys, and log levels |
| 24061 `/open-settings` | Settings window trigger | Dispatches requests to open or focus the settings window |
| 24061 `/offsets/*` | Offsets management | Provides offset status queries, live table generation, and remote update endpoints |
| 24061 `/cover/*` | Cover proxy | Proxies beatmap background image assets |
| 24062 | osu! compatible origin | Emulates tosu's `/websocket/v2` and beatmap file endpoints for native reading paths |
| `osu/` | osu! memory reader | Reads osu!stable and osu!lazer game states directly from memory on Windows |
| `malody4/` | Malody 4.3.7 reader | Observes Malody 4.3.7 song selection and gameplay via process memory and logs on Windows |
| Etterna polling | File polling | Polls file outputs generated by the Etterna bridge scripts |

## RC difficulty feedback (on by default, can be turned off)

- The five-point strip only appears on the result screen when `enableDifficultyFeedback` is on and the page is hosted by the shell (port 24061 or the `#mma-shell` marker).
- The page submits only the rating and the displayed value; the shell rebuilds the objective fields (beatmap ID/checksum, mods, hits, ScoreV1 weight) from its native snapshot and signs them with Ed25519. The signing seed is injected at build time via `MMA_FEEDBACK_SIGNING_SEED` / `MMA_FEEDBACK_KEY_ID`; without them a dev seed is used (rejected by the server by default).
- Local endpoint: `POST /feedback` (24061). The shell never calls the internet directly — the page forwards the signed envelope.

## Window and Interaction

- Main Window: Supports transparency, borderless dragging, resizing, always-on-top, and click-through mode. Window geometry and toggle states are saved in `mma-shell-state.json`.
- Settings Window: A dedicated configuration window hosting `settings.html`. Can be opened via the global shortcut `Ctrl+Shift+S`, CLI flag `--settings`, or the HTTP trigger endpoint.
- Global Hotkeys: Default hotkeys include always-on-top (`Ctrl+Shift+T`), click-through (`Ctrl+Shift+C`), settings (`Ctrl+Shift+S`), and close (`Ctrl+Q`). Keys can be customized in `mma-shell-config.json`. Under Linux Wayland where global shortcuts may not trigger, internal hotkeys handle events when the window is focused.

## Platform Support

- Windows: Full support for all features, including osu! native memory reading, Malody 4.3.7 external observation, Malody V bridge, and Etterna bridge.
- Linux: Supports the overlay frontend and Etterna bridge. The native memory reader and Malody 4.3.7 rely on Windows APIs and are marked unsupported on Linux, where osu! falls back to tosu.

## Configuration Files

Configuration files are located in the executable directory:
- `mma-shell-config.json`: Shell runtime configuration including source preference, game roots, hotkey definitions, and log levels.
- `mma-settings.json`: Card display settings. Managed by the shell when tosu is offline; replaced by tosu's settings file when tosu is online.
- `mma-shell-state.json`: Window runtime state (geometry for the main/settings windows) plus the anonymous `install_id` used by feedback. The legacy `mma-shell-feedback.json` is migrated into it and removed.

## Building and Development

Run in development mode:
```bash
cargo run --release
```

Build release packages:
- Windows: Run `release.ps1` from the repository root.
- Linux: Run `desktop/build-linux.sh`.

See [BUILD.md](BUILD.md) for the full build guide (seed location, Docker, CI, verification).

Both release scripts require the RC feedback signing seed and fail without it, so a dev-signed build cannot be shipped by accident; the seed is read from `.secrets\feedback-seed.txt` by default:

```bash
# Windows PowerShell
$env:MMA_FEEDBACK_SIGNING_SEED = "<64 hex>"
$env:MMA_FEEDBACK_KEY_ID = "release-2026-10"
.\release.ps1

# Linux
export MMA_FEEDBACK_SIGNING_SEED="<64 hex>"
export MMA_FEEDBACK_KEY_ID="release-2026-10"
desktop/build-linux.sh
```

- Signing check only: `release.ps1 -SigningCheckOnly` / `build-linux.sh --check-signing`.
- Local test build: `release.ps1 -AllowDevSigning` / `MMA_FEEDBACK_ALLOW_DEV=1 build-linux.sh` (the server needs `FEEDBACK_ALLOW_DEV_KEY=true`).
- Never commit the seed; put the public key in the server's `FEEDBACK_PUBLIC_KEYS`. See `backend/docs/deployment.md` §14.
- Linux via Docker on Windows: `.\release.ps1 -Linux` (or `-All`). The image is `desktop/docker/Dockerfile` (`rust:1-bookworm` + Tauri deps) and the seed is passed at run time with `-e`, never baked into the image.

Common development environment variables:
- `MMA_PLUGIN_DIR`: Overrides the static asset directory path.
- `MMA_SKIP_TOSU_PROBE=1`: Skips tosu probing and forces offline mode.
- `MMA_ETTERNA_ROOT` / `MMA_MALODY_ROOT` / `MMA_MALODY4_ROOT`: Overrides respective game root paths.
- `MMA_LAZER_OFFSETS`: Explicitly specifies the path to a lazer offset table.
- `MMA_OSU_COMPAT_REPLAY=1`: Switches port 24062 to replay static test data.
