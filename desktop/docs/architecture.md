# Desktop (mma-shell) 架构与目录规范指引

本文档阐述 `desktop/` 目录（Tauri v2 + Rust 壳工程 `mma-shell`）的整体架构、工程拓扑、子模块职责划分与编码规约。

---

## 1. 整体拓扑与设计哲学

`mma-shell` 作为 ManiaMapAnalyser 的跨游戏本地聚合桥接外壳，承载了以下核心职责：
1. **本地多源数据采集**：直接读取 osu! (stable x86 与 lazer x64) 进程内存、Malody 4.3.7 进程内存与数据库、Malody V 桥接、以及 tosu 代理探测；
2. **本地网络协议适配**：提供 24060（POST 桥接）、24061（本地静态文件托管与配置服务）、24062（统一高频 WebSocket 广播通道）；
3. **窗口与桌面宿主**：基于 Tauri v2 承载 WebView2 覆盖层窗口，支持跨窗口置顶、无缝拖拽、点击穿透热键、无边框透明渲染；
4. **纯数据驱动偏移体系**：解耦所有硬编码结构位移，通过外置纯数据表（`offsets/`）与活体验证器保障对上游游戏更新的快速适配。

---

## 2. 模块拓扑与职责划分

```text
desktop/src/
├── main.rs                        # 极简应用启动入口 (委派 app::run())
├── lib.rs                         # 核心导出与库入口
│
├── app/                           # 【UI 表现层】Tauri v2 窗口、托盘与快捷键生命周期
│   ├── builder.rs                 # Tauri AppBuilder 构建与生命周期挂钩
│   ├── instance.rs                # 单例互斥锁检测与已有实例前台激活
│   ├── shortcuts.rs               # 全局热键解析与绑定 (置顶、穿透、设置)
│   ├── window_state.rs            # 窗口物理尺寸、位置、穿透与置顶持久化与动态应用
│   ├── window_events.rs           # 主窗口与设置窗口物理事件监听分流
│   ├── profile.rs                 # 便携式 WebView2 用户数据目录初始化
│   ├── url.rs                     # 内部服务 HTTP 路由 URL 格式化与解析
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
├── malody4/                       # 【Malody 4 内存与分析引擎】
│   ├── spec.rs                    # [纯标准库] 常量、ClientSpec、RawSettings、内存规格
│   ├── anchor.rs                  # PE 头解析、特征码扫描、签名自证
│   ├── library.rs                 # 谱面本地索引构建与 SQLite 数据库查询
│   ├── runtime.rs                 # 后台轮询主循环、Attached 状态持有、日志拦截与心跳
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
│   │   ├── table.rs               # 偏移表查找梯子与加载逻辑
│   │   └── frame.rs               # 逐帧读取、解引用链、L0 attach 与 read_tick
│   ├── offsets.rs                 # 偏移表统一门面
│   ├── offsets/                   # --- 偏移表子模块划分 ---
│   │   ├── lookup.rs              # FieldLookup、LookupError、canonical_type
│   │   ├── lazer_table.rs         # OffsetTable、RuntimeSection、就近回落
│   │   ├── stable_table.rs        # StableTable、StableTopology、本地梯子发现
│   │   ├── validation.rs          # 纯数据 Schema 安全与合理性校验器
│   │   └── remote.rs              # Ed25519 签名验证、清单解析与原子落盘
│   ├── invariants.rs              # 不变量规则集、L0~L2 校验器与 Gate 状态机
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
│   └── win.rs                     # 32/64 位 Windows 内存访问基础
│
├── server/                        # 【本地网络与 IPC 服务层】
│   ├── state.rs                   # 子系统状态封装 (BroadcastHub, SettingsHub, SourcesRegistry, WindowBridge)
│   ├── bridge.rs                  # Malody HTTP 桥接门面
│   ├── bridge/                    # --- 桥接子模块划分 ---
│   │   ├── state.rs               # MalodyBridgeState、ContentKey、LogGate
│   │   ├── settings.rs            # SettingsSnapshot、判定缩放与判定线推导
│   │   ├── chart.rs               # 沙箱路径安全检查、谱面字段推导、SongFrame
│   │   └── handler.rs             # HTTP POST /malody-select 与 /malody-state 路由处理
│   ├── ws.rs                      # 24062 广播服务、心跳保持与客户端连接池
│   ├── http.rs                    # 24061 本地 HTTP 静态托管、/shell-config REST API
│   ├── log.rs                     # 环形内存日志缓冲与滚动落盘
│   ├── error.rs                   # 服务端专用错误类型
│   └── mod.rs                     # 顶层服务聚合与生命周期
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
前端页面与桌面壳的通信受到 [`desktop/docs/CONTRACT.md`](CONTRACT.md) v6 协议保护：
- **24060 POST**：Malody 桥接通信通道；
- **24061 HTTP**：页面静态资源托管、`/shell-config` 读取与修改、`/offsets/generate` 与 `/offsets/update` 操作；
- **24062 WS**：原生广播通道，推送 `sources.osu`、`sources.malody`、`sources.malody4` 等结构帧。

---

## 4. 后续开发与扩展规范

1. **就近原则**：添加新特性或辅助逻辑时，必须就近放置在相关领域子目录下，严禁重回单文件千行以上的大膨胀模式；
2. **测试伴随**：纯逻辑函数（如版本号解析、配置合并、特征掩码比对）需随文件就地编写 `#[cfg(test)]` 单测，确保 `cargo test --lib` 能够秒级覆盖；
3. **平台安全**：Windows 特有 API 必须使用 `#[cfg(windows)]` 条件编译，并在对应位置提供 `#[cfg(not(windows))]` 安全空实现或回落，确保跨平台编译（如 Linux CI）不被破坏。
