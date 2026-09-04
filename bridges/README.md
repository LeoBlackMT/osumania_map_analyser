# bridges/ — 游戏侧桥文件

游戏侧注入物（Lua），供桌面壳的数据源通道使用。安装方法详见 [docs/shell-guide.md](https://github.com/LeoBlackMT/osumania_map_analyser/blob/main/docs/shell-guide.md)：

- Etterna：`bridges/etterna/mma_bridge.lua` + `mma_gameplay.lua` —— 主题目录、LoadActor 注入、主题更新后重装；
- Malody V：`bridges/malody/mma_editor.lua`（编辑器插件）—— 在 `MalodyV/Editor/` 放置，编辑器菜单 MMA Analyze 按钮触发分析。

## 自动安装 / 卸载（推荐）

面向不熟悉手动操作的用户，本目录提供交互式安装器：

- 双击 `install-bridge.bat`（或命令行运行 `powershell -NoProfile -ExecutionPolicy Bypass -File install-bridge.ps1`）。
- 主菜单选择 **Install** / **Uninstall**，再选择游戏（Etterna / Malody V）。全程交互确认。
- 游戏根目录自动探测：正在运行的进程 → `MMA_ETTERNA_ROOT` / `MMA_MALODY_ROOT` 环境变量 → Steam 库（注册表 + `steamapps/libraryfolders.vdf`）→ 常见安装路径 → 手动输入兜底。
- Etterna 默认安装到 **Rebirth** 主题（不存在时列出主题供选择；不提供一次性安装到全部主题）。
- 写入/更新插件根目录（`bridges/..`，即 `mma-shell.exe` 旁）的 `mma-shell-config.json` 的 `etternaRoot` / `malodyRoot`（正斜杠路径）。
- 与已安装的其他脚本（如 DanOverlay、elements/titlesplash）共存：只新增/移除自己的一行 `LoadActor`，不触碰他人注入行。
- `default.lua` 不存在时报错并跳过该屏（绝不自动创建、绝不盲目注入）。
- 高级参数：`-Game Etterna|Malody`、`-Uninstall`、`-Yes`（自动化）、`-Root <path>`、`-Theme <name>`、`-ConfigPath <path>`。

工作原理（替代手动步骤）：
1. Etterna：复制两个 lua 到 `Themes\<theme>\BGAnimations\{ScreenSelectMusic decorations, ScreenGameplay overlay}\`，并在各自 `default.lua` 的 `return t` 前注入 `t[#t + 1] = LoadActor("<file>")`（幂等，注入前备份 `default.lua.mma-backup`）。
2. Malody V：复制 `mma_editor.lua` 到 `MalodyV/editor/`（目录不存在则创建）。
3. 写配置：读取→更新→写回 `mma-shell-config.json`（保留 `gameClient`/`hotkeys`/`logLevel` 等既有字段；UTF-8 无 BOM）。

卸载：删除注入行（仅自己的行）+ 删除桥文件 + 删除备份；询问是否同时清空配置中的 `etternaRoot` / `malodyRoot`（默认不清）。

# bridges/ — game-side bridge files

Lua injection assets for the shell's data-source channels. Follow the bridge header comments and `docs/shell-guide.md` for installation (Etterna theme directories with `LoadActor` injection and re-install after theme updates; Malody Editor/ plugin triggered from the editor More menu — the in-game skin display was removed).

## Automated install / uninstall (recommended)

For users unfamiliar with manual steps, this folder ships an interactive installer:

- Double-click `install-bridge.bat` (or run `powershell -NoProfile -ExecutionPolicy Bypass -File install-bridge.ps1`).
- Main menu: **Install** / **Uninstall**, then pick the game (Etterna / Malody V). Every step asks for confirmation.
- Game roots are auto-detected: running process → `MMA_ETTERNA_ROOT` / `MMA_MALODY_ROOT` env vars → Steam libraries (registry + `steamapps/libraryfolders.vdf`) → common install paths → manual entry fallback.
- Etterna installs into the **Rebirth** theme by default (falls back to a theme picker if missing; installing to all themes at once is intentionally not offered).
- Writes/updates `etternaRoot` / `malodyRoot` (forward-slash paths) in `mma-shell-config.json` next to the plugin root (`bridges/..`, i.e. next to `mma-shell.exe`).
- Coexists with other installed scripts (e.g. DanOverlay, elements/titlesplash): only its own `LoadActor` line is added/removed; other injections are never touched.
- A missing `default.lua` is an error and that screen is skipped (never auto-created, never blindly injected).
- Advanced flags: `-Game Etterna|Malody`, `-Uninstall`, `-Yes` (automation), `-Root <path>`, `-Theme <name>`, `-ConfigPath <path>`.

What it does (replacing the manual steps):
1. Etterna: copies the two lua files into `Themes\<theme>\BGAnimations\{ScreenSelectMusic decorations, ScreenGameplay overlay}\` and injects `t[#t + 1] = LoadActor("<file>")` before `return t` in each `default.lua` (idempotent; backs up `default.lua.mma-backup` first).
2. Malody V: copies `mma_editor.lua` into `MalodyV/editor/` (creates the folder if missing).
3. Config: read → update → write back `mma-shell-config.json` (preserves `gameClient`/`hotkeys`/`logLevel` and any other fields; UTF-8 without BOM).

Uninstall: removes only its own injection lines, deletes the bridge files and its backup; optionally clears `etternaRoot` / `malodyRoot` from the config (default: keep).