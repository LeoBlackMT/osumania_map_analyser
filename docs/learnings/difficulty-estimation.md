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

## 4. 方法论（有效的工作流）

1. **先离线探针，再改生产代码**：用 benchmark `results/` 的 got 值模拟候选改动的效果（如量化、路由规则），确认方向后再实现。注意：**离线模拟会忽略 Mixed 路由交互**（Azusa 量化离线显示 +9.3pp，真实 Mixed 反而退化——因为 azusaNumeric 参与换路 delta），最终必须真实验证。
2. **显示一致性是硬约束**：`numericDifficulty` 与 `estDiff` 必须同源。任何只改 numeric 不改 estDiff 的改动都是 benchmark 游戏。
3. **路由改动必须跑完整 Mixed benchmark**：路由规则的触发条件（delta 阈值、handBias/anchorRate）与输出量化、序数模型强耦合。
4. **小样本段（<11 的 20 张、≥17 的 6~22 张）不可靠**：Exact ±5pp 可能只是 1-2 张图；除非有明确的机制性理由（如序数模型上界外推），否则不要为小样本段做针对性修复。
5. **group-split CV 是唯一可信的过拟合判据**：full-fit 提升（如 53.9%→61%）若无 CV 支撑（39.5% vs 旧 47.5%）即过拟合。
6. **训练集边界决定外推行为**：只拟合 11~17 → 上界外推偏低；纳入 ≥17 → 修复。拟合集应覆盖目标 scope 的完整边界。

---

## 4.1 基准验证基础设施（harness 模式，2026-09 起）

- **不触碰 benchmark 仓库的验证方法**：把官方 runner + esm-loader 复制到本地私有临时目录（gitignore，不入库），改副本路径（插件 import 指向本地 js、samples 用绝对路径只读引用、输出重定向到该临时目录、禁用 bid 缓存拷贝），`node --loader esm-loader.mjs benchmark-runner.mjs --algorithm X` 即可全量复现官方结果（Sunny 745/746 行逐位一致；Mixed 因 main 后续合入的 Ett 0.74.0 重建有 45 行漂移，属预期）。
- **先转储后搜索**：给 runner 副本加 debug 转储（每行导出 numeric/star/actualAlgo/流程内 Companella/Roxy finalNumeric 等），一次批量跑完五个算法，之后所有参数搜索（融合权重、scope 阈值、三方融合、族级稳定性）都在离线脚本里秒级完成，只对最终配置跑一次确认。**不要用 10 分钟一轮的整跑去做参数搜索。**
- **official results/ 有滞后**：官方 CSV 生成后 main 又合入了影响估算的提交（Ett wasm 重建等），跨版本对比必须用同代码同环境的 harness before/after，不能拿官方旧数字当基线。

---

## 5. 当前状态与数据

### 2026-09 轮（分支 `feat/estimator-accuracy-experiments`）

本轮方案（零新增标注，纯算法/路由侧）：
- **Mixed 低难段 RC 融合**：Azusa⊕Companella 0.5/0.5（scope=azusa<11 + star<9 门控，`onDisagree` 回落分支原赢家，详见 §3.6 与 difficulty-estimation.md §3.6）。
- **Azusa LN 门控生效**：`rcLnRatioLimit=0.18` 在入口断言（此前只存在于文档，见 §3.5）。
- **缓存键 bump star-v4 → star-v5**（低难 numeric/estDiff 语义变化）。
- **缓存键 bump star-v5 → star-v6**（Roxy `graph` 时间轴从 canonical 还原为原始谱面时间；旧快照的 `times` 会导致图表 x 轴窗口与进度线错位——见 [graph-visualization.md](../features/graph-visualization.md) §4.5 与 roxy_algorithm.md）。
- **缓存键 bump star-v6 → star-v7**（Etterna junk 文件（技能值全零）不再显示 `0.00`，快照里 `ettResult` 语义变化：旧快照没有 `junkFile` 字段，命中后会静默退回 `0.00`——见 [../breakings/2026-09-20-ett-ux-and-companella-capsule.md](../breakings/2026-09-20-ett-ux-and-companella-capsule.md)）。
- **缓存键 bump star-v7 → star-v8**（与主线 `star-v7`（Etterna junk 文件不再显示 `0.00`）合流后重新编号；Roxy meta 头对退化修正项 `corr_lowCj` 改用「特征关闭状态取值」截断，即 `|z| <= |mean/scale|`——该系数是在特征恒为常数处拟合的（beta/scale = -134），遇到正常触发值即外推出极端离群项，单张图实测被压低 2.49 分、标签从 Delta 级掉到 Beta low，见 roxy_algorithm.md §11.1；报告图的 `metaNumeric` 由 `9.60 → 13.92`，最终数值 `11.65 → 14.19`，标签 `Beta low → Delta mid/high`，其 Azusa(14.48)/Daniel(14.62) 参考未变）。**为什么不是按 σ 截断**：issue #70 的两张参考图几乎同源（同一时间轴、90.1% 的行内音符数相同、98.3% 的音符能在对方里找到同列同刻的对应音符），只因为 `overlapRate` 是 0.7772 对 0.7413——刚好跨过 lowCj 闸门的 0.75 下界——一张触发一张不触发；±3σ 截断下仍差 1.01 分（触发的那张仍被压低 1.86 分），而 Azusa(差 0.06)/Daniel(差 0.07)/Roxy structural(差 0.10) 都认为两张同难。改成关闭状态截断后两图差 0.05、标签一致。**名单只含 `corr_lowCj`**：其余 corr_* 项病态程度低 1~3 个量级（`corr_courseSustainLift` 的 beta/scale 仅 21、`corr_denseJsLift` 仅 0.4、另两项 beta 恒为 0），且在 benchmark 上携带有效信号——一并按 σ 截断会改动 7 行、其中 4 行变差（MAE `0.2424`），只处理 `corr_lowCj` 则不改变任何已报出数值。对照的"直接删除该特征"方案会连带丢掉它在 `f ≈ mean` 处的常数贡献（+0.028），使全表整体下移（实测改动 474/746 行）。

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

### 2026-09 二轮（分支 `feat/estimator-accuracy-opt-2026-09`，全方案被拒）

在 2026-09 一轮（融合）基础上做的全量离线实验轮：一次性转储（746 行 × sunny/azusa 双口径/roxy(含 111 维特征)/流程内 companella/mixed 全中间值）后用离线脚本横扫参数面，未逐轮整跑。harness 与官方 results 逐行对比仅 45 行漂移（Ett 0.74.0 重建，§4.1 已知）；roxy 特征重放金标准验证误差 <1e-4（管线可信）。

| 方案 | 结果 | 裁决 |
|---|---|---|
| **C1 Azusa isotonic 表 746 重拟合**（PAVA 自洽迭代；连续/0.5 网格/端点 pin × 拼接 cut 9.5/10/11/17） | 11~17 full exact 47.2→49.7%、CV exact +1.96pp；**但 <11 CV MAE 0.54→0.63（-0.09）**、mae_all CV 变差、≥17 插值过渡退化 | 拒（CV 全量口径不提升） |
| **C2 Roxy ridge 746 重训**（λ∈{0.1…8} × LN 排除/变体行排除 5 变体） | **所有变体 CV 11~17 exact 48~49% vs 旧模型 60.4%、MAE 0.267~0.28 vs 0.217（-12pp）**；排除 LN/变体行反而更差 | 拒（全面退化） |
| **C3 软路由探针** | 低难融合 w 全局扫 best=0.6（与 0.5 差 0.0007，噪声内）；条件 w(d) 无增益；11~17 软融合 w=1（纯 Roxy）最优=现状 | 拒（平坦面确认，无空间） |
| **C4 LN 专项** | 102 行（<11 47 / 11~17 52 / ≥17 3），基线 MAE 1.1~2.1；违反小样本判据 | 未实验 |

**本轮教训（新增 220 行数据的真实价值）**：
1. **746 上重训全被数据拒绝**：新增 220 行（变速变体 + LN Dan Courses 的 RC 掺杂）对 ridge/isotonic 是弱信号；526 时代的纯 RC 拟合分布在 746 上依然最优。排除变体行/LN 行只会更差（同族变体行删除后有效样本骤减，CV 崩到 0.38~0.40）。
2. **PAVA 平台化伤害低难个体**：重拟合表在低难段形成平台（多个 preOutput 映射同一输出，如 9.7565），使个别行误差从 0.06 放大到 0.98——老表在低难段有更细的梯度（526 拟合时低难行占比更高）。
3. **docs/azusa_algorithm.md 的 "79-point" 描述过时**：`AZUSA_ISOTONIC_POINTS` 实际 84 点（后续扩充未同步文档，已修复）。
4. 11~17 段仍有 ~2pp exact 的"表级"空间（CV 稳定复现），但任何重拟合都以低难/高难段退化换取——全局单调表无法分带优化，拆分拼接（cut 方案）也因插值过渡区与单调约束失效。
5. C3 三连证伪闭环：一致性硬门控（一轮）→ 参数面平坦（一轮）→ 条件权重/软融合（本轮），低难融合机制族内无剩余结构。

### 2026-10 轮（分支 `feat/polaris-estimator` 的后续调研：键型族修正——**全轴被严格 CV 否决**）

**背景**：在 Polaris（7K 表修正）+ 4K LN 后修正落地后，尝试利用 Mania Tracker 行为数据集（4.84M plays，`Desktop/Dev/files/mania-tracker-dataset`，仅聚合 CSV、**不含任何 .osu**）与 benchmark，进一步引入"键型族（tag family）修正"。三处候选全部经严格 CV 否决，过程与证据如下（全部脚本在本地私有 `temp/polaris/fit/`，gitignore 不入库）。

**1. 7K tag 族——否**。以 `charts_7k.csv`（in_table、n_players≥8，n=6899）拟合"残差 = y − Polaris 修正"，做 level/rate 混淆控制回归 + 按 beatmap 分组 CV：全 8 族联合 held-out 仅 **+0.53%**，与 shuffle 安慰剂（+0.15%）几乎无差；in-sample 反而 **−0.12%**。唯一有量的 `lnrelease`（level-matched diff +0.84）单族 CV **−0.16%**（族只在 L3–6、跨 beatmap 不泛化）；`lninverse` 在低/高段**反号**（+0.7 → −0.5），常量会主动搞错。⇒ **原始 tag 表的"效应"几乎全是 level/rate 混淆。**

**2. 两个必须记住的数据陷阱（导致早期假阳性 +4.52%）**：
- **pooled-algo 混淆**：`charts_4k.csv` 的 `companella_numeric` 列在 **algo=Azusa 的行上也存在**（是 CX 原始输出，非采纳值）。把 `algo∈{Azusa,Companella}` 混池拟合 `y=play_implied−companella_numeric` 会得到假的 +4.52%——因为 algo=Azusa 行 mean_y=−0.364 vs algo=Companella +0.53，相差 0.9，混池后 tag 只是在编码"这行被谁采纳"。**正确做法：只取 `algo=="Companella"` 的行（n=8278）**，真实增益 +4.08%。
- **nps 是难度水平的近共线代理，绝不可用**：在 rate==1.0 内 `corr(nps, companella_numeric)=0.827`（跨 rate 混池时会稀释到 0.054，极具迷惑性）。nps-only 的"CV +14%"与 shuffle 安慰剂 **+14.26% 完全相同** ⇒ 纯属把难度刻度重学一遍、与 CX 自身校准互相抵消。

**3. 4K tag 族（Companella 基线，9 族）——被 LOO 与退化性否决**。只取 `algo=="Companella"` 后，9 族联合 group CV +4.08%，shuffle 安慰剂 ±0.1%。但 drop-one 显示 `quadstream`/`chordjack` 贡献 ≈0（早期 −0.71 的 quadstream beta 也是 algo 混淆的产物）；`ln` 族 prevalence=0.998（近乎全体，退化为全局截距而非"族"）。剔除后退化为 6 族，CV 反而 **+4.11%**。

**4. 独立于 Mania Tracker 的本地特征验证（决定性）**。为绕开"MT tag 定义 ≠ 本仓 pattern 定义"的映射不可验证问题（数据集无 .osu、benchmark 分类正交），直接用本仓 `js/patterns/service.analyzePatternFromText` 跑 benchmark 全部谱面（737/776 成功），产出本地 `SpecificTypes`/`Pattern`，与**已含 LN 修正的当前 `Mixed.csv`** 按谱面名连接：
- 本地族对 benchmark delta 的分组 CV（按谱面名分组）**+2.52%**，安慰剂 −0.12…+0.61% ⇒ 本地特征确有真信号，**映射风险不成立**。
- 但结构定位彻底清楚：**RC 行残差 = −0.004（已完全校准）** ⇒ RC 侧族效应（Quadstream+0.15/Handstream+0.10/Jumpstream+0.09）不是系统偏差，**不该修**；**LN 行残差 = +0.570**（`lnRatio≥0.375` 的 100 行）⇒ 全部剩余信号都在 LN，且**所有 LN 子类型残差一致在 +0.40~+0.76**（HS Density/Inverse +0.76、Shield +0.67、JS Density +0.62、Speedy WC +0.60、Release +0.57…）——这是**一个统一的 LN 偏移，不是可分的"族"**。

**5. 结论**：**不新增"pattern 族修正"、不改 LN 幅度**。
- 水平相关 LN 修正（按 2-dan 分箱 knot）CV **−1.19%**（knot 非单调：L4 +1.58 / L10 −0.11 / L14 +1.0，100 行样本不稳）。
- 提高 LN 修正幅度也不稳：train-fit bonus=+0.574（隐含 magnitude→1.07）held-out 仅 **+1.18%**；敏感度扫描在 0.25→+2.08%、0.50→+1.56%、0.75→**−0.87%** 之间跳 ⇒ 最优值不稳定。**当前 flat +0.5 已属合理保守值。**
- **可复用教训**：① 行为数据集的算法残差若按"采纳算法"混池，会引入与 base 身份相关的巨大伪效应，**必须严格按被采纳算法分层**；② 任何"密度/速度"类连续特征在**同难度内**检验前，先看它与难度本身的相关系数（>0.8 即共线，只能当难度代理）；③ 当所有差异最终收敛成一个统一偏移时，说明缺的不是模型而是**标注样本量**（benchmark LN 仅 100 行）。

**未来正确方向**：若要吃 LN 剩余的 +0.57，需要**更多 LN 标注样本**（或改用本仓自算 LN 结构难度作连续特征重拟合，但同样受样本量限制），而不是更花哨的键型族模型。4K RC 侧已无系统性偏差可修；7K 侧受 benchmark 无 7K 样本限制，只能用 Mania Tracker 行为集（已做 Polaris）。

### 2026-10 轮（分支 `feat/speed-estimation-refinement`，Follow Speed 扩充 30 样本与 Gamma 速度流平滑过渡）

- **背景**：基准数据集扩充 30 个 Follow Speed 练习曲样本（Gamma/Delta 段位，746 → 776 行），Speed 样本总数由 147 增至 177 行。
- **发现**：
  1. Azusa Stage 6 `computeReferenceCorrection` 在 `x = 12.5` 存在硬截断阶跃：`[11.5, 12.5)` 区间 Daniel 权重为 0.20，而 `[12.5, 16.0)` 跃升至 0.40。
  2. 纯单键速度流（如 *Sakura no Uta*, *golden hour*, *Brionac*, *Yuudachi no Ribbon* 等 Gamma 速度曲）受制于 PAVA 单调表在 `[12.2947, 12.4150]` 区间的平台，`outputNumeric` 被锁定在 ~12.44，恰好被卡在 12.5 阶跃左侧；即使 Daniel 已明确指向 Gamma 段（12.9~13.2），Daniel 纠偏强度仍被腰斩（0.20 而非 0.40）。
- **实验与教训**：
  1. **全局修改单调表**：尝试压平 12.29~12.87 平台，导致非速度曲（*Blue Planet*, *Asymmetrical Grooves* 等）严重回归，不可行。
  2. **Stage 6 全局参考拉力抑制**：在 Daniel > Sunny 时抑制 Sunny 负拉力，导致高体力通货膨胀图（*Xecus*, *Youthful eyes*, *LUCIA*）大幅高估，不可行。
  3. **速度流条件化的 Gamma 平滑参考过渡（成功方案）**：
     仅当 `refD > 12.0`（Daniel 指向 Gamma+）且 `chordRate < 0.45`（纯单键速度流，`streamWeight = clamp((0.45 - chordRate) / 0.15, 0, 1)`）时，对 `[11.5, 12.5)` 的纠偏系数按 `refD` 线性插值过渡到 0.40 / 0.25。
- **结果（776 行全量 Harness 验证）**：
  - **Azusa**：
    - 新增 30 张 Speed：MAE `0.2257 → 0.2193`（-0.0063），Exact `56.7% → 60.0%`（+3.3pp）。
    - 全部 177 张 Speed：MAE `0.3398 → 0.3388`（-0.0010），Exact `36.2% → 36.7%`（+0.5pp）。
    - **全部 497 张 Non-Speed（Jack/Stamina/Tech/Course）：MAE 0.3293，Exact 44.5%（逐行 0 回归，diff 0.0000）**。
    - 全部 674 张有效谱面：MAE `0.3320 → 0.3318`，Exact `42.3% → 42.4%`。
  - **Roxy**：通过 Azusa 融合自动受益，全量 390 张 Non-Speed 逐位一致，Gamma 速度图估值更趋近标注。
  - **Mixed**：新增 30 张 Speed MAE `0.2170 → 0.2160`，599 张 Non-Speed 0 回归（MAE 0.4510 保持一致）。

---

## 5.1 2026-10 复审三项候选（Companella 偏移后）——全部否决，附方法论

在 Companella 系统性偏移（4K，RC +0.443 / LN +0.786，`companellaOffset.js`）落地后，对复审提出的三项候选逐一做了严格 CV。**三项均不落地**，但失败原因各不相同，方法论价值高。

### 关键先验：`delta` 列的离散度远大于系统偏差，MAE 口径会"看不见"真实偏差

行为数据集的 `delta` 组内 sd ≈ 0.85~1.1，而各候选的 mean 偏差只有 0.1~0.6。因此**任何"按组加一个常数"的修正在总体 MAE 口径下都极难检出**，且**安慰剂常常优于真特征**（因为随机组同样能把均值往 0 拉一点）。判定"是否真有系统偏差"必须（a）按被采纳算法分层，（b）做 level / nps 匹配，（c）用"修正前后 |delta| 改善比例"而非 MAE 增益。

### 候选 1：4K RC 侧 Hybrid LN 过渡补偿（`0.15 <= lnRatio < 0.375`）——否决

- 现象真实：`bias_tables_4k.md` 显示 RC 侧 `(0.15,0.25]` +0.27、`(0.25,0.375]` +0.59，看起来像"纯米饭算法看不见长条"。
- **但分层后发现该区间几乎全是 Companella 路由**（`(0.25,0.375]` 的 2862 行里 2830 行是 Companella）⇒ 该 gap **已被 Companella 偏移吸收**，扣除后 `(0.15,0.25]` 残差仅 −0.014。
- 扣除后残余集中在 **Azusa 路由**（+0.226，n=1645）。但严格 CV：真实增益 **−0.08%**，安慰剂 **+1.63%**（安慰剂更好）⇒ 不可检出。组内 sd 1.073 ≫ mean 0.226；加常数后 |delta| 改善仅 48%（52% 反而变差）。
- **结论**：不是"RC 算法缺一个 LN 项"，而是 **Azusa 本体对 hybrid 系统性低估**——属移植算法内部问题，按硬约束不在后处理层修。

### 候选 2：马拉松时长修正折减回捞——否决

- 现象比候选 1 更强且**跨算法一致**：扣除 Companella 偏移后，marathon 行相对基线的 gap 在 Azusa / Companella / Roxy 上分别为 **+0.225 / +0.199 / +0.380**（不是某算法的产物）。
- **且经 level 与 nps 双重匹配后仍稳定存在**（level 各档 gap +0.20~+0.33；nps 各档 +0.17~+0.37；双重匹配 +0.18）——这不是混淆。
- 但**统计功效不足**：marathon=1.0 仅占总体 **5.6%**（2033/36305），扣除偏移后组内 mean 仅 **+0.099**，sd 0.972。总体 MAE 口径 CV 增益 **+0.05%** ≪ 安慰剂 **+0.9~1.05%**。
- **结论**：信号真实但不值得动——修正幅度（~0.1）小于该组的噪声地板，且会波及 5.6% 的谱面换来不可检出的收益。**维持现状**。（若未来 marathon 样本量大幅增加，可重估。）

### 候选 3：Benchmark 考题标签审计（诊断，非修正）——已产出

用 benchmark `results/Mixed.csv` 自身审计标注质量，**不做任何代码改动**，结论高价值：

| 子组 | n | mean | MAE | &#124;err&#124;≥2 |
|---|---|---|---|---|
| `ln` 中 "4K LN Dan Courses v2"（Stage Map Pack） | 64 | **+1.067** | **1.388** | **20** |
| `ln` 中普通 LN 图 | 38 | +0.928 | 0.957 | 3 |
| `course`（Dan ~ REFORM ~ Nth Pack） | 34 | −0.459 | **0.473** | **0** |

- **全 benchmark 的 23 行 `|err|≥2.0` 里，20 行是同一个 "4K LN Dan Courses v2" 包**。这解释了 LN 类目 MAE 异常高的主因：该包是**旧 LN course 包**，其 `expected` 是 course 段位而非单谱面难度，与算法"逐谱面估算"口径错位。
- 对照：标准 `course`（REFORM Nth Pack）MAE 仅 0.473，**零行超标**——说明 course 标注本身没问题，问题只在那一个旧 LN 包。
- **用途**：未来评估 LN 改善时应**分层报告**（普通 LN 图 vs 旧 course 包），避免为 20 行标签口径错位的样本做针对性过拟合。这与 §3 的"LN 样本少、标注难"相互印证。

### 方法论沉淀（新增，重要）

1. **安慰剂必须同规模**：安慰剂组若与真特征组占比不同，增益不可直接比。比较时须固定组大小或改用"修正前后改善比例"。
2. **`delta` 高离散下，MAE 不是检测系统偏差的合适指标**：应辅以 signed bias 与"改善比例"。
3. **混淆检验三步**：先按被采纳算法分层（避免 pooled base-identity），再做 level 匹配，最后做密度（nps）匹配。候选 1 倒在第一步，候选 2 通过了两步但仍倒在统计功效。
4. **总体占比决定可行性**：5.6% 的子群即使有 +0.1 的真实偏差，也无法在总体指标上体现——这类"真实但不可用"的信号应记录而非实现。

---

## 5.2 2026-10 自查：Companella 偏移自身的两处缺陷（已修正）

§5.1 的结论确立后，梳理 Mixed 路由细节时查出**本分支自己刚引入的偏移**（`companellaOffset.js`）有两处缺陷。两处都不是"某个候选被否决"，而是"已落地的修正本身错了"——记录于此，作为 §4 工作流的补充检查项。

### 缺陷 1：跨量纲之差被当作偏移（LN 侧 +0.786）

**错误推导**：`play-implied − companella_numeric` 在 LN 侧均值 +0.786，据此对 Companella 数值施加 +0.786。

**为什么无效**：偏移要求被相减的两个量测的是**同一个量**。

| 侧 | 数据集取的判定量 | Companella 数值实际是什么 | 同量纲？ |
|---|---|---|---|
| RC | RC 半段（`verdict_used == "RC half"`） | RC 半段 | ✅ 5621 行上 `leo_numeric == companella_numeric` **逐位相等** |
| LN | **LN 半段**（`verdict_used == "LN half"`，承载 Sunny 星数过 LN 表的值） | **RC 半段** | ❌ 两个不同的量 |

⇒ LN 侧 +0.786 是跨量纲之差，不是偏移量。同人群上同量纲的残余是 +0.191，而它描述的是 LN 半段（Sunny LN 表），与 Companella 无关。LN 侧无任何同量纲的 Companella 测量 ⇒ 偏移归 0，LN 侧修正仍由 `lnCorrection` 承担。

**排查手法**（可复用）：对目标人群做 `leo_numeric vs companella_numeric` 的**逐位相等计数**。RC 侧 100% 相等 ⇒ 量纲一致；LN 侧仅 0.5% 相等、差均值 +0.594 ⇒ 立刻暴露。再对 `leo_numeric vs leo_continuous` 做同样检查可确认判定量取自哪一半。

### 缺陷 2：融合值上全额施加偏移

`C8` 输出 `0.5·Azusa + 0.5·Companella`，只有一半来自偏移所描述的算法。全额施加把只占一半的来源的尺度差整体计入结果。修正：按 `companellaWeight` 缩放（C6/C9 = 1，C8 = 0.5）。

行为数据侧确认（RC 侧 Mix 带，n=1645）：基线 +0.226 → 权重 ×0.5 得 **+0.004**，全额得 **−0.217**（比不修正更差）。

**注意**：该带 MAE **几乎不动**（0.5946 → 0.5996，反而略升），而 signed bias 从 0.226 降到 0.004——再次印证 §5.1 的关键先验；若用 MAE 选方案会误判全额为"更好"。

### 修正后的验证（RC 侧）

严格分组 CV（`beatmap_id % 10`，n=5621）：bias +0.443 → **+0.077**，MAE 0.9107 → **0.8611（−5.44%）**，**10/10 折同号**（+0.41…+0.49）。对照：错误幅度 +0.786 → MAE 0.9245（比不修正更差）；符号反转 −0.443 → 1.0944。

Benchmark ln 行（102 行，**101 行在 LN 侧**故不被本偏移触及）：

| 方案 | bias | MAE | Exact | Close |
|---|---|---|---|---|
| 官方基线 | +1.015 | 1.228 | 25.5% | 48.0% |
| 仅 lnCorrection | +0.554 | 0.948 | 39.2% | 57.8% |
| RC-only 偏移（本设计） | +0.550 | 0.953 | 39.2% | 57.8% |
| LN 全额 +0.786（已撤回） | −0.229 | 0.915 | **30.4%** | 65.7% |

撤回方案的 bias/MAE/Close 更好但 **Exact 更差**：把 LN 数值整体上推是"拿精确命中换接近"。该张力如实记录，未用常数抹平。

### 方法论沉淀（比 §5.1 更前置的一条）

> **0. 量纲/来源核对必须在 CV 之前。** 确认"被拟合的量"就是"被修正的量"：同一算法、同一半段、同一数值口径。§5.1 的 base-identity 陷阱是**跨算法**版本，本轮栽的是**跨半段**版本——同类错误换了一层皮。CV 只回答"这个修正是否可复现"，不回答"这个修正是否在修正确的东西"；后者只能靠量纲核对，且核对成本极低（一次逐位相等计数）。
>
> 附带结论：对称地，任何"按侧/按组分别拟合常数"的方案都应先做这一步，否则最坏情况下会得到一个自洽且 CV 良好、但完全无意义的常数。

---

## 6. 遗留问题与未来方向

- **低难段标签噪声地板（本轮核心判断）**：若低难段每图标注误差 σ≈0.5~0.7 段位，完美估算器的观测 MAE 下限即为 0.4~0.55——当前 0.715 仍有少量空间但已接近；低难点精度的进一步提升**受标注质量封顶**，需要标签审计（复标 30~50 张测噪声地板）/ 成对比较标注 / 分级容差指标等评测侧工作，纯算法侧已穷尽（见 §3.6 参数面结论）。**§6.1.3 已给出该判断的第一个可量化证据**：把 benchmark 与行为数据集的 `play_implied_dan` 对齐后，误差预算在"标签虚高"（118 行）与"算法漏算"（69 行）之间约各占一半。
- **LN 段**：Mixed LN 最弱（MAE 1.228），但 LN 样本少、人工标注难、特征分析难——需要独立的数据/标注工作。**2026-10 标签审计（§5.1 候选 3）显示其中 20/23 的超标行来自单一 "4K LN Dan Courses v2" 旧 course 包（`expected` 为 course 段位而非单谱面难度）；§6.1.3 用 `play_implied_dan` 独立确认该包标签虚高 1.5~2.6**；普通 LN 图 MAE 0.957、仅 3 行超标。⇒ LN 评估应分层报告。当前 benchmark LN 的 bias +1.015 由 `lnCorrection`（+0.5 ramp）压到 +0.554（Exact 25.5% → 39.2%）；**benchmark 想要的总量更大（raw +1.477），但该证据仅 101 行且单源，按 §3 判据不足以调幅**——若要提高，入口是 `lnCorrection` 的幅度本身，而非其他模块的常数（见 §5.2）。
- **≥17 段**：22 张小样本，Roxy 结构模型对部分图固有低估（structural 13~15 vs expected 17+）——标注分歧，无法可靠修复。
- **低难跨界图**（6 张 Azusa≥11）：所有算法都说 11+ 但标注 <11——无可靠区分信号。
- **更高容量模型（GBDT/神经网络）**：~500 有效样本下必然过拟合（历史反复验证），除非获得更多标注数据。
- **新特征（SV/BPM/变速段）**：现有 stats 已覆盖大部分；序列复杂度特征历史验证无效；本轮补充：Roxy finalNumeric 作低难第三融合信号已验证为单调变差（离线搜索，§3.6）。

## 6.1 评测侧实测（2026-10，倍速单调性 / 成对偏序 / 外部锚定）

> 本节全部只读分析，**不涉及任何算法改动**。脚本在 `temp/`（`phase2-audit.py` / `phase2-anchor.py`）。

### 6.1.1 倍速单调性：**插件侧不存在缺陷**（4K 行为数据集，9,022 对）

用"同谱面不同真实 rate"配对（6,713 个 beatmap、9,022 对；同谱面同 rate 无重复行）：

| 量度 | 严格倒挂（rate 升而值降） | 说明 |
|---|---|---|
| **Sunny star** | **1 / 9,022 = 0.01%**（幅度 −0.041） | star 全程由插件内部计算、不经下游，是最干净的量度 |
| 数据集 `leo_numeric`（未过滤） | 391 / 9,022 = 4.33% | **假象**，见下 |
| 数据集 `leo_numeric`（两侧 star 均在表内） | 192 / 8,685 = 2.21% | 仍偏高，见下 |

**为什么 `leo_numeric` 不能用来测这件事**：4K RC 表的起点是 **star 1.502**（首段为 `Intro 1 low`）。当 star 低于表起点时，插件只输出边界标签（`< Intro 1 low`）且 `numericDifficulty = null`；而下游数据集此时取的是**目录侧的判定值**（例：star 1.004 的行的 `leo_verdict_text` 是 `Reform 2 high`——这是插件表映射**不可能**产出的标签，因为该 star 已在表外）。这类值随 rate 变化时不受插件控制，于是被误读成"倒挂"。实测：19 个幅度 >0.5 的"倒挂"全部 star < 1.7，且 `leo_numeric == leo_continuous`（即取自判定文本）。

⇒ **结论：倍速单调性无需修复。真要测该性质，必须用 star 或已确证量纲一致的列**（如 RC 侧 Companella 行，`leo_numeric` 与 `companella_numeric` 逐位相等）；用未过滤的 `leo_numeric` 会得到 4.33% 的假违规率。

### 6.1.2 成对偏序（Kendall τ）：稳健但**未揭示 MAE 漏掉的信号**

benchmark 775 样本，各算法可用行数不同（Roxy 532、Daniel 551、Azusa 673、Sunny 756、Mixed 775），**跨不同子集比较无意义**。共同子集（四算法同时可用，n=532）：

| 算法 | τ（共同子集） | 同子集 MAE |
|---|---|---|
| Mixed | **0.8971** | **0.2376** |
| Roxy | 0.8965 | 0.2415 |
| Azusa | 0.8698 | 0.2808 |
| Sunny | 0.8632 | 0.5315 |
| Daniel | 0.8619 | 0.4664 |

**τ 与 MAE 给出完全一致的排序**。⇒ τ 的价值在"标签本身可疑时仍可作相对判据"，而**不在当前能产出新结论**；不应把它当"独立于 MAE 的核心基准"来预期收益。

### 6.1.3 benchmark × 行为数据集外部锚定（新方法，439/775 可匹配）

**方法**：benchmark 的 `bid` 非空 541 / 775，其中 439 行能在行为数据集中找到同 `beatmap_id`（取 rate = 1.0 行）。以 `play_implied_dan`（484 万次真实成绩经下游 ACC 积分曲线反推）作为**独立于人工标签**的锚，比较 `expected` 与它。

**整体**：`expected − play_implied` 均值 **+0.147**、中位数 +0.130 ⇒ **benchmark 标签整体略高于玩家实际表现**。

| 类别 | 行数 | 该组 Mixed 的 MAE |
|---|---|---|
| 两者一致（\|差\| ≤ 0.5） | 252 | **0.3327** |
| **标签虚高**（差 > +0.5） | **118** | **0.5231** |
| **算法漏算**（差 < −0.5） | **69** | **0.5255** |

⇒ **benchmark 的误差预算在"标签错"与"算法错"之间约各占一半**（118 vs 69 行，且两组的 MAE 都约为一致组的 1.6 倍）。这是 §6"需要评测侧工作"的第一个可量化证据。

**独立确认 LN course 包标签不可信**：残差 Top-20 仍由 `4K LN Dan Courses v2` 包主导，而外部锚定把它们判为**标签虚高**——多个条目的 `play_implied` 比 `expected` 低 1.5~2.6（例：exp 6.0 / 实测 4.54；exp 5.0 / 实测 2.39；exp 1.0 / 实测 −1.55；均约 12~14 次游玩、acc 95~98%）。**这为"排除该包"的既有决策提供了 484 万次游玩的独立背书**（此前依据仅为该包 20/23 行超标这一统计特征）。

**真正指向算法（而非标签）的候选**（标签 ≈ 实测，但插件偏离大，值得人工复审）：

| 谱面 | pattern | expected | Mixed got | play_implied | 判定 |
|---|---|---|---|---|---|
| `Kikai Shoujo Gensou(rg9v3)` | jack | 8.5 | 10.86 | 9.57 | 标签与实测一致（差 −1.07）⇒ **算法高估 2.36**（178 次游玩、acc 97.9%） |
| `Dusk(mintapril)` | jack | 8.0 | 10.28 | 7.94 | 标签与实测一致（差 +0.06）⇒ **算法高估 2.28** |

两张都是 jack，且都有充足游玩量（178 / 19 次）与高 acc ⇒ **是算法盲点而非标签问题**，与 §6.1.1 无关。可作为后续 jack 特征的定点复核样本（注意 §3.4 记录 chordjack 类修正已被反复否决，动它需要新证据）。

### 6.1.4 元模型不可复现（供未来避免误设任务）

`roxyMetaModel.generated.js` 的训练集**不是 benchmark**，故"用 benchmark 重拟合以复现权重"在原理上不可能。三条证据：

1. 头部：`ordinal target, grid=0.5, lambda=2.0, fit 11~17+>=17`
2. `ROXY_META_MEAN`：`pred_Azusa` 均值 **13.994**、`pred_Roxy` **13.233**（训练集集中在 14 段附近）
3. **6 个特征的 `scale == 1.0`（训练集中恒定）**：`has_Azusa` / `has_Sunny` / `has_Daniel` / `has_Roxy` / `corr_courseBreakDamp` / `corr_handBiasLift`——其中 `has_Sunny` 均值 = 0、其余三个 = 1，即**训练集里四条参考预测全部可用**；而 benchmark 上 Azusa 仅 673/775、Roxy 仅 532/775，**根本凑不出该训练集**。

⇒ 用 benchmark 重拟合只能得到**一个新模型**，不是恢复基准线。另：111 维向量的组装函数在 `roxyEstimator.js:1358`，**未导出**，`debug` 只给 `featureCount`——任何"独立脚本直接调用特征提取"的方案必须先做一处导出改动。
