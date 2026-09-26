# 桌面壳使用教程 / Desktop Shell Guide

> English version [below](#english).

# 中文

## 这是什么

桌面壳（mma-shell）是一个可选的小窗口程序，用来：把分析卡片显示在**独立的置顶小窗口**里（可以盖在游戏/浏览器上）；在**不启动 tosu** 的情况下，让卡片接收来自 Etterna、Malody V 编辑器或 Malody 4.3.7 原生客户端的数据。浏览器旧用法（tosu 插件）不受影响，可以正常使用。

- 系统要求：Windows / Linux（桥安装器为 PowerShell 脚本，仅 Windows；Linux 按下文手动步骤安装）。
- Etterna 与 Malody V 数据源需要安装对应游戏的桥文件（见下文「安装桥」）；**Malody 4.3.7 不需要任何游戏侧文件**——它是零注入只读观察，安装器只记录路径。
- 最低游戏版本：**Etterna 0.70+**（Linux 需 0.75+ 官方 Linux 版）；**Malody V 6.6.43+**（无 Linux 版，该源仅 Windows）；**Malody 4.3.7 原生客户端**（仅 Windows，且只支持 4.3.7 这一个版本）。
- 支持的谱面类型：`.mc` `.ssc` `.sm`。

> 注意：
> - 本子项目受[DanielEtterna](https://github.com/JoseMGS3/DanielEtterna)启发，感谢 DanielEtterna 的作者提供的思路与部分代码。
> - 壳是实验性功能，可能存在未知问题。请在使用中遇到问题时及时反馈。
> - 受限于 Malody V 的 API，Malody V 源有两条通道：不装插件时走编辑器（`MMA Analyze` 按钮），装了「游戏内选曲桥」后**选曲 / 游玩 / 结算都会自动跟随**（见「安装桥」的 Malody V 一段）。两条通道并存，编辑器那条是不装插件时的回退。
> - Malody 4.3.7 数据源相反：**在游戏内选曲与游玩时自动跟随**（只读观察游戏高亮的谱面），游戏侧无需安装任何东西；它只支持 4.3.7 这一个版本，版本不符时该源不可用并给出原因，其余三个源不受影响。
> - 转换得到的谱面默认取 **OD 9**（`.sm` / `.ssc` / `.mc`）；**例外**是 Malody 4.3.7 与 Malody V 两个源：它们的 OD 由游戏内判定档（A~E）与当局倍率共同决定（星级会随之变化）。Malody V 还取决于是否开启 **Pro**（严格组）：Pro 状态未知时不会猜，会**关闭动态 OD 并在状态行说明原因**。

## 一、快速开始

- 从发布页下载压缩包（推荐），或从 CI artifact 单独下载 `mma-shell.exe`（Windows）/ `mma-shell`（Linux）。
  - **压缩包**：已包含插件目录、`mma-shell.exe` 与 `bridges/`，整体解压到 tosu 插件目录（`tosu/static/`）即可。
  - **单独 exe**：请放进插件目录 `ManiaMapAnalyser by Leo_Black` 内，放好后双击运行。
  - 如果你不使用 tosu，直接解压到你喜欢的位置即可。
- 随后请参照下方「安装桥」章节安装 Etterna 与 Malody V 的桥文件（Malody 4.3.7 无需安装任何游戏侧文件，安装器会替你记录路径即可）。
- 如有需要，请编辑 `mma-shell-config.json` 来对壳进行配置（游戏安装路径 `etternaRoot` / `malodyRoot` / `malody4Root`、快捷键 `hotkeys` 等）；离线模式 / 没有 tosu 时，也可编辑 `mma-settings.json` 配置卡片显示（见「配置」）。
- 启动壳，然后在 Etterna 中选歌即可显示；或者在 Malody V 中选择编辑谱面，在编辑器中点击「MMA Analyze」按钮进行分析；或者直接打开 Malody 4.3.7，在游戏里选歌/游玩即自动跟随。
- 数据源指示：卡片右上状态行末尾的小圆点——粉色=osu!、紫色=Etterna、亮青色=Malody 4、蓝色=Malody V、灰色空心=当前没有数据源。

## 二、窗口操作说明

| 操作 | 方法 |
| --- | --- |
| 拖动整个窗口 | 按住窗口顶部拖动条（顶端发光细条，中间有 `⋮⋮` 标志）拖动 |
| 改变窗口大小 | 拖动窗口边缘（无边框，四周可拖） |
| 页面缩放 | 按住 `Ctrl` 滚动鼠标滚轮；或 `Ctrl +` / `Ctrl -`；`Ctrl 0` 复位 |
| 开关置顶（默认置顶） | 默认按 `Ctrl + Shift + T`（全局快捷键） |
| 开关点击穿透（默认关） | 默认按 `Ctrl + Shift + C`（全局快捷键） |
| 打开设置窗口 | 默认按 `Ctrl+Shift+S`（全局快捷键；也可用 `mma-shell.exe --settings` 或浏览器直开，见「五、设置窗口」） |
| 关闭窗口 | `Alt+F4` 或默认 `Ctrl+Q`（全局快捷键） |

窗口位置/尺寸/置顶/点击穿透会跨启动记忆（下次打开自动恢复）；全局快捷键在窗口失焦或点击穿透时同样有效（Windows/X11；**Linux Wayland 会话**下全局快捷键不可用，窗口聚焦时由页面内快捷键兜底，键位相同）。

## 三、安装桥

**推荐：自动安装**（桥安装器为 Windows PowerShell 脚本，仅 Windows；Linux 按下方手动步骤操作）。bridges 目录自带交互式安装器，按菜单选择 Install / Uninstall 与游戏：

- 双击 `bridges/install-bridge.bat`（英文界面）或 `bridges/install-bridge-zh.bat`（中文界面）；也可命令行运行 `powershell -NoProfile -ExecutionPolicy Bypass -File install-bridge.ps1 [-Chinese]`。
- 自动探测游戏安装目录（正在运行的进程 → 环境变量 → 常见安装路径；Malody V 额外探测 Steam 库）。
- 探测失败时可从图形对话框「浏览选择」，或手动输入路径（`/`、`\`、`\\`、包裹引号与尾部斜杠均能兼容归一化）。
- 默认安装到 Etterna 的 Rebirth 主题（安装前有主题结构预检；Rebirth 不存在时列出可安装主题供选择）。
- 自动把游戏路径写入 `mma-shell-config.json`。
- 选择第三个游戏项 **Malody 4** 时**不会复制任何文件**：只把 `malody4Root` 写进 `mma-shell-config.json`（游戏侧无需任何操作）；卸载时也只清空这个键。
- 卸载时优先使用配置中记录的游戏目录，会询问卸载哪个主题，只移除自身注入行，与其他脚本（如 DanOverlay）共存。
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
3. **主题更新后必须重装这两个文件**（主题包覆盖会删掉它们）。
4. 打开插件目录下的 `mma-shell-config.json` 文件，在设置里填写 `etternaRoot`（例如 `D:\\Games\\Etterna`）。

### Malody V

Malody V 有两个互不影响的通道，按需要选：

#### A. 编辑器通道（不需要插件）

- 把 `bridges/malody/mma_editor.lua` 放到 `MalodyV/editor/` 中（目录不存在就新建）。随后打开游戏 → 打开谱面编辑器 → **MMA Analyze** → 卡片显示本谱分析。
- 或者在游戏编辑器左上角点击按钮 → 插件管理 → 导入。
- 打开插件目录下的 `mma-shell-config.json` 文件，在设置里填写 `malodyRoot`（例如 `D:\\Steam\\steamapps\\common\\MalodyV`）。**一般不用手动填**：壳会自动探测正在运行的进程、`MMA_MALODY_ROOT` 环境变量、Steam 库与常见安装路径。

#### B. 游戏内选曲桥（推荐；选曲/游玩/结算自动跟随）

1. **先准备加载器**。从 BepInEx 6 IL2CPP `6.0.0-be.788` 的 GitHub Release 下载资产 `bepinex-il2cpp-788.zip`，保存为 `bridges/malody/bepinex/loader/bepinex-il2cpp-788.zip`（该目录不入库），或设环境变量 `MMA_BEPINEX_LOADER_ZIP` 指向它。安装前会校验 SHA256 `F4CC496BD098A0DF4164B81E3737297707F13A47C2478DBA2F60EEFAB784817A`，不匹配就中止、不写任何文件。
2. **双击 `install-bridge-zh.bat`**（或 `install-bridge.bat`）→ **Install** → **Malody V** → **游戏内选曲桥**。装完目标文件是 `BepInEx\\plugins\\MalodyInsight\\MMAMalodySelection.dll`。
3. **验证**：开着壳启动游戏，进选曲界面切谱 —— 卡片应立刻跟着换，且星数随判定档、Pro、倍率变化。
4. **卸载**：同样的入口选 **Uninstall**。只删除本安装器记录过的文件；加载器会保留（不想要就按下面"排障"里的六项自行删）。

> 装过上游那份 `MalodyInsightBridge.dll` 的话，安装器会**报告并给出指引，但绝不替你删除**——两份插件会挂在同一批游戏方法上（双 Hook）。要改用本 fork，请自行删掉那个文件再装。

#### 排障

- **卡片没反应 / 一直空着**：先确认壳在运行；再打开壳日志（见「五、日志」）搜 `malody bridge` —— 若出现 `REJECTED … malodyRoot unavailable`，就是壳没解析到游戏目录，手动在 `mma-shell-config.json` 里填 `malodyRoot`。
- **端口 17653 被占用**：卡片不动且壳日志提示端口被占。关掉多余的 `mma-shell` 实例（同时只能有一个），或重启壳。
- **游戏里看不到任何变化**：看游戏目录下的 `BepInEx\\LogOutput.log` —— 应有 `Selection bridge ready` 与一行行 `Observe …`。若整个文件都不存在，说明加载器没被游戏加载（多半是杀软隔离了 `winhttp.dll`，或游戏没重启过）。
- **星数不随速率变化**：确认装的是本 fork 的 `MMAMalodySelection.dll`，而不是上游那份。
- **Pro（严格组）没生效**：插件会主动读 Pro，**不需要**特意打开 JUDGE 面板；若状态行提示「动态 OD 未启用：Pro 状态未知」，打开一次 JUDGE 面板再关闭即可。
- **想彻底移除加载器**：卸载后手工删除游戏目录下这六项 —— `winhttp.dll`、`doorstop_config.ini`、`.doorstop_version`、`changelog.txt`、`dotnet\\`、`BepInEx\\`。

### Malody 4.3.7（无需安装）

- **游戏侧不需要任何操作**：不复制文件、不装插件、不改游戏设置。安装器里选 **Malody 4** 只会把游戏目录写进 `mma-shell-config.json` 的 `malody4Root`（也可以手动填，例如 `D:\\Games\\Malody-4.3.7`）。
- 之后开着壳启动游戏，在游戏里选歌/游玩即自动跟随；游戏高亮的谱面若不在索引范围内（该源只索引 `beatmap/` 下的 `.mc` Key 谱面），卡片会保留上一张并给出原因。
- 想彻底关掉该源：把环境变量 `MMA_MALODY4_ROOT` 指到一个不存在的路径（留空配置**不会**关断，见下文）。

## 四、配置

壳有两份独立配置，都在 **exe 旁**（首次启动自动生成骨架），与 tosu 设置无关（tosu 侧只服务 osu! 来源）：

**① 壳配置 `mma-shell-config.json`**（仅壳使用）：

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

- **gameClient**：
  - `Auto`（推荐，按游玩中 > 近期活动 > osu!>Etterna>Malody 4>Malody 自动跟随）
  - 可锁定为某个来源（`Osu!` / `Etterna` / `Malody` / `Malody 4`）。
- **etternaRoot / malodyRoot / malody4Root**：游戏安装路径。`malody4Root` 由安装器的 **Malody 4** 选项自动写入（Malody 4.3.7 游戏侧无需任何操作）；**留空不等于关断**——壳的启发探测可能仍会采纳一个通过的目录，要关断请把环境变量 `MMA_MALODY4_ROOT` 指向不存在的路径。**路径可写正斜杠或双反斜杠**（如 `D:/Games/Etterna` 或 `D:\\Games\\Etterna`——单反斜杠 `D:\Games` 在 JSON 里是非法转义，请用 `/` 或 `\\`）；
- **hotkeys**：窗口快捷键。默认 `Ctrl+Shift+T` 置顶 / `Ctrl+Shift+C` 穿透 / `Ctrl+Shift+S` 打开设置窗口 / `Ctrl+Q` 关闭；若与系统冲突可改（支持 Ctrl/Shift/Alt/Win + 单个字母键 A–Z）。快捷键只在启动时注册，改动后需**重启壳**生效。
- **logLevel**：日志级别。`debug` / `info` / `warn` / `error` / `off`（默认 info）。

**② 插件设置 `mma-settings.json`**：

- 考虑到用户可能不使用 tosu，壳提供了**离线模式**，允许用户直接编辑 `mma-settings.json` 来配置卡片显示。修改后**约 30 秒内自动生效**（壳会周期性重读配置并推送，无需重启；壳未运行时则下次启动生效）。
  - **tosu 在线**时以 tosu 设置文件为准（tosu 安装目录的 `settings/<插件目录名>.values.json`，只读，此时壳既不生成也不使用 `mma-settings.json`）。
  - **tosu 离线**时以本地 `mma-settings.json` 为准——判定只看 tosu 是否在线，不看 tosu 设置文件在不在：tosu 装着但没运行时，即使 `values.json` 里还留着旧值也不采用。离线且本地文件不存在时，壳按插件的 `settings.json` 生成一份默认骨架。
- 设置键与 tosu 设置界面完全一致；只改 `gameClient/etternaRoot/malodyRoot/malody4Root` 的用户**不需要碰它**（这四个在 mma-shell-config.json）。
- 也提供了**图形化设置界面**（见下文「五、设置窗口」）。不想开窗口时，仍可直接编辑 JSON 文件，请对照[settings.md](settings.md)的说明来修改。

配置填错/JSON损坏将自动回落默认并在日志中警告。

## 五、设置窗口

壳自带一个图形化设置窗口（页面 `settings.html`，由壳的本机端口 `24061` 提供），用来改**插件设置**、**壳配置**与**预设**。窗口由壳进程承载，所以**壳必须在运行**；改动只作用于卡片显示本身，不会打断分析（打开期间主窗口的置顶会临时取消，见下），关闭它也不会退出壳。

三种打开方式（任选一种）：

| 方式 | 做法 |
| --- | --- |
| 全局快捷键 | 默认 `Ctrl+Shift+S`（可经 `mma-shell-config.json` 的 `hotkeys.settings` 改，改后需重启壳） |
| 命令行 | `mma-shell.exe --settings`。已有一个壳在运行时，它把请求转交给那个实例并**立即退出**（不会开出第二个叠加窗口，也不会闪一下主窗口） |
| 浏览器直开 | `http://127.0.0.1:24061/settings.html`（仅本机；从其他地址打开时页面只显示提示 `Open this page from the desktop shell: http://127.0.0.1:24061/settings.html`） |

窗口内有三块内容：**插件设置**（与 tosu 设置界面同一套键、按分组排列，改一项即保存）、**壳配置**（`gameClient`、三个游戏根目录与实际采纳路径、快捷键、日志级别；改根目录会立刻反映实际采纳的路径，改快捷键会提示需要重启）、**预设区**（完整的预设管理，与 `presets.html` 相同）。窗口的位置与大小记忆在 exe 旁的 `mma-shell-settings-window.json`（与主窗口的 `mma-shell-state.json` 分开，互不影响）；设置窗口打开期间主窗口的置顶会临时取消（关闭或建窗失败后按原状态恢复，此过程不写主窗口状态文件）。

**在线与离线行为不同**：

- **tosu 在线（只读）**：设置表单整体禁用，保存会被壳拒绝（返回 403）。插件设置请在 tosu 设置界面里改；预设请在 tosu 的 **Presets 页面**里管理——设置窗口会显示该页面地址并可一键复制（地址取插件 `settings.json` 里那个按钮的值，再把主机端口换成当前的 `wsEndpoint`）。
- **离线（可写）**：改一项立即写入本地 `mma-settings.json`，并**立即广播给叠加界面**（无需重启；周期检测也会在约 30 秒内复核一次）。预设区可用，自定义预设保存在本地 `mma-settings.json` 的 `presetStorage` 键里，重启壳后仍在。离线时**不显示** LastSavedPreset 行（那是 tosu 设置页面的跟随标记）。
- tosu 中途启动或关闭：壳会在 ≤30 秒内跟随切换，页面与预设区的可写状态一起切换。

## 六、日志

壳运行日志写在 **exe 旁的 `logs/` 目录**：`logs/mma-shell-YYYYMMDD.log`（按日轮转，保留 7 天）。排查问题时将 `logLevel` 设为 `debug`，把最新日志发到 [Issues](https://github.com/LeoBlackMT/osumania_map_analyser/issues) 即可；遇到问题请先确认日志内容。

## 七、常见问题/已知问题

- **圆点灰空心 / 卡片不动**：确认对应游戏已开、桥已装（Malody 4.3.7 不需要桥）、壳在运行。
- **Malody 4.3.7 不跟随**：把 `logLevel` 设为 `debug` 看壳日志——日志会给出原因（进程没找到 / 版本不符 / 没有权限 / 该谱不在索引里等）；常见成因是游戏版本不是 4.3.7、或游戏目录设错、或谱面不是 `beatmap/` 下的 `.mc` Key 谱面。
- **Malody 编辑器点了没反应/报超时**：壳没在运行，或窗口还没加载完（先开壳，等几秒再点）。
- **窗口是黑/白的闪一下**：透明白闪为已知小抖动；不影响使用。
- **卡片主体偶尔显示No Data**：对于Etterna，切成另一张谱面再切回来即可。对于Malody，请重新点击 MMA Analyze。

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
| Move the whole window | drag the top drag bar (glow strip with `⋮⋮` hint at the window top) |
| Resize | drag any window edge (borderless, draggable on all sides) |
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

1. **Get the loader first.** Download the asset `bepinex-il2cpp-788.zip` from the upstream BepInEx 6 IL2CPP `6.0.0-be.788` GitHub release and save it as `bridges/malody/bepinex/loader/bepinex-il2cpp-788.zip` (that folder is not committed), or point `MMA_BEPINEX_LOADER_ZIP` at it. The archive is checked against SHA256 `F4CC496BD098A0DF4164B81E3737297707F13A47C2478DBA2F60EEFAB784817A`; a mismatch aborts without writing a byte.
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
**plugin settings**, **shell config** and **presets**. The window is hosted by the shell process, so **the shell must be
running**; changes only affect the card display itself and never interrupt an analysis (while it is open the main
window's always-on-top is temporarily cancelled — see below), and closing it does not quit the shell.

Three ways to open it (any one works):

| How | What to do |
| --- | --- |
| Global shortcut | `Ctrl+Shift+S` by default (change `hotkeys.settings` in `mma-shell-config.json`; restart the shell after editing) |
| Command line | `mma-shell.exe --settings`. If a shell is already running it forwards the request to that instance and **exits immediately** (no second overlay window, no main-window flash) |
| Browser | open `http://127.0.0.1:24061/settings.html` (local machine only; from any other address the page just shows `Open this page from the desktop shell: http://127.0.0.1:24061/settings.html`) |

The window has three parts: **plugin settings** (the same keys as the tosu settings UI, grouped; every change is saved
immediately), **shell config** (`gameClient`, the three game roots with the paths actually adopted, hotkeys, log level;
root changes show the adopted path right away and hotkey changes tell you a restart is needed), and the **preset area**
(the full preset manager, identical to `presets.html`). The window remembers its own position and size in
`mma-shell-settings-window.json` next to the exe (separate from the main window's `mma-shell-state.json`, so the two
never interfere); while it is open the main window's always-on-top is temporarily cancelled (restored per the saved
state when the settings window closes or fails to build — the main window's state file is not touched by this).

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
