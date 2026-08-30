# 马拉松时长修正（Marathon Duration Correction）功能文档

> 目标：AI（LLM）。本文档描述马拉松时长修正功能的机制、参数、接入点与验证方法。
> 功能对象：Roxy / Azusa 估算器的 `numericDifficulty`（RC 段位数值）。

## 1. 功能说明

对**超过 5 分钟且整体难度均衡**的谱面（马拉松/课程包），Roxy 与 Azusa 的估算可能存在时长虚高：耐力成分随谱面拉长而累积，均衡型长图的高难段（peak/strain 分位）本身已隐含耐力难度，但时长进一步放大数值。

该功能在估算完成后对 `numericDifficulty` 进行**只降不升**的修正，并同步重派生 `estDiff` 标签。默认**关闭**（`enableMarathonCorrection` 设置），供用户按需开启。

## 2. 机制来源与差异

- 来源：Dan-Overlay `pipeline.py` `_merge_primary_and_mina` 的马拉松时长修正（>300s + 技能均衡 → 超出分钟 × 0.08、封顶 0.65、高难度 taper、只降不升）。
- 差异（本项目）：
  - 修正对象从 SR/DP 改为 `numericDifficulty`（Roxy/Azusa 的语义输出；本项目无 DP 概念）。
  - taper 从 SR 域（6.5~7.0）改为 **numeric 域（10~16）**：numeric ≤ 10 全量修正、10~16 线性渐减至 0、≥ 16 不修正。理由：低段位（Reform 1~10 马拉松课程包）时长虚高最严重；高段位（Zeta 16+）校准稳定。机制性区间，非 benchmark 数据拟合。
  - 均衡条件依赖 Etterna MSD skillsets（ett values）；无 MSD（ett 不可用）→ 不修正（缺信号不动作）。

## 3. 参数

| 参数 | 值 | 说明 |
|---|---|---|
| `MARATHON_DURATION_THRESHOLD_S` | 300 | drain 时长门槛（秒，未按速率缩放） |
| `MARATHON_CORRECTION_PER_MIN` | 0.08 | 每超出 1 分钟的修正量（numeric 单位） |
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
  - `computeMarathonCorrection({durationS, ettValues, numeric})` → 修正量（0 = 不修正）；
  - `applyMarathonCorrectionToRcResult(result, {durationS, ettValues})` → 新结果对象（`numericDifficulty` 修正 + `estDiff` 经 `numericToRcLabel` 重派生）。
- 管线：`js/pipeline/runAnalysisPipeline.js` §11（派生段，Ett 段之后、返回之前）：
  - 条件：`options.enableMarathonCorrection === true` 且 `actualEstimatorAlgorithm ∈ {Azusa, Roxy}`（Mixed 路由命中时同样生效）且 `ettResult.values` 可用且 `noteStarts` 长度 ≥ 2。
  - `durationS` = `(max(noteStarts) - min(noteStarts)) / 1000`（未按速率缩放）。
  - star 不在此段处理：Azusa/Roxy/Mixed 的 star 已在 §4 归一化为 Sunny raw，修正不影响星数胶囊。
  - 修正软失败（异常）→ 保持原结果，不并入 `errors[]`（与附属段语义一致）。
- 设置链路：`enableMarathonCorrection` checkbox（默认 false）→ settings.json / config.js defaults / settingsParser / appContext state / settings.js（apply、applySettingsFrom、SETTING_HANDLERS、**SETTING_RECOMPUTE_KEYS + SETTING_CACHE_KEYS**——计算影响设置，开关变更必须清缓存并重算）。
- 展示：无新增 UI；变更体现在难度数值与 RC 标签上。

## 5. 设计约束与安全性

- **默认关闭**：不改变现有用户与 benchmark 默认口径；开启后缓存键语义变化由失效列表兜底。
- **只降不升**：修正永不提高估算。
- **无效结果保护**：`numericDifficulty === null`（Roxy scope 外 `< Alpha Low`/`> Emik Zeta high`、Azusa 错误结果）严格类型判断，不修正（`Number(null)=0` 陷阱已规避）。
- **无 MSD 不动作**：缺均衡条件信号时不修正，避免误伤单一技能高压长图（如马拉松 jack 图）。

## 6. 验证

- 冒烟：`temp/smoke-marathon-correction.mjs`（纯函数 + pipeline 集成，Node 直跑）。
- Benchmark（只读）：`temp/bench-marathon-course.mjs` 在 VSRG-DanEstimation-Benchmark 上验证——
  - 样本：`samples/data.csv` 中 `pattern == "course"` 的行（34 张，RC 天然；用户指定 course 分类 + RC 筛选）；
  - 口径与 benchmark runner 对齐（got 优先 `numericDifficulty`；estDiff 含 `<`/`>` 的行不参与统计）；
  - baseline vs corrected 对比 MAE/RMSE/Bias/Exact/Close 与修正触发行数；
  - **参数固定迁移，绝不依据样本结果调参**（反过拟合规则）。

## 7. 已知限制

- 仅 4K RC（Roxy/Azusa 的既有 scope）；Sunny/Daniel/Companella 不适用（用户指定 Roxy+Azusa）。
- MANIA 模式下 `withEtterna` 关闭（contentBar 不含 Etterna）时修正不生效——依赖 MSD 均衡条件的固有代价。
- taper 区间（10~16）为机制性初值，若后续 benchmark 数据显示高段位（≥16）马拉松也存在虚高，可另行评估（需先征求用户意见再调参）。