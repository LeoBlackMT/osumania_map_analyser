# 桌面壳使用教程 / Desktop Shell Guide

> 面向人类用户。技术说明见 [docs/features/desktop-shell.md](features/desktop-shell.md)。
> This is the human-facing tutorial; technical details live in
> [docs/features/desktop-shell.md](features/desktop-shell.md).

# 中文

## 这是什么

桌面壳（mma-shell）是一个可选的小窗口程序，用来：

- 把分析卡片显示在**独立的置顶小窗口**里（可以盖在游戏/浏览器上）；
- 在**不启动 tosu** 的情况下，让卡片跟随 **Etterna** 或 **Malody V**（无需
  osu! / tosu 也在线）；
- 浏览器旧用法（tosu 插件页）不受影响，可以继续用。

系统要求：Windows（推荐）或带合成器的 Linux；Etterna/Malody 数据源需要
对应游戏的桥文件（见下文「安装桥」）。

## 一、获取与安装

1. 从发布页（或 CI artifact）下载 `mma-shell.exe`（Windows）/ `mma-shell`
   （Linux）。
2. **放在插件文件夹旁边**：插件文件夹名为 `ManiaMapAnalyser by Leo_Black`
   （tosu 的 static 目录下）。壳会自动向上找这个文件夹，不需要配置；
   放好后双击运行即可。

> 路径探测规则（一般不用管）：壳从 exe 所在位置向上找 3 层内的
> `ManiaMapAnalyser by Leo_Black` 文件夹；还会向上找 tosu 的 `tosu.env`
> 判断「在线模式」（tosu 开着→直接打开 tosu 插件页；没开→本地离线模式）。

## 二、第一次运行

双击 exe。窗口打开后：

- **tosu 在运行** → 窗口直接显示 tosu 插件页（和浏览器一样），Etterna/Malody
  桥照常可用；
- **tosu 没运行**（离线）→ 窗口加载本地页面，数据源只有 Etterna/Malody
  （按钮/设置里选）；本轮页面右上角圆点会显示当前数据源。

## 三、窗口怎么操作

| 操作 | 方法 |
| --- | --- |
| 改变窗口大小 | 拖动窗口边缘（无边框，四周可拖） |
| 页面缩放 | 按住 `Ctrl` 滚动鼠标滚轮；或 `Ctrl +` / `Ctrl -`；`Ctrl 0` 复位 |
| 开关置顶（默认置顶） | 点击窗口后按 `Ctrl + Shift + T` |
| 关闭窗口 | `Alt+F4` 或 `Ctrl+Q`（退出壳进程） |
| 拖动位置 | 按住窗口任意位置拖动（无边框窗口整窗可拖） |

> 置顶/透明在 Windows 与 Linux（合成器）可用；Windows 的「点击穿透」
> （鼠标穿过窗口点到下层）为尽力支持，若不可用窗口保持置顶形态。

## 四、安装桥（Etterna / Malody 数据源）

### Etterna（跟随选歌/难度/速率；游玩状态参与 Auto 路由）

1. 复制 `bridges/etterna/mma_bridge.lua` 到
   `Themes/<你的主题>/BGAnimations/ScreenSelectMusic decorations/`；
   在该目录 `default.lua` 的 `return t` 之前加一行：
   `t[#t + 1] = LoadActor("mma_bridge.lua")`
2. 复制 `bridges/etterna/mma_gameplay.lua` 到
   `Themes/<你的主题>/BGAnimations/ScreenGameplay overlay/`；同样在
   `default.lua` 的 `return t` 前加：
   `t[#t + 1] = LoadActor("mma_gameplay.lua")`
3. **主题更新后必须重装这两个文件**（主题包覆盖会删掉它们）。
4. 设置里填 `Etterna Folder`（壳找不对时才需要；通常是 `D:\Games\Etterna`）。

### Malody V（编辑器内分析一次触发；游玩皮肤显示最近结果）

1. 编辑场景：把 `bridges/malody/mma_editor.lua` 放到 `MalodyV/Editor/`
   （目录不存在就新建）。打开谱面编辑器 → 「更多」菜单 → **LeosMma Analyser** →
   卡片显示本谱分析。
2. 游玩场景（可选）：把 `bridges/malody/mma_skin.lua` 复制到你用的皮肤目录
   （`skin/<皮肤名>/`，文件名任意）；在皮肤 Composer 里添加一个 **Text 模块，
   命名 `mma_result`**；在该皮肤目录里新建空文件 `mma.txt`（壳靠它找到写入
   目标）。游玩/选歌界面该文字模块会显示壳最近一次分析结果。
3. 设置里填 `Malody V Folder`（通常是 `D:\Steam\steamapps\common\MalodyV`）。

## 五、设置（tosu 设置页 → Functional 组）

- **Game Client**：`Auto`（推荐）——按 游玩中 > 近期活动 > osu!>Etterna>Malody
  自动跟随；也可以强制锁定某一个游戏。
- **Etterna Folder / Malody V Folder**：壳自动探测失败时才填。

数据源指示：卡片右上状态行末尾的小圆点——蓝色=osu!、绿色=Etterna、
橙色=Malody、灰色空心=当前没有数据源。悬停可看说明。

## 六、常见问题

- **圆点灰空心 / 卡片不动**：确认对应游戏已开、桥已装、壳在运行。
- **Malody 编辑器点了没反应（30 秒后报超时）**：壳没在运行，或窗口还没加载
  完（先开壳，等几秒再点）。
- **窗口是黑/白的闪一下**：透明白闪为已知小抖动；不影响使用。
- **更新了插件文件夹但窗口显示旧版**：关掉壳重新打开（页面每次启动重新加载）。

# English

## What this is

mma-shell is an optional desktop window that shows the analysis card in an
always-on-top, borderless, transparent mini window, and lets the card follow
**Etterna** or **Malody V** even when tosu/osu! is not running (offline mode).
The classic browser usage (tosu plugin page) is unaffected.

## Install

1. Get `mma-shell.exe` (Windows) or `mma-shell` (Linux) from the release page
   or CI artifacts.
2. Place it **next to the plugin folder** `ManiaMapAnalyser by Leo_Black`
   (inside tosu's static folder). Detection is automatic: the shell walks up
   to 3 levels looking for that folder (with `index.html`), and looks for
   `tosu.env` to decide online vs offline mode.

## Window controls

| Action | How |
| --- | --- |
| Resize | drag any window edge |
| Zoom | `Ctrl` + mouse wheel, or `Ctrl +` / `Ctrl -`; `Ctrl 0` resets |
| Always-on-top toggle (default on) | click the window, then `Ctrl + Shift + T` |
| Close | `Alt+F4` or `Ctrl+Q` |
| Move | drag anywhere on the window |

Click-through on Windows is best-effort; when unavailable the window stays
always-on-top.

## Bridges

- **Etterna**: copy `bridges/etterna/mma_bridge.lua` into
  `Themes/<theme>/BGAnimations/ScreenSelectMusic decorations/` and add
  `t[#t + 1] = LoadActor("mma_bridge.lua")` before `return t` in its
  `default.lua`; do the same with `mma_gameplay.lua` in the
  `ScreenGameplay overlay/` folder. Re-install after every theme update.
- **Malody**: editor — put `bridges/malody/mma_editor.lua` into
  `MalodyV/Editor/` (create it), then trigger via the More menu in the editor.
  Optional in-game display — copy `bridges/malody/mma_skin.lua` into your skin
  folder, add a Text module named `mma_result` in the skin Composer, and create
  an empty `mma.txt` sentinel in that skin folder.

## Settings

Functional group: **Game Client** (Auto recommended; or lock to one game),
**Etterna Folder** / **Malody V Folder** (only when auto-detection fails).

The status-row dot at the top right of the card: blue = osu!, green = Etterna,
orange = Malody, hollow grey = no source.

## Troubleshooting

Hollow grey dot / frozen card: game bridge not installed, game closed, or shell
not running. Malody editor timing out: restart the shell and wait a few seconds
before triggering. White flash on open is a known cosmetic quirk.