# 2026-10-25 — LaneAcc 分轨准确度与 ScoreV1 实时转换

> 重大破坏性更改说明（中文为主；代码为准）。分支：`feat/mania-lane-acc`。
> 项目版本保持 **2.1.0**（不 bump）；壳-页面契约 v7 → **v10**（v8 未发布，v9 命名统一，v10 增加原始 ReplayFrame）。

## ① 壳-页面契约新增逐判定事件（高）

**改了什么**：`desktop/src/frames.rs::CONTRACT_VERSION` 7 → **9**；新增下行 `mania_hit_events`
（`{reset, events:[{col,result,offset}]}`）与上行 `laneacc_sync`（v8 曾用名 `handacc_sync`）；
页面 `bridgeClient.js` 常量同步为 10，接受区间 `[3,10]`。特性统一命名为 **LaneAcc**（无 HandAcc 残留）。

**为什么**：LaneAcc 需要局内逐 note / LN 头尾 / 断持事件；结算屏六档汇总无法分轨。

**影响面**：`desktop/docs/CONTRACT.md` §16；`docs/features/desktop-shell.md` §3e；页面/壳必须
一起更新，v7 壳 + v10 页 LaneAcc 占位，v10 壳 + v7 页无 LaneAcc。

**回滚**：两边同回 v7；删除 `score.rs` / `hit_events.rs` 与页面 laneAcc 模块。

**验证**：`cargo test --lib`；WS 冒烟（T10/T16）；`mania_hit_events` reset/增量/分块。

## ② 偏移表新增 `score` 段（高）

**改了什么**：`OffsetTable.score` / `ScoreSection` / `validation.rs` 专用校验；当前
2026.1005 表的 `score` 段：`player_score_processor=1096`、`hit_events=656`、
`hit_object=0x00`、`time_offset=0x10`、`result=0x18`、`column=0x48`、`stride=0x40`（live 实测；unmanaged `Marshal.OffsetOf` 不可用）。
`lazer-offsets-gen` 与 `gen.exe` 的 `live` 共用同一发现实现；`extract/emit` 不产出。

**为什么**：`HitEvent` 布局需与安装程序集一致；早期探针把 `GameplayRate` 当 `TimeOffset`，
已用 2026-10-26 的 18 分钟 live 回放复核（`temp/laneacc-evidence/CONCLUSION.md`，临时证据不提交）；`Marshal.OffsetOf` 的 unmanaged 布局不可当 managed 偏移用。

**影响面**：旧表可加载但 `score=None`（LaneAcc 占位）；新表需重签 `manifest.json`。

**回滚**：恢复旧 `desktop/offsets/lazer/*.json` 与 `manifest.json`。

**验证**：`shipped_lazer_table_has_usable_score_section`；`self-test 35/35`；真机 `live`（T16）。

## ②b 原始 ReplayFrame（契约 v10）

**改了什么**：`ScoreSection` 新增 `player_score` / `score_replay` / `replay_frames` / `replay_time` /
`replay_actions` / `actions_items` / `actions_size`；壳下发 `mania_replay_frames`，页面读取
`Player.Score.Replay.Frames`（`Time` + `ManiaAction` 位掩码）重算 Stable ScoreV1。

**为什么**：`hitEvents` 在 LN regrab 时只记录第一次松手，无法还原稳定判定的最终释放。

**影响面**：页面/壳同升 v10；旧表无 replay 段时自动退回 hitEvents。

**回滚**：恢复 v9 表与页面，删除 `replay_frames` 数据面。

**验证**：18 分钟 4K HT+MR 回放 80,777 帧；10K 原始帧逐档与 stored 一致。

## ③ 新增 `contentBar="LaneAcc"` 与外壳入口（中）

**改了什么**：`config.js` / `settings.json` 的 `contentBar` 增加 `LaneAcc`；
`#content-bar-tabs` 第 5 枚胶囊、`#sep-laneacc` / `#laneacc-wrap`；Combined 允许模块增加
`LaneAcc`，默认四模块不变；`json.rs` 的允许列表同步。

**为什么**：当前卡片是 Tab + Combined 架构，LaneAcc 需要三种入口。

**影响面**：`content-bar-tabs.md` / `settings.md`；浏览器/OBS 只在用户显式选 LaneAcc 时显示
占位，其他路径零行为变化。

**回滚**：移除 `LaneAcc` 选项与 DOM，恢复 `contentBar` 六项。

**验证**：4K–18K 截图（T16）；`node --check`；`i18n-check`。

## ④ `laneAccSpaceMode` 壳专属设置与预设（中）

**改了什么**：`mma-shell-config.json` 新增 `laneAccSpaceMode`（`independent` 默认 / `left` /
`right`）；`json.rs` 骨架 + backfill；设置页下拉；`/shell-config`、`shellConfigClient`、
`applyLaneAccSpaceMode`；预设 `SHELL_ONLY_SNAPSHOT_KEYS` / `SHELL_FORM_KEYS` / manager / snapshot
/ io 全链路；浏览器导入自动剥离。

**为什么**：奇数键中列归属是壳专属显示设置，不能进 tosu `settings.json` 或缓存键。

**影响面**：`docs/features/presets.md` / `docs/shell-guide.md`；旧配置缺键自动补
`independent`，合法值不覆盖。

**回滚**：删除键与面板/预设字段；旧配置多出的键会被忽略。

**验证**：`backfill_shell_laneacc_space_mode_keeps_valid_values`；预设保存/加载/导入导出（T15/T16）。

## ⑤ 失败局分母口径（中）

**改了什么**：`gameTotal` / `sv1Total` 恒为已判定/已消费计数；`finish()` 只冻结显示，不再切
谱面总音符数。

**为什么**：用户明确要求失败局用“已判定音符”分母。

**影响面**：`accumulator.js` 的 `finish()`；完整打完时二者自然等于谱面总数。

**回滚**：恢复“结算屏切谱面总数”的旧行为（不推荐）。

**验证**：Node 单测 `failed-play denominator stays at judged notes`；真机失败退出（T16）。
