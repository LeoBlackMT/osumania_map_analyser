# osu! 原生源（osu native source）

> 面向 AI 的技术文档。人类安装/使用说明见 [docs/shell-guide.md](../shell-guide.md)；多源整体架构、路由与传输层一句话概览见 [multi-source.md](multi-source.md)；壳侧帧契约（契约 v6）见 `desktop/docs/CONTRACT.md`；lazer 偏移表生成器的完整手册见 `tools/lazer-offsets-gen/README.md`。

## 1. 定位

`osu native` **不是第五个数据源**（`temp/osu-native-memory/DECISIONS.md` 的 REJ-03 明确否决"新增第五来源 `osu-native`"）：它是 **osu 源的第二套传输**——壳（`desktop/`）在进程内只读读取 osu!stable / osu!lazer 的内存，并在 `127.0.0.1:24062` 上提供一个 **tosu 兼容子集 origin**；页面的 osu 数据面（WS、`.osu`、背景图）原样指向它，页面代码与算法层零改动（DEC-01）。

三条边界（DEC-08 / DEC-03 / DEC-04）：

- **只承诺本插件消费的子集**：不承诺完整 tosu 兼容；不实现 tosu 的其它 overlay 面。
- **Windows only**：Linux 上整条通道以 `platform-unsupported` 优雅不可用（`desktop/src/osu/model.rs:32-33`），浏览器模式与其余三源不受影响。
- **只读**：只声明 `ReadProcessMemory` / `VirtualQueryEx` / Toolhelp32；句柄权限恒为 `PROCESS_VM_READ | PROCESS_QUERY_INFORMATION`（`desktop/src/osu/win.rs:28`：`0x0010 | 0x0400`），无任何写/挂起/注入 API。dump 采集只存在于 dev-time 工具（§10），产品内不存在。

tosu 保留为兜底传输：读取器不健康、24062 未绑定、或操作者显式逃生时，壳下发 `mode:"tosu"`，页面回到自己设置里的 `wsEndpoint`，行为与无壳时逐字节相同（`desktop/src/server/osu_source.rs:83-111`）。

## 2. 架构

```
osu!stable (32 位)          osu!lazer (64 位)
      │ 只读内存（签名扫描）        │ 只读内存（偏移表驱动）
      └────────────┬───────────────┘
       desktop/src/osu/**：读取线程（250 ms 一拍）
         ├─ L0 锚点定址 → L1 结构校验 → L2 不变量 → L3 影子比对（tosu 在线时）
         └─ Snapshot → packet.rs → tosu v2 形状载荷
                   │
      desktop/src/server/osu_compat.rs：24062（tosu 兼容子集 origin）
         ├─ WS /websocket/v2（150 ms 推一帧）＋ WS /websocket/commands（黑洞）
         └─ GET /files/beatmap/{file,background}（全部带 ACAO）
                   │
      desktop/src/server/osu_source.rs → state 帧 sources.osu.osuTransport
                   │
      页面：shellState.applyShellState → appContext.applyOsuTransport → socket.setHost
```

## 3. 只读内存层（`desktop/src/osu/**`）

产品内没有"两个读取器"的抽象：**客户端按位数分派**（DEC-19），两者进程名逐字相同（`osu!.exe`），PE machine `0x014C`（i386/32 位）⇒ stable、`0x8664`（x86-64/64 位）⇒ lazer，其它 machine 一律报 reason、绝不猜（`desktop/src/osu/model.rs:116-127`；`desktop/src/osu/win.rs` 的候选按位数分桶，`desktop/src/osu/discovery.rs` 给出 `Decision::Attach{pid,client}`）。

### 3.1 stable：签名扫描 + 锚点台账

- **锚点台账**（唯一权威）在 `desktop/src/osu/patterns.rs`：`ANCHORS` 共 **7 枚**（`patterns.rs:176-184`）——`statusPtr`（`state.name`）、`baseAddr`（Beatmap 对象/身份字段）、`playTimeAddr`（`beatmap.time.live`）、`rulesetsAddr`（局内/结算 mods、hits）、`menuModsPtr`（`menu.mods`）、`getAudioLengthPtr`（`mp3Length`，best-effort）、`settingsClassAddr`（`folders.songs` 回退链，best-effort）。每枚记录 `pattern`（含 `??` 通配）/ 有符号 `offset` / `derivation`（可独立复核的推导）/ `verified_build`（`osu!.exe` MD5 `f845ef10bf97c3260b818fc02e73c196`）/ `evidence`（验证方法，不是文件路径）。
- **best-effort 与必需组分开**：`BEST_EFFORT_KEYS`（`patterns.rs:256`）缺席只降级对应字段；必需组缺席 ⇒ `AnchorTable::missing()`（`patterns.rs:266-283`）给出键名 ⇒ reason `signature-miss:<key>`。
- **扫描**：两种过滤器档位各扫一遍（`desktop/src/osu/mod.rs:223`：0 = 参考过滤器 RW/RWX、1 = 宽过滤器"任一可读已提交区"），每枚锚点只取首个命中，命中后施加位移并做候选自证（对齐、落区、解引用合理性；`mod.rs::anchor_proves_out`）。
- **读原语是 fail-closed**：单次 `ReadProcessMemory` 硬上限 1 MiB（`desktop/src/osu/win.rs:966-967`），必须整段读满，短读（含 `ERROR_PARTIAL_COPY = 299`）一律当失败并按"对半缩小"重试（`win.rs` 的 `plan_shrink_sequence`），**绝不静默零填充**（零填充会把"读不到"伪装成"值是 0"）。
- **锚点缓存**（`desktop/src/osu/anchor_cache.rs`）：同一进程实例内的重复定址可复用地址，但必须**重新通过同一份验证**（签名复核 + 结构自证）；键 `(exe md5, 位数, module_base)` 一变即丢，绝不跨加载复用绝对地址（`patterns.rs:46-50`）。

### 3.2 lazer：偏移表驱动（不写死任何偏移）

lazer 是托管进程（osu! 每周级更新），**字段偏移只能从目标构建自己身上提取**，因此读取路径全部由表驱动（`desktop/src/osu/lazer.rs:1-60`）：

- **结构常数**（来自我们自己的 P4 探针，`osu/lazer.rs:12`）：12 字节标记模式、站点位移候选表 `SITE_DELTAS`（首项 `0x24`）、GameBase 多跳 `site → +0x0 → +0x218 → +0x310`。
- **偏移表**（来自 `tools/lazer-offsets-gen/`，SOS + IL 双见证）：每个 `Type.Field → offset`、`game_base_vtable`、版本键 `(lazer 版本, runtime 版本, 架构)`。
- **表梯子**（`desktop/src/osu/offsets.rs::resolve_lazer_table`）：① `$MMA_OFFSETS_DIR` 或 `$MMA_LAZER_OFFSETS` → ② `%APPDATA%\ManiaMapAnalyser\offsets\lazer\`（远端更新/用户生成的本地缓存）→ ③ `<壳 exe 目录>\offsets\lazer\<lazer>__<runtime>__<arch>.json` → ④ 同目录兼容旧路径 `<壳 exe 目录>\lazer-offsets\` → ⑤ 编译期内嵌默认表（`default_table()`，**绝不回落到不存在的状态**）。
- **`game_base_vtable` 不是准入条件**：它是运行期 MethodTable 指针（ASLR），跨进程必然不同，只作见证/溯源（`lazer.rs:28-30`）。
- **L1 结构证明**是跨进程可用的那一版：链能解通 + `[gameBase]` 对齐可读 + 会话内 MT 稳定 + 表侧字段探针（`lazer.rs:60-` 的整段注释；`desktop/src/osu/invariants.rs` 的 I-06 lazer 分支）。
- **`state.name` 的来源**（Step 10f）：屏幕栈 `_array`/`_size` → 屏幕对象的 MethodTable → EEType 的 `token@+0x8` / `loader_module@+0x18` / `Module.image_base@+0xc8` → 从表里 2284 条 typedef 名字解出类型名 → 我们自己的"类型名 → 状态名"映射表（观测集外的类型名**不猜**，报 `<类型名>-not-in-observed-set` 并降级）。
- **`Bindable<T>.value` 的偏移依赖 `T`**：查表键必须包含实例化实参，多个实例化命中 ⇒ 拒读并降级（OPEN-03 的修正：`NonNullableBindable<WorkingBeatmap>` 上是 `+0x20`，`+0x40` 是 `<Description>`）。
- **字符串字段也由表驱动**：`System.String._stringLength@+0x08` / `_firstChar@+0x0C` 缺任一 ⇒ 所有字符串类字段降级，绝不退回写死的 `0x08/0x0C`（`lazer.rs:345-348`）。

### 3.4 纯数据表化、Schema 校验与全自动分发体系（P2）

为了实现游戏更新后"只换表、不换二进制"，P2 引入了完整的表数据化与全自动生命周期体系：

1. **统一数据表目录（`desktop/offsets/`）**：
   - `desktop/offsets/stable/stable__x86.json`：包含 stable 的 7 枚锚点、特征字节码（含通配符 `??`）、相对位移与验证信息。
   - `desktop/offsets/lazer/*.json`：包含 lazer 的类型字段偏移映射、游戏版本、运行时版本与架构。
   - `desktop/offsets/manifest.json`：各表的 SHA-256 与 Ed25519 签名清单。
2. **纯数据红线与 Schema 严格校验**：
   - 内存特征表为纯静态 JSON，**严禁包含任何可执行代码、宏或脚本语义**（解析器防御性拦截包含 `<script>`、`eval`、`javascript:` 等字符串）。
   - 严格类型与范围校验（`validate_stable_schema`, `validate_lazer_schema`）：校验 pattern 字符合法性、签名长度（`4..=64` 字节）、位移合理范围（`±1048576` 字节）、字符串字段长度限制（≤256 字符），任何非法结构立即拒绝加载。
3. **随包分发的 1-Click 生成器（`gen.exe`）**：
   - 基于 `tools/lazer-offsets-gen/main.rs` 与 `live.rs` 编译为轻量独立工具随 `release.ps1` 一同打包。
   - **零 .NET SDK、零游戏挂起**：通过 `OpenProcess` 只读句柄在 1 秒内完成内存扫描定位，解析 MethodTable 与字段布局。
   - **活体自校验门禁**：生成后立即在运行中游戏的真实内存上执行解引用多跳验证：`site → GameBase → Beatmap → WorkingBeatmap → BeatmapInfo → MD5Hash`，唯有验证出 32 字符合法十六进制哈希并成功解析屏幕栈顶部状态名，才允许落盘。
   - 提供 HTTP 端点 `POST /offsets/generate` 与壳设置页一键按钮。
4. **Ed25519 签名远端表与静默离线回落**：
   - 壳内固化 Master 公钥（32 字节 Ed25519 公钥）。
   - 提供 `POST /offsets/update` 端点，从官方源获取 `manifest.json`，在本地用 Master 公钥验证每张表的数字签名与 SHA-256 哈希。
   - 校验通过后原子写入本地缓存目录（`%APPDATA%\ManiaMapAnalyser\offsets\`）。若无网络或校验失败，静默使用随包或编译期内嵌表，零报错打扰用户。
5. **stable 维护者专用探测生成器**：
   - `tools/stable-offsets-gen/main.rs`：针对 32 位 `osu!.exe` 执行签名扫描与自证验证，全自动导出符合 `mma-offset-v2` 格式的稳定版特征表。
6. **L3 影子比对诊断面板（Topic 12）**：
   - 在线运行时，壳在 `osu/compare.rs` 实时比对 Native 内存直读与 tosu 的 11 项核心字段（`COMPARED_FIELDS`），记录比对总帧数、匹配率与逐字段差异计数。
   - 提供 `GET /offsets/status`、`GET /shadow/status` 与 `POST /shadow/reset` 端点。
   - 设置页的 Shell Configuration 面板内提供特征表状态监控、生成/更新操作按钮与影子对拍诊断看板。

### 3.3 共享的读门槛

| 项 | 值/行为 | 出处 |
|---|---|---|
| 句柄权限 | `PROCESS_VM_READ \| PROCESS_QUERY_INFORMATION`（`0x0410`） | `osu/win.rs:28` |
| 单次读上限 | 1 MiB，超限=错误（不截断） | `osu/win.rs:966-967`、`osu/scan.rs:15` |
| 短读 | =失败（fail-closed） | `osu/win.rs:14-16` |
| C# 字符串上限 | 4096 码元（L2 另给 512 的"合理长度"降级） | `osu/win.rs:61`、`osu/invariants.rs:53` |
| 读取线程节奏 | 250 ms 一拍（`mod::TICK`）；对拍记录默认 100 ms | `osu/compare.rs:114-126` |
| 零写入审计 | `desktop/src/osu` 内 `WriteProcessMemory\|VirtualProtect\|CreateRemoteThread\|VirtualAllocEx\|SetWindowsHookEx` = 0 命中 | 计划 §7「零写入审计」；B1 现场 0 命中 |

## 4. 兼容 origin：`127.0.0.1:24062`

`desktop/src/server/osu_compat.rs` 在 24062 上"假装成 tosu 的三条端点"（DEC-15/OPEN-02；端口既不与 24050/24060/24061/17653 冲突，也是"将来换成壳内 WS 中继时页面零改动"的固定 origin）：

| 端点 | 行为 |
|---|---|
| `WS /websocket/v2` | 每 150 ms 推一帧（`osu_compat.rs:488`）；**实时模式**推壳内读取线程的最新载荷，`packet()` 为 `None`（未附着/`unhealthy`）时**这一帧不发**；入站消息一律忽略，Close 干净收场 |
| `WS /websocket/commands` | 接受握手（101）后**一帧都不发**（黑洞）——页面每 1 Hz 会连它并请求 `getSettings`，不回包即可（B1 实测 0 帧） |
| `GET /files/beatmap/file` | 200 = 当前谱面 `.osu`（`text/plain; charset=utf-8`）；实时模式下没有当前谱面 ⇒ **404**（绝不回落到别的图）；读盘失败 ⇒ 500 |
| `GET /files/beatmap/background` | 同上，`Content-Type` 由扩展名推定；没有背景字段时 404 |
| 其它路径 | 404 |

- **Host 门禁**：`http::is_local_host(head, 24062)`——只放行 `127.0.0.1:24062` / `localhost:24062` / `[::1]:24062` / 无 Host 头，其余 **403**（`osu_compat.rs:169-173`、`osu_compat.rs:554-558`；raw-socket 实测 `Host: evil.example` ⇒ 403，见 `temp/osu-native-memory/evidence/B1-replay/host-gate-probe.txt`）。
- **ACAO 是硬要求**：**所有**响应（含 403/404/405/500）统一出口带 `Access-Control-Allow-Origin: *`（`osu_compat.rs:188-202`）——页面在 24061 上跨源取封面并 `getImageData`，缺它封面取色静默失效（A10）。
- **WS 的 Origin 校验**：loopback Origin 或**无 Origin**放行，其它 **403**（`osu_compat.rs:464-486`，与 24061 的 `ws.rs` 同一份判定；非 loopback Origin 实测 403）。
- **回放开关（测试用）**：`MMA_OSU_COMPAT_REPLAY=1` 回到 B1 的固定一张真实 `.osu` + 手写 tosu-v2 包（`osu_compat.rs:17-24,78-82`）；**缺省是实时载荷**。
- **绑定失败不 panic、不 exit**：记 error 日志 + 把「osu! 兼容端点端口 24062 被占用，原生传输不可用」推入 `state.errors`，且**不下发** native 端点（`osu_compat.rs:654-679`）。
- 与 tosu 的有意差异（记录在案）：`play.hits`/`resultsScreen.hits` 只供字段表要求的 6 键（真实 tosu 供 10/9 键，多出的页面不消费）；`resultsScreen` 只给 `{hits,mods}`。

## 5. 契约 v6：端点下发与页面切流

### 5.1 壳侧下发

`state` 帧新增 `sources.osu`（`desktop/src/frames.rs:185-220`、`desktop/src/frames.rs:153-168`）：

```
osu: { alive, osuTransport: { mode, host, port, wsPath, filesPath },
       gate?, client?, reason?, degradedFields?, phase?, notice?, progress? }
```

- `alive ≡ (mode == "native")`；`mode:"tosu"` 时端点字段仍在下发（只作诊断），但**页面不应用**。
- 值变化**立即推** state 帧（2 s 一拍的检测），不等 30 s 周期帧。
- `mode` 决策是纯函数（`desktop/src/server/osu_source.rs:95-111`）：

| 入参 | 结果 |
|---|---|
| 24062 未绑定 | `tosu`（立即：绝不下发没人听的端口） |
| 壳配置 `osuTransport:"tosu"` | `tosu`（立即：操作者的逃生开关） |
| 读取器正在发布载荷 | `native`（立即） |
| 刚掉线、且此前健康过（< `READER_GRACE_MS` = **25 s**） | `native`（抗抖动：页面留在我们的端点、保留最后一张好卡片） |
| 掉线 ≥ 25 s 或从未健康过 | `tosu`（回落） |

25 s 这个值是为了覆盖一次冷路径全量扫描（实测 12.6–18.6 s，`osu_source.rs:41-49`）。

### 5.2 页面侧：socket 层运行时切换

- `shellState.applyShellState` 读 `payload.sources.osu`（`js/app/sources/shellState.js:70-79`）。
- `appContext.applyOsuTransport`（`js/app/appContext.js:89-98`）做**唯一**的切换动作：`state.runtimeOsuHost = "host:port"`（或清空）+ `socket.setHost(getSocketHost(), true)`。
- `getSocketHost()`（`js/app/appContext.js:25-31`）是**唯一** host 派生点：WS URL（`socket.js:41`）、`.osu` 端点（`appContext.js:49-51`）、背景图（`coverTheme.js`）全部由它派生，因此一次覆盖即三处跟随（DEC-20）。
- **绝不写 `state.wsEndpoint`**：它在 `SETTING_CACHE_KEYS` 里，走设置路径会每次切换 `clearResultCache()`（DEC-12）。运行时覆盖另存 `state.runtimeOsuHost`，`state.wsEndpoint` 始终是用户设置的权威值 ⇒ **切传输不清结果缓存**（真机实测 `resultCacheGeneration()` 1→1，见计划 Step 9 进度记录）。
- **只接受合格端点（fail-closed）**：必须 `mode:"native"`、必须当前页是**壳页**（`location.port === "24061"`，`appContext.js:43-47`）、host 非空、端口 1..=65535、且 `wsPath`/`filesPath` **逐字等于**页面自己的 `/websocket/v2` 与 `/files/beatmap`；任一不符即拒绝覆盖（仍走 tosu）。浏览器 tosu 页（24050）永不接受覆盖。
- `socket.setHost` 关闭**曾创建过的全部** socket（`this.allSockets` 集合，`js/app/socket.js:13-35`）——`sockets` 以 URL 为键、`/websocket/commands` 可能被打开两次，只遍历 map 会漏掉被覆盖的那条（A11 的修法）。
- **native 位进路由**：`state.shellOsuNativeAlive` 同时用于 L3' 存活回窗（`js/app/sources/sourceManager.js:127`）与 osu 败方门控（`sourceManager.js:186`）——缺它原生帧会被当成"败方帧"静默缓冲，用户可见现象是**换图不更新**（DEC-21 缺陷①）。
- **壳主窗策略**（DEC-21 缺陷②）：壳配置 `osuTransport != "tosu"`（缺省 `auto`）时，主窗**一律**落 `http://127.0.0.1:24061/`；只有显式 `"osuTransport": "tosu"` 才维持旧策略（在线 ⇒ tosu 插件页）——主窗落 tosu 页时端点下发与桥都不可达（`desktop/src/main.rs:42-65`、`desktop/src/config.rs:217-219`）。
- **扫描提示**（Step 9e/9f）：壳把 L0 附着/扫描相位写成 `phase`（闭集 `waiting-for-game` / `attaching` / `scanning` / `healthy` / `unavailable`，`osu/mod.rs:132-161`）+ 英文 `notice` + 实测 `progress`；页面只在自己的元素 `#osu-scan-hint`（`index.html:26`）渲染那句话（`js/app/sources/osuScanHint.js`、纯规则在 `osuScanHintRules.js`）。三条纪律：**只写自己的元素**（借写共享 `#status` 会被别的写手打掉，真机实测只闪 ~300 ms）、**同一相位值内"载荷到过"粘住不复活**、**只在壳页**（浏览器 tosu 页不提示）。真机实测提示连续可见 25 s。

## 6. 能力边界

### 6.1 stable（32 位）

已发布字段 = 计划 §3.3 的完整字段表（本步的字段实现在 `desktop/src/osu/packet.rs`）：`client` / `state.name`（观测集内 6 个状态，见下）/ `beatmap.{id,set,md5,checksum 兼容键,artist,title,version,mapper}` / `files.{beatmap,background,audio}` / `folders.{songs,beatmap,game}` / `directPath.*` / `beatmap.time.{live,firstObject,lastObject,mp3Length}` / `menu.mods` / `play.mods` / `resultsScreen.mods` / `play.hits` / `resultsScreen.hits` / `game.paused`（停滞推导，镜像 tosu，DEC-13）。

- **`state.name` 的名字表只能来自实测观测集**（硬约束：不得从 tosu 源码抄写）：`OBSERVED_STATE_NAMES`（`osu/model.rs:150-161`）登记 `0/menu`、`1/edit`、`2/play`、`4/selectEdit`、`5/selectPlay`、`7/resultScreen` 六个，来源 = P8 扫掠台账（`temp/osu-native-memory/evidence/P8/sweep-20260928b/observations.jsonl`）。观测集外索引 ⇒ 发 `""` + 降级（**不判 unhealthy**，否则会在选歌等日常态冻结）。
- **`firstObject`/`lastObject` 由我们解析 `.osu`**，口径见 DEC-22：首个对象的 `startTime` / **文件顺序最后一个对象的 `endTime`**（不是 `max(endTime)`），都不做 rate 缩放；回退与不变量在 `osu/beatmap_file.rs` + `osu/invariants.rs`。

### 6.2 lazer（64 位）

今天一个健康的 lazer 目标发布：`client:"lazer"` + `state.{name,number}` + `beatmap.{id,set,md5,兼容键,version,artist,title,mapper}` + `files.beatmap` + `folders.songs` + `directPath.beatmapFile` + `beatmap.time.{live,firstObject,lastObject}`；**并逐条上报缺来源的字段**（见 §6.3）。

- 真机对拍（2026-09-29/30，`lazer 2026.921.0.0 / .NET 10.0.12 / x64`）：`client` / `checksum` / `title` / `version` / `artist` / `mapper` / `id` / `set` / `files.beatmap` 与同刻 tosu **逐字节相等**；`state.name`/`state.number` 逐值相等（62 s / 400 帧 / `avg_fps 6.45` / 0 脱附）；`beatmap.time.live` 差 38 ms（独立采样相位）。证据 `.omo/evidence/osu-native-memory-transport/task-10f-build.txt`、`temp/osu-native-memory/evidence/D-contract-v6/D-notes.md` §17–§19。
- **状态映射只对已对拍的态负责**：`selectPlay`（活机对拍）与 `menu`（dump 结构见证）已证；`edit` / `selectEdit` / `play` / `resultScreen` 的类型名在表里、映射规则已写，但**未与 tosu 活机对拍**（D-notes §19.5 末段）。

### 6.3 lazer 已知缺口（7 条 `degradedFields`，机制原因逐条记档）

| 载荷字段 | 降级原因字面量 | 机制原因 |
|---|---|---|
| `menu.mods` / `play.mods` / `resultsScreen.mods` | `offsets-missing-ScoreInfo.ModsJson` | `Mod.acronym` **不是字段**（IL 全库无该字段）；参照实现读的是 `ScoreInfo.<ModsJson>` 的 JSON 串，而那份 dump（主菜单态）没有 `ScoreInfo` 活对象 ⇒ 需要**一次带 mod 的选歌/游玩态 dump** |
| `play.hits` / `resultsScreen.hits` | `offsets-missing-score-chain` | 计数域在 `ScoreInfo.<StatisticsJson>` / `ScoreProcessor`；同理需要游玩态 dump |
| `files.background` / `files.audio` | `offsets-missing-BeatmapSetInfo.Files` | 需要 `BeatmapSetInfo.<Files>` 列表的**元素**（`filename` + `hash`），而 `dumpobj` 对数组只回 `Fields: None` ⇒ 需要"数组元素布局"的表格式扩展（`dumparray` 见证） |

⇒ lazer 上 `modSignature` **只在 NM 时**与 tosu 逐字节相等（带 mod 的图差在 mods 段），`isInPlayState` 与图表时间窗在 lazer 上可用（`state.name` + `.osu` 解析都在），hits/livePP 面不可用。这是"**绝不编造**"的取舍（缺字段 = 不出现在载荷里 + 上报原因，绝不发 0/占位/沿用上一帧）。

### 6.4 明确不做

| 项 | 结论 |
|---|---|
| Linux 内存读取 | 不做（DEC-03；`platform-unsupported`） |
| "已验证 exe 哈希"白名单硬门 | 不做（DEC-05/REJ-02）；`unsupported-build:<hash>` 只是**人工诊断标签**，没有任何哈希白名单会让它自动触发 |
| 第五来源（`osun:` identity / 独立帧型） | 不做（REJ-03） |
| 壳内 WS 中继（页面永远只看壳） | 暂缓（REJ-07，DEC-06 的升级路径；固定 24062 就是为它留的） |
| lazer 官方 IPC / WS 取状态 | 不做（REJ-05：实测无可用出口） |
| `paused` 的"更准语义" | 不做，先镜像 tosu 的停滞推导（DEC-13 / OPEN-05） |

## 7. 失败语义

### 7.1 reason 闭集（11 个字面量，唯一权威 `desktop/src/osu/model.rs:20-73`）

| 字面量 | 语义 | 触发面 |
|---|---|---|
| `process-not-found` | 没找到唯一目标进程 | 发现层 |
| `multiple-instances` | 同一位数下有多个候选 | 发现层 |
| `client-ambiguous` | 分不出客户端（PE machine 非 0x014C/0x8664） | 发现层 |
| `signature-miss:<key>` | 某枚**必需**锚点未命中（key = `patterns::Anchor::key`） | L0 |
| `unsupported-build:<hash>` | 人工诊断标签（非自动门） | 人工标注 |
| `lazer-offsets-missing:<ver>` | lazer 偏移表缺失/不可用 | L0（lazer） |
| `access-denied` | `OpenProcess` 被拒或某段内存读不动 | 发现/读取层 |
| `platform-unsupported` | 非 Windows | 平台层 |
| `read-error` | 其它读取失败（含短读） | 读取层 |
| `invariant-failed:<field>` | L1/L2 硬失败（field = 不变量字段名） | L1/L2 |
| `shadow-mismatch:<field>` | L3 影子比对连续不一致（升级后回落时用） | L3 |

- 前三条是**进程选择**类原因（`Reason::is_discovery`，`model.rs:93-98`）：**新附着**时清掉（它们描述的是上一次发现时的进程集合）；读取/定址类原因**不清**，只能靠"重解析 + 清白帧"退出。
- `signature-miss:<key>` / `lazer-offsets-missing:<ver>` 的值变化**立即推** state 帧（`sources.osu` 的值变即推），不等 30 s 周期。

### 7.2 `degradedFields`

形状 = `<载荷字段>:<原因>` 的字符串清单（`desktop/docs/CONTRACT.md` §8；内容说明见 §8 的 `degradedFields` 段）。来源两条：① 读取器每帧的字段级降级（lazer 侧 = `lazer.rs::read_frame` 的 `gaps`：表缺口 `offsets-<LookupError>`、链读失败 `read-<跳>`、字符串布局 `string-layout`、形状不符 `shape`，以及 `<类型名>-not-in-observed-set` 这类映射缺口）；② 整表缺失时的"因此没有来源"清单（`lazer::missing_table_degraded_fields()`，随 `lazer-offsets-missing:<ver>` 一起下发）。两者都经 `invariants.rs::evaluate` **原样并入**门的上报（顺序保持、逐条去重）。页面只按"键在不在/前缀"消费，不解析原因里的数字。

### 7.3 逐状态的帧与文件路由行为

门的输出是一个**闭集动作**（`FrameAction`，`osu/invariants.rs:869-898`）：`publish` / `freeze-state-only` / `hold-last-good` / `stop`。

| 情形 | WS 帧 | 两条文件路由 |
|---|---|---|
| 正常 | 全量载荷（可能带 `degradedFields`） | 200 = **本帧**发布的谱面 |
| **字段级冻结**（DEC-18：硬失败累计但未到 `unhealthy`） | 只发 `client` + `state`（**省略 `beatmap`**） | 404 |
| **身份保持**（Step 9g：冻结由身份指针 `beatmap.object` 的**瞬态**失败引起，且距本次冻结里第一次这类失败 < `IDENTITY_HOLD_GRACE` = 2.5 s） | 发**最后一张好图**的 `beatmap`/`files`/`directPath`/`folders` + **本帧** `state` | 200 = **同一张图**的文件（身份逐字一致才供奉） |
| 保持期内出现**已对拍**的换图（内存 md5 == 磁盘 `.osu` 的 md5） | 立刻 `publish` 新图（保持旧图 = 已知的错误答案） | 200 = 新图 |
| `unhealthy`（硬失败到 `INVARIANT_STRIKES`，或 L0 失败） | **一帧都不发** | 404 |

为什么要字段级冻结而不是整包停发：页面 L1 的 `isInPlayState` 靠 `state.name` 维生，60 s 的 L2 窗口一过期就把路由甩离 osu（DEC-18）。为什么要身份保持：页面把一切都键在 identity 上，扣留它会触发重抓 ⇒ 把 24062 的 404 原样渲染成用户可见的错误（结算界面那条 `Request failed with status 404`，Step 9g 的真机现象）；保持的是**同一张图上一次验证过的值**，既不可见、也不可能编造出另一张图。文件路由另有 `held_fresh` 判据（距读者最后一次处理帧仍在 2.5 s 内），窗口过后逐字回到 404 —— 那段时间里"最后一张好图"已是无界陈旧值（`osu/packet.rs:128-174` 的 7 行规则表）。

### 7.4 页面侧对瞬态失败的处置

- **抓谱面文件失败只对壳原生端点重试一次**（约 1 s）：`js/app/sources/beatmapFetchRetry.js` 的重试状态闭集 = `{404, 500, 502, 503, 504}`；`403`（Host 门禁）/`405`（方法）不重试（语义错误，重试只是白等）；**tosu 通道（无壳 / `mode:"tosu"`）逐字节不变**（native 位未生效 ⇒ 恒不重试，错误照旧原样抛出）。重试前后各判一次 stale 守卫，被取代的请求不重试、不返回。
- 真机结果：结算界面的 gate 行从 400 行降到 **2 次 `action=HoldLastGood`**，404 从页面消失（计划 Step 9 进度记录；复测方法见 `D-notes.md` §14.7）。

## 8. 健康状态机（L0–L3）

四层门（DEC-17）：**L0 锚点解析**（附着时/进程变化时/失败刷新时）→ **L1 结构校验**（每次读：整块读满 + 候选自证 + 指针合理性）→ **L2 不变量**（`osu/invariants.rs` 的 I-01…I-10；无状态者每帧判、时序性者只在状态迁移那一帧判）→ **L3 影子比对**（tosu 在线时，`osu/shadow.rs` 纯函数 + `osu/compare.rs` IO 驱动）。

四态（`invariants.rs:847-865`）：`idle`（没目标）/ `healthy` / `degraded`（出帧 + 上报缺字段）/ `unhealthy`（不出帧）。

门常数（`invariants.rs:38-98`，实现期不得再选）：

| 常数 | 值 | 语义 |
|---|---|---|
| `INVARIANT_STRIKES` | 3 | 连续硬失败次数达到即 `unhealthy`（250 ms 一拍 ⇒ 亚秒级停帧） |
| `RECOVERY_CLEAN_FRAMES` | 20 | 恢复所需的连续清白帧（≈5 s @250 ms） |
| `FREEZE_WINDOW` | 30 s | 冻结的**有界窗口**；到期强制重解析锚点（不得靠"值看起来合理"退出） |
| `BACKOFF_SECONDS` | 2/4/8/16/30 | L0 连续失败后的退避阶梯（最后一档封顶） |
| `IDENTITY_HOLD_GRACE` | 2.5 s | 身份保持窗口（§7.3） |
| `HOLDABLE_HARD_FIELDS` | `beatmap.object` | 唯一可保持的硬失败字段 |
| `MIN_DWELL` | 10 s | **已定义但当前无消费者**（全壳 grep 只有定义处）——"切换推迟到非 play 态"由页面路由与抗抖动窗口承担 |

退出冻结的三条路（`invariants.rs:25-31`，都得记 reason）：① 锚点重解析成功 + 随后 20 帧全清；② 有界窗口到期强制重解析；③ 原因**本身已失效**（新附着清掉进程选择类原因）。

附着/重连的时间常数（`osu/mod.rs:60-120`）：附着失败重试 2 s；兜底 PID 扫描失败 30 s；**曾附着过的目标刚丢**的 30 s 窗口内 1 s 一拍（快重连）；其后 2/4/8/16/30 s 退避。

相位机（`osu/mod.rs:132-201`）：`waiting-for-game`（从没附着过且原因就是"进程不在"）/ `attaching`（目标丢了正在重附着）/ `scanning`（已附着、锚点未定址）/ `healthy`（L1+ 正在出帧）/ `unavailable`（锚点已定址但门停帧）；`notice` 只在会带提示的三相非空，`progress` 只在 `scanning` 出现。**提示与进度只在"原生传输结构性可达"时下发**（24062 已绑定且操作者没强制 tosu）：判据**不是** `mode`（冷启动时 `mode` 恰是 `"tosu"`，而那一刻页面正需要这句提示）。

## 9. 操作手册 A：如何定位一次锚点失效

**现象**：卡片停更/源点变灰；`sources.osu.reason` 出现字面量（页面状态行与壳日志都会给出）。

1. **先看 state 帧与日志里的 reason 字面量**（§7.1）。壳侧日志的稳定前缀（按日志顺序）：
   - `[osu] no target: …`（发现层；后跟 `[osu] attach retry in …ms`）
   - `[osu] anchor scan failed (attempt N): signature-miss:<key>` / `[osu] lazer L0 failed (attempt N): …`
   - `[osu] scan <key> -> N hit(s) chosen=Some("0x…")`、`[osu] diag <key> candidate=0x… reject=<slot|object|ruleset|vtable> … why=…`（候选被哪条判据拒掉）
   - `[osu] scan filter#N regions=… bytes=… elapsed=…ms unresolved=["statusPtr", …]`（两档过滤器的扫描预算与未解析项）
   - `[osu] gate: state=<…> action=<…> transition reason=<…>`（门迁移；`freeze-state-only`/`stop`/`hold-last-good` 都在这里）
   - `[osu] phase=<相位> …`（相位跳变；跳进 `healthy` 的那一行带刚结束的扫描实测 `scan_ms=`/`filter=`/`regions=`/`bytes=`）
2. **拿 reason 里的 key 去台账对号**：`desktop/src/osu/patterns.rs` 的 `ANCHORS`（stable，7 枚）或 `tools/lazer-offsets-gen/spec.rs` 的锚点/跳序（lazer）。台账每枚都写了 `derivation`（为什么这串字节能定位该全局量）与 `evidence`（当时怎么证实的），**按推导重新推导一次**比重扫更重要——签名失配通常意味着这次更新动了那段代码。
3. **区分"签名没命中"与"签名命中但语义漂了"**：前者是 `signature-miss:<key>` + 日志里 `0 hit(s)`；后者表现为命中 1 次却反复被候选自证拒（`reject=slot/object/ruleset/vtable`），或以 `invariant-failed:<field>` 冻结——这正是 L1/L2 存在的理由。
4. **lazer 先查表**：`lazer-offsets-missing:<ver>` ⇒ 走 §10 重建表；日志里 `[osu] lazer offsets: FALLBACK TABLE …` 说明在走就近回落（表键与目标版本不同），而**没有**这一行 = 精确命中。
5. **用破坏性签名测试确认"停帧而不是发假数据"**（验收口径，C2 现场）：变体 A（改锚点模式的一个字节）⇒ `[osu] scan statusPtr -> 0 hit(s)`、门报 `signature-miss:statusPtr`、**0 帧伪造值**；变体 B（把状态索引 `+100`，模拟"读到垃圾"）⇒ 先 `action=FreezeStateOnly transition reason=invariant-failed:state.name`（冻结帧仍带 `state.name`），再到 `action=Stop` + `unhealthy`；解除锚点到 `unhealthy` 约 **1.4 s**。逐字日志：`temp/osu-native-memory/evidence/C2-stable-full/corruption-test.txt`（另有 `corruption-A.jsonl` / `corruption-B.jsonl`）。
6. **修复后的判据**：日志出现 `[osu] anchors resolved in …ms: statusPtr=… baseAddr=… …`（stable）或 `[osu] lazer: L1 proof passed (structural) — gameBase=… anchors=1 scan_ms=…`，随后 `phase=healthy`，且 `sources.osu` 回到 `{alive:true, mode:"native"}`，`degradedFields` 里不再有该字段。

## 10. 操作手册 B：如何重建 lazer 偏移表

完整手册在 `tools/lazer-offsets-gen/README.md`（工具已入库，DEC-23）。端到端命令序列（lazer 正在跑，Windows x64）：

```powershell
# 0) 构建（仓库根目录；裸 rustc、零依赖、--edition 2021 必需）
rustc --edition 2021 -O -A dead_code -o temp/lazer-offsets-gen.exe tools/lazer-offsets-gen/main.rs

# 1) 先看清要执行的命令（不碰游戏）
temp/lazer-offsets-gen.exe collect --out %TEMP%\lazer-offsets-gen\run1 --dry-run

# 2) 采 dump（~70 s 游戏挂起、~3 GB；见下面的代价与同意要求）
temp/lazer-offsets-gen.exe collect --out %TEMP%\lazer-offsets-gen\run1

# 3) 离线提取：扫锚点 → 站点 → 多跳解出 GameBase → dumpobj 逐级解引用 + dump 字节解引用自证
#    （已有表时可加 --table <表.json> / --expect-vtable <hex> / $MMA_LAZER_OFFSETS 一起验候选）
temp/lazer-offsets-gen.exe extract --dump %TEMP%\lazer-offsets-gen\run1\lazer-<ts>.dmp --out %TEMP%\lazer-offsets-gen\run1

# 4) IL 结构清单（读 lazer 安装目录的托管程序集；--diff 与上一次清单比较字段增删改）
temp/lazer-offsets-gen.exe il --sos %TEMP%\lazer-offsets-gen\run1\sos-intermediate-<ts>.tsv --out %TEMP%\lazer-offsets-gen\run1 --diff <上一次的 il-inventory.tsv>

# 5) 双见证校验 + 出表，并直接部署到壳 exe 旁
#    ⚠ 自包含发布的 osu!.dll/osu!.exe 文件版本是占位 0.0.0.0 ⇒ 这个构建上 emit 会先拒一次，
#      按提示加 --allow-version-mismatch（不一致会写进表里的 evidence）
temp/lazer-offsets-gen.exe emit --sos %TEMP%\lazer-offsets-gen\run1\sos-intermediate-<ts>.tsv `
                                --il  %TEMP%\lazer-offsets-gen\run1\il-inventory-<ts>.tsv `
                                --deploy "<放着 mma-shell.exe 的目录>" --allow-version-mismatch
```

- **dump 采集的代价与挂起**：`dotnet-dump collect` 会**短暂挂起游戏的全部线程**并 spawn `createdump` 子进程（不注入、不写内存、不挂调试器）；本机实测一次 **约 70 s**（最近一次 dump 2.696 GB / pid 22028），落盘前需 ≥4 GB 空闲（`--force` 可跳过检查）。因此采集**只在 dev-time、只在用户同意且在场时**执行；产品内一律走 `ReadProcessMemory`，工具不随发布产物分发。
- **运行位置**：在受限的工作区上下文里 Toolhelp32 只能看到自己那一族进程（表现为"游戏没在跑"）——**从工作区外（如 `%TEMP%`）或普通 shell 启动**即可正常发现游戏（与壳自身的 B2 结论同族）。
- **两见证规则（出表的硬判据）**：每个 offset 必须同时有 ① **SOS 行**（`dumpobj` 打印的 `Offset`，来自真实 dump，中间件里带 transcript 与逐字命令）**且** ② **IL 结构行**（同一字段在 lazer 安装目录的托管程序集元数据里的存在性 + `instance`/`static` + 字段类型 + 显式布局偏移）；第三个见证是 **dump 字节解引用**（引用类型比指针、原始类型比内容、结构体比"字段自身地址 = 对象地址 + Offset"——它同时机械证明了"SOS 的 Offset 基准 = 对象地址"）。两边对不上 ⇒ **丢弃该字段**并在 `emit-report-*.txt` 里逐条列出，绝不"取其一"；元数据推导的偏移**永不作为偏移发布**。另有拒绝门：fixture 未显式放行、`provenance` 非 `dump`、`deref_checked=0`、模块版本不一致（可 `--allow-version-mismatch` 放行但写进 `evidence`）。
- **版本键**：表按 **`(lazer 版本, runtime 版本, 架构)`** 建键，文件名 `<lazer>__<runtime>__<arch>.json`（本机 = `2026.921.0.0__10.0.12__x64.json`）。runtime 版本变了 ⇒ **新表另存一份**，旧表保留（回落梯子靠它）；`arch` 不同 ⇒ **永不回落**（`offsets.rs` 的架构不匹配不回落）。
- **表落点**：`<壳 exe 目录>\offsets\<client>\<同名文件>`（以及兼容旧路径 `<壳 exe 目录>\lazer-offsets\`；也可用 `$MMA_OFFSETS_DIR` 或 `$MMA_LAZER_OFFSETS` 指向显式位置）。在 P2 中，`desktop/release.ps1` 会自动编译随包分发的 `gen.exe` 并完整拷贝 `offsets/` 目录与签名 `manifest.json`，用户亦可通过设置页一键活体生成或检查远端更新。
- **自测**：`temp/lazer-offsets-gen.exe self-test --fixtures tools/lazer-offsets-gen/fixtures --work temp/lazer-offsets-gen-selftest`（不需要 lazer / dump / 分析器；当前 **35/35**），跑的是真实代码路径（合成 minidump → 锚点扫描 → 链走法 → dump 字节自证 → SOS 中间件装配 → emit 双见证）。
- **刷新前先看差分**：`il --diff <上一版清单>` 的 `ADDED/REMOVED/CHANGED` 段就是"这次更新到底变了什么"，再对照 `emit-report` 的 `omitted` 清单。要发布新字段时改 `spec.rs::WANTED` 后**只重跑 `emit`**（中间件里存了每个被 dump 对象的全部字段行，不必重采 dump）。

## 11. 自检清单（维护者按顺序跑）

| # | 检查 | 命令 | 判据 |
|---|---|---|---|
| 1 | 壳构建 | `cd desktop; cargo build` | 0 warning |
| 2 | 壳单测 + 基线比对 | `cd desktop; cargo test` | 通过数只能增加；**失败名集合**与 `temp/osu-native-memory/evidence/B1-replay/cargo-test-BASELINE-stashed.txt` **逐名相同**（36 项，全部是沙箱 `temp_dir` `PermissionDenied` 的既有环境失败；新增失败 = 本步引入）。提取名字比对：`Select-String -Path <日志> -Pattern '^test (.+) \.\.\. FAILED'` 取两侧集合作对称差，应为空 |
| 3 | osu 家族单测 + 全量读数 | `cd desktop; cargo test osu::`；`cd desktop; cargo test` | osu 家族全绿；全量 = **382 passed / 36 failed / 1 ignored**（2026-09-30 复跑的实测值；failed 全部是上面那 36 项） |
| 4 | 影子对拍（tosu 在线） | `cd desktop; $env:MMA_OSU_COMPARE="1"; $env:MMA_OSU_COMPARE_INTERVAL_MS="100"; $env:MMA_OSU_COMPARE_OUT="$env:TEMP\samples.jsonl"; cargo run --release` | `samples.jsonl` 每行有 `cmp`；第一断言 = `cmp.identity` / `cmp.mod_signature` / `cmp.state_name` 逐字节相等；不等/跳过只在 `diff` 里列；`"skipped:<why>"` **不是** `false`。关整套装置用 `MMA_OSU_COMPARE=0`；`MMA_OSU_TOSU` 换 tosu 端点；`MMA_OSU_COMPARE_FULL=1` 恢复整包落盘 |
| 5 | 端点探针（HTTP） | `Invoke-WebRequest http://127.0.0.1:24062/files/beatmap/file -UseBasicParsing` | 200 + `Access-Control-Allow-Origin: *`；body = 当前谱面 `.osu`（`text/plain; charset=utf-8`） |
| 6 | Host 门禁 | raw socket（或 `curl -H "Host: evil.example"`）请求 24062 | **403** `{"error":"forbidden host"}` 且带 ACAO；`127.0.0.1:24062` ⇒ 200 |
| 7 | 端点探针（WS） | `node -e` 用内置 `WebSocket` 连 `ws://127.0.0.1:24062/websocket/v2`（Node 24 有全局 WebSocket） | 101；实时模式 ≈6.45 帧/s（真机 `avg_fps 6.45`）；回放模式（`MMA_OSU_COMPAT_REPLAY=1`）32 帧/5 s、帧间隔 153–158 ms；`/websocket/commands` 101 且 0 帧 |
| 8 | 契约版本三处一致 | `grep`：`desktop/src/frames.rs`、页面 `js/app/sources/bridgeClient.js`、`desktop/docs/CONTRACT.md` | 三处都是 `6`，页面接受区间 `[3,6]` |
| 9 | 破坏性签名测试 | 按 §9 第 5 条复制源码到工作区外打补丁、编译、跑 | 变体 A ⇒ `signature-miss:statusPtr` + 0 伪造帧；变体 B ⇒ 冻结帧（带 `state.name`、无 `beatmap`）→ `unhealthy` 停帧，解除锚点到 unhealthy ≈1.4 s |
| 10 | lazer 表存在性与自检 | `Get-Content "<壳 exe 目录>\offsets\lazer\<ver>__<rt>__<arch>.json" -TotalCount 12`；`tools/lazer-offsets-gen` 的 `self-test` | `game_base_vtable` 与 `types` 都非空；自测 35/35 |
| 11 | 零写入审计 | `grep -rn "WriteProcessMemory\|VirtualProtect\|CreateRemoteThread\|VirtualAllocEx\|SetWindowsHookEx" desktop/src/osu` | 0 命中 |
| 12 | 页面语法 | `Get-Content -Raw -Encoding utf8 <file> \| node --input-type=module --check` | exit=0（⚠ 必须用 `--input-type=module --check` 走 stdin：Node 24 的 `node --check <file>.js` 在模块语法推断路径上会**假绿**，见 `.omo/evidence/osu-native-memory-transport/task-9-build.txt` §4） |
| 13 | 版本同步 | `grep`：`ManiaMapAnalyser by Leo_Black/index.js` 的 `_VERSION` 与 `metadata.txt` 的 `Version` | 两者**逐字相同**（当前 `2.2.0`）；`cargo test` 与页面语法检查同时通过 |

## 12. 已知限制与未完成（如实标注）

- **stable 锚点/位移是构建相关**：本机台账固定 `osu!.exe` MD5 `f845ef10bf97c3260b818fc02e73c196`（32 位、FileVersion 1.3.3.8）；换构建就得重扫并按 reason 定位（可使用维护者探测工具 `tools/stable-offsets-gen` 自动提取）。
- **lazer 表是构建相关的**：表键不同 ⇒ 就近回落（**仅在 L1 结构证明通过时**，且大声记日志），或者通过生成器/远端更新生成新表，否则缺表回落 tosu；`arch` 不同一律不回落。
- **lazer 的 7 条字段缺口**（§6.3）需要一次**带 mod 的选歌/游玩态 dump** 与一次表格式扩展（数组元素见证）才能关；在此之前 lazer 的 `modSignature` 只在 NM 时与 tosu 逐字节相等。
- **lazer 状态映射**只对 `selectPlay` / `menu` 有真机对照（§6.2）；其余四态的类型名与规则已就位但**未核实**（需用户走到那些态时用同一条探针复测）。
- **首次附着/锚点失效后的检测时间由扫描决定**：stable 冷启动首次全量扫描实测 7–15 s、破坏性测试现场 11.2 s；lazer 冷路径 12.6–18.6 s（`scan_ms=14426` 为其一例）——这期间页面靠 `#osu-scan-hint` 的提示而不是假数据。
- **`MIN_DWELL` 未接线**（§8）；**保持期内的图表读数是上一份的**（`state.name` 实时，`beatmap.time.live`/hits 是最后一张好图的值，≤2.5 s 或到恢复完成为止，结算界面不可见）。
- **tosu 4.25.1 的已知缺陷（我们的有意偏离）**：它在非结算态发布垃圾 `resultsScreen.mods`（观测 `number=1529628200`），页面在非游玩态取三槽并集 ⇒ tosu 模式下 NM 图会被算成 DT；我们的 native 路径只在结算态发布它（更正确但与 tosu 不同）。见计划 Step 9 进度记录与 `D-notes.md`。