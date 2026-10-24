# 桌面壳使用教程 / Desktop Shell Guide

> English version [below](#english).

# 中文

## 这是什么

本项目最初通过 tosu 获取来自 osu 的数据。然而，随着功能的扩充，tosu本身已经不再满足我们的需求。桌面壳（mma-shell）是一个推荐的小窗口程序，把分析卡片显示在独立的小窗口里；在不启动 tosu 的情况下，让卡片接收来自osu stable/lazer、Etterna、Malody V 及其编辑器或 Malody 4.3.7 客户端的数据；同时，如果内存读取失效，依然保留了 tosu 的支持。

- 系统要求：Windows / Linux（桥安装器为 PowerShell 脚本，仅 Windows；Linux 按下文手动步骤安装）。
- Etterna 与 Malody V 数据源需要安装对应游戏的桥文件（见下文「安装桥」）。
- 最低游戏版本：
  - Etterna 0.70+（Linux 需 0.75+ 官方 Linux 版）
  - Malody V 6.6.43+（仅 Windows）
  - Malody 4.3.7（仅 Windows）
  - osu stable和lazer随更新手动维护
- 支持的谱面类型：`.mc` `.ssc` `.sm` `.osu`。

> - 本子项目受[DanielEtterna](https://github.com/JoseMGS3/DanielEtterna)启发，感谢其作者[JoseMGS3](https://github.com/JoseMGS3)提供的思路与部分代码。
> - 壳目前处于初期阶段，可能存在未知问题。请在使用中遇到问题时及时反馈。
> - 从 Etterna 转换得到的谱面默认取 OD 9；其中 Malody 4.3.7 与 Malody V 的 OD 由游戏内判定档（A~E或Pro）与谱面倍速（模组或Turbo Mode）共同决定。详见[docs/features/malody-od.md](docs/features/malody-od.md)。

## 一、快速开始

- 从发布页下载带shell的压缩包，解压并放在你喜欢的位置。如果你不使用 tosu，直接运行`mma-shell.exe`即可。Linux 包的目录结构与 Windows 相同（插件内容在包根、`mma-shell` 与 `index.html` 同级），解压后运行`./mma-shell`；tar 已保留可执行位。
- 如果你需要在 Etterna 或 Malody V 中使用，请运行`bridges/install-bridge-zh.bat`或`bridges/install-bridge.bat`。对于Linux用户，请参照下方「安装桥」章节手动安装 Etterna 与 Malody V 的桥文件。
- 按下`Ctrl+Shift+S`打开设置页，在`Shell Configuration`中检查自动探测游戏目录结果是否正确，否则请填写需要使用的游戏目录，用于寻找谱面进行分析。设置的详细说明见[docs/settings.md](docs/settings.md)，预设系统的使用方法见[docs/presets-guide.md](docs/presets-guide.md)。
- 完成后，按需重启应用，随后即可正常使用。其中 Malody V 编辑器需要手动点击`MMA Analyze`按钮进行分析。窗口的操作说明见下方。
- 数据源指示：卡片右上状态行末尾的小圆点——粉色=osu!、紫色=Etterna、亮青色=Malody 4、蓝色=Malody V、灰色空心=当前没有数据源。

## 二、窗口操作说明

| 操作 | 方法 |
| :---: | :--- |
| 拖动整个窗口 | 按住窗口顶部拖动条（发光细条，中间有 `⋮⋮` 标志）拖动。鼠标移上去会显示一圈描边，标出即将移动的整扇窗口范围 |
| 改变窗口大小 | 拖动窗口边缘（无边框，四周可拖）。看不到边界时把鼠标移向窗口边缘，贴近的边会发光 |
| 页面缩放 | 按住 `Ctrl` 滚动鼠标滚轮；或 `Ctrl +` / `Ctrl -`；`Ctrl 0` 复位 |
| 开关置顶（默认开） | 默认按 `Ctrl+Shift+T`（全局快捷键） |
| 开关点击穿透（默认关） | 默认按 `Ctrl+Shift+C`（全局快捷键） |
| 打开设置窗口 | 默认按 `Ctrl+Shift+S`（全局快捷键；也可用 `mma-shell.exe --settings` 或浏览器直开，见「五、设置窗口」） |
| 关闭窗口 | 默认按 `Ctrl+Q`（全局快捷键） |
| 刷新卡片 | 选中窗口后按下 `F5`；刷新缓存时按下`Ctrl+F5` |

窗口位置/尺寸/置顶/点击穿透状态自动记忆；标注全局快捷键的操作在窗口失焦或点击穿透时同样有效。

## 三、安装桥

推荐：自动安装（桥安装器为 Windows PowerShell 脚本，仅 Windows；Linux 按下方手动步骤操作）。bridges 目录自带交互式安装器，按菜单选择 Install / Uninstall 与游戏：

- 双击 `bridges/install-bridge.bat`（英文界面）或 `bridges/install-bridge-zh.bat`（中文界面）；也可命令行运行 `powershell -NoProfile -ExecutionPolicy Bypass -File install-bridge.ps1 [-Chinese]`。
- 自动探测游戏安装目录（正在运行的进程 → 环境变量 → 常见安装路径或Steam 库）。
- 探测失败时可从图形对话框「浏览选择」，或手动输入路径（`/`、`\`、`\\`均可）。
- 默认安装到 Etterna 的 Rebirth 主题或其他通过主题结构校验的主题。
- 自动把游戏路径写入 `mma-shell-config.json`。
- 选择第三个游戏项 Malody 4 时只把 `malody4Root` 写进 `mma-shell-config.json`（游戏侧无需任何操作）；卸载时也只清空这个键。
- 卸载时优先使用配置中记录的游戏目录，会询问卸载哪个主题，且只移除自身注入行，与其他脚本兼容。
- 高级参数与完整行为说明见 [bridges/README.md](../bridges/README.md)（`-Game` `-Uninstall` `-Chinese` `-Yes` `-Root` `-Theme` `-ConfigPath`）。

若你想手动安装，按下文步骤操作。

### Etterna

1. 复制 `bridges/etterna/mma_bridge.lua` 到 `Etterna/Themes/<你的主题>/BGAnimations/ScreenSelectMusic decorations/` 中；随后在该目录 `default.lua` 的 `return t` 之前加一行：`t[#t + 1] = LoadActor("mma_bridge.lua")`。例如：

```lua
-- BGAnimations/ScreenSelectMusic decorations/default.lua
-- ...
    LoadActorWithParams("generalBox", {
        widthRatio = widthRatio,
    }),
}
-- 在此处添加
t[#t + 1] = LoadActor("mma_bridge.lua")
-- 如果你之前在这里安装过其他 lua 脚本，例如 DanOverlay，无需删除原来的，可以共存。
return t
```

2. 复制 `bridges/etterna/mma_gameplay.lua` 到 `Etterna/Themes/<你的主题>/BGAnimations/ScreenGameplay overlay/`；同样在 `default.lua` 的 `return t` 前加：`t[#t + 1] = LoadActor("mma_gameplay.lua")`
3. 主题更新后必须重装这两个文件（主题包覆盖会删掉它们）。
4. 打开插件目录下的 `mma-shell-config.json` 文件，在设置里填写 `etternaRoot`（例如 `D:\\Games\\Etterna`）。

### Malody V

Malody V 有两个互不影响的通道，按需选择：

#### A. 编辑器通道

- 把 `bridges/malody/mma_editor.lua` 放到 `MalodyV/editor/` 中（目录不存在就新建）。随后打开游戏 → 打开谱面编辑器 → MMA Analyze → 卡片显示本谱分析。
- 或者在游戏编辑器左上角点击按钮 → 插件管理 → 导入。
- 打开插件目录下的 `mma-shell-config.json` 文件，在设置里填写 `malodyRoot`（例如 `D:\\Steam\\steamapps\\common\\MalodyV`）。一般不用手动填：壳会自动探测正在运行的进程、`MMA_MALODY_ROOT` 环境变量、Steam 库与常见安装路径，当无法正常读取时需要手动填写。

#### B. 游戏内选曲桥

1. 先准备加载器。从 BepInEx 6 IL2CPP `6.0.0-be.788` 的 GitHub Release 下载资产 `bepinex-il2cpp-788.zip`，保存为 `bridges/malody/bepinex/loader/bepinex-il2cpp-788.zip`，或设环境变量 `MMA_BEPINEX_LOADER_ZIP` 指向它。安装前会校验 SHA256 `F4CC496BD098A0DF4164B81E3737297707F13A47C2478DBA2F60EEFAB784817A`。另外还需要 Unity 参考程序集 `bridges/malody/bepinex/unity-libs/2022.3.62.zip`（来源 [https://unity.bepinex.dev/libraries/2022.3.62.zip](https://unity.bepinex.dev/libraries/2022.3.62.zip)，`MMA_MALODY_UNITY_LIBS_ZIP` 可指向别处，SHA256 `575E7D600F69DE8200CCF4DB700B3AE6252366C22E8C3434C860E428974518D1`）。安装器会把它放在 `BepInEx\\unity-libs\\2022.3.62.zip`。从 Release 下载整包的用户无需准备，包里已带。
2. 双击 `install-bridge-zh.bat`（或 `install-bridge.bat`）→ Install → Malody V → 游戏内选曲桥。装完目标文件是 `BepInEx\\plugins\\MalodyInsight\\MMAMalodySelection.dll`。
3. 如果不使用安装脚本，将`bepinex-il2cpp-788.zip`解压后直接覆盖到Malody V运行目录下，并将`2022.3.62.zip`置于`BepInEx\\unity-libs\\2022.3.62.zip`。
4. 验证：开着壳启动游戏，应当出现bepinex命令行界面。进选曲界面切谱，卡片应立刻跟着换，且星数随判定档、Pro、倍速变化。
5. 卸载：同样的入口选 Uninstall。只删除本安装器记录过的文件；加载器会保留。如需要删除加载器，请手动删除`winhttp.dll`、`doorstop_config.ini`、`.doorstop_version`、`changelog.txt`、`dotnet\`、`BepInEx\`。

> 如果你曾装过 `MalodyInsightBridge.dll` ，安装器会报告并指出两份插件挂在同一批游戏方法上。要改用本方案，请自行备份并删除旧dll再安装。

### Malody 4.3.7（无需安装）

- 游戏侧不需要任何操作：安装器里选 Malody 4，把游戏目录写进 `mma-shell-config.json` 的 `malody4Root`（也可以手动填，例如 `D:\\Games\\Malody-4.3.7`）。
- 之后开着壳启动游戏，在游戏里选歌/游玩即自动跟随；若游戏高亮的谱面若不在索引范围内（该源只索引 `beatmap/` 下的 `.mc` Key 谱面），卡片会保留上一张的结果并给出原因。

## 四、注意事项

- 壳窗口里若打不开系统浏览器，页面会把地址复制到剪贴板并提示 `Link copied — open it in your browser.`，自己在浏览器里粘贴打开即可。
- 当无法正常通过快捷键打开设置时，允许用户直接编辑 `mma-settings.json` 来配置卡片显示。修改后约 30 秒内自动生效。注意，请对照[settings.md](settings.md)的说明来修改，避免出现错误。
- 配置填错/JSON损坏将自动回落默认并在日志中警告。
- tosu 在线时以 tosu 设置文件为准（tosu 安装目录的 `settings/<插件目录名>.values.json`，只读，此时壳既不生成也不使用 `mma-settings.json`）。此时你需要通过tosu来编辑设置，在 tosu 的 Presets 页面里管理预设。
- 如果你想关闭一个来源，请在设置中将对应的游戏安装目录设置为无效目录。

## 五、日志

壳运行日志写在 exe 旁的 `logs/` 目录：`logs/mma-shell-YYYYMMDD.log`（按日轮转，保留 7 天）。排查问题时将 `logLevel` 设为 `debug`，把最新日志发到 [Issues](https://github.com/LeoBlackMT/osumania_map_analyser/issues) 即可；遇到问题请先确认日志内容。

## 六、界面语言

壳设置面板的第一行是**界面语言**下拉，可选 **自动 / 简体中文 / English**：

- **自动**（默认）：跟随系统语言，中文系统显示中文，其他显示英文。
- 选择会写进壳配置文件 `mma-shell-config.json` 的 `language` 字段（`auto` / `zh-CN` / `en-US`），重启后保留；也可以在文件里直接改。
- 只影响壳自己的页面（设置页、预设管理器）。**tosu 自带的设置面板始终是英文**，因为 tosu 不支持中文设置标题。
- 中文使用系统字体显示（微软雅黑 / 苹方 / Noto Sans CJK），不需要额外安装字体。

## 七、常见问题/已知问题

- **圆点灰空心 / 卡片不动**：确认对应游戏已开、桥已装、壳在运行。
- **端口 17653 被占用**：卡片不动且壳日志提示端口被占。关掉多余的 `mma-shell` 实例（同时只能有一个），或重启壳。
- **端口 24061 被占用**：壳无法启动或日志报 `port 24061 is occupied by another process`。先关闭旧 `mma-shell`；仍被占用时用 `Get-NetTCPConnection -LocalPort 24061` 或 `netstat -ano | findstr :24061` 查 PID，再关闭对应程序。
- **端口 24062 被占用**：壳日志会提示 `osu! 兼容端点端口 24062 被占用，原生传输不可用`，页面自动回退 tosu；关闭占用程序并重启壳即可恢复。
- **tosu 端口不是 24050**：在插件设置 `wsEndpoint` 填写实际的 `host:port`；`presets.html`/`debug.html` 的固定链接可能需要手动打开。
- **Malody V 相关**
  - **卡片没反应 / 一直空着**：先确认壳在运行；再打开壳日志（`logs/mma-shell-YYYYMMDD.log`）搜 `malody bridge` —— 若出现 `REJECTED … malodyRoot unavailable`，就是壳没解析到游戏目录，手动在 `mma-shell-config.json` 里填 `malodyRoot`。
  - **游戏里看不到任何变化**：看游戏目录下的 `BepInEx\\LogOutput.log` —— 应有 `Selection bridge ready` 与一行行 `Observe …`。若整个文件都不存在，说明加载器没被游戏加载（可能是杀软隔离了 `winhttp.dll`）。
  - **启动时控制台报 `End of Central Directory record could not be found`**：`BepInEx\\unity-libs\\2022.3.62.zip` 损坏，而 BepInEx 之后只会复用这个坏文件，日志中紧接着的 `Unable to execute IL2CPP chainloader, no plugins will be loaded` 就是它导致的。重跑一次安装器即可，然后重启游戏。
  - **星数不随速率变化**：确认装的是本 fork 的 `MMAMalodySelection.dll`，而不是上游那份`MalodyInsightBridge.dll`。
  - **Pro判没生效**：插件会主动读 Pro，不需要特意打开 JUDGE 面板；若状态行提示「动态 OD 未启用：Pro 状态未知」，打开一次 JUDGE 面板再关闭即可。
- **Malody 4.3.7 不分析**：把 `logLevel` 设为 `debug` 看壳日志——日志会给出原因（进程没找到 / 版本不符 / 没有权限 / 该谱不在索引里等）；常见成因是游戏版本不是 4.3.7、或游戏目录设错、或谱面不是 `beatmap/` 下的 `.mc` Key 谱面。
- **卡片主体偶尔显示No Data**：对于Etterna，切成另一张谱面再切回来即可。对于Malody，请重新点击 MMA Analyze。
- **无法找到窗口位置**：删除`mma-shell-state.json`后，重启壳。

# English

## What this is

This project originally gathered osu! data via tosu. However, as features expanded, tosu alone no longer met all requirements. The desktop shell (`mma-shell`) is a recommended mini-window application that displays the analysis card in a standalone window. It allows the card to receive data from osu!stable / lazer, Etterna, Malody V and its editor, or the Malody 4.3.7 client without needing to launch tosu; at the same time, if memory reading fails, tosu support is still preserved as a fallback.

- System requirements: Windows / Linux (the bridge installer is a PowerShell script, Windows only; for Linux, follow the manual steps below).
- Etterna and Malody V sources require installing their respective bridge files (see "Bridges" below).
- Minimum game versions:
  - Etterna 0.70+ (Linux requires the official 0.75+ Linux release)
  - Malody V 6.6.43+ (Windows only)
  - Malody 4.3.7 (Windows only)
  - osu!stable and lazer are manually maintained across updates
- Supported beatmap/chart types: `.mc`, `.ssc`, `.sm`, `.osu`.

> - This subproject is inspired by [DanielEtterna](https://github.com/JoseMGS3/DanielEtterna); thanks to its author [JoseMGS3](https://github.com/JoseMGS3) for the inspiration and parts of the code.
> - The shell is currently in its early stages; unknown issues may exist. Please provide feedback if you encounter any problems.
> - Converted charts from Etterna default to OD 9; OD for Malody 4.3.7 and Malody V is determined jointly by the in-game judge rank (A–E or Pro) and speed rate (mods or Turbo Mode). See [docs/features/malody-od.md](features/malody-od.md) for details.

## 1. Quick Start

- Download the package with shell from the release page, extract it, and place it wherever you like. If you do not use tosu, run `mma-shell.exe` directly. The Linux package uses the same layout (plugin files at the archive root, `mma-shell` next to `index.html`); extract it and run `./mma-shell` (the tar keeps the executable bit).
- If you need to use it with Etterna or Malody V, run `bridges/install-bridge-zh.bat` or `bridges/install-bridge.bat`. For Linux users, refer to the "Bridges" section below to install bridge files manually.
- Press `Ctrl+Shift+S` to open the settings window. Under `Shell Configuration`, check whether auto-detected game directories are correct; otherwise, fill in the game directories you use to locate beatmaps for analysis. See [docs/settings.md](settings.md) for settings details and [docs/presets-guide.md](presets-guide.md) for preset usage.
- Once configured, restart the application if needed, and it will be ready to use. In the Malody V editor, click the `MMA Analyze` button manually to analyze. Window controls are described below.
- Source indicator: the small dot at the end of the top status row on the card — pink = osu!, purple = Etterna, bright cyan = Malody 4, blue = Malody V, hollow grey = no current data source.

## 2. Window Controls

| Action | Method |
| :---: | :--- |
| Move window | Drag the top drag bar (the glowing bar with the `⋮⋮` mark). Hovering displays an outline showing the full window boundary |
| Resize window | Drag any window edge (borderless, draggable on all sides). Hover near an edge to make it glow |
| Page zoom | Hold `Ctrl` and scroll mouse wheel; or `Ctrl +` / `Ctrl -`; `Ctrl 0` to reset |
| Toggle always-on-top (default on) | `Ctrl+Shift+T` by default (global shortcut) |
| Toggle click-through (default off) | `Ctrl+Shift+C` by default (global shortcut) |
| Open settings window | `Ctrl+Shift+S` by default (global shortcut; you can also run `mma-shell.exe --settings` or open in a browser) |
| Close window | `Ctrl+Q` by default (global shortcut) |
| Refresh card | Focus the window and press `F5`; press `Ctrl+F5` to refresh cache |

Window position, size, topmost, and click-through states are remembered automatically. Global shortcuts remain active even when the window is unfocused or click-through is enabled.

## 3. Bridges

Recommended: automated installation (the installer is a Windows PowerShell script, Windows only; on Linux, follow manual steps below). The bridges directory includes an interactive installer with options for Install / Uninstall and game selection:

- Double-click `bridges/install-bridge.bat` (English interface) or `bridges/install-bridge-zh.bat` (Chinese interface); or run `powershell -NoProfile -ExecutionPolicy Bypass -File install-bridge.ps1 [-Chinese]` from command line.
- Automatically probes game installation directories (running process → environment variables → common paths or Steam libraries).
- If probing fails, choose "Browse" in the folder dialog or enter the path manually (`/`, `\`, or `\\` are all supported).
- Installs to Etterna's Rebirth theme by default, or other themes passing theme structure validation.
- Automatically writes game paths into `mma-shell-config.json`.
- Selecting the third option, Malody 4, only writes `malody4Root` into `mma-shell-config.json` (no files copied on the game side); uninstalling only clears this key.
- Uninstalling prioritizes the game path recorded in config, asks which theme to remove from, and only removes its own injected lines, coexisting with other scripts.
- For advanced parameters and full behavior, see [bridges/README.md](../bridges/README.md) (`-Game`, `-Uninstall`, `-Chinese`, `-Yes`, `-Root`, `-Theme`, `-ConfigPath`).

If you prefer to install manually, follow the steps below.

### Etterna

1. Copy `bridges/etterna/mma_bridge.lua` to `Etterna/Themes/<your_theme>/BGAnimations/ScreenSelectMusic decorations/`; then add a line before `return t` in that directory's `default.lua`: `t[#t + 1] = LoadActor("mma_bridge.lua")`. For example:

```lua
-- BGAnimations/ScreenSelectMusic decorations/default.lua
-- ...
    LoadActorWithParams("generalBox", {
        widthRatio = widthRatio,
    }),
}
-- Add here
t[#t + 1] = LoadActor("mma_bridge.lua")
-- If you previously installed other lua scripts such as DanOverlay, they can coexist without removal.
return t
```

2. Copy `bridges/etterna/mma_gameplay.lua` to `Etterna/Themes/<your_theme>/BGAnimations/ScreenGameplay overlay/`; similarly add `t[#t + 1] = LoadActor("mma_gameplay.lua")` before `return t` in its `default.lua`.
3. Reinstall these two files whenever the theme is updated (theme updates overwrite them).
4. Open `mma-shell-config.json` in the plugin directory and set `etternaRoot` (e.g. `D:\\Games\\Etterna`).

### Malody V

Malody V offers two independent channels; choose as needed:

#### A. Editor Channel

- Place `bridges/malody/mma_editor.lua` in `MalodyV/editor/` (create the folder if it does not exist). Then start the game → open the chart editor → click `MMA Analyze` → the card displays analysis for the chart.
- Alternatively, click the top-left button in the game editor → Plugin Management → Import.
- Open `mma-shell-config.json` in the plugin directory and set `malodyRoot` (e.g. `D:\\Steam\\steamapps\\common\\MalodyV`). You usually do not need to set this manually: the shell automatically detects running processes, `MMA_MALODY_ROOT` environment variable, Steam libraries, and common paths. Manual entry is only needed if auto-detection fails.

#### B. In-Game Song Selection Bridge

1. Prepare the loader. Download `bepinex-il2cpp-788.zip` from BepInEx 6 IL2CPP `6.0.0-be.788` GitHub Releases and save it to `bridges/malody/bepinex/loader/bepinex-il2cpp-788.zip`, or set the `MMA_BEPINEX_LOADER_ZIP` environment variable pointing to it. The installer validates SHA256 `F4CC496BD098A0DF4164B81E3737297707F13A47C2478DBA2F60EEFAB784817A`. You also need the Unity reference assemblies `bridges/malody/bepinex/unity-libs/2022.3.62.zip` (source: [https://unity.bepinex.dev/libraries/2022.3.62.zip](https://unity.bepinex.dev/libraries/2022.3.62.zip); `MMA_MALODY_UNITY_LIBS_ZIP` can point elsewhere, SHA256 `575E7D600F69DE8200CCF4DB700B3AE6252366C22E8C3434C860E428974518D1`). The installer puts it in `BepInEx\unity-libs\2022.3.62.zip`. Users downloading full release packages need no preparation; it is included.
2. Double-click `install-bridge.bat` (or `install-bridge-zh.bat`) → Install → Malody V → In-game song selection bridge. The destination file is `BepInEx\plugins\MalodyInsight\MMAMalodySelection.dll`.
3. If not using the installation script, extract `bepinex-il2cpp-788.zip` directly into the Malody V root directory, and place `2022.3.62.zip` at `BepInEx\unity-libs\2022.3.62.zip`.
4. Verification: Start the game while the shell is running. A BepInEx console window should appear. In the song selection screen, change charts; the card should update immediately, and star rating moves with judge level, Pro, and rate.
5. Uninstall: Use the same installer menu and select Uninstall. Only files tracked by this installer are removed; the loader is kept. If you want to remove the loader, manually delete `winhttp.dll`, `doorstop_config.ini`, `.doorstop_version`, `changelog.txt`, `dotnet\`, and `BepInEx\`.

> If you previously installed `MalodyInsightBridge.dll`, the installer will report that both plugins hook the same game methods. Back up and remove the old DLL before installing this bridge.

### Malody 4.3.7 (No Installation Needed)

- No action required on the game side: select Malody 4 in the installer to write the game path to `malody4Root` in `mma-shell-config.json` (or fill it in manually, e.g. `D:\\Games\\Malody-4.3.7`).
- Keep the shell running and start the game; selecting or playing charts in-game will follow automatically. If the selected chart is outside the index scope (this source only indexes `.mc` Key charts under `beatmap/`), the card retains the previous result and displays the reason.

## 4. Notes

- If the shell window cannot open your system browser, the page copies the link to your clipboard and notifies `Link copied — open it in your browser.`; paste it into your browser manually.
- When the settings window cannot be opened via shortcut, users may edit `mma-settings.json` directly to configure card display. Changes take effect automatically within ~30 seconds. Refer to [settings.md](settings.md) when making changes to avoid errors.
- Invalid configuration or corrupted JSON automatically falls back to defaults with a warning in the log.
- When tosu is online, tosu's settings file takes precedence (`settings/<plugin_folder>.values.json` in tosu directory, read-only; the shell neither generates nor uses `mma-settings.json` in this state). In this case, edit settings via tosu and manage presets on tosu's Presets page.
- If you want to disable a data source, set the corresponding game installation directory in settings to an invalid directory.

## 5. Logs

Shell runtime logs are written to the `logs/` directory next to the executable: `logs/mma-shell-YYYYMMDD.log` (rotated daily, kept for 7 days). When troubleshooting, set `logLevel` to `debug` and attach the latest log to [Issues](https://github.com/LeoBlackMT/osumania_map_analyser/issues). Always check log contents first when encountering issues.

## 6. Interface Language

The first row of the shell settings panel is an **interface language** dropdown: **Auto / 简体中文 / English**:

- **Auto** (default): follows the system language — Chinese systems get Chinese, everything else gets English.
- The choice is stored in `language` inside `mma-shell-config.json` (`auto` / `zh-CN` / `en-US`) and survives restarts; you can also edit that file directly.
- It only affects the shell's own pages (settings page, presets manager). **tosu's built-in settings panel stays English**, because tosu does not support Chinese setting titles.
- Chinese text uses system fonts (Microsoft YaHei / PingFang / Noto Sans CJK); no extra font installation is needed.

## 7. Troubleshooting / Known Issues

- **Hollow grey dot / Card frozen**: Confirm the game is running, bridges are installed, and the shell is running.
- **Port 17653 in use**: Card does not move and shell log indicates the port is occupied. Close extra `mma-shell` instances (only one can run at a time) or restart the shell.
- **Port 24061 in use**: The shell cannot start or logs `port 24061 is occupied by another process`. Close any old `mma-shell` first; if it is still occupied, find the PID with `Get-NetTCPConnection -LocalPort 24061` (or `netstat -ano | findstr :24061`) and close that program.
- **Port 24062 in use**: The shell logs `osu! compatibility origin port 24062 is occupied; native transport unavailable`; the page falls back to tosu. Close the occupying program and restart the shell.
- **tosu is not on 24050**: Enter the actual `host:port` in the plugin's `wsEndpoint` setting; the fixed `presets.html`/`debug.html` links may need to be opened manually.
- **Malody V related**
  - **Card does not respond / Stays empty**: Confirm the shell is running; open shell logs (`logs/mma-shell-YYYYMMDD.log`) and search for `malody bridge`. If `REJECTED … malodyRoot unavailable` appears, the shell failed to resolve the game path; fill in `malodyRoot` manually in `mma-shell-config.json`.
  - **No changes visible in-game**: Check `BepInEx\LogOutput.log` in the game directory — it should contain `Selection bridge ready` and `Observe …` lines. If the file does not exist, the loader was not loaded (possibly antivirus quarantined `winhttp.dll`).
  - **Console error `End of Central Directory record could not be found` on startup**: `BepInEx\unity-libs\2022.3.62.zip` is corrupted, and BepInEx will only reuse this bad file, causing the subsequent `Unable to execute IL2CPP chainloader, no plugins will be loaded` log. Re-running the installer fixes this; restart the game afterwards.
  - **Star rating does not change with rate**: Confirm the installed plugin is this fork's `MMAMalodySelection.dll` and not upstream `MalodyInsightBridge.dll`.
  - **Pro judge does not take effect**: The plugin reads Pro actively; you do not need to open the JUDGE panel. If the status line displays "Dynamic OD disabled: Pro status unknown", open the JUDGE panel once and close it.
- **Malody 4.3.7 does not analyze**: Set `logLevel` to `debug` and check shell logs — it provides the reason (process not found / version mismatch / access denied / chart not in index, etc.). Common causes are game versions other than 4.3.7, wrong directory, or charts not being `.mc` Key charts under `beatmap/`.
- **Card body occasionally shows No Data**: For Etterna, switch to another chart and switch back. For Malody, click `MMA Analyze` again.
- **Cannot find window position**: Delete `mma-shell-state.json` and restart the shell.

# 了解更多 / Learn more

- 桌面壳技术细节（架构、目录检测、契约、构建）/ Shell technical details: [features/desktop-shell.md](features/desktop-shell.md)
- 安装器完整参数与行为 / Installer flags & behavior: [bridges/README.md](../bridges/README.md)
- 多数据源架构（Auto 跟随、转换器）/ Multi-source architecture: [features/multi-source.md](features/multi-source.md)
