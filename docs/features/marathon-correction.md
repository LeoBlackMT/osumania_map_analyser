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

## 3. 参数（v2.0.2 校准后，对数饱和版）

| 参数 | 值 | 说明 |
|---|---|---|
| `MARATHON_DURATION_THRESHOLD_S` | 300 | drain 时长门槛（秒，未按速率缩放） |
| `MARATHON_CORRECTION_SCALE` | **0.40** | 对数域系数（course 样本排序约束校准，见 §8） |
| `MARATHON_CORRECTION_CAP` | **0.50** | 修正量封顶（排序约束校准） |
| `MARATHON_BALANCE_RATIO` | 0.45 | 技能均衡阈值：`max(4 技能)/总和 < 0.45` 才触发 |
| `MARATHON_TAPER_LO / HI` | 10 / 16 | numeric taper 区间 |

修正式（对数饱和，次线性）：

```
excessMin = (durationS − 300) / 60          # 超出分钟数
raw       = min(CAP, SCALE × ln(1 + excessMin))
corr      = raw × taper(numeric)
```

**为何对数饱和**：线性惩罚（perMin × excessMin）随时长增长，会导致**修正差超过相邻段位课程的估算差而翻转相对顺序**（验收实测：REFORM 2nd Pack 8th 修正后 9.28 > 9th 9.25，base 本为 9.78 < 9.90）。对数在长端收敛修正差（8th/9th 修正差从 0.157 降到 ~0.07），让排序重新由估算器 base 差支配；全 pack 内相邻段位对修正后零新增倒挂（§8）。

技能聚合（ett values 键名首字母大写，见 `js/ett/calc.js` `OFFICIAL_OUTPUT_ORDER`）：

```
jack    = max(JackSpeed, Chordjack)
stream  = max(Stream, Jumpstream)
stamina = 0.7 * Stamina + 0.3 * Handstream
tech    = Technical
```

## 4. 接入点（估算器内嵌，无管线派生段）

**架构原则（用户决策）**：修正本身作为**估算器本体的参数化环节**——`runRoxyEstimatorFromText` / `runAzusaEstimatorFromText` 内部在 finalNumeric 之后应用修正，`estDiff`/`star`/`numericDifficulty` 统一由修正后的值派生（输出自洽）。管线不做"输出后修正"。

- 估算器入口：`options.marathonCorrection = { durationS, ettValues }`（可选；缺省或无 MSD 时不触发，行为与旧版逐位一致——benchmark 参考 runner 不传该参数，基准保持"算法本体"口径）。
  - `durationS`：谱面 drain 时长（秒，未按速率缩放；首个到最后一个 note start 之差）；
  - `ettValues`：Ett WASM skillsets（均衡条件数据源；缺失则不修正）。
  - 修正公式与参数（§3）由 `computeMarathonCorrection`（`js/estimator/marathonCorrection.js`）提供。
- 管线（`runAnalysisPipeline`）：**按需前置 Ett**——仅当 `durationS > 300` 且 `columnCount === 4` 且算法 ∈ {Azusa, Roxy, Mixed} 时提前计算一次 Ett（供估算器注入），并**复用于段 9/10**（不重复 WASM 调用；`withEtterna` 关闭时也前置——修正是恒定语义，不依赖展示开关）。短图/非 4K/其他算法零额外开销。
- 性能约束遵守（详见 `docs/breakings/2026-08-30-marathon-correction-in-estimator.md`）：解析仍共享一次（13→1-2 不变）、无估算器重跑（修正发生在首次估算内部）、前置 Ett 仅命中长图候选且复用给既有 ett 段。
- 设置链路：**无设置项**（恒定应用，用户不可关闭）。
- 展示：无新增 UI；变更体现在难度数值与 RC 标签上。

## 5. 设计约束与安全性

- **只降不升**：修正永不提高估算。
- **无效结果保护**：`numericDifficulty === null`（Roxy scope 外 `< Alpha Low`/`> Emik Zeta high`、Azusa 错误结果）严格类型判断，不修正（`Number(null)=0` 陷阱已规避）。
- **无 MSD 不动作**：缺均衡条件信号时不修正，避免误伤单一技能高压长图（如马拉松 jack 图）。
- **高段位保护**：numeric ≥ 16 不修正（校准数据中 ≥16 的课程图本就准确，见 §8 逐行验证）。
- **排序保护（对数饱和）**：修正式次线性（`ln(1+excessMin)`），相邻段位课程的修正差被压缩到小于估算器 base 差，避免"9th 因时长更长被扣更多而低于 8th"类倒挂（验收项，逐行复现验证见 §8）。

## 6. 与估算器本体的关系

修正**属于估算器本体**：`runRoxyEstimatorFromText` / `runAzusaEstimatorFromText` 通过 `options.marathonCorrection` 参数启用的内嵌环节（缺省不触发，逐位兼容旧行为）。"估算器输出的数值化难度"即最终值（含修正），前端不做输出后修正。**基准口径**：本仓库 `results/` 的 Azusa/Roxy/Mixed 数据已按"插件等价调用"（候选行注入 `marathonCorrection`）重跑——`got` 反映修正后数值（course 行变化：Azusa 30/34、Roxy 13/34、Mixed 30/34）；benchmark 参考 runner（`runner/benchmark-runner.mjs`）缺省不传该参数，其输出为算法本体（无修正）口径——两套口径的差异即修正本身。详见 [azusa_algorithm.md](../azusa_algorithm.md) §13 与 [roxy_algorithm.md](../roxy_algorithm.md) §19。

## 7. 验证

- 冒烟：本地冒烟脚本（不入库；纯函数 + pipeline 集成，Node 直跑）。
- Benchmark（只读基准仓库；本地脚本，不入库）——
  - 样本：`samples/data.csv` 中 `pattern == "course"` 的行（34 张，RC 天然；用户指定 course 分类 + RC 筛选）；
  - 口径与 benchmark runner 对齐（got 优先 `numericDifficulty`；estDiff 含 `<`/`>` 的行不参与统计）；
  - baseline vs corrected 对比 MAE/RMSE/Bias/Exact/Close 与修正触发行数；
  - 排序检查：同一 pack 内按 expected 升序的相邻有效对，修正后 got 必须非降且不得新增倒挂（pack 按 name 的 ` [` 前缀分组，跨 pack 体系不可比不计入）。
- 倒挂复现（本地脚本，不入库）：REFORM 2nd 8th/9th 专项——base 9.78/9.90 → 修复后 9.28/9.32，顺序保持。

## 8. 参数校准记录（用户授权使用 course 样本校准，2026-08）

### 8.1 第一轮（线性修正，未通过验收）

网格：perMin ∈ {0.08~0.24} × cap ∈ {0.65~1.50}（threshold=300、balance=0.45、taper=10~16 固定）。合并口径（Roxy 有效 14 行 + Azusa 有效 34 行 = 48 行）：

| perMin | cap | 合并 MAE | Bias | Exact% | Close% |
|---|---|---|---|---|---|
| 0.20 | 0.65 | 0.3225 | −0.053 | 45.8 | 79.2 |
| 0.08（Dan-Overlay SR 单位移植） | 0.65 | 0.3845 | −0.258 | 29.2 | 79.2 |

结论：0.08 是 Dan-Overlay 的 **SR 单位**值（其 SR≈DP×1.75），搬 numeric 后偏小 ~2.5 倍；选 0.20/0.65 时 MAE 0.3225。**验收失败**：REFORM 2nd 8th（449s）修正后 9.28 **>** 9th（496s）9.25——线性惩罚差（47s×0.20/60≈0.157）超过两图 base 差（0.12），相邻段位顺序被翻转。单图公式（段位内位置保护）对本例无效（两图同处 9.x 段位）。

### 8.2 第二轮（对数饱和，发布版）

改用次线性修正式 `scale × ln(1 + excessMin)`。网格：scale ∈ {0.30, 0.35, 0.40, 0.45} × cap ∈ {0.50, 0.65, 0.90}，**新增排序约束**（pack 内相邻 expected 对 got 非降；base 基线违规数 = 0）：

| scale | cap | 合并 MAE | Bias | Exact% | Close% | newViolations |
|---|---|---|---|---|---|---|
| 0.45 | 0.50 | 0.3159 | −0.111 | 43.8 | 83.3 | 0 |
| 0.40 | 0.50 | 0.3167 | −0.112 | 43.8 | 83.3 | 0 |
| 0.35 | 0.65 | 0.3290 | −0.110 | 43.8 | 79.2 | 0 |
| 0.30 | 0.65 | 0.3347 | −0.154 | 37.5 | 83.3 | 0 |

- 全部组合零新增倒挂；**选定 scale=0.40, cap=0.50**（与最优差 0.0008，留余量防 34 张过拟合）。
- 修复后 REFORM 2nd 8th = 9.28 < 9th = 9.32（base 差 0.12 恢复主导）✅；8th/9th 修正差 0.08（对数收敛）。
- 发布数字（course 子集）：Roxy MAE 0.4036 → 0.2250（−44%，Exact 21.4%→50.0%，Close 92.9%）；Azusa MAE 0.4782 → 0.3544（−26%，Exact 26.5%→41.2%，Close 79.4%，Bias −0.424→−0.085）。

## 9. 已知限制

- 仅 4K RC（Roxy/Azusa 的既有 scope）；Sunny/Daniel/Companella 不适用（用户指定 Roxy+Azusa）。
- 分析管线中 `withEtterna` 关闭（contentBar 不含 Etterna）时修正不生效——依赖 MSD 均衡条件的固有代价。
- 校准基于 34 张 course 样本（有效合并 48 行），scale=0.40 已留余量；若后续更大的语料显示需要调整，须重新走留出法校验（用户决策）。
- 对数饱和修正的次线性特性：超长图（>15 分钟）的修正量增速趋缓（`ln` 饱和），长端虚高可能修正不足——当前语料（最长 901s）无此样本，为已知边界。