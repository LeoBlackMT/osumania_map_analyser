# 2026-08-30 多数据源与桌面壳（multi-source & desktop shell）

## 修改内容（What changed）

1. 新增外部数据源：Etterna/Malody（转换器 `js/parser/{smSscToOsuConverter,mcToOsuConverter}.js`、页面 `js/app/sources/`、壳 `desktop/`、桥 `bridges/`）。
2. `analysis.js` `fetchBeatmapFile`：新增外部文本入口（`state.pendingSourceText` 绕过 tosu 抓取）与 result 帧 finally 汇合（四路，浏览器无壳 no-op；外部源 result 与 stale 解耦——仅外发、无页面副作用）。
3. 缓存键语义：外部 identity 带源前缀+内容 md5（`ett:`/`mdy:`），外部 modSignature 由 externalSource 直构（speedRate 段 = rate 派生），`gameClient` 加入 `SETTING_CACHE_KEYS`。
4. 设置管线：新增 `gameClient`/`etternaRoot`/`malodyRoot`；socketHandlers 拆分信号/状态段并加败方门控（含缓冲回放）。
5. 遥测：analyze 事件新增 `client` 字段；后端 daily_agg 新增 `client` 维度与 dashboard Client 饼图（Version 单开一行）。
6. shell 桥契约：`desktop/docs/CONTRACT.md`（v2，含窗口控制帧；本仓库新增项目，不影响浏览器模式）。

## 修改原因（Why）

社区玩家同时游玩 osu!mania/Etterna/Malody V；需求为难度分析卡自动跟随当前游戏。算法层保持零改动（转换器输出 .osu 进既有管线）；浏览器模式行为不变（无壳时自动 osu 单源）。

## 兼容性影响（Impact）

浏览器模式（无壳/tosu）：除新增三个设置项与圆点外无行为变化；routing/缓存键在无外部源时不改变。外部源场景：osu 被压制时 identity/mod 写入挂起（信号仍读），切回 osu 回放最后一条 tosu 状态（缓存键对齐）。老遥测事件（无 client 字段）：后端静默忽略该维度，不产生新段。版本：2.0.2 → 2.1.0。CI：桌面壳构建流水线仅监听 main 的 `desktop/**`。

## 回滚方式（How to roll back）

自 `feat/multi-source-shell` 相对 main 的提交整体 revert；删除 `desktop/`、`bridges/`；恢复 settings.json/config.js 原状；浏览器模式立即回到基线。

## 验证（Verification）

转换器 golden 50/50、壳桥 16/16、Etterna 轮询 11/11、Malody skin 9/9、路由 14/14、遥测 7/7（本地冒烟，脚本不入库）；WebView 端到端（Malody POST → 页面分析 → result 200，缓存命中 0.06s）；浏览器端到端与真机 PoC 清单见 `docs/features/multi-source.md`。

---

# 2026-08-30 Multi-source data sources & desktop shell (EN)

1. Added Etterna/Malody data sources (converters, page `sources/`, `desktop/` shell, `bridges/` Lua).
2. `fetchBeatmapFile` external-text entry + result frame finally merge (four paths, no-op without bridge; result decoupled from stale guard).
3. Cache semantics: external identity prefix+content-md5, external modSignature direct-build (speedRate = rate), `gameClient` in cache invalidation keys.
4. Settings pipeline: `gameClient`/`etternaRoot`/`malodyRoot`; socketHandlers split + osu suppression gate (buffered replay).
5. Telemetry: `client` field, daily_agg `client` dim, dashboard Client pie (Version own row).
6. Shell bridge contract `desktop/docs/CONTRACT.md` v2 (window control frame; new asset; browser mode untouched).

Reason: multi-game community; auto-follow card. Algorithm layer unchanged; browser mode unchanged (osu single-source without shell). Impact: browser mode unaffected besides new settings + dot; external sources gate osu state writes (signals kept, buffered replay); legacy telemetry events ignored for the new dim; version 2.0.2 → 2.1.0; CI pipeline listens to main `desktop/**` only.

Rollback: revert `feat/multi-source-shell` vs main; drop `desktop/`, `bridges/`; restore settings/config. Verification: local smokes 50/50+16/16+11/11+9/9+14/14+7/7 (scripts kept local, not committed); WebView end-to-end POST→analysis→result 200 (0.06s cached); live PoC list in `docs/features/multi-source.md`.