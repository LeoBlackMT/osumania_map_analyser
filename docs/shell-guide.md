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

- 从发布页下载带shell的压缩包，解压并放在你喜欢的位置。如果你不使用 tosu，直接运行`mma-shell.exe`即可。
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

## 六、常见问题/已知问题

- **圆点灰空心 / 卡片不动**：确认对应游戏已开、桥已装、壳在运行。
- **端口 17653 被占用**：卡片不动且壳日志提示端口被占。关掉多余的 `mma-shell` 实例（同时只能有一个），或重启壳。
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

mma-shell is an optional small window program that: shows the analysis card in a **standalone always-on-top mini
window** (can overlay games/browsers); and, **without tosu running**, lets the card receive data from **Etterna**, the
**Malody V editor** or the **Malody 4.3.7 native client**. The classic browser usage (tosu plugin) is unaffected.

- System requirements: Windows / Linux (the bridge installer is a PowerShell script, Windows-only; on Linux follow the manual steps below). 
- Etterna and Malody V data sources need their game bridge files installed (see "Bridges" below); **Malody 4.3.7 needs no game-side files at all** — it is read-only observation with zero injection, and the installer only records its path. 
- Minimum game versions: **Etterna 0.70+** (Linux needs the official 0.75+ Linux build); **Malody V 6.6.43+** (no Linux version — this source is Windows-only); the **Malody 4.3.7 native client** (Windows only, and only that exact version). 
- Supported chart types: `.mc`, `.ssc`, `.sm`.

> Note:
> - This subproject is inspired by [DanielEtterna](https://github.com/JoseMGS3/DanielEtterna); thanks to its author for the ideas and parts of the code.
> - The shell is experimental — unknown issues may exist. Please report anything you find.
> - Due to Malody V API limitations, before you install the plugin the Malody V source works only in the editor. With the **in-game song-selection bridge** installed, **selection, gameplay and results all follow automatically** (see the Malody V part of "Bridges"). The "MMA Analyze" button in the editor keeps working as the fallback channel when the plugin is not installed.
> - The Malody 4.3.7 source is the opposite: it **follows the highlighted chart automatically during selection and gameplay** (read-only observation), with nothing to install on the game side. It supports the 4.3.7 version only; on a version mismatch this source becomes unavailable with a reason while the other three data sources keep working.
> - Default OD after conversion is 9 (`.sm` / `.ssc` / `.mc` charts); the **exception** is the Malody 4.3.7 and Malody V sources, whose OD comes from the in-game judge level (A–E) together with the run's speed. For Malody V it also depends on whether **Pro** is on (the strict group): when the Pro state is unknown nothing is guessed — dynamic OD is turned off and the status line says why.

## Quick start

- Download the archive from the release page (recommended), or just `mma-shell.exe` (Windows) / `mma-shell` (Linux) from CI artifacts.
  - **Archive**: it already contains the plugin folder, `mma-shell.exe` and `bridges/` — extract it into the tosu
    plugin folder (`tosu/static/`) as a whole.
  - **Standalone exe**: place it inside the `ManiaMapAnalyser by Leo_Black` plugin folder and double-click to run.
  - If you don't use tosu, just extract it wherever you like.
- Then install the Etterna and Malody V bridge files per the "Bridges" section below (Malody 4.3.7 needs no game-side files — the installer merely records its path).
- If needed, edit `mma-shell-config.json` to configure the shell (game install paths `etternaRoot` / `malodyRoot` / `malody4Root`,
  `hotkeys`, etc.). In offline mode / without tosu you can also edit `mma-settings.json` for the card display
  (see "Configuration").
- Start the shell: select a song in Etterna to display, or select a chart in Malody V and click "MMA Analyze" in the
  editor, or simply open Malody 4.3.7 and select/play a chart to have the card follow it.
- Source indicator: the small dot at the end of the card's top status row — pink = osu!, purple = Etterna,
  cyan = Malody 4, blue = Malody V, hollow grey = no data source.

## Window controls

| Action | How |
| --- | --- |
| Move the whole window | drag the top drag bar (glow strip with the `⋮⋮` hint). The strip follows the **card's top edge**, so with the card docked at the bottom (reverse extension) it sits right above the card, and hovering it shows a thin full-window outline marking the window you are about to move |
| Resize | drag any window edge (borderless, draggable on all sides). Cannot see where the window ends? Move the cursor towards an edge: the edge you are within ~16 pixels of lights up — that glow is exactly where the drag border is (a corner lights two edges at once) |
| Zoom | `Ctrl` + mouse wheel, or `Ctrl +` / `Ctrl -`; `Ctrl 0` resets |
| Always-on-top toggle (default on) | `Ctrl + Shift + T` by default (global shortcut) |
| Click-through toggle (default off) | `Ctrl + Shift + C` by default (global shortcut) |
| Open the settings window | `Ctrl+Shift+S` by default (global shortcut; `mma-shell.exe --settings` or a browser also works — see "Settings window") |
| Close | `Alt+F4` or `Ctrl+Q` by default (global shortcut) |

Window position/size/topmost/click-through are remembered across launches. Global shortcuts keep working while the
window is unfocused or click-through is active (Windows/X11; global shortcuts are unavailable on **Linux Wayland**
sessions — while the shell window is focused, in-page shortcuts take over with the same key bindings).

## Bridges

**Recommended: automated install** (the installer is a Windows PowerShell script; on Linux, follow the manual steps
below)**.** The bridges folder ships an interactive installer — double-click
`bridges/install-bridge.bat` (or `bridges/install-bridge-zh.bat` for the Chinese UI; or run
`powershell -NoProfile -ExecutionPolicy Bypass -File install-bridge.ps1 [-Chinese]`) and pick Install / Uninstall
and the game from the menu:

- Auto-detects game install folders (running process → env vars → common paths; Malody V additionally probes Steam libraries).
- If detection finds nothing: pick the folder from a browse dialog or type a path (`/`, `\`, `\\`, wrapping quotes and trailing slashes are all normalized).
- Installs into the Etterna Rebirth theme by default (theme-structure pre-check; installable themes are listed when Rebirth is missing).
- Writes the game paths into `mma-shell-config.json` for you.
- Picking the third game option, **Malody 4**, copies **no files at all**: it only writes `malody4Root` into `mma-shell-config.json` (nothing to do on the game side), and uninstall clears just that key.
- On uninstall it reuses the game folder recorded in the config and asks which theme to remove,
  touching only its own injection lines — other scripts (e.g. DanOverlay) coexist.
- Full flags & behavior: [bridges/README.md](../bridges/README.md) (`-Game` `-Uninstall` `-Chinese` `-Yes` `-Root` `-Theme` `-ConfigPath`).

To install manually, follow the steps below.

### Etterna

1. Copy `bridges/etterna/mma_bridge.lua` into
   `Etterna/Themes/<your theme>/BGAnimations/ScreenSelectMusic decorations/`; then add one line before `return t` in
   that folder's `default.lua`: `t[#t + 1] = LoadActor("mma_bridge.lua")`. Example:

```lua
-- BGAnimations/ScreenSelectMusic decorations/default.lua
-- ...
    LoadActorWithParams("generalBox", {
        widthRatio = widthRatio,
    }),
}
-- add here
t[#t + 1] = LoadActor("mma_bridge.lua")
-- Other lua scripts you installed before (e.g. DanOverlay) can stay — they coexist.
return t
```

2. Copy `bridges/etterna/mma_gameplay.lua` into
   `Etterna/Themes/<your theme>/BGAnimations/ScreenGameplay overlay/`; add `t[#t + 1] = LoadActor("mma_gameplay.lua")`
   before `return t` in its `default.lua` too.
3. **Re-install both files after every theme update** (theme packages overwrite and remove them).
4. Open `mma-shell-config.json` next to the plugin and fill in `etternaRoot` (e.g. `D:\\Games\\Etterna`).

### Malody V

- Put `bridges/malody/mma_editor.lua` into `MalodyV/editor/` (create the folder if missing). Then open the game →
  open the chart editor → **MMA Analyze** → the card shows this chart's analysis.
- Or, in the game editor's top-left, open the plugin manager and import the file.
- Open `mma-shell-config.json` next to the plugin and fill in `malodyRoot` (e.g. `D:\\Steam\\steamapps\\common\\MalodyV`). **You normally do not need to**: the shell auto-detects the running process, `MMA_MALODY_ROOT`, Steam libraries and common install paths.

#### In-game song-selection bridge (recommended: selection / gameplay / results all follow)

1. **Get the loader first.** Download the asset `bepinex-il2cpp-788.zip` from the upstream BepInEx 6 IL2CPP `6.0.0-be.788` GitHub release and save it as `bridges/malody/bepinex/loader/bepinex-il2cpp-788.zip` (that folder is not committed), or point `MMA_BEPINEX_LOADER_ZIP` at it. The archive is checked against SHA256 `F4CC496BD098A0DF4164B81E3737297707F13A47C2478DBA2F60EEFAB784817A`; a mismatch aborts without writing a byte. You also need the **Unity reference assemblies** `bridges/malody/bepinex/unity-libs/2022.3.62.zip` (that folder is not committed either; source <https://unity.bepinex.dev/libraries/2022.3.62.zip>, `MMA_MALODY_UNITY_LIBS_ZIP` may point elsewhere, SHA256 `575E7D600F69DE8200CCF4DB700B3AE6252366C22E8C3434C860E428974518D1`). The installer places it at `BepInEx\\unity-libs\\2022.3.62.zip`: BepInEx would otherwise download that file on the first launch and afterwards only reuse it without validating it, so one incomplete download breaks every later launch. **Users who download the whole release package need to prepare nothing — it is already in the package.**
2. **Double-click `install-bridge.bat`** → **Install** → **Malody V** → **in-game song-selection bridge**. The installed file is `BepInEx\\plugins\\MalodyInsight\\MMAMalodySelection.dll`.
3. **Verify**: with the shell running, start the game and move through the song list — the card should follow immediately, and star ratings should move with the judge level, Pro and the rate.
4. **Uninstall**: same entry point, choose **Uninstall**. Only files this installer recorded are removed; the loader is kept (delete the six items listed under Troubleshooting if you want it gone).

> If the upstream `MalodyInsightBridge.dll` is installed, the installer **reports it with instructions but never deletes it** — both plugins patch the same game methods (double hooking). Remove that file yourself before switching to this fork.

**Troubleshooting**

- **The card does nothing / stays empty**: make sure the shell is running, then open its log (see "Logs") and search for `malody bridge`. If you see `REJECTED … malodyRoot unavailable`, the shell could not resolve the game folder — fill in `malodyRoot` in `mma-shell-config.json` by hand.
- **Port 17653 already in use**: the card stops following and the shell log says the port is taken. Close extra `mma-shell` instances (only one can run at a time) or restart the shell.
- **Nothing changes in game**: check `BepInEx\\LogOutput.log` in the game folder — it should contain `Selection bridge ready` and a series of `Observe …` lines. If the file does not exist at all, the loader was never loaded (usually antivirus quarantining `winhttp.dll`, or the game has not been restarted since installing).
- **Star ratings do not move with the rate**: confirm the installed DLL is this fork's `MMAMalodySelection.dll` and not the upstream one.
- **Pro (strict group) has no effect**: the plugin reads Pro on its own — you do **not** need to open the JUDGE panel. If the status line says "dynamic OD off: Pro state unknown", open the JUDGE panel once and close it.
- **The console reports `End of Central Directory record could not be found` at startup**: `BepInEx\\unity-libs\\2022.3.62.zip` was damaged by the first download (truncated or never finished) and BepInEx only ever reuses that broken file — the `Unable to execute IL2CPP chainloader, no plugins will be loaded` right after it is the consequence, so no plugin ever loads (and nothing in the game reacts). **Re-running the installer fixes it**: it verifies that file against SHA256 and replaces it. Restart the game afterwards.
- **Remove the loader completely**: after uninstalling, delete these six items from the game folder by hand — `winhttp.dll`, `doorstop_config.ini`, `.doorstop_version`, `changelog.txt`, `dotnet\\`, `BepInEx\\`.

### Malody 4.3.7 (nothing to install)

- **No action on the game side**: no files copied, no plugin installed, no game setting changed. Choosing **Malody 4** in the installer only records the game folder as `malody4Root` in `mma-shell-config.json` (you may also fill it in by hand, e.g. `D:\\Games\\Malody-4.3.7`).
- Afterwards, keep the shell running and start the game: selecting/playing a chart makes the card follow it. If the highlighted chart is not in the indexed set (this source indexes only `.mc` Key charts under `beatmap/`), the card keeps the previous chart and reports the reason.
- To switch the source off completely, point the `MMA_MALODY4_ROOT` environment variable at a nonexistent path (leaving the config empty does **not** disable it — see "Configuration").

## Configuration

The shell has two independent config files, both **next to the exe** (auto-created on first run), independent of tosu
settings (the tosu side only serves the osu! source):

**① Shell config `mma-shell-config.json`** (shell-only):

```json
{
  "gameClient": "Auto",
  "etternaRoot": "",
  "malodyRoot": "",
  "malody4Root": "",
  "hotkeys": { "topmost": "Ctrl+Shift+T", "clickThrough": "Ctrl+Shift+C", "settings": "Ctrl+Shift+S", "close": "Ctrl+Q" },
  "logLevel": "info"
}
```

- **gameClient**:
  - `Auto` (recommended; play state > recent activity > osu!>Etterna>Malody 4>Malody auto-follow)
  - or lock to one source (`Osu!` / `Etterna` / `Malody` / `Malody 4`).
- **etternaRoot / malodyRoot / malody4Root**: game install paths. `malody4Root` is written by the installer's **Malody 4** option (nothing to do on the Malody 4.3.7 game side); leaving it empty does **not** disable the source, because the shell's heuristic may still adopt a passing folder — to disable it, point the `MMA_MALODY4_ROOT` environment variable at a nonexistent path. Use forward slashes or double backslashes
  (`D:/Games/Etterna` or `D:\\Games\\Etterna` — a single `\` is invalid JSON escaping).
- **hotkeys**: window shortcuts. Defaults `Ctrl+Shift+T` topmost / `Ctrl+Shift+C` click-through / `Ctrl+Shift+S` open
  the settings window / `Ctrl+Q` close.
  Change if they conflict with your system (Ctrl/Shift/Alt/Win + single letter A–Z). Hotkeys register at startup
  only — restart the shell after editing.
- **logLevel**: log level. `debug` / `info` / `warn` / `error` / `off` (default `info`).

**② Plugin settings `mma-settings.json`**:

- Since some users don't use tosu, the shell provides an **offline mode**: edit `mma-settings.json` directly to
  configure the card display. Edits are picked up automatically within ~30 seconds (the shell re-reads the config
  periodically and pushes a settings frame — no restart needed; if the shell isn't running, they apply on next launch).
  - While tosu is **online** the tosu settings file wins (`settings/<plugin folder name>.values.json` inside the tosu
    install, read-only — the shell neither creates nor uses `mma-settings.json` then).
  - While tosu is **offline** the local `mma-settings.json` wins: the criterion is tosu being online, not the tosu
    settings file existing. With tosu installed but not running, old values left in `values.json` are **not** used.
    Offline with no local file, the shell generates a default skeleton from the plugin's `settings.json`.
- Keys match the tosu settings UI exactly; users who only change `gameClient` / `etternaRoot` / `malodyRoot` / `malody4Root` never
  touch this file (those live in `mma-shell-config.json`).
- A **graphical settings window** is now provided as well (see "Settings window" below). Editing the JSON files
  directly still works; refer to [settings.md](settings.md) for the key meanings.

A malformed config falls back to defaults with a warning in the log.

## Settings window

The shell ships a graphical settings window (the `settings.html` page, served by the shell's local port `24061`) for
the **shell config** (Shell), the **card settings** (Card Settings — the overlay card's own display and behaviour
settings) and **presets** (Presets). The window is hosted by the shell process, so **the shell must be
running**; changes only affect the card display itself and never interrupt an analysis (while it is open the main
window's always-on-top is temporarily cancelled — see below), and closing it does not quit the shell.

Three ways to open it (any one works):

| How | What to do |
| --- | --- |
| Global shortcut | `Ctrl+Shift+S` by default (change `hotkeys.settings` in `mma-shell-config.json`; restart the shell after editing) |
| Command line | `mma-shell.exe --settings`. If a shell is already running it forwards the request to that instance and **exits immediately** (no second overlay window, no main-window flash) |
| Browser | open `http://127.0.0.1:24061/settings.html` (local machine only; from any other address the page just shows `Open this page from the desktop shell: http://127.0.0.1:24061/settings.html`) |

The window's navigation order is **Shell → Card Settings → Presets**: **Shell** (`gameClient`, the three game roots
with the paths actually adopted, hotkeys, log level; root changes show the adopted path right away and hotkey changes
tell you a restart is needed), **Card Settings** (the overlay card's own settings: the same keys as the tosu settings
UI, grouped as in `settings.json`; every change is saved immediately) and **Presets** (the full preset manager,
identical to `presets.html`). The window remembers its own position and size in `mma-shell-state.json`'s `settings` field
next to the exe (with backward compatibility for the legacy `mma-shell-settings-window.json`); while it is open the main
window's always-on-top is temporarily cancelled (restored per the saved state when the settings window closes or fails
to build — the main window's state file is not touched by this).

**The Card Settings form is generated from `settings.json`** (no code is needed per setting):

- At runtime the page reads the plugin's `settings.json` and renders one row per entry: `header` becomes a group title,
  `checkbox` / `options` / `text` / `color` become the matching controls, and the default comes from the entry's
  `value`. The two system keys `preset` and `presetStorage` are not shown here (they belong to the preset area below).
- **Adding a setting needs no change to the settings page**: add the entry to `settings.json` (the tosu settings-UI
  definition plus its default) and keep `config.js`'s `defaults` and the matching parse/apply pair in sync as the
  repository's `AGENTS.md` requires — the page picks it up automatically the next time it opens.
- Entries whose `settings.json` type is `button` that are reachable from the shell (four of them: settings guide,
  presets guide, report issue, benchmark results) are not part of the form; they are rendered as the **Links** row at
  the very top of the page (below the status bar, above the nav), so they are visible the moment the window opens
  instead of at the bottom of the Card Settings form. The other two (presets manage page, debug page) point at
  tosu-hosted addresses that do not exist inside the shell, so the settings window does not offer them (`settings.json`
  and tosu's own settings dashboard keep all six). The read-only notice below still shows the tosu presets-page address
  while tosu is online, so that entry point is not lost.
- **Link behaviour**: clicking a link never navigates the settings window away. Inside the shell window, when the
  system browser cannot be opened (`window.open` is blocked), the page **copies the address to the clipboard** and
  shows `Link copied — open it in your browser.` so you can paste it into your own browser. The preset area's **Guide**
  button goes through the same path (it previously did nothing inside the shell window).
- **The hotkey fields always have a value**: at startup the shell checks `mma-shell-config.json`'s `hotkeys` and fills
  in any missing or empty key with its built-in default (`topmost` / `clickThrough` / `close` / `settings`, where
  opening the settings window defaults to **`Ctrl+Shift+S`**) without ever overwriting a value you wrote — so the
  "Open settings window" field in the Shell panel never shows up empty. Hotkeys register at startup only: restart the
  shell after changing them there.

**Online and offline behave differently**:

- **tosu online (read-only)**: the whole form is disabled and the shell rejects saves (HTTP 403). Change plugin
  settings in the tosu settings UI; manage presets on tosu's **Presets page** — the settings window shows that page's
  URL with a copy button (the URL comes from the button entry in the plugin's `settings.json`, with host:port replaced
  by the current `wsEndpoint`).
- **Offline (writable)**: each change is written to the local `mma-settings.json` immediately and **broadcast to the
  overlay right away** (no restart; the periodic re-read also re-checks within ~30 seconds). The preset area works and
  custom presets live in the local `mma-settings.json`'s `presetStorage` key, so they survive a restart. Offline the
  **LastSavedPreset row is not shown** (that is a tosu settings-page follow marker).
- tosu starting or stopping mid-run: the shell follows within ≤30 seconds and switches what is writable together with
  the page and its preset area.

## Logs

Shell logs go to the `logs/` folder **next to the exe**: `logs/mma-shell-YYYYMMDD.log` (rotated daily, 7 kept). When
troubleshooting, set `logLevel` to `debug` and post the latest log to
[Issues](https://github.com/LeoBlackMT/osumania_map_analyser/issues). If something misbehaves, check the log output
first.

## Troubleshooting / known issues

- **Hollow grey dot / frozen card**: game not running, bridge not installed (Malody 4.3.7 needs no bridge), or shell not running.
- **Malody 4.3.7 not following**: set `logLevel` to `debug` and read the shell log — it reports the reason (process not found / version mismatch / access denied / chart not indexed, and so on). Common causes are a game version other than 4.3.7, a wrong game folder, or a chart that is not an `.mc` Key chart under `beatmap/`.
- **Malody editor no reaction / timeout**: shell not running, or the window hasn't finished loading (start the shell
  first, wait a few seconds, then trigger).
- **White/black flash on open**: known cosmetic quirk of transparency; doesn't affect use.
- **Card body occasionally shows No Data**: for Etterna, switch to another chart and back; for Malody, click
  MMA Analyze again.

# 了解更多 / Learn more

- 桌面壳技术细节（架构、目录检测、契约、构建）/ Shell technical details: [features/desktop-shell.md](features/desktop-shell.md)
- 安装器完整参数与行为 / Installer flags & behavior: [bridges/README.md](../bridges/README.md)
- 多数据源架构（Auto 跟随、转换器）/ Multi-source architecture: [features/multi-source.md](features/multi-source.md)
