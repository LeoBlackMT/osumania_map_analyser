# 马拉松时长修正（Marathon Duration Correction）功能文档

> 目标：AI（LLM）。本文档描述马拉松时长修正功能的机制、参数、接入点、校准与验证方法。
> 功能对象：Roxy / Azusa 估算器的 `numericDifficulty`（RC 段位数值）。
> 灵感来源：[Dan-Overlay](https://github.com/acarranzao1a-png/Dan-Overlay)（JoseMGS3/DanielEtterna 系项目）的马拉松时长修正——其 `pipeline.py` `_merge_primary_and_mina` 对校准自 6th–10th Reform Marathon Pack 的时长虚高做修正。本插件移植其机制并将修正对象/量纲改为本项目的数值语义。

## 1. 功能说明

对**超过 5 分钟且整体难度均衡**的谱面（马拉松/课程包），Roxy 与 Azusa 的估算可能存在时长虚高：耐力成分随谱面拉长而累积，均衡型长图的高难段（peak/strain 分位）本身已隐含耐力难度，但时长进一步放大数值。

该功能**恒定应用**（无设置开关）：在估算完成后对 `numericDifficulty` 进行**只降不升**的修正，并同步重派生 `estDiff` 标签。适用条件（全部满足才触发）：

- 实际算法 ∈ {Azusa, Roxy}（Mixed 路由命中时同样生效）；
- drain 时长 > 300 秒（首个到最后一个 note start 之差，未按速率缩放）；
- Etterna MSD skillsets 可用（均衡性条件的数据源）；
- 技能均衡：`max(4 聚合技能)/总和 < 0.45`（防止"真 marathon-jack 图"被时长误伤）；
- `numericDifficulty` 为有限数值（Roxy scope 外结果 `null` 不修正）。

## 2. 机制来源与差异

- 来源：Dan-Overlay `_merge_primary_and_mina`（>300s + 技能均衡 → 超出分钟 × perMin、封顶 cap、高难度 taper、只降不升）。
- 差异（本项目）：
  - 修正对象从 SR/DP 改为 `numericDifficulty`（Roxy/Azusa 的语义输出；本项目无 DP 概念）。
  - taper 从 SR 域（6.5~7.0）改为 **numeric 域（10~16）**：numeric ≤ 10 全量修正、10~16 线性渐减至 0、≥ 16 不修正。理由：低段位（Reform 1~10 马拉松课程包）时长虚高最严重；高段位（Zeta 16+）校准稳定。
  - 均衡条件依赖 Etterna MSD skillsets（ett values）；无 MSD（ett 不可用）→ 不修正（缺信号不动作）。

## 3. 参数（v2.0.2 校准后）

| 参数 | 值 | 说明 |
|---|---|---|
| `MARATHON_DURATION_THRESHOLD_S` | 300 | drain 时长门槛（秒，未按速率缩放） |
| `MARATHON_CORRECTION_PER_MIN` | **0.20** | 每超出 1 分钟的修正量（numeric 单位；course 样本网格校准，见 §8） |
| `MARATHON_CORRECTION_CAP` | 0.65 | 修正量封顶 |
| `MARATHON_BALANCE_RATIO` | 0.45 | 技能均衡阈值：`max(4 技能)/总和 < 0.45` 才触发 |
| `MARATHON_TAPER_LO / HI` | 10 / 16 | numeric taper 区间 |

技能聚合（ett values 键名首字母大写，见 `js/ett/calc.js` `OFFICIAL_OUTPUT_ORDER`）：

```
jack    = max(JackSpeed, Chordjack)
stream  = max(Stream, Jumpstream)
stamina = 0.7 * Stamina + 0.3 * Handstream
tech    = Technical
```

## 4. 接入点

- 核心模块：`js/rework/marathonCorrection.js`（共享 DOM-free，浏览器/Node 一致）。
  - `computeMarathonCorrection({durationS, ettValues, numeric}, params?)` → 修正量（0 = 不修正）；`params` 为可选参数覆盖（默认模块常量），供校准/测试使用；
  - `applyMarathonCorrectionToRcResult(result, {durationS, ettValues})` → 新结果对象（`numericDifficulty` 修正 + `estDiff` 经 `numericToRcLabel` 重派生）。
- 管线：`js/pipeline/runAnalysisPipeline.js` §11（派生段，Ett 段之后、返回之前）：**恒定应用**——`actualEstimatorAlgorithm ∈ {Azusa, Roxy}` 且 `ettResult.values` 可用且 `noteStarts` 长度 ≥ 2 时执行。
  - `durationS` = `(max(noteStarts) - min(noteStarts)) / 1000`（未按速率缩放）。
  - star 不在此段处理：Azusa/Roxy/Mixed 的 star 已在 §4 归一化为 Sunny raw，修正不影响星数胶囊。
  - 修正软失败（异常）→ 保持原结果，不并入 `errors[]`（与附属段语义一致）。
- 设置链路：**无设置项**（恒定应用，用户不可关闭；结果恒包含修正，缓存键语义稳定、无需失效列表）。
- 展示：无新增 UI；变更体现在难度数值与 RC 标签上。

## 5. 设计约束与安全性

- **只降不升**：修正永不提高估算。
- **无效结果保护**：`numericDifficulty === null`（Roxy scope 外 `< Alpha Low`/`> Emik Zeta high`、Azusa 错误结果）严格类型判断，不修正（`Number(null)=0` 陷阱已规避）。
- **无 MSD 不动作**：缺均衡条件信号时不修正，避免误伤单一技能高压长图（如马拉松 jack 图）。
- **高段位保护**：numeric ≥ 16 不修正（校准数据中 ≥16 的课程图本就准确，见 §8 逐行验证）。

## 6. 与估算器本体的关系

修正不改变 `runRoxyEstimatorFromText` / `runAzusaEstimatorFromText` 的输出（算法本体未经修改，benchmark runner 直接调用估算器时不含此修正）；修正位于插件分析管线（`runAnalysisPipeline`）的派生段，属于**插件展示语义层**的时长校正。详见 [azusa_algorithm.md](../azusa_algorithm.md) 与 [roxy_algorithm.md](../roxy_algorithm.md) 的「马拉松时长修正」小节。

## 7. 验证

- 冒烟：`temp/smoke-marathon-correction.mjs`（纯函数 + pipeline 集成，Node 直跑）。
- Benchmark（只读基准仓库）：`temp/bench-marathon-course.mjs`——
  - 样本：`samples/data.csv` 中 `pattern == "course"` 的行（34 张，RC 天然；用户指定 course 分类 + RC 筛选）；
  - 口径与 benchmark runner 对齐（got 优先 `numericDifficulty`；estDiff 含 `<`/`>` 的行不参与统计）；
  - baseline vs corrected 对比 MAE/RMSE/Bias/Exact/Close 与修正触发行数。

## 8. 参数校准记录（用户授权使用 course 样本校准，2026-08）

网格：perMin ∈ {0.08, 0.12, 0.16, 0.20, 0.24} × cap ∈ {0.65, 0.90, 1.20, 1.50}（threshold=300、balance=0.45、taper=10~16 固定：taper 高段位保护在逐行验证中成立——≥16 的课程图本就准确，不可调低）。

合并口径（Roxy 有效 14 行 + Azusa 有效 34 行 = 48 行；course 子集）：

| perMin | cap | MAE | RMSE | Bias | Exact% | Close% |
|---|---|---|---|---|---|---|
| **0.20** | **0.65** | **0.3225** | 0.4307 | −0.053 | **45.8** | 79.2 |
| 0.24 | 0.65 | 0.3196 | 0.4261 | −0.029 | 43.8 | 79.2 |
| 0.16 | 0.65 | 0.3367 | 0.4460 | −0.106 | 43.8 | 79.2 |
| 0.08（初始移植值） | 0.65 | 0.3845 | 0.4823 | −0.258 | 29.2 | 79.2 |

结论与选择：
- 原 `0.08` 是 Dan-Overlay 的 **SR 单位**值（其 SR≈DP×1.75，即 0.08 SR ≈ 0.14 DP），直接搬到 numeric（段位单位）系统性偏小约 2.5 倍；网格校准将其修正为 **0.20/分钟**（cap 0.65 在 0.20 时优于更大 cap：部分超长图 raw 修正量 2.0+ 会被 0.65 截断，反而更准，说明大修正量会过修）。
- 最终选择 **perMin=0.20, cap=0.65**：MAE 与最优（0.24）几乎持平（差 0.003），但 Exact 最高（45.8%）且 Bias 更保守（−0.053，不会反转成低估），对 34 张校准集留有余量，降低过拟合风险。
- 校准后 course 指标（v2.0.2 发布值）：合并 MAE 0.3845 → 0.3225（−16%）；Exact 29.2% → 45.8%；Bias −0.258 → −0.053（近乎无偏）；Close 79.2% 持平。

## 9. 已知限制

- 仅 4K RC（Roxy/Azusa 的既有 scope）；Sunny/Daniel/Companella 不适用（用户指定 Roxy+Azusa）。
- 分析管线中 `withEtterna` 关闭（contentBar 不含 Etterna）时修正不生效——依赖 MSD 均衡条件的固有代价。
- 校准基于 34 张 course 样本（有效合并 48 行），perMin=0.20 已留余量；若后续更大的语料显示需要调整，须重新走留出法校验（用户决策）。