# 4K LN 数值后修正（LN Ratio Correction）功能文档

> 目标：AI（LLM）。本文档描述 4K LN 主导谱面数值后修正的机制、参数、接入点与验证方法。
> 功能对象：Mixed 估算器在 4K LN 主导图上的最终 `numericDifficulty`（来源为 Companella，或其与 Azusa 的 0.5/0.5 融合输出）。
> 设计约束（用户决策）：Daniel / Companella 为外部移植算法，禁止直接修改；如需修正，必须在**输出结果之后**进行——本修正即该约束下的"输出后修正"实现（与 [marathon-correction.md](marathon-correction.md) 的"估算器内嵌"路线形成对照：那条路线作用于本项目自有估算器，本条作用于移植算法产物）。

## 1. 功能说明

对 **4K 且 Sunny `lnRatio` 主导**（渐进门控，见 §3）的谱面，在 Mixed 合并 Companella 结果之后，对最终 numeric 做 +0.5 以内的上调修正，并由修正值重派生 RC 半段标签（numeric 与 estDiff 保持同源硬约束）。修正恒定应用（无设置开关），仅在"采纳/融合了 Companella 数值"的路径上生效。

## 2. 依据与幅度选择

| 来源 | 口径 | 结论 |
|---|---|---|
| Benchmark（`ln` pattern，102 行） | `delta = expected − got`（正 = 低估） | Mixed 输出 bias **+1.015**、MAE **1.228**（benchmark 全系统最弱环节） |
| Mania Tracker 行为数据集（4K LN 侧） | `delta = play-implied − 判定` | +0.2 ~ +0.5（按 lnRatio 分档） |

幅度取 **0.5**（两源保守折中）：

- 纯 benchmark 口径下 MAE 随幅度单调下降（0.6 时 0.932、0.8 时 0.894），但该段仅 102 行、来源单一，按 learnings「不为小样本段做针对性修复」的判据不采用最优值；
- 0.5 使 benchmark ln 行 MAE 1.228 → 0.958（−22%）、bias +1.015 → +0.564（约减半），同时相对行为数据侧（+0.2~0.5）的过冲不超过 ~0.3。

## 3. 参数与公式

| 参数 | 值 | 说明 |
|---|---|---|
| `LN_CORRECTION_MAGNITUDE` | 0.5 | 全量修正幅度（numeric 单位） |
| `LN_CORRECTION_RAMP_LO` | 0.375 | 门控下界（≤ 不触发；与数据集 RC/LN 侧切分同值） |
| `LN_CORRECTION_RAMP_HI` | 0.55 | 全量触发点（之上恒为全量） |

```
corr = 0.5 × clamp((lnRatio − 0.375) / (0.55 − 0.375), 0, 1)   # 且 lnRatio ∈ (0.375, 1.5]
```

上界 1.5 用于排除 `forceSunnyWindow` 覆盖 LN 半段时写入的 `4e65` 哨兵（`analysis.js` 的 `pendingMixedCompanellaContext.lnRatio = 4e65`）——该路径不触发修正。

## 4. 接入点

- `js/estimator/lnCorrection.js`：纯函数模块（`computeLnRatioCorrection` / `applyLnCorrectionToRcFields`），共享 DOM-free 约束。
- `js/estimator/mixedEstimator.js applyCompanellaToMixedResult`：三个"采纳 Companella（或融合）数值"的返回路径（C6 / C8 / C9，见 [mixed-routing.md](mixed-routing.md) §5）统一包裹。未触发时 `applied=false` **原样返回**（非 LN 行与非 4K 流程逐位不变）。
- **与 Companella 系统性偏移的顺序（固定）**：本修正不是该合并点上的唯一后处理。包装函数 `applyCompanellaPostCorrections` 先应用 [companella-offset.md](companella-offset.md) 的 Companella 偏移（**仅 RC 侧** +0.443，并按 Companella 权重缩放），**再**应用本 LN 修正。顺序不可交换：偏移把 Companella 分量拉回自身尺度（算法级、来源侧的尺度差），本修正再叠加谱面侧的 LN 残余低估（谱面级）。两级**作用域不重叠**：偏移只在 `lnRatio < 0.375`（RC 侧）生效，本修正只在 `lnRatio > 0.375` 生效——因此对任一具体谱面，实际只有一级在动。
- 修正语义：`numeric = round2(numeric + corr)`；RC 半段标签 = `numericToRcLabel(修正值)`；LN 半段（`plan.lnDifficulty`，来自 Sunny 的 4K LN 表）不动。
- 胶囊：`companellaCapsule` 不因修正改变（数值来源未变）。
- 缓存：语义变化由 `star-v8 → star-v9` 覆盖（见 [../pipeline/result-cache.md](../pipeline/result-cache.md) §5）。
- 基准 runner：`benchmark-runner.mjs runMixedFlow → applyCompanellaToMixedResult` 走同一函数，benchmark 结果自动反映修正（"插件等价调用"口径已覆盖）。

## 5. 行为保证

- **非 4K / 非 LN 零触碰**：计划（`mixedCompanellaPlan`）只在 4K 流程生成；lnRatio 门控再过滤 LN 占比低的图。
- **逐位不变性**：`applied=false` 时返回值与旧代码逐字段一致（含 estDiff 组合的原有求值路径）。
- **哨兵保护**：`4e65` 不触发（§3）。
- **不修改移植算法**：`companellaEstimator.js`、Daniel 相关文件零改动；修正只发生在 `applyCompanellaToMixedResult`（输出之后的合并环节）。

## 6. 验证

- 离线扫描（benchmark 官方 `results/Mixed.csv` 的 ln 行，幅度 0→0.8 的 MAE/bias/Exact 曲线）：见 §2。
- Harness before/after（本地 runner 副本，不改基准仓库；before 与官方 `results/` 逐位一致，实测 MAE 0.414 / ln MAE 1.228 完全复现）：
  - before：ln 行 n=102，MAE **1.228** / bias **+1.015** / Exact 9.8% / Close 26.5%；
  - after：ln 行 n=102，MAE **0.958** / bias **+0.564** / Exact 17.6% / Close 39.2%（102 行中 99 行数值变化，3 行为零/近零修正）；
  - 不变性：非 ln 673 行（含 4K RC 全部）**逐位一致**；全量 MAE 0.414 → 0.379。
- Golden 全量回归（748 样本）：0 diffs（harness 的 `mixed` 输出不经过 `applyCompanellaToMixedResult`，天然不受影响；作为回归门执行）。

## 7. 维护

- 调参（幅度 / 门控阈值）必须三步齐全：① 离线扫描选幅度；② harness before/after 确认 ln 行改善且**非 ln 行逐位一致**；③ golden 全量 0 diff。
- 行为数据或 benchmark 标注更新后可重估幅度；同步更新 §2 与参数表、以及 `lnCorrection.js` 的常量注释。
