# offsets — 内存偏移表与生成器 / Memory Offset Tables & Generator

本目录包含 ManiaMapAnalyser 原生内存读取所需的纯数据偏移表与生成工具。  
This directory contains pure-data offset tables and generation tools required for ManiaMapAnalyser native memory reading.

---

## 目录结构 / Directory Structure

```text
offsets/
├── README.md               # 本说明文档 / This document
├── manifest.json           # 偏移表索引与 Ed25519 签名清单 / Manifest & Ed25519 signatures
├── gen.exe                 # (可选/分发) osu!lazer 活体偏移生成器 / Live offset generator
├── lazer/                  # osu!lazer 偏移表目录 / osu!lazer offset tables
│   ├── 2026.921.0.0__10.0.12__x64.json
│   └── 2026.1005.0.0__10.0.12__x64.json
└── stable/                 # osu!stable 偏移表目录 / osu!stable offset tables
    └── stable__x86.json
```

---

## 使用指南 / Usage Guide

### 1. 日常游玩 / Daily Play
- **通常无需进行任何操作**：安装包内自带的偏移表已覆盖当前主流的 osu!stable 与 osu!lazer 版本。
- 启动 `mma-shell.exe` 后，壳程序会自动在此目录或用户缓存目录检索与当前游戏版本完全匹配的偏移表。

### 2. 当 osu!(lazer) 更新后 / When osu!(lazer) Updates
如果 osu!(lazer) 进行了新版本更新，且提示偏移表未匹配（`lazer-offsets-missing`），可通过以下任意一种方式一键生成新版本的偏移表：

#### 方式一：在壳管理界面一键生成（推荐）
1. 启动 osu!(lazer) 并停留在主菜单或选歌界面。
2. 打开 ManiaMapAnalyser 壳设置面板中的「偏移表管理 (Offsets Management)」。
3. 点击 **「扫描并生成偏移表 (Scan & Generate Offsets)」** 按钮，生成完成后刷新卡片即可生效。

#### 方式二：双击运行 `gen.exe`
1. **先启动 osu!(lazer)**：进入游戏并停留在主界面或选歌界面。
2. 双击运行本目录下的 **`gen.exe`**。
3. 控制台将弹出交互式菜单，输入数字 `1` 并按回车执行扫描。
4. 生成器会在数秒内完成内存扫描与活体自校验，并自动将新表保存至 `offsets/lazer/` 与 `%APPDATA%/ManiaMapAnalyser/offsets/lazer/`。
5. 按回车键退出，切换谱面或重启 `mma-shell.exe` 即刻生效。

*(注：可在交互菜单中按 `3` 随时切换中文 / 英文界面)*

---

## 原理与安全性保障 / Safety & Design Guarantees

1. **绝对安全只读 (Read-Only Safety)**
   - 生成器与壳程序仅申请 Windows 进程只读权限（`PROCESS_VM_READ | PROCESS_QUERY_INFORMATION`）。
   - **绝不**向游戏注入任何 DLL，**绝不**修改游戏内存或代码，绝不挂载调试器。
2. **零卡顿与零挂起 (Zero Thread Suspension)**
   - 区别于开发期采集所用的 `dotnet-dump`，`gen.exe` 在扫描时**完全不挂起**游戏线程，游戏全程平滑运行无卡顿。
3. **免 .NET SDK 依赖 (Zero SDK Footprint)**
   - 纯原生机器码（Rust 编写），无需用户安装 .NET SDK、Visual Studio 或任何开发运行库。
4. **活体自校验门禁 (Live Validation Gate)**
   - 提取的 GameBase、曲目 MD5Hash、界面 ScreenStack 及 VTable 必须通过活体结构自证，100% 验证成功才允许落盘，绝不产生损坏或错误的表。
