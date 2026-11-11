# 2026-11-10 — 回放暂停读取（osu!lazer `.osr`）

> 重大破坏性更改说明（中文为主；代码为准）。分支：`feat/replay-pauses`。
> 项目版本保持 **3.0.0**（不 bump）；壳-页面契约 v10 → **v11**。

## ① 壳-页面契约新增回放暂停数据面（高）

**改了什么**：`desktop/src/frames.rs::CONTRACT_VERSION` 10 → **11**；新增下行
`replay_pauses`（`{hash, pauses, available, speedRate}`）与上行 `replay_pauses_sync`；
页面 `bridgeClient.js` 常量同步为 11，接受区间 `[3,11]`；24062 兼容子集新增根字段
`replayHash`（64-hex，可缺省）。

**为什么**：lazer 的 `ReplayPlayer` 播放回放时 `game.paused` 不会因历史暂停置位，回放自身
暂停反而会造成假标记；历史暂停只存在于 `.osr` 尾部的 `scoreInfo.pauses` 里。

**影响面**：`desktop/docs/CONTRACT.md` §18；`docs/features/desktop-shell.md` §3/§3f；
页面与壳必须一起更新：v11 壳 + v10 页会让页面判为契约越界进入终态；v10 壳 + v11 页可用，
回放暂停显示 `Pause Data Unavailable`。

**回滚**：两边同回 v10；删除 `replay_pauses.rs` / `osr.rs` 与页面 `replayPauses.js`。

**验证**：`cargo test --lib` 478 项；真机 76 次暂停样本 → `Pause Count: 76`；WS `replay_pauses_sync` 快照。

## ② 偏移表新增三个哈希字段（高）

**改了什么**：`ResultsSection.score_hash`、`ScoreSection.score_info` / `score_hash`；
2026.1005 两份表实测写入 `results.score_hash=0x40`、`score.score_info=0x08`、
`score.score_hash=0x40`（2026-11-10 live，ReplayPlayer + SoloResultsScreen）。
`tools/lazer-offsets-gen` 的 `live` 新增两段测量与 IL 见证（`BeatmapHash < Hash`，均为 `System.String`），
`gen.exe` 与 dev 入口共用同一实现；`extract/emit` 仍不产出这两段。

**为什么**：回放暂停需要 `ScoreInfo.<Hash>`（`.osr` 的 store key）。
2026.1005 起 `ScoreInfo.BeatmapHash` **也是 64-hex**（`.osu` 的 SHA-256），光靠长度分不开；
生成器在测量时用 store 目录的文件头判定（`.osu` 是文本 `osu file format`，`.osr` 是二进制）。

**影响面**：缺字段的旧表照常加载，读侧返回 `None` ⇒ `Pause Data Unavailable`；判定数、
LaneAcc 等既有字段不受影响。**2026.921 的偏移表已移除**（2026-11-11 cleanup）：该构建原生读取
不可用（回落 tosu），回放/历史结算固定显示 `Pause Data Unavailable`。

**回滚**：恢复 `desktop/offsets/lazer/2026.1005.*.json`；读侧代码可留（字段缺省即降级）。

**验证**：`self-test 35/35`；`shipped_lazer_table_*` 通过；两份表逐字节一致；真实 `.osr`
（76 次暂停）解出 `74712 … 583330`、`speedRate=1.0`。

## ③ 页面：回放暂停状态机与 HUD（中）

**改了什么**：`appContext.js` 新增 9 个 state 字段；新增纯函数
`js/app/net/replayPauses.js`；`socketHandlers.js` 新增 `applyReplayPauses` /
`applyReplayPausesToMarkers`，并在 `applySignalState` 写 `isReplay` / `nativeReplayHash`、
在 `updateSongTimeState` 加 `!state.isReplay` 守卫与重试钩子；`hud.js` 增加
`Pause Data Unavailable` 与 `Pause Count: 0` 两态；`settings.js` / `graph/index.js`
在开关/清标记时失效去重键。

**为什么**：回放暂停是**全景静态**数据（进入即全部显示），不能走实时检测的"渐进累加"；
且 24061 帧常先于 24062 的 `replayHash` 到达，必须先接受、后校验。

**顺带修复**：结算屏 mods 优先取该成绩自己的 `ScoreInfo.ModsJson`；历史成绩/回放不再继承上一局 play 帧的模组（修复「打完 1.3x 后清空模组，打开任意历史成绩仍显示 1.3x」）。

**影响面**：仅 lazer + 壳；浏览器模式、stable、Etterna、Malody 行为不变。

**回滚**：同 ①。

**验证**：Node 纯函数 6/6；真机 A1–A10（见 `docs/features/pause-detection.md` §7）。

## ④ 已知限制

- 2026.921 的偏移表已移除；该构建回放/历史结算固定 `Pause Data Unavailable`（原生读取回落 tosu）。
- **版本门槛**：`pauses` 数组自 lazer `2025.724.1-tachyon` / `2025.816.0-lazer` 起才有；更早的 lazer 成绩只有 `pause_count`（无时间戳），同样显示 `Pause Data Unavailable`。
- `.osr` 只解析 `pauses` 与 mods 倍速；按键帧按长度跳过。
- 刚打完、尚未落盘的回放（无 `.osr`）在结算屏也显示 `Unavailable`，落盘后重进即可。

## English summary

Contract v10 → **v11** adds the `replay_pauses` downlink and `replay_pauses_sync` uplink on
24061, plus a root `replayHash` on the 24062 tosu-compatible payload. The shell reads
`ScoreInfo.<Hash>` through the offset table (`results.score_hash`, `score.score_info`,
`score.score_hash`), locates `<storage>/files/<h>/<hh>/<hash>`, skips the replay frames and
LZMA-decompresses the trailing `scoreInfo` JSON to extract `pauses` (physical chart time).
The page divides by the mods speed rate, filters to the playable range and renders static
markers for replay playback, replay results and historical results; unavailable data shows
`Pause Data Unavailable`, an empty array shows `Pause Count: 0`. Browser mode is unchanged.

The `pauses` array only exists in native lazer scores produced by builds containing
[ppy/osu#34058](https://github.com/ppy/osu/pull/34058) (2025-07-22): the first tachyon build is
`2025.724.1-tachyon` and the first lazer-stream build is `2025.816.0-lazer`. Older lazer scores
only carry `pause_count` (no timestamps) and therefore also show `Pause Data Unavailable`.
Stable scores never contain pause data.

This change also fixes a pre-existing bug where a results screen reused the previous play's
mods even for historical scores: the score's own **non-empty** `ScoreInfo.ModsJson` now wins,
replay results never inherit the cache, and the cache is cleared as soon as the player leaves
play/results.

The 2026.921 offset tables were removed (2026-11-11 cleanup); that build falls back to tosu and
shows `Pause Data Unavailable`. Rollback: revert both sides to v10 and restore the two 2026.1005 offset tables.
