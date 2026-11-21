# RD 列置换胶囊（RD Column Order Capsule）

> 面向 AI 的功能技术文档。相关文档：[pipeline/mod-handling.md](../pipeline/mod-handling.md) §9、[breakings/2026-11-18-full-card-rd.md](../breakings/2026-11-18-full-card-rd.md)、[features/lane-acc.md](lane-acc.md)。

## 1. 功能说明

当 osu!mania 源处于 RD（Random）且拿到有效整数 seed、列变换实际改变了轨道时，在左上角状态行的 metadata 左侧显示一个胶囊，按**原列 1..K 的顺序**列出每个原列的新列（old → new，1-based）。例如 lazer 4K、seed `55` 的映射为 `[2, 3, 0, 1]`，胶囊显示 `3412`，表示原 1→3、2→4、3→1、4→2。

以下情况不显示胶囊：MR-only（没有 RD seed）、RD 但 seed 为空/无效、非 osu! 源（Etterna/Malody）、列映射恰好是恒等（如 `1234`）、键数大于 9K、本次分析结果带 metadata 错误。stable/ppysb 的 `play.seed` 在 play 阶段可能尚未由 `Score.Seed` 写入（通常为 0）；此时页面不应用 stable RD 列变换，也不显示胶囊，避免显示 seed 0 的假映射。结算帧拿到 `resultsScreen.seed` 后再应用真实 RD 变换并显示映射。选歌态不会沿用上一局的 seed。MR-only 与 RD 无 seed 时的 MR 仍照旧参与主分析（谱面列镜像），只是不显示胶囊。RD + MR 时显示 RD→MR 复合后的最终映射；MR 只影响最终映射，不单独触发胶囊。

## 2. 数据流与单一事实源

- `js/parser/modColumnRemap.js` 的 `buildColumnMap(keyCount, { engine, seed, mirror, specialStyle })` 返回 `oldColumn -> newColumn`；`remapOsuTextColumns(osuText, map)` 按 `map[oldColumn]` 重写 `.osu` 的 x 字段。胶囊与主分析共用这份 map，不另写一套排列算法。
- `analysis.js` 的 `currentColumnTransform()` 在请求开始时快照 `requestColumnTransform`：`columnTransformFromState()` 负责 RD/MR 组合、stable/lazer 引擎选择，并先做 osu 源守卫。
- 拿到原文后，`columnMapFromOsuText(osuText, transform)` 用 `readOsuKeyCount()`、`readOsuSpecialStyle()`、`buildColumnMap()` 解析出实际映射 `analysisColumnMap`；主分析路径用 `remapOsuTextColumns(rawText, analysisColumnMap)` 生成 `analysisText`。`rawText` 始终是原图，供 LaneAcc、原文 LRU、identity 与缓存使用，避免二次置换。只有 `requestColumnTransform.seed != null` 时才把同一份 map 另存为 `rdColumnMap` 供胶囊显示；MR-only / RD 无 seed 的 MR 不会清空主分析的镜像语义。
- 缓存命中时 `rawText` 为 null，使用快照里保留的 `cached.laneAccText` 与同一请求 transform 重新得到 `rdColumnMap`；结果缓存键（`star-v16|算法|contentKey|modSignature`）和快照结构不变，不把胶囊显示状态写进缓存。
- `modSignature` 的 `seed|engine|mirror` 三段保证同图不同 RD seed、lazer/stable、MR/NM 不会互相命中缓存；胶囊只消费请求快照，绝不读分析期间可变的 `state.randomSeed` / `state.modCodes`。

## 3. DOM 与样式

- `index.html` 的 `#rd-column-order` 位于 `#section-time` 之后、`#status` 之前；默认 `hidden`。
- `js/app/core/appContext.js` 绑定 `rdColumnOrderEl`；`js/app/ui/hud.js` 的 `setRdColumnOrder(map)` 是唯一写入者。
- `styles/status.css` 让 `.rd-column-order` 与 `.section-time` 共用胶囊样式（`flex: 0 0 auto`、圆角、`white-space: nowrap`、tabular-nums），并保留 `[hidden]` 的 `display: none !important`。
- 更新顺序固定为“先 `setRdColumnOrder`，再 `setStatus`”：`setStatus` 的跑马灯按 `statusEl.clientWidth` 计算溢出距离，胶囊宽度必须先落定。`aria-label` 输出 `RD column order: 3 4 1 2` 供辅助技术读取。

## 4. 格式化规则

- 1-based：内部列号 0..K-1 加 1 后输出。
- 仅支持 1K–9K：`formatColumnMap(map)` 在 `map.length > 9` 时返回空串，调用方隐藏胶囊；>9K 的两位数会与相邻列粘连（1/10/11 无法区分），因此不做有歧义的展示。
- 恒等映射由 `buildColumnMap()` 返回 `null`，`formatColumnMap()` 也返回空串，胶囊不显示。
- 不显示 seed 本身，也不显示 `RD` 前缀；需要时由 `aria-label` 补充语义。

## 5. 生命周期与复位

| 事件 | 位置 | 行为 |
|---|---|---|
| 分析请求开始，本次 `requestColumnTransform?.seed == null` | `analysis.js fetchBeatmapFile` | `setRdColumnOrder(null)`，先撤掉上一张图的胶囊 |
| 分析成功且无 metadata 错误 | `analysis.js` 的 `if (rework)` 末尾 | `setRdColumnOrder(rdColumnMap)`，在 `setStatus` 之前 |
| 分析失败 / 素材复位 | `analysis.js resetReworkDisplay` | `setRdColumnOrder(null)` |
| 清卡（Malody 回 `other`） | `analysis.js clearSourceCard` / `invokeCardClear` | `setRdColumnOrder(null)` |
| 离开 play/result 回到选歌，seed 清空 | `socketHandlers.applySignalState` | `setRdColumnOrder(null)`，不等下一次分析 |
| 进入 play/replay 或结算，seed 仍有效 | 现有 freeze / recompute 流程 | 胶囊保持或由新结果覆盖；play→result 的 freeze 不重算时不闪 |
| play→result，局内 seed 缺失/为 0，结算帧有真实 seed | `socketHandlers.applySignalState` | 用 `resultsScreen.seed` 补齐 seed 并触发一次重算；LaneAcc 终态仍由 `hasResult` 分支保留 |
| 区间跑批 | `analysis.js` 的 `isSectionRun` 分支 | 使用同一 `rdColumnMap`；不额外复位胶囊 |

## 6. 旧 bug 防护

1. **null seed 不能变成 0**：只用 `columnTransformFromState()` 归一化后的 transform；`randomSeed` 为 `null`/`""` 时它返回无 RD。胶囊显示条件用 `requestColumnTransform?.seed != null`，禁止 `!seed`（`0` 是有效 seed）。`laneAcc/view.js randomSeed()` 也保留显式判空，防止 `Number(null) === 0` 回归。
2. **源串扰**：只消费 `currentColumnTransform()` 的返回值；该函数在 `activeSource !== "osu"` 时返回 null，胶囊不会因切到 Etterna/Malody 后残留的 osu mod 而出现。
3. **二次置换**：胶囊只读 `rdColumnMap`，不改写 `rawText`、`laneAccChart`，不调用 LaneAcc 的 `applyRandomColumns()`；`analysisText` 仍是唯一被列变换的文本。
4. **缓存旧快照**：不改变缓存键/结构；缓存命中从 `cached.laneAccText` 与同一请求 transform 重算 map；拿不到原文就不显示，绝不按当前 state 猜。
5. **胶囊残留/布局**：请求开始无 seed、分析失败、清卡、离开 play/result 四条路径显式隐藏；胶囊 `flex: 0 0 auto`、`white-space: nowrap`，先更新胶囊再 `setStatus`，避免跑马灯按旧宽度计算；窄卡片下 metadata 仍按 `.status` 的 `min-width: 0` 省略。

## 7. 验收要点

- lazer 4K、seed `55`：胶囊显示 `3412`，与 `buildColumnMap(4, {engine:"lazer", seed:55})` 的 `[2,3,0,1]` 一致。
- `seed = 0` 显示对应映射；`randomSeed = null` 或空串不显示。
- stable 6K/8K 的 `SpecialStyle` 首列规则与主分析一致；RD+MR 显示最终映射；MR-only 不显示。
- 缓存命中、切图、切源、清卡、分析失败、离开 play/result 后无残留胶囊。
- 9K 显示单字符 `1..9`；10K 及以上隐藏胶囊。
- 状态行窄宽度下 metadata 省略号正常，source dot 不被挤压；选区时间胶囊与 RD 胶囊可同时存在。
- 不改 Worker、契约、`settings.json`、LaneAcc 与估计算法。
