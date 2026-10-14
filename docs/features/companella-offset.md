# Companella 系统性偏移校正（Companella Offset）功能文档

> 目标：AI（LLM）。本文档描述 4K Companella 路线的系统性偏移后修正的机制、参数、接入点与验证方法。
> 功能对象：Mixed 估算器在 4K 图上采纳 Companella 数值（或其与 Azusa 的 0.5/0.5 融合输出）后的最终 `numericDifficulty`。
> 设计约束（用户决策）：Daniel / Companella 为外部移植算法，禁止直接修改；如需修正，必须在**输出结果之后**进行——本修正即该约束下的"输出后修正"实现，与 [ln-correction.md](ln-correction.md) 同层级、同应用点、同顺序链（见 §4）。

## 1. 功能说明

在 Mixed 合并 Companella 结果之后，对 4K 且**采纳了 Companella 分量**的输出，在 **RC 侧**施加一个常数上调偏移 **+0.443**，并按该数值中 **Companella 的权重**缩放（纯 Companella 数值 ×1，0.5/0.5 Azusa⊕Companella 融合值 ×0.5），最后由修正值重派生 RC 半段标签（numeric 与 estDiff 保持同源硬约束）。修正恒定应用（无设置开关）。

这不是"某张图偏了"，而是 Companella 该移植算法与真实游玩分布之间的**整体尺度差**。

**作用域**（三条采纳路径，与 [mixed-routing.md](mixed-routing.md) §5 的 C6/C8/C9 对应）：

| 路径 | 数值构成 | Companella 权重 | 实际偏移 |
|---|---|---|---|
| C6 / C9（纯 Companella） | Companella 数值 | 1 | +0.443 |
| C8（低难 0.5/0.5 融合） | `0.5·Azusa + 0.5·Companella` | 0.5 | +0.2215 |
| LN 侧（任何路径） | — | — | **0（不施加）**，见 §2.3 |

> **层级依赖：已调研，未采用。** 该尺度差按层级并不恒定（低段约 +0.19、中高段约 +0.80），在行为数据集上层级版把严格分组 CV 的 MAE 改善从 −5.22% 提到 **−10.20%**；但 benchmark 侧给出反向证据。两个裁判方向相反、量级相当，故**按本项目"不发布未解决冲突"的一贯原则，仍发布常数版**。完整数据见 §2.4。

## 2. 依据

### 2.1 RC 侧：有效样本，常数偏移成立

在 Mania Tracker 行为数据集（4K，`side == "RC"` 且 `algo == "Companella"`，in-table 且 ≥8 玩家）上，`delta = play-implied − 判定` 的均值即 Companella 分量的残余低估：

| 项 | 值 |
|---|---|
| 样本 | n = 5621 chart×rate |
| 基线 bias | **+0.443** |
| 基线 MAE | 0.9107 |

**量纲一致性（本偏移成立的前提）**：这批行上 `leo_numeric`（数据集的输出侧数值）与 `companella_numeric`（Companella 原始数值）**逐位相等**（n=5621，100% 一致，差均值 0.0000）。因此数据集的 `delta` 与插件输出的数值处在**同一量纲**上，+0.443 可以直接作为该数值的修正量。

### 2.2 融合路径：为什么必须按权重缩放（0.5，而非全额）

低难 4K 的 RC 数值是 `0.5·Azusa + 0.5·Companella`（`RC_AZUSA_COMPANELLA_FUSION_WEIGHT`，见 [mixed-routing.md](mixed-routing.md) C8）。偏移刻画的是 **Companella 分量**的尺度差，而融合值里 Companella 只占一半——全额施加等于把一个只占一半的来源的尺度差整体计入结果。

数据侧同样否定全额：RC 侧 Mix 带（`0.15 ≤ lnRatio < 0.375`，该带内的 Azusa 行必然携带融合计划）n = 1645，

| 方案 | bias | MAE |
|---|---|---|
| 不修正 | +0.226 | 0.5946 |
| **常数 ×0.5（本设计）** | **+0.004** | 0.5996 |
| 层级 ×0.5（未采用，见 §2.4） | −0.018 | 0.5709 |
| 全额（×1） | −0.217 | 0.6700 |

> 注意 MAE 在这里几乎不动（0.5946 → 0.5996，反而略升）：组内 sd 约 1.07，远大于待检偏置 0.23，**MAE 对均值漂移不敏感**。判据应是带符号 bias 的消除（0.226 → 0.004）。该陷阱见 [../learnings/difficulty-estimation.md](../learnings/difficulty-estimation.md) §5.1。

### 2.3 LN 侧：不施加（原 +0.786 的推导是跨量纲误算）

初版实现曾对 LN 侧施加 **+0.786**，其来源为 `play-implied − companella_numeric` 在 LN 侧（n=2657）的均值 **+0.786**。该数字**不是有效的偏移量**：

- 行为数据集对 LN 侧谱面取用的判定量是 **LN 半段**（`verdict_used == "LN half"`，全部 6054 条 LN 侧行；且这些行 `leo_numeric == leo_continuous`，即"Sunny 星数过 LN 表"的值），
- 而 Companella 的数值是 **RC 半段**（插件的 `numericDifficulty` 恒取 RC 半段）。

**两者不同量纲**，因此 `play-implied − companella_numeric` 是跨量纲之差，不能当作 Companella 的偏移。同一人群上，同量纲的残余是 `delta` = **+0.191**——而它描述的是 **LN 半段（Sunny LN 表）**的残余，与 Companella 无关。

LN 半段自身的表偏差后来由 Polaris 的 4K LN 曲线承担，且 Mixed 在 4K 上也接入了同一份校准（见 [polaris_algorithm.md](../polaris_algorithm.md) §1）。LN 侧现有修正仍由 `lnCorrection.js` 承担。

> 该量纲陷阱是 [learnings](../learnings/difficulty-estimation.md) 中"base-identity 混淆"的同类：**在按算法分层拟合时必须先确认被比较的两个量是同一个量**，否则一切 CV 都只是自洽。

### 2.4 层级依赖：已验证但未采用（两个裁判冲突）

**行为数据集的证据（支持层级版）**——按 level 分箱的平均残余并不恒定：低段（level < 2.5）只有 **+0.19**，中高段（level ≥ 3.5）升到 **+0.80**，差一个量级。严格分组 CV：

| 方案 | 纯路径 CV MAE（n=5621） | 融合路径（n=1645） |
|---|---|---|
| 不修正 | 0.9107 | bias +0.226 / MAE 0.5946 |
| **常数 +0.443（已发布）** | 0.8631（−5.22%） | bias +0.004 / MAE 0.5996 |
| 层级（两段 + 斜坡，未采用） | **0.8166（−10.34%）** | bias −0.018 / MAE **0.5709** |

层级版的切点扫描为 2.5 / 3.0 / 3.5 → 0.8144 / 0.8166 / 0.8383（2.5–3.0 是平台）；因硬阶梯会在 level ≈ 3 处造出约 0.61 的输出跳变（同一张图换个小数位跳半级），改用 1.0 宽线性过渡，CV 代价 0.8166 → 0.8178（噪声级）。

**benchmark 的反向证据（否定层级版）**——同一改动在官方 benchmark 上回退：

| 指标 | 常数版 | 层级版 |
|---|---|---|
| 全量 776 行 MAE | **0.3895** | 0.4008 |
| 全量 − ln course（713 行）MAE | **0.3216** | 0.3341（+3.89%） |
| tech / jack / stamina / course 的 \|bias\| | 0.045 / 0.065 / 0.068 / 0.569 | 0.069 / 0.085 / 0.089 / 0.635（四者皆变差） |
| speed 的 \|bias\| | 0.030 | 0.005（改善） |

受影响的是 127 行非 LN 样本，其修正量被压低到平均 +0.13（常数版 +0.22）；由此可知这些行的 Companella 层级落在 2.5–3.5 的斜坡带，即**冲突集中在低段**——行为数据集说低段该少修，benchmark 说该多修。

**结论与后续**：两个裁判方向相反、量级相当（行为数据集 MAE 改善 ~0.03–0.05，benchmark 退化 ~0.0125），故不发布。要解决需先解释低段分歧——可行路径：(a) 在 level 匹配下比较两个来源的样本构成（benchmark 是精选谱面，行为数据集是全网分布）；(b) 用 benchmark 的低段行单独检验斜坡形状；(c) 把该段拆成更多层级结点后分别验证。在分歧解释清楚之前，常数是更低容量、两个裁判都不反对的选择。

## 3. 参数与公式

| 参数 | 值 | 说明 |
|---|---|---|
| `COMPANELLA_OFFSET_RC` | 0.443 | RC 侧常数偏移（numeric 单位，正值上调，作用在"纯 Companella 分量"上） |
| （LN 侧） | 0 | 不施加，理由见 §2.3 |
| `companellaWeight` | 1 / 0.5 | 纯 Companella / 0.5/0.5 融合 |

```
offset(side, w) = (side == "RC" ? 0.443 : 0) × w      # w = 数值中 Companella 的权重
numeric'        = round2(numeric + offset(side, w))
```

**为什么不做 rate 依赖**：按 rate 分档时 RC 在 1.0x / 1.5x / 0.75x 下分别为 +0.441 / +0.404 / +0.951——1.0x 与 1.5x 几乎一致，0.75x 样本少且 HT 侧本来就不稳定，无系统性 rate 趋势。

**为什么暂不做 level 依赖**：见 §2.4——行为数据集的层级版在两倍改善的同时被 benchmark 反向否定，冲突未解决。

## 4. 应用顺序与接入点

Companella 输出后共**两级**修正，顺序固定：

1. **Companella 系统性偏移**（本模块，`companellaOffset.js`）——修正移植算法分量的尺度差；
2. **4K LN 后修正**（`lnCorrection.js`）——修正 LN 主导谱面的残余低估（lnRatio 门控）。

两级作用域**不重叠**：偏移只在 RC 侧（`lnRatio < 0.375`）生效，LN 修正只在 `lnRatio > 0.375` 生效。逐位不变性由各自的 `applied=false` 原样返回保证。

- `js/estimator/companellaOffset.js`：纯函数模块（`computeCompanellaOffset(side)` / `applyCompanellaOffset`，后者接受 `companellaWeight`），共享 DOM-free 约束。
- `js/estimator/mixedEstimator.js`：私有助手 `applyCompanellaPostCorrections({numeric, rcLabel, lnRatio, side, companellaWeight})` 把两级按序串联；在 `applyCompanellaToMixedResult` 的三个"采纳 Companella（或融合）数值"返回路径统一包裹，C6/C9 传 `companellaWeight: 1`、C8 传 `1 − RC_AZUSA_COMPANELLA_FUSION_WEIGHT`（即 0.5）。
- `side` 由 `plan.lnRatio ≥ 0.375` 直接判定（**注意不能复用 `modeTagFromLnRatio`**，其阈值为 0.15 / 0.9，与数据集分侧不同）。
- 修正语义：`numeric = round2(numeric + offset)`；RC 半段标签 = `numericToRcLabel(修正值)`；LN 半段（`plan.lnDifficulty`）不动——4K 下它已由 Polaris 的 4K LN 曲线校准（见 [mixed-routing.md](mixed-routing.md) §4）。
- 胶囊：`companellaCapsule` 不因修正改变（数值来源未变）。
- 缓存：与 LN 修正共用 `star-v10` 语义版本（v10 = 4K LN 校准接入 Mixed；见 [../pipeline/result-cache.md](../pipeline/result-cache.md) §5）。

## 5. 行为保证

- **非采纳路径零触碰**：仅包裹 4K `applyCompanellaToMixedResult` 的三条返回路径；Roxy / Azusa（无计划）/ Daniel / Sunny 等路径的数值不由本模块产生。
- **LN 侧零触碰**：`computeCompanellaOffset("LN") === 0` ⇒ `applied=false`，返回值逐字段与旧代码一致。
- **逐位不变性**：未知侧、无效输入（`null`/`undefined`/空串/布尔）、权重为 0 时均原样返回。
- **不修改移植算法**：`companellaEstimator.js` 与 Companella ONNX 资源零改动；修正只发生在合并环节。
- **RC 侧覆盖范围**：`side == "RC"` 即 `lnRatio < 0.375`，**包含** 0.15–0.375 的混合带——这与数据集的分侧口径一致（该带的拟合样本本身就在 `side == "RC"` 组内）；`modeTagFromLnRatio` 的 0.15/0.90 分档不参与本模块。

## 6. 验证

- **严格分组 CV（行为数据集 RC 侧纯 Companella 人群，按 `beatmap_id % 10` 分组 10 折；训练折拟合常数、留出折应用）**：

  | 指标 | 基线 | 修正后 |
  |---|---|---|
  | 带符号 bias | +0.443 | **+0.077** |
  | MAE | 0.9107 | **0.8611（−5.44%）** |
  | 逐折带符号均值 | — | +0.41 / +0.44 / +0.48 / +0.49 / +0.41 / +0.45 / +0.44 / +0.44 / +0.45 / +0.41（**10/10 折同号**） |

  10 折全部为正且幅度集中，说明该偏移是可复现的尺度差，而非某一子群的偶然。
- **全量人群上应用后**：纯 Companella n=5621 的 bias +0.443 → **+0.000**、MAE 0.9107 → **0.8631**；融合带 n=1645 的 bias +0.226 → **+0.004**。
- **对照（同人群）**：错误幅度 +0.786 → bias −0.343 / MAE 0.9245（比不修正更差）；符号反转 −0.443 → MAE 1.0944。
- **Benchmark（官方 `results/Mixed.csv`，776 行）**：本设计 MAE 0.3895 / bias +0.019（官方基线 0.4141 / +0.115）。ln 段 101/102 行为 LN 侧，本偏移只触及其中 1 行。
- **层级依赖的 benchmark 反证与完整对比表**：见 §2.4（本模块唯一一项"已验证但未采用"的方案）。
- **Golden 全量回归**（778 样本）：本模块不在此回归的覆盖范围内（harness 的 `mixed` 输出不经过 `applyCompanellaToMixedResult`）。
- **冒烟**：`temp/polaris/smoke/smoke.mjs` 的偏移/融合/侧判定用例全通过（47/47）。

## 7. 维护

- 调参（常数与作用域）必须三步齐全：① 离线严格分组 CV 选值（务必限定 `algo == "Companella"` 子集，**并先校验量纲一致**）；② 对两条路径（纯 / 融合×权重）**与官方 benchmark 双向确认改善**；③ golden 全量 0 diff。
- 数据集快照更新后可重估常数；同步更新 §2、参数表与 `companellaOffset.js` 的常量注释。
- **任何"分层化"提案（level / rate / 密度）都必须同时通过 benchmark**：§2.4 的层级版在行为数据集上把 CV 改善翻倍，却在 benchmark 上回退 3.89%（clean subset），因此未发布。分层越多，越容易只在单一来源上成立。
- **重估前的强制检查**：改动分侧口径前，先确认 `leo_numeric`/`leo_continuous` 在该侧对应的**判定半段**（`verdict_used`）与 Companella 数值是否同量纲——这是 §2.3 那次误算的根因。
- **不要把判据退回 MAE 总体口径**：MAE 对均值漂移不敏感（§2.2 的注），分层判断必须条件化地看 bias。
