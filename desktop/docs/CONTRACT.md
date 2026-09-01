# 壳-页面桥契约（CONTRACT.md）

> 版本：**1**（变更即 v→v+1 重冻结；hello 帧 `contract` 字段 = 本版本号，页面不匹配则呈现终态提示并停止重连）
> 定位：`desktop/` 与插件页面之间的协议实现规范（桌面壳内部文档，不入 docs/ 公开索引；文档 `docs/features/desktop-shell.md` 引用本文件，不复制）。

## 0. 帧信封

- 传输：壳 24061 单 listener 上的 WebSocket `/ws`；壳校验 Origin 仅 loopback。
- 帧：`{v: 1, type, seq}`；`seq` 单向前递增。
- type 全集（六型职责语义闭合）：
  | type | 方向 | 职责 |
  |---|---|---|
  | hello | 壳→页 | 握手：`{tosuOnline, contract}` + 全量状态复位 |
  | state | 壳→页 | 30s 周期推送：`{tosuOnline, errors[], sources}` |
  | song | 壳→页 | 谱面到达（元信息 + 原文 + 身份） |
  | settings | 壳→页 | 设置推送（tosu 设置文件 mtime 变化 / 离线 POST 变更时主动推送载荷） |
  | result | 页→壳 | 分析结果（成功/失败/路由拒绝，见 §4） |
  | ping | 壳→页 | keepalive，间隔 15s；页面据此检测壳存活 |

- 浏览器模式（无壳）：24061 不可达 → 页面不建壳通道，osu 单源（现状行为）。

## 1. song 帧 schema

```
{ requestId, source, identity, modData, meta, cover, rawText }
```

- `requestId`：POST 场景由壳生成（生命周期见 §4）；推送场景可为空。
- `source`：`"etterna" | "malody"`。
- `identity`：三源格式见 §5。
- `modData`：`{ speedRate: rate.toFixed(5)（Malody 无 rate 时 "1.0"）, odFlag: "none", cvtFlag: "none", classic: 0 }`——
  **外部源 modSignature 由页面 externalSource 直构，不走 modData 派生、与 client 值无关**（跨在线/离线模式签名稳定）；
- `meta`：`{ title, artist, version, keys, devMsd8 }`（`devMsd8` = 桥 msd×8 数组，**仅开发对照，不进 modData**；页面显示 MSD 由 MinaCalc 自算）。
- `cover`：白名单封面文件相对路径与同帧 URL（见 §7）。
- `rawText`：**源谱面原文（.sm/.ssc/.mc/.osu）**；**转换在页面侧**（.osu 直通，壳不转换），转换时机 = 页面缓存检查之后（命中快照短路免转换）；**体量上限：rawText 字节数 > 5MB 拒绝**（壳推送丢弃并经 state.errors 提示；POST 立即 504 PAYLOAD_TOO_LARGE）。

## 2. result 帧 schema

```
{ requestId（可空：非 POST 推送场景为空）, statusHint, star, pattern, msd, graph,
  activeSource, updatedAt, errors[] }
```

- `statusHint`：**页面可发三值** `success | analysis-failed | routing-reject`；
  `payload-too-large` / `timeout` **永不进页面帧**（壳在 HTTP 层直接 504 + 常量文本）。
- 发出点：**fetchBeatmapFile 的 finally 汇合**（非 stale 守卫；成功/失败/缓存命中/未命中四路统一；catch 路径汇入；浏览器模式无壳连接时 no-op）。
- `errors[]`：非空 = 失败（转换/解析/分析错误并入）；失败时壳回 500，且不写 mma_state.txt。
- 写门：`{malodyRoot}/skin/` 状态文件**仅当帧内 `activeSource === "malody"` 且 errors 为空**时写入（与 requestId 无关）。

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

## 5. 三源 identity（缓存键输入，均含内容摘要 md5）

- etterna：`ett:{stepFileStem}:{difficulty}:{meter}:{contentMd5}`（contentMd5 = 壳对谱面原文计算，非桥文件 mtime）
- malody：`mdy:{chartName|title}:{level}:{keys}:{contentMd5}`
- osu：沿用现有 id/hash/path（不变）

## 6. settings

- 在线（tosu.env 存在且存活）：**设置权威 = tosu**——壳**只读** `{tosuRoot}/settings/{插件目录名}.json`（mtime 变化重读，30s 周期内生效，**绝不写**）；页面经 tosu getSettings/sendCommand 读写。
- 离线：**优先级链** = tosu 设置文件（离线也读）> **`mma-settings.json`**（exe 旁，全量插件设置；无 tosu 用户可直接编辑，重启生效；不存在则按插件 `settings.json` 生成默认骨架）——`/settings` GET 按链返回 + POST（页面收变更并**落盘 mma-settings.json**）；壳 30s 周期检测 `mma-settings.json` 与 `mma-shell-config.json` 变化 → 重载并推送 settings 帧；页面经 `applySettingsPayload` 注入。
- **壳配置 `mma-shell-config.json`**（exe 旁）：`gameClient`/`etternaRoot`/`malodyRoot`/`hotkeys`/`logLevel`——仅壳使用（源路径/快捷键/日志），与插件设置分离。
- settings.json（插件）始终是唯一 schema。

## 7. 封面白名单

- 白名单 = 桥数据上报的**具体封面文件路径**（仅图片扩展名），非整目录；
- 封面 URL 与白名单更新**同帧**下发（cover 字段）；非白名单路径 404。

## 8. state 帧

```
{ tosuOnline, errors[], sources: { etterna: { alive, playing, playingExpireAt }, malody: { alive } } }
```

- `playing` 判定：gameplay 桥 playing 标志 + 外壳推过期——`playingExpireAt = 桥文件 lastWrite + total_seconds/rate×1.2 + 30s 裕量`；过期视为离开游玩态（防崩溃残留永驻 L1）；文档标注取舍：马拉松+长暂停可致误判离场，接受。
- `malody.alive` = 最近 POST/song 时间仍在 60s 窗口内。
- `errors[]`：壳侧推送错误面（如 payload 超限被丢弃提示），页面 status 行展示。
- tosu 探测：`GET {ip}:{port}/` 健康探测，30s 周期重探测并推 state。

## 9. 皮肤状态文件（mma_state）与哨兵

- 哨兵：皮肤目录内存在 `mma.txt`（用户按安装文档在 skin_script.lua 旁创建）；壳扫描 `{malodyRoot}/skin/` 下含哨兵的目录，命中多个全部写入（幂等）。
- 写入：壳收到 result 帧（activeSource=malody 且 errors 空）后原子写（tmp+rename）到 `{malodyRoot}/skin/{皮肤名}/mma_state.txt`。
- schema：KV 文本——`star` / `pattern` / `msd` / `graph` / `client` / `updatedAt`。

## 10. 变更流程

- 任何字段/语义变更：本文件 v+1；hello.contract 同步；页面版本不匹配呈现「契约版本不匹配，请更新插件」并停止重连（终态，防无限握手）。