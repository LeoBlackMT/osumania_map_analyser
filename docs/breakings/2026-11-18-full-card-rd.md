# 2026-11-18 全卡 RD / MR 支持（Full-card Random & Mirror Support）

> 重大破坏性更改说明（双语；代码为准）。分支：`feat/full-card-rd`。
> 项目版本：**3.0.3**；壳-页面契约 v11 → **v12**；结果缓存版本 `star-v15` → **`star-v16`**。相关文档：[pipeline/mod-handling.md](../pipeline/mod-handling.md)、[pipeline/result-cache.md](../pipeline/result-cache.md)、[features/lane-acc.md](../features/lane-acc.md)。

## 中文

### ① 修改内容（What changed）

- **主分析全卡应用 RD / MR 列变换**（`js/pipeline/analysisPipeline.js`、`js/app/core/analysis.js`）：
  - 主分析（Sunny/Daniel/Azusa/Roxy/Polaris/XDC1/Mixed/Companella、Pattern、Ett、Interlude、Graph、PP、6K）在进入估算器与子模块前，统一执行 RD/MR 列变换。
  - **MR（Mirror）**：执行 `K - 1 - col` 镜像变换。
  - **RD（Random）**：按实际 seed 执行列置换。Lazer 采用 `ManiaModRandom` 置换；Stable 采用独立的拒绝采样置换与 `SpecialStyle` 首列规则。变换顺序固定为 `RD → MR`。
- **生命周期流转重算与冻结策略**：
  - **选歌 → Play（RD）**：进入 play 态后从 `ScoreInfo.ModsJson.settings.seed`（lazer）或 `Score.Seed`（stable/ppysb）读取真实 seed，并触发整卡全量重算。
  - **选歌 → ResultScreen（历史成绩）**：按该历史成绩自身的 `resultsScreen.mods` 与 `resultsScreen.seed` 重算，不再沿用当前选曲态 mod。
  - **Play / Replay → ResultScreen**：**不触发重算**，冻结本局 mod 与 seed；调用 `clearSection("results", {recompute: false})`，完整保留 LaneAcc 终态曲线与暂停标记。
- **缓存与契约升级**：
  - `CACHE_KEY_STAR_UNIFIED_VERSION` 升级至 `star-v16`，旧版本在 RD seed 键下的未置换快照全部自动失效。
  - `modSignature` 规范化为 7 段：`speedRate|odFlag|cvtFlag|classic|randomSeed|randomEngine|mirror`。
  - 壳-页面帧契约升级为 v12（`CONTRACT_VERSION = 12`，页面兼容区间 `[3, 12]`）；stable/ppysb 新增可选根字段 `play.seed` 与 `resultsScreen.seed`。
  - stable/ppysb 偏移表新增 `topology.score_seed = 0x80`；`tools/stable-offsets-gen` 同步维护该字段。

### ② 修改原因（Why）

- 原架构中 RD/MR 仅作用于 LaneAcc 击打事件与部分轻量预览，而核心难度估算（8 大估算器）、键型分析（Pattern）、Etterna MSD、Interlude、Graph 及 PP 均直接读取原始未置换音符流。这导致开 RD/MR 后的所有难度评级、能量曲线和键型分布依然显示原谱数值，与玩家实际打到的谱面严重失真。
- 引入全卡列变换后，难度估算、图表与键型分析与玩家实际游玩的轨位分布完全一致，大幅提升了随机/镜像模式下的数据权威性与可用性。

### ③ 影响范围（Scope）

- 影响所有启用 RD（Random）或 MR（Mirror）mod 的谱面分析流程。
- 未启用 RD/MR 的常规游玩（`randomSeed=""`、`randomEngine=""`、`mirror=0`）完全不受影响，计算输出逐位一致。
- 影响桌面壳 stable/ppysb 内存抓取（新增 `score_seed` 拓扑偏移）。

### ④ 兼容策略（Compat）

- 缓存键前缀升级为 `star-v16`，隔离旧版本快照，无需用户手动清理缓存。
- 缺省 `score_seed` 的旧版偏移表自动降级为无 seed 模式，不影响既有其他内存字段的读取与展示。
- 页面向下兼容 v3~v12 壳契约。

### ⑤ 验证方式（Verification）

- 自动化回归验证：覆盖 4K~8K 多键数下的 Lazer 与 Stable 置换算法，验证 `RD → MR` 组合变换的确定性与幂等性。
- 状态机流转验证：实测验证选歌 → Play 自动重算与 Play → ResultScreen 结果冻结，确认 LaneAcc 终态与暂停徽标不丢失。
- 偏移表与抓取验证：在 Stable 32 位与 Lazer 64 位环境下验证 seed 读取正确率，缺字段降级路径行为符合预期。

---

## English

### What changed

- **Full-card Random (RD) and Mirror (MR) column transformation across main analysis** (`js/pipeline/analysisPipeline.js`, `js/app/core/analysis.js`):
  - Main analysis pipelines (Sunny, Daniel, Azusa, Roxy, Polaris, XDC1, Mixed, Companella, Pattern, Ett, Interlude, Graph, PP, 6K) apply RD and MR column permutations before invoking estimators and submodules.
  - **MR (Mirror)**: Applies `K - 1 - col` mirror permutation.
  - **RD (Random)**: Applies column permutation according to the actual seed. Lazer uses `ManiaModRandom`; Stable uses independent rejection sampling with `SpecialStyle` scratch column rules. Permutation order is fixed at `RD → MR`.
- **Lifecycle state transition recomputation and freeze policy**:
  - **Song Select → Play (RD)**: Upon entering play state, the actual seed is acquired from `ScoreInfo.ModsJson.settings.seed` (lazer) or `Score.Seed` (stable/ppysb), triggering a full-card recompute.
  - **Song Select → ResultScreen (Historical Score)**: Recomputed using the score's own `resultsScreen.mods` and `resultsScreen.seed`, rather than inheriting song select mods.
  - **Play / Replay → ResultScreen**: **Does not recompute**, freezing the played mods and seed; calls `clearSection("results", {recompute: false})` to preserve LaneAcc final state and pause markers.
- **Cache and contract updates**:
  - `CACHE_KEY_STAR_UNIFIED_VERSION` bumped to `star-v16`, invalidating legacy untransformed snapshots under RD seed keys.
  - `modSignature` standardized to 7 segments: `speedRate|odFlag|cvtFlag|classic|randomSeed|randomEngine|mirror`.
  - Shell contract bumped to v12 (`CONTRACT_VERSION = 12`, page accepts `[3, 12]`); stable/ppysb adds optional root fields `play.seed` and `resultsScreen.seed`.
  - stable/ppysb offset tables add `topology.score_seed = 0x80`; synchronized in `tools/stable-offsets-gen`.

### Why

- Previously, RD and MR only transformed LaneAcc hit events and certain preview components, while primary difficulty estimation (all 8 estimators), pattern analysis, Etterna MSD, interlude, graph curves, and PP evaluated untransformed notes. This caused all ratings and distribution stats to reflect the vanilla chart rather than the actual played chart.
- Applying column transformation across the entire analysis pipeline brings full consistency between displayed metrics and the player's actual gameplay under Random and Mirror mods.

### Scope

- Applies to all beatmap analysis workflows with RD or MR mods enabled.
- Normal plays without RD/MR (`randomSeed=""`, `randomEngine=""`, `mirror=0`) remain completely unaffected with bit-identical outputs.
- Affects stable/ppysb memory reading in the desktop shell with the new `score_seed` offset.

### Compatibility

- Cache key version `star-v16` isolates legacy snapshots without manual cache clearance.
- Legacy offset tables lacking `score_seed` gracefully degrade without crashing or breaking other fields.
- Web overlay remains backward compatible with shell contract versions v3 through v12.

### Verification

- Automated test suites: verified Lazer and Stable column permutation algorithms across 4K through 8K charts, confirming determinism of `RD → MR`.
- State machine verification: confirmed automatic recomputation on Song Select → Play and result freezing on Play → ResultScreen, retaining LaneAcc final state and pause markers.
- Memory offsets verification: tested seed retrieval on 32-bit stable and 64-bit lazer; verified graceful degradation when seed offsets are absent.
