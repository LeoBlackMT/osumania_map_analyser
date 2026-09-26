# 桌面壳（desktop shell）技术文档

> 面向 AI 的技术文档（实现细节、检测逻辑、契约、构建）。**人类使用教程**见 [docs/shell-guide.md](../shell-guide.md)（安装/窗口操控/桥接线/故障排查）。多数据源整体架构见 [multi-source.md](multi-source.md)。

## 1. 定位与架构

桌面壳是可选的 Tauri v2 桌面窗口，同时是三大协作方：

1. **宿主窗口**：加载插件页（在线= tosu 插件页；离线=壳自身 24061 静态服务），提供置顶/无边框/透明形态，可覆盖在游戏（含全屏）之上。
2. **本地聚合桥**（无 tosu 环境时的数据面）：
   - `24060` HTTP POST——Malody 编辑器分析入口（经 WS 中转到页面）；
   - `24061` HTTP——插件页静态服务（离线模式的 `index.html`、`presets.html`、**设置页 `settings.html`**）、`/settings`（插件设置，见 §3c）、`/shell-config`（壳配置）、`/open-settings`（打开设置窗口）、`/cover/`（封面白名单）；
   - `24061/ws` WS——帧通道（hello/state/song/settings/result/control + ping），页面与壳双向（帧契约见 §3，**本机 HTTP 面不属于该契约**，见 §3c）。
3. **Etterna 数据源轮询器**：2Hz 轮询桥文件（`Save/MmaBridge.txt` / `Save/MmaGameplay.txt`），解析后广播 song/state 帧。
4. **Malody 4.3.7 只读观察器**：200ms 一拍——从进程外读锚点身份键（`ReadProcessMemory`）、tail 游戏日志取场景、轮询 `config.json` 取变速位与判定档，广播 `malody4_selection` / song / state 帧。**不向游戏目录写入任何文件**、不注入、不 hook（详见 [multi-source.md](multi-source.md) 与 [malody4-source.md](malody4-source.md)）。

模块：`desktop/src/{main,config,frames,etterna,malodyv,settings_window}.rs` + `malody4/{mod,anchor,library,gamelog,selection,config,model}.rs` + `server/`（mod/http/ws/post/log/bridge）；契约 `desktop/docs/CONTRACT.md`（**v5**）。窗口内插件数据面还有一个**独立本机端点**：`127.0.0.1:17653` 的 `POST /selection`，由 Malody V 游戏内选曲桥（BepInEx 插件）推送（见 §3b）。

## 2. 启动流程（main.rs + config.rs）

```
probe_existing_instance()（24061，3×200ms 探测；命中本壳 → 原样带 --settings 则转交 POST /open-settings，
  然后一律 exit(0)（不建任何窗口）；端口被外部进程占用 → error + exit(2)）
plugin_dir() 解析（env MMA_PLUGIN_DIR 覆盖 → exe 上溯 0..=3 层找
  含 index.html 的 "ManiaMapAnalyser by Leo_Black" → 兜底相对路径）
probe_tosu_env()（exe 目录向上 ≤3 层找 tosu.env；MMA_SKIP_TOSU_PROBE 跳过）
  ├─ 命中且 tosu_online()（TCP connect 2s）→ url = http://{ip}:{port}/{插件目录 %20}/
  └─ 未命中/离线 → url = http://127.0.0.1:24061/
server::start（24060/24061 + **17653 桥监听** + 30s 定时帧 + etterna poller）
window.navigate(url)
setup：单实例且带 --settings → settings_window::open_or_focus（主窗恢复之后）
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
| settings | 双向 | 设置 JSON（在线 = tosu 设置文件内容，离线 = 本地 `mma-settings.json`；壳在来源切换/文件变化/离线 POST 后主动推，设置页离线时另做 pull-on-notify，见 §3c） |
| result | 页→壳 | requestId/statusHint/errors/activeSource/star/pattern/updatedAt |
| control | 页→壳 | `{action: toggleTopmost\|toggleClickThrough\|alwaysOnTop\|clickThrough\|close\|dragStart, value: bool}`（窗口操控；toggle 为 Wayland 页面内快捷键兜底，状态以 `mma-shell-state.json` 为权威） |
| ping | 双向 | 15s keepalive |

> **本机 HTTP 面不属于帧契约**：§3c 的 `/settings`、`/shell-config`、`/open-settings` 与静态服务是壳内的本机 HTTP 端点，没有帧型、没有 `{v, type, seq}` 信封、也不新增 `state`/`song` 字段，因此**契约版本不变**（`CONTRACT_VERSION` 与页面 `bridgeClient.js` 的同名常量都仍是 **5**）。改这一面不需要升契约版本（CONTRACT.md §13）。

## 3b. Malody V 选曲桥端点（`127.0.0.1:17653`）

游戏侧是 BepInEx 6 IL2CPP 插件 `MMAMalodySelection.dll`（本仓库自建 fork，源码 `bridges/malody/bepinex/plugin/`），它 `POST /selection` 推送选曲/游玩/结算观察。壳侧实现 `server/bridge.rs`：

- **载荷 11 字段**：上游 8 字段 + `judge_level`（0..4 ↔ A~E，`MAX` 不算档位）+ `pro_judge` + `turbo`。三者全部容错：缺失/`null`/类型错/越界一律视为"未知"，**绝不拒收**（旧插件仍可用）。
- **门禁顺序**：Host 白名单 → 路径沙箱（`canonicalize` 后必须落在 `{malodyRoot}/chart/` 内且后缀 `.mc`/`.osu`）→ **任何 Origin 头出现即 403** → 方法 → Content-Type → body 长度 → JSON。任一步不过都返回 JSON 错误体。
- **真实事件判定 = 内容六元组**（`path, rate_text, screen, judge_text, pro_text, turbo_text`）；心跳与重复观察不产生帧、不续期。`turbo` 必须在元组里：选曲界面切 Turbo 时倍率不变，少了它会把它当成重复而静默停在上一个值。
- **`winScale`**：只有"确认非 Turbo（`turbo == false`）且倍率命中名义值 1.2/1.5/0.8（±0.005）"才给 `1/名义值`；Turbo／未知／自定义倍率一律 `null`。**未知不得与"确认非 Turbo"混为一谈**——那正好会让 Dash/Rush/Slow 被当成 Turbo。
- **判定档来源**：插件值优先；插件没给才回落 `{malodyRoot}/config.json` 的 `user_judge_level`，两者都有且不一致时用插件值并打**一条** info。Pro **永不**来自该文件。
- **端口被占**：壳报"Malody 选曲桥端口 17653 被占用，游戏内选曲跟随不可用"，其余功能不受影响。

## 3c. 本机 HTTP 端点（`127.0.0.1:24061`）

Host 头只放行 `127.0.0.1:24061` / `localhost:24061` / `[::1]:24061`（无 Host 头的裸 HTTP/1.0 客户端放行），其余 403（DNS rebinding 防护）；设置页与端点同源，故无需 CORS。

| 端点 | 方法 | 状态码与语义 |
| --- | --- | --- |
| `/settings` | GET | 200 + **权威链全量对象**（在线 = tosu 设置文件；离线 = `mma-settings.json`；都没有则生成默认骨架并落盘）。读取侧不设只读门控——只读体现在写侧 |
| `/settings` | POST | 200 + **合并后的全量对象**（请求键优先的读-改-写，base 的未知键保留，绝不把文件截断成请求体）；**403 `tosu online: settings are read-only`**（在线只读；判据与 GET 第 1 级是**同一个表达式**，且 POST 内先复探 tosu 存活、跳变经 `apply_tosu_online_transition` 切源）；**400**（body 不是合法 JSON 或不是对象）；**500**（base 不可读或写盘失败）。成功 → 更新缓存 + 广播 settings 帧 |
| `/shell-config` | GET | 200 + `{config, resolved:{etternaRoot,malodyRoot,malody4Root}}`（`config` = `mma-shell-config.json` 全文；`resolved` = **实际采纳**路径，解析不到为 JSON `null`）；**400 `shell config unreadable`**（配置不可读） |
| `/shell-config` | POST | 200 + 合并后的全量壳配置（请求键优先，未知键保留）；**400**（body 非 JSON / 非对象 / base 不可读或写盘失败——这几种情况**什么都不写**，不生成骨架）。成功 → 更新缓存 + `clear_detect_caches()`（根目录探测缓存失效）+ 广播 settings 帧 |
| `/open-settings` | POST | 200 `{}`（**异步**发起建窗/聚焦，不等窗口真的建出来）；**503 `no app handle`**（无窗口模式：app 句柄未注入） |
| `/cover/...` | GET | 白名单内的具体文件 200（`Access-Control-Allow-Origin: *`）；未列入白名单 404 |
| `/`、`/settings.html`、`/presets.html`、`/js/**`、`/styles/**` 等 | GET/HEAD | 插件目录静态文件（`canonicalize` 防穿越，`Cache-Control: no-store`）；其他方法 405，目录穿越/不存在 404 |
| `/ws` | WS | 帧通道（§3 的八型 + diag 旁路），Origin 仅 loopback |

设置页资源（`settings.html`、`js/app/settingsPage/*.js`、`styles/settings-page.css`）全部由静态分支服务，无需为 24061 增加路由。

## 4. 窗口操控（v2 起）

无边框（decorations:false）、透明、置顶（alwaysOnTop:true）为默认形态；`resizable:true`——**拖拽边缘改窗口尺寸**；**整窗移动**用页面顶部 `data-tauri-drag-region` 拖动把手（22px 发光条，中间 `⋮⋮` 提示）；页面缩放走 WebView 原生（`Ctrl+滚轮` / `Ctrl+=` / `Ctrl+-`）。

**全局快捷键**（tauri-plugin-global-shortcut，启动时注册，与页面焦点/连接无关，点击穿透时同样生效）：默认 `Ctrl+Shift+T` 置顶开关、`Ctrl+Shift+C` 点击穿透开关（`set_ignore_cursor_events`）、`Ctrl+Q` 关闭；**可经 `mma-shell-config.json` 的 `hotkeys` 覆盖**（避免与系统冲突），注册成败写入日志（`shortcut FAILED` 即冲突）。**平台差异**：Windows（RegisterHotKey）/X11（XGrabKey）为真全局，失焦/穿透均生效；**Wayland 会话下 XGrabKey 注册成功但永不触发**——壳窗口聚焦时由页面内快捷键兜底（键位一致，经 control 帧 `toggleTopmost`/`toggleClickThrough`/`close`，见 §3）。窗口位置/尺寸/置顶/穿透状态持久化到 `mma-shell-state.json`（exe 旁），启动恢复、切换与关闭时保存；置顶/穿透以该文件为唯一权威（快捷键、control toggle、5s persister、窗口事件四写通道均先合并磁盘值再写，避免旧内存快照回滚对侧通道的切换）。Windows 透明白闪已由 `noRedirectionBitmap` 配置缓解（如仍有白闪属已知抖动）。

**设置窗口（`settings_window.rs`，本轮新增）**：壳内第二个普通 OS 窗口（Tauri label `settings`，标题 `ManiaMapAnalyser — Settings`，默认 1040×720、首次居中、`always_on_top(false)`），承载 24061 的 `settings.html`。三个入口共用 `settings_window::open_or_focus`：全局快捷键 `hotkeys.settings`（默认 `Ctrl+Shift+S`）、CLI `--settings`（单实例启动时在 `setup` 里打开；已有实例时转交 `POST /open-settings` 后 `exit(0)`）、`POST /open-settings`（浏览器/脚本）。`settings.html` 缺失时只记 warn 且**不置标志**（不会出现"显示已开却没有窗口"）；`build()` 失败时复位标志 + 按磁盘恢复主窗置顶并记 error（自愈）。

- **线程规则**（改这个模块前必读）：除原子标志与文件 I/O 外，**一切**窗口 API（`build`/`show`/`unminimize`/`set_focus`/`close`/`set_always_on_top`）只在 `std::thread::spawn` 的闭包内调用。原因是主线程内联派发 + wry 建 WebView2 会阻塞在 `wait_with_pump`（`send_user_message` 在主线程上是内联执行，`run_on_main_thread` 也不是解法），在主线程/回调里同步建窗会自死锁。快捷键、HTTP、`on_window_event` 回调只碰原子标志与文件。
- **几何**：写入**独立文件** `mma-shell-settings-window.json`（`SettingsWindowState`），与主窗 `WindowState`/`mma-shell-state.json` 完全分离（主窗的 4 条写通道与 5s persister 只碰后者，两窗几何不会互相踩）。`Moved`/`Resized` 载荷是 **physical** 像素，恢复用 `PhysicalPosition`/`PhysicalSize`（builder 的 `position`/`inner_size` 是 logical，混用会在高 DPI 下逐次漂移）。
- **置顶与事件分流**：窗口打开期间临时 `set_always_on_top(false)` 主窗（**不写** `mma-shell-state.json`；关闭或建窗失败时按该文件恢复），两个全局快捷键回调（置顶/穿透）在 `is_open()` 时只记 debug 并 return，避免把设置窗口盖住。`on_window_event` 按 `window.label()` 分流：`main` = 原逻辑 + `CloseRequested` 连带 `settings_window::close()`（关 overlay 一起关设置窗口）；`settings` = 几何写独立文件，`CloseRequested`/`Destroyed` 复位标志并恢复主窗置顶。
- **已知差异（计划 §11-Q9）**：置顶/穿透的**页面内兜底路径**（Wayland 无全局快捷键时页面经 control 帧 `toggleTopmost`/`toggleClickThrough` → `server/ws.rs handle_control`）**没有** `is_open()` 门控——设置窗口打开期间按这两个兜底键仍会切换置顶并写 `mma-shell-state.json`，主窗可能重新盖住设置窗口。本轮刻意保留该差异（保持"不动 `ws.rs`"）；修法是 4 行（让 `handle_control` 也走 `is_open()` 判定），另立处理。

## 4b. 配置、日志与路径容错

- **壳配置 `mma-shell-config.json`**（exe 旁，首启自动生成骨架）：`gameClient`/`etternaRoot`/`malodyRoot`/`malody4Root`/`hotkeys`/`logLevel`；损坏或非法字段自动回落默认并警告（不崩溃）。`malody4Root` 由桥安装器的第三个游戏项（`Malody 4`）自动写入——该选项**只写这个键、不复制任何文件**，卸载只清该键。
- **全量插件设置 `mma-settings.json`**（exe 旁）：**权威链只有一个判据——tosu 是否在线**（`config::resolve_plugin_settings`）：① tosu 在线（`shared.tosu.is_some()` 且 `tosu_online`）→ tosu 设置文件权威（只读；离线**绝不**读该文件，即使它存在）；② 否则读本地 `mma-settings.json`；③ 本地也没有 → 按插件 `settings.json` 的 `value` 生成默认骨架并落盘（`ensure_plugin_settings` 只在**离线且本地无文件**时播种，不继承 tosu `values.json` 里的旧值）。`GET /settings` 按此链返回；离线 `POST /settings` 走 `merge_plugin_settings` 读-改-写（请求键优先、未知键保留）并广播全量；定时器对本地文件的检测带 `if !online` 门控（`stat → read → stat` 防读到写一半的内容），对 tosu 文件的检测带 `if online` 门控。
- **图形化设置界面**：设置窗口（§4）运行 24061 上的 `settings.html`——在线只读（表单禁用 + `POST /settings` 403 + 预设区换成 tosu Presets 页面链接），离线可写（改一项即 POST → 落盘 + 广播 overlay；预设区为完整管理器，自定义预设写进 `mma-settings.json` 的 `presetStorage`，离线不显示 LastSavedPreset 行）。页面侧经预设 transport 注入（`setPresetTransport`，见 [presets.md](presets.md)「离线（壳）模式」）。
- **路径容错**：根路径字段经 `normalize_path` 归一化——`\` 与 `/` 混用、尾部斜杠均处理（用户写 `D:\Games\Etterna` 或 `D:/Games/Etterna` 均可；JSON 内单反斜杠是非法转义，文档已提示用 `/` 或 `\\`）。
- **日志**：`mma-shell-{YYYYMMDD}.log`（exe 旁 `logs/` 目录）按日轮转、保留 7 个，每行 `[YYYY-MM-DD HH:MM:SS] [级别] 消息`；`log_level()` 按 `mma-shell-config.json logLevel` 过滤（debug/info/warn/error/off）。逐帧/轮询/诊断日志为 debug 级（info 只留启动/错误/快捷键注册）。

## 5. 构建与发布

- 开发：`cargo build`（debug 保留控制台输出）。
- 发布：`cargo build --release`；打包脚本 `desktop/release.ps1`（Windows，打 zip）/ `desktop/build-linux.sh`（Linux，打 tar.gz，保留执行位），版本号均读插件 `metadata.txt`，产物落在仓库 `release/`；**Windows release 为 GUI 子系统**（`#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]`）——正式交付无命令行窗口。
- **平台**：Windows 全功能；Linux 支持 Etterna 数据源（0.75+ 官方 Linux 版），Malody V 无 Linux 版（该源仅 Windows）。**Malody 4.3.7 源同样仅 Windows**（`ReadProcessMemory` 与 PE 版本校验是 Win32 语义）：Linux 上该源以 `platform-unsupported` **优雅不可用**，不影响其余三个源与浏览器模式。
- CI：`.github/workflows/shell-build.yml`——仅当 **main 分支 `desktop/**` 变更**时构建 **Windows + Linux**（release 产物上传 artifact `mma-shell-windows` / `mma-shell-linux`；Linux = ubuntu-24.04 + Tauri 系统依赖）；`workflow_dispatch` 可手动触发。Linux 构建曾因无受众移除，Etterna 0.75 发布官方 Linux 版后恢复（Ubuntu 24.04 VM 全链路实测）。

## 6. 已知限制与待办

离线设置持久化已实现（`mma-shell-config.json` 壳配置 + `mma-settings.json` 插件设置 + 页面 `/settings` 应用，在线时仍以 tosu 为准只读）；本轮新增**图形化设置窗口**（§4）与三个本机端点（§3c），离线可写、在线只读，权威链收敛为"在线 tosu / 离线本地"。**已知限制**：置顶/穿透的页面内兜底路径不经 `is_open()` 门控（§4 末的 §11-Q9 差异）；第二实例遇到不带 `/open-settings` 的旧壳时静默退出并记日志；`mma-shell-state.json` 既有的并发写隐患未处理（超出本轮范围）；Wayland 无全局快捷键（设置窗口用 `--settings` 或浏览器直开兜底）；离线且本地无 `mma-settings.json` 时骨架取插件默认值，**不继承** tosu `values.json` 里的旧值。外部源封面消费（壳 cover URL 已下发）为待办。真机验证项：Etterna 主题桥真实写入、Malody 编辑器文件通道（WriteFile `<base>_mma_request.json` → 壳扫 chart/（两级目录 mtime 快筛，≤1Hz）→ 谱面 = 同目录 `<base>.mc|.osu` → 分析 → 卡片展示，处理完删 request；DoRequest POST 实测被 Malody 网络层拒绝（invalid url: {body}），故不走网络通道）、PlayMeta 字段、Malody 4.3.7 真机端到端跟随；窗口穿透与透明目视。浏览器模式（无壳）不受影响：osu! 单源，control/result no-op。来源指示器：空心=无源；osu! 粉 / Etterna 紫 / Malody 4 亮青（`#22d3ee`）/ Malody V 蓝实心——**Malody 4 = Malody 4.3.7 原生客户端，与 Malody V 是两个独立源、两个独立圆点颜色**。

**版本冻结的后果（必须知悉）**：桥契约已升到 **v5**（v3→v4→v5），但插件版本号**不 bump**（`index.js` `_VERSION` 与 `metadata.txt` `Version` 均为 `2.1.0`）。因此**使用陈旧 tosu 静态页的壳用户**在 hello 握手会因 `contract` 不匹配进入终态（页面提示更新插件并停止重连），**外部源全部不可用**——这类用户必须**更新插件文件**，且因为版本号没变，他们**不会**收到"有新版本"的提示。