# 壳-页面桥契约（CONTRACT.md）

> 版本：**5**（变更即 v→v+1 重冻结；hello 帧 `contract` 字段 = 本版本号，页面不匹配则呈现终态提示并停止重连）。
> v1→v2 变更：新增 control 帧（窗口操控）与 diag 诊断旁路；所有 song 帧携带 requestId（POST `m{seq}` / Malody 文件通道 `r{seq}` / Etterna 轮询 `e{seq}`，v1 仅 POST 有）；result 帧字段对齐实现（`star`/`pattern`/`activeSource`/`updatedAt`，移除未实现的 `msd`/`graph`）；Origin/Host 校验改为 loopback host 精确匹配。
> v2→v3 变更：新增第四数据源 **Malody 4.3.7 原生客户端**（零注入只读观察通道）——新增 `malody4_selection` 帧（八型）；`source` 枚举新增 `"malody4"`；`requestId` 新增 `n{seq}` 前缀；`identity` 新增 `mdy4:{md5}`；`sources` 新增 `malody4{alive,playing,screen,reason?,judge?}`；`meta` 新增可选 `judge`（Malody 4 判定档字母）。
> v3→v4 变更：Malody V 通道新增**游戏内选曲桥**（BepInEx 插件 `POST /selection` → 壳 17653）——`sources.malody` 由 `{alive}` 扩为 `{alive,transport,screen,playing,eventSeq,judge}`；`song` 帧新增 `screen` / `judge` / `winScale`（仅桥通道携带）；`requestId` 新增 `b{eventSeq}`（真实事件）与 `b{eventSeq}r{k}`（重连补发）前缀；`identity` 的桥通道带 `u` 后缀（`mdy:…:{contentMd5}u`）；新增端口 **17653**；详情见 §11。**注**：v4 当时装的是上游插件，v5 起改为本仓库自建的 fork（§12），上面的历史描述保留原样。
> 定位：`desktop/` 与插件页面之间的协议实现规范（桌面壳内部文档，不入 docs/ 公开索引；文档 `docs/features/desktop-shell.md` 引用本文件，不复制）。

## 0. 帧信封

- 传输：壳 24061 单 listener 上的 WebSocket `/ws`；壳校验 Origin 仅 loopback——解析 Origin 的 host 段**精确匹配** `127.0.0.1` / `localhost` / `[::1]`（子串匹配会放过 `localhost.evil.com` 类域）；24061 HTTP 侧同样校验 Host 头仅本机 24061。**17653 是另一个独立 listener（Malody 选曲桥的入站面向游戏插件，不是页面通道），只认 `POST /selection`，Host 头必须精确等于 `127.0.0.1:17653` / `localhost:17653` / `[::1]:17653`，且**任何** `Origin` 头出现即 403（见 §11）。**
- 帧：`{v: 4, type, seq}`；`seq` 单向前递增。
- type 全集（八型职责语义闭合）：
  | type | 方向 | 职责 |
  |---|---|---|
  | hello | 壳→页 | 握手：`{tosuOnline, contract}` + 全量状态复位 |
  | state | 壳→页 | 30s 周期推送：`{tosuOnline, errors[], sources}` |
  | song | 壳→页 | 谱面到达（元信息 + 原文 + 身份） |
  | malody4_selection | 壳→页 | Malody 4.3.7 选曲记录（v3 新增）：`{path, speed_rate, screen, sequence, event, version, chart_hash, source}`——与参考桥（MalodyV）逐字对齐，`event ∈ anchor-changed / scene-changed / heartbeat / hidden`；`path` 为空 = hidden（未选中/不可用），原因另经 `sources.malody4.reason` 与壳日志给出 |
  | settings | 壳→页 | 设置推送（tosu 设置文件 mtime 变化 / 离线 POST 变更时主动推送载荷） |
  | result | 页→壳 | 分析结果（成功/失败/路由拒绝，见 §4） |
  | ping | 壳→页 | keepalive，间隔 15s；页面据此检测壳存活 |
  | control | 页→壳 | 窗口操控（v2 新增）：`{action, value?}`，action ∈ `close` / `alwaysOnTop` / `clickThrough` / `dragStart` / `toggleTopmost` / `toggleClickThrough`（后两者为 v2 行内扩展——Wayland 页面内快捷键兜底，页面与壳同版本发布故不升 v3；toggle 以 `mma-shell-state.json` 为权威读-翻-写，主线程执行）；壳经主窗口句柄执行（`server/ws.rs handle_control`） |

- diag（页→壳，**诊断旁路**）：`{type:"diag", payload:{message}}`——页面诊断日志直写壳 debug 日志，不参与任何状态机/应答，不计数入八型。
- 浏览器模式（无壳）：24061 不可达 → 页面不建壳通道，osu 单源（现状行为）。

## 1. song 帧 schema

```
{ requestId, source, identity, modData, meta, cover, rawText, screen?, judge?, winScale? }
```

- `requestId`：壳为**每个 song 帧生成**——POST `m{seq}`、Malody 文件通道 `r{seq}`、Etterna 轮询 `e{seq}`、Malody 4 锚点轮询 `n{seq}`、Malody 选曲桥真实事件 `b{eventSeq}`、桥重连补发 `b{eventSeq}r{k}`（v4，见 §11）；页面 result 帧原样回带，供应答关联（生命周期见 §4）。
- `source`：`"etterna" | "malody" | "malody4"`。
- `identity`：四源格式见 §5。
- `modData`：`{ speedRate: rate.toFixed(5)（无 rate 的外部源为 "1.0"）, odFlag: "none", cvtFlag: "none", classic: 0 }`——
  **外部源 modSignature 由页面 externalSource 直构，不走 modData 派生、与 client 值无关**（跨在线/离线模式签名稳定）；
- `meta`：`{ title, artist, version, keys, devMsd8, judge? }`（`devMsd8` = 桥 msd×8 数组，**仅开发对照，不进 modData**；页面显示 MSD 由 MinaCalc 自算）。
  `judge`：可选，Malody 4 源的判定档字母（`A`~`E`）；缺省表示未知（页面回落 C 档）。
- `cover`：白名单封面文件相对路径与同帧 URL（见 §7）。
- `rawText`：**源谱面原文（.sm/.ssc/.mc/.osu）**；**转换在页面侧**（.osu 直通，壳不转换），转换时机 = 页面缓存检查之后（命中快照短路免转换）；**体量上限：rawText 字节数 > 5MB 拒绝**（壳推送丢弃并经 state.errors 提示；POST 立即 504 PAYLOAD_TOO_LARGE；桥通道在读取前 stat，>5MB 立即 504 并留痕，见 §11）。
- `screen` / `judge` / `winScale`（v4，**只有 Malody 选曲桥通道携带**）：见 §11。`screen` 缺省即"不是桥帧"——页面据此判定卡片归属，故其它源与 Lua 通道**绝不出这三个字段**。

## 2. result 帧 schema

```
{ requestId, statusHint, star, pattern, activeSource, updatedAt, errors[] }
```

- `statusHint`：**页面可发三值** `success | analysis-failed | routing-reject`；
  `payload-too-large` / `timeout` **永不进页面帧**（壳在 HTTP 层直接 504 + 常量文本）。
- 发出点：**fetchBeatmapFile 的 finally 汇合**（非 stale 守卫；成功/失败/缓存命中/未命中四路统一；catch 路径汇入；浏览器模式无壳连接时 no-op）。
- `errors[]`：非空 = 失败（转换/解析/分析错误并入）；失败时壳回 500。
- 写门：无（皮肤状态文件已废弃；result 帧用于 24060 POST 应答与 Malody 文件通道的完成判定——壳据 `errors` 空/非空决定删除 request 文件或保留重试）。
- 帧以 Envelope 包裹发送；壳端 `parse_result_payload` 兼容 Envelope 与裸帧两种形态。
- `star` 取自渲染后的星数胶囊文本（首个数值，正则提取）；`pattern` = 当前模式标签（RC/LN/HB/Mix）。

## 3. HTTP 应答两种来源（24060 POST）

| 来源 | 条件 | 状态码与文本 |
|---|---|---|
| 页面帧驱动 | statusHint=success | 200 |
| 页面帧驱动 | statusHint=analysis-failed | 500 + `ANALYSIS_FAILED=「分析失败：{errors}」` |
| 页面帧驱动 | statusHint=routing-reject | 504 + `SOURCE_NOT_ACTIVE=「路由不可用：当前活跃源为 {X}」` |
| 壳自驱 | rawText > 5MB 字节 | 504 + `PAYLOAD_TOO_LARGE=「谱面文件过大（>5MB 字节）」` |
| 壳自驱 | 30s 超时 / 无页面连接 / 被取代 | 504 + `TIMEOUT=「分析超时（30s）」` |

- 页面不设 30s 定时器（超时只由壳判定）。
- 编辑器 `ShowMessage` 直用上述常量文本。

## 4. requestId 生命周期状态机

1. 壳收 POST → 生成 requestId → song 帧下发；
2. 页面 `fetchBeatmapFile` 函数开头快照外部源请求上下文（requestId 局部捕获，沿用函数开头既有的请求序号本地快照模式——`isStaleRequest` 守卫即该模式）；
3. 分析串行：新请求使旧 pending 失效——旧请求的 result 帧照发（携带**旧** requestId）；
4. 壳仅应答「仍 pending 且未被后继 POST 取代」的 requestId；其余静默忽略；
5. **状态变化→重算串行**：所有源（tosu 状态变化、外部源 song 帧）汇入同一 fetchBeatmapFile/requestSeq 单飞路径；分析结束后 activeSource 若已变更 → 卡片随后续 recompute 翻转（编辑器内已渲染的 malody 结果与屏幕卡片短暂不同步，属预期语义）。

## 5. 四源 identity（缓存键输入，均含内容摘要 md5）

- etterna：`ett:{stepFileStem}:{difficulty}:{meter}:{contentMd5}`（contentMd5 = 壳对谱面原文计算，非桥文件 mtime）
- malody：`mdy:{chartName|title}:{level}:{keys}:{contentMd5}`
- malody（选曲桥通道，v4）：`mdy:{title}:{level}:{keys}:{contentMd5}u`——`u` 后缀 = "上游桥通道"来源标记，与 Lua 通道区分。同一谱面在两条通道间切换时会多算一次（LRU 里两条快照），这是刻意的来源可追溯性取舍。
- malody4：`mdy4:{md5}`（md5 = 谱面文件字节摘要，即游戏自身使用的身份键；`mdy4:` 与 `mdy:` 是**两个不同源**的独立前缀，互不命中；md5 免疫曲名/文件名变动）
- osu：沿用现有 id/hash/path（不变）

## 6. settings

- 在线（tosu.env 存在且存活）：**设置权威 = tosu**——壳**只读** `{tosuRoot}/settings/{插件目录名}.json`（mtime 变化重读，30s 周期内生效，**绝不写**）；页面经 tosu getSettings/sendCommand 读写。
- 离线：**优先级链** = tosu 设置文件（离线也读）> **`mma-settings.json`**（exe 旁，全量插件设置；无 tosu 用户可直接编辑，重启生效；不存在则按插件 `settings.json` 生成默认骨架）——`/settings` GET 按链返回 + POST（页面收变更并**落盘 mma-settings.json**）；壳 30s 周期检测 `mma-settings.json` 与 `mma-shell-config.json` 变化 → 重载并推送 settings 帧；页面经 `applySettingsPayload` 注入。
- **壳配置 `mma-shell-config.json`**（exe 旁）：`gameClient`/`etternaRoot`/`malodyRoot`/`malody4Root`/`hotkeys`/`logLevel`——仅壳使用（源路径/快捷键/日志），与插件设置分离。
- settings.json（插件）始终是唯一 schema。

## 7. 封面白名单

- 白名单 = 桥数据上报的**具体封面文件路径**（仅图片扩展名），非整目录；
- 封面 URL 与白名单更新**同帧**下发（cover 字段）；非白名单路径 404。

## 8. state 帧

```
{ tosuOnline, errors[], sources: {
    etterna: { alive, playing, playingExpireAt },
    malody:  { alive, transport, screen, playing, eventSeq, judge },
    malody4: { alive, playing, screen, reason?, judge? }
} }
```

- `playing` 判定：gameplay 桥 playing 标志 + 外壳推过期——`playingExpireAt = 桥文件 lastWrite + total_seconds/rate×1.2 + 30s 裕量`；过期视为离开游玩态（防崩溃残留永驻 L1）；文档标注取舍：马拉松+长暂停可致误判离场，接受。
- `malody`（v4 六字段，见 §11）：`alive` = **桥存活（8s 内有过 POST，含心跳）或 Lua 通道存活（60s 内有过 POST/文件请求）**；`transport` = `bridge` / `lua` / `none`；`screen` = 桥最近一次真实事件的场景，**桥不存活时恒为 `none`**；`playing` = 桥存活且 `screen == "playing"`；`eventSeq` = 真实事件计数器（心跳与重复观察都不增）；`judge` = 判定档数值 `0..=4`（未采集到为 `null`）。
- `malody.screen` / `playing` 的桥存活门控是**必须的**：游戏在 `playing` 中途被关掉时桥不再发心跳，缺这层门控会永久上报 `playing: true`，页面 L1 会把路由钉死在 Malody。
- `malody4`（v3 新增）：`alive` = 本 tick 是否成功附着到唯一 `malody.exe` 并读到锚点；`playing` = 场景号 3（游玩）且新鲜（≤10s）；`screen` = 游戏场景名（`selection` / `playing` / `result` / `other`，仅在已知时出现）；`reason` = 不可用原因（正常/健康时**不出现**，闭集见下）；`judge` = 判定档字母（`A`~`E`，未知则不出现）。
- `malody4.reason` 闭集（逐字）：`chart-not-indexed`、`chart-unresolved`、`chart-unknown-identity`、`process-not-found`、`multiple-instances`、`access-denied`、`bad-read`、`target-mismatch:pe_timestamp_mismatch`、`target-mismatch:file_size_mismatch`、`target-mismatch:pe_header_out_of_range`、`target-mismatch:unknown`、`root-not-configured`、`no-library`、`platform-unsupported`。
  - `chart-unknown-identity`（2026-09-22 追加，**契约版本仍为 v3**）：这是本闭集的**向后兼容扩展**——`reason` 对页面是**不透明诊断串**（页面只把它存进诊断位 `state.malody4Reason`，不按取值分支），加值不改变任何页面行为，故不升 `CONTRACT_VERSION`。语义：这个身份键**已查过、且重建尝试（`MISS_REBUILD_ATTEMPTS`）已耗尽仍解析不出来**（本次会话不再为它重建）。相应地 `chart-not-indexed` 只表示**尚未试完**的未命中（库可能仍在建、文件可能刚落地）。
  - `chart-unresolved`（2026-09-22 追加，**契约版本仍为 v3**，同上：加值、不透明串、不改帧形态）：**两档提示的临时态**——高亮的这张谱**已经查过、当前查不到，但重试还没试完**（重建请求已发出 / 仍在 `INDEX_REBUILD_THROTTLE` × `MISS_REBUILD_ATTEMPTS` ≈ 10s 的重试窗口内）。存在的理由纯粹是**时间**：miss 在 200ms 内就能测出来，而结论要等窗口走完，中间这 ~10s 页面必须已经有话说（真机复测：旧行为下卡片提示要 9~10s 才出现）。页面对它显示"正在解析"这类临时提示，结论出来后才换成 `chart-unknown-identity`。重试本身不缩短、不取消（那是"游戏里刚导入的谱能被重新扫到"的唯一途径）。
  - `chart-not-indexed`：语义仍是"身份键在、索引里没有对应条目"（未试完/库可能仍在建）。轮询器自 `chart-unresolved` 起**不再自己发这个字面量**（miss 窗口被临时态覆盖）；它仍是选源状态机（`selection.rs`）在该情形下的内部成因、也仍是本闭集的合法取值（保留以便向后兼容与诊断）。
  - 三者（`chart-not-indexed` / `chart-unresolved` / `chart-unknown-identity`）对**帧形态与卡片行为完全相同**（hidden 记录、卡片保留上一张谱面）：它们只让壳日志与页面状态行给出不同的说明，绝不门控或隐藏卡片；区别只在诊断面能否分辨"库里没有这张谱"（结论）、"还在给你找"（临时）与"索引还没建好"。
- `malody4.reason` **变化即刻推 state 帧**（与 `alive` / `playing` / `screen` / `judge` 的变化一样）：不依赖 30s 周期帧，否则卡片上的临时提示与最终提示都要等下一拍周期才出现（真机复测的原始缺陷）。每 tick 值不变则**不**广播。
- `errors[]`：壳侧推送错误面（如 payload 超限被丢弃提示），页面 status 行展示。
- tosu 探测：`GET {ip}:{port}/` 健康探测，30s 周期重探测并推 state。

## 9. 皮肤状态文件（已废弃）

皮肤内显示方案（skin_script.lua + mma_state.txt 写入）已按用户拍板**移除**（2026-09 验收轮）：壳不再扫描 `{malodyRoot}/skin/`、不再写任何状态文件。Malody 结果只经编辑器插件（AddText）与页面卡片展示。

## 10. 变更流程

- 任何字段/语义变更：本文件 v+1；hello.contract 同步；页面版本不匹配呈现「契约版本不匹配，请更新插件」并停止重连（终态，防无限握手）。

## 11. Malody V 选曲桥（v4 起，v5 扩展）

游戏侧由 **本仓库自建的 BepInEx 插件** `MMAMalodySelection.dll`（GUID `local.mma.malody.selection`）推送 `POST /selection`。壳侧实现 = `desktop/src/server/bridge.rs`。插件源码在 `bridges/malody/bepinex/plugin/`（v2.1 数据桥的瘦身 fork，仅保留数据通道、不含游戏内叠加界面），构建方式与上游引用见其 `NOTICES.md`；HTTP 语义最初参照上游参考伴生程序 `malody_insight/service.py:283-299`，壳**重写实现**、不引入 Python。

> ⚠️ **不要与上游 `MalodyInsightBridge`（GUID `local.malody.insight.selection`）共存**：两者挂在同一批游戏方法上，会双 Hook。安装器发现上游文件时只报告、绝不删除。

### 11.1 v3→v4 差异

| 面 | v3 | v4 | 原因 |
|---|---|---|---|
| `CONTRACT_VERSION` / `hello.contract` / 帧信封 `v` | 3 | **4** | 新增字段与语义（§10 规则） |
| `state.sources.malody` | `{ alive }` | `{ alive, transport, screen, playing, eventSeq, judge }` | 场景语义与边沿清空需要机读字段 |
| `song` 帧 | 7 字段 | + `screen` / `judge` / `winScale`（**仅桥通道携带**） | 判定与桥事件异步，只放 state 帧会有 ordering 竞态 |
| `requestId` | `m/r/e/n{seq}` | + `b{eventSeq}`（真实事件）、`b{eventSeq}r{k}`（重连补发） | 桥事件与补发在日志里可区分 |
| `malody` identity | `mdy:…:{contentMd5}` | + `mdy:…:{contentMd5}u`（**只桥通道带后缀**） | 上游桥通道与 Lua 通道来源可追溯 |
| 监听端口 | 24060 / 24061 | + **17653** | 桥固定 POST 到 17653 |
| `malody.alive` 语义 | 仅 24060 POST 的 60s 窗口 | 桥 8s **或** Lua 60s（`transport` 说明是哪条） | 文件通道此前永不置真 |

### 11.2 桥协议（入站，17653）

请求门禁顺序**固定**（任一不过即拒，插队会被上游误判为"没收到"）：`Host` 必须精确为 `127.0.0.1:17653` / `localhost:17653` / `[::1]:17653` 否则 **403** → `path != "/selection"` ⇒ **404** → 任意 `Origin` 头存在 ⇒ **403** → 非 `POST` ⇒ **405** → `Content-Type` 的 type 不是 `application/json` ⇒ **415** → body 长度不满足 `0 < len <= 16384` ⇒ **413** → JSON 解析失败 ⇒ **400**。成功一律 **202**（插件只看 2xx，`Plugin.cs:701`）。

载荷 8 字段（全部按缺省容错）：`path` / `speed_rate` / `screen` / `sequence` / `event` / `version` / `chart_hash` / `source`。

| 字段 | 处理 |
|---|---|
| `screen` | 归一化到 `selection` / `playing` / `result` / `other`，未知值 ⇒ `other` |
| `speed_rate` | 非有限值或 `< 0.05` / `> 10` ⇒ **整帧按 `other` 处理**并记 error 日志 |
| `path` | `other` 时恒为空；其余场景必须过路径沙箱 |
| `sequence` | **只**用于识别心跳与游戏重启，不用于判事件（见 11.4） |
| `event` / `chart_hash` / `source` | 仅诊断，不进任何判据 |

### 11.3 路径沙箱（`screen ∈ {selection, playing, result}` 必过；`other` 跳过）

`normalize_path` → `canonicalize`（失败即拒）→ 必须 `starts_with(canonicalize(malodyRoot)/chart/)` → 后缀 ∈ `{.mc, .osu}`（大小写不敏感）→ `is_file()` → `1 B <= len <= 50 MB`。任一不过 ⇒ **403** + 壳日志（逃逸类记录含 `path outside malodyRoot/chart`）。**前缀校验不得放宽**：这是"本地 HTTP 端点不得成为任意文件读取原语"的唯一保证。

### 11.4 内容四元组去重 / 心跳 / 会话重置（唯一去重点）

基线三项与展示字段同处 `Shared::malody_bridge`（`last_event` / `last_seq` / `event_seq` / `last_seen` / `screen` / `chart_path` / `rate_text`）。**事件判据 = 内容四元组 `(path, rate_text, screen, judge_text)` 的变化**，`rate_text = rate.toFixed(5)`，`judge_text` 为当前判定档（未采集到为 `none`）。

| 条件 | 分支 | 动作 |
|---|---|---|
| `sequence == last_seq` **且**内容相同 | **心跳** | 刷新 `last_seen`，回 202；不发帧、不算事件、基线不动 |
| `sequence < last_seq` | **游戏重启** | 记一条 **info** 日志（文本固定：`malody bridge: sequence went backwards (A -> B) — game restarted, dedupe baseline cleared`）、清空 `last_event`，然后按正常规则处理本帧（⇒ 必为真实事件） |
| 内容相同但 `sequence` 不同 | **重复观察** | 刷新 `last_seen`，回 202，`last_seq = sequence`（基线前移）；不发帧、`event_seq` 不变 |
| 内容不同 | **真实事件** | 更新 `last_event` / `last_seq`，`event_seq += 1`，进入 11.5 |

- ⚠️ **不得只按 `sequence == last_seq` 判心跳**：重启前后首帧序号相同而内容不同时会被误吞且永不恢复；加上内容判据后该帧必然走真实事件。
- 三种"不发帧"分支都必须刷新 `last_seen`（心跳 = 桥还活着）。
- **除重启外不再对序号异常打日志**（早期"序号不在 recent 集合就 warn"会在每次真实选曲时误报）。

### 11.5 真实事件

读谱面文本（沙箱上限 50 MB）→ 写桥状态（`last_seen` / `screen` / `chart_path` / `rate_text`）→ `requestId = b{eventSeq}` → `identity = mdy:{title}:{level}:{keys}:{contentMd5}u` → 广播 `song` 帧 → **立即广播 `state` 帧**（不等 30s 周期帧）→ 回 202。

- **`screen = other` 不发 `song` 帧**：只更新桥状态并广播 `state` 帧；页面的清空由 `eventSeq` 边沿驱动。
- **两道闸显式分开**：沙箱允许 50 MB，而 `song` 帧的 `rawText` 受既有的 **5 MB**（`MAX_PAYLOAD_BYTES`）限制。因此**读文件前先 stat**：`> 5 MB` ⇒ **504** + 壳日志明写"谱面超过 rawText 上限，未广播"。**绝不允许"读进来又被自己的上限静默丢掉"**——5~50 MB 的谱面必须显式拒绝并留痕。

字段来源钉死（谱面可能是 `.mc` 也可能是 osu 导入的 `.osu`）：

| 字段 | 首选 | 回落 | 说明 |
|---|---|---|---|
| `level`（难度标签） | payload 的 `version`（游戏里的 `DisplayVersion`） | `.mc` 的 `/meta/version` → 文件名 stem | **不从 `.osu` 里猜**（osu 无 level 概念） |
| `keys` | `.mc` 的 `/meta/mode_ext/column` | `.osu` 的 `[Difficulty]` 段 `CircleSize`（壳侧简单扫描）→ `0` | `0` 时按 `post.rs` 既有做法回落 `4` |
| `title` / `artist` | `.mc` 的 `/meta/song/{title,artist}` | 文件名 stem / `""` | `.osu` 时回落文件名（与既有 Lua 通道行为一致） |
| `chart_hash` | 不使用 | — | 仅诊断；identity 用壳自算的 `contentMd5` |
| `judge` | 判定档采集通道（数值 `0..=4` ↔ A~E） | 未采集到 ⇒ `null` | 与 `winScale` 一起**随 song 帧**下发（判定与桥事件异步，只放 state 帧会有竞态） |
| `winScale` | 窗口缩放因子（Mod 掩码 + `speed_rate` 推出） | 判不出来 ⇒ `1.0` + info 日志 | 语义见插件文档「Malody V 判定窗口」一节 |

### 11.6 8s 存活语义

`bridge_alive = last_seen.elapsed() < 8s`（任意 POST，含心跳）。桥静默超时后：`alive` 退回 `lua_alive`（60s 窗口），`transport` 变 `lua` 或 `none`，**`screen` 恒为 `none`、`playing` 恒为 `false`**。上游约定"8s 未收到心跳应清除旧结果，且不得把进入游玩/结算当作断线"。

### 11.7 重连补发

新 WS 连接注册 sink 后（在 `hello` 之后）**只给这个连接**补发：

1. **一帧 `state`**（无条件）。注册 sink 原本只发 `hello`，而 `state` 帧只由 30s 周期帧或真实桥事件触发（`spawn_timers` = 15s ping + 15s sleep ⇒ **周期 30s**）⇒ 页面刷新/壳重启发生在游玩中时，`song` 帧回来了、`state.malodyPlaying` 也置了真，但 `state.malodyAlive` 会一直是 `undefined`，页面 L1 门控 `malodyPlaying && malodyAlive` 不成立，路由最长 30s 内仍指向 osu/Etterna。
2. **当前谱面的 `song` 帧**，触发条件钉死：`bridge_alive` **且** 已存 `screen ∈ {selection, playing, result}` **且** `chart_path` 非空。⚠️ 不能只判 `last_event.is_some()`——`screen=other` 的真实事件会把 `last_event` 覆盖成空 `path` 的四元组，那样会补发出一个空 `path` 的 song 帧，与 11.5 冲突。

补发**不是新事件**：不修改 `last_event` / `last_seq` / `event_seq`；内容按当前状态重算（重读谱面、重算 `contentMd5` 与 identity）；`requestId = b{eventSeq}r{k}`（`k` = 该连接的补发序号，当前每次注册只补发一次 ⇒ `k = 1`）。

### 11.8 页面接受版本区间（兼容矩阵）

页面**按字段逐个存在性降级**，不按版本号分支；版本号只用于决定"接受 / 终态"。

| 壳 `hello.contract` | 页面行为 |
|---|---|
| `5` | 全功能（本文件） |
| `4` | 接受：`sources.malody` 无 `pro`/`turbo`，桥帧也无这两个字段；`winScale` 恒为数值（旧语义）。页面据此关闭动态 OD 并说明原因 |
| `3` | 接受：`state.sources.malody` 只有 `alive`（`transport`/`screen`/`playing`/`eventSeq`/`judge` 一律 `undefined`），桥通道的 `song` 帧也没有 `screen`/`judge`/`winScale` ⇒ 页面不得进入桥的场景/清空/判定路径，退回"Lua 通道 + 旧形态" |
| `< 3` 或 `> 5` | 终态：呈现「契约版本不匹配，请更新插件」并停止重连（防无限握手） |

即：`MIN_ACCEPTED_CONTRACT = 3`、`CONTRACT_VERSION = 5`，区间为 `[3,5]`。

> ⚠️ **两个常量必须同步升版**：壳升而页面不升，`hello` 被判越界 ⇒ `contractOk` 为假 ⇒ `bridgeOnline()` 为假 ⇒ 除了数据帧不通，`sendControl()` 也会在首行返回，**窗口拖动把手与置顶/穿透/关闭快捷键一起静默失效**。这是本契约里唯一一处"只升一侧就出事"的地方。

### 11.9 端口 17653 被占用

`TcpListener::bind("127.0.0.1:17653")` 失败**不 panic、不 exit**：记一条 error 日志 + 把「Malody 选曲桥端口 17653 被占用，游戏内选曲跟随不可用」推入 `state.errors`，`bridge_listen_ok = false`，其余端点（24060/24061）照常。24060 的绑定失败按同一口径容错。游戏侧表现为插件按 15s 节流告警并重试（上游行为，无需本壳配合）。

## 12. v4 → v5 差异

变更动机与影响面见 [docs/breakings/2026-09-24-malody-v-selection-bridge-fork.md](../../docs/breakings/2026-09-24-malody-v-selection-bridge-fork.md)；本节只描述契约面的增删。

| 面 | v4 | v5 | 原因 |
|---|---|---|---|
| `CONTRACT_VERSION` / `hello.contract` / 帧信封 `v` | 4 | **5** | §10 规则 |
| `POST /selection` 载荷 | 8 字段 | **11 字段**：+ `judge_level` / `pro_judge` / `turbo`（全部 `#[serde(default)]` + 容错反序列化） | 判定档与 Pro 决定等效 OD；Turbo 决定窗口是否被补偿 |
| `song` 帧 | + `screen` / `judge` / `winScale`（数值） | + **`pro` / `turbo`（可为 `null`）**，且 **`winScale` 改为可为 `null`** | `null` 表达"未知"，与"确认非 Turbo 的 1.0"是两件事（§11.5） |
| `state.sources.malody` | 6 字段 | **8 字段**：+ `pro` / `turbo` | Auto 决策与源圆点要能看出当局是否 Turbo / Pro |
| 内容判据（真实事件） | 四元组 `(path, rate_text, screen, judge_text)` | **六元组**：+ `pro_text` / `turbo_text` | 选曲界面切 Turbo 时倍率不变，少了它会被当成"重复观察"而**不广播 song 帧**，页面 `winScale` 停在旧值 |
| 判定档来源 | `config.json` 的 `user_judge_level`（当局冻结） | **插件上报（权威）**，`config.json` 仅在插件未上报时兜底；两者不一致时用插件值 + **一条** info | 插件能实时读，文件只在启动时写 |
| Pro 来源 | 无 | **仅插件**（面板记录 → `Malody.Play.boq.get_bcgh` → 具名控件）；**绝不来自该文件**（该文件没有对应键） | 那正是两套判定窗口的唯一判别信息 |
| 冻结状态机 | `snapshot_action` / `apply_snapshot` / `frozen_judge` / `frozen_mod_mask` / `run_snapshot_taken` | **已删除**（插件实时值取代其用途） | 为"不实时文件"设计的机制随通道更换而作废 |
| 页面接受区间 | `[3,4]` | **`[3,5]`** | §11.8 |
| 游戏侧 cfg | 安装器创建 `local.malody.insight.selection.cfg` 并写 `[Overlay] Enabled = false` | **安装器不写任何 cfg**；`local.mma.malody.selection.cfg` 由 BepInEx 首次加载时自行生成 | fork 已无叠加界面 |

**向后兼容**：8 字段旧载荷 ⇒ 202 且三个新字段为 `null`，绝不因缺字段报 400（回归断言见 `smoke-malody-bridge.mjs` 第 11 节）。
