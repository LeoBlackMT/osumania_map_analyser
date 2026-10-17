# docs/breakings/2026-10-17-rc-difficulty-feedback.md

> 破坏性更改说明（人类/AI 共读）。插件版本 2.1.0 → **2.2.0**。

## 中文

### 1. 修改内容（What changed）

- 新增 **RC 难度反馈**：桌面壳结算屏底部的五档评分条 + 页面 gate + 壳侧 Ed25519 签名 + 后端严格入库，默认关闭（`enableDifficultyFeedback=false`）。
- 新增页面模块 `js/app/feedback/{feedbackSpec,feedbackMath,feedbackGate,feedbackStrip}.js` 与 `styles/feedback.css`；`index.html` 增加一个 `#feedback-strip` 容器。
- `analysis.js` 在写显示值的同时写 `state.feedbackContext`；`socketHandlers.js` 记录 `state.currentBeatmapId/Checksum` 并在谱面/ mod 变化时清空反馈上下文。
- 壳新增 `src/feedback/mod.rs` 与 `POST /feedback`（本机 24061，CORS 仅允许 24050/24061 页面）；`build.rs` 注入签名 seed；`startup_url()` 追加 `#mma-shell` 标记；`packet.rs` 增发 `beatmap.stats.objects.total`。
- 后端新增独立 `feedback.db`、`/api/v1/feedback`、osu! API 校验（50/min 全局预算、singleflight、正/负缓存）、聚合表、私有 admin 页/CSV、`cmd/feedback-keygen`；OBS 备份改为成对对象 + manifest。
- **mod 码表扩容**：`config.js` 的 `knownCodes/bitFlags` 与 `desktop/src/osu/keys.rs` 的 `KNOWN_MOD_CODES/MOD_BIT_FLAGS` 从 12 码/6 位扩到完整 mania 码表（含 RD/FL/NF/HD/AT/CN/`nK` 等）。

### 2. 修改原因（Why）

知识库 `docs/learnings/difficulty-estimation.md` §3.4/§6 指出：人工标签噪声封顶，继续堆特征或高容量模型会过拟合；同时多个算法在高倍速叠键、LN 长条等谱面上存在**同向共享盲区**，仅靠算法间融合无法跳出既有预测的凸包。RC 反馈用于**发现**系统性残差并筛选候选谱面，不作为在线校准值，避免 per-map 查表把测试集自由度吃掉。

### 3. 影响范围（Scope）

- 页面：关闭设置时无 UI、无网络、无 DOM 构建；浏览器 tosu 页与 OBS 源永不创建反馈条（只认 24061 端口或壳的 `#mma-shell` 标记）。
- 壳：`/feedback` 仅在 `resultScreen` 且全部 gate 通过时签名；不改变任何现有端点、窗口行为或数据源选择。
- 后端：`FEEDBACK_ENABLED=false` 时路由不注册、不打开 `feedback.db`；`telemetry.db` 的表结构、保留期、公开 dashboard 不变。
- 缓存：`modSignature` 仍只由 rate/OD/cvt/classic 四段构成，扩容 mod 码表**不改变** `star-v12` 缓存键。
- 显示：`numericDifficulty` / `estDiff` 的算法与显示路径完全未改。

### 4. 兼容策略（Compat）

- 旧版页面在壳内运行时，`#mma-shell` 只影响 `location.hash`，不改变 `search`/`pathname`、不影响相对资源路径与 `getCounterPathForCommand()`。
- 旧壳（无 `/feedback`）配新页面：反馈条出现但点击会得到 4xx，显示 `Not recorded`，不写本地去重；默认关闭时不会发生。
- 后端返回 404 当 `FEEDBACK_ENABLED=false` 或密钥不全；已有 `/api/v1/event` 与 dashboard 不受影响。
- `feedback.db` 可随时删除重建；OBS 旧备份（只有 `telemetry-*.db`）仍可单独恢复。
- 公开隐私契约新增第二类 opt-in 豁免，已同步 `CLAUDE.md`、`docs/features/telemetry.md`、`docs/settings.md`。

### 5. 验证方式（Verification）

- JS gate/math：46 条断言（ScoreV1、weight 表、RC/双槽、全部禁用 mod、rate、准度/完成率/miss 边界、去重）。
- Go：`internal/feedback` 的签名 golden、静态校验逐项拒绝码、入库/去重/nonce 复用、截尾中位数。
- Rust：签名字符串 golden、签名往返、weight 曲线、ScoreV1、install id 持久化、nonce 唯一性；mod 码表测试已按新表更新。
- 手工：stable/lazer 各一张 RC 图、禁用 mod/双槽/自定义 rate/断网路径仍待实机验证（见 PR #100 清单）。

---

# English

## 1. What changed

- Added **RC difficulty feedback**: a five-point strip at the bottom of the shell result screen, page-side gates, shell-side Ed25519 signing, and a strict ingest endpoint. Disabled by default (`enableDifficultyFeedback=false`).
- New page modules `js/app/feedback/{feedbackSpec,feedbackMath,feedbackGate,feedbackStrip}.js` plus `styles/feedback.css`; `index.html` gains one `#feedback-strip` container.
- `analysis.js` writes `state.feedbackContext` together with the displayed value; `socketHandlers.js` records `state.currentBeatmapId/Checksum` and clears the context on map/mod changes.
- Shell: `src/feedback/mod.rs` and `POST /feedback` (loopback 24061, CORS limited to the 24050/24061 pages); `build.rs` injects the signing seed; `startup_url()` appends a `#mma-shell` marker; `packet.rs` publishes `beatmap.stats.objects.total`.
- Backend: separate `feedback.db`, `/api/v1/feedback`, osu! API validation (50/min global budget, singleflight, positive/negative caches), aggregate tables, a private admin page/CSV, `cmd/feedback-keygen`; OBS backup now uploads paired objects plus a manifest.
- **Mod table expanded** from 12 codes / 6 bits to the full mania set in `config.js` and `desktop/src/osu/keys.rs` (RD/FL/NF/HD/AT/CN/`nK`, ...).

## 2. Why

`docs/learnings/difficulty-estimation.md` §3.4/§6 records that human labels have hit their noise floor and that heavier models overfit, while several estimators share the same blind spots on high-rate chordjacks and LN charts. Ratings are used to **find** systematic residuals and to shortlist maps for expert re-labelling — never as an online calibration value, so no per-map lookup table is built from the test set.

## 3. Scope

- Page: with the setting off there is no UI, no network call, and no DOM; browser pages and OBS sources never build the strip (only port 24061 or the shell's `#mma-shell` marker).
- Shell: `/feedback` signs only in `resultScreen` after every gate passes; no existing endpoint, window behaviour, or source selection changes.
- Backend: with `FEEDBACK_ENABLED=false` the route is not registered and `feedback.db` is not opened; `telemetry.db`, its retention, and the public dashboard are untouched.
- Cache: `modSignature` still consists of rate/OD/cvt/classic only, so the expanded mod table does **not** change the `star-v12` cache key.
- Display: `numericDifficulty` / `estDiff` algorithms and rendering are unchanged.

## 4. Compatibility

- The `#mma-shell` marker only touches `location.hash`; `search`, `pathname`, relative assets, and `getCounterPathForCommand()` are unaffected.
- An old shell (no `/feedback`) with a new page shows the strip but returns 4xx on click (`Not recorded`) and does not write the local dedup marker; with the default setting off this never happens.
- The backend returns 404 while `FEEDBACK_ENABLED=false` or keys are incomplete; `/api/v1/event` and the dashboard keep working.
- `feedback.db` can be deleted and rebuilt; old OBS backups containing only `telemetry-*.db` remain restorable.
- The privacy contract gains a second opt-in exemption, synced to `CLAUDE.md`, `docs/features/telemetry.md`, and `docs/settings.md`.

## 5. Verification

- JS gate/math: 46 assertions (ScoreV1, weight table, RC/single-slot, every banned mod, rate, accuracy/completion/miss boundaries, dedup).
- Go: sign-string golden, one rejection test per static rule, insert/dedup/nonce-reuse, trimmed weighted median.
- Rust: sign-string golden, sign/verify roundtrip, weight curve, ScoreV1, install-id persistence, nonce uniqueness; the mod-table test was updated to the new table.
- Manual stable/lazer checks (RC chart, banned mod, double slot, custom rate, offline) are still pending; see PR #100.

[返回 docs 索引](../README.md)
