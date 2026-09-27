# 桌面壳（desktop shell）技术文档

> 面向 AI 的技术文档（实现细节、检测逻辑、契约、构建）。**人类使用教程**见 [docs/shell-guide.md](../shell-guide.md)（安装/窗口操控/桥接线/故障排查）。多数据源整体架构见 [multi-source.md](multi-source.md)。

## 1. 定位与架构

桌面壳是可选的 Tauri v2 桌面窗口，同时是三大协作方：

1. **宿主窗口**：加载插件页（在线= tosu 插件页；离线=壳自身 24061 静态服务），提供置顶/无边框/透明形态，可覆盖在游戏（含全屏）之上。
2. **本地聚合桥**（无 tosu 环境时的数据面）：
   - `24060` HTTP POST——Malody 编辑器分析入口（经 WS 中转到页面）；
   - `24061` HTTP——插件页静态服务（离线模式）、`/settings`（离线设置）、`/cover/`（封面白名单）；
   - `24061/ws` WS——帧通道（hello/state/song/settings/result/control + ping），页面与壳双向。
3. **Etterna 数据源轮询器**：2Hz 轮询桥文件（`Save/MmaBridge.txt` / `Save/MmaGameplay.txt`），解析后广播 song/state 帧。
4. **Malody 4.3.7 只读观察器**：200ms 一拍——从进程外读锚点身份键（`ReadProcessMemory`）、tail 游戏日志取场景、轮询 `config.json` 取变速位与判定档，广播 `malody4_selection` / song / state 帧。**不向游戏目录写入任何文件**、不注入、不 hook（详见 [multi-source.md](multi-source.md) 与 [malody4-source.md](malody4-source.md)）。

模块：`desktop/src/{main,config,frames,etterna,malodyv}.rs` + `malody4/{mod,anchor,library,gamelog,selection,config,model}.rs` + `server/`（mod/http/ws/post/log/bridge）；契约 `desktop/docs/CONTRACT.md`（**v5**）。窗口内插件数据面还有一个**独立本机端点**：`127.0.0.1:17653` 的 `POST /selection`，由 Malody V 游戏内选曲桥（BepInEx 插件）推送（见 §3b）。

## 2. 启动流程（main.rs + config.rs）

```
plugin_dir() 解析（env MMA_PLUGIN_DIR 覆盖 → exe 上溯 0..=3 层找
  含 index.html 的 "ManiaMapAnalyser by Leo_Black" → 兜底相对路径）
probe_tosu_env()（exe 目录向上 ≤3 层找 tosu.env；MMA_SKIP_TOSU_PROBE 跳过）
  ├─ 命中且 tosu_online()（TCP connect 2s）→ url = http://{ip}:{port}/{插件目录 %20}/
  └─ 未命中/离线 → url = http://127.0.0.1:24061/
server::start（24060/24061 + **17653 桥监听** + 30s 定时帧 + etterna poller）
window.navigate(url)
```

目录检测（如实标注）：

- **插件目录**：`MMA_PLUGIN_DIR` 环境变量 > exe 目录逐级上溯 0–3 层，每层拼 `ManiaMapAnalyser by Leo_Black` 并校验 `index.html` 存在；都不中则用相对路径。发布形态（exe 与插件目录同层）上溯 0 层即命中；开发形态（target/debug）上溯 2 层命中仓库根。
- **tosu.env**：从 exe 所在目录开始向上（含当前层）最多 3 层；解析 `SERVER_PORT`（默认 24050）与 `SERVER_IP`（默认 127.0.0.1）；根目录 = tosu.env 所在目录（用于 `settings/{插件名}.json` 只读读取）。
- **Etterna 与 Malody V 根**：`MMA_ETTERNA_ROOT` / `MMA_MALODY_ROOT` 环境变量 > **壳配置 `mma-shell-config.json`**（exe 旁，`{gameClient, etternaRoot, malodyRoot, malody4Root, hotkeys, logLevel}`，可直接编辑，30s 周期检测变化后重载并推送 settings 帧）> tosu 在线只读。无 tosu 用户无需下载 tosu 即可配置游戏路径。启发探测（Steam 库/常见路径）带**盘符就绪预检**——不存在的盘符（用户没有 D: 盘等）快速跳过、绝不 panic/阻塞；且探测结果 30s TTL 缓存，未配置根目录时轮询器不会每个周期都打注册表与盘符。
- **Malody 4.3.7 根**：解析链顺序为「运行中的进程目录（只要求同目录有 `malody.exe`，不做版本校验，以便版本不符能如实报 `target-mismatch:*`）→ `MMA_MALODY4_ROOT` → 壳配置 `malody4Root` → tosu 设置同键 → 启发候选」。前四级一律"非空即采纳"；第五级是**唯一做版本校验**的一级，候选是**绝对路径**且仅在 PE 三重校验通过时采纳，故"留空 `malody4Root`"不等于关断（要关断请把 `MMA_MALODY4_ROOT` 指向不存在的路径）。`root-not-configured` 只表示整条链走完仍为 `None`。

## 3. 契约 v5 帧

| 帧 | 方向 | 载荷要点 |
| --- | --- | --- |
| hello | 壳→页 | `{contract: 5, tosuOnline}`；页面接受 `[3,5]`，越界=终态（页面停止重连并提示）。**壳升版而页面不升会让 `sendControl` 一起失效**（拖动把手与置顶/穿透/关闭快捷键），两处常量必须同步 |
| state | 壳→页 | tosuOnline/errors/sources{etterna{alive,playing,playingExpireAt},**malody{alive,transport,screen,playing,eventSeq,judge,pro,turbo}**,malody4{alive,playing,screen,reason?,judge?}} |
| song | 壳→页 | requestId/source/identity/modData{rate,...}/meta{...judge?}/cover/rawText；**桥通道另带 `screen`/`judge`/`pro`/`turbo`/`winScale`**（`winScale` 可为 `null`＝未知，页面据此关闭动态 OD 而非假定 1.0） |
| malody4_selection | 壳→页 | `{path, speed_rate, screen, sequence, event, version, chart_hash, source}`；`event ∈ anchor-changed/scene-changed/heartbeat/hidden`，`path` 为空 = hidden（未选中/不可用），原因另经 `sources.malody4.reason` 与壳日志给出 |
| settings | 双向 | 离线设置 JSON（在线只读不推） |
| result | 页→壳 | requestId/statusHint/errors/activeSource/star/pattern/updatedAt |
| control | 页→壳 | `{action: toggleTopmost\|toggleClickThrough\|alwaysOnTop\|clickThrough\|close\|dragStart, value: bool}`（窗口操控；toggle 为 Wayland 页面内快捷键兜底，状态以 `mma-shell-state.json` 为权威） |
| ping | 双向 | 15s keepalive |

## 3b. Malody V 选曲桥端点（`127.0.0.1:17653`）

游戏侧是 BepInEx 6 IL2CPP 插件 `MMAMalodySelection.dll`（本仓库自建 fork，源码 `bridges/malody/bepinex/plugin/`），它 `POST /selection` 推送选曲/游玩/结算观察。壳侧实现 `server/bridge.rs`：

- **载荷 11 字段**：上游 8 字段 + `judge_level`（0..4 ↔ A~E，`MAX` 不算档位）+ `pro_judge` + `turbo`。三者全部容错：缺失/`null`/类型错/越界一律视为"未知"，**绝不拒收**（旧插件仍可用）。
- **门禁顺序**：Host 白名单 → 路径沙箱（`canonicalize` 后必须落在 `{malodyRoot}/chart/` 内且后缀 `.mc`/`.osu`）→ **任何 Origin 头出现即 403** → 方法 → Content-Type → body 长度 → JSON。任一步不过都返回 JSON 错误体。
- **真实事件判定 = 内容六元组**（`path, rate_text, screen, judge_text, pro_text, turbo_text`）；心跳与重复观察不产生帧、不续期。`turbo` 必须在元组里：选曲界面切 Turbo 时倍率不变，少了它会把它当成重复而静默停在上一个值。
- **`winScale`**：只有"确认非 Turbo（`turbo == false`）且倍率命中名义值 1.2/1.5/0.8（±0.005）"才给 `1/名义值`；Turbo／未知／自定义倍率一律 `null`。**未知不得与"确认非 Turbo"混为一谈**——那正好会让 Dash/Rush/Slow 被当成 Turbo。
- **判定档来源**：插件值优先；插件没给才回落 `{malodyRoot}/config.json` 的 `user_judge_level`，两者都有且不一致时用插件值并打**一条** info。Pro **永不**来自该文件。
- **端口被占**：壳报"Malody 选曲桥端口 17653 被占用，游戏内选曲跟随不可用"，其余功能不受影响。

## 4. 窗口操控（v2 起）

无边框（decorations:false）、透明、置顶（alwaysOnTop:true）为默认形态；`resizable:true`——**拖拽边缘改窗口尺寸**；**整窗移动**用页面顶部 `data-tauri-drag-region` 拖动把手（22px 发光条，中间 `⋮⋮` 提示）；页面缩放走 WebView 原生（`Ctrl+滚轮` / `Ctrl+=` / `Ctrl+-`）。

**全局快捷键**（tauri-plugin-global-shortcut，启动时注册，与页面焦点/连接无关，点击穿透时同样生效）：默认 `Ctrl+Shift+T` 置顶开关、`Ctrl+Shift+C` 点击穿透开关（`set_ignore_cursor_events`）、`Ctrl+Q` 关闭；**可经 `mma-shell-config.json` 的 `hotkeys` 覆盖**（避免与系统冲突），注册成败写入日志（`shortcut FAILED` 即冲突）。**平台差异**：Windows（RegisterHotKey）/X11（XGrabKey）为真全局，失焦/穿透均生效；**Wayland 会话下 XGrabKey 注册成功但永不触发**——壳窗口聚焦时由页面内快捷键兜底（键位一致，经 control 帧 `toggleTopmost`/`toggleClickThrough`/`close`，见 §3）。窗口位置/尺寸/置顶/穿透状态持久化到 `mma-shell-state.json`（exe 旁），启动恢复、切换与关闭时保存；置顶/穿透以该文件为唯一权威（快捷键、control toggle、5s persister、窗口事件四写通道均先合并磁盘值再写，避免旧内存快照回滚对侧通道的切换）。Windows 透明白闪已由 `noRedirectionBitmap` 配置缓解（如仍有白闪属已知抖动）。

## 4b. 配置、日志与路径容错

- **壳配置 `mma-shell-config.json`**（exe 旁，首启自动生成骨架）：`gameClient`/`etternaRoot`/`malodyRoot`/`malody4Root`/`hotkeys`/`logLevel`；损坏或非法字段自动回落默认并警告（不崩溃）。`malody4Root` 由桥安装器的第三个游戏项（`Malody 4`）自动写入——该选项**只写这个键、不复制任何文件**，卸载只清该键。
- **全量插件设置 `mma-settings.json`**（exe 旁，仅离线模式）：优先级链 = tosu 设置文件（在线只读 / 离线读文件）> `mma-settings.json` > 按插件 `settings.json` 生成默认骨架（用户手改重启生效）。`GET /settings` 按此链返回；离线 `POST /settings` 写 `mma-settings.json`；timers 检测 `mma-settings.json` 变化并推送 settings 帧。
- **路径容错**：根路径字段经 `normalize_path` 归一化——`\` 与 `/` 混用、尾部斜杠均处理（用户写 `D:\Games\Etterna` 或 `D:/Games/Etterna` 均可；JSON 内单反斜杠是非法转义，文档已提示用 `/` 或 `\\`）。
- **日志**：`mma-shell-{YYYYMMDD}.log`（exe 旁 `logs/` 目录）按日轮转、保留 7 个，每行 `[YYYY-MM-DD HH:MM:SS] [级别] 消息`；`log_level()` 按 `mma-shell-config.json logLevel` 过滤（debug/info/warn/error/off）。逐帧/轮询/诊断日志为 debug 级（info 只留启动/错误/快捷键注册）。

## 5. 构建与发布

- 开发：`cargo build`（debug 保留控制台输出）。
- 发布：`cargo build --release`；打包脚本 `desktop/release.ps1`（Windows，打 zip）/ `desktop/build-linux.sh`（Linux，打 tar.gz，保留执行位），版本号均读插件 `metadata.txt`，产物落在仓库 `release/`；**Windows release 为 GUI 子系统**（`#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]`）——正式交付无命令行窗口。
- **平台**：Windows 全功能；Linux 支持 Etterna 数据源（0.75+ 官方 Linux 版），Malody V 无 Linux 版（该源仅 Windows）。**Malody 4.3.7 源同样仅 Windows**（`ReadProcessMemory` 与 PE 版本校验是 Win32 语义）：Linux 上该源以 `platform-unsupported` **优雅不可用**，不影响其余三个源与浏览器模式。
- CI：`.github/workflows/shell-build.yml`——仅当 **main 分支 `desktop/**` 变更**时构建 **Windows + Linux**（release 产物上传 artifact `mma-shell-windows` / `mma-shell-linux`；Linux = ubuntu-24.04 + Tauri 系统依赖）；`workflow_dispatch` 可手动触发。Linux 构建曾因无受众移除，Etterna 0.75 发布官方 Linux 版后恢复（Ubuntu 24.04 VM 全链路实测）。

## 6. 已知限制与待办

离线设置持久化已实现（`mma-shell-config.json` 壳配置 + `mma-settings.json` 插件设置 + 页面 `/settings` 应用，在线时仍以 tosu 为准只读）。外部源封面消费（壳 cover URL 已下发）为待办。真机验证项：Etterna 主题桥真实写入、Malody 编辑器文件通道（WriteFile `<base>_mma_request.json` → 壳扫 chart/（两级目录 mtime 快筛，≤1Hz）→ 谱面 = 同目录 `<base>.mc|.osu` → 分析 → 卡片展示，处理完删 request；DoRequest POST 实测被 Malody 网络层拒绝（invalid url: {body}），故不走网络通道）、PlayMeta 字段、Malody 4.3.7 真机端到端跟随；窗口穿透与透明目视。浏览器模式（无壳）不受影响：osu! 单源，control/result no-op。来源指示器：空心=无源；osu! 粉 / Etterna 紫 / Malody 4 亮青（`#22d3ee`）/ Malody V 蓝实心——**Malody 4 = Malody 4.3.7 原生客户端，与 Malody V 是两个独立源、两个独立圆点颜色**。

**版本冻结的后果（必须知悉）**：本轮桥契约升到 **v3**，但插件版本号**不 bump**（`index.js` `_VERSION` 与 `metadata.txt` `Version` 均为 `2.1.0`）。因此**使用陈旧 tosu 静态页的壳用户**在 hello 握手会因 `contract` 不匹配进入终态（页面提示更新插件并停止重连），**外部源全部不可用**——这类用户必须**更新插件文件**，且因为版本号没变，他们**不会**收到"有新版本"的提示。