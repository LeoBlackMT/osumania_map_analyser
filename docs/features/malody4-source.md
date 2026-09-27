# Malody 4.3.7 原生客户端数据源（malody4）

> 面向 AI 的技术文档。人类安装/使用说明见 [docs/shell-guide.md](../shell-guide.md)；多源整体架构与路由见 [multi-source.md](multi-source.md)；壳侧契约见 `desktop/docs/CONTRACT.md`（v3）；动态 OD 表见 [malody-od.md](malody-od.md)。

## 1. 定位

`malody4` 是本插件的**第四个**数据源，对应 **Malody 4.3.7 原生 32 位 C++ 客户端**（不是 Malody V——那是独立客户端、独立源 `malody`、独立 identity 前缀）。

接入方式是**零注入的只读观察**：壳（`desktop/`）从进程外读游戏的四条信号，**不向游戏目录写入任何文件**、不注入 DLL、不用 BepInEx、不用 Lua、不 hook、不修改游戏内存。这条约束是本源能成立的前提：游戏侧**无需任何安装操作**，安装器（`bridges/install-bridge.ps1` 的第三个游戏项 `Malody 4`）只往 `mma-shell-config.json` 写一个 `malody4Root` 键，安装前后游戏目录哈希逐字节相同。

平台：**仅 Windows**（`ReadProcessMemory` 与 PE 版本校验都是 Win32 语义）；其他平台整条通道以 `platform-unsupported` 优雅不可用，不影响其余三个源。

## 2. 四条只读观测信号

| 信号 | 采集方式 | 得到什么 |
| --- | --- | --- |
| 锚点身份键 | `ReadProcessMemory`：模块基址 + 固定 RVA `0x8310EC` 取出一个指针，再读该指针指向的 ASCII 串 | 形如 `<32 位小写 md5>_<slot>` 的**游戏自身身份键**（`slot` 只做诊断，不进 path/hash/identity） |
| 设置单例 | `ReadProcessMemory`：`module_base + 0x8310C4` → **本局 play-config `P`**（判定档 `P+0x14`、`user_mods` `P+0x00`，另有 `P+0x04` 作对象身份证明：它由 `P+0x00` 只清位得来，故必须 `P+0x04 & ~P+0x00 == 0`）；`module_base + 0x8311C4` → 用户设置单例 `S`（`S+0x9C` / `S+0xD0`） | **判定档与变速位的主来源**：游戏里一改立刻生效。**发布 `P`、`S` 兜底**——真机实测显示 **mods 面板只写 `P+0x00`**，`S` 要到开局提交才跟上（判定面板则同写两者），所以只发布 `S` 会让"改模组"滞后一整局；两条都读不到才回落 `config.json`。所有守卫按链独立生效，**绝不把两条链的值拼接** |
| 游戏日志 | tail 最新的 `log-*.txt` 的新增行 | 场景：`[MS] LOG: switch to N` 的 `N` 映射 `1/2 → selection`、`3 → playing`、`4 → result`、其余 `other`；`switchback` / `switch back`（回退）与 `switchbefore to N`（预声明）**不算切换** |
| `config.json` | 周期轮询（读盘）；**只在内存读不到时兜底** | 只取 `user_mods`（变速位 `DASH 0x10` / `RUSH 0x20` / `SLOW 0x100`，互斥；多位同时命中按 `DASH > RUSH > SLOW` 取值并记一次日志）与 `user_judge_level`（`0..=4` → `A`~`E`，越界 = 判定未知、不猜档位） |

轮询节奏：主循环 200ms 一拍；`malody4_selection` 帧的心跳间隔 2s，换锚点/换场景有 300ms 防抖；`playing` 的"新鲜"窗口为 10s（日志目录消失等异常下 `playing` 自然失鲜，而不是永驻）。

**判定档 / 变速位的取值链（内存优先，`config.json` 兜底）**：游戏只在**开局与退出**时把 `config.json` 整份重写（实测游戏内改判定 / 改 mod 前后该文件的 mtime 一动不动），所以这个文件永远反映不了本局中途的改动——判定档与变速位因此改从进程内存读，`config.json` 只在内存读不到时顶上。每 tick 都**重新读指针**（两个对象都在惰性单例后面、拆解路径会把指针清零，缓存指针等于把上一局的设置当成现役值），任一环不成立即回落 `config.json`、**绝不发布一个可能错的数字**：指针为 0（`P` 在首个场景之前就是 0）、指针不是 4 字节对齐或超出 32 位用户地址空间、判定档不在 `0..=4`、对象块短读、以及 `P+0x04 & !P+0x00 != 0`（`P+0x04` 是由 `P+0x00` 复制后**只清位**得来的，这条不变量是 `P` 的免费身份证明；不成立说明那个地址上的东西不是我们以为的对象）。两条链都读到时**发布 `S`**（"玩家设的值"，且早于首个场景就存在），`P` 只当旁证——连续 3 拍不一致才记一条一次性的 warn，因为那只可能是其中一条链的偏移理解有误。

`config.json` 是**软信号，不是门**：附着后的 30s 内两者不一致只记一条 warn（两边数字都给出），**不拒绝、不覆盖**内存值——本局中途改判定造成的分歧正是这个设计的目的。有效值（来源 + 判定档字母 + 速率）**发生变化时**记一条 info：`malody4 settings: source=memory|config.json judge=<A~E|unknown> speed_rate=<n.nn>`，同值绝不重复记（200ms 一拍，逐 tick 记会淹掉日志）。

**版本门（重要）**：附着前校验目标 PE 的 `TimeDateStamp == 0x5D79AC91` **且**文件大小 `4,750,848`（版本表在 `desktop/src/malody4/anchor.rs` 里只出现一处）。任一项不符 → **整条通道不可用并给出 `target-mismatch:*` 原因**，绝不"按偏移量猜着读"——RVA 属于特定二进制，猜错会把任意内存当成 md5。设置单例的 RVA（`0x8311C4` / `0x8310C4`）与四个字段偏移（`0x9C` / `0xD0` / `0x14` / `0x00`）同样**只对这一个构建成立**，与 `KNOWN_CLIENTS` 同处 `anchor.rs` 且全仓库只此一处，消费者一律按名字引用。

隐私：`config.json` 里还有 `user_id` / `token` / `sound_hash` 等字段，壳**只读上面两个字段**，其余一律不进结构体、不进日志、不落盘、不提交；测试夹具全部为合成数据。设置单例的内存读取只读判定档与 `user_mods` 两个字段，**不写、不注入、不分配**目标进程内存。

## 3. 谱面库索引与身份

- **遍历范围**：`<root>/beatmap/**`，递归深度上限 8 层。
- **准入判据（两类谱面都收）**：
  - `.mc`：`meta.mode ∈ {0, 6}`（两种 Key 模式）**且** `meta.mode_ext.column` 存在。真机实测 `mode=6` 是列式 Key 谱（声明 `column=8` 时 note 的 `column` 取值为 `0..7`，与声明一致），旧判据 `mode == 0` 曾把 13 张 8K 谱挡在库外；`mode=1/2/3/4/5`（Catch/Pad/Taiko）即便带 `column` 也**拒收**——转换器处理不了，收进来只会变成"分析失败"。
  - `.osu`：从文件头读 `[General] Mode` 必须为 `3`（osu!mania），`[Difficulty] CircleSize` 作键数，`[Metadata]` 取 Title/Artist/Version。**游戏自己就会播放 `.osu` 谱面**（真机实测：某曲目目录下只有 `.mc` 是 4K，而玩家在游戏内选中的 7K 是那个 `.osu`，锚点给出的正是它的 md5），所以 `.osu` 必须进索引；且 `.osu` 本来就是目标格式，**无需转换**（`externalSource` 的 `looksLikeOsu()` 直通）。
  - `__MACOSX` 目录、`._*` 文件、symlink/reparse point 一律跳过（跳过计数进日志，不静默；非 Key 模式与 `.osu` 头部不合规各自单独计数）。
- **哈希**：逐文件单趟流式 md5（64 KiB 块喂 `Md5`，绝不整份读进内存），同时保留前 64 KiB 前缀提取头部（`.mc` 走**手工括号配对**取 `meta`、`.osu` 走**分区感知的 INI 扫描**，都不整份 JSON 解析）。
- **重复 md5**：确定性取"相对路径字典序最小"的那一份，其余计数进日志（同一张谱在库里有多份副本时结果稳定可复现）。
- **identity**：`mdy4:{md5}`。`mdy4:` 与 Malody V 的 `mdy:` 是**两个不同源**的独立前缀，互不命中；`md5` 是游戏自身的身份键，**免疫改名与曲名变动**。
- **`reason` 与 `path_count` 的边界**：见 §7.1——**游戏持有的身份键并不总能对应到磁盘文件**（真机实测 25 个身份键里只有 6 个能在盘上找到），这类谱面按 `chart-unknown-identity` 处理，不再反复重建索引。
- 索引进度：索引未构建完成前整源按"未就绪"处理（`no-library`），构建完成后后台线程独占写入、主循环只读。

## 4. 帧与契约（v3）

契约版本从 v2 升到 **v3**（`desktop/docs/CONTRACT.md`），本源的帧变化是这次升级的内容之一：

- 新增 `malody4_selection` 帧（帧集合由 **7 型扩为 8 型**）：`{path, speed_rate, screen, sequence, event, version, chart_hash, source}`，`event ∈ anchor-changed / scene-changed / heartbeat / hidden`，`source` 恒为 `malody4-native`。
- `song` 帧 `source` 枚举新增 `"malody4"`；`requestId` 新增 `n{seq}` 前缀（锚点轮询通道）。
- `song.meta` 新增可选 `judge`（判定档字母 `A`~`E`；缺省 = 判定未知，页面回落 C 档）。判定档随 song 帧下发，判定已进页面签名 → 改判定即触发一次重发（下一 tick 刷新 OD，**不必等下次换谱**）。
- `state.sources` 新增 `malody4{alive, playing, screen, reason?, judge?}`：`alive` = 本 tick 是否成功附着到唯一 `malody.exe` 并读到锚点；`playing` = 场景为游玩且新鲜（≤10s）；`screen ∈ selection / playing / result / other`（仅在已知时出现）；`reason` 与 `judge` 健康时不出现。

**`hidden` 不携带原因（与参考桥的有意偏离）**：参考桥把原因写进 hidden 记录里，本实现让 hidden 记录的五个字段全空、`event = "hidden"`，原因另经 `sources.malody4.reason`（30s 周期 state 帧）与壳日志给出。理由：hidden 是 2s 心跳级的高频帧，而原因集合是闭集且变化远慢于心跳；把原因塞进每一条 hidden 只会在线上重复同一句话，且会让"原因从哪来"有两个权威（记录 vs state 帧）。代价：只看 selection 帧的消费者拿不到原因，必须同时消费 state 帧。

**`reason` 闭集（14 个字面量，逐字引用 `desktop/docs/CONTRACT.md` §8；不在此处改写）**：`chart-not-indexed`、`chart-unresolved`、`chart-unknown-identity`、`process-not-found`、`multiple-instances`、`access-denied`、`bad-read`、`target-mismatch:pe_timestamp_mismatch`、`target-mismatch:file_size_mismatch`、`target-mismatch:pe_header_out_of_range`、`target-mismatch:unknown`、`root-not-configured`、`no-library`、`platform-unsupported`。其中 `root-not-configured` **只表示整条解析链走完仍为 `None`**（见 §5），不是"用户没配"的同义词；谱面级的未命中原因按**"试完没有"分两层**——`chart-unresolved` = 刚查过、重建尝试**还在进行中**（临时态，首次未命中即上报，约 0.2s 就在状态行可见），`chart-unknown-identity` = 这个身份键**已查过、重建尝试也用完仍解析不出来**（确定态，约 10s 后给出，本会话不再为它重建，见 §7.1）；`chart-not-indexed` 保留为闭集里的合法取值与选源状态机的内部成因（轮询器自引入临时态后不再自己发它）。新字面量是闭集的**向后兼容扩展**（`reason` 对页面是不透明诊断串，页面只存不分支），故契约仍是 v3。各态对帧形态与卡片行为完全相同（hidden 记录、卡片保留上一张谱面），差别只在状态行给出的说明与壳日志的可分辨性。

**判定未知时保守 withhold**：若此刻**内存与 `config.json` 都给不出判定**（未附着 / 两个设置单例都读不到 / 文件缺失或解析失败），则该 tick **不发 song 帧**（记一次日志，同因由去重），等下一刻读到判定再发。理由：把 `judge` 当 `null` 发出去会让页面回落 C 档 8.08，等于把"读到半截状态"变成一个静默的错误 OD。该 withhold 不影响 selection 帧的 2s 心跳。

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
- `state.malody4Alive` 由 selection 帧新鲜度派生（6s 窗口 ≈ 心跳 2s + 容忍丢一拍），是**诊断位/圆点用**的值，`decide()` 不读它；`malody4Screen` / `malody4Judge` 同为诊断位（`js/app/sources/shellState.js`）。`malody4Reason` 有一个例外：取值等于 `chart-unknown-identity` 且当前路由就是 malody4 时，页面在状态行给出一句可见提示；原因被壳清空时只收掉自己写的那一条，绝不覆盖分析流程后来的状态行更新（见 §7.1）。
- 源圆点第四色 `malody4 = #22d3ee`（亮青），与 `#ff66aa`（osu!）/ `#a855f7`（Etterna）/ `#3b82f6`（Malody V）**四色可区分**；Malody 4 与 Malody V 是两个独立源、两个独立圆点颜色。
- 强制锁定：`gameClient` 取 `Malody 4`（亦接受 `malody4` / `malody-4`）；精确匹配在 `startsWith("mal")` 之前，避免 Malody 4 被折叠成 Malody V。

## 7. 已知限制与未做项（如实标注）

- **判定 OD 动态化仅覆盖 PC 端窗口**：判定档 × 速率 → 等效 OD 的换算与 `FAIR` 模组不建模的限制见 [malody-od.md](malody-od.md)。
- **判定档 / 变速位的内存链只对 4.3.7 这一个构建成立**：`S` / `P` 的 RVA 与四个字段偏移和锚点 RVA 一样是构建专用的（同处 `anchor.rs`）。读不到就如实回落 `config.json`——行为与改动前完全一致（不报错、不影响其余信号），只是不再"立即生效"；将来换二进制时这条链会随版本门一起 fail closed，而不是按旧偏移读出垃圾。
- **screen 显示门控未做**：`screen` 只进 state 帧与诊断，卡片不会因"当前是结算/选曲场景"而隐藏或清空。
- **只索引 `.mc`**：库里的 `.osu` 谱面不会被跟随（§3 的理由与代价）。
- **单一库、单一实例**：只支持一个 `malody4Root` 下的一个 `beatmap/`；多个 `malody.exe` 同时运行时整条通道不可用（`multiple-instances`），不"选一个猜"。
- **只支持 4.3.7 这一个二进制**：其他版本（含 4.3.x 的其他构建）一律 `target-mismatch:*`；新增版本 = 往 `KNOWN_CLIENTS` 加一行并重新取证。
- **不做原生 MSD、不做暂停检测 / livePP**：非 osu 源一律不提供（与 Etterna/Malody V 口径一致）。
- **不需要管理员权限**：同用户权限读取自己启动的游戏进程即可；被系统策略挡住时如实报 `access-denied`。

### 7.1 覆盖边界：身份解析不出来的谱面不被跟随（实测）

现场取证（真实运行中的 4.3.7 客户端，只读侦察）：游戏内存里持有 **25** 个身份键，其中只有 **6** 个 md5 在磁盘上真的有对应文件，**19 个查不出来**。这 19 个游戏侧只持有身份与展示元信息（难度名、BPM/时长文本），**不持有文件路径**：全进程扫描到 46 条绝对 `.mc` 路径与 5 条 `.osu` 路径，没有一条属于这些谱面；整进程 UTF-16 扫描找到 **0** 条宽字符路径。对 `D:\Games\Malody-4.3.7`、`C:\Games`、`D:\BaiduNetdiskDownload` 做全内容哈希搜索，也没有任何文件命中这些 md5。

所以对这些身份键而言"路径解析"在信息上就是**不可能**的——信息不存在，不是实现没做到。**结论：身份解析不出来的谱面不被跟随**。用户看到的现象是卡片继续显示上一张已分析的谱面（这是被接受的既定行为，卡片不隐藏也不门控），而"为什么没换"由两处明说：壳日志一行 **info**（`malody4 chart cannot be identified from the local library: md5=… — 2 rebuild attempts exhausted …; the previous chart stays on screen`，同一 md5 每个会话只此一行），以及页面状态行在 malody4 为当前源时显示的提示 `Malody 4: this chart is not in the local beatmap library — the card still shows the previous chart.`（换到可解析的谱面时被分析流程自然覆盖）。诊断面上这两处对应用户可见的原因字面量 **`chart-unknown-identity`**，与"索引还没建好"的 `chart-not-indexed` 区分开（见 §4）。每个身份键的重建尝试有上限（`MISS_REBUILD_ATTEMPTS = 2`）：耗尽后本会话不再为它重建整库、也不再重复记日志——实测的 19 个身份键永远不会因为重试而出现，重试只会白哈希整库。

## 8. 诊断工具

`tools/malody4-anchor-check/`（裸 `rustc` 单文件、直接注入壳里的同一份 `anchor.rs`，故版本常量在仓库里只有一份）：把"找进程 → 校验 PE 版本 → 读锚点指针 → 解析身份键"逐环打印，一行命令即可看出断在哪一环；`--exe <path>` 做**离线 PE 版本校验**（不需要游戏在跑，`--exe notepad.exe` 即失败路径自检）。它**不查询壳的谱面索引**：确认"这张谱在不在索引里"要看壳日志（命中/未命中行 `logLevel: info` 起就可见）与未命中的原因字面量——`chart-not-indexed` = 还在重试窗口内（结果未定），`chart-unknown-identity` = 重建尝试已用完仍查不到（结论已定，见 §7.1）。

# Malody 4.3.7 native client data source (malody4)

Technical document for AI readers. Human-facing guide: [docs/shell-guide.md](../shell-guide.md); multi-source architecture and routing: [multi-source.md](multi-source.md); shell contract: `desktop/docs/CONTRACT.md` (v3); dynamic OD table: [malody-od.md](malody-od.md).

## 1. What it is

`malody4` is the plugin's **fourth** data source and it targets the **native 32-bit C++ Malody 4.3.7 client** (not Malody V, which is a separate client, a separate source id `malody`, and a separate identity prefix).

The attachment is **read-only observation with zero injection**: the shell (`desktop/`) reads four signals from outside the process and **writes no file into the game folder**, injects no DLL, uses no BepInEx, no Lua and no hooks, and never modifies game memory. That constraint is what makes the source acceptable: the game side needs **no installation step at all** — the installer's third game option (`Malody 4` in `bridges/install-bridge.ps1`) only records a `malody4Root` key in `mma-shell-config.json`, and the game tree hash is byte-identical before and after.

Platform: **Windows only** (`ReadProcessMemory` and the PE version check are Win32 semantics); on other platforms the whole channel degrades gracefully to `platform-unsupported` without affecting the other three data sources.

## 2. The four read-only signals

- **Anchor identity key** via `ReadProcessMemory`: module base + fixed RVA `0x8310EC` yields a pointer whose target is an ASCII string `<32-lowercase-md5>_<slot>` — the game's own identity key (`slot` is diagnostic only).
- **Settings singletons** via `ReadProcessMemory`: `module_base + 0x8310C4` → the **per-play `P`** (judge at `P+0x14`, `user_mods` at `P+0x00`, with `P+0x04` usable as an object-identity proof because it is produced by only clearing bits of `P+0x00`, so `P+0x04 & !P+0x00 == 0` must hold); `module_base + 0x8311C4` → the user-settings singleton `S` (judge at `S+0x9C`, `user_mods` at `S+0xD0`). This is the **primary source for the judge level and the speed mod**: an in-game change takes effect immediately. **`P` is published and `S` is the fallback**, because measurement showed the **mods panel writes only `P+0x00`** while `S` catches up at the game's next commit (the judge-level handler writes both), so publishing `S` alone made a mod change lag a whole play; `config.json` is used only when neither chain is readable. Every guard applies per chain, and values from the two chains are never spliced together.
- **Game log tail**: scene lines `[MS] LOG: switch to N` map `1/2 → selection`, `3 → playing`, `4 → result`, anything else `other`; `switchback` / `switch back` and `switchbefore to N` are not scene changes.
- **`config.json` polling** (disk), **fallback only when memory cannot be read**: `user_mods` (speed bits `DASH 0x10` / `RUSH 0x20` / `SLOW 0x100`, mutually exclusive, `DASH > RUSH > SLOW` on the abnormal multi-hit case) and `user_judge_level` (`0..=4` → `A`–`E`, out of range = unknown).

Cadence: a 200 ms main loop, a 2 s `malody4_selection` heartbeat, 300 ms debounce on anchor/scene changes, and a 10 s freshness window for `playing`.

**Judge / speed resolution chain (memory first, `config.json` fallback)**: the game rewrites the whole `config.json` only **on start and on exit** (measured: its mtime does not move while the player changes the judge level or the mods in game), so the file can never reflect a mid-session change — the judge level and the speed mod are therefore read from process memory, and `config.json` is used only when the memory read yields nothing. The pointers are **re-read every poll** (both objects sit behind lazy singletons whose teardown path zeroes them, so caching a pointer would publish the previous session's settings), and every broken link falls back to `config.json` instead of publishing a possibly-wrong number: a zero pointer (`P` is 0 before the first scene), a pointer that is not 4-byte aligned or lies outside the 32-bit user address space, a judge level outside `0..=4`, a short object read, and `P+0x04 & !P+0x00 != 0` (since `P+0x04` is produced by copying `P+0x00` and only **clearing** bits, that invariant is a free identity proof for `P`; when it fails the object is not what we think it is). When both chains read, **`S` is published** ("what the player set", and it exists before the first scene) and `P` is only a corroborator — a persistent disagreement (3 consecutive polls) logs one warning, because it can only mean that one of the two chains is misread.

`config.json` is a **soft signal, never a gate**: within 30 s after attach a disagreement logs one warning carrying both numbers and **never rejects or overrides** the memory value — a mid-session judge change is exactly what this design is for. When the effective value (source, judge letter, rate) **changes**, one info line is logged: `malody4 settings: source=memory|config.json judge=<A~E|unknown> speed_rate=<n.nn>`; the same value is never logged twice (a 200 ms poll would otherwise flood the log).

**Version gate**: before attaching, the target PE must have `TimeDateStamp == 0x5D79AC91` **and** file size `4,750,848` (the version table lives in exactly one place, `desktop/src/malody4/anchor.rs`). Any mismatch makes the **whole channel unavailable with a `target-mismatch:*` reason** instead of guessing offsets — the RVA belongs to one specific binary. The settings-singleton RVAs (`0x8311C4` / `0x8310C4`) and the four field offsets (`0x9C` / `0xD0` / `0x14` / `0x00`) are **build-specific in exactly the same way**, live next to `KNOWN_CLIENTS` in `anchor.rs`, and are referenced by name everywhere else.

Privacy: `config.json` also holds `user_id` / `token` / `sound_hash`; only the two fields above are read, nothing else is parsed, logged, persisted or committed, and all fixtures are synthetic. The settings-singleton read touches only the judge level and `user_mods`; it never writes to, injects into, or allocates memory in the target process.

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

**`reason` closed set** (14 literals quoted from `desktop/docs/CONTRACT.md` §8, not paraphrased here): `chart-not-indexed`, `chart-unresolved`, `chart-unknown-identity`, `process-not-found`, `multiple-instances`, `access-denied`, `bad-read`, `target-mismatch:pe_timestamp_mismatch`, `target-mismatch:file_size_mismatch`, `target-mismatch:pe_header_out_of_range`, `target-mismatch:unknown`, `root-not-configured`, `no-library`, `platform-unsupported`. `root-not-configured` means **the whole resolution chain produced nothing**, not "the user did not configure it". The chart-level miss reasons come in **two tiers split by whether the retries are exhausted**: `chart-unresolved` = the chart was just looked up and the rebuild attempts are **still in progress** (provisional, published on the first miss so the status line says so within about 0.2 s), and `chart-unknown-identity` = this identity was looked up and still fails to resolve **after the rebuild attempts were exhausted** (definite, reached after about 10 s, no further rebuilds for it this session, see §7.1); `chart-not-indexed` remains a legal value of the set and the source-selection state machine's internal cause, but the poller no longer emits it itself now that the provisional tier exists. The new literals are a **backwards-compatible extension** of the closed set (`reason` is an opaque diagnostic string to the page, which stores it and never branches on it), so the contract stays at v3. Every tier produces the same frame shape and the same card behaviour (a hidden record, the card keeping the previous chart); they differ only in what the status line and the shell log say.

**Conservative withhold**: if neither memory nor `config.json` yields a judge level at this tick (not attached, both settings singletons unreadable, or the file missing/unparsable), no song frame is sent for that tick (logged once per cause). Sending `judge: null` would make the page fall back to judge C (OD 8.08), turning a half-read state into a silently wrong OD. The 2 s selection heartbeat is unaffected.

## 5. Root resolution chain (the only authoritative order)

1. the running process directory (only requires `malody.exe` in it — no version check, so a version mismatch surfaces as `target-mismatch:*` rather than being disguised as "no process");
2. `MMA_MALODY4_ROOT`;
3. the shell config key `malody4Root` in `mma-shell-config.json`;
4. the same key in the tosu settings file;
5. the heuristic candidates (absolute paths, the only level that version-checks, with a drive-ready pre-check and a 30 s TTL cache).

The first four levels accept any non-empty value. Clearing `malody4Root` is **not** an off switch, because level 5 may still adopt a passing directory; point `MMA_MALODY4_ROOT` at a nonexistent path to disable the source deterministically.

## 6. Page-side routing and state

Priority is `["osu", "etterna", "malody4", "malody"]`. L1 preempts when the shell reports `malody4.playing` (scene 3, fresh). The L2 60 s freshness window is renewed **only** by a non-heartbeat selection frame with a non-empty `path`; hidden frames and heartbeats never renew it, otherwise a running shell would pin routing to malody4 forever. `state.malody4Alive` is derived from selection freshness (6 s) and is diagnostic only; `malody4Screen` / `malody4Judge` are diagnostic too. `malody4Reason` has one exception: when it equals `chart-unknown-identity` and malody4 is the active route, the page shows a one-line notice on the status line, and once the shell clears the reason it removes only its own notice, never overwriting a newer status written by the analysis flow (see §7.1). The fourth source dot colour is `#22d3ee`, chosen for maximum hue separation from `#ff66aa` / `#a855f7` / `#3b82f6`; Malody 4 and Malody V are two independent sources with two independent dot colours. Forced locking uses `gameClient` = `Malody 4` (also `malody4` / `malody-4`), matched exactly before the `startsWith("mal")` branch.

## 7. Known limitations

Dynamic judge OD covers the PC windows only (see [malody-od.md](malody-od.md)); the judge/speed memory chain is **build-specific** to the 4.3.7 binary (its RVAs and field offsets live next to the anchor RVA in `anchor.rs`) and falls back to `config.json` whenever it cannot read — identical to the pre-change behaviour (no error, no impact on the other signals), just without the immediate effect, and a future binary fails closed with the version gate instead of reading garbage at stale offsets; screen-based display gating is not implemented (`screen` is diagnostic only); only `.mc` is indexed, so `.osu` charts in the library are never followed; one library and one instance only (several `malody.exe` processes make the channel unavailable as `multiple-instances`); only the 4.3.7 binary is supported (any other build is `target-mismatch:*`); no native MSD, no pause detection, no live PP; no administrator rights required (same-user reads suffice, and a policy block reports `access-denied`).

### 7.1 Coverage boundary: charts whose identity cannot be resolved are not followed (measured)

Measured on a real running 4.3.7 client with read-only reconnaissance: the game holds **25** identity keys in memory, only **6** of those md5s actually exist as files on disk, and **19 cannot be resolved**. For those 19 the game holds the identity plus display metadata (difficulty name, BPM/length text) but **no file path**: a whole-process scan found 46 absolute `.mc` paths and 5 `.osu` paths, none of them belonging to these charts, and a whole-process UTF-16 scan found **0** wide-char paths. A full-content hash search over `D:\Games\Malody-4.3.7`, `C:\Games` and `D:\BaiduNetdiskDownload` found no file matching those md5s either.

So for these identity keys path-based resolution is **impossible in principle** — the information does not exist; it is not an implementation gap. **Conclusion: a chart whose identity cannot be resolved is not followed.** What the user sees is the card keeping the previously analysed chart (accepted behaviour; the card is neither hidden nor gated), and the reason is stated in two places: one **info** log line from the shell (`malody4 chart cannot be identified from the local library: md5=… — 2 rebuild attempts exhausted …; the previous chart stays on screen`, once per md5 per session) and, while malody4 is the active source, the page status line notice `Malody 4: this chart is not in the local beatmap library — the card still shows the previous chart.` (overwritten by the analysis flow as soon as a resolvable chart is loaded). On the diagnostic surface both correspond to the reason literal **`chart-unknown-identity`**, kept distinct from the "index not built yet" case `chart-not-indexed` (see §4). Rebuild attempts per identity are bounded (`MISS_REBUILD_ATTEMPTS = 2`): once exhausted, this session requests no further full-library rebuilds for it and logs nothing more — the measured 19 identities will never appear through retrying, retrying only re-hashes the whole library for nothing.

## 8. Diagnostic tool

`tools/malody4-anchor-check/` (bare `rustc`, single file, injecting the shell's own `anchor.rs`) prints every stage — find process, validate PE version, read the anchor pointer, parse the identity key — and `--exe <path>` performs an **offline PE version check** with no process involved. It does **not** query the shell's chart index: to see whether a chart is indexed, read the shell log (the hit/miss lines are visible from `logLevel: info`) and the miss reason literal — `chart-not-indexed` = still inside the retry window (undecided), `chart-unknown-identity` = the rebuild attempts were exhausted and it still cannot be resolved (decided; see §7.1).
