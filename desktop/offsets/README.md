# 内存偏移表说明

本目录包含桌面壳（`mma-shell`）直接读取 osu! 游戏数据所需的偏移配置文件及辅助生成工具。

> **提示**：发行包已内置适配主流版本的配置文件。启动 `mma-shell.exe` 后会自动识别并匹配游戏版本，**日常游玩通常不需要进行任何手动配置**。

> **回放暂停**：看回放时如果右下角显示 `Pause Data Unavailable`，说明当前 lazer 版本的偏移表缺少回放哈希字段（`score_hash`）、该成绩没有回放文件，或成绩来自 **2025.816.0 之前**的 lazer（那时只记录暂停次数、不记录时间点）。2026.921 的偏移表已移除（该构建原生读取不可用、回落 tosu）；2026.1005 / 2026.1115 / 2026.1117 已包含。需要该功能时，可在游玩/回放中与结算屏各运行一次 `gen.exe` 重新生成表（见下文「方式三」）。

## 目录结构

```text
offsets/
├── manifest.json       # 偏移表索引清单
├── gen.exe             # osu!lazer 偏移表生成工具
├── stable/             # osu!stable 适配文件
└── lazer/              # osu!lazer 各版本适配文件
```

> stable/ppysb__x86.json 用于 ppy.sb 注入版的基础内存读取；LaneAcc 使用与 stable 相同的 `Score.Seed` / RD 映射（表含 `score_seed` 段时可用）。

> 维护者：stable 表新增两个**可选**段——`topology.replay`（`Score+0x34` 原始按键帧）与
> `topology.notes`（HOM per-note 判定，列/档位）与 `topology.score_seed`（`Score.Seed`，0x80）。`tools/stable-offsets-gen` 重新生成时会
> 保留这两段；换 `osu!.exe` 构建后必须 live 自证再更新，缺段时 Stable 分轨显示占位。

## 当游戏更新后无法读取数据时

由于 osu!lazer 会不定期更新，如果更新后桌面壳无法读取谱面或状态，请按以下方式获取最新适配：

### 方式一：在设置中检查更新（推荐，最简单）
1. 按 `Ctrl+Shift+S` 打开桌面壳设置面板。
2. 进入 **Memory Offsets Management**（内存偏移表管理）。
3. 点击 **Check Remote Updates**（检查远端更新），程序会自动拉取并更新官方适配表。若部分表验签/schema 失败，设置页会显示失败文件名，不再静默显示成功。

### 方式二：在设置中自动生成
1. 启动 osu!lazer 并停留在选歌界面或主菜单。
2. 按 `Ctrl+Shift+S` 打开桌面壳设置面板，进入 **Memory Offsets Management**。
3. 点击 **Live Generator** 按钮，程序将自动扫描当前运行的 lazer 并生成新配置。

### 方式三：运行独立生成工具
1. 启动 osu!lazer 并停留在选歌界面或主菜单。
2. 双击运行本目录下的 `gen.exe`（如遇管理员权限提示请允许）。
3. 在控制台菜单中输入 `1` 并按回车执行扫描。工具将自动生成新配置并写入对应目录。
4. 生成完成后按回车退出，重启桌面壳即可生效。（控制台中输入 `3` 可切换语言）。

### 维护者：清单签名与校验

- 每张 manifest 表都有单调递增的 `revision`。lazer 的 revision 从 `lazer_version` 推导（`2026.1117` → `20261117`）；stable/ppysb 的 `"revision"` 写在表 JSON 内。远端更新先比较 revision，stable/ppysb 不再依赖哈希形式的 `version` 字符串。
- 偏移表 JSON 与 `manifest.json` 都按精确字节签名，必须保持 LF。`.gitattributes` 已强制 `text eol=lf`，不要用会写 CRLF 的编辑器修改。manifest schema 3 还包含 `manifest_signature`，用于绑定清单元数据。
- 签名 seed 不进入仓库（例如 `.secrets/offsets-seed.txt`，已被 `.gitignore` 覆盖）。不要提交、记录或粘贴 seed。
- 使用离线工具签名/校验：

```powershell
cargo run --manifest-path tools/offsets-tool/Cargo.toml -- verify --root desktop/offsets
cargo run --manifest-path tools/offsets-tool/Cargo.toml -- sign --root desktop/offsets --seed-file .secrets/offsets-seed.txt
```

- `cargo test --lib` 会以只读方式校验 `manifest.json` 与所有随包表（SHA-256 + Ed25519，LF 字节）；文件缺失或过期会直接失败。
- 重新生成 stable/ppysb 表时，`tools/stable-offsets-gen` 会写入当天 `revision`；同一天多次发布时用 `--revision <更大的数>` 手动递增。lazer 表的 revision 由 `lazer_version` 推导，不需要额外字段。

---

# Memory Offset Tables Guide

This directory contains configuration files and helper tools used by the desktop shell (`mma-shell`) to read osu! game data directly from memory.

> **Note**: Bundled offset tables already support common versions of osu!stable and osu!lazer. When `mma-shell.exe` starts, it automatically matches the game client. **No manual configuration is required under normal use.**

> **Replay pauses**: if the overlay shows `Pause Data Unavailable` while watching a replay, the offset table for this lazer build lacks the replay hash fields (`score_hash`), the score has no replay file, or the score was produced by a lazer build **older than 2025.816.0** (those only stored a pause count, not timestamps). The 2026.921 offset tables were removed (that build falls back to tosu); 2026.1005 / 2026.1115 / 2026.1117 do. To enable the feature, run `gen.exe` once during play/replay and once at the results screen (see "Method 3" below).

## Directory Layout

```text
offsets/
├── manifest.json       # Manifest index for offset tables
├── gen.exe             # Offset table generator for osu!lazer
├── stable/             # Offset tables for osu!stable
└── lazer/              # Offset tables for osu!lazer releases
```

## Troubleshooting After Game Updates

When osu!lazer updates and the shell can no longer read game data, use one of the following methods:

### Method 1: Check Remote Updates (Recommended)
1. Press `Ctrl+Shift+S` to open the shell settings panel.
2. Go to **Memory Offsets Management**.
3. Click **Check Remote Updates** to download the latest official offset tables. If some tables fail signature/schema checks, the settings panel now shows the failing filenames instead of reporting a silent success.

### Method 2: Live Generator in Settings
1. Keep osu!lazer running on the song select screen or main menu.
2. Press `Ctrl+Shift+S` to open shell settings and go to **Memory Offsets Management**.
3. Click **Live Generator**; the shell will scan the running lazer process and generate the table.

### Method 3: Run the Standalone Generator Tool
1. Keep osu!lazer running on the song select screen or main menu.
2. Launch `gen.exe` in this directory (approve administrator prompt if requested).
3. Type `1` in the console menu and press Enter to scan. The tool generates and installs the new table automatically.
4. Press Enter to exit once finished, then restart the desktop shell. (Type `3` in the menu to switch languages.)

## Maintainer: manifest signing and verification

- Every manifest table carries a monotonic `revision`. Lazer revisions are derived from `lazer_version` (`2026.1117` -> `20261117`); stable/ppysb carry `"revision"` inside the table JSON. Remote updates compare revisions first, so stable/ppysb no longer depend on their hash-like `version` strings.
- Offset JSON files and `manifest.json` are signed as exact bytes and must stay LF. `.gitattributes` enforces `text eol=lf`; do not edit with an editor that writes CRLF. Manifest schema 3 also carries `manifest_signature`, which binds the manifest metadata.
- The signing seed stays outside the repository (for example `.secrets/offsets-seed.txt`, gitignored). Never commit, log, or paste it.
- Sign and verify with the offline tool:

```powershell
cargo run --manifest-path tools/offsets-tool/Cargo.toml -- verify --root desktop/offsets
cargo run --manifest-path tools/offsets-tool/Cargo.toml -- sign --root desktop/offsets --seed-file .secrets/offsets-seed.txt
```

- `cargo test --lib` also verifies the packaged `manifest.json` against every packaged table (SHA-256 + Ed25519, LF bytes) and fails if any file is missing or stale.
- `tools/stable-offsets-gen` writes the current UTC date as `revision`; pass `--revision <larger>` for a second publish on the same day. Lazer revisions are derived from `lazer_version`, so no extra field is needed.
