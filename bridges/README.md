# bridges/ — 游戏侧桥文件

游戏侧注入物（Lua），供桌面壳的数据源通道使用。安装方法详见 [docs/shell-guide.md](https://github.com/LeoBlackMT/osumania_map_analyser/blob/main/docs/shell-guide.md)：

- Etterna：`bridges/etterna/mma_bridge.lua` + `mma_gameplay.lua` —— 主题目录、LoadActor 注入、主题更新后重装；
- Malody V：`bridges/malody/mma_editor.lua`（编辑器插件）—— 在 `MalodyV/Editor/` 放置，编辑器菜单 MMA Analyze 按钮触发分析。
- Malody 4.3.7：**无需任何桥文件**（该源由桌面壳以零注入只读观察接入，游戏侧不安装任何东西）。

## 自动安装 / 卸载（推荐）

面向不熟悉手动操作的用户，本目录提供交互式安装器：

- 双击 `install-bridge.bat`（英文界面）或 `install-bridge-zh.bat`（中文界面）即可；命令行运行：`powershell -NoProfile -ExecutionPolicy Bypass -File install-bridge.ps1 [-Chinese]`。
- 主菜单选择 **Install** / **Uninstall**，再选择游戏（**Malody 4** / Etterna / Malody V）。选 Malody V 时会再问一次：**编辑器 Lua 插件** / **游戏内选曲桥** / 两者。全程交互确认。
- **Malody 4 选项零文件复制**：它只把 `malody4Root` 写进 `mma-shell-config.json`（游戏侧无需任何操作），卸载时只清空该键。
- 游戏根目录自动探测：正在运行的进程 → `MMA_ETTERNA_ROOT` / `MMA_MALODY_ROOT` 环境变量 → 常见安装路径；Malody V 额外探测 Steam 库（注册表 + `steamapps/libraryfolders.vdf`。
- **自动探测失败时**：可从「浏览选择目录」图形对话框选择、手动输入路径（`/`、`\`、`\\` 写法与包裹引号、尾部斜杠都能兼容归一化），或按提示查看「如何找到游戏路径」指引。
- **Etterna 主题预检**：安装前检查每个主题的目录结构，只列出结构完整（`ScreenSelectMusic decorations/default.lua` 与 `ScreenGameplay overlay/default.lua` 都存在）的主题；`_fallback` 这类缺少屏目录的主题会被跳过并说明原因。默认安装到 **Rebirth**（存在时；否则列出可安装主题供选择；不提供一次性安装到全部主题）。
- 写入/更新插件根目录（`bridges/..`，即 `mma-shell.exe` 旁）的 `mma-shell-config.json` 的 `etternaRoot` / `malodyRoot`（正斜杠路径）；选择 **Malody 4** 时只写 `malody4Root`（不复制任何文件）。
- 与已安装的其他脚本（如 DanOverlay、elements/titlesplash）共存：只新增/移除自己的一行 `LoadActor`，不触碰他人注入行。
- `default.lua` 不存在时报错并跳过该屏（绝不自动创建、绝不盲目注入）。
- 高级参数：`-Game Etterna|Malody|Malody4|MalodyBridge`、`-Uninstall`、`-Chinese`（中文界面）、`-Yes`（自动化）、`-Root <path>`、`-Theme <name>`、`-ConfigPath <path>`、**`-LoaderOnly`**（只装 Malody V 加载器、跳过插件 DLL）。
- **Malody V 游戏内选曲桥（`-Game MalodyBridge`）**：把 BepInEx 6 IL2CPP 加载器（`winhttp.dll`、`dotnet/`、`BepInEx/`）与本仓库**自建 fork** 的插件 DLL（`MMAMalodySelection.dll`）装进游戏目录。插件不是上游那份：上游桥与本 fork **会挂在同一批游戏方法上，两者共存等于双 Hook**，所以安装前若发现上游 `MalodyInsightBridge.dll` 仍在，安装器只**报告并给出指引，绝不删除**（删不删是你的决定）。
  - **加载器获取方式（必须逐字节匹配）**：从上游 BepInEx 6 IL2CPP `6.0.0-be.788` 的 GitHub Release 下载资产 `bepinex-il2cpp-788.zip`，保存为 `bridges/malody/bepinex/loader/bepinex-il2cpp-788.zip`（该目录不入库），或把环境变量 `MMA_BEPINEX_LOADER_ZIP` 指向它。安装前会校验 **SHA256 `F4CC496BD098A0DF4164B81E3737297707F13A47C2478DBA2F60EEFAB784817A`**（34,336,405 字节），不匹配即中止且**不写入任何文件**。
  - **Unity 参考程序集（同样必须逐字节匹配）**：`bridges/malody/bepinex/unity-libs/2022.3.62.zip`（该目录不入库），来源 <https://unity.bepinex.dev/libraries/2022.3.62.zip>，SHA256 **`575E7D600F69DE8200CCF4DB700B3AE6252366C22E8C3434C860E428974518D1`**（1,900,589 字节），`MMA_MALODY_UNITY_LIBS_ZIP` 可指向别处。安装器把它装到 `BepInEx\unity-libs\2022.3.62.zip`：BepInEx 首次启动本会自己下载这个文件、此后只复用不再校验，一次下载不完整就会让之后每次启动都失败（`End of Central Directory record could not be found` → 无插件加载）；已存在但哈希不符的文件会被替换，因此重跑安装器即可修复这种装坏的安装。
  - **`-LoaderOnly`**：只装加载器、不装插件 DLL。用于"先让 BepInEx 生成 `BepInEx/interop/`"或插件产物暂不可用时——首次安装加载器后**启动一次游戏**，BepInEx 会生成 `BepInEx\interop\`，之后不带该开关再跑一次即可补上插件。
  - **不写任何配置文件**：fork 已无游戏内叠加界面，`BepInEx\config\local.mma.malody.selection.cfg` 由 BepInEx 首次加载时自行生成，安装器既不创建也不删除它。

工作原理（替代手动步骤）：
1. Etterna：复制两个 lua 到 `Themes\<theme>\BGAnimations\{ScreenSelectMusic decorations, ScreenGameplay overlay}\`，并在各自 `default.lua` 的 `return t` 前注入 `t[#t + 1] = LoadActor("<file>")`（幂等，注入前备份 `default.lua.mma-backup`）。
2. Malody V：复制 `mma_editor.lua` 到 `MalodyV/editor/`（目录不存在则创建）。
3. Malody 4：**不复制任何文件**——只把游戏目录写进 `mma-shell-config.json` 的 `malody4Root`（游戏侧零改动）。
4. 写配置：读取→更新→写回 `mma-shell-config.json`（保留 `gameClient`/`hotkeys`/`logLevel` 等既有字段；UTF-8 无 BOM）。

卸载：优先使用 `mma-shell-config.json` 中记录的 `etternaRoot` / `malodyRoot` / `malody4Root`（不重新探测；`-Root` 指定则用指定值），并询问要卸载的主题（单主题也先确认；`-Theme` 指定或 `-Yes` 可跳过）；按「屏幕-文件」配对删除注入行（仅自己的行）+ 删除桥文件 + 删除备份，全程不触碰其他脚本的注入行；最后询问是否同时清空配置中的 `etternaRoot` / `malodyRoot` / `malody4Root`（默认不清）。

# bridges/ — game-side bridge files

Lua injection assets for the shell's data-source channels. Follow the bridge header comments and `docs/shell-guide.md` for installation (Etterna theme directories with `LoadActor` injection and re-install after theme updates; Malody Editor/ plugin triggered from the editor More menu — the in-game skin display was removed). **Malody 4.3.7 needs no bridge file at all**: that source is attached by the shell through zero-injection read-only observation, so nothing is installed on the game side.

## Automated install / uninstall (recommended)

For users unfamiliar with manual steps, this folder ships an interactive installer:

- Double-click `install-bridge.bat` (English UI) or `install-bridge-zh.bat` (Chinese UI); or run `powershell -NoProfile -ExecutionPolicy Bypass -File install-bridge.ps1 [-Chinese]`.
- Main menu: **Install** / **Uninstall**, then pick the game (**Malody 4** / Etterna / Malody V). Choosing Malody V asks once more: **editor Lua plugin** / **in-game song-selection bridge** / both. Every step asks for confirmation.
- **The Malody 4 option copies zero files**: it only writes `malody4Root` into `mma-shell-config.json` (nothing to do on the game side), and uninstall clears just that key.
- Game roots are auto-detected: running process → `MMA_ETTERNA_ROOT` / `MMA_MALODY_ROOT` env vars → common install paths; Malody V additionally probes Steam libraries (registry + `steamapps/libraryfolders.vdf`).
- **If auto-detection finds nothing**: pick the folder from a browse dialog, type a path (`/`, `\` or `\\` separators, wrapping quotes and trailing slashes are all normalized), or follow the built-in "how do I find the path" hint.
- **Etterna theme pre-check**: each theme's structure is validated before listing — only themes with both `ScreenSelectMusic decorations/default.lua` and `ScreenGameplay overlay/default.lua` present are offered; incomplete themes (like `_fallback`) are skipped with the reason shown. Installs into **Rebirth** by default (falls back to the installable-theme picker if missing; installing to all themes at once is intentionally not offered).
- Writes/updates `etternaRoot` / `malodyRoot` (forward-slash paths) in `mma-shell-config.json` next to the plugin root (`bridges/..`, i.e. next to `mma-shell.exe`); picking the **Malody 4** game option writes only `malody4Root` and copies no file.
- Coexists with other installed scripts (e.g. DanOverlay, elements/titlesplash): only its own `LoadActor` line is added/removed; other injections are never touched.
- A missing `default.lua` is an error and that screen is skipped (never auto-created, never blindly injected).
- Advanced flags: `-Game Etterna|Malody|Malody4|MalodyBridge`, `-Uninstall`, `-Chinese` (Chinese UI), `-Yes` (automation), `-Root <path>`, `-Theme <name>`, `-ConfigPath <path>`, **`-LoaderOnly`** (Malody V loader only, skip the plugin DLL).
- **Malody V in-game song-selection bridge (`-Game MalodyBridge`)**: installs the BepInEx 6 IL2CPP loader (`winhttp.dll`, `dotnet/`, `BepInEx/`) plus **this repository's own fork** of the plugin (`MMAMalodySelection.dll`). The fork is not the upstream DLL: the upstream bridge and this fork **patch the same game methods, so having both installed double-hooks the game**. If the installer finds the upstream `MalodyInsightBridge.dll` still present it **only reports it with instructions and never deletes it** — removing it is your call.
  - **Getting the loader (must match byte for byte)**: download the asset `bepinex-il2cpp-788.zip` from the upstream BepInEx 6 IL2CPP `6.0.0-be.788` GitHub release and save it as `bridges/malody/bepinex/loader/bepinex-il2cpp-788.zip` (that folder is not committed), or point `MMA_BEPINEX_LOADER_ZIP` at it. The archive is checked against **SHA256 `F4CC496BD098A0DF4164B81E3737297707F13A47C2478DBA2F60EEFAB784817A`** (34,336,405 bytes); a mismatch aborts without writing a single byte.
  - **Unity reference assemblies (also byte-exact)**: `bridges/malody/bepinex/unity-libs/2022.3.62.zip` (that folder is not committed), sourced from <https://unity.bepinex.dev/libraries/2022.3.62.zip>, **SHA256 `575E7D600F69DE8200CCF4DB700B3AE6252366C22E8C3434C860E428974518D1`** (1,900,589 bytes); `MMA_MALODY_UNITY_LIBS_ZIP` may point elsewhere. The installer places it at `BepInEx\unity-libs\2022.3.62.zip`: BepInEx would otherwise download that file on the first launch and afterwards only reuse it without ever validating it, so a single incomplete download breaks every later launch (`End of Central Directory record could not be found` → no plugin loads). A file that is present but does not match the hash is replaced, so re-running the installer repairs such an install.
  - **`-LoaderOnly`**: install (or keep) the loader and stop before the plugin DLL. Use it to let BepInEx generate `BepInEx/interop/` first — install the loader, **launch the game once**, then run the installer again without the flag to add the plugin.
  - **No config file is written**: the fork no longer has an in-game overlay, so `BepInEx\config\local.mma.malody.selection.cfg` is generated by BepInEx on first load; the installer neither creates nor deletes it.

What it does (replacing the manual steps):
1. Etterna: copies the two lua files into `Themes\<theme>\BGAnimations\{ScreenSelectMusic decorations, ScreenGameplay overlay}\` and injects `t[#t + 1] = LoadActor("<file>")` before `return t` in each `default.lua` (idempotent; backs up `default.lua.mma-backup` first).
2. Malody V: copies `mma_editor.lua` into `MalodyV/editor/` (creates the folder if missing).
3. Malody 4: copies **nothing at all** — it only records the game folder as `malody4Root` in `mma-shell-config.json` (zero change on the game side).
4. Config: read → update → write back `mma-shell-config.json` (preserves `gameClient`/`hotkeys`/`logLevel` and any other fields; UTF-8 without BOM).

Uninstall: prefers the `etternaRoot` / `malodyRoot` / `malody4Root` recorded in `mma-shell-config.json` (no re-probing; `-Root` overrides), asks which theme to remove (single-theme confirm included; skipped via `-Theme` or `-Yes`), then removes only its own injection lines, the bridge files and its backup — paired screen↔file, other scripts' injections are never touched — and optionally clears `etternaRoot` / `malodyRoot` / `malody4Root` from the config (default: keep).