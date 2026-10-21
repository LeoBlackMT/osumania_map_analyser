# 非 4K 难度反馈（6K / 7K）

> 状态：**已实现（2026-10-21）**。4K 的条件和限制保持不变；6K/7K 支持算法覆盖的 RC/LN/Mix/HB/SV 谱面。

## 1. 范围

| 键数 | 可用算法 | 反馈范围 | 4K 旧行为 |
|---|---|---|---|
| 4K | Azusa / Roxy / Sunny / Daniel / Companella / Mixed | RC-only，`lnRatio <= 0.15`，单标签 | **完全不变** |
| 6K | Sunny | RC / LN / Mix / HB / SV，单/双标签 | 不适用 |
| 7K | Polaris（Sunny 基线） | RC / LN / Mix / HB / SV，单/双标签 | 不适用 |

非 4K 的反馈目标使用 `actualEstimatorAlgorithm`：6K 为 Sunny，7K 为 Polaris；若用户选择其他算法并回退 Sunny，以实际值为准。

## 2. 页面侧

### 2.1 范围判定

`js/app/feedback/feedbackGate.js`：

- `SUPPORTED_FEEDBACK_KEYS = [4, 6, 7]`。
- **4K**：保持原来的 `slotCount === 1`、`isRcChart()`、`numericDifficulty` 非空、`modeTag === "RC"`、`lnRatio <= 0.15`。
- **6K/7K**：
  - `modeTag` 允许 `RC / LN / Mix / HB`；
  - `0 <= lnRatio <= 1`；
  - 标签数 1 或 2；不再因 `multi_slot` 拒绝双标签；
  - 实际算法来自 `actualEstimatorAlgorithm`。

### 2.2 双标签与 target side

`feedbackMath.js` 的 `splitDifficultyLabels()` 把 `"RC 标签 || LN 标签"` 拆成两段。双标签时通常对应 HB 谱面，候选侧为 **RC 和 LN 两侧**，每侧各问一次：

1. 页面按本地去重记录计算已问过的侧；
2. 优先选**尚未问过**的一侧；
3. 如果两侧都未问过，用 `FNV-1a(beatmapId | rate | keyCount)` 的奇偶决定先问哪侧；
4. 下次同一张图/倍速的结算会选另一侧；两侧都问过后不再提示。

因此不是“同一张图永远固定一侧”。本地去重键在 6K/7K 为：

```text
mma.feedback.v1.<bid>.<rate>.<target_side>
```

4K 继续使用原有 `mma.feedback.v1.<bid>.<rate>` 键，已有去重记录不受影响。

- `target_side = "RC" | "LN"`。
- `displayed_val` 是**被选侧**的 numeric：
  - 先用 `rcLabelToNumeric(selectedLabel)` 从标签换算；
  - RC 侧边界标签（`< ...` / `> ...`）可回退到 `context.numericDifficulty`；
  - LN 侧无法换算时直接不提问（`no_numeric`），不制造数值。

### 2.3 页面载荷

`feedbackStrip.js` 提交给壳的 `PageFeedback` 新增：

```json
{"target_side": "RC", "displayed_val": 5.2, "mode_tag": "HB", "key_count": 7}
```

4K 仍然只提交 RC 单侧；壳会忽略页面的 `target_side` 并走 v1 签名。

## 3. 壳侧

`desktop/src/feedback/mod.rs`：

- `PageFeedback.target_side` 为可选字段（旧页面没有）。
- `build_payload` 的 key 校验：
  - 4K：`mode_tag == "RC"`、`ln_ratio <= 0.15`；忽略页面 side，内部留空 → v1 签名。
  - 6K/7K：`mode_tag ∈ {RC,LN,Mix,HB}`、`ln_ratio ∈ [0,1]`、`target_side ∈ {RC,LN}`。
- `FeedbackPayload.mode_tag` 改为页面报告的实际模式（4K 仍 RC），`target_side` 进入签名。
- 签名版本：
  - 4K / 旧客户端：`v1`，与既有后端逐字节兼容；
  - 6K/7K：`v2`，末尾追加 `|target_side`。
- 壳仍在 native 快照上重算 `bid/checksum/rate/mods/hits/weight`；页面只报告需要校验的标签和展示值。

## 4. 后端

`backend/internal/feedback/spec.go`：

- `Payload.target_side` 可选。
- `SignVersion = "v1"`，`SignVersionV2 = "v2"`；`CanonicalSignString` 根据 `TargetSide` 是否为空选择版本。
- `ValidateStatic`：
  - 4K：旧 RC 限制不变；`target_side` 为空或 `RC`；
  - 6K/7K：模式 `RC/LN/Mix/HB/SV`，`lnRatio 0..1`，`target_side` 必填 `RC/LN`；
  - 其余 key 拒绝 `bad_keycount`。
- 新拒绝码：`bad_mode`、`bad_side`。

`backend/internal/store/feedback.go`：

- `feedback_events.target_side TEXT NOT NULL DEFAULT 'RC'`。
- 旧数据库启动时自动 `ALTER TABLE` 补列；旧 4K 事件默认 RC。
- `InsertFeedback` 写入 side。
- `feedback_map_agg` 增加 `target_side`；聚合按 side 分组，CSV/管理页显示 side。
- `feature_daily` 继续按模式/键数/pattern/ln bucket 聚合；side 从事件表可查。

## 5. 兼容性

- **4K 老客户端**：不提交 `target_side`，壳/后端按 v1 和 RC 处理，签名与校验完全不变。
- **新 4K 客户端**：页面可能提交 `target_side: "RC"`，壳对 4K 内部清空，仍签 v1；后端兼容。
- **本地去重**：6K/7K 按 `bid + rate + side` 分侧记录；4K 继续用旧的 `bid + rate` 键。一次成功提交只标记当前侧，另一侧仍可被询问。
- **服务端去重**：HMAC token 仍为 `install_id|bid|rate`，查询时额外带 `target_side`，因此同一安装的 RC 和 LN 各可各提交一次；同侧重复仍返回 204 且不落库。
- **旧后端**：不认识 6K/7K 的 v2 载荷，会拒绝；6K/7K 上线需要先部署后端（或同时部署）。
- **旧缓存**：反馈不进入结果缓存；无影响。

## 6. 数据与观察

- 6K/7K 的事件按 `key_count`、`pattern`、`ln_bucket`、`target_side` 可分组。
- 4K 的统计口径、阈值、权重、去重和校准数据不受影响。
- 现阶段 6K/7K 只做观察：不自动修改 interval 表、不自动调权，避免小样本污染。
- 自定义倍速仍不在反馈范围（标准 rate 0.75/1.0/1.5）。

## 7. 测试与验收

- 页面 gate smoke：
  - 4K RC 单标签显示；
  - 4K 双标签拒绝（`multi_slot`）；
  - 4K LN 拒绝（`not_rc`）；
  - 6K LN 显示且 target side = LN；
  - 7K 双标签先问一侧，标记该侧后下一次问另一侧；两侧都标记后返回 `dedup`；
  - 本地去重按侧隔离，4K 旧键仍命中。
- 壳：`build_sign_string` v1/v2 测试；v2 末尾 side；反馈 mods/rate 既有测试。
- 后端：`ValidateStatic` 覆盖 4K 旧条件、6K LN、缺 side、坏 mode、4K 错 side；v2 sign string 测试；同一安装的 RC + LN 两次提交均落库，同侧重复仍去重。
- 目标：`cargo test --lib`、`go test -count=1 ./internal/feedback ./internal/store` 全绿；4K 载荷 byte-identical。

## 8. 维护

- 新增键数支持时：同步 `SUPPORTED_FEEDBACK_KEYS`、壳/后端 key 校验、`ALLOWED_MODE_TAGS_6K7K`、`ALLOWED_TARGET_SIDES` 和本文档。
- 新增算法时：只把它加入 `actualEstimatorAlgorithm` 白名单；不要改变 4K 的 RC gate。
- side 选择算法若改变，必须同步页面/壳/文档，并保证同一次结算稳定、两侧都能被覆盖；否则会重复提问或永远漏掉一侧。
- 后端新增字段必须同时更新 SQLite schema、迁移、`RefreshAll` 聚合、CSV/管理页和测试。

## 9. 不在本功能范围

- 4K 的 RC 条件、mod/rate 白名单、权重和校准数据。
- 5K/8K/10K 等无算法覆盖的键数。
- 自定义倍速反馈。
- 自动用 6K/7K 投票调参；先观察数据量再决定。
