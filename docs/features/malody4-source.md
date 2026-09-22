# Malody 4.3.7 原生客户端数据源（malody4）

> 面向 AI 的技术文档。人类安装/使用说明见 [docs/shell-guide.md](../shell-guide.md)；多源整体架构与路由见 [multi-source.md](multi-source.md)；壳侧契约见 `desktop/docs/CONTRACT.md`（v3）；动态 OD 表见 [malody4-od.md](malody4-od.md)。

## 1. 定位

`malody4` 是本插件的**第四个**数据源，对应 **Malody 4.3.7 原生 32 位 C++ 客户端**（不是 Malody V——那是独立客户端、独立源 `malody`、独立 identity 前缀）。

接入方式是**零注入的只读观察**：壳（`desktop/`）从进程外读游戏的三条信号，**不向游戏目录写入任何文件**、不注入 DLL、不用 BepInEx、不用 Lua、不 hook、不修改游戏内存。这条约束是本源能成立的前提：游戏侧**无需任何安装操作**，安装器（`bridges/install-bridge.ps1` 的第三个游戏项 `Malody 4`）只往 `mma-shell-config.json` 写一个 `malody4Root` 键，安装前后游戏目录哈希逐字节相同。

平台：**仅 Windows**（`ReadProcessMemory` 与 PE 版本校验都是 Win32 语义）；其他平台整条通道以 `platform-unsupported` 优雅不可用，不影响其余三个源。

## 2. 三条只读观测信号

| 信号 | 采集方式 | 得到什么 |
| --- | --- | --- |
| 锚点身份键 | `ReadProcessMemory`：模块基址 + 固定 RVA `0x8310EC` 取出一个指针，再读该指针指向的 ASCII 串 | 形如 `<32 位小写 md5>_<slot>` 的**游戏自身身份键**（`slot` 只做诊断，不进 path/hash/identity） |
| 游戏日志 | tail 最新的 `log-*.txt` 的新增行 | 场景：`[MS] LOG: switch to N` 的 `N` 映射 `1/2 → selection`、`3 → playing`、`4 → result`、其余 `other`；`switchback` / `switch back`（回退）与 `switchbefore to N`（预声明）**不算切换** |
| `config.json` | 周期轮询（读盘） | 只取 `user_mods`（变速位 `DASH 0x10` / `RUSH 0x20` / `SLOW 0x100`，互斥；多位同时命中按 `DASH > RUSH > SLOW` 取值并记一次日志）与 `user_judge_level`（`0..=4` → `A`~`E`，越界 = 判定未知、不猜档位） |

轮询节奏：主循环 200ms 一拍；`malody4_selection` 帧的心跳间隔 2s，换锚点/换场景有 300ms 防抖；`playing` 的"新鲜"窗口为 10s（日志目录消失等异常下 `playing` 自然失鲜，而不是永驻）。

**版本门（重要）**：附着前校验目标 PE 的 `TimeDateStamp == 0x5D79AC91` **且**文件大小 `4,750,848`（版本表在 `desktop/src/malody4/anchor.rs` 里只出现一处）。任一项不符 → **整条通道不可用并给出 `target-mismatch:*` 原因**，绝不"按偏移量猜着读"——RVA 属于特定二进制，猜错会把任意内存当成 md5。

隐私：`config.json` 里还有 `user_id` / `token` / `sound_hash` 等字段，壳**只读上面两个字段**，其余一律不进结构体、不进日志、不落盘、不提交；测试夹具全部为合成数据。

## 3. 谱面库索引与身份

- **遍历范围**：`<root>/beatmap/**`，递归深度上限 8 层。
- **准入判据**：只索引 `.mc`，且该文件自己的 `meta.mode == 0`（Key 模式）且 `meta.mode_ext.column` 存在；`__MACOSX` 目录、`._*` 文件、symlink/reparse point 一律跳过（跳过计数进日志，不静默）。
- **哈希**：逐文件单趟流式 md5（64 KiB 块喂 `Md5`，绝不整份读进内存），同时保留前 64 KiB 前缀**手工括号配对**提取 `meta`（不整份 JSON 解析）。
- **重复 md5**：确定性取"相对路径字典序最小"的那一份，其余计数进日志（同一张谱在库里有多份副本时结果稳定可复现）。
- **identity**：`mdy4:{md5}`。`mdy4:` 与 Malody V 的 `mdy:` 是**两个不同源**的独立前缀，互不命中；`md5` 是游戏自身的身份键，**免疫改名与曲名变动**。
- **`.mc` only 的有意偏离**：外部规格建议"只索引 `.mc`，`.osu` 亦可"，本实现**不索引 `.osu`**——因为 `.mc` 自己的 `meta.mode == 0` 才是 Key 模式判据，而 `.osu` 没有等价字段，收进来会把 osu! 的其它模式谱面一并放进索引。代价：库里的 `.osu` 谱面永远不会被跟随（见 §7 已知限制）。
- 索引进度：索引未构建完成前整源按"未就绪"处理（`no-library`），构建完成后后台线程独占写入、主循环只读。

## 4. 帧与契约（v3）

契约版本从 v2 升到 **v3**（`desktop/docs/CONTRACT.md`），本源的帧变化是这次升级的内容之一：

- 新增 `malody4_selection` 帧（帧集合由 **7 型扩为 8 型**）：`{path, speed_rate, screen, sequence, event, version, chart_hash, source}`，`event ∈ anchor-changed / scene-changed / heartbeat / hidden`，`source` 恒为 `malody4-native`。
- `song` 帧 `source` 枚举新增 `"malody4"`；`requestId` 新增 `n{seq}` 前缀（锚点轮询通道）。
- `song.meta` 新增可选 `judge`（判定档字母 `A`~`E`；缺省 = 判定未知，页面回落 C 档）。判定档随 song 帧下发，判定已进页面签名 → 改判定即触发一次重发（下一 tick 刷新 OD，**不必等下次换谱**）。
- `state.sources` 新增 `malody4{alive, playing, screen, reason?, judge?}`：`alive` = 本 tick 是否成功附着到唯一 `malody.exe` 并读到锚点；`playing` = 场景为游玩且新鲜（≤10s）；`screen ∈ selection / playing / result / other`（仅在已知时出现）；`reason` 与 `judge` 健康时不出现。

**`hidden` 不携带原因（与参考桥的有意偏离）**：参考桥把原因写进 hidden 记录里，本实现让 hidden 记录的五个字段全空、`event = "hidden"`，原因另经 `sources.malody4.reason`（30s 周期 state 帧）与壳日志给出。理由：hidden 是 2s 心跳级的高频帧，而原因集合是闭集且变化远慢于心跳；把原因塞进每一条 hidden 只会在线上重复同一句话，且会让"原因从哪来"有两个权威（记录 vs state 帧）。代价：只看 selection 帧的消费者拿不到原因，必须同时消费 state 帧。

**`reason` 闭集（12 个字面量，逐字引用 `desktop/docs/CONTRACT.md` §8；不在此处改写）**：`chart-not-indexed`、`process-not-found`、`multiple-instances`、`access-denied`、`bad-read`、`target-mismatch:pe_timestamp_mismatch`、`target-mismatch:file_size_mismatch`、`target-mismatch:pe_header_out_of_range`、`target-mismatch:unknown`、`root-not-configured`、`no-library`、`platform-unsupported`。其中 `root-not-configured` **只表示整条解析链走完仍为 `None`**（见 §5），不是"用户没配"的同义词。

**判定未知时保守 withhold**：若此刻 `config.json` 不存在/解析失败导致判定无法确定，则该 tick **不发 song 帧**（记一次日志，同因由去重），等下一 tick 读到完整配置再发。理由：把 `judge` 当 `null` 发出去会让页面回落 C 档 8.08，等于把"读到半截配置"变成一个静默的错误 OD。该 withhold 不影响 selection 帧的 2s 心跳。

## 5. 根目录解析链（唯一权威顺序）

壳侧 `server::malody4_root` 按顺序取第一个非空值：

1. **正在运行的进程目录**：poller 从 `anchor::find_target()` 拿到 exe 路径，只要求同目录存在 `malody.exe`（**不做版本校验**——这样"版本不符"才会由锚点打开如实产生 `target-mismatch:*`，而不是被伪装成"找不到进程"）；
2. 环境变量 `MMA_MALODY4_ROOT`；
3. 壳配置 `mma-shell-config.json` 的 `malody4Root`（安装器自动写入的键）；
4. tosu 设置文件的同键；
5. **启发候选列表**（`config::detect_malody4_root`）——**唯一做版本校验的一级**（它可能撞上 MalodyV / Maupdate 目录），且只在三重校验通过时才采纳；候选是**绝对路径**，且带盘符就绪预检与 30s TTL 缓存。

前四级一律"非空即采纳"，不做存在性/版本校验。**"留空 `malody4Root`"不是关断**：末级启发候选仍可能自动采纳一个通过的目录。要真正关断，把 `MMA_MALODY4_ROOT` 指向一个不存在的路径（非空即采纳 ⇒ 之后稳定为 `process-not-found`），或移走/更名游戏目录让三重校验失败。

## 6. 页面侧路由与状态

- 优先级 `PRIORITY = ["osu", "etterna", "malody4", "malody"]`（`js/app/sources/sourceManager.js`）。
- **L1**：壳 state 帧报告 `malody4.playing`（场景 3 且新鲜）即抢占。
- **L2**：60s 新鲜事件窗口，**只由"真的选中了谱面"的 selection 帧续约**——`payload.path` 非空且 `event !== "heartbeat"`。hidden 帧与心跳**永不续约**；否则壳只要在跑（hidden 心跳不停）就会把路由永久钉在 malody4，饿死 osu/Etterna。
- `state.malody4Alive` 由 selection 帧新鲜度派生（6s 窗口 ≈ 心跳 2s + 容忍丢一拍），是**诊断位/圆点用**的值，`decide()` 不读它；`malody4Screen` / `malody4Reason` / `malody4Judge` 同为诊断位（`js/app/sources/shellState.js`）。
- 源圆点第四色 `malody4 = #22d3ee`（亮青），与 `#ff66aa`（osu!）/ `#a855f7`（Etterna）/ `#3b82f6`（Malody V）**四色可区分**；Malody 4 与 Malody V 是两个独立源、两个独立圆点颜色。
- 强制锁定：`gameClient` 取 `Malody 4`（亦接受 `malody4` / `malody-4`）；精确匹配在 `startsWith("mal")` 之前，避免 Malody 4 被折叠成 Malody V。

## 7. 已知限制与未做项（如实标注）

- **判定 OD 动态化仅覆盖 PC 端窗口**：判定档 × 速率 → 等效 OD 的换算与 `FAIR` 模组不建模的限制见 [malody4-od.md](malody4-od.md)。
- **screen 显示门控未做**：`screen` 只进 state 帧与诊断，卡片不会因"当前是结算/选曲场景"而隐藏或清空。
- **只索引 `.mc`**：库里的 `.osu` 谱面不会被跟随（§3 的理由与代价）。
- **单一库、单一实例**：只支持一个 `malody4Root` 下的一个 `beatmap/`；多个 `malody.exe` 同时运行时整条通道不可用（`multiple-instances`），不"选一个猜"。
- **只支持 4.3.7 这一个二进制**：其他版本（含 4.3.x 的其他构建）一律 `target-mismatch:*`；新增版本 = 往 `KNOWN_CLIENTS` 加一行并重新取证。
- **不做原生 MSD、不做暂停检测 / livePP**：非 osu 源一律不提供（与 Etterna/Malody V 口径一致）。
- **不需要管理员权限**：同用户权限读取自己启动的游戏进程即可；被系统策略挡住时如实报 `access-denied`。

## 8. 诊断工具

`tools/malody4-anchor-check/`（裸 `rustc` 单文件、直接注入壳里的同一份 `anchor.rs`，故版本常量在仓库里只有一份）：把"找进程 → 校验 PE 版本 → 读锚点指针 → 解析身份键"逐环打印，一行命令即可看出断在哪一环；`--exe <path>` 做**离线 PE 版本校验**（不需要游戏在跑，`--exe notepad.exe` 即失败路径自检）。它**不查询壳的谱面索引**：确认"这张谱在不在索引里"要看壳日志（`logLevel: debug`）里的命中/未命中行与 `chart-not-indexed` 原因。

# Malody 4.3.7 native client data source (malody4)

Technical document for AI readers. Human-facing guide: [docs/shell-guide.md](../shell-guide.md); multi-source architecture and routing: [multi-source.md](multi-source.md); shell contract: `desktop/docs/CONTRACT.md` (v3); dynamic OD table: [malody4-od.md](malody4-od.md).

## 1. What it is

`malody4` is the plugin's **fourth** data source and it targets the **native 32-bit C++ Malody 4.3.7 client** (not Malody V, which is a separate client, a separate source id `malody`, and a separate identity prefix).

The attachment is **read-only observation with zero injection**: the shell (`desktop/`) reads three signals from outside the process and **writes no file into the game folder**, injects no DLL, uses no BepInEx, no Lua and no hooks, and never modifies game memory. That constraint is what makes the source acceptable: the game side needs **no installation step at all** — the installer's third game option (`Malody 4` in `bridges/install-bridge.ps1`) only records a `malody4Root` key in `mma-shell-config.json`, and the game tree hash is byte-identical before and after.

Platform: **Windows only** (`ReadProcessMemory` and the PE version check are Win32 semantics); on other platforms the whole channel degrades gracefully to `platform-unsupported` without affecting the other three data sources.

## 2. The three read-only signals

- **Anchor identity key** via `ReadProcessMemory`: module base + fixed RVA `0x8310EC` yields a pointer whose target is an ASCII string `<32-lowercase-md5>_<slot>` — the game's own identity key (`slot` is diagnostic only).
- **Game log tail**: scene lines `[MS] LOG: switch to N` map `1/2 → selection`, `3 → playing`, `4 → result`, anything else `other`; `switchback` / `switch back` and `switchbefore to N` are not scene changes.
- **`config.json` polling**: only `user_mods` (speed bits `DASH 0x10` / `RUSH 0x20` / `SLOW 0x100`, mutually exclusive, `DASH > RUSH > SLOW` on the abnormal multi-hit case) and `user_judge_level` (`0..=4` → `A`–`E`, out of range = unknown).

Cadence: a 200 ms main loop, a 2 s `malody4_selection` heartbeat, 300 ms debounce on anchor/scene changes, and a 10 s freshness window for `playing`.

**Version gate**: before attaching, the target PE must have `TimeDateStamp == 0x5D79AC91` **and** file size `4,750,848` (the version table lives in exactly one place, `desktop/src/malody4/anchor.rs`). Any mismatch makes the **whole channel unavailable with a `target-mismatch:*` reason** instead of guessing offsets — the RVA belongs to one specific binary.

Privacy: `config.json` also holds `user_id` / `token` / `sound_hash`; only the two fields above are read, nothing else is parsed, logged, persisted or committed, and all fixtures are synthetic.

## 3. Library index and identity

- Walks `<root>/beatmap/**` to a depth cap of 8.
- Admission: only `.mc` whose own `meta.mode == 0` (Key mode) with `meta.mode_ext.column` present; `__MACOSX`, `._*` files and symlinks/reparse points are skipped with counters in the log.
- Streamed md5 per file (64 KiB chunks) plus a 64 KiB prefix scanned by manual bracket pairing for `meta`.
- Duplicate md5 resolves deterministically to the lexicographically smallest relative path; the rest are counted in the log.
- Identity is `mdy4:{md5}`; `mdy4:` and Malody V's `mdy:` are independent prefixes of **two different sources** and never collide, and the md5 is the game's own key, so renames and title changes do not matter.
- **Deliberate `.mc`-only deviation**: the external spec suggests indexing `.mc` and optionally `.osu`; this implementation does **not** index `.osu`, because `meta.mode == 0` is the Key-mode criterion for `.mc` and `.osu` has no equivalent field, so accepting `.osu` would pull other osu! modes into the index. Cost: a `.osu` chart in the library can never be followed.

## 4. Frames and contract (v3)

The contract moved from v2 to **v3** (`desktop/docs/CONTRACT.md`) and this source is part of that bump: a new `malody4_selection` frame (the frame set grows from seven to **eight** types) with `{path, speed_rate, screen, sequence, event, version, chart_hash, source}` and `event ∈ anchor-changed / scene-changed / heartbeat / hidden`; `song.source` gains `"malody4"`; `requestId` gains the `n{seq}` prefix; `song.meta.judge` is optional (`A`–`E`, absent = unknown, page falls back to judge C); `state.sources.malody4{alive, playing, screen, reason?, judge?}`.

**`hidden` carries no reason (deliberate deviation from the reference bridge)**: the reference bridge puts the reason inside the hidden record; here the hidden record has all fields empty with `event = "hidden"`, and the reason is delivered through `sources.malody4.reason` (the 30 s state frame) and the shell log. Rationale: hidden is a 2 s heartbeat-class frame while the reason set is closed and changes far more slowly, so duplicating it in every hidden only repeats one sentence and gives the reason two authorities. Cost: a consumer that reads only selection frames must also consume the state frame.

**`reason` closed set** (12 literals quoted from `desktop/docs/CONTRACT.md` §8, not paraphrased here): `chart-not-indexed`, `process-not-found`, `multiple-instances`, `access-denied`, `bad-read`, `target-mismatch:pe_timestamp_mismatch`, `target-mismatch:file_size_mismatch`, `target-mismatch:pe_header_out_of_range`, `target-mismatch:unknown`, `root-not-configured`, `no-library`, `platform-unsupported`. `root-not-configured` means **the whole resolution chain produced nothing**, not "the user did not configure it".

**Conservative withhold**: if `config.json` is missing or unparsable at this tick so the judge level cannot be determined, no song frame is sent for that tick (logged once per cause). Sending `judge: null` would make the page fall back to judge C (OD 8.08), turning a half-read config into a silently wrong OD. The 2 s selection heartbeat is unaffected.

## 5. Root resolution chain (the only authoritative order)

1. the running process directory (only requires `malody.exe` in it — no version check, so a version mismatch surfaces as `target-mismatch:*` rather than being disguised as "no process");
2. `MMA_MALODY4_ROOT`;
3. the shell config key `malody4Root` in `mma-shell-config.json`;
4. the same key in the tosu settings file;
5. the heuristic candidates (absolute paths, the only level that version-checks, with a drive-ready pre-check and a 30 s TTL cache).

The first four levels accept any non-empty value. Clearing `malody4Root` is **not** an off switch, because level 5 may still adopt a passing directory; point `MMA_MALODY4_ROOT` at a nonexistent path to disable the source deterministically.

## 6. Page-side routing and state

Priority is `["osu", "etterna", "malody4", "malody"]`. L1 preempts when the shell reports `malody4.playing` (scene 3, fresh). The L2 60 s freshness window is renewed **only** by a non-heartbeat selection frame with a non-empty `path`; hidden frames and heartbeats never renew it, otherwise a running shell would pin routing to malody4 forever. `state.malody4Alive` is derived from selection freshness (6 s) and is diagnostic only; `malody4Screen` / `malody4Reason` / `malody4Judge` are diagnostic too. The fourth source dot colour is `#22d3ee`, chosen for maximum hue separation from `#ff66aa` / `#a855f7` / `#3b82f6`; Malody 4 and Malody V are two independent sources with two independent dot colours. Forced locking uses `gameClient` = `Malody 4` (also `malody4` / `malody-4`), matched exactly before the `startsWith("mal")` branch.

## 7. Known limitations

Dynamic judge OD covers the PC windows only (see [malody4-od.md](malody4-od.md)); screen-based display gating is not implemented (`screen` is diagnostic only); only `.mc` is indexed, so `.osu` charts in the library are never followed; one library and one instance only (several `malody.exe` processes make the channel unavailable as `multiple-instances`); only the 4.3.7 binary is supported (any other build is `target-mismatch:*`); no native MSD, no pause detection, no live PP; no administrator rights required (same-user reads suffice, and a policy block reports `access-denied`).

## 8. Diagnostic tool

`tools/malody4-anchor-check/` (bare `rustc`, single file, injecting the shell's own `anchor.rs`) prints every stage — find process, validate PE version, read the anchor pointer, parse the identity key — and `--exe <path>` performs an **offline PE version check** with no process involved. It does **not** query the shell's chart index: to see whether a chart is indexed, read the shell log with `logLevel: debug` (the hit/miss lines and the `chart-not-indexed` reason).
