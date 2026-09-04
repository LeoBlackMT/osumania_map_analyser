# bridges/ — 游戏侧桥文件

游戏侧注入物（Lua），供桌面壳的数据源通道使用。安装方法详见 [docs/shell-guide.md](https://github.com/LeoBlackMT/osumania_map_analyser/blob/main/docs/shell-guide.md)：

- Etterna：`bridges/etterna/mma_bridge.lua` + `mma_gameplay.lua` —— 主题目录、LoadActor 注入、主题更新后重装；
- Malody V：`bridges/malody/mma_editor.lua`（编辑器插件）—— 在 `MalodyV/Editor/` 放置，编辑器菜单 MMA Analyze 按钮触发分析。

## 自动安装 / 卸载（推荐）

面向不熟悉手动操作的用户，本目录提供交互式安装器：

- 双击 `install-bridge.bat`（英文界面）或 `install-bridge-zh.bat`（中文界面）即可；命令行运行：`powershell -NoProfile -ExecutionPolicy Bypass -File install-bridge.ps1 [-Chinese]`。
- 主菜单选择 **Install** / **Uninstall**，再选择游戏（Etterna / Malody V）。全程交互确认。
- 游戏根目录自动探测：正在运行的进程 → `MMA_ETTERNA_ROOT` / `MMA_MALODY_ROOT` 环境变量 → 常见安装路径；Malody V（Steam 游戏）额外探测 Steam 库（注册表 + `steamapps/libraryfolders.vdf`），Etterna 不是 Steam 应用、不走 Steam 检测。
- **自动探测失败时**：可从「浏览选择目录」图形对话框选择、手动输入路径（`/`、`\`、`\\` 写法与包裹引号、尾部斜杠都能兼容归一化），或按提示查看「如何找到游戏路径」指引。
- **Etterna 主题预检**：安装前检查每个主题的目录结构，只列出结构完整（`ScreenSelectMusic decorations/default.lua` 与 `ScreenGameplay overlay/default.lua` 都存在）的主题；`_fallback` 这类缺少屏目录的主题会被跳过并说明原因。默认安装到 **Rebirth**（存在时；否则列出可安装主题供选择；不提供一次性安装到全部主题）。
- 写入/更新插件根目录（`bridges/..`，即 `mma-shell.exe` 旁）的 `mma-shell-config.json` 的 `etternaRoot` / `malodyRoot`（正斜杠路径）。
- 与已安装的其他脚本（如 DanOverlay、elements/titlesplash）共存：只新增/移除自己的一行 `LoadActor`，不触碰他人注入行。
- `default.lua` 不存在时报错并跳过该屏（绝不自动创建、绝不盲目注入）。
- 高级参数：`-Game Etterna|Malody`、`-Uninstall`、`-Chinese`（中文界面）、`-Yes`（自动化）、`-Root <path>`、`-Theme <name>`、`-ConfigPath <path>`。

工作原理（替代手动步骤）：
1. Etterna：复制两个 lua 到 `Themes\<theme>\BGAnimations\{ScreenSelectMusic decorations, ScreenGameplay overlay}\`，并在各自 `default.lua` 的 `return t` 前注入 `t[#t + 1] = LoadActor("<file>")`（幂等，注入前备份 `default.lua.mma-backup`）。
2. Malody V：复制 `mma_editor.lua` 到 `MalodyV/editor/`（目录不存在则创建）。
3. 写配置：读取→更新→写回 `mma-shell-config.json`（保留 `gameClient`/`hotkeys`/`logLevel` 等既有字段；UTF-8 无 BOM）。

卸载：删除注入行（仅自己的行）+ 删除桥文件 + 删除备份；询问是否同时清空配置中的 `etternaRoot` / `malodyRoot`（默认不清）。

# bridges/ — game-side bridge files

Lua injection assets for the shell's data-source channels. Follow the bridge header comments and `docs/shell-guide.md` for installation (Etterna theme directories with `LoadActor` injection and re-install after theme updates; Malody Editor/ plugin triggered from the editor More menu — the in-game skin display was removed).

## Automated install / uninstall (recommended)

For users unfamiliar with manual steps, this folder ships an interactive installer:

- Double-click `install-bridge.bat` (English UI) or `install-bridge-zh.bat` (Chinese UI); or run `powershell -NoProfile -ExecutionPolicy Bypass -File install-bridge.ps1 [-Chinese]`.
- Main menu: **Install** / **Uninstall**, then pick the game (Etterna / Malody V). Every step asks for confirmation.
- Game roots are auto-detected: running process → `MMA_ETTERNA_ROOT` / `MMA_MALODY_ROOT` env vars → common install paths; Malody V (a Steam game) additionally probes Steam libraries (registry + `steamapps/libraryfolders.vdf`), while Etterna is not a Steam app and is never probed there.
- **If auto-detection finds nothing**: pick the folder from a browse dialog, type a path (`/`, `\` or `\\` separators, wrapping quotes and trailing slashes are all normalized), or follow the built-in "how do I find the path" hint.
- **Etterna theme pre-check**: each theme's structure is validated before listing — only themes with both `ScreenSelectMusic decorations/default.lua` and `ScreenGameplay overlay/default.lua` present are offered; incomplete themes (like `_fallback`) are skipped with the reason shown. Installs into **Rebirth** by default (falls back to the installable-theme picker if missing; installing to all themes at once is intentionally not offered).
- Writes/updates `etternaRoot` / `malodyRoot` (forward-slash paths) in `mma-shell-config.json` next to the plugin root (`bridges/..`, i.e. next to `mma-shell.exe`).
- Coexists with other installed scripts (e.g. DanOverlay, elements/titlesplash): only its own `LoadActor` line is added/removed; other injections are never touched.
- A missing `default.lua` is an error and that screen is skipped (never auto-created, never blindly injected).
- Advanced flags: `-Game Etterna|Malody`, `-Uninstall`, `-Chinese` (Chinese UI), `-Yes` (automation), `-Root <path>`, `-Theme <name>`, `-ConfigPath <path>`.

What it does (replacing the manual steps):
1. Etterna: copies the two lua files into `Themes\<theme>\BGAnimations\{ScreenSelectMusic decorations, ScreenGameplay overlay}\` and injects `t[#t + 1] = LoadActor("<file>")` before `return t` in each `default.lua` (idempotent; backs up `default.lua.mma-backup` first).
2. Malody V: copies `mma_editor.lua` into `MalodyV/editor/` (creates the folder if missing).
3. Config: read → update → write back `mma-shell-config.json` (preserves `gameClient`/`hotkeys`/`logLevel` and any other fields; UTF-8 without BOM).

Uninstall: removes only its own injection lines, deletes the bridge files and its backup; optionally clears `etternaRoot` / `malodyRoot` from the config (default: keep).