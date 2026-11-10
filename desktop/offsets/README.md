# 内存偏移表说明

本目录包含桌面壳（`mma-shell`）直接读取 osu! 游戏数据所需的偏移配置文件及辅助生成工具。

> **提示**：发行包已内置适配主流版本的配置文件。启动 `mma-shell.exe` 后会自动识别并匹配游戏版本，**日常游玩通常不需要进行任何手动配置**。

## 目录结构

```text
offsets/
├── manifest.json       # 偏移表索引清单
├── gen.exe             # osu!lazer 偏移表生成工具
├── stable/             # osu!stable 适配文件
└── lazer/              # osu!lazer 各版本适配文件
```

> stable/ppysb__x86.json 用于 ppy.sb 注入版的基础内存读取；LaneAcc 不支持 ppy.sb，会显示占位提示。

> 维护者：stable 表新增两个**可选**段——`topology.replay`（`Score+0x34` 原始按键帧）与
> `topology.notes`（HOM per-note 判定，列/档位）。`tools/stable-offsets-gen` 重新生成时会
> 保留这两段；换 `osu!.exe` 构建后必须 live 自证再更新，缺段时 Stable 分轨显示占位。

## 当游戏更新后无法读取数据时

由于 osu!lazer 会不定期更新，如果更新后桌面壳无法读取谱面或状态，请按以下方式获取最新适配：

### 方式一：在设置中检查更新（推荐，最简单）
1. 按 `Ctrl+Shift+S` 打开桌面壳设置面板。
2. 进入 **Memory Offsets Management**（内存偏移表管理）。
3. 点击 **Check Remote Updates**（检查远端更新），程序会自动拉取并更新官方适配表。

### 方式二：在设置中自动生成
1. 启动 osu!lazer 并停留在选歌界面或主菜单。
2. 按 `Ctrl+Shift+S` 打开桌面壳设置面板，进入 **Memory Offsets Management**。
3. 点击 **Live Generator** 按钮，程序将自动扫描当前运行的 lazer 并生成新配置。

### 方式三：运行独立生成工具
1. 启动 osu!lazer 并停留在选歌界面或主菜单。
2. 双击运行本目录下的 `gen.exe`（如遇管理员权限提示请允许）。
3. 在控制台菜单中输入 `1` 并按回车执行扫描。工具将自动生成新配置并写入对应目录。
4. 生成完成后按回车退出，重启桌面壳即可生效。（控制台中输入 `3` 可切换语言）。

---

# Memory Offset Tables Guide

This directory contains configuration files and helper tools used by the desktop shell (`mma-shell`) to read osu! game data directly from memory.

> **Note**: Bundled offset tables already support common versions of osu!stable and osu!lazer. When `mma-shell.exe` starts, it automatically matches the game client. **No manual configuration is required under normal use.**

## Directory Layout

```text
offsets/
├── manifest.json       # Manifest index for offset tables
├── gen.exe             # Offset table generator for osu!lazer
├── stable/             # Offset tables for osu!stable
└── lazer/              # Offset tables for osu!lazer releases
```

## Troubleshooting After Game Updates

When osu!lazer updates and the shell can no longer read game data, use one of the following methods:

### Method 1: Check Remote Updates (Recommended)
1. Press `Ctrl+Shift+S` to open the shell settings panel.
2. Go to **Memory Offsets Management**.
3. Click **Check Remote Updates** to download the latest official offset tables.

### Method 2: Live Generator in Settings
1. Keep osu!lazer running on the song select screen or main menu.
2. Press `Ctrl+Shift+S` to open shell settings and go to **Memory Offsets Management**.
3. Click **Live Generator**; the shell will scan the running lazer process and generate the table.

### Method 3: Run the Standalone Generator Tool
1. Keep osu!lazer running on the song select screen or main menu.
2. Launch `gen.exe` in this directory (approve administrator prompt if requested).
3. Type `1` in the console menu and press Enter to scan. The tool generates and installs the new table automatically.
4. Press Enter to exit once finished, then restart the desktop shell. (Type `3` in the menu to switch languages).
