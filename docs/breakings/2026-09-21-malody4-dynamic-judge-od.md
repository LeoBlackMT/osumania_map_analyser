# 2026-09-21 Malody 4 源的动态判定 OD（破坏性变更）

## 修改内容（What changed）

1. `malody4` 源的 `.mc → .osu` 转换 OD 由**写死 9** 改为**按判定档（`A`~`E`）× 速率（NM / DASH 1.2 / RUSH 1.5 / SLOW 0.8）动态取值**，值域 `-4.56 ~ 16.42`（PC 表，20 格；表值、方法学与 σ\* 明细见 [../features/malody-od.md](../features/malody-od.md)）。
2. `state.modSignature` 由 **4 段扩到 5 段**（外部源第 5 段 = 判定档字母，未知为 `"?"`）——判定决定转换出的 OD，不进键就会在换判定档时命中旧快照（旧星数配新 OD，静默错误）。
3. 判定档经壳由 `song.meta.judge` 下发（`sources.malody4.judge` 同步给出），页面用 `js/parser/judgeOdTable.js` 的 `computeOd()` 换算；`mcToOsuConverter` 新增可选 `{ overallDifficulty }` 参数。
4. 越界策略：`OD_BOUNDS = { lo: -5, hi: 21.3 }`；越界或非有限值**不夹断**，而是告警一次并回落默认分支（输出 `9`）。
5. 判定无法确定（`config.json` 缺失/解析失败）时壳**保守 withhold**：该 tick 不发 song 帧，避免把"半截配置"变成一个静默错误 OD。`FAIR` 判定模组（`user_mods` bit `0x400`）**不建模**，由壳记一次 warn。

## 修改原因（Why）

旧行为对所有 `.mc` 一律写 OD 9，但 Malody 的判定档（`A`~`E`）与速率档实打实地改变判定窗口：判定 A 比 C 宽、E 比 C 严，DASH/RUSH/SLOW 再按 `×1/rate` 缩放窗口。写死 9 等于把"最宽松档 + SLOW"和"最严档 + RUSH"算成同一张谱——难度估计在这两类玩家那里系统性偏高/偏低。等精度（96% 准确率下的 σ\*）换算把两侧难度锚在同一个"玩家实际能打出的准确率"上，比直接照搬档位数字更接近真实感受。

## 兼容性影响（Impact）

- **`malody4` 源的星数、难度标签与 PP 派生量会变**：A/B 档明显变松（A+NM = 1.05、A+SLOW = -4.56），E 档整体变严（E+NM = 13.96、E+RUSH = 16.42）；C+NM = 8.08 与旧值 9 只差 0.92，是最接近旧行为的一格。同一张谱在不同判定档下会得到不同的估计结果——这是预期语义。
- **缓存键语义变更**：外部源签名由 4 段变 5 段，旧快照的键与新键不同 → 旧条目**永不命中**，等价于**自动失效**，用户**无需手动清缓存**；缓存不会给出"旧 OD 的星数"。
- **只影响 `malody4` 源**：osu!（走 tosu 的真实 OD）、Etterna（`.sm/.ssc`）与 Malody V（`.mc`）**完全不变**——后两者仍走转换器默认值 `9` 且逐字节不变（`malody` 分支**不传** `overallDifficulty`，否则 Malody V 会拿到 C 档回落值 8.08 被悄悄改掉）。Daniel 算法不受影响（其内部把 `od` 写死为 9）。
- **越界风险已设边界**：上界 21.3 是因为 Sunny 族的 `0.3 * sqrt((64.5 - ceil(3 * od)) / 500)` 无定义域保护，`od > 21.3̄` 时根号内为负 → NaN；本表最大值 16.42 留有余量，且越界值不会被静默夹断成失真值。
- **已知局限**：PC 表只适用于 4.3.7 **PC 端**窗口；Malody V 用更宽的一套窗口，**需要另算一张表**；`FAIR` 模组不建模（OD 偏高约 2.9），提示只在壳日志里。

## 兼容策略（Compat）

- **按源分流**而非全局改默认：转换器不传参时输出仍是 `OverallDifficulty:9`（逐字节不变），因此 Etterna、Malody V 与所有既有调用点零回归；只有 `malody4` 分支显式传值。
- **判定缺失回落 C 档**（8.08）：C 是标准档，也是旧写死值 9 的邻近档，回落不会造成突兀跳变；同时发一条页面诊断（`malody4 judge unknown -> OD fallback 8.08`）并让签名第 5 段为 `"?"`。
- **不做静默夹断**：越界值告警后走默认分支而不是夹到 0~10——夹断会让最宽松/最严两端静默失真。
- **表值可复算**：`tools/malody4-od-check/verify.py` 从 PC 窗口值重新推导 20 格并与代码表逐值比对（容差 0.005、残差 ≤ 0.05 ms、最外档三种口径一致）；口径改变必须整表重算，**不得**为对齐某张谱面手改表值。
- 回滚：`git checkout --` 三个被改动的页面侧文件（`js/app/sources/externalSource.js`、`js/app/sources/shellState.js`、`js/parser/mcToOsuConverter.js`）并删除新增的 `js/parser/judgeOdTable.js`；回滚后 `malody4` 恢复写死 OD 9。

## 验证（Verification）

`computeOd` 的 20 格逐值抽查与容差/回落用例（`C` 与 `c` 同值、`1.2000001` 取 DASH、`1.3` 取 NM、`"Z"`/`null` 回落 8.08 且 `known === false`）全部通过；转换器默认路径输出与改动前**逐字节相同**（同一张合成 `.mc` 前后 diff）；传 `8.08` / `-4.56` / `16.42` 分别得到同值输出，传 `25` / `NaN` 回落 9 且有一次 warn。合成帧用例断言 `(malody4, B)` 与 `(malody4, C)` 的 identity 相同而签名不同、OD 分别为 `4.52` / `8.08`，无判定时为 `8.08` 且发一次诊断，而 `(malody, …)` 仍为 `9`。60 组（5 判定 × 4 速率 × 3 键数）合成谱回归断言 20 组表值正确、所有组合星数有限（重点验 `E+RUSH = 16.42` 与 `A+SLOW = -4.56` 两端不产生 NaN/Infinity）、且同一谱同判定下星数随 OD 单调不降。复算脚本正/负向验证均通过（故意改错一格 → 非 0 退出并报差异）。

---

# 2026-09-21 Dynamic judge-based OD for the Malody 4 source (breaking change, EN)

## What changed

1. The `.mc → .osu` OD for the `malody4` source changed from a **hard-coded 9** to a value derived from the **judge level (`A`–`E`) × rate (NM / DASH 1.2 / RUSH 1.5 / SLOW 0.8)**, ranging over `-4.56 … 16.42` (PC table, 20 cells; values, method and per-cell σ\* in [../features/malody-od.md](../features/malody-od.md)).
2. `state.modSignature` grew from **4 to 5 segments** (the external source's 5th segment is the judge letter, `"?"` when unknown): the judge level determines the converted OD, so leaving it out of the key would let a judge-level change hit a stale snapshot (old stars with a new OD).
3. The judge level travels from the shell in `song.meta.judge` (mirrored in `sources.malody4.judge`) and the page converts it with `computeOd()` from `js/parser/judgeOdTable.js`; `mcToOsuConverter` gained an optional `{ overallDifficulty }` argument.
4. Out-of-range policy: `OD_BOUNDS = { lo: -5, hi: 21.3 }`; out-of-range or non-finite values are **not clamped** — they warn once and fall back to the default branch (output `9`).
5. When the judge level cannot be determined (`config.json` missing or unparsable) the shell **withholds** the song frame for that tick, so a half-read config can never become a silently wrong OD. The `FAIR` judge mod (`user_mods` bit `0x400`) is **not modelled** and is logged once as a warning on the shell side.

## Why

The old behaviour wrote OD 9 for every `.mc`, but Malody's judge levels (`A`–`E`) and speed mods genuinely change the judgement windows: judge A is wider than C, E is stricter, and DASH/RUSH/SLOW scale the windows by `1/rate`. A fixed 9 makes "most lenient judge + SLOW" and "strictest judge + RUSH" score identically, systematically over- or underestimating difficulty for those players. Equating precision (σ\* at 96% accuracy) anchors both sides to the accuracy a player actually achieves, which is closer to felt difficulty than copying tier numbers.

## Impact

Star ratings, difficulty labels and PP-derived values for the **`malody4`** source will change: judges A/B become markedly more lenient (A+NM = 1.05, A+SLOW = -4.56) and judge E becomes stricter throughout (E+NM = 13.96, E+RUSH = 16.42), while C+NM = 8.08 is the closest cell to the old 9 — the same chart under different judge levels now yields different estimates, which is the intended semantics. The **cache-key semantics change**: the external signature going from 4 to 5 segments means old snapshots can never be hit, which is equivalent to **automatic invalidation** and needs no manual cache clearing. Only the `malody4` source is affected: osu! (tosu's real OD), Etterna (`.sm/.ssc`) and Malody V (`.mc`) are unchanged and the latter two still emit the converter default `9` byte-identically (the `malody` branch never passes `overallDifficulty`, otherwise Malody V would silently pick up the judge-C fallback 8.08), and the Daniel algorithm is unaffected because it hard-codes `od = 9` internally. The bounds are deliberate: the upper limit 21.3 exists because the Sunny family's `0.3 * sqrt((64.5 - ceil(3 * od)) / 500)` has no domain guard and goes NaN for `od > 21.3̄`, the table maximum 16.42 stays clear of it, and out-of-range values are never silently clamped into distortion. Known limits: the PC table applies to the 4.3.7 **PC** windows only (Malody V uses a wider set and needs its own table), and `FAIR` is not modelled (OD overestimated by roughly 2.9, warned in the shell log only).

## Compatibility

The change is **per-source branching** rather than a global default change: with no argument the converter still emits `OverallDifficulty:9` byte-identically, so Etterna, Malody V and every existing call site are regression-free and only the `malody4` branch passes a value. A missing judge falls back to judge C (8.08), the standard tier and the neighbour of the old 9, so there is no abrupt jump, and the page also sends a diagnostic (`malody4 judge unknown -> OD fallback 8.08`) while the signature's 5th segment stays `"?"`. Out-of-range values are not silently clamped but warn and take the default branch, because clamping would distort both extremes. The values stay recomputable: `tools/malody4-od-check/verify.py` re-derives all 20 cells from the PC window values and compares them cell by cell (tolerance 0.005, residual ≤ 0.05 ms, all three outermost-band variants agreeing); changing the accuracy model requires recomputing the whole table and values must never be hand-edited to match one chart. Rollback restores the three changed page-side files (`js/app/sources/externalSource.js`, `js/app/sources/shellState.js`, `js/parser/mcToOsuConverter.js`) and deletes the new `js/parser/judgeOdTable.js`, returning `malody4` to a fixed OD 9.

## Verification

All 20 cells were spot-checked against `computeOd`, together with the tolerance and fallback cases (`C` equals `c`, `1.2000001` takes DASH, `1.3` takes NM, `"Z"`/`null` fall back to 8.08 with `known === false`). The converter's default path is **byte-identical** to before the change (the same synthetic `.mc` diffed before and after), `8.08` / `-4.56` / `16.42` round-trip into the output, and `25` / `NaN` fall back to 9 with a single warning. Synthetic frame cases assert that `(malody4, B)` and `(malody4, C)` share an identity but differ in signature with OD `4.52` / `8.08`, that a missing judge yields `8.08` plus one diagnostic, and that `(malody, …)` still yields `9`. A 60-case regression (5 judge levels × 4 rates × 3 key counts) on synthetic charts asserts the 20 table values, finite star ratings in every combination (focused on the `E+RUSH = 16.42` and `A+SLOW = -4.56` endpoints), and monotonic non-decreasing stars as OD rises within one chart and judge level. The recompute script passes both positive and negative verification (deliberately corrupting one cell exits non-zero with the difference reported).
