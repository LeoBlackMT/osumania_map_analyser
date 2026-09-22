# Malody 4.3.7 判定档 → 等效 osu!mania OD（PC 表）

> 面向人类与 AI 的说明文档。表值由 `tools/malody4-od-check/verify.py` 从窗口值复算校验，代码表在 `ManiaMapAnalyser by Leo_Black/js/parser/judgeOdTable.js`。数据源本身见 [malody4-source.md](malody4-source.md)；转换器见 [../pipeline/converters.md](../pipeline/converters.md)；缓存键语义见 [../pipeline/result-cache.md](../pipeline/result-cache.md)。

## 1. 为什么不能直接照搬判定档

Malody 的判定档（`A`~`E`）与 osu!mania 的 OD 是**两套互不相同的判定体系**，档位数量、窗口数值、准确率权重、最外档的含义都不一样，所以"判定 A 就等于某个 OD"这种对应关系并不存在，必须**先定义什么叫"相等"**。

本表采用的相等定义是：**96% 准确率下的等精度（σ\*）等价**——把玩家每次击键的时间误差建模为零均值正态分布，分别求出 Malody 侧与 osu!mania 侧"期望准确率恰好 96%"所对应的标准差 σ\*，令两侧 σ\* 相等，再解出对应的 OD。这样"难度"的定义落在**玩家实际能打出的准确率**上，而不是"窗口数值看起来像"。

- Malody 侧：判定档与速率决定三档基础窗口，再按各档权重求期望准确率；96% 对应的 σ\* 即该格的目标精度。
- osu!mania 侧：OD 决定六档窗口（`DifficultyRange(od, v0, v5, v10)` 三元组，ScoreV1 权重），同样求 96% 对应的 σ\*。
- 两者相等即可解出 OD（搜索区间 `[-5, 21.3]`，二分）。

因为是"等精度"而不是"窗口数值对齐"，表里会出现负 OD（`A+SLOW = -4.56`）和超过 10 的 OD（`E+RUSH = 16.42`）——这两端都超出 osu! 的常规 0~10 区间，但**在 96% 这个口径下就是等价的**。

## 2. PC 端窗口值及其来源

窗口值来自 Malody 4.3.7 **PC 端** `malody.exe` 的反汇编材料（用户自行解包/反汇编：PE32 MSVC RTTI → `_TypeDescriptor` → COL → vtable → 构造函数立即数 `0x535b80`；等级函数 `0x5359c0`；自动 MISS 常量在 `0x7cc370`），核对记录见本仓库计划 `.omo/plans/malody4-native-source.md` §2 #23/#24。窗口数值**只出现在 `tools/malody4-od-check/verify.py` 一个地方**，JS 侧不复制，避免两处表：

| 项 | 值 |
| --- | --- |
| C 档（标准档）三档基础窗口 | `36 / 76 / 110` ms（BEST / COOL / GOOD 外侧界） |
| A~E 档偏移 | `+20 / +10 / 0 / -8 / -16` ms |
| 自动 MISS（最外档） | 桌面端固定 `160` ms，与判定档无关 |
| 准确率权重 | `1.0 / 0.75 / 0.25 / 0.0`（BEST / COOL / GOOD / MISS） |
| 速率缩放 | 全部窗口按 `× 1/rate` 缩放；`NM = 1.0`、`DASH = 1.2`、`RUSH = 1.5`、`SLOW = 0.8` |

**稳健性**：最外档权重为 0，故它在期望准确率里被消掉——取 `160`、`150`、`min(160, w3+off)` 三种口径各解一次，20 格必须完全一致（复算脚本会断言这一点）。

## 3. PC 表（20 格等效 OD）

| Judge | NM | DASH(1.2) | RUSH(1.5) | SLOW(0.8) |
|---|---|---|---|---|
| **A** | 1.05 | 4.66 | 8.16 | -4.56 |
| **B** | 4.52 | 7.47 | 10.33 | -0.06 |
| **C** | 8.08 | 10.36 | 12.58 | 4.56 |
| **D** | 11.00 | 12.74 | 14.47 | 8.33 |
| **E** | 13.96 | 15.19 | 16.42 | 12.10 |

读法：判定档越严（A → E）OD 越高；同一判定下速率越快（SLOW → NM → DASH → RUSH）OD 越高。行内方向单调、列内方向单调，与"判定更严/更快 = 更难"的直觉一致。

## 4. σ\* 明细（每格 96% 等精度目标，单位 ms）

| Judge | NM | DASH(1.2) | RUSH(1.5) | SLOW(0.8) |
|---|---|---|---|---|
| **A** | 37.72 | 31.43 | 25.14 | 47.14 |
| **B** | 31.69 | 26.40 | 21.12 | 39.61 |
| **C** | 25.29 | 21.08 | 16.86 | 31.62 |
| **D** | 19.87 | 16.56 | 13.25 | 24.84 |
| **E** | 14.23 | 11.86 | 9.49 | 17.79 |

σ\* 越小 = 该格越严格。全表范围 **9.49 ms（E+RUSH，最严）~ 47.14 ms（A+SLOW，最宽）**，跨度约 5 倍。表中每一格的 OD 都是"osu!mania 侧达到同一 σ\* 所需的 OD"，复算残差 ≤ 0.05 ms（即精确命中，不是在搜索区间边界饱和）。

## 5. 代码接线

- `ManiaMapAnalyser by Leo_Black/js/parser/judgeOdTable.js`：表本体 `JUDGE_OD`（`Object.freeze`，不做插值、不含拟合系数）+ `computeOd(judgeLetter, speedRate)`。判定字母大小写不敏感；速率用容差匹配（`|rate - 1.2| < 1e-5` 等，非 1.2/1.5/0.8 一律视为 NM）；判定不在 `A`~`E` 或缺失 → **回落 `C/NM = 8.08`** 且 `known: false`（C 是标准档，且是旧写死值 9 的邻近档，回落不产生突兀跳变）。
- `js/parser/mcToOsuConverter.js`：可选参数 `{ overallDifficulty }`；**不传参时输出仍是 `OverallDifficulty:9`（逐字节不变）**，传参时按两位小数规范化。越界/非有限值 → `console.warn` 一次并**回落默认分支**（输出 `9`），不做静默夹断。
- `js/app/sources/externalSource.js`：**按源分流**——`malody4` 用 `computeOd(meta.judge, speedRate)` 的结果传给转换器；`malody`（Malody V）**绝不传该参数**（它的 song 帧没有 `meta.judge`，共用调用会拿到 C 档回落值 8.08，把 Malody V 的 OD 从 9 悄悄改掉）。
- 判定档字母进 `state.modSignature` 第 5 段（未知为 `"?"`）：判定决定转换出的 OD，不进键就会在换判定档时命中旧快照（旧星数配新 OD）。
- `js/rework/sunnyAlgorithm.js` / `sunnyWindowAlgorithm.js` 在 `odFlag` 为空或无法解析时直接使用谱面 OD（`p.od`），因此新 OD 会真实进入 Sunny 族的星数；`E+RUSH` 端点的星数在回归里被断言为**有限值**（见下）。

## 6. 越界边界与上界 21.3 的理由

`OD_BOUNDS = { lo: -5, hi: 21.3 }`。

- **负端不夹断**：`.osu` 解析器不夹断 OD，Sunny 族直接用 `p.od`，负 OD 在管线里是安全的；夹断到 0 会让最宽松的两档静默失真。
- **上界 21.3 是硬边界**：Sunny 族用 `x = 0.3 * sqrt((64.5 - ceil(3 * od)) / 500)` 且**没有定义域保护**（`js/rework/sunnyAlgorithm.js`、`sunnyWindowAlgorithm.js`；Roxy 另有保护）。当 `ceil(3 * od) >= 65`（即 `od > 21.3̄`）时根号内为负 → **NaN**。所以上界写成 21.4 会让 `OD_BOUNDS` 自己落进 NaN 区、断言反而放行 NaN；写成 21.3 才是有效边界。
- 本表最大值 `16.42` 距 21.3 有余量，故 20 格全部可表达；越界值不会被夹断，而是被拒绝并回落默认（见 §5）。

## 7. 与旧行为（写死 OD 9）的差异

改动前 `.mc → .osu` 一律写 `OverallDifficulty:9`；现在 **`malody4` 源**按判定档 × 速率写入 `-4.56 ~ 16.42`（PC 表）之间的等效 OD。

- **A/B 档会明显变松**（A+NM = 1.05、A+SLOW = -4.56），**E 档整体变严**（E+NM = 13.96、E+RUSH = 16.42）；C+NM = 8.08 与旧值 9 只差 0.92，是最接近旧行为的一格。
- 影响面：星数、难度标签、PP/数值派生量都会随之变化（Sunny 族吃谱面 OD）。这是**破坏性变更**，详见 [../breakings/2026-09-21-malody4-dynamic-judge-od.md](../breakings/2026-09-21-malody4-dynamic-judge-od.md)。
- **osu!、Etterna、Malody V 这三个源完全不受影响**：osu! 走 tosu 的真实 OD；Etterna（`.sm/.ssc`）与 Malody V（`.mc`）仍走转换器默认值 9，逐字节不变。
- **Daniel 算法不受影响**：`js/rework/danielAlgorithm.js` 内部把 `od` 写死为 9（与其原始 Python 移植保持一致），所以 Daniel 的输出对判定档不敏感。

## 8. 前提与局限

- **这是 PC 表；Malody V 另需一张表**。Malody V（6.7.x）是另一个客户端、用**更宽**的一套判定窗口，两边不能共用本表——要用 Malody V 自己的窗口值另算一张，且必须等实测确定后才能落地（不得从 4.3.7 的表外推）。
- **PC 表与早前流传的移动端表在 A/B 两档不同**：早前那张表的 A/B 档用的是**移动端**窗口值，PC 端应比它**低 0.45~1.79 OD**（本轮已按 PC 窗口重算）。C/D/E 三档两平台逐格相同——三档基础窗口三元组一致，且最外档权重为 0 会在相减时抵消，所以那三档不受平台差异影响。
- **FAIR 判定模组不建模**：桌面端 `JudgeKeyFair`（`user_mods` bit `0x400`）用的是一组**更宽**的窗口（C 档 `45/85/120` vs 默认 `36/76/110`），而本表只按"判定档 × 速率"建模。因此开了 FAIR 的玩家会被按**更严**的默认组换算，**OD 偏高约 2.9**（≈ A↔C 的档距）。本轮不建模 FAIR（等效表材料亦未覆盖）；`user_mods` 只存在于壳读到的 `config.json`、**从不进入任何帧**，所以提示只能由壳侧在 attach 后检测该位并记一次 `warn`（`malody4: FAIR judge mod detected - the OD table does not model it (OD may be overestimated)`），**页面侧无数据通路、不发该提示**。"绝不静默"的落点是壳日志 + 本文档。
- **未建模的其它因素**：判定档之外的 Malody 判定相关选项（若有）与音符类型差异都不在等效口径内；本表只回答"判定档 × 速率"。
- **方法学的纪律**：误差分布模型或准确率口径一旦改动，**整张表必须重算**；**不得**为了对齐某张谱面的观感手改表值（表值是推导出来的，不是拟合出来的）。

## 9. 复算与校验

`python tools/malody4-od-check/verify.py`（纯 stdlib，单文件，入库工装）做三件事：① 从窗口值重算 20 格并与 `judgeOdTable.js` 里的表逐值比对（容差 `≤ 0.005`，正则提取、不需要 Node）；② 断言 20 格残差 `≤ 0.05 ms`；③ 断言最外档三种口径给出同一张表。`--emit-md` 会打印与本文 §3/§4 同形的表格（含 σ\*），供重算后更新本文。故意改错 JS 表里任一个值，脚本会以非 0 退出并报出差异（负向验证，证明它不是永远绿的橡皮章）。

# Malody 4.3.7 judge level → equivalent osu!mania OD (PC table)

Human- and AI-facing document. Table values are recomputed and checked by `tools/malody4-od-check/verify.py`; the code table lives in `ManiaMapAnalyser by Leo_Black/js/parser/judgeOdTable.js`. The data source itself: [malody4-source.md](malody4-source.md); converter: [../pipeline/converters.md](../pipeline/converters.md); cache-key semantics: [../pipeline/result-cache.md](../pipeline/result-cache.md).

## 1. Why the judge level cannot simply be copied across

Malody's judge levels (`A`–`E`) and osu!mania's OD are two different judgement systems: the number of tiers, the window values, the accuracy weights and the meaning of the outermost band all differ, so "judge A equals some OD" does not exist as a fact — equality has to be **defined** first.

This table defines it as **equal precision (σ\*) at 96% accuracy**: each hit's timing error is modelled as a zero-mean normal distribution, the standard deviation σ\* that yields an expected accuracy of exactly 96% is solved on each side, and the OD whose osu!mania-side σ\* matches is the equivalent OD. Difficulty is thus anchored to the accuracy a player actually achieves, not to window values that merely look similar.

Because equality is defined by precision, the table legitimately contains negative OD (`A+SLOW = -4.56`) and OD above 10 (`E+RUSH = 16.42`); both ends fall outside osu!'s usual 0–10 range yet are equivalent under this definition.

## 2. PC window values and their source

The window values come from disassembly material of the **PC** `malody.exe` 4.3.7 (user-unpacked: PE32 MSVC RTTI → `_TypeDescriptor` → COL → vtable → constructor immediates at `0x535b80`; level function `0x5359c0`; auto-MISS constant at `0x7cc370`), cross-checked in `.omo/plans/malody4-native-source.md` §2 #23/#24. The numbers live in **exactly one place**, `tools/malody4-od-check/verify.py`, and are never copied into the JS side:

| Item | Value |
| --- | --- |
| Judge C (standard) three base windows | `36 / 76 / 110` ms (BEST / COOL / GOOD outer bounds) |
| Judge offsets A–E | `+20 / +10 / 0 / -8 / -16` ms |
| Auto MISS (outermost band) | fixed `160` ms on desktop, independent of judge level |
| Accuracy weights | `1.0 / 0.75 / 0.25 / 0.0` (BEST / COOL / GOOD / MISS) |
| Rate scaling | every window is scaled by `1/rate`; `NM = 1.0`, `DASH = 1.2`, `RUSH = 1.5`, `SLOW = 0.8` |

**Robustness**: the outermost band has weight 0 and cancels out of the expected accuracy, so solving it as `160`, `150`, or `min(160, w3+off)` must reproduce the same 20 cells — the checker asserts this.

## 3. The PC table (20 equivalent-OD cells)

| Judge | NM | DASH(1.2) | RUSH(1.5) | SLOW(0.8) |
|---|---|---|---|---|
| **A** | 1.05 | 4.66 | 8.16 | -4.56 |
| **B** | 4.52 | 7.47 | 10.33 | -0.06 |
| **C** | 8.08 | 10.36 | 12.58 | 4.56 |
| **D** | 11.00 | 12.74 | 14.47 | 8.33 |
| **E** | 13.96 | 15.19 | 16.42 | 12.10 |

Reading it: a stricter judge (A → E) raises OD, and a faster rate within one judge level (SLOW → NM → DASH → RUSH) raises OD; both directions are monotonic, matching "stricter/faster means harder".

## 4. Per-cell σ\* (the 96%-accuracy target, in ms)

| Judge | NM | DASH(1.2) | RUSH(1.5) | SLOW(0.8) |
|---|---|---|---|---|
| **A** | 37.72 | 31.43 | 25.14 | 47.14 |
| **B** | 31.69 | 26.40 | 21.12 | 39.61 |
| **C** | 25.29 | 21.08 | 16.86 | 31.62 |
| **D** | 19.87 | 16.56 | 13.25 | 24.84 |
| **E** | 14.23 | 11.86 | 9.49 | 17.79 |

A smaller σ\* means a stricter cell. The table spans **9.49 ms (E+RUSH, strictest) to 47.14 ms (A+SLOW, most lenient)**, roughly a factor of five. Every OD above is the osu!mania OD that reaches the same σ\*, and the solve residual is ≤ 0.05 ms (an exact hit, not saturation at the search boundary).

## 5. Wiring in code

`judgeOdTable.js` holds the frozen table plus `computeOd(judgeLetter, speedRate)` (case-insensitive letter, tolerance-matched rate, fallback `C/NM = 8.08` with `known: false` when the judge is missing or invalid). `mcToOsuConverter.js` takes an optional `{ overallDifficulty }` and **still emits `OverallDifficulty:9` byte-identically when called with no argument**; out-of-range or non-finite values warn once and fall back to the default branch instead of being silently clamped. `externalSource.js` branches per source: `malody4` passes the computed OD, while `malody` (Malody V) never passes it (Malody V has no `meta.judge`, and sharing the call would silently move its OD from 9 to the judge-C fallback 8.08). The judge letter is the 5th segment of `state.modSignature` (unknown = `"?"`), so changing the judge level recomputes instead of hitting a stale snapshot. Sunny-family estimators use the chart OD when `odFlag` is null or unparsable, so the new OD really reaches their star ratings, and the `E+RUSH` endpoint is asserted to stay finite.

## 6. Bounds, including why the upper bound is 21.3

`OD_BOUNDS = { lo: -5, hi: 21.3 }`. The negative end is not clamped (the `.osu` parser does not clamp OD and Sunny-family estimators use `p.od` directly, so negative OD is safe; clamping would silently distort the two most lenient cells). The upper bound is hard: the Sunny family computes `0.3 * sqrt((64.5 - ceil(3 * od)) / 500)` with **no domain guard**, so `ceil(3 * od) >= 65` (i.e. `od > 21.3̄`) makes the radicand negative and the result NaN. Writing the bound as 21.4 would place `OD_BOUNDS` itself inside the NaN region and let the assertion pass NaN through; 21.3 is the valid bound. The table maximum 16.42 stays clear of it, and out-of-range values are rejected into the default branch rather than clamped.

## 7. Difference from the old behaviour (hard-coded OD 9)

Before, every `.mc → .osu` conversion wrote `OverallDifficulty:9`; now the **`malody4`** source writes the equivalent OD between `-4.56` and `16.42` (PC table). Judge A/B become markedly more lenient (A+NM = 1.05, A+SLOW = -4.56) while judge E becomes stricter throughout (E+NM = 13.96, E+RUSH = 16.42); C+NM = 8.08 is the closest cell to the old 9. Star ratings, difficulty labels and PP-derived values follow, making this a breaking change (see [../breakings/2026-09-21-malody4-dynamic-judge-od.md](../breakings/2026-09-21-malody4-dynamic-judge-od.md)). osu!, Etterna and Malody V are unaffected (osu! uses tosu's real OD; Etterna `.sm/.ssc` and Malody V `.mc` still take the converter default 9), and the Daniel algorithm is unaffected because `danielAlgorithm.js` hard-codes `od = 9` internally.

## 8. Preconditions and limitations

**This is the PC table; Malody V needs a different one.** Malody V (6.7.x) is another client with a **wider** set of judge windows, so the two platforms cannot share this table; Malody V requires its own table computed from its own window values once they are measured (never extrapolated from the 4.3.7 values). The previously circulated table used the **mobile** windows for judges A/B, which is 0.45–1.79 OD above the PC result; judges C/D/E are identical on both platforms because their three-window triplet is the same and the weight-0 outermost band cancels. **FAIR is not modelled**: desktop `JudgeKeyFair` (bit `0x400`) uses a wider set (judge C `45/85/120` vs the default `36/76/110`), so FAIR players are converted with the stricter default set and their OD is overestimated by roughly 2.9 (about the A↔C gap). `user_mods` never enters any frame, so the shell logs the warning once after attach and the page cannot warn at all; not being silent is achieved by that shell log plus this document. Changing the error-distribution model or the accuracy target requires recomputing the whole table, and values must never be hand-edited to match a particular chart.

## 9. Recompute and check

`python tools/malody4-od-check/verify.py` (single file, stdlib only) recomputes all 20 cells from the window values, compares them cell by cell with the JS table (tolerance ≤ 0.005, extracted by regex, no Node needed), asserts every residual ≤ 0.05 ms, and asserts the three outermost-band variants agree; `--emit-md` prints tables shaped exactly like §3/§4 for updating this document. Deliberately corrupting one JS value makes the script exit non-zero with the difference reported (negative verification, so it is not a rubber stamp).
