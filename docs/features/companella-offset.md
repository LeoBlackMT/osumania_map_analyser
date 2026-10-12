# Companella 系统性偏移校正（Companella Offset）功能文档

> 目标：AI（LLM）。本文档描述 4K Companella 路线的系统性偏移后修正的机制、参数、接入点与验证方法。
> 功能对象：Mixed 估算器在 4K 图上采纳 Companella 数值（或其与 Azusa 的 0.5/0.5 融合输出）后的最终 `numericDifficulty`。
> 设计约束（用户决策）：Daniel / Companella 为外部移植算法，禁止直接修改；如需修正，必须在**输出结果之后**进行——本修正即该约束下的"输出后修正"实现，与 [ln-correction.md](ln-correction.md) 同层级、同应用点、同顺序链（见 §4）。

## 1. 功能说明

在 Mixed 合并 Companella 结果之后，对 4K 且**采纳了 Companella（或 Azusa⊕Companella 融合）数值**的输出，按谱面侧（RC / LN）施加一个常数上调偏移（RC +0.443、LN +0.786），并由修正值重派生 RC 半段标签（numeric 与 estDiff 保持同源硬约束）。修正恒定应用（无设置开关）。

这不是"某张图偏了"，而是 Companella 该移植算法与真实游玩分布之间的**整体尺度差**：在受控同一难度层级下，Companella 的输出平均低于玩家实际表现。

## 2. 依据

| 来源 | 口径 | 结论 |
|---|---|---|
| Mania Tracker 行为数据集（4K，`algo == "Companella"`，in-table 且 ≥8 玩家，n=8278 chart×rate） | `delta = play-implied − Companella 数值` | RC **+0.443**（n=5621）、LN **+0.786**（n=2657） |
| 同上，Mixed 路由为 Companella 的 overall `delta` | `delta = play-implied − 判定` | **+0.44**（与 `bias_tables_4k.md` "By Mixed route" 一致） |
| Benchmark（`ln` pattern，102 行，全部为 Companella 路由） | `delta = expected − got` | 原始 Companella bias +1.477 / MAE 1.607 |

关键点：必须**限定在 `algo == "Companella"` 的子集**上拟合。若把 `algo ∈ {Azusa, Companella}` 池化，会引入 base-identity 混淆——两侧的 `play-implied − companella_numeric` 因基座不同而系统性相差约 0.9 dan，与结构性无关（该陷阱在 [../learnings/difficulty-estimation.md](../learnings/difficulty-estimation.md) 有完整记录）。

## 3. 参数与公式

| 参数 | 值 | 说明 |
|---|---|---|
| `COMPANELLA_OFFSET_RC` | 0.443 | RC 侧常数偏移（numeric 单位，正值为上调） |
| `COMPANELLA_OFFSET_LN` | 0.786 | LN 侧常数偏移 |

```
offset(side) = RC ? 0.443 : LN ? 0.786 : 0     # side 判定：lnRatio ≥ 0.375 记 LN，否则 RC（与数据集/Polaris 同语义）
numeric'     = round2(numeric + offset(side))
```

**为什么是常数、不做 level / rate 依赖**：

- 按 rate 分档：RC 在 1.0x / 1.5x / 0.75x 下分别为 +0.441 / +0.404 / +0.951（1.0x 与 1.5x 几乎一致；0.75x 样本少且 HT 侧本来就不稳定）——无系统性 rate 趋势。
- 按 level 分档：非单调且噪声大（lvl≤4 +0.337、lvl≤6 +1.078、lvl≤10 +0.316…）。
- 严格分组 CV 下，**细化分箱没有额外收益**（level-binned 与 finer-binned 同为 +12.06%）——证明显式 level 依赖是过拟合，常数是正确的最低容量选择。

## 4. 应用顺序与接入点

Companella 输出后共**两级**修正，顺序固定：

1. **Companella 系统性偏移**（本模块，`companellaOffset.js`）——修正移植算法的整体尺度差；
2. **4K LN 后修正**（`lnCorrection.js`）——修正 LN 主导谱面的残余低估（lnRatio 门控）。

两级都只作用于 Companella 侧数值，且**互补**（行为数据集上：仅偏移 +9.21%；叠加现有 LN 修正后 +4.84%，两者不冲突；见 §6）。

- `js/estimator/companellaOffset.js`：纯函数模块（`computeCompanellaOffset` / `applyCompanellaOffset`），共享 DOM-free 约束。
- `js/estimator/mixedEstimator.js`：新增私有助手 `applyCompanellaPostCorrections({numeric, rcLabel, lnRatio, side})`，把两级按序串联；在 `applyCompanellaToMixedResult` 的三个"采纳 Companella（或融合）数值"返回路径统一包裹。`side` 由 `plan.lnRatio ≥ 0.375` 直接判定（**注意不能复用 `modeTagFromLnRatio`**，其阈值为 0.15 / 0.9，与数据集分侧不同）。
- 修正语义：`numeric = round2(numeric + offset)`；RC 半段标签 = `numericToRcLabel(修正值)`；LN 半段（`plan.lnDifficulty`）不动。
- 胶囊：`companellaCapsule` 不因修正改变（数值来源未变）。
- 缓存：与 LN 修正共用 `star-v9` 语义版本（同一次改动引入）。

## 5. 行为保证

- **非 Companella 路径零触碰**：仅包裹 4K `applyCompanellaToMixedResult` 的三条返回路径；Azusa / Daniel / Roxy / Sunny 等路径的数值不由本模块产生。
- **逐位不变性**：`applied=false`（未知侧、无效输入）时返回值与旧代码逐字段一致。
- **不修改移植算法**：`companellaEstimator.js` 与 Companella ONNX 资源零改动；修正只发生在合并环节。
- `Mix` 侧（`modeTagFromLnRatio` 返回 "Mix"，即 0.15 < lnRatio < 0.375 的 RC 侧混合图）**不施加偏移**（保守；数据集未在该细分上单独拟合）。

## 6. 验证

- **严格分组 CV（行为数据集，按 `beatmap_id` 分组 10 折，训练集拟合常数、测试集应用）**：
  - 无其他修正的 Companella-only 基线：MAE 0.9708 → 0.8813（**+9.21%**）；
  - 叠加现有 4K LN 修正后：MAE 0.9211 → 0.8765（**+4.84%**）。
- **Benchmark（102 ln 行，本地 runner 复算，反推原始 Companella 后对比）**：

  | 方案 | bias | MAE | Exact |
  |---|---|---|---|
  | 原始 Companella | +1.477 | 1.607 | 13.7% |
  | 仅现有 LN 修正（+0.5 ramp） | +1.015 | 1.228 | 25.5% |
  | 仅 Companella 偏移（+0.786） | +0.691 | 1.069 | 32.4% |
  | **偏移 + LN 修正（本次）** | **+0.229** | **0.896** | **38.2%** |

- **Golden 全量回归**（748 样本）：**0 diffs**（harness 的 `mixed` 输出不经过 `applyCompanellaToMixedResult`，天然不受影响；作为回归门执行）。非 Companella 行不变性同时由 CV 与 golden 双确认。

## 7. 维护

- 调参（两常数）必须三步齐全：① 离线严格分组 CV 选值（务必限定 `algo == "Companella"` 子集）；② 对 benchmark 与行为数据集双口径确认改善；③ golden 全量 0 diff。
- 数据集快照更新后可重估两常数；同步更新 §2、参数表与 `companellaOffset.js` 的常量注释。
