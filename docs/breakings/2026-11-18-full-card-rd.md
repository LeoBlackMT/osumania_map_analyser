# 2026-11-18 全卡 RD / MR 支持（star-v16 / 契约 v12）

> 性质：功能 + 行为修正；不涉及数据迁移。相关文档：[pipeline/mod-handling.md](../pipeline/mod-handling.md)、[pipeline/result-cache.md](../pipeline/result-cache.md)、[features/lane-acc.md](../features/lane-acc.md)。

## 1. 行为变化

- 主分析（Sunny/Daniel/Azusa/Roxy/Polaris/XDC1/Mixed/Companella、Pattern、Ett、Interlude、Graph、PP、6K）现在会在进入估算器前应用 RD/MR 的列变换：
  - **MR**：`K-1-column` 镜像；
  - **RD**：按实际 seed 做 `ManiaModRandom`（lazer）或 stable mania Random 的列置换；
  - 顺序为 `RD → MR`。
- **选歌 → play（RD）**：进入 play 后从 `ScoreInfo.ModsJson.settings.seed`（lazer）或 `Score.Seed`（stable/ppysb）取实际 seed，重算整卡。
- **选歌 → resultScreen（历史成绩）**：按该成绩自己的 `resultsScreen.mods` 重算，避免沿用当前选歌 mod。
- **play/replay → resultScreen**：**不重算**，冻结本局 mod/seed；`clearSection("results", {recompute:false})`，保留 LaneAcc 终态与暂停标记。
- **LaneAcc**：Lazer/Stable 都支持 RD；Stable 使用独立的拒绝采样置换与 `SpecialStyle` 首列规则。

## 2. 缓存与契约

- `CACHE_KEY_STAR_UNIFIED_VERSION` 升到 `star-v16`（旧 RD seed 键下的“未变换”快照全部失效）。
- `modSignature` 从 5 段扩为 7 段：`speedRate|odFlag|cvtFlag|classic|seed|engine|mirror`。
- 帧契约 v11 → v12：stable/ppysb 新增可选 `play.seed` / `resultsScreen.seed`；lazer 仍使用 `mods.array[].settings.seed`。
- stable/ppysb 偏移表新增 `topology.score_seed = 0x80`；`tools/stable-offsets-gen` 同步生成该字段。

## 3. 回滚

- 代码回滚到本提交前即可；偏移表可回退到旧 `stable__x86.json` / `ppysb__x86.json`（`score_seed` 是可选段，旧表缺段时稳定退化为“无 RD seed”）。
- 结果缓存无需手工清理：`star-v16` 前缀与旧版本隔离。
