# 桌面壳使用教程 / Desktop Shell Guide

> 面向人类用户。技术说明见 [docs/features/desktop-shell.md](features/desktop-shell.md)。This is the human-facing tutorial; technical details live in [docs/features/desktop-shell.md](features/desktop-shell.md).

# 中文

## 这是什么

桌面壳（mma-shell）是一个可选的小窗口程序，用来：把分析卡片显示在**独立的置顶小窗口**里（可以盖在游戏/浏览器上）；在**不启动 tosu** 的情况下，让卡片跟随 **Etterna** 或 **Malody V**（无需 osu! / tosu 也在线）。浏览器旧用法（tosu 插件页）不受影响，可以继续用。

系统要求：Windows（推荐）或带合成器的 Linux；Etterna/Malody 数据源需要对应游戏的桥文件（见下文「安装桥」）。最低游戏版本：**Etterna 0.70+**（桥使用的 RageFileUtil/LoadActor 等 API 长期稳定，更低版本大概率可用但未验证）；**Malody V 6.6.43+**（编辑器插件的 `Editor:DoRequest` 自该版本起可用，更低版本只能走文件输入兜底）。

## 一、获取与安装

1. 从发布页（或 CI artifact）下载 `mma-shell.exe`（Windows）/ `mma-shell`（Linux）。
2. **放在插件文件夹旁边**：插件文件夹名为 `ManiaMapAnalyser by Leo_Black`（tosu 的 static 目录下）。壳会自动向上找这个文件夹，不需要配置；放好后双击运行即可。

> 路径探测规则（一般不用管）：壳从 exe 所在位置向上找 3 层内的 `ManiaMapAnalyser by Leo_Black` 文件夹；还会向上找 tosu 的 `tosu.env` 判断「在线模式」（tosu 开着→直接打开 tosu 插件页；没开→本地离线模式）。

## 二、第一次运行

双击 exe。窗口打开后：**tosu 在运行** → 窗口直接显示 tosu 插件页（和浏览器一样），Etterna/Malody 桥照常可用；**tosu 没运行**（离线）→ 窗口加载本地页面，数据源只有 Etterna/Malody（按钮/设置里选）；状态行右侧圆点显示当前数据源。

## 三、窗口怎么操作

| 操作 | 方法 |
| --- | --- |
| 拖动整个窗口 | 按住窗口**顶部拖动条**（顶端发光细条，中间有 `⋮⋮` 标志）拖动 |
| 改变窗口大小 | 拖动窗口边缘（无边框，四周可拖） |
| 页面缩放 | 按住 `Ctrl` 滚动鼠标滚轮；或 `Ctrl +` / `Ctrl -`；`Ctrl 0` 复位 |
| 开关置顶（默认置顶） | 任意时刻按 `Ctrl + Shift + T`（全局快捷键，不点窗口也有效） |
| 开关点击穿透（默认关） | 任意时刻按 `Ctrl + Shift + C`；穿透时鼠标可穿过窗口操作下层，再按一次切回 |
| 关闭窗口 | `Alt+F4` 或 `Ctrl+Q`（全局快捷键） |

> 窗口位置/大小/置顶/穿透状态会自动记忆，下次启动恢复。全局快捷键在窗口失焦或点击穿透时依然生效（穿透时没有鼠标事件，但快捷键仍在）。置顶/透明在 Windows 与 Linux（合成器）可用。

## 四、安装桥（Etterna / Malody 数据源）

### Etterna（跟随选歌/难度/速率；游玩状态参与 Auto 路由）

1. 复制 `bridges/etterna/mma_bridge.lua` 到 `Themes/<你的主题>/BGAnimations/ScreenSelectMusic decorations/`；在该目录 `default.lua` 的 `return t` 之前加一行：`t[#t + 1] = LoadActor("mma_bridge.lua")`
2. 复制 `bridges/etterna/mma_gameplay.lua` 到 `Themes/<你的主题>/BGAnimations/ScreenGameplay overlay/`；同样在 `default.lua` 的 `return t` 前加：`t[#t + 1] = LoadActor("mma_gameplay.lua")`
3. **主题更新后必须重装这两个文件**（主题包覆盖会删掉它们）。
4. 设置里填 `Etterna Folder`（壳找不对时才需要；通常是 `D:\Games\Etterna`）。

### Malody V（编辑器内分析一次触发；游玩皮肤显示最近结果）

1. 编辑场景：把 `bridges/malody/mma_editor.lua` 放到 `MalodyV/Editor/`（目录不存在就新建）。打开谱面编辑器 → 「更多」菜单 → **MMA Analyze** → 卡片显示本谱分析。
2. 游玩场景（独立皮肤，推荐）：在 `skin/` 下新建**独立皮肤目录**（如 `skin/MMA-Result/`），把 `bridges/malody/z_mma_skin.lua` 复制进去（文件名可任意）；在皮肤 Composer 里添加一个 **Text 模块，命名 `mma_result`**；在该皮肤目录里新建空文件 `mma.txt`（壳靠它找到写入目标）。游玩时选择 **MMA-Result** 作为皮肤（Base 皮肤不变）。**不要放进已有皮肤目录**（`UpdateSharedData` 等全局钩子会互相覆盖）。
3. 设置里填 `Malody V Folder`（通常是 `D:\Steam\steamapps\common\MalodyV`）。

## 五、配置（exe 旁：mma-shell-config.json + mma-settings.json）

壳有两份独立配置，都在 **exe 旁**（首次启动自动生成骨架），与 tosu 设置无关（tosu 侧只服务 osu! 来源）：

**① 壳配置 `mma-shell-config.json`**（仅壳使用）：源路径 + 快捷键 + 日志级别。

```json
{
  "gameClient": "Auto",
  "etternaRoot": "",
  "malodyRoot": "",
  "hotkeys": { "topmost": "Ctrl+Shift+T", "clickThrough": "Ctrl+Shift+C", "close": "Ctrl+Q" },
  "logLevel": "info"
}
```

- **gameClient**：`Auto`（推荐，按游玩中 > 近期活动 > osu!>Etterna>Malody 自动跟随）或锁定某来源。
- **etternaRoot / malodyRoot**：游戏安装路径。**路径可写正斜杠或双反斜杠**（如 `D:/Games/Etterna` 或 `D:\\Games\\Etterna`——单反斜杠 `D:\Games` 在 JSON 里是非法转义，请用 `/` 或 `\\`）；壳也会按常见位置自动探测，找不到才需手填。
- **hotkeys**：窗口快捷键（可选）。默认 `Ctrl+Shift+T` 置顶 / `Ctrl+Shift+C` 穿透 / `Ctrl+Q` 关闭；若与系统冲突可改（支持 Ctrl/Shift/Alt/Win + A–Z 单键）。改动后重启壳生效。
- **logLevel**：`debug` / `info` / `warn` / `error` / `off`（默认 info）。

**② 全量插件设置 `mma-settings.json`**（离线模式的卡片设置；无 tosu 用户手动编辑，重启后生效）：

- **tosu 设置文件可用**（tosu 安装目录的 `settings/ManiaMapAnalyser by Leo_Black.json`）时，壳**优先使用它**（在线只读 / 离线读文件），不生成也不使用 mma-settings.json。
- **找不到 tosu 设置文件**时进入本地模式：存在 `mma-settings.json` 则直接使用；不存在则由壳按插件的 `settings.json` 生成默认骨架（全部条目），用户手动编辑后重启壳生效。
- 设置键与 tosu 设置界面完全一致（估计算法、内容栏、显示等全部条目）；只改 `gameClient/etternaRoot/malodyRoot` 的用户**不需要碰它**（这三个在 mma-shell-config.json）。

配置填错/JSON 损坏不会崩溃——自动回落默认并警告。数据源指示：卡片右上状态行末尾的小圆点——**蓝色=osu!、绿色=Etterna、橙色=Malody、灰色空心=当前没有数据源**。悬停可看说明。

## 六、日志

壳运行日志写在 **exe 旁的 `mma-shell-YYYYMMDD.log`**（按日轮转，保留 7 天），每行带时间戳与级别。排查问题（来源没反应、分析失败）时把最新日志内容发给我们即可。`logLevel` 设为 `debug` 会输出更详细诊断。

## 六、常见问题

- **圆点灰空心 / 卡片不动**：确认对应游戏已开、桥已装、壳在运行。
- **Malody 编辑器点了没反应（30 秒后报超时）**：壳没在运行，或窗口还没加载完（先开壳，等几秒再点）。
- **窗口是黑/白的闪一下**：透明白闪为已知小抖动；不影响使用。
- **更新了插件文件夹但窗口显示旧版**：关掉壳重新打开（页面每次启动重新加载）。

# English

## What this is

mma-shell is an optional desktop window that shows the analysis card in an always-on-top, borderless, transparent mini window, and lets the card follow **Etterna** or **Malody V** even when tosu/osu! is not running (offline mode). The classic browser usage (tosu plugin page) is unaffected.

## Install

1. Get `mma-shell.exe` (Windows) or `mma-shell` (Linux) from the release page or CI artifacts.
2. Place it **next to the plugin folder** `ManiaMapAnalyser by Leo_Black` (inside tosu's static folder). Detection is automatic: the shell walks up to 3 levels looking for that folder (with `index.html`), and looks for `tosu.env` to decide online vs offline mode.

## Window controls

| Action | How |
| --- | --- |
| Move the whole window | drag the **top drag bar** (glow strip with `⋮⋮` hint at the window top) |
| Resize | drag any window edge |
| Zoom | `Ctrl` + mouse wheel, or `Ctrl +` / `Ctrl -`; `Ctrl 0` resets |
| Always-on-top toggle (default on) | `Ctrl + Shift + T` (global shortcut, works even when the window is unfocused) |
| Click-through toggle (default off) | `Ctrl + Shift + C`; while enabled the mouse passes through to windows below, press again to restore |
| Close | `Alt+F4` or `Ctrl+Q` (global) |

Window position/size/topmost/click-through are remembered across launches. Global shortcuts keep working while the window is unfocused or click-through is active. Always-on-top/transparency work on Windows and Linux (compositor).

## Bridges

- **Etterna**: copy `bridges/etterna/mma_bridge.lua` into `Themes/<theme>/BGAnimations/ScreenSelectMusic decorations/` and add `t[#t + 1] = LoadActor("mma_bridge.lua")` before `return t` in its `default.lua`; do the same with `mma_gameplay.lua` in the `ScreenGameplay overlay/` folder. Re-install after every theme update.
- **Malody**: editor — put `bridges/malody/mma_editor.lua` into `MalodyV/Editor/` (create it), then trigger via the More menu in the editor (auto-reads the chart via the shell's resolve scan). Optional in-game display — create a **standalone skin dir** (e.g. `skin/MMA-Result/`), copy `bridges/malody/z_mma_skin.lua` there, add a Text module named `mma_result` in the skin Composer, create an empty `mma.txt` sentinel, and select that skin when playing (do not drop it into an existing skin — global hooks like `UpdateSharedData` would conflict).

## Settings

The shell has two independent config files, both **next to the exe** (auto-created on first run), independent of tosu settings (the tosu side only serves the osu! source):

**① Shell config `mma-shell-config.json`** (shell-only): source paths + shortcuts + log level.

```json
{
  "gameClient": "Auto",
  "etternaRoot": "",
  "malodyRoot": "",
  "hotkeys": { "topmost": "Ctrl+Shift+T", "clickThrough": "Ctrl+Shift+C", "close": "Ctrl+Q" },
  "logLevel": "info"
}
```

- `gameClient`: `Auto` (recommended; play state > recent activity > osu!>Etterna>Malody) or a locked source.
- `etternaRoot` / `malodyRoot`: install paths. Use forward slashes or double backslashes (`D:/Games/Etterna` or `D:\\Games\\Etterna` — a single `\` is invalid JSON escaping); auto-detection covers common locations, fill only when detection fails.
- `hotkeys`: optional window shortcuts; defaults `Ctrl+Shift+T` topmost / `Ctrl+Shift+C` click-through / `Ctrl+Q` close. Change if they conflict with your system (Ctrl/Shift/Alt/Win + single letter). Restart the shell after editing.
- `logLevel`: `debug` / `info` / `warn` / `error` / `off` (default `info`).

**② Full plugin settings `mma-settings.json`** (offline-mode card settings; no-tosu users edit it manually, effective after restart):

- When the **tosu settings file** (`settings/ManiaMapAnalyser by Leo_Black.json` inside the tosu install) is available, the shell **prefers it** (read-only online / read file offline) and never creates or uses `mma-settings.json`.
- Without a tosu settings file, local mode kicks in: existing `mma-settings.json` is used as-is; otherwise the shell generates a default skeleton from the plugin's `settings.json` (all entries), which you edit manually and restart the shell to apply.
- Keys match the tosu settings UI exactly (estimator, content bar, display, ...). Users who only need `gameClient/etternaRoot/malodyRoot` never touch this file (those live in `mma-shell-config.json`).

A malformed config falls back to defaults with a warning (never crashes). The status-row dot at the top right of the card: **blue = osu!, green = Etterna, orange = Malody, hollow grey = no source**; hover for details.

## Logs

Shell logs go to **`mma-shell-YYYYMMDD.log` next to the exe** (rotated daily, 7 kept), each line timestamped with a level. When something misbehaves (no source reaction, failed analysis), send us the latest log. Set `logLevel` to `debug` for more detail.

## Troubleshooting

Hollow grey dot / frozen card: game bridge not installed, game closed, or shell not running. Malody editor timing out: restart the shell and wait a few seconds before triggering. White flash on open is a known cosmetic quirk. Shortcuts not working: check `mma-shell-*.log` for `shortcut FAILED` (conflict) and adjust `hotkeys` in `mma-shell-config.json`.