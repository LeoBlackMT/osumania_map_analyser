# Companella 系统性偏移校正（Companella Offset）功能文档

> 目标：AI（LLM）。本文档描述 4K Companella 路线的系统性偏移后修正的机制、参数、接入点与验证方法。
> 功能对象：Mixed 估算器在 4K 图上采纳 Companella 数值（或其与 Azusa 的 0.5/0.5 融合输出）后的最终 `numericDifficulty`。
> 设计约束（用户决策）：Daniel / Companella 为外部移植算法，禁止直接修改；如需修正，必须在**输出结果之后**进行——本修正即该约束下的"输出后修正"实现。
> **实装范围（实机测试后收敛）：只保留低段。** `level < 2.5 → +0.19`，其余一律 0；曾与之并列的 4K LN 后修正（`lnCorrection.js`）已整体移除（见 §2.5）。

## 1. 功能说明

在 Mixed 合并 Companella 结果之后，对 4K 且**采纳了 Companella 分量**的输出，在 **RC 侧**施加一个**仅低段**的上调偏移：`level < 2.5 → +0.19`，其余层级一律 **0**。偏移按该数值中 **Companella 的权重**缩放（纯 Companella 数值 ×1，0.5/0.5 Azusa⊕Companella 融合值 ×0.5），最后由修正值重派生 RC 半段标签（numeric 与 estDiff 保持同源硬约束）。修正恒定应用（无设置开关）。

这不是"某张图偏了"，而是 Companella 该移植算法与真实游玩分布之间的**整体尺度差**；该尺度差**按层级并不恒定**——低段与中高段差一个量级（§2.4），但实装只取低段（§2.5 说明为什么）。

**作用域**（三条采纳路径，与 [mixed-routing.md](mixed-routing.md) §5 的 C6/C8/C9 对应）：

| 路径 | 数值构成 | Companella 权重 | 实际偏移 |
|---|---|---|---|
| C6 / C9（纯 Companella） | Companella 数值 | 1 | level < 2.5 → **+0.19**；其余 0 |
| C8（低难 0.5/0.5 融合） | `0.5·Azusa + 0.5·Companella` | 0.5 | level < 2.5 → **+0.095**；其余 0 |
| LN 侧（任何路径） | — | — | **0（不施加）**，见 §2.3 |

层级查找键恒取 **Companella 自身输出的数值**（分段值就是在"纯 Companella 行"上按该值拟合的），与 numeric 是否已被融合无关。

## 2. 依据

### 2.1 RC 侧：有效样本，偏移成立

在 Mania Tracker 行为数据集（4K，`side == "RC"` 且 `algo == "Companella"`，in-table 且 ≥8 玩家）上，`delta = play-implied − 判定` 的均值即 Companella 分量的残余低估：

| 项 | 值 |
|---|---|
| 样本 | n = 5621 chart×rate |
| 基线 bias | **+0.443** |
| 基线 MAE | 0.9107 |

**量纲一致性（本偏移成立的前提）**：这批行上 `leo_numeric`（数据集的输出侧数值）与 `companella_numeric`（Companella 原始数值）**逐位相等**（n=5621，100% 一致，差均值 0.0000）。因此数据集的 `delta` 与插件输出的数值处在**同一量纲**上，+0.443 可以直接作为该数值的修正量。

### 2.2 融合路径：为什么必须按权重缩放（0.5，而非全额）

低难 4K 的 RC 数值是 `0.5·Azusa + 0.5·Companella`（`RC_AZUSA_COMPANELLA_FUSION_WEIGHT`，见 [mixed-routing.md](mixed-routing.md) C8）。偏移刻画的是 **Companella 分量**的尺度差，而融合值里 Companella 只占一半——全额施加等于把一个只占一半的来源的尺度差整体计入结果。

数据侧同样否定全额：RC 侧 Mix 带（`0.15 ≤ lnRatio < 0.375`，该带内的 Azusa 行必然携带融合计划）n = 1645，不修正 bias +0.226，全额 +0.443 ⇒ bias −0.217（过修正 0.44）。

### 2.3 LN 侧：不施加（原 +0.786 的推导是跨量纲误算）

初版实现曾对 LN 侧施加 **+0.786**，其来源为 `play-implied − companella_numeric` 在 LN 侧（n=2657）的均值 **+0.786**。该数字**不是有效的偏移量**：

- 行为数据集对 LN 侧谱面取用的判定量是 **LN 半段**（`verdict_used == "LN half"`，全部 6054 条 LN 侧行；且这些行 `leo_numeric == leo_continuous`，即"Sunny 星数过 LN 表"的值），
- 而 Companella 的数值是 **RC 半段**（插件的 `numericDifficulty` 恒取 RC 半段）。

**两者不同量纲**，因此 `play-implied − companella_numeric` 是跨量纲之差，不能当作 Companella 的偏移。同一人群上，同量纲的残余是 `delta` = **+0.191**——而它描述的是 **LN 半段（Sunny LN 表）**的残余，与 Companella 无关。

> 该量纲陷阱是 [learnings](../learnings/difficulty-estimation.md) 中"base-identity 混淆"的同类：**在按算法分层拟合时必须先确认被比较的两个量是同一个量**，否则一切 CV 都只是自洽。

### 2.4 层级结构：证据、历史实装与实机否决

**行为数据集按层级分箱的残余**（纯路径 n=5621；数据集最低层级就是 2.0，不存在更低的行）：

| 层级带 | n | bias |
|---|---|---|
| [2.0, 2.5) | 2518 | **+0.054** |
| [2.5, 3.0) | 784 | +0.626 |
| [3.0, 3.5) | 224 | +1.493 |
| [3.5, 4.0) | 56 | +0.935 |
| [4.0, 6.0) | 733 | +0.886 |
| [6.0, 8.0) | 351 | +0.745 |
| [8.0, 10.0) | 330 | +0.596 |
| [10.0, ∞) | 625 | +0.591 |

**历史实装（已移除）**：层级完整版曾按"两段 + 1.0 宽斜坡"实装，`LOW=+0.19`、`HIGH=+0.804`。**这两个值的拟合口径是 level < 3.0 与 level ≥ 3.0 的均值**（斜坡 2.5→3.5 中点两侧的分组），而**不是**代码注释与文档当时所写的"level < 2.5 / ≥ 3.5"——`clamp((level−2.5)/1.0, 0, 1)` 在 level ≤ 2.5 处取 LOW、level ≥ 3.5 处取 HIGH，与拟合分组错位。这是本文档本次重写时核对数据才发现的：**分段值与分段边界来自同一次拟合，但二者被记成了不同的段**。

该层级完整版的两个外部评价都反对它：

| 裁判 | 结果 |
|---|---|
| **benchmark**（776 行） | 全量 MAE 0.3895 → 0.4008；受影响 127 行非 LN 样本里 76 行变差，分歧集中在高段（level ≥ 3.5 的 100 行贡献 +9.36 的累计误差增量），且这些谱面 `got > expected` 一致成立——任何上调都更差 |
| **实机测试**（项目所有者） | **把实机难度过分拉高**——这是最终否决它的直接原因 |

### 2.5 实装范围：为什么只剩低段 +0.19，以及它的一个已知偏差

层级完整版被实机测试否决后，实装收敛为**只保留低段**：`level < 2.5 → +0.19`，斜坡与高段全部移除。选择保留低段的理由：它是层级结构里**最小的修正**，也是 benchmark 上唯一一段两个裁判同向的（Δgot −0.16…−0.12 的 15 行里 benchmark 也变好）。

**一个需要如实记录的已知偏差**：`+0.19` 这个数值来自 PR 描述里的"Low band (<2.5) receives +0.19"，但按 §2.4 的分箱，**level < 2.5 段的真实残余是 +0.054**——`+0.19` 实际上是 **level < 3.0** 的均值（历史拟合的分组口径，见 §2.4）。因此对低段施加 +0.19 会把该段从 +0.054 推到 **−0.136**（过修正 0.136）：

| 人群 | 基线 bias | 低段Only 实装后 |
|---|---|---|
| 纯路径全部（n=5621） | +0.443 | +0.358 |
| 纯路径低段（n=2518） | **+0.054** | **−0.136** |
| 纯路径高段（n=3103） | +0.759 | +0.759（不修正） |
| 融合带全部（n=1645） | +0.226 | +0.186 |

即实装版本**有意接受低段的过修正**：项目所有者在实机测试后选择只保留这一段最小修正（数值沿用 PR 描述的 +0.19），换取消除高段的大幅拉高。若日后要收敛低段数值，**+0.054（该段真实残余）是下一个候选**——但这必须连同实机复测一起做，不要只按数据侧数字改。

**同轮移除的 4K LN 后修正**（`lnCorrection.js`，+0.5、lnRatio 0.375→0.55 渐进门控）：依据与偏移同源（benchmark ln 行 bias +1.02、行为数据 4K LN 侧 +0.2~0.5），与偏移同处 Companella 输出之后。实机测试同样否决它——与高段一起把难度拉得过高——故整体移除，模块与接线一并删除。其证据与量纲结论（LN 侧 +0.786 是跨量纲误算）仍有效，记录于 [../learnings/difficulty-estimation.md](../learnings/difficulty-estimation.md) §5.2。

## 3. 参数与公式

| 参数 | 值 | 说明 |
|---|---|---|
| `COMPANELLA_OFFSET_RC_LOW` | 0.19 | 低段 RC 侧偏移（见 §2.5 的已知偏差） |
| `COMPANELLA_OFFSET_RC_LOW_BAND_MAX` | 2.5 | 低段上限 |
| （LN 侧） | 0 | 不施加，理由见 §2.3 |
| `companellaWeight` | 1 / 0.5 | 纯 Companella / 0.5/0.5 融合 |

```
offset   = (side == "RC" && level < 2.5 ? 0.19 : 0) × w    # level = Companella 自身输出的数值
numeric' = round2(numeric + offset)
```

**为什么不做 rate 依赖**：按 rate 分档时 RC 在 1.0x / 1.5x / 0.75x 下分别为 +0.441 / +0.404 / +0.951——1.0x 与 1.5x 几乎一致，0.75x 样本少且 HT 侧本来就不稳定，无系统性 rate 趋势。

**为什么不再做层级**：见 §2.4 / §2.5——层级完整版被 benchmark 与实机双双否决；现装的低段Only 是实机测试后的收敛结果。

## 4. 接入点

- `js/estimator/companellaOffset.js`：纯函数模块（`computeCompanellaOffset(side, level)` / `applyCompanellaOffset`，后者接受 `level` 与 `companellaWeight`），共享 DOM-free 约束。
- `js/estimator/mixedEstimator.js`：私有助手 `applyCompanellaPostCorrection({numeric, rcLabel, side, companellaLevel, companellaWeight})`；在 `applyCompanellaToMixedResult` 的三个"采纳 Companella（或融合）数值"返回路径统一调用，C6/C9 传 `companellaWeight: 1`、C8 传 `1 − RC_AZUSA_COMPANELLA_FUSION_WEIGHT`（即 0.5）。`companellaLevel` 三处都取 `companellaResult.numericDifficulty`（纯 Companella 数值本身）。
- `side` 由 `plan.lnRatio ≥ 0.375` 直接判定（**注意不能复用 `modeTagFromLnRatio`**，其阈值为 0.15 / 0.9，与数据集分侧不同）。
- 修正语义：`numeric = round2(numeric + offset)`；RC 半段标签 = `numericToRcLabel(修正值)`；LN 半段（`plan.lnDifficulty`）不动——4K 下它已由 Polaris 的 4K LN 曲线校准（见 [mixed-routing.md](mixed-routing.md) §4）。
- 胶囊：`companellaCapsule` 不因修正改变（数值来源未变）。
- 缓存：`star-v11` 语义版本（v11 = 后修正收敛为低段Only；见 [../pipeline/result-cache.md](../pipeline/result-cache.md) §5）。

## 5. 行为保证

- **非采纳路径零触碰**：仅包裹 4K `applyCompanellaToMixedResult` 的三条返回路径；Roxy / Azusa（无计划）/ Daniel / Sunny 等路径的数值不由本模块产生。
- **低段之外零触碰**：`level ≥ 2.5` 的任何谱面（含全部中高段）偏移为 0，`applied=false` 原样返回——这是与层级完整版最大的行为差异。
- **LN 侧零触碰**：`computeCompanellaOffset("LN", level) === 0`。
- **逐位不变性**：未知侧、无效输入（`null`/`undefined`/空串/布尔）、权重为 0、`level` 非有限时均原样返回。
- **不修改移植算法**：`companellaEstimator.js` 与 Companella ONNX 资源零改动；修正只发生在合并环节。
- **RC 侧覆盖范围**：`side == "RC"` 即 `lnRatio < 0.375`，**包含** 0.15–0.375 的混合带——这与数据集的分侧口径一致；`modeTagFromLnRatio` 的 0.15/0.90 分档不参与本模块。

## 6. 验证

- **实装版本的 tracker 度量**（见 §2.5 表）：纯路径全量 bias +0.443 → +0.358；低段子群 +0.054 → **−0.136**（有意接受的过修正）；高段子群不动。**MAE 在这里不是判据**（组内 sd ≈ 1.07 远大于待检偏置），判据是带符号 bias。
- **Benchmark**：低段Only 版未单独跑分；历史对照——常数版 0.3895、层级版 0.4008（回退）。低段在 benchmark 上的 15 行（Δgot −0.16…−0.12）两个裁判同向。
- **Golden 全量回归**（778 样本）：本模块不在此回归的覆盖范围内（harness 的 `mixed` 输出不经过 `applyCompanellaToMixedResult`）。
- **冒烟**：合成谱冒烟覆盖低段/低段边界（2.4999 → +0.19、2.5 → 0）、高段为 0、权重缩放、侧判定与缺 `level` 守卫。工装为本地一次性脚本（不入库），用例清单与期望值已在本文与 `mixed-routing.md` 中逐条写明，可据此重建。

## 7. 维护

- **实机测试现在是必要门槛**：本轮的直接教训——层级完整版在行为数据集与 benchmark 之外的第三个裁判（实机手感）上被否决。**任何偏移/修正类改动的验收都必须包含实机复测**，三个裁判缺一不可。
- 调参必须三步齐全：① 离线严格分组 CV 选值（务必限定 `algo == "Companella"` 子集，**并先校验量纲一致**）；② benchmark 双向确认；③ 实机复测。
- **分段值与分段边界必须同源记录**：§2.4 的教训——历史实装把"level<3.0 的均值"记成了"level<2.5 段的值"。任何分段拟合都要写清**每一段值是在哪个区间上算的**。
- **低段数值的收敛候选**：§2.5 记录了 +0.19 对低段过修正 0.136；若实机复测仍嫌低段偏高，下一档是 **+0.054**（该段真实残余）。改动前先跑 §2.5 的表并复测。
- **重估前的强制检查**：改动分侧口径前，先确认 `leo_numeric`/`leo_continuous` 在该侧对应的**判定半段**（`verdict_used`）与 Companella 数值是否同量纲——这是 §2.3 那次误算的根因。
- **不要把判据退回 MAE 总体口径**：MAE 对均值漂移不敏感，分层判断必须条件化地看 bias。
