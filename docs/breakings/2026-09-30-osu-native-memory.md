# 2026-09-30 — osu! 原生内存传输（壳内只读读取 + 24062 兼容 origin，契约 v5→v6）

> 破坏性变更记录。格式：改了什么 / 为什么 / 影响范围 / 兼容性 / 验证方式。
> 域内完整说明（架构、失败语义、两本操作手册、自检清单）见 [docs/features/osu-native-source.md](../features/osu-native-source.md)；帧契约权威说明见 `desktop/docs/CONTRACT.md` §14。

## 1. 改了什么

| 面 | 之前 | 现在 |
|---|---|---|
| osu 数据面 | 只有 tosu 一条传输（`wsEndpoint`，默认 24050） | **两条**：壳内**只读内存读取器**（`desktop/src/osu/**`，按位数分派：stable 32 位签名扫描 / lazer 64 位偏移表驱动）经 **24062** 的 tosu 兼容子集 origin 供数（native，优先）；tosu 保留为兜底。缺失时页面按 `mode:"tosu"` 回到 `state.wsEndpoint` |
| 新增端口 | 24050 / 24060 / 24061 / 17653 | **+ 24062**：WS `/websocket/v2`（150 ms 一帧）、WS `/websocket/commands`（黑洞）、`GET /files/beatmap/{file,background}`；全部响应带 `Access-Control-Allow-Origin: *`，Host 门禁只放行本机 24062；`MMA_OSU_COMPAT_REPLAY=1` 回到固定谱面回放（测试用），缺省实时载荷 |
| 契约版本 | `CONTRACT_VERSION = 5`，页面接受 `[3,5]` | **`6`**，页面接受 `[3,6]`（`desktop/src/frames.rs` 与 `js/app/sources/bridgeClient.js` 同升，`desktop/docs/CONTRACT.md` 同步） |
| state 帧 | `sources{etterna,malody,malody4}` | **+ `sources.osu`**：`{alive, osuTransport{mode,host,port,wsPath,filesPath}, gate?, client?, reason?, degradedFields?, phase?, notice?, progress?}`；`alive ≡ mode == "native"`；**值变即推**（2 s 一拍检测，不等 30 s 周期帧） |
| 页面切流 | 端点只来自用户设置 | 契约 v6：页面在 **socket 层**运行时切换（`appContext.applyOsuTransport` → `socket.setHost()`），**绝不写 `state.wsEndpoint`**（它在 `SETTING_CACHE_KEYS` 里，走设置路径会每次切换清空结果缓存）；只有**壳页**（24061）接受覆盖，且要求 `wsPath`/`filesPath` 逐字符合，否则拒绝（fail-closed） |
| 壳主窗 | 在线（tosu 存活）⇒ 导航到 tosu 插件页；离线 ⇒ 24061 | **缺省一律 24061**；只有壳配置显式 `"osuTransport": "tosu"`（AV 误报排查的逃生开关）才恢复旧策略。原因：主窗落 tosu 页时端点下发与桥都不可达，native 传输永远不会被启用（DEC-21 缺陷②） |
| osu 败方门控 | `isOsuSuppressed()` 首行只看 `shellTosuOnline` | 加入 `state.shellOsuNativeAlive`（原生覆盖真的生效的位）：否则原生帧被当"败方帧"静默缓冲，用户可见现象是**换图不更新**（DEC-21 缺陷①） |
| 端点切换时的连接 | `setHost` 遍历以 URL 为键的 map | 关闭**曾创建过的全部** socket（`socket.allSockets` 集合）——`/websocket/commands` 会被打开两次，只遍历 map 会漏掉被覆盖的那条（A11） |
| 冷启动体验 | 扫描期卡片十几秒毫无说明 | 新增页面元素 `#osu-scan-hint`：壳下发 `phase`/`notice`/`progress`（相位闭集 + 英文提示 + 实测扫描读数），提示**只写自己的元素**（借写共享状态行会被别的写手打掉，实测只闪 ~300 ms）、同一相位值内"载荷到过"不复活、只在壳页显示 |
| 抓谱面文件失败 | 结算界面出现 `Request failed with status 404`，约 2 s 后自愈 | 壳侧：身份指针瞬态失败时进入 **2.5 s 身份保持**（发最后一张好图的 `beatmap` 块 + 本帧 `state`，文件路由供奉同一张图 200）；页面侧：壳原生端点上的瞬态失败（404/5xx）**静默重试一次**（tosu 通道行为逐字不变） |
| 插件版本号 | 2.1.0 | **2.2.0**（`index.js` `_VERSION` 与 `metadata.txt` `Version` 同步；提交内容为功能新增，按仓库惯例取 minor bump） |
| 零写入约束 | — | 读取层只声明 `ReadProcessMemory` / `VirtualQueryEx` / Toolhelp32，句柄权限恒 `PROCESS_VM_READ \| PROCESS_QUERY_INFORMATION`；dump 采集只存在于 dev-time 工具 `tools/lazer-offsets-gen/`（入库），产品内不存在 |

## 2. 为什么

- **验收目标**：关闭 tosu、只留壳 + 游戏时，星数 / 键型图 / 暂停检测 / livePP / 结算 / 换图 / 换 mod 全部正常。这要求壳自己能把 osu 数据面撑起来（原生读取），而不是换个设置项。
- **为什么走"兼容 origin"而不是第五数据源**：页面只有一个 host 字符串（WS、`.osu`、背景图全由它派生），"只加文件端点"无法驱动卡片；而新增第五来源（`osun:` identity + 新帧型）会改动路由/缓存键/播放态/时间/暂停信号全线，且两套缓存不互通（REJ-03）。兼容 origin 让页面**零改动**、identity/modSignature 天然同形 ⇒ 与 tosu 模式**共用结果缓存**。
- **为什么端点下发必须走 socket 层**：`wsEndpoint` 在页面的 `SETTING_CACHE_KEYS` 里，任何"改设置"的路径都会触发 `clearResultCache()`；运行时覆盖另存 `state.runtimeOsuHost`，切换的代价降到 0（真机实测切换前后 `resultCacheGeneration()` 不变）。
- **为什么主窗必须改落点**：端点下发与桥都只在壳自己的页面上生效；主窗落在 tosu 页时，native 传输即使健康也永远到不了页面。
- **为什么保 tosu**：native 不健康、24062 绑定失败、读取器未附着（游戏没开）时页面必须仍然可用；`osuTransport: "tosu"` 同时是"不打开任何内存句柄"的一键逃生开关。
- **许可证洁净**：不复制 tosu/gosumemory 的代码或整表；每个 stable 锚点、每个 lazer 偏移都有自己的推导与真机自证（锚点台账 `desktop/src/osu/patterns.rs`，偏移表由 SOS + IL 双见证生成）。

## 3. 影响范围

- **只在桌面壳 + 壳页**：浏览器模式（无壳、24050 页面）行为逐字节不变——端点覆盖只由壳页接受（`isShellPage()` 门控），`state.wsEndpoint` 与结果缓存语义未动。
- **页面侧改动 9 个文件**（`appContext.js`、`socket.js`、`settings.js`、`sources/shellState.js`、`sources/sourceManager.js`、`sources/bridgeClient.js`、`analysis.js` + 两个新模块 `sources/beatmapFetchRetry.js`、`sources/osuScanHint{,Rules}.js`，另 `index.html`/CSS 加提示元素）；算法/解析/缓存算法零改动。
- **用户可见变化**：① 主窗缺省落 24061（不再自动去 tosu 插件页）；② 冷启动/游戏重启后有 13–23 s 的扫描提示；③ 结算界面的 404 闪烁消失（身份保持 + 一次静默重试）；④ tosu 模式下那张被 tosu 垃圾 `resultsScreen.mods` 算成 DT 的 NM 图，在 native 模式下得到正确结果（我们**不对齐** tosu 的垃圾值，属有意偏离）。
- **能力边界（如实）**：stable 供完整字段表；**lazer 有 7 条字段缺口**——mods 三槽、`play.hits`/`resultsScreen.hits`、`files.background`/`files.audio`，逐条进 `sources.osu.degradedFields` 且**绝不发占位值**。因此 lazer 上 `modSignature` 只在 NM 时与 tosu 逐字节相等，hits/livePP 面不可用；`state.name`/`number` 已接通并与 tosu 逐值相等。
- **分发形态**：lazer 偏移表随壳分发，落点 `<壳 exe 目录>\lazer-offsets\<lazer>__<runtime>__<arch>.json`（`MMA_LAZER_OFFSETS` 可覆盖）。**`desktop/release.ps1` 当前只复制 `mma-shell.exe`，不会自动带上 `lazer-offsets/`**，打包时需手工复制；缺表时 lazer 报 `lazer-offsets-missing:<ver>` 并回落 tosu（stable 不受影响）。
- **运行位置**：受限会话里 Toolhelp32 只能看到自己那一族进程（游戏"找不到"）——壳与生成器都要**从工作区外（如 `%TEMP%`）启动**，这是真机实测的运行约束，不是代码路径变更。

## 4. 兼容性

- **帧契约 v6 是"只升一侧就出事"的那一类**：页面 `bridgeClient.js` 用 `[MIN_ACCEPTED_CONTRACT, CONTRACT_VERSION] = [3,6]` 判定 hello。**旧页面（常量 5）+ 新壳（发 6）⇒ 越界终态**（页面提示更新插件并停止重连，不只是数据不通，窗口拖动/置顶/穿透/关闭快捷键一起失效）。因此本次插件版本号同步 bump 到 **2.2.0**，陈旧页用户能在 tosu 插件管理界面看到版本变化。
- **新页面 + 旧壳（v3–v5）**：区间下界仍是 3，握手成功；旧壳没有 `sources.osu` ⇒ 页面保持"无运行时覆盖"，`getSocketHost()` 回到 `wsEndpoint`，行为与改动前一致（逐字段存在性降级）。
- **配置数据**：无破坏性迁移。壳配置新增可选键 `osuTransport`（`auto`/`native`/`tosu`，缺省 `auto`）；旧配置文件没有该键 = `auto`。插件 `settings.json` **未新增任何设置项**（传输开关属壳配置，DEC-11）。
- **回滚**：① `mma-shell-config.json` 写 `"osuTransport": "tosu"` ⇒ 壳不打开任何内存句柄、页面回到 `wsEndpoint`，行为等同改动前（当前进程内已建立的连接会被 `socket.setHost` 关掉）；② `git revert` 本特性的提交（页面守卫与端点下发都是新增分支，回退即恢复旧行为）；③ 契约 v6 回退须**两侧同一提交**回退（`desktop/src/frames.rs` + `js/app/sources/bridgeClient.js`），且插件版本号随之回退；④ 删除 `docs/features/osu-native-source.md` 及其索引行，删除 `docs/breakings/2026-09-30-osu-native-memory.md` 及其索引行。
- **未纳入本次回滚面**：`tools/lazer-offsets-gen/`（dev-time 工具，入库但不在发布产物内；保留不影响任何运行路径）。

## 5. 验证方式

- **构建**：壳 `cargo build` 干净通过（0 warning）。
- **单测**：壳 `cargo test` 的失败名集合与改动前基线**逐名相同**（36 项，全部是沙箱 `temp_dir` 的既有环境失败；新增用例全绿）。osu 家族（`cargo test osu::`）单独跑全绿。
- **stable 数据面对拍（真机）**：`state.name` / `beatmap.md5`（= checksum）/ `identity` 在 382 条采样上逐字节相等；引导会话 3119 条 / 21 张图稳态 3107/3107（100%）三字段全等；12 条不一致逐条证明为跨状态切换的采样相位差（我方值出现在相邻 ±3 条的对侧，12/12 成立）。
- **lazer 数据面对拍（真机）**：`client` / `checksum` / `title` / `version` / `artist` / `mapper` / `id` / `set` / `files.beatmap` 与同刻 tosu 逐字节相等；`state.name`/`state.number` 逐值相等（62 s / 400 帧 / 0 脱附）；`beatmap.time.live` 差 38 ms（两侧独立采样）。
- **切换与缓存（真机，SC3）**：强制切回 tosu 时 `resultCacheGeneration()` 不变（1→1）、页面未重载、无 `clearResultCache` 命中 ⇒ 传输切换**不清结果缓存**。
- **端到端（真机两轮）**：路径 A（tosu 在线，native 驱动）：卡片正常渲染（星数 23.67 / `LN%: 67.6%` / Companella 结果），`.osu` 与封面均来自 24062，页面仍在 24061；路径 B（tosu 关闭，419 s / 16 张图 / 6 种 mod 配置 / 4 个状态）：全部由 native 驱动，页面错误面 0，新增控制台错误只是对已关闭 24050 的 WS 重连失败（无新种类）。
- **失败语义（破坏性签名测试）**：变体 A（改锚点模式一个字节）⇒ 门报 `signature-miss:statusPtr`、**0 帧伪造值**；变体 B（状态索引 +100，模拟读到垃圾）⇒ 先字段级冻结（帧仍带 `state.name`、无 `beatmap`），再到 `unhealthy` 停帧；从锚点解出到 unhealthy 约 1.4 s。
- **端点自证**：Host 头非本机 ⇒ 403（带 ACAO）；未知路径 404；两条文件路由 200 且 body 的 MD5 与磁盘一致；`/websocket/v2` 握手 101、回放模式 32 帧/5 s、帧间隔 153–158 ms；`/websocket/commands` 101 且 0 帧（黑洞）。
- **零写入审计**：`desktop/src/osu` 内 `WriteProcessMemory|VirtualProtect|CreateRemoteThread|VirtualAllocEx|SetWindowsHookEx` 0 命中；句柄权限常量恒为 `PROCESS_VM_READ | PROCESS_QUERY_INFORMATION`。
- **不变量**：`CONTRACT_VERSION` 三处一致（`frames.rs` / `bridgeClient.js` / `CONTRACT.md` 均为 6）；`index.js` `_VERSION` 与 `metadata.txt` `Version` 均为 **2.2.0**；页面 9 个改动文件用 ES module 语法检查逐文件通过（`node --input-type=module --check`，并带破损样本对照）；浏览器模式（无壳）回归轴逐字节不变。

---

# 2026-09-30 — osu! native memory transport (in-shell read-only reader + the 24062 compatible origin, contract v5→v6)

> Breaking-change record. Format: what changed / why / scope / compatibility / verification.
> Full domain documentation (architecture, failure semantics, both operating manuals, self-check list) lives in [docs/features/osu-native-source.md](../features/osu-native-source.md); the authoritative frame contract is `desktop/docs/CONTRACT.md` §14.

## 1. What changed

| Surface | Before | Now |
|---|---|---|
| osu data plane | one transport only (tosu, `wsEndpoint`, default 24050) | **two**: the in-shell **read-only memory reader** (`desktop/src/osu/**`, dispatched by bitness: 32-bit stable signature scan / 64-bit lazer offset table) serves a **tosu-compatible subset origin on 24062** (native, preferred); tosu remains the fallback and the page returns to `state.wsEndpoint` when `mode:"tosu"` |
| New port | 24050 / 24060 / 24061 / 17653 | **+ 24062**: WS `/websocket/v2` (one frame per 150 ms), WS `/websocket/commands` (black hole), `GET /files/beatmap/{file,background}`; every response carries `Access-Control-Allow-Origin: *`, the Host gate admits only local 24062; `MMA_OSU_COMPAT_REPLAY=1` restores the fixed-chart replay (testing), live payload is the default |
| Contract version | `CONTRACT_VERSION = 5`, page accepts `[3,5]` | **`6`**, page accepts `[3,6]` (`desktop/src/frames.rs` and `js/app/sources/bridgeClient.js` bumped together, `desktop/docs/CONTRACT.md` synced) |
| state frame | `sources{etterna,malody,malody4}` | **+ `sources.osu`**: `{alive, osuTransport{mode,host,port,wsPath,filesPath}, gate?, client?, reason?, degradedFields?, phase?, notice?, progress?}`; `alive ≡ mode == "native"`; pushed **the moment a value changes** (2 s detection, not the 30 s periodic frame) |
| Page switching | endpoints came from settings only | contract v6: the page switches at the **socket layer** (`appContext.applyOsuTransport` → `socket.setHost()`) and **never writes `state.wsEndpoint`** (it sits in `SETTING_CACHE_KEYS`, so the settings path would clear the result cache on every switch); only the **shell page** (24061) accepts the override, and only when `wsPath`/`filesPath` match verbatim — otherwise it refuses (fail-closed) |
| Shell main window | online (tosu alive) ⇒ the tosu plugin page; offline ⇒ 24061 | **always 24061 by default**; only an explicit shell-config `"osuTransport": "tosu"` (the escape hatch for AV false-positive triage) restores the old policy. Reason: with the main window on the tosu page neither the endpoint delivery nor the bridge is reachable, so the native transport would never activate (DEC-21 defect ②) |
| osu gate | `isOsuSuppressed()`'s first line looked only at `shellTosuOnline` | now also honours `state.shellOsuNativeAlive` (the "override actually took effect" bit); otherwise native frames are silently buffered as "losing-side" frames and the user-visible symptom is that **chart changes stop updating** (DEC-21 defect ①) |
| Connections on endpoint switch | `setHost` walked the URL-keyed map | closes **every socket ever created** (`socket.allSockets`); `/websocket/commands` is opened twice, so walking the map alone leaves the overwritten one behind (A11) |
| Cold-start UX | the card sat silent for tens of seconds during the scan | new page element `#osu-scan-hint`: the shell publishes `phase`/`notice`/`progress` (closed phase set + English sentence + measured scan readings); the hint writes **only its own element** (borrowing the shared status line made it flash for ~300 ms), never comes back within the same phase value once payload arrived, and only shows on the shell page |
| Beatmap-file fetch failures | the results screen showed `Request failed with status 404` and self-healed after ~2 s | shell side: a transient identity-pointer failure now enters a **2.5 s identity hold** (serves the last good `beatmap` block plus this frame's `state`, and the file routes serve that same chart with 200); page side: a transient failure (404/5xx) on the shell's native endpoint is **silently retried once** (the tosu path is byte-identical to before) |
| Plugin version | 2.1.0 | **2.2.0** (`index.js` `_VERSION` and `metadata.txt` `Version` together; a feature addition, hence the repo's minor bump) |
| Zero-write constraint | — | the reader only declares `ReadProcessMemory` / `VirtualQueryEx` / Toolhelp32 with handle access fixed at `PROCESS_VM_READ \| PROCESS_QUERY_INFORMATION`; dump collection exists only in the dev-time `tools/lazer-offsets-gen/` (in-repo) and nowhere in the product |

## 2. Why

- **Acceptance target**: with tosu closed and only the shell plus the game running, star rating / pattern graph / pause detection / livePP / results / chart changes / mod changes must all work. That requires the shell to carry the osu data plane itself (the native reader) rather than flipping a setting.
- **Why a "compatible origin" and not a fifth data source**: the page has a single host string (WS, `.osu` and the background image all derive from it), so "add file endpoints only" cannot drive the card; and a fifth source (`osun:` identity, new frame types) would touch routing, cache keys, play state, time and pause signals at once while the two caches would not interoperate (REJ-03). The compatible origin needs zero page changes and yields the same `identity`/`modSignature` shapes, so it **shares the result cache** with tosu mode.
- **Why the endpoint must be delivered at the socket layer**: `wsEndpoint` is in the page's `SETTING_CACHE_KEYS`, so any "change a setting" path triggers `clearResultCache()`; the runtime override is stored separately in `state.runtimeOsuHost`, which brings the switch cost to zero (measured: `resultCacheGeneration()` unchanged across a switch).
- **Why the main window had to change**: endpoint delivery and the bridge only work on the shell's own page; with the main window on the tosu page a healthy native transport could never reach it.
- **Why tosu stays**: when native is unhealthy, 24062 failed to bind, or the reader never attached (game not running) the page must keep working; `osuTransport: "tosu"` is also the one-switch escape hatch that opens no memory handle at all.
- **Licence hygiene**: no code or table is copied from tosu/gosumemory; every stable anchor and every lazer offset carries its own derivation and on-machine proof (anchor ledger `desktop/src/osu/patterns.rs`; the offset table is emitted only under the SOS + IL two-witness rule).

## 3. Scope

- **Shell + shell page only**: the browser mode (no shell, the 24050 page) is byte-identical to before — endpoint overrides are accepted only by the shell page (`isShellPage()` gate) and `state.wsEndpoint` plus the result-cache semantics are untouched.
- **Nine page files changed** (`appContext.js`, `socket.js`, `settings.js`, `sources/shellState.js`, `sources/sourceManager.js`, `sources/bridgeClient.js`, `analysis.js` plus the new `sources/beatmapFetchRetry.js`, `sources/osuScanHint{,Rules}.js`, and the hint element in `index.html`/CSS); estimators, parsers and the cache algorithm are untouched.
- **User-visible changes**: ① the main window lands on 24061 by default (no longer navigates to the tosu plugin page); ② a 13–23 s scan hint after cold start or a game restart; ③ the results-screen 404 flicker is gone (identity hold + one silent retry); ④ the NM chart that tosu's garbage `resultsScreen.mods` turns into DT renders correctly under native (we deliberately do **not** copy tosu's garbage value).
- **Capability boundary (honest)**: stable publishes the full field table; **lazer has 7 field gaps** — the three mod slots, `play.hits`/`resultsScreen.hits`, and `files.background`/`files.audio` — each reported in `sources.osu.degradedFields` with **no placeholder values ever emitted**. Consequently on lazer `modSignature` is byte-identical to tosu only for NM and the hits/livePP surface is unavailable, while `state.name`/`number` are wired and equal to tosu value-for-value.
- **Distribution shape**: the lazer offset table ships beside the shell at `<shell exe dir>\lazer-offsets\<lazer>__<runtime>__<arch>.json` (`MMA_LAZER_OFFSETS` overrides). **`desktop/release.ps1` currently copies only `mma-shell.exe` and does not carry `lazer-offsets/`**, so packaging must copy that folder by hand; without the table lazer reports `lazer-offsets-missing:<ver>` and falls back to tosu (stable is unaffected).
- **Where it runs**: inside a restricted session Toolhelp32 only sees its own process family (the game looks missing) — both the shell and the generator must be started **from outside the workspace (e.g. `%TEMP%`)**; that is a measured operating constraint, not a code-path change.

## 4. Compatibility

- **Contract v6 is the "one side only" kind**: the page validates hello against `[MIN_ACCEPTED_CONTRACT, CONTRACT_VERSION] = [3,6]`. **Old page (constant 5) + new shell (sends 6) ⇒ out of range ⇒ terminal state** (the page asks the user to update the plugin and stops reconnecting; not just data loss — the drag handle and the always-on-top/click-through/close shortcuts fail too). That is why the plugin version is bumped to **2.2.0** so stale-page users can see the change in tosu's plugin manager.
- **New page + old shell (v3–v5)**: the lower bound is still 3, so the handshake succeeds; an old shell has no `sources.osu`, the page keeps "no runtime override" and `getSocketHost()` returns `wsEndpoint` — behaviour identical to before (per-field existence degradation).
- **Configuration data**: no breaking migration. The shell config gains an optional `osuTransport` key (`auto`/`native`/`tosu`, default `auto`); an existing file without it means `auto`. The plugin `settings.json` gains **no new entries** (the transport switch is shell configuration, DEC-11).
- **Rollback**: ① set `"osuTransport": "tosu"` in `mma-shell-config.json` — the shell opens no memory handle, the page returns to `wsEndpoint` and behaviour matches the pre-change state (connections already established are closed by `socket.setHost`); ② `git revert` the feature's commits (the page guards and endpoint delivery are added branches, so reverting restores the old behaviour); ③ a contract v6 revert must revert **both sides in the same commit** (`desktop/src/frames.rs` + `js/app/sources/bridgeClient.js`) together with the version number; ④ delete `docs/features/osu-native-source.md` plus its index rows and `docs/breakings/2026-09-30-osu-native-memory.md` plus its index rows.
- **Not part of the rollback surface**: `tools/lazer-offsets-gen/` (a dev-time tool, in-repo but outside the release artefacts; keeping it affects no runtime path).

## 5. Verification

- **Build**: the shell's `cargo build` passes cleanly (0 warnings).
- **Tests**: the shell's `cargo test` failing-name set is **identical name-for-name** to the pre-change baseline (36 entries, all pre-existing `temp_dir` environment failures in the sandbox); every new case passes, and the osu family (`cargo test osu::`) is fully green.
- **stable data-plane comparison (on-machine)**: `state.name` / `beatmap.md5` (= checksum) / `identity` byte-equal over 382 samples; in the guided session 3107/3107 (100%) over 21 distinct charts; the 12 mismatches in an earlier run were each proven to be a sampling-phase artefact across a state change (our value appears on the other side within ±3 adjacent records, 12/12).
- **lazer data-plane comparison (on-machine)**: `client` / `checksum` / `title` / `version` / `artist` / `mapper` / `id` / `set` / `files.beatmap` byte-equal to tosu at the same instant; `state.name` / `state.number` value-equal (62 s / 400 frames / 0 detaches); `beatmap.time.live` differs by 38 ms (two independent samplers).
- **Switching and caching (on-machine, SC3)**: forcing a switch back to tosu leaves `resultCacheGeneration()` unchanged (1→1), the page does not reload and no `clearResultCache` fires ⇒ a transport switch does **not** clear the result cache.
- **End-to-end (two on-machine rounds)**: path A (tosu online, native drives): the card renders normally (star 23.67 / `LN%: 67.6%` / a Companella result), the `.osu` and the cover both come from 24062, and the page stays on 24061; path B (tosu closed, 419 s / 16 charts / 6 mod configurations / 4 states): everything is driven by native, the page's error surface is 0, and the only new console errors are WS reconnect failures against the already-closed 24050 (no new kinds).
- **Failure semantics (corruption tests)**: variant A (one byte of the anchor pattern changed) ⇒ the gate reports `signature-miss:statusPtr` with **0 fabricated frames**; variant B (state index +100, simulating a garbage read) ⇒ field-level freeze first (the frame still carries `state.name` and no `beatmap`), then `unhealthy` and no frames; anchors-resolved to unhealthy takes about 1.4 s.
- **Endpoint self-check**: a non-local Host header ⇒ 403 (with ACAO); unknown paths 404; both file routes 200 with body MD5 matching the file on disk; `/websocket/v2` handshakes 101 and delivers 32 frames per 5 s with 153–158 ms gaps in replay mode; `/websocket/commands` handshakes 101 and delivers 0 frames (black hole).
- **Zero-write audit**: `WriteProcessMemory|VirtualProtect|CreateRemoteThread|VirtualAllocEx|SetWindowsHookEx` ⇒ 0 hits under `desktop/src/osu`; the handle access constant is always `PROCESS_VM_READ | PROCESS_QUERY_INFORMATION`.
- **Invariants**: `CONTRACT_VERSION` is 6 in all three places (`frames.rs` / `bridgeClient.js` / `CONTRACT.md`); `index.js` `_VERSION` and `metadata.txt` `Version` are both **2.2.0**; the nine changed page files each pass an ES-module syntax check (with a broken sample as the negative control); the browser-mode (no shell) regression axis is byte-identical.