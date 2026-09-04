# 桌面壳使用教程 / Desktop Shell Guide

> English version [below](#english).

# 中文

## 这是什么

桌面壳（mma-shell）是一个可选的小窗口程序，用来：把分析卡片显示在**独立的置顶小窗口**里（可以盖在游戏/浏览器上）；在**不启动 tosu** 的情况下，让卡片接收来自 Etterna 或 Malody V 编辑器的数据。浏览器旧用法（tosu 插件）不受影响，可以正常使用。

系统要求：Windows；Etterna/Malody 数据源需要安装对应游戏的桥文件（见下文「安装桥」）。最低游戏版本：**Etterna 0.70+**；**Malody V 6.6.43+**
支持的谱面类型：`.mc` `.ssc` `.sm`。

> 注意：
> - 本子项目受[DanielEtterna](https://github.com/JoseMGS3/DanielEtterna)启发，感谢 DanielEtterna 的作者提供的思路与部分代码。
> - 壳是实验性功能，可能存在未知问题。请在使用中遇到问题时及时反馈。
> - 受限于 Malody V 的 API，Malody 数据源仅在编辑器中可用，无法在游玩时使用。请在 Malody V 编辑器中使用「MMA Analyze」功能来查看谱面分析。
> - 转换后默认为 OD9。

## 一、快速开始

- 从发布页直接下载压缩包，或从 CI artifact 下载 `mma-shell.exe`。
  - 如果你下载的是exe，请**放在 Mania Map Analyser 插件目录内**，放好后双击运行即可。
  - 如果你下载的是压缩包，请解压后放在 **tosu 插件目录**（`tosu/static/`）内。
- 随后请参照下方「安装桥」章节安装 Etterna/Malody 桥文件。
- 如有需要，请编辑 `mma-shell-config.json` 来对壳进行配置，例如指定游戏安装路径（`etternaRoot` / `malodyRoot`）或修改快捷键（`hotkeys`）。你也可以编辑 `mma-settings.json` 来配置卡片显示（离线模式 / 没有 tosu 时使用）。
- 启动壳，然后在 Etterna 中选歌即可显示；或者在 Malody V 中选择编辑谱面，在编辑器中点击「MMA Analyze」按钮进行分析。
- 数据源指示：卡片右上状态行末尾的小圆点——蓝色=osu!、绿色=Etterna、橙色=Malody、灰色空心=当前没有数据源。

## 二、窗口操作说明

| 操作 | 方法 |
| --- | --- |
| 拖动整个窗口 | 按住窗口顶部拖动条（顶端发光细条，中间有 `⋮⋮` 标志）拖动 |
| 改变窗口大小 | 拖动窗口边缘（无边框，四周可拖） |
| 页面缩放 | 按住 `Ctrl` 滚动鼠标滚轮；或 `Ctrl +` / `Ctrl -`；`Ctrl 0` 复位 |
| 开关置顶（默认置顶） | 默认按 `Ctrl + Shift + T`（全局快捷键） |
| 开关点击穿透（默认关） | 默认按 `Ctrl + Shift + C`（全局快捷键） |
| 关闭窗口 | `Alt+F4` 或默认 `Ctrl+Q`（全局快捷键） |

## 三、安装桥

> **推荐：自动安装**。bridges 目录自带交互式安装器：双击 `bridges/install-bridge.bat`（中文界面用 `bridges/install-bridge-zh.bat`；或命令行运行 `powershell -NoProfile -ExecutionPolicy Bypass -File install-bridge.ps1 [-Chinese]`），按菜单选择 Install / Uninstall 与游戏即可——自动探测游戏安装目录（进程/环境变量/常见路径；探测失败可浏览选择或手动输入路径）、默认安装到 Etterna 的 Rebirth 主题、自动写入 `mma-shell-config.json`；卸载时优先使用配置中记录的游戏目录（不重复探测），会询问卸载哪个主题（`-Theme`/`-Yes` 可跳过），只移除自身注入行，与其他脚本（如 DanOverlay）共存。若你想手动安装，按下文步骤操作。

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
3. **主题更新后必须重装这两个文件**（主题包覆盖会删掉它们）。
4. 打开插件目录下的 `mma-shell-config.json` 文件，在设置里填写 `etternaRoot`（例如 `D:\\Games\\Etterna`）。

### Malody V

- 把 `bridges/malody/mma_editor.lua` 放到 `MalodyV/editor/` 中（目录不存在就新建）。随后打开游戏 → 打开谱面编辑器 → **MMA Analyze** → 卡片显示本谱分析。
- 或者在游戏编辑器左上角点击按钮 → 插件管理 → 导入。
- 打开插件目录下的 `mma-shell-config.json` 文件，在设置里填写 `malodyRoot`（例如 `D:\\Steam\\steamapps\\common\\MalodyV`）。

## 四、配置

壳有两份独立配置，都在 **exe 旁**（首次启动自动生成骨架），与 tosu 设置无关（tosu 侧只服务 osu! 来源）：

**① 壳配置 `mma-shell-config.json`**（仅壳使用）：

```json
{
  "gameClient": "Auto",
  "etternaRoot": "",
  "malodyRoot": "",
  "hotkeys": { "topmost": "Ctrl+Shift+T", "clickThrough": "Ctrl+Shift+C", "close": "Ctrl+Q" },
  "logLevel": "info"
}
```

- **gameClient**：
  - `Auto`（推荐，按游玩中 > 近期活动 > osu!>Etterna>Malody 自动跟随）
  - 可锁定为某个来源（`Osu!` / `Etterna` / `Malody`）。
- **etternaRoot / malodyRoot**：游戏安装路径。**路径可写正斜杠或双反斜杠**（如 `D:/Games/Etterna` 或 `D:\\Games\\Etterna`——单反斜杠 `D:\Games` 在 JSON 里是非法转义，请用 `/` 或 `\\`）；
- **hotkeys**：窗口快捷键。默认 `Ctrl+Shift+T` 置顶 / `Ctrl+Shift+C` 穿透 / `Ctrl+Q` 关闭；若与系统冲突可改（支持 Ctrl/Shift/Alt/Win + A–Z）。改动后重启壳生效。
- **logLevel**：日志级别。`debug` / `info` / `warn` / `error` / `off`（默认 info）。

**② 插件设置 `mma-settings.json`**：

- 考虑到用户可能不使用 tosu，壳提供了**离线模式**，允许用户直接编辑 `mma-settings.json` 来配置卡片显示。修改后重启壳生效。
  - 如果**tosu 设置文件可用**（tosu 安装目录的 `settings/<插件目录名>.values.json`）时，壳**优先使用它**（在线只读 / 离线读文件），不生成也不使用 mma-settings.json。
  - 如果**找不到 tosu 设置文件**时进入本地模式：存在 `mma-settings.json` 则直接使用；不存在则由壳按插件的 `settings.json` 生成默认骨架。
- 设置键与 tosu 设置界面完全一致；只改 `gameClient/etternaRoot/malodyRoot` 的用户**不需要碰它**（这三个在 mma-shell-config.json）。
- 受限于框架和操作复杂度，**暂不提供图形化设置界面**，目前折中的方案是直接编辑 JSON 文件。请对照[settings.md](settings.md)的说明来修改。

配置填错/JSON损坏将自动回落默认并在日志中警告。

## 五、日志

壳运行日志写在 **插件log目录** `log/mma-shell-YYYYMMDD.log`（按日轮转，保留 7 天）。排查问题时将 `logLevel` 设为 `debug` ，随后把最新日志内容发给我们即可。如果你遇到问题，请先确认日志输出内容。

## 六、常见问题/已知问题

- **圆点灰空心 / 卡片不动**：确认对应游戏已开、桥已装、壳在运行。
- **Malody 编辑器点了没反应/报超时**：壳没在运行，或窗口还没加载完（先开壳，等几秒再点）。
- **窗口是黑/白的闪一下**：透明白闪为已知小抖动；不影响使用。
- **卡片主体偶尔显示No Data**：对于Etterna，切成另一张谱面再切回来即可。对于Malody，请重新点击 MMA Analyze。

# English

## What this is

mma-shell is an optional small window program that: shows the analysis card in a **standalone always-on-top mini window** (can overlay games/browsers); and, **without tosu running**, lets the card receive data from **Etterna** or the **Malody V editor**. The classic browser usage (tosu plugin) is unaffected.

System requirements: Windows; Etterna/Malody data sources need their game bridge files installed (see "Bridges" below). Minimum game versions: **Etterna 0.70+**; **Malody V 6.6.43+**. Supported chart types: `.mc`, `.ssc`, `.sm`.

> Note:
> - This subproject is inspired by [DanielEtterna](https://github.com/JoseMGS3/DanielEtterna); thanks to its author for the ideas and parts of the code.
> - The shell is experimental — unknown issues may exist. Please report anything you find.
> - Due to Malody V API limitations, the Malody source only works in the editor, not during gameplay. Use the "MMA Analyze" button in the Malody V editor to view a chart's analysis.
> - Default OD after conversion is 9.

## Quick start

- Download the archive from the release page, or `mma-shell.exe` from CI artifacts.
  - If you downloaded the exe, **place it inside the ManiaMapAnalyser plugin folder** and double-click to run.
  - If you downloaded the archive, extract it into **the tosu plugin folder** (`tosu/static/`).
- Then install the Etterna/Malody bridge files per the "Bridges" section below.
- If needed, edit `mma-shell-config.json` to configure the shell — e.g. game install paths (`etternaRoot` / `malodyRoot`) or shortcuts (`hotkeys`). You can also edit `mma-settings.json` to configure the card display (offline mode / when tosu is absent).
- Start the shell: select a song in Etterna to display, or select a chart in Malody V and click "MMA Analyze" in the editor.
- Source indicator: the small dot at the end of the card's top status row — blue = osu!, green = Etterna, orange = Malody, hollow grey = no data source.

## Window controls

| Action | How |
| --- | --- |
| Move the whole window | drag the top drag bar (glow strip with `⋮⋮` hint at the window top) |
| Resize | drag any window edge (borderless, draggable on all sides) |
| Zoom | `Ctrl` + mouse wheel, or `Ctrl +` / `Ctrl -`; `Ctrl 0` resets |
| Always-on-top toggle (default on) | `Ctrl + Shift + T` by default (global shortcut) |
| Click-through toggle (default off) | `Ctrl + Shift + C` by default (global shortcut) |
| Close | `Alt+F4` or `Ctrl+Q` by default (global shortcut) |

Window position/size/topmost/click-through are remembered across launches. Global shortcuts keep working while the window is unfocused or click-through is active.

## Bridges

> **Recommended: automated install.** The bridges folder ships an interactive installer: double-click `bridges/install-bridge.bat` (use `bridges/install-bridge-zh.bat` for the Chinese UI; or run `powershell -NoProfile -ExecutionPolicy Bypass -File install-bridge.ps1 [-Chinese]`), pick Install / Uninstall and the game from the menu — it auto-detects game install folders (process / env vars / common paths; if detection fails you can browse for the folder or type a path), installs into the Etterna Rebirth theme by default, and writes `mma-shell-config.json` for you; on uninstall it reuses the game folder recorded in the config (no re-probing) and asks which theme to remove (`-Theme`/`-Yes` skip the prompt), removing only its own injection lines and coexisting with other scripts (e.g. DanOverlay). To install manually, follow the steps below.

### Etterna

1. Copy `bridges/etterna/mma_bridge.lua` into `Etterna/Themes/<your theme>/BGAnimations/ScreenSelectMusic decorations/`; then add one line before `return t` in that folder's `default.lua`: `t[#t + 1] = LoadActor("mma_bridge.lua")`. Example:

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

2. Copy `bridges/etterna/mma_gameplay.lua` into `Etterna/Themes/<your theme>/BGAnimations/ScreenGameplay overlay/`; add `t[#t + 1] = LoadActor("mma_gameplay.lua")` before `return t` in its `default.lua` too.
3. **Re-install both files after every theme update** (theme packages overwrite and remove them).
4. Open `mma-shell-config.json` next to the plugin and fill in `etternaRoot` (e.g. `D:\\Games\\Etterna`).

### Malody V

- Put `bridges/malody/mma_editor.lua` into `MalodyV/editor/` (create the folder if missing). Then open the game → open the chart editor → **MMA Analyze** → the card shows this chart's analysis.
- Or, in the game editor's top-left, open the plugin manager and import the file.
- Open `mma-shell-config.json` next to the plugin and fill in `malodyRoot` (e.g. `D:\\Steam\\steamapps\\common\\MalodyV`).

## Configuration

The shell has two independent config files, both **next to the exe** (auto-created on first run), independent of tosu settings (the tosu side only serves the osu! source):

**① Shell config `mma-shell-config.json`** (shell-only):

```json
{
  "gameClient": "Auto",
  "etternaRoot": "",
  "malodyRoot": "",
  "hotkeys": { "topmost": "Ctrl+Shift+T", "clickThrough": "Ctrl+Shift+C", "close": "Ctrl+Q" },
  "logLevel": "info"
}
```

- **gameClient**:
  - `Auto` (recommended; play state > recent activity > osu!>Etterna>Malody auto-follow)
  - or lock to one source (`Osu!` / `Etterna` / `Malody`).
- **etternaRoot / malodyRoot**: game install paths. Use forward slashes or double backslashes (`D:/Games/Etterna` or `D:\\Games\\Etterna` — a single `\` is invalid JSON escaping).
- **hotkeys**: window shortcuts. Defaults `Ctrl+Shift+T` topmost / `Ctrl+Shift+C` click-through / `Ctrl+Q` close. Change if they conflict with your system (Ctrl/Shift/Alt/Win + A–Z). Restart the shell after editing.
- **logLevel**: log level. `debug` / `info` / `warn` / `error` / `off` (default `info`).

**② Plugin settings `mma-settings.json`**:

- Since some users don't use tosu, the shell provides an **offline mode**: edit `mma-settings.json` directly to configure the card display. Changes take effect after restarting the shell.
  - When the **tosu settings file** (`settings/<plugin folder name>.values.json` inside the tosu install) is available, the shell **prefers it** (read-only online / read file offline) and never creates or uses `mma-settings.json`.
  - Without a tosu settings file, local mode kicks in: existing `mma-settings.json` is used as-is; otherwise the shell generates a default skeleton from the plugin's `settings.json`.
- Keys match the tosu settings UI exactly; users who only change `gameClient/etternaRoot/malodyRoot` never touch this file (those live in `mma-shell-config.json`).
- No GUI settings panel is provided for now (framework/effort tradeoff); the pragmatic approach is editing the JSON files directly. Refer to [settings.md](settings.md) for the key meanings.

A malformed config falls back to defaults with a warning in the log.

## Logs

Shell logs go to **the plugin's log folder** `log/mma-shell-YYYYMMDD.log` (rotated daily, 7 kept). When troubleshooting, set `logLevel` to `debug` and send us the latest log. If something misbehaves, check the log output first.

## Troubleshooting / known issues

- **Hollow grey dot / frozen card**: game not running, bridge not installed, or shell not running.
- **Malody editor no reaction / timeout**: shell not running, or the window hasn't finished loading (start the shell first, wait a few seconds, then trigger).
- **White/black flash on open**: known cosmetic quirk of transparency; doesn't affect use.
- **Card body occasionally shows No Data**: for Etterna, switch to another chart and back; for Malody, click MMA Analyze again.