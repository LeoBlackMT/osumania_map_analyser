# 游戏桥接组件说明

本目录包含用于将外部游戏数据对接到桌面壳（mma-shell）的桥接文件与安装脚本。

当前支持的游戏数据源：
- Etterna：通过主题脚本监听选歌与游玩数据。
- Malody V：支持编辑器插件与游戏内选曲桥两种接入方式。
- Malody 4.3.7：桌面壳直接读取进程数据，游戏侧无需安装任何桥接文件。

## 自动安装与卸载

Windows 用户推荐使用目录下的交互式脚本进行安装或卸载：

- 双击运行 `install-bridge-zh.bat`（中文）或 `install-bridge.bat`（英文）。
- 也可在 PowerShell 中执行：
  ```powershell
  powershell -NoProfile -ExecutionPolicy Bypass -File install-bridge.ps1 -Chinese
  ```

安装器功能：
- 自动检测正在运行的游戏、环境变量、Steam 库及常见安装路径。如果未检测到，支持手动浏览选择或输入路径。
- Etterna：校验主题结构，默认注入到 Rebirth 主题的选歌和游玩界面。
- Malody V：支持安装编辑器插件或基于 BepInEx 的游戏内选曲桥。
- Malody 4.3.7：仅将游戏目录记录到 `mma-shell-config.json`，不向游戏目录复制任何文件。
- 自动将游戏路径写入桌面壳配置文件 `mma-shell-config.json`。
- 卸载时仅清理本工具添加的脚本和文件，不会影响其他已安装的游戏脚本。

常用命令行参数：
- `-Game <Etterna|Malody|Malody4|MalodyBridge>`：指定目标游戏或组件，跳过菜单选择。
- `-Uninstall`：执行卸载操作。
- `-Chinese`：使用中文界面。
- `-Root <路径>`：指定游戏根目录。
- `-Theme <主题名>`：指定 Etterna 主题名称。
- `-ConfigPath <路径>`：指定 `mma-shell-config.json` 的保存位置。
- `-LoaderOnly`：仅安装 Malody V 的 BepInEx 加载器，不安装插件 DLL。
- `-Yes`：在有默认选项的确认提示时自动确认。

## 手动安装

### 1. Etterna

1. 将 `etterna/mma_bridge.lua` 复制到主题的 `BGAnimations/ScreenSelectMusic decorations/` 目录下。
2. 打开该目录下的 `default.lua`，在 `return t` 之前添加：
   ```lua
   t[#t + 1] = LoadActor("mma_bridge.lua")
   ```
3. 将 `etterna/mma_gameplay.lua` 复制到主题的 `BGAnimations/ScreenGameplay overlay/` 目录下。
4. 打开该目录下的 `default.lua`，在 `return t` 之前添加：
   ```lua
   t[#t + 1] = LoadActor("mma_gameplay.lua")
   ```
5. 打开 `mma-shell.exe` 同级目录下的 `mma-shell-config.json`，将 `etternaRoot` 修改为游戏安装目录，例如 `D:/Games/Etterna`。
6. 注意：主题更新会覆盖上述文件，更新主题后需要重新配置。

### 2. Malody V 编辑器插件

1. 将 `malody/mma_editor.lua` 复制到游戏目录的 `MalodyV/editor/` 下（若目录不存在则创建）。
2. 在游戏中打开谱面编辑器，通过菜单中的插件功能点击 MMA Analyze 即可向桌面壳发送当前谱面。
3. 在 `mma-shell-config.json` 中配置 `malodyRoot` 为 Malody V 目录。

### 3. Malody V 游戏内选曲桥

安装游戏内选曲桥后，在游戏内选歌、游玩与结算时卡片会自动跟随，无需打开编辑器。

1. 准备 BepInEx 6 IL2CPP 加载器（版本 6.0.0-be.788）压缩包 `bepinex-il2cpp-788.zip`，以及 Unity 库文件 `2022.3.62.zip`。完整发布包已自带所需文件。
2. 将加载器解压到 Malody V 游戏根目录，将 Unity 库放置在 `BepInEx/unity-libs/2022.3.62.zip`。
3. 将 `malody/bepinex/plugin/MMAMalodySelection.dll` 复制到 `BepInEx/plugins/MalodyInsight/` 目录下。
4. 在 `mma-shell-config.json` 中配置 `malodyRoot` 为 Malody V 目录。
5. 启动桌面壳后再启动游戏，切换谱面即可自动接收数据。
6. 如需卸载，删除 `BepInEx/plugins/MalodyInsight/MMAMalodySelection.dll` 即可。

### 4. Malody 4.3.7

Malody 4.3.7 数据源由桌面壳在外部进行状态读取，游戏目录内不需要放置任何文件。只需在 `mma-shell-config.json` 中将 `malody4Root` 设置为游戏目录路径，启动游戏和桌面壳后即可自动跟随。

---

# Game Bridge Documentation

This directory contains bridge files and installation scripts used to feed song and gameplay data from external rhythm games into the desktop shell (mma-shell).

Supported game sources:
- Etterna: Uses theme scripts to report song selection and gameplay data.
- Malody V: Supports an editor plugin and an in-game song selection bridge.
- Malody 4.3.7: Read directly by the desktop shell from the outside; no files need to be installed in the game directory.

## Automated Installation and Uninstallation

Windows users can use the interactive script in this directory to install or uninstall bridges:

- Double-click `install-bridge.bat` (English) or `install-bridge-zh.bat` (Chinese).
- Or run in PowerShell:
  ```powershell
  powershell -NoProfile -ExecutionPolicy Bypass -File install-bridge.ps1
  ```

Installer capabilities:
- Automatically detects running games, environment variables, Steam libraries, and common installation folders. If not detected, manual folder selection and path input are supported.
- Etterna: Verifies theme directory layout and injects hooks into the Rebirth theme's song selection and gameplay screens by default.
- Malody V: Supports installing either the editor plugin or the BepInEx-based in-game song selection bridge.
- Malody 4.3.7: Only writes the game directory into `mma-shell-config.json` without copying files.
- Writes game directory paths to `mma-shell-config.json` automatically.
- Cleanly removes only the scripts and lines added by this tool during uninstallation, leaving other third-party scripts intact.

Common command line options:
- `-Game <Etterna|Malody|Malody4|MalodyBridge>`: Targets a specific game or component, skipping the menu.
- `-Uninstall`: Runs in uninstallation mode.
- `-Chinese`: Uses Chinese interface.
- `-Root <path>`: Specifies game root directory.
- `-Theme <name>`: Specifies Etterna theme name.
- `-ConfigPath <path>`: Specifies path to `mma-shell-config.json`.
- `-LoaderOnly`: Installs only the BepInEx loader for Malody V without the plugin DLL.
- `-Yes`: Automatically confirms prompts that have safe defaults.

## Manual Installation

### 1. Etterna

1. Copy `etterna/mma_bridge.lua` to `BGAnimations/ScreenSelectMusic decorations/` inside your Etterna theme directory.
2. Open `default.lua` in that directory and add this line before `return t`:
   ```lua
   t[#t + 1] = LoadActor("mma_bridge.lua")
   ```
3. Copy `etterna/mma_gameplay.lua` to `BGAnimations/ScreenGameplay overlay/` inside your theme directory.
4. Open `default.lua` in that directory and add this line before `return t`:
   ```lua
   t[#t + 1] = LoadActor("mma_gameplay.lua")
   ```
5. Open `mma-shell-config.json` next to `mma-shell.exe` and set `etternaRoot` to your Etterna installation folder, e.g. `D:/Games/Etterna`.
6. Theme updates overwrite these files, so reapply this setup after updating themes.

### 2. Malody V Editor Plugin

1. Copy `malody/mma_editor.lua` to `MalodyV/editor/` in the game folder (create the directory if missing).
2. Open the chart editor in Malody V, and trigger MMA Analyze from the plugin menu to send the active chart to the shell.
3. In `mma-shell-config.json`, set `malodyRoot` to your Malody V directory.

### 3. Malody V In-Game Selection Bridge

With the in-game song selection bridge, the overlay follows chart selection, gameplay, and results automatically without needing the editor.

1. Prepare the BepInEx 6 IL2CPP loader (version 6.0.0-be.788) package `bepinex-il2cpp-788.zip` and the Unity library file `2022.3.62.zip`. Full release archives already include these files.
2. Extract the loader into the Malody V root directory, and place the Unity library file at `BepInEx/unity-libs/2022.3.62.zip`.
3. Copy `malody/bepinex/plugin/MMAMalodySelection.dll` to `BepInEx/plugins/MalodyInsight/`.
4. In `mma-shell-config.json`, set `malodyRoot` to your Malody V directory.
5. Start the desktop shell before launching the game. As you scroll through charts in game, the card updates automatically.
6. To uninstall, delete `BepInEx/plugins/MalodyInsight/MMAMalodySelection.dll`.

### 4. Malody 4.3.7

Malody 4.3.7 is observed by the desktop shell externally, requiring no files inside the game folder. Set `malody4Root` in `mma-shell-config.json` to the game folder path, and launch both the game and the shell to start tracking.