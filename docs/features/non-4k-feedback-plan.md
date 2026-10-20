# 非 4K 难度反馈：需求、缺口与实施计划

> 状态：**草案（未实现）**。本文只定义需求和实施边界，不改变运行时行为。
> 目标：4K 的现行条件和限制**完全不变**；为 6K / 7K 增加难度反馈。

## 1. 需求

1. 4K 继续走当前的 RC 条件（`lnRatio <= 0.15`、单槽位、既有 mod/rate 白名单），不因本功能改变。
2. 6K / 7K 允许**算法支持的所有谱面类型**（RC、LN、Mix、HB），不再受 4K 的 RC 限制。
3. 6K / 7K 的显示值来自实际算法：
   - 6K：Sunny（当前没有 6K 专用算法）；
   - 7K：Polaris（PR #99 已合并；7K RC/LN 都校准）；
   - 用户选择其他算法时若回退到 Sunny，以 `actualEstimatorAlgorithm` 为准。
4. 当 `estDiff` 有两个难度标签时（通常是 HB 谱面），**随机取一侧**询问用户是否准确；只问这一侧，不把两半混在一个问题里。
5. 反馈数据必须按 `key_count` / `pattern` / `ln_bucket` 分桶，不能污染 4K 的校准数据。

## 2. 当前实现（2026-10-20 合并后）

### 2.1 已经具备

- `key_count` 协议本身接受 **4 / 6 / 7**：页面 `feedbackGate`、壳 `build_payload`、后端 `ValidateStatic` 都允许这三个键数。
- 后端已存储并按 `key_count`、`pattern`、`ln_bucket`、`od_bucket` 聚合；dashboard/CSV 已有这些维度。
- 7K Polaris 已在 main；6K 仍走 Sunny，7K 的 Mixed 路由也指向 Polaris。
- 页面 `feedbackContext` 对所有键数都会写入（`rework.columnCount`），并不只对 4K。
- 本地去重、`resultsFromPlay`、replay 判定、标准 rate/mod 白名单、OD/完成率/acc 门槛都已存在。

### 2.2 硬限制（本功能要改的只有这些）

| 位置 | 限制 | 影响 |
|---|---|---|
| `js/app/feedback/feedbackSpec.js` | `RC_LN_MAX = 0.15`；`NON_RC_MODE_TAGS = ["LN","Mix","HB"]` | 6K/7K 的 LN/Mix/HB 被 `not_rc` 拒绝 |
| `feedbackGate.js` | `slotCount(context.estDiff) !== 1` 直接拒绝 | 双标签 HB 被 `multi_slot` 拒绝 |
| `feedbackGate.js` / 壳 / 后端 | `mode_tag` 必须等于 `"RC"` | 非 RC 模式无法提交 |
| 壳 `build_payload` | `page.ln_ratio <= RC_LN_MAX`、`mode_tag == "RC"` | 6K/7K 的 LN 占比无法通过 |
| 后端 `spec.go` | `ModeTag == "RC"` 且 `LnRatio <= RCLnMax` | 同上 |
| 载荷 | 没有“问的是哪一侧”的字段 | 双标签时无法说明目标标签 |

## 3. 关键缺口与待决问题

### G1. 双标签时“显示值”到底指哪一侧

现在报文只有一个 `displayed_val`，投票语义是“显示值偏高/偏低”。双标签下至少要区分：

- `estDiff = "RC 标签 || LN 标签"`；
- 页面随机选一侧提问；
- 目标值应是**被选侧**的数值（RC 段的 numeric，或 LN 标签换算出的 numeric），而不是固定的 RC numeric。

需要决定：
- 方案 A（推荐）：新增 `target_side: "RC" | "LN"`，`displayed_val` 取该侧的值；页面用现有 `rcLabelToNumeric`（或 Polaris/Sunny 的 per-side level）算出。
- 方案 B：只问 RC 侧，不随机；会丢失 LN/HB 数据，和需求不符。
- 方案 C：把两半都提交成两条反馈；会产生重复票、放大权重，不建议。

### G2. 随机必须“每局稳定”

若每次重试/每帧都重新随机，同一局可能出现两个问题甚至重复投票。建议：

- 以 `bid | rate | plugin_version` 做确定性哈希选侧（同一局恒定）；
- 或结算屏第一次进入时选一次并缓存在 `state`；
- 本地去重键加入 `side`（`bid|rate|side`），30 天只问一次；
- 只对**实际存在**的侧随机，单标签图不随机。

### G3. 6K/7K 的 mode_tag 与 lnRatio 放宽

- 4K 保持不变：`mode_tag === "RC"` 且 `lnRatio <= 0.15`。
- 6K/7K：允许 `RC / LN / Mix / HB`，`0 <= lnRatio <= 1`；`pattern` 继续作为分桶字段。
- 需要三处一致修改：页面 gate、壳 `build_payload`、后端 `ValidateStatic`；任何一处漏改都会静默拒绝。
- 需要确认 `mode_tag` 是页面报告字段，壳只做闭集/范围校验，不重新推断。

### G4. 6K/7K 的算法目标

- 6K 只有 Sunny；7K 有 Polaris（#99 已合并）。
- 用户选择 Azusa/Roxy/Daniel/Companella 时，6K/7K 会因算法不支持而回退 Sunny；反馈必须使用 `actualEstimatorAlgorithm`，不能使用用户选择值。
- 如果 6K 以后接入新算法，按同一个规则加入 `ALLOWED_ACTUAL_ALGORITHMS`，不在这里写死。

### G5. 载荷/后端兼容

- 新增 `target_side`（可选字段）；旧客户端不传时：
  - 4K 仍按 RC 处理；
  - 6K/7K 没有 side 的旧请求直接拒绝（避免把双标签混成未知目标）。
- 后端 dashboard 增加按 side 的视图；已有 `pattern`/`ln_bucket` 可以继续用。
- 是否需要 bump 反馈协议版本（`WeightVersion`/`SignVersion`）需要评估：新增字段本身不改签名串则不必，但改变 `mode_tag`/`ln_ratio` 的校验语义要记录在文档。
- 反馈仍只接受标准 rate（0.75/1.0/1.5）和现有 mod 白名单；自定义倍速不在本次范围。

### G6. 数据量与校准

- 6K/7K 样本远少于 4K；需要先观察投票量，再决定是否进入校准。
- 绝不把 6K/7K 的投票混入 4K 的 interval/weight 校准；按 key/pattern 分桶，必要时先只做“展示值准确性”统计，不做自动调参。
- 双标签随机侧会让 RC/LN 的样本量减半；需要记录 side，便于后续补量。

## 4. 实施阶段（建议）

### Phase 0 — 本草案（本 PR）

- 固定需求、缺口、字段和验收口径。
- 不写代码，不改 gate。

### Phase 1 — 数据契约

- 引入 `target_side`（`RC`/`LN`）和“被选侧的显示值”。
- 定义 4K 旧行为不变；6K/7K 必填 side。
- 更新 `docs/features/telemetry.md` §7、页面 `feedbackSpec`/`feedbackMath`、壳 `PageFeedback`/`build_payload`、后端 `spec.go` 与存储字段。
- 单元测试：4K 无 side 仍通过；6K/7K 无 side 拒绝；side 越界拒绝；旧 4K 行为 byte-identical。

### Phase 2 — 页面 gate 与随机侧

- 按 key 分 scope：4K 保持 `RC_LN_MAX`；6K/7K 放开 LN/Mix/HB。
- 双标签：确定性选侧，单标签不随机；`slotCount` 不再作为 6/7K 的拒绝条件。
- 去重键加 side；确保同一局只问一次。
- 页面测试：4K 行为不变；6K/7K RC/LN/HB 各一例；双标签随机稳定。

### Phase 3 — 壳与后端

- 壳校验 side、mode、lnRatio 的 key-scoped 规则；签名串加入 side（若需要）。
- 后端 `ValidateStatic` 按 key 分类；旧字段兼容；dashboard 增加 side 维度。
- 端到端：6K/7K 各提交一票；4K 回归。

### Phase 4 — 观察与校准

- 先收集 2–4 周数据，检查每 key/pattern/side 的票量、重复率、拒绝原因。
- 达到样本门槛后再讨论是否调整 interval 表；不在 Phase 1–3 自动改算法。

## 5. 验收标准

- 4K：现有全部 gate 行为、权重、去重、签名、后端校验逐项不变。
- 6K/7K：RC、LN、HB 各能弹条；双标签只问一侧且每局稳定；4K 不会因为“双标签”或“非 RC”被误放开。
- 载荷/后端：`target_side` 可追溯；dashboard 能按 key/pattern/side 聚合。
- 失败时：缺 side、非 mania、回放、旧 rate/mod、缺失 hits 都按预期静默/拒绝，不写入错误客观字段。

## 6. 本轮不做的内容

- 不改任何 gate、常量、载荷或后端。
- 不对 6K/7K 做算法调参。
- 不放开自定义倍速反馈。
- 不改变 4K 的条件和限制。

## 7. 需要 owner 确认的问题

1. **G1**：双标签时 `displayed_val` 取被选侧数值，是否同意？（推荐）
2. **G2**：随机侧用确定性哈希还是进入结算屏时随机一次？
3. **G3**：6K/7K 是否允许 `LN / Mix / HB` 全部三种，还是先只放 LN？
4. **G5**：新增 `target_side` 是否接受一次协议字段变更（旧客户端 4K 兼容）？
5. **G6**：6K/7K 的投票是否只做观察，暂不进入自动校准？
