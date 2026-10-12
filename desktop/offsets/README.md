# 内存偏移表与生成工具说明

本目录包含桌面壳（mma-shell）读取 osu! 游戏状态所需的偏移表及生成工具。

## 目录结构

```text
offsets/
├── manifest.json       # 偏移表索引文件
├── gen.exe             # osu!lazer 偏移表生成工具
├── stable/             # osu!stable 偏移表（含官方客户端与 ppy.sb 客户端）
└── lazer/              # osu!lazer 各版本偏移表
```

## 使用说明

### 1. 常规使用
发行包内置的偏移表已经适配主流版本的 osu!stable 与 osu!lazer。启动 `mma-shell.exe` 后，程序会自动匹配对应的游戏版本，通常不需要进行额外配置。

### 2. osu!lazer 更新后的处理
当 osu!lazer 更新后，若现有偏移表无法匹配当前游戏版本，可以通过以下方式生成或获取新的偏移表：

方式一：通过设置面板生成
1. 保持 osu!lazer 运行在主菜单或选歌界面。
2. 按 `Ctrl+Shift+S` 打开桌面壳设置面板，进入 Memory Offsets Management。
3. 点击 Live Generator 按钮，程序将扫描当前运行的 lazer 进程并生成偏移表。

方式二：运行 `gen.exe`
1. 保持 osu!lazer 运行在主菜单或选歌界面。
2. 运行本目录下的 `gen.exe`。
3. 在控制台菜单中输入 `1` 并回车执行扫描。生成器会自动将新表写入 `offsets/lazer/` 及系统的配置目录。
4. 生成完成后按回车退出，重启桌面壳或切换谱面即可生效。控制台中输入 `3` 可切换中英文界面。

方式三：检查远端更新
在桌面壳设置面板的 Memory Offsets Management 中，点击 Check Remote Updates，可检查并同步官方更新的偏移表。

---

# Memory Offset Tables & Generator Documentation

This directory contains offset tables and tools required by the desktop shell (mma-shell) to read osu! game states.

## Directory Layout

```text
offsets/
├── manifest.json       # Offset tables manifest index
├── gen.exe             # Offset table generator for osu!lazer
├── stable/             # osu!stable offset tables (official client and ppy.sb client)
└── lazer/              # osu!lazer offset tables by version
```

## Usage

### 1. General Usage
The release package includes offset tables matching current versions of osu!stable and osu!lazer. When `mma-shell.exe` starts, it automatically selects the appropriate table for the running game client. No manual configuration is needed under normal circumstances.

### 2. Handling osu!lazer Updates
When osu!lazer updates and existing tables cannot match the new build, you can acquire or generate a new table using one of the following methods:

Method 1: Generate from the Settings Window
1. Keep osu!lazer running on the main menu or song selection screen.
2. Press `Ctrl+Shift+S` to open the shell settings window, then go to Memory Offsets Management.
3. Click Live Generator. The shell scans the running lazer process and generates the offset table.

Method 2: Run `gen.exe`
1. Keep osu!lazer running on the main menu or song selection screen.
2. Launch `gen.exe` in this directory.
3. Type `1` in the console menu and press Enter to start scanning. The tool writes the new table to `offsets/lazer/` and the system appdata folder.
4. Press Enter to exit after generation. Restart the shell or change songs to apply. Type `3` in the console menu to toggle between English and Chinese.

Method 3: Check for Remote Updates
In the shell settings window under Memory Offsets Management, click Check Remote Updates to check for and download official updated tables.
