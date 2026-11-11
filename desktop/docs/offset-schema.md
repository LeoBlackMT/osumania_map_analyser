# osu!lazer 内存偏移表结构技术规范 / Offset Table Schema Technical Specification

> 本文档用于记录 `desktop/src/osu/offsets/` 所加载的 osu!lazer 偏移表（JSON）结构、各活体探针段的字段定义与读侧约束。
> 面向开发人员与逆向维护人员，不随发行包向终端用户分发。
> 随包用户说明请参阅 [desktop/offsets/README.md](../offsets/README.md)。

---

## 1. 结构概览 (Overview)

osu!lazer 偏移表是由 `tools/lazer-offsets-gen`（或桌面壳内置的 Live Generator）采集并生成的 JSON 数据表。
表包含基础元数据、版本校验标识（`version`、`file_version`、`clr_version`、`vtable`）、多跳解引用链以及活体探针段（`results`、`mods`、`score`）。

示例骨架：

```json
{
  "version": "2026.1005.0",
  "file_version": "2026.1005.0.0",
  "clr_version": "10.0.12",
  "arch": "x64",
  "vtable": "0x7ff...",
  "chain": { ... },
  "results": { ... },
  "mods": { ... },
  "score": { ... }
}
```

---

## 2. `mods` 段（模组自定义设置）

自 2026-10-20 起，lazer 表可携带可选的 `mods` 段，用于读取 `SelectedMods` 里每个 mod 对象的自定义设置（如自定义倍速、DA 的自定义 OD/HP）：

```json
"mods": {
  "bindable_value": 64,
  "difficulty_current": 96,
  "settings": {
    "DT": {"speed_change": 16, "adjust_pitch": 24},
    "NC": {"speed_change": 16},
    "HT": {"speed_change": 16, "adjust_pitch": 24},
    "DC": {"speed_change": 16},
    "DA": {"drain_rate": 16, "extended_limits": 32, "overall_difficulty": 40}
  }
}
```

### 字段说明
- `bindable_value`：普通 `Bindable<T>.Value` 的字段位移（本构建通常为 `0x40` = 64）；
- `difficulty_current`：`DifficultyBindable` 内部 `CurrentNumber` 的位移（本构建通常为 `0x60` = 96）；
- `settings`：模组缩写（acronym）→ 设置名（setting name）→ mod 对象上的字段位移。
  设置名为固定闭集（`speed_change`、`adjust_pitch`、`drain_rate`、`overall_difficulty`、`extended_limits`），表内出现未知名称会被 `validation.rs` 拒绝。

### 探针机制与维护
这一段与 `results` 一样属于**活体探针段**：`gen.exe` / Live Generator 在选歌或游玩界面、且玩家开启了速度 mod 与 mania DA 时，会现场从进程内存采样读数并写出 `mods` 段（离线 dump 链路的 `emit` 默认不产出）。
lazer 更新后如果字段顺序发生变化，探针校验失败将不会写入该段。
可通过环境变量 `MMA_LAZER_MODS_PROBE=1` 查看壳日志核对字段位移，并在必要时更新对应版本的 JSON 表与 `manifest.json`。

详细设计与活体验证分析参见 [desktop/docs/lazer-mod-settings.md](lazer-mod-settings.md)。

---

## 3. `score` 段（LaneAcc 逐判定事件）

`score` 段记录 `Player → ScoreProcessor.hitEvents`（`List<HitEvent>`）的读侧偏移，供分轨准确度（LaneAcc）在局内与回放中逐 note 获取判定事件：

```json
"score": {
  "player_score_processor": 1096,
  "hit_events": 656,
  "hit_object": 0,
  "time_offset": 16,
  "result": 24,
  "column": 72,
  "stride": 64,
  "score_info": 8,
  "score_hash": 64
}
```

### 字段说明
- `player_score_processor`：`Player` 对象上 `ScoreProcessor` 引用的字段位移；
- `hit_events`：`ScoreProcessor` 上 `hitEvents`（`List<HitEvent>`）引用的字段位移；
- `hit_object` / `time_offset` / `result`：`HitEvent` 内部字段位移（**managed** 内存布局：实测 `HitObject@0x00`、`LastHitObject@0x08`、`TimeOffset@0x10`、`Result@0x18`；注意 `Marshal.OffsetOf` 给的是 unmanaged 互操作布局，不能作为托管堆内的字段偏移）；
- `column`：`ManiaHitObject.Column` 在 HitObject 实例内的位移（通常为 `0x48` = 72）；
- `stride`：`HitEvent` 结构体在托管数组中的步长（通常为 `0x40` = 64）；
- `player_score`：`Player` 上 `Score` 字段的位移（`LegacyDatabasedScore`，实测 `0x480`）；
- `score_info` / `score_hash`：`Score.ScoreInfo` 与 `ScoreInfo.<Hash>` 的位移（实测 `0x08` / `0x40`），
  供回放暂停读取回放文件的 64-hex store key。

### 回放哈希字段（`results.score_hash` / `score.score_info` / `score.score_hash`）

这三个键只服务「回放暂停」数据面，**全部可选**：缺任何一个，读侧返回 `None`，页面显示
`Pause Data Unavailable`，不影响判定数、LaneAcc 等既有字段。

- `results.score_hash`：`ResultsScreen.Score → ScoreInfo.<Hash>`（2026.1005 实测 `0x40`）；
- `score.score_info` / `score.score_hash`：`Player.Score → Score.ScoreInfo → ScoreInfo.<Hash>`（实测 `0x08` / `0x40`）。

**注意**：2026.1005 起 `ScoreInfo.BeatmapHash` 也是 64-hex（`.osu` 文件的 SHA-256），与
`ScoreInfo.Hash`（`.osr` 的 SHA-256）**无法只靠长度区分**。生成器在测量时用 store 目录里的
文件头判定（`.osu` 是文本 `osu file format`，`.osr` 是二进制），读侧只做 64-hex 校验，不猜。

### 探针机制与约束
这一段同样是**活体探针段**：`gen.exe` / Live Generator 在玩家游玩或观看回放时扫描 `ScoreProcessor` 字段，并通过“`LastHitObject == 上一事件 HitObject` 引用链 + 轨道列号有效范围”自证后生成；离线静态提取链路不产出此段。
若表中缺失 `score` 段，桌面壳读侧将不发射逐判定数据帧，前端 LaneAcc 卡片将优雅降级为占位展示。

---

# English Technical Reference

This document describes the schema of the `osu!lazer` memory offset tables (JSON) consumed by `desktop/src/osu/offsets/`. It is intended for developers and maintainers and is NOT distributed to end users. For user-facing guides, see [desktop/offsets/README.md](../offsets/README.md).

## 1. Schema Overview

The offset table contains metadata, validation signatures (`version`, `file_version`, `clr_version`, `vtable`), dereference pointer chains, and live-probed sections (`results`, `mods`, `score`).

## 2. The `mods` Section

Introduced to parse per-mod custom configurations in `SelectedMods` (e.g. custom speed multipliers, DA custom OD/HP):

- `bindable_value`: Field offset of `Bindable<T>.Value` (typically `0x40` = 64).
- `difficulty_current`: Offset of `CurrentNumber` inside `DifficultyBindable` (typically `0x60` = 96).
- `settings`: Map of acronym -> setting name -> field offset on the mod instance. Recognized setting names: `speed_change`, `adjust_pitch`, `drain_rate`, `overall_difficulty`, `extended_limits`.

This section is live-probed when speed mods and DA are active. If absent, the shell falls back to legacy speed heuristics and omits DA overall difficulty to avoid bogus values.

## 3. The `score` Section

Provides offsets for reading `Player -> ScoreProcessor.hitEvents` (`List<HitEvent>`) to support real-time per-lane accuracy (LaneAcc):

- `player_score_processor`: Field offset of `ScoreProcessor` on `Player`.
- `hit_events`: Field offset of `hitEvents` list on `ScoreProcessor`.
- `hit_object` / `time_offset` / `result`: Managed layout offsets within a `HitEvent` element (`HitObject@0x00`, `TimeOffset@0x10`, `Result@0x18`).
- `column`: Offset of `ManiaHitObject.Column` (typically `0x48` = 72).
- `stride`: Size of each `HitEvent` element in the backing array (typically `0x40` = 64).

Live-probed during gameplay/replay inspection. When missing, the hit-event reader is disabled and LaneAcc displays placeholder state safely.
