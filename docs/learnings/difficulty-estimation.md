# 难度估计算法调优知识与教训

> 本文档记录在难度估计算法（Roxy / Azusa / Daniel / Sunny / Mixed / Companella）开发与调优过程中积累的知识与教训，来源包括本地私有临时目录中的历史探针/调优日志（gitignore，不入库）、benchmark 验证记录与历次优化会话结论。
>
> 目标读者：后续继续改进估计算法的 LLM 与开发者。**核心价值在于记录「什么无效」及「为什么」，避免重复踩坑。**

---

## 1. 环境与约束（必须遵守的前提）

- **运行时只有谱面文件本身**：插件在真实环境中只能获取 `.osu` 谱面文本，tosu WebSocket 不提供任何键型分类（pattern/subPattern 是 benchmark 数据集的标注，**不是运行时可用信号**）。任何依赖 benchmark 标注的算法改动都是过拟合。
- **benchmark 规则**：
  - `expected` = 段位谱面的数值化难度（Reform 体系，1st=1.0 … alpha=11.0，实际标签含 0.1 步进如 14.7）。
  - 指标：Exact = `|delta| ≤ 0.2`，Close = `≤ 0.5`，Moderate = `≤ 1.0`，Miss = `> 1.0`，`delta = expected − got`（正 = 低估）。
  - **禁止读取 `samples/` 谱面数据**（防止硬编码/过拟合）；训练/验证只允许用 `results/` CSV 的 `expected` 与各算法 `got`。
- **代码约束**：只能修改 git 跟踪文件；不得修改 benchmark 仓库（验证时把 runner 复制到本地私有临时目录并重定向输出，或仅用官方 runner 在获得授权后更新 `results/`，且 `index.json` 的 `source` 键会被非覆盖式保留）。
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
- **历史 15 轮调优**（2026-06，本地调优日志，不入库）：ridge lambda grid、GBDT（500 树）、isotonic 标定、reference stacking、浅残差树、二阶门控残差 ridge、序列复杂度特征、chordjack 修正、特征组消融、加权 ridge 等——**全部因 full/CV gap 过大或 KPI5（Miss<2%、Close+>80%、Moderate<10%）不达标而拒绝**。唯一部分接受的是保守的 reference-gap 残差校正（`±0.10` 最终影响）。
- **历史 GBDT 路由探针**（本地探针脚本，不入库）：500 树 GBDT、bucket 线性、isotonic、segmented 等模型对比，group-split CV 均退化——高容量模型在 ~500 样本上必然过拟合。

### 3.5 Azusa 相关的教训

- **低难 floor 量化**（<11 向下取整 0.5 网格）曾带来 Mixed <11 Close +14pp，但同样存在显示一致性问题（estDiff 基于未量化值），且对 expected 11~12 但输出 10.75~10.88 的图有害（floor 10.88→10.50，Close→Moderate）——**已随量化整体回退**。
- Azusa 对「结构难但段位低」的图（star of andromeda 等）也系统性低估——这是**标注与算法共识的分歧**（expected 11+ 但主流算法都认为 10 级），任何规则修复都是过拟合。
- **Azusa 的 LN 门控曾长期只存在于文档**（2026-09 发现）：`rcLnRatioLimit: 0.18` 自 Azusa 引入起只写在 config 与文档里，入口从未检查。此前未暴露是因为 benchmark runner 按 pattern 跳过 ln 行、Mixed 只把 RC tag（ln≤0.15）的图喂给 Azusa；一旦 Mixed 的低难融合在 Mix 分支（ln 可到 0.18）调用 Azusa，LN 主导谱面就会拿到 RC 模型的无意义数值。**教训：文档声明的输入约束必须在入口断言，否则迟早被新调用路径绕过。**

### 3.6 Mixed 低难段 Azusa⊕Companella 融合（2026-09）

- **方向有效但收益有限**：低难段（RC <11，n=187）融合真实运行 MAE 0.734→0.715、Exact +1.6pp，全段无回归（LN/≥17 与基线逐位一致），改动行净效应 −4.89 MAE 点（73 改善 / 52 回退）。离线探针（用独立 Companella CSV 代理流程内 Companella）预估的 MAE −0.08 在真实运行中缩水到 −0.019——**Mixed 流内 Companella 输入（sunnyStar 归一化、Ett 版本）与独立运行不同，离线代理会高估收益**。
- **一致性门控（|Azusa−Companella| ≤ 1.0）被证伪并移除**：它同时挡掉有益与有害的融合（净收益 4.89 → 2.87 MAE 点，变差）；移除后收益恢复至 −4.89。分歧大小无法区分方向对错，靠净效应平坦性兜底即可。
- **作用域门控是必须的**：仅当 Azusa 数值 <11 时融合 + `onDisagree` 回落分支原赢家，保证 11~17/LN 段逐位不变。无门控版本曾把 LN 段 MAE 1.228 → 1.303（Azusa 无 LN 门控时在 LN 主导图上输出假数值并被融合）。
- **参数面已穷尽**（五算法 debug 转储 + 离线全参数搜索）：w∈[0.4,0.7] 平坦（0.5 对称最优），scope>11 重新引入 11-17 泄漏且族级稳定性崩坏（17-19 worse），Roxy finalNumeric 作第三信号单调变差（0.528→0.533）——当前配置位于该机制族的平台上。
- **辅助发现**：数据集扩充后（746 行）低难段大部分行（84/137）走 LN/Mix 分支（ln∈(0.15,0.18]），「RC course 被 LN 分支劫持」不是个别现象而是低难段的主体路径——低难改进必须同时覆盖两条分支。

## 4A. 基准验证基础设施（harness 模式，2026-09 起）

- **不触碰 benchmark 仓库的验证方法**：把官方 runner + esm-loader 复制到本地私有临时目录（gitignore，不入库），改副本路径（插件 import 指向本地 js、samples 用绝对路径只读引用、输出重定向到该临时目录、禁用 bid 缓存拷贝），`node --loader esm-loader.mjs benchmark-runner.mjs --algorithm X` 即可全量复现官方结果（Sunny 745/746 行逐位一致；Mixed 因 main 后续合入的 Ett 0.74.0 重建有 45 行漂移，属预期）。
- **先转储后搜索**：给 runner 副本加 debug 转储（每行导出 numeric/star/actualAlgo/流程内 Companella/Roxy finalNumeric 等），一次批量跑完五个算法，之后所有参数搜索（融合权重、scope 阈值、三方融合、族级稳定性）都在离线脚本里秒级完成，只对最终配置跑一次确认。**不要用 10 分钟一轮的整跑去做参数搜索。**
- **official results/ 有滞后**：官方 CSV 生成后 main 又合入了影响估算的提交（Ett wasm 重建等），跨版本对比必须用同代码同环境的 harness before/after，不能拿官方旧数字当基线。

---

## 4. 方法论（有效的工作流）

1. **先离线探针，再改生产代码**：用 benchmark `results/` 的 got 值模拟候选改动的效果（如量化、路由规则），确认方向后再实现。注意：**离线模拟会忽略 Mixed 路由交互**（Azusa 量化离线显示 +9.3pp，真实 Mixed 反而退化——因为 azusaNumeric 参与换路 delta），最终必须真实验证。
2. **显示一致性是硬约束**：`numericDifficulty` 与 `estDiff` 必须同源。任何只改 numeric 不改 estDiff 的改动都是 benchmark 游戏。
3. **路由改动必须跑完整 Mixed benchmark**：路由规则的触发条件（delta 阈值、handBias/anchorRate）与输出量化、序数模型强耦合。
4. **小样本段（<11 的 20 张、≥17 的 6~22 张）不可靠**：Exact ±5pp 可能只是 1-2 张图；除非有明确的机制性理由（如序数模型上界外推），否则不要为小样本段做针对性修复。
5. **group-split CV 是唯一可信的过拟合判据**：full-fit 提升（如 53.9%→61%）若无 CV 支撑（39.5% vs 旧 47.5%）即过拟合。
6. **训练集边界决定外推行为**：只拟合 11~17 → 上界外推偏低；纳入 ≥17 → 修复。拟合集应覆盖目标 scope 的完整边界。

---

## 5. 当前状态与数据

### 2026-09 轮（分支 `feat/estimator-accuracy-experiments`）

本轮方案（零新增标注，纯算法/路由侧）：
- **Mixed 低难段 RC 融合**：Azusa⊕Companella 0.5/0.5（scope=azusa<11 + star<9 门控，`onDisagree` 回落分支原赢家，详见 §3.6 与 difficulty-estimation.md §3.6）。
- **Azusa LN 门控生效**：`rcLnRatioLimit=0.18` 在入口断言（此前只存在于文档，见 §3.5）。
- **缓存键 bump star-v4 → star-v5**（低难 numeric/estDiff 语义变化）。

Benchmark（harness harness before/after，746 行，官方 results 为旧代码口径不可直接对比）：

| 段 | Exact 前→后 | MAE 前→后 |
|---|---|---|
| Mixed RC <11 | 20.3% → 21.9% | 0.734 → 0.715 |
| Mixed RC 11~17 | 53.4% → 53.4% | 0.318 → 0.316 |
| Mixed RC ≥17 | 45.5% → 45.5% | 0.520 → 0.520 |
| Mixed RC ALL | 50.5% → 50.9% | 0.302 → 0.294 |
| Mixed LN | 9.8% → 9.8% | 1.228 → 1.228 |
| Mixed ALL | 44.9% → 45.3% | 0.428 → 0.422 |

改动行净效应 −4.89 MAE 点（73 改善 / 52 回退）。

### 2026-08 轮（分支 `feat/roxy-calibration-head`，已合并）

- **Roxy**：序数 ridge meta 头（0.5 网格标签，lambda=2.0，拟合 11~17+≥17，全分布标准化），**无输出量化**，numeric=finalNumeric 与 estDiff 一致。
- **Mixed**：换路判定用 `debug.finalNumeric`；跨界规则（Roxy≥11 & Azusa<11 → Azusa）。
- **Azusa**：无改动（量化已回退）。

Benchmark（官方 runner，4K RC，当时数据集 526 行）：

| 算法 | 段 | Exact 前→后 | MAE 前→后 |
|---|---|---|---|
| Roxy | 11~17 | 55.1% → 59.4% | 0.229 → 0.219 |
| Mixed | 11~17 | 54.4% → 58.4% | 0.240 → 0.233 |
| Mixed | <11 | 22.1% → 22.9% | 0.545 → 0.545 |
| Mixed | ≥17 | 52.6% → 52.6% | 0.263 → 0.273 |
| Mixed | ALL | 47.4% → 50.5% | 0.307 → 0.302 |

注：数据集此后扩充到 746 行（新增变速变体、LN Dan Courses 等），新旧数字不可直接对比。

未受影响：Mixed LN（MAE 1.228，Exact 9.8%——仍是整个系统最弱环节，样本/标注/特征三重受限）。

---

## 6. 遗留问题与未来方向

- **低难段标签噪声地板（本轮核心判断）**：若低难段每图标注误差 σ≈0.5~0.7 段位，完美估算器的观测 MAE 下限即为 0.4~0.55——当前 0.715 仍有少量空间但已接近；低难点精度的进一步提升**受标注质量封顶**，需要标签审计（复标 30~50 张测噪声地板）/ 成对比较标注 / 分级容差指标等评测侧工作，纯算法侧已穷尽（见 §3.6 参数面结论）。
- **LN 段**：Mixed LN 最弱（MAE 1.228），但 LN 样本少、人工标注难、特征分析难——需要独立的数据/标注工作。
- **≥17 段**：22 张小样本，Roxy 结构模型对部分图固有低估（structural 13~15 vs expected 17+）——标注分歧，无法可靠修复。
- **低难跨界图**（6 张 Azusa≥11）：所有算法都说 11+ 但标注 <11——无可靠区分信号。
- **更高容量模型（GBDT/神经网络）**：~500 有效样本下必然过拟合（历史反复验证），除非获得更多标注数据。
- **新特征（SV/BPM/变速段）**：现有 stats 已覆盖大部分；序列复杂度特征历史验证无效；本轮补充：Roxy finalNumeric 作低难第三融合信号已验证为单调变差（离线搜索，§3.6）。
