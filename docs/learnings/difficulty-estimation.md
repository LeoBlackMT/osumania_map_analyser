# 难度估计算法调优知识与教训

> 本文档记录在难度估计算法（Roxy / Azusa / Daniel / Sunny / Mixed / Companella）开发与调优过程中积累的知识与教训，来源包括本仓库 `temp/` 目录的历史探针/调优日志、benchmark 验证记录与历次优化会话结论。
>
> 目标读者：后续继续改进估计算法的 LLM 与开发者。**核心价值在于记录「什么无效」及「为什么」，避免重复踩坑。**

---

## 1. 环境与约束（必须遵守的前提）

- **运行时只有谱面文件本身**：插件在真实环境中只能获取 `.osu` 谱面文本，tosu WebSocket 不提供任何键型分类（pattern/subPattern 是 benchmark 数据集的标注，**不是运行时可用信号**）。任何依赖 benchmark 标注的算法改动都是过拟合。
- **benchmark 规则**：
  - `expected` = 段位谱面的数值化难度（Reform 体系，1st=1.0 … alpha=11.0，实际标签含 0.1 步进如 14.7）。
  - 指标：Exact = `|delta| ≤ 0.2`，Close = `≤ 0.5`，Moderate = `≤ 1.0`，Miss = `> 1.0`，`delta = expected − got`（正 = 低估）。
  - **禁止读取 `samples/` 谱面数据**（防止硬编码/过拟合）；训练/验证只允许用 `results/` CSV 的 `expected` 与各算法 `got`。
- **代码约束**：只能修改 git 跟踪文件；不得修改 benchmark 仓库（验证时把 runner 复制到 `temp/` 并重定向输出，或仅用官方 runner 在获得授权后更新 `results/`，且 `index.json` 的 `source` 键会被非覆盖式保留）。
- **防过拟合判据**：group-split CV（按谱面名/家族分组）的 full→CV gap 必须小；CV 不提升就回退。

---

## 2. 核心架构速览

- **Roxy**（4K RC，高难聚焦 11~17）：7-stream 结构 strain（speed/handStream/jack/chordjack/tech/stamina/course）→ corrections（门控）→ 111 维 meta 特征（Azusa/Daniel/Sunny references + pairwise diff + 结构统计）→ ridge 线性 meta 头 → 后处理（OD 校正、high-reference 结构下限、reference-gap 残差校正、Azusa high-gap lift）→ **Azusa 融合（w=0.4）** → scope 检查（<11 → `< Alpha Low`，≥17 → `> Emik Zeta high`）。
- **Azusa**：融合 Daniel+Sunny 的调校算法，4K RC；低难段由 Mixed 路由负责。
- **Daniel**：4K Reform Alpha+；只在自身段位带内输出 native numeric（<Alpha 返回 null）。
- **Sunny**：通用星数算法（4/6/7K RC/LN）。
- **Mixed**：路由算法——4K RC 高难 → Roxy，Roxy 不可用 → Daniel/Azusa，LN/Mix → Sunny+Companella/Daniel。
- **Companella**：ONNX 模型（Etterna MinaCalc 特征），4K Reform Delta+ 及以下。

---

## 3. 历次实验结论（按主题）

### 3.1 输出量化到 0.5 网格——**已证伪并回退**（最重要的教训）

**实验**：把 Roxy 的 `numericDifficulty` round 到 0.5 网格（`ROXY_OUTPUT_GRID`），并配合「用 0.5 网格标签训练 ridge」。

**结果**：
- benchmark Exact 提升显著（Roxy 11~17：55.1% → 61.6%；Mixed 11~17：54.4% → 60.2%）。
- **但这是「benchmark 游戏」**：`numericDifficulty` 量化后，benchmark 网页从 got 小数推断的段位变体只剩 `low`（小数 0.0）和 `mid`（0.5），`mid/low`、`mid/high`、`high` 全部消失（BEFORE 有 5 种变体，AFTER 只剩 2 种）。
- **更严重**：插件实际显示的 `estDiff` 基于未量化 `finalNumeric`（`numericToRcLabel` 按最近中心点映射，中心间隔 0.2），量化 got 与 estDiff **不一致甚至跨段位**（如 final=14.60 → 插件显示 "Epsilon low"，量化 got=14.5 → benchmark 推断 "Delta high"）。
- 量化拆解：序数 ridge 重训（真实改变 finalNumeric → 改变 estDiff）贡献约 +4.6pp；纯输出量化贡献约 +1.9pp 但只改 numeric 不改 estDiff（虚假提升）。

**结论**：**输出量化是 benchmark 特化，不是真实优化**。`numericDifficulty` 必须与 `estDiff` 完全一致（都是最终后处理值），否则优化对用户无意义。**保留序数 ridge 重训（真实），放弃输出量化**。

### 3.2 序数 meta 头重训——**有效**

**实验**：ridge meta 头改用 0.5 网格量化后的 `expected` 标签训练（lambda=2.0，全分布 MEAN/SCALE）。

**结果**（无输出量化）：
- Roxy 11~17：Exact 55.1% → 59.4%，MAE 0.229 → 0.219。
- Mixed 11~17：54.4% → 58.4%，MAE 0.240 → 0.233。
- group-split CV：序数模型 CV Exact 54.4% vs 连续模型 48.8%（+4.6pp，非纯记忆）。

**要点**：
- 训练集选择：**只拟合 11~17 段会使 ≥17 边界外推偏低**（Mixed ≥17 Exact 42.1% vs 基线 52.6%）；**纳入 ≥17 段（6 张）拟合后恢复 52.6%**，11~17 仅 -0.4pp。→ 拟合集应覆盖 scope 上界。
- MEAN/SCALE 用**全分布**（所有行）计算，scope 外推理不畸变。
- lambda 细扫（0.5~8.0）CV 差异 <0.6pp——lambda 不是关键因素。
- FIT_ALL（全范围拟合）比 11~17+≥17 拟合差（11~17 Exact 58.6% vs 60.2% 的量化版对比）——低难段（<11，20 张）纳入训练会稀释高难拟合。

### 3.3 Mixed 路由规则——**两条有效、若干无效**

**有效 1：换路判定用未量化 finalNumeric**。Roxy 输出量化（已回退）会放大 Azusa−Roxy delta 干扰换路；即使无量化，用 `debug.finalNumeric`（全部后处理后的连续值）比 `numericDifficulty`（2 位小数）更精确。修复了 Matusa Bomber、Ragnarok 两张被错误换到 Azusa 的图。

**有效 2：跨界换路规则**。当 Roxy 未量化输出 ≥11（Alpha 边界）而 Azusa reference <11 时，换到 Azusa——针对「结构难但段位低」的谱面（Roxy 结构模型系统性高估，如 star of andromeda 家族）。11~17 段正常图（Azusa 参考也在 11+）不受影响（误伤率 <2/476）。

**无效**：
- **Daniel 全局优先**：Daniel 对 ≥17 段严重低估（bias +0.16），全局优先导致 ≥17 段 Mixed Exact 从 52.6% 崩到 5.3%。
- **Daniel 段感知优先（danielNumeric<11）**：不触发——因为跨界图（expected<11 但 Roxy≥11）的 Daniel 输出也多在 11~11.6，`<11` 条件不满足。
- **跨界换 Daniel**：Daniel 普遍比 Roxy 低 0.12~0.23（所有段），无法区分跨界图；且跨界图 6 张 Azusa≥11 无可靠信号。
- **Azusa 全段/11~17 量化**：量化后的 azusaNumeric 参与 Mixed 换路 delta，破坏路由（11~17 Mixed 从 60.4% 掉到 56.3%）。

### 3.4 特征工程方向——**全部无效**

- **合规分组残差校正**（用 Roxy 自算 stats 如 chordRate/anchorRate 分箱做组残差）：无效（53.0% → 52.8~53.7%，噪声内）。之前用 benchmark subPattern 分组的「+3.2pp」不可复现且**违规**（subPattern 是标注，运行时不可得）。
- **lambda 细扫**：见 3.2。
- **0.25 网格 / 分段量化（≥16.5 ceil）**：0.25 网格 Exact 略低（57.9% vs 58.5%）；分段量化 ≥17 +1 张但 11~17 -3 张，净亏损。
- **历史 15 轮调优**（2026-06）：ridge lambda grid、GBDT（500 树）、isotonic 标定、reference stacking、浅残差树、二阶门控残差 ridge、序列复杂度特征、chordjack 修正、特征组消融、加权 ridge 等——**全部因 full/CV gap 过大或 KPI5（Miss<2%、Close+>80%、Moderate<10%）不达标而拒绝**。唯一部分接受的是保守的 reference-gap 残差校正（`±0.10` 最终影响）。
- **历史 GBDT 路由探针**：500 树 GBDT、bucket 线性、isotonic、segmented 等模型对比，group-split CV 均退化——高容量模型在 ~500 样本上必然过拟合。

### 3.5 Azusa 相关的教训

- **低难 floor 量化**（<11 向下取整 0.5 网格）曾带来 Mixed <11 Close +14pp，但同样存在显示一致性问题（estDiff 基于未量化值），且对 expected 11~12 但输出 10.75~10.88 的图有害（floor 10.88→10.50，Close→Moderate）——**已随量化整体回退**。
- Azusa 对「结构难但段位低」的图（star of andromeda 等）也系统性低估——这是**标注与算法共识的分歧**（expected 11+ 但主流算法都认为 10 级），任何规则修复都是过拟合。

---

## 4. 方法论（有效的工作流）

1. **先离线探针，再改生产代码**：用 benchmark `results/` 的 got 值模拟候选改动的效果（如量化、路由规则），确认方向后再实现。注意：**离线模拟会忽略 Mixed 路由交互**（Azusa 量化离线显示 +9.3pp，真实 Mixed 反而退化——因为 azusaNumeric 参与换路 delta），最终必须真实验证。
2. **显示一致性是硬约束**：`numericDifficulty` 与 `estDiff` 必须同源。任何只改 numeric 不改 estDiff 的改动都是 benchmark 游戏。
3. **路由改动必须跑完整 Mixed benchmark**：路由规则的触发条件（delta 阈值、handBias/anchorRate）与输出量化、序数模型强耦合。
4. **小样本段（<11 的 20 张、≥17 的 6~22 张）不可靠**：Exact ±5pp 可能只是 1-2 张图；除非有明确的机制性理由（如序数模型上界外推），否则不要为小样本段做针对性修复。
5. **group-split CV 是唯一可信的过拟合判据**：full-fit 提升（如 53.9%→61%）若无 CV 支撑（39.5% vs 旧 47.5%）即过拟合。
6. **训练集边界决定外推行为**：只拟合 11~17 → 上界外推偏低；纳入 ≥17 → 修复。拟合集应覆盖目标 scope 的完整边界。

---

## 5. 当前状态与数据（截至 2026-08）

分支 `feat/roxy-calibration-head`（待合并）最终方案：
- **Roxy**：序数 ridge meta 头（0.5 网格标签，lambda=2.0，拟合 11~17+≥17，全分布标准化），**无输出量化**，numeric=finalNumeric 与 estDiff 一致。
- **Mixed**：换路判定用 `debug.finalNumeric`；跨界规则（Roxy≥11 & Azusa<11 → Azusa）。
- **Azusa**：无改动（量化已回退）。

Benchmark（官方 runner，4K RC）：

| 算法 | 段 | Exact 前→后 | MAE 前→后 |
|---|---|---|---|
| Roxy | 11~17 | 55.1% → 59.4% | 0.229 → 0.219 |
| Mixed | 11~17 | 54.4% → 58.4% | 0.240 → 0.233 |
| Mixed | <11 | 22.1% → 22.9% | 0.545 → 0.545 |
| Mixed | ≥17 | 52.6% → 52.6% | 0.263 → 0.273 |
| Mixed | ALL | 47.4% → 50.5% | 0.307 → 0.302 |

未受影响：Mixed LN（MAE 1.228，Exact 9.8%——仍是整个系统最弱环节，样本/标注/特征三重受限）。

---

## 6. 借鉴 DanOverlay ISOR 的改进尝试（2026-08）

### 6.1 背景

DanOverlay 3.0.0 的 ISOR（Isotonic Strain Organic Residual）引擎在 benchmark 上略优于 Roxy（11-17 Exact 60.0% vs 59.4%）。分析其架构差异后，尝试借鉴其核心思想改进我们的三个算法组件。

ISOR 的关键创新：
1. **Choke NPS**：10 秒窗口峰值密度信号
2. **动态权重**：根据信号质量调整 blend 权重
3. **双频段 Ridge**：高难/低难分别训练的 98 维 meta 模型
4. **持续密度特征**：rolling NPS 分位数
5. **Marathon fatigue**：长地图疲劳修正
6. **Apex cosine gate**：高难段官方段位恢复
7. **Rate monotonicity**：三层速率单调性保证

### 6.2 已尝试的方法

#### Azusa（7 种方法，1 种有效）

| 方法 | MAE (baseline 0.670) | Exact±0.5 (baseline 70.1%) | 结论 |
|---|---|---|---|
| **10s Choke NPS dynamic gate** | **0.665** | 70.0% | **MAE -0.7%，已提交** |
| 5s Choke NPS 修正 | 0.6699 | 70.0% | 窗口太短，无提升 |
| midSpeedBonus 范围调整 | 0.6698 | 70.1% | 校准表已覆盖 |
| chordjackBoost 阈值降低 | 0.6712 | 69.2% | 过校正，退化 |
| lowBase primary 权重 0.05 | 0.6614 | 69.2% | MAE↑ Exact↓ |
| highBase primary 权重 0.10 | 0.7348 | 59.0% | 严重退化 |
| Sustained density CV | 0.6707 | 69.8% | 修正量太小 |

**成功的 Azusa 改进**：在 blend 公式中用 10s Choke NPS 与 avgNPS 的比值（burstRatio）微调 gate。当 burstRatio > 1.5 且 avgNPS > 5 时，gate 增加（更多权重给 lowBase）。

#### Roxy（3 种方法，均无效）

| 方法 | 结论 |
|---|---|
| 融合权重搜索 | 0.4 已近最优（MAE 差异仅 0.0017） |
| burst correction（peakToSustainGap） | gate 条件未触发，无效果 |
| structural backstop 阈值降低 | 11-12 段无改善（gap 为负不触发） |

#### Mixed（1 种方法，无效）

| 方法 | 结论 |
|---|---|
| Marathon fatigue correction | 严重退化（MAE 0.97→1.63） |

### 6.3 关键发现

1. **Azusa 已达架构极限**：校准表（block calibration、isotonic、residual correction）已针对当前 blend 输出高度优化。任何对 blend 或 strain 模型的修改都会改变校准表处理的值分布，导致不可预测的结果。

2. **Roxy 的 11-12 段是最大瓶颈**（MAE=1.57，其他段 0.19~0.46）。ISOR 用双频段 Ridge 解决，但我们无法重新训练 meta 模型（111 维 ridge，固定维度）。

3. **Roxy 融合权重已 near-optimal**：网格搜索显示 0.25~0.60 权重范围的 MAE 差异 < 0.005。

4. **Mixed 路由已很成熟**：当前路由逻辑（Roxy→Azusa→Daniel→Sunny）和跨界规则已精心设计。

5. **根本限制**：在不重新训练校准表/meta 模型的前提下，三个核心算法组件已接近其架构的理论上限。ISOR 的优势来自多信号三角测量、双频段 Ridge 和动态权重——这些都需要更大的架构变更。

### 6.4 未来方向

如果要进一步提升，需要：
1. **重新拟合 Azusa 校准表**：允许使用 benchmark 数据重新拟合 block calibration 和 isotonic 表
2. **重新训练 Roxy meta 模型**：添加 Choke NPS 等新特征，训练双频段 ridge
3. **探索多信号融合**：借鉴 ISOR 的三角测量思想，将 Choke NPS 作为独立信号加入 blend
4. **架构变更**：考虑将 Azusa 的 blend 公式改为 ISOR 风格的凸组合

---

## 7. 遗留问题与未来方向

- **LN 段**：Mixed LN 最弱（MAE 1.228），但 LN 样本少、人工标注难、特征分析难——需要独立的数据/标注工作。
- **≥17 段**：22 张小样本，Roxy 结构模型对部分图固有低估（structural 13~15 vs expected 17+）——标注分歧，无法可靠修复。
- **低难跨界图**（6 张 Azusa≥11）：所有算法都说 11+ 但标注 <11——无可靠区分信号。
- **更高容量模型（GBDT/神经网络）**：~500 有效样本下必然过拟合（历史反复验证），除非获得更多标注数据。
- **新特征（SV/BPM/变速段）**：现有 stats 已覆盖大部分；序列复杂度特征历史验证无效。
