# 2026-09-21 新增第四数据源 Malody 4.3.7（零注入只读观察）

## 修改内容（What changed）

1. 新增第四个数据源 `malody4`：Malody 4.3.7 原生 32 位 C++ 客户端，由桌面壳**零注入只读观察**接入（`ReadProcessMemory` 读锚点身份键、tail 游戏日志取场景、轮询 `config.json` 取变速位与判定档）。**不向游戏目录写入任何文件**、不注入 DLL、不用 BepInEx/Lua、不修改游戏内存。
2. 新增 identity 前缀 `mdy4:{md5}`（md5 = 谱面文件字节摘要，即游戏自身身份键）；与既有 `ett:`（Etterna）/ `mdy:`（Malody V）**并列且互不命中**（`mdy4:` 与 `mdy:` 是两个不同源的独立前缀）。
3. 壳桥契约 **v2 → v3**（`desktop/docs/CONTRACT.md`，`js/app/sources/bridgeClient.js` 的 `CONTRACT_VERSION = 3`）：帧集合由**7 型扩为 8 型**，新增 `malody4_selection` 帧（`{path, speed_rate, screen, sequence, event, version, chart_hash, source}`，`event ∈ anchor-changed / scene-changed / heartbeat / hidden`）；`song.source` 枚举新增 `"malody4"`；`requestId` 新增 `n{seq}` 前缀（锚点轮询通道）；`state.sources` 新增 `malody4{alive, playing, screen, reason?, judge?}`；`song.meta` 新增可选 `judge`。`reason` 为 **14 个字面量**的**闭集**（后续补入 `chart-unresolved` 临时态与 `chart-unknown-identity` 确定态；两者都是该闭集的**向后兼容扩展**——`reason` 对页面是不透明诊断串、页面只存不按取值分支——故契约版本仍为 v3，帧形态与卡片行为不变）。
4. 壳配置 `mma-shell-config.json` 新增键 `malody4Root`，并有完整解析链（运行中进程 → `MMA_MALODY4_ROOT` → 壳配置 → tosu 设置 → 启发候选）；安装器 `bridges/install-bridge.ps1` 新增第三个游戏项 `Malody 4`，**只写这个键**，**零文件复制**（游戏树哈希安装前后一致），卸载只清该键。
5. 页面侧：优先级 `["osu","etterna","malody4","malody"]`；L1 由壳 state 帧的 `playing`（场景 3 且新鲜）抢占；L2 只由**非心跳且 `path` 非空**的 selection 帧续约（hidden 与心跳永不续约）；源圆点新增第四色 `#22d3ee`；`gameClient` 可锁定为 `Malody 4`。
6. 新增**动态 OD**（判定档 × 速率 → 等效 OD，PC 表）——该部分单列一则说明：[2026-09-21-malody4-dynamic-judge-od.md](2026-09-21-malody4-dynamic-judge-od.md)。
7. 新增诊断工具 `tools/malody4-anchor-check/`（裸 `rustc` 单文件，注入壳里的同一份 `anchor.rs`；`--exe` 可做离线 PE 版本校验）。
8. **插件版本号不变**：`index.js` 的 `_VERSION` 与 `metadata.txt` 的 `Version` 均保持 `2.1.0`（版本冻结是本次的既定决策）。
9. **判定档与变速位改从进程内存读取**（后续补入）：游戏的 `config.json` 只在**开局与退出**时重写（实测：打开游戏、在游戏内改判定档与模组、`mtime` 均不变；只有真正游玩后才更新），因此文件通道无法反映游戏内的即时改动。改为读进程内的两个设置单例（发布本局 play-config，用户设置单例作兜底与旁证，`config.json` 最后兜底），使**改判定档 / 变速位立即生效**。所有偏移集中在 `anchor.rs` 一处，读值逐条守卫（指针为空/未对齐/越界、对象块短读、判定越界、对象身份不变量不成立 → 一律回落而不发布），且**不做任何夹断**。
10. **谱面无法解析时的可见提示**（后续补入）：游戏持有的身份键**并不总能对应磁盘文件**（实测某次会话 25 个身份键里仅 6 个在盘上存在），这类谱面按重试预算耗尽后的 `chart-unknown-identity` 上报，并在**切过去约 0.2s** 先给出 `chart-unresolved` 临时提示、约 10s 后换成确定提示；卡片保留上一张的行为不变（不门控、不隐藏），重试预算与重建节流保留（它们是"游戏里新导入的谱面能被扫到"的唯一途径）。

## 修改原因（Why）

社区玩家同时游玩 osu!mania、Etterna、Malody V 与 Malody 4.3.7 原生客户端，需求是分析卡自动跟随当前游戏。Malody 4.3.7 是原生客户端（无 Unity、无 Lua、`.mui` 只是声明式皮肤布局），此前按"需要另开原生注入逆向项目"被判定不做；本轮验证了**零注入只读观察**通道可行（进程外读固定 RVA 锚点 + 日志 + 配置文件，游戏侧无需任何安装），故翻案接入。之所以坚持零注入：不写游戏目录、不注入、不 hook，才能既不动游戏完整性、又让游戏侧零安装步骤。

## 兼容性影响（Impact）

- **契约不匹配的老页面**：契约升到 v3 而插件版本号仍是 `2.1.0`，**使用陈旧 tosu 静态页的壳用户**会在 hello 握手时因 `contract` 不匹配而进入终态（提示更新插件并停止重连），外部源不可用——**必须更新插件文件**。这是本次最需要用户注意的发布事项。
- 其余三个源不受影响：osu! 单源、Etterna、Malody V 的行为与 identity 前缀均不变；`mdy4:` 不会命中 `mdy:`。
- 浏览器模式（无壳）不受影响：仍是 osu! 单源。
- 平台：该源**仅 Windows**；Linux 上以 `platform-unsupported` 优雅不可用，其余三个源照常（Etterna 仍支持 Linux）。
- 缓存：`mdy4:{md5}` 是新前缀，与新升级的 modSignature 段位一起构成全新的缓存条目，不会与旧快照混淆。
- 版本号不变意味着用户侧**不会**收到"有新版本"提示，因此"需要更新插件文件"只能靠发布说明与本文档传达。

## 兼容策略（Compat）

- 契约不匹配按既有设计处理：页面进入终态并提示更新，不无限重连；壳侧对未知帧类型不做破坏性处理。
- 安装器的新增项对老用户是纯增量：不加参数时行为不变，老配置缺 `malody4Root` 时自动补空串键，卸载只清自己写的键。
- 该源不可用时按设计**退化为"不可用 + 原因"**（12 字面量闭集），不猜版本、不猜偏移量、不伪造跟随；其余三个源与浏览器模式不受牵连。
- 关断方式明确：把 `MMA_MALODY4_ROOT` 指向不存在的路径（非空即采纳 ⇒ 之后稳定 `process-not-found`），或移走/更名游戏目录让启发候选的三重校验失败。**"留空 `malody4Root`"不是关断**（末级启发候选仍可能采纳）。

## 验证（Verification）

契约与帧形状：壳侧单测覆盖 `build_state_frame` 的 JSON 形状（`judge` 缺省不出现、`Some('B')` 时为 `"B"`）与 `dispatch_plan` 的签名/心跳规则。只读通道：离线诊断工具对任意 PE 做版本校验（`--exe notepad.exe` 走失败路径自检），真机上锚点读取与身份键解析逐环打印。零写入：安装器实跑前后对游戏目录取哈希，逐字节相同。路由：页面侧合成帧用例断言"改判定 → 签名变化 → 重算"、"hidden/心跳不续约"、"Malody V 仍为 OD 9 且逐字节不变"。文档侧：索引完整性、相对链接可达性、语言门、陈旧表述门与 OD 表逐值比对全部通过。

---

# 2026-09-21 New fourth data source: Malody 4.3.7 (zero-injection read-only observation, EN)

## What changed

1. Added the fourth data source `malody4`: the native 32-bit C++ Malody 4.3.7 client, attached by the desktop shell through **zero-injection read-only observation** (`ReadProcessMemory` for the anchor identity key, log tailing for the scene, `config.json` polling for the speed bits and judge level). **No file is written into the game folder**, no DLL is injected, no BepInEx/Lua, no memory modification.
2. New identity prefix `mdy4:{md5}` (md5 = the chart file's own byte digest, i.e. the game's identity key), sitting beside the existing `ett:` (Etterna) and `mdy:` (Malody V) prefixes and never colliding with them (`mdy4:` and `mdy:` belong to two different sources).
3. Shell contract **v2 → v3** (`desktop/docs/CONTRACT.md`, `CONTRACT_VERSION = 3` in `js/app/sources/bridgeClient.js`): the frame set grows from **seven to eight** types with the new `malody4_selection` frame (`{path, speed_rate, screen, sequence, event, version, chart_hash, source}`, `event ∈ anchor-changed / scene-changed / heartbeat / hidden`); `song.source` gains `"malody4"`; `requestId` gains the `n{seq}` prefix; `state.sources` gains `malody4{alive, playing, screen, reason?, judge?}`; `song.meta.judge` is optional. `reason` is a closed set of **14 literals** (the provisional `chart-unresolved` and the definite `chart-unknown-identity` were added later; both are **backwards-compatible extensions** of that set, because `reason` is an opaque diagnostic string that the page stores without branching on it, so the contract stays at v3 and the frame shape and card behaviour are unchanged).
4. Shell config key `malody4Root` with a full resolution chain (running process → `MMA_MALODY4_ROOT` → shell config → tosu settings → heuristic candidates); the installer's third game option `Malody 4` writes **only** that key and copies **zero files** (game-tree hash identical before and after), and uninstall clears only that key.
5. Page side: priority `["osu","etterna","malody4","malody"]`; L1 preempts via the shell's `playing` (scene 3, fresh); L2 is renewed only by a non-heartbeat selection frame with a non-empty `path` (hidden frames and heartbeats never renew it); a fourth source dot colour `#22d3ee`; `gameClient` can lock to `Malody 4`.
6. **Dynamic OD** (judge level × rate → equivalent OD, PC table) is a separate note: [2026-09-21-malody4-dynamic-judge-od.md](2026-09-21-malody4-dynamic-judge-od.md).
7. New diagnostic tool `tools/malody4-anchor-check/` (bare `rustc`, single file, injecting the shell's own `anchor.rs`; `--exe` performs an offline PE version check).
8. **The plugin version is not bumped**: `index.js` `_VERSION` and `metadata.txt` `Version` both stay `2.1.0` (a deliberate freeze).
9. **The judge level and speed mod are now read from process memory** (added later): the game rewrites `config.json` only when it starts and when it exits (measured: opening the game and changing both settings in game left the file's mtime untouched, and it only updated once a chart was actually played), so file polling can never reflect an in-game change promptly. Both values are now read from the two in-process settings singletons (the per-play object is published, the user-settings singleton is the fallback and corroborator, `config.json` is the last resort), which makes **a judge or speed-mod change take effect immediately**. All offsets live in `anchor.rs` in one place, and every read is guarded (null, misaligned or out-of-range pointer, short object block, out-of-range judge, violated object-identity invariant all fall back instead of publishing), with no clamping anywhere.
10. **A visible notice when a chart cannot be resolved** (added later): the identity the game holds **does not always correspond to a file on disk** (measured: only 6 of the 25 identities held in one session existed as files). Those charts are reported as `chart-unknown-identity` once the rebuild budget is spent, and the card says so — first about 0.2 s after the highlight with a provisional `chart-unresolved` notice, then with the definite one after about 10 s. The card still keeps the previous chart (no gating, no hiding), and both the retry budget and the rebuild throttle are kept, because they are what lets a chart newly imported in game get picked up.

## Why

Players run osu!mania, Etterna, Malody V and the Malody 4.3.7 native client side by side and want the analysis card to follow the active game. The 4.3.7 client is native (no Unity, no Lua, `.mui` is only declarative skin layout) and was previously ruled out as "it would mean a separate native-injection reverse-engineering project"; this round verified that zero-injection read-only observation works (out-of-process reads of a fixed-RVA anchor plus the log and the config file, with no install step on the game side), so it was reinstated. Zero injection is the point: no game-folder writes, no injection, no hooks — the game stays untouched and needs nothing installed.

## Impact

The contract is now v3 while the plugin version stays `2.1.0`, so a shell user with a **stale tosu page** hits a contract mismatch in the hello handshake and lands in a terminal state (an update prompt, no reconnection) with no external sources available — **updating the plugin files is required**. This is the release-critical item. The other three data sources are unaffected (osu! single source, Etterna and Malody V behaviour and identity prefixes unchanged; `mdy4:` never matches `mdy:`), browser mode without the shell is unaffected, and the new source is **Windows-only** (elsewhere it degrades to `platform-unsupported` while Etterna keeps working on Linux). Cache-wise `mdy4:{md5}` is a new prefix, so it cannot collide with old snapshots, and because the version is unchanged users get no "new version" prompt — the required update travels only through release notes and this document.

## Compatibility

A contract mismatch is handled by the existing design (terminal state with an update prompt, no reconnect loop) and unknown frame types are non-destructive on the shell side. The installer change is purely additive (default behaviour without arguments is unchanged, a missing `malody4Root` is back-filled as an empty string, uninstall clears only its own key). When the source is unavailable it degrades by design to "unavailable plus a reason" from the closed 12-literal set — it never guesses a version, guesses offsets or fakes following — and the other three data sources and browser mode are untouched. Disabling it deterministically means pointing `MMA_MALODY4_ROOT` at a nonexistent path (any non-empty value is accepted, so it then stays `process-not-found`) or moving/renaming the game folder so the heuristic triple check fails; clearing `malody4Root` is not an off switch.

## Verification

Contract and frame shape: shell unit tests assert the `build_state_frame` JSON shape (`judge` absent by default, `"B"` when `Some('B')`) and the signature/heartbeat rules in `dispatch_plan`. Read-only channel: the offline diagnostic tool validates any PE file (`--exe notepad.exe` exercises the failure path) and prints every stage of process lookup, anchor read and identity parsing on a real machine. Zero writes: the installer was run and the game-tree hash was byte-identical before and after. Routing: synthetic page-side frame cases assert "judge change → signature change → recompute", "hidden and heartbeats never renew" and "Malody V still emits OD 9 byte-identically". Documentation: index integrity, relative-link reachability, the language gate, the stale-phrasing gate and the cell-by-cell OD table comparison all pass.
