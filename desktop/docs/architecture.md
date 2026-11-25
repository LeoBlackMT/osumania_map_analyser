# Desktop (mma-shell) 架构与目录规范指引

本文档阐述 `desktop/` 目录（Tauri v2 + Rust 壳工程 `mma-shell`）的整体架构、工程拓扑、子模块职责划分与编码规约。壳-页面协议以 [CONTRACT.md](CONTRACT.md) 为唯一权威；面向功能实现的完整说明见 [docs/features/desktop-shell.md](../../docs/features/desktop-shell.md)。

---

## 1. 整体拓扑与设计哲学

`mma-shell` 作为 ManiaMapAnalyser 的跨游戏本地聚合桥接外壳，承载以下核心职责：

1. **本地多源数据采集**：只读读取 osu!（stable x86 与 lazer x64）进程内存、Malody 4.3.7 进程内存/日志/数据库，接收 Malody V BepInEx 选曲桥推送，轮询 Etterna 桥文件，并探测 tosu 运行环境。
2. **本地网络协议适配**：提供 24060（Malody 编辑器 POST 桥接）、24061（静态文件、设置/壳配置/偏移表/反馈等本机 HTTP 端点与 `/ws` 帧循环）、24062（壳内 osu 原生读取器的 tosu 兼容子集 origin）、17653（Malody V 选曲桥）。
3. **窗口与桌面宿主**：基于 Tauri v2 承载 WebView2 覆盖层窗口，支持跨窗口置顶、无缝拖拽、点击穿透热键、无边框透明渲染与独立设置窗口。
4. **纯数据驱动偏移体系**：解耦所有硬编码结构位移，通过外置纯数据表（`offsets/`）与活体验证器保障对上游游戏更新的快速适配。
5. **本地数据加工**：把读取线程的逐判定事件与原始按键帧整理成 LaneAcc 数据面，解析 lazer `.osr` 的暂停信息，并在壳内完成 RC 难度反馈的客观字段重算与 Ed25519 签名（壳不直接访问外网，由页面转发签名信封）。

---

## 2. 模块拓扑与职责划分

```text
desktop/src/
├── main.rs                        # 极简应用启动入口：便携 profile → 单实例转交 → app::run()
├── lib.rs                         # 核心导出与库入口
├── frames.rs                      # 壳-页面桥帧定义与端口/时限常量 (CONTRACT_VERSION = 13)
├── etterna.rs                     # Etterna 桥轮询 (2Hz 读 MmaBridge.txt / MmaGameplay.txt)
├── malodyv.rs                     # Malody V 编辑器文件通道轮询
├── settings_window.rs             # 设置窗口 (壳内第二个 OS 窗口，承载 settings.html)
│
├── app/                           # 【UI 表现层】Tauri v2 窗口、托盘与快捷键生命周期
│   ├── builder.rs                 # Tauri AppBuilder 构建与生命周期挂钩
│   ├── instance.rs                # 单例互斥锁检测与已有实例前台激活
│   ├── kde_overlay.rs             # KDE Wayland/XWayland 窗口置顶、全屏层级与规则同步
│   ├── shortcuts.rs               # 全局热键解析与绑定 (置顶、穿透、设置、退出)
│   ├── window_state.rs            # 窗口物理尺寸、位置、穿透与置顶持久化与动态应用
│   ├── window_events.rs           # 主窗口与设置窗口物理事件监听分流
│   ├── profile.rs                 # 便携式 WebView2 用户数据目录初始化
│   ├── url.rs                     # startup_url 决策：缺省落 24061，osuTransport:"tosu" 才回 tosu 页
│   └── mod.rs                     # 模块汇聚与 run() 启动函数
│
├── common/                        # 【通用基础设施】纯标准库/高频通用组件
│   ├── throttle.rs                # ChangeDedupe<T>、LogGate、TimedThrottle 节流去重
│   └── mod.rs                     # 公共工具导出
│
├── config/                        # 【配置与环境探测】
│   ├── common.rs                  # 路径规范化、文件 mtime、可执行文件目录
│   ├── tosu.rs                    # tosu 运行环境探测、plugin_dir、tosu 设置解析
│   ├── json.rs                    # shell_config.json、settings.json 读写与安全合并
│   ├── detect.rs                  # 驱动器就绪判断、Steam 库枚举、Etterna/Malody 根目录探测
│   ├── window.rs                  # 窗口物理状态持久化读写
│   └── mod.rs                     # 导出宏与门面
├── config.rs                      # [门面] 保持原有 use crate::config::* 绝对兼容
│
├── feedback/                      # 【RC 难度反馈】
│   └── mod.rs                     # native 快照客观字段重算 + Ed25519 签名 (/feedback 的本机实现)
│
├── malody4/                       # 【Malody 4 内存与分析引擎】
│   ├── spec.rs                    # [纯标准库] 规格常量、已知版本表、PE 静态校验与文法解析
│   ├── anchor.rs                  # 版本表、身份键解析、PE 版本校验与 Win32 读取层
│   ├── config.rs                  # config.json 只读解析 (user_mods 变速位 / user_judge_level 判定档)
│   ├── gamelog.rs                 # 游戏日志场景行解析 (`switch to N` / `switchback`) 与日志文件选择器
│   ├── library.rs                 # 谱面本地索引构建与 SQLite 数据库查询
│   ├── model.rs                   # 线上记录模型：场景、mod 速率、判定档、selection 记录与不可用原因
│   ├── selection.rs               # selection 状态机 (纯逻辑，不持有任何阈值)
│   ├── settings.rs                # 从 mod.rs 拆出的设置项 (同语义，父模块再导出)
│   ├── runtime.rs                 # 后台轮询主循环、索引构建线程、进程附着与 selection/song 帧分发
│   └── mod.rs                     # 模块聚合与公共入口
│
├── osu/                           # 【osu! 原生内存与对拍引擎】
│   ├── mod.rs                     # ReaderState、Phase、Reader 状态句柄与外部查询
│   ├── runner.rs                  # run() 后台主循环、L0 定址、帧采样与状态发布
│   ├── lazer.rs                   # osu!lazer 64 位读取全套接口门面
│   ├── lazer/                     # --- lazer 子模块划分 ---
│   │   ├── fields.rs              # 结构常量、字段名、FieldLookup、降级标记
│   │   ├── source.rs              # Source trait 内存抽象与 64 位小端基元读取
│   │   ├── types.rs               # ChainAddrs、SessionProof、Resolved、Diagnostics、LazerFrame
│   │   ├── screen.rs              # EEType 运行时解析与屏幕状态映射
│   │   ├── resolve.rs             # 站点到 GameBase 多跳解析与 L1 结构证明
│   │   ├── linux.rs               # Linux x86_64 `/proc/<pid>/mem` osu!lazer LaneAcc 只读读取器
│   │   ├── table.rs               # 偏移表查找梯子与加载逻辑
│   │   ├── results.rs             # 结算屏判定数链路（ResultsScreen → ScoreInfo）
│   │   ├── mods.rs                # 模组自定义设置：mods 段读取与 ScoreInfo.ModsJson 探针
│   │   ├── score.rs               # ScoreProcessor.hitEvents、原始 ReplayFrame 与 replay hash 读取
│   │   ├── osr.rs                 # lazer .osr 尾部 scoreInfo 只读解析 (pauses / mods rate)
│   │   └── frame.rs               # 逐帧读取、解引用链、L0 attach 与 read_tick
│   ├── offsets.rs                 # 偏移表统一门面
│   ├── offsets/                   # --- 偏移表子模块划分 ---
│   │   ├── lookup.rs              # FieldLookup、LookupError、canonical_type
│   │   ├── lazer_table.rs         # OffsetTable、RuntimeSection、就近回落
│   │   ├── stable_table.rs        # StableTable、StableTopology、本地梯子发现
│   │   ├── validation.rs          # 纯数据 Schema 安全与合理性校验器
│   │   └── remote.rs              # Ed25519 签名验证、清单解析与原子落盘
│   ├── invariants.rs              # L1/L2 不变量、健康状态机与字段级冻结（L0 在 mod.rs，L3 影子在 shadow.rs）
│   ├── stable.rs                  # 32 位 stable 内存读取、多跳链解析、hits 抽取
│   ├── anchor_cache.rs            # 锚点特征码缓存池与复核
│   ├── beatmap_file.rs            # .osu 谱面解析器
│   ├── compare.rs                 # tosu live 影子对拍装置与 JSONL 日志输出
│   ├── discovery.rs               # 进程发现与位数分派
│   ├── keys.rs                    # 谱面特征与 mod 签名派生
│   ├── model.rs                   # Client、Reason、Snapshot 基础数据类型
│   ├── packet.rs                  # 载荷生成器与保持帧构造
│   ├── patterns.rs                # stable 锚点台账与掩码模式
│   ├── scan.rs                    # 内存区域扫描与掩码搜索
│   ├── shadow.rs                  # 影子比对逻辑
│   └── win.rs                     # osu 专用 Win32 直接 FFI 与 RAII 目标进程（与 sys 的通用抽象并存）
│
├── server/                        # 【本地网络与 IPC 服务层】
│   ├── state.rs                   # 子系统状态封装 (BroadcastHub, SettingsHub, SourcesRegistry, WindowBridge)
│   ├── bridge.rs                  # 17653 Malody V BepInEx 选曲桥门面与统一重导出
│   ├── bridge/                    # --- 桥接子模块划分 ---
│   │   ├── state.rs               # MalodyBridgeState、去重基线与事件分类
│   │   ├── settings.rs            # 判定档、Pro/Turbo 开关与 winScale 计算与交叉校验
│   │   ├── chart.rs               # 谱面路径沙箱校验、元数据与键数派生、song 帧生成
│   │   └── http.rs                # 17653 HTTP 监听、POST /selection 门禁流转与 WS 补发
│   ├── ws.rs                      # 24061 /ws 帧循环 (hello/state/song/settings 出站 + diag/control/result/laneacc_sync/replay_pauses_sync 入站)
│   ├── http.rs                    # 24061 单 listener：静态、/settings、/shell-config、/open-settings、
│   │                              #   /show-main、/feedback、/cover、/offsets/* 与 WS 分发
│   ├── post.rs                    # 24060 POST：Malody 编辑器分析入口 + resolve 通道
│   ├── osu_compat.rs              # 24062 tosu 兼容子集 origin (/websocket/v2 + 两条文件路由)
│   ├── osu_source.rs              # sources.osu 组装：读取器健康位 + 壳配置 + 24062 绑定结果
│   ├── hit_events.rs              # 逐判定事件 100ms 合并 → mania_hit_events 帧
│   ├── replay_frames.rs           # 原始按键帧 100ms 合并 → mania_replay_frames 帧
│   ├── replay_pauses.rs           # .osr 暂停解析 → replay_pauses / replay_pauses_sync
│   ├── log.rs                     # 环形内存日志缓冲与按日滚动落盘
│   └── mod.rs                     # 顶层服务聚合与生命周期
│
└── sys/                           # 【操作系统底层只读访问】通用平台抽象（独立保留；osu/malody4 仍各自使用本地 Win32 实现）
    ├── mod.rs                     # 平台抽象导出
    └── windows/                   # Windows 平台只读系统访问
        ├── ffi.rs                 # 严格受限的只读 Win32 API 声明与类型
        ├── handle.rs              # RAII 进程与系统句柄管理
        ├── memory.rs              # 带缩小重试机制的安全内存读取器
        ├── process.rs             # 进程枚举、Toolhelp 快照与模块基址探测
        └── mod.rs                 # 模块导出
```

---

## 3. 核心设计模式与约定

### 3.1 门面模式 (Facade Pattern)

为保障各个历史模块引用的兼容性与最小心智负担，所有完成拆分的子领域均保留了顶层门面：

- `crate::config::*` 继续由 `desktop/src/config.rs` 转发至 `desktop/src/config/`；
- `crate::osu::offsets::*` 继续由 `desktop/src/osu/offsets.rs` 转发至 `desktop/src/osu/offsets/`；
- `crate::osu::lazer::*` 继续由 `desktop/src/osu/lazer.rs` 转发至 `desktop/src/osu/lazer/`；
- `crate::server::bridge::*` 继续由 `desktop/src/server/bridge.rs` 转发至 `desktop/src/server/bridge/`。

新增业务功能时，应直接在对应的子模块中定义并就近编写单元测试，仅在确实需要对外广泛公开时再提升至门面。

### 3.2 外部通信协议保持严格兼容

前端页面与桌面壳的 WebSocket 通信受 [CONTRACT.md](CONTRACT.md) **v13** 协议保护；页面侧 `bridgeClient.js` 的 `CONTRACT_VERSION`、`desktop/src/frames.rs` 的 `CONTRACT_VERSION` 与 `CONTRACT.md` 必须同号，任一帧字段或语义变更都要三处同步自增。页面接受区间为 `[3,13]`，越界进入终态并停止重连。

| 通道 | 用途 |
| --- | --- |
| **24060 POST** | Malody 编辑器分析入口与 resolve 通道。 |
| **24061 HTTP** | 页面静态资源托管、`/settings`、`/shell-config`、`/open-settings`、`/show-main`、`/feedback`、`/cover/*`、`/offsets/*` 等本机端点；这些 HTTP 端点不属于帧契约，自身不升契约版本。 |
| **24061 /ws** | 帧循环：`hello` / `state` / `song` / `settings` / `result` / `control` / `ping`，以及 LaneAcc 与回放数据面：`mania_hit_events`（v9）、`mania_replay_frames`（v10）、`replay_pauses` / `replay_pauses_sync`（v11）。v12 追加 stable/ppysb 的 `play.seed` / `resultsScreen.seed` 字段；v13 追加 `control.resizeStart`（`direction`）与 `sources.osu.lazerLaneAccReady`。 |
| **24062** | 壳内 osu 原生读取器的 tosu 兼容子集 origin（`/websocket/v2` + `/files/beatmap/*`），仅在 `sources.osu.mode == "native"` 时被页面使用。 |
| **17653 POST** | Malody V BepInEx 选曲桥（`POST /selection`）。 |

### 3.3 只读边界

`osu/**`、`malody4/**`、`sys/**` 与 `.osr` 解析都属于只读观察面：只允许 `PROCESS_VM_READ | PROCESS_QUERY_INFORMATION` 等读权限，不注入、不写游戏目录、不修改数据库、不挂起目标进程。反馈签名同样只在壳内完成，壳不直接发起 HTTPS；签名信封由页面转发到唯一后端。

---

## 4. 后续开发与扩展规范

1. **就近原则**：添加新特性或辅助逻辑时，必须就近放置在相关领域子目录下，严禁重回单文件千行以上的大膨胀模式。
2. **测试伴随**：纯逻辑函数（如版本号解析、配置合并、特征掩码比对）需随文件就地编写 `#[cfg(test)]` 单测，确保 `cargo test --lib` 能够秒级覆盖。
3. **平台安全**：Windows 特有 API 必须使用 `#[cfg(windows)]` 条件编译，并在对应位置提供 `#[cfg(not(windows))]` 安全空实现或回落，确保跨平台编译（如 Linux CI）不被破坏。
4. **协议与文档同步**：修改帧字段、端点或只读边界时，同步更新 `desktop/docs/CONTRACT.md`、页面 `bridgeClient.js` 与 [docs/features/desktop-shell.md](../../docs/features/desktop-shell.md)；功能与技术细节不要在本文件重复维护。
