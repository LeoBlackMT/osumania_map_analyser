# lazer 模组自定义设置（自定义倍速 / DA 的自定义 OD、HP）

> 状态：**已接入（2026-10-20）**。`SelectedMods` 的每个 mod 对象按偏移表的 `mods`
> 段读取；表里没有这一段时只保留旧的倍速启发式，**不再猜 DA 的 OD**。
> 活体验证：Lazer `2026.1005.0-lazer` 选歌界面，DT 1.2x + DA OD 15 / HP 5.5，
> 读到 `DT speed_change=1.2 / adjust_pitch=false`、`DA drain_rate=5.5 /
> overall_difficulty=15 / extended_limits=true`（与 tosu v2 的 `menu.mods.array[].settings`
> 逐值一致）。

---

## 1. 为什么不能继续用旧启发式

旧代码在 mod 对象的候选位移上找 bindable，按 `+0x20` 读 f64。实测：

- **倍速**：`BindableNumber<double>` 的 `Value` 在 **`+0x40`**，不是 `+0x20`；
- **DA**：`DrainRate` / `OverallDifficulty` 是 `DifficultyBindable : Bindable<float?>`，
  真正的自定义值在它内部 `BindableFloat CurrentNumber`（`+0x60`）的 `+0x40`，
  而且是 `float`。按 f64 读 `+0x20` 会读到 `1.37e-314` 这类垃圾值；旧代码还把它
  当成合法 OD（0 ≤ x ≤ 11）发了出去。

## 2. 活的字段布局（2026.1005.0.0 / .NET 10.0.12 / x64）

`SelectedMods` 的 mod 对象：

| mod | 字段 | 位移 | bindable 类型 | 值读取 |
|---|---|---|---|---|
| DT / HT | `SpeedChange` | `+0x10` | `BindableDouble` | `f64 @ +0x40` |
| DT / HT | `AdjustPitch` | `+0x18` | `BindableBool` | `u8 @ +0x40`（1 = on） |
| NC / DC | `SpeedChange` | `+0x10` | `BindableDouble` | `f64 @ +0x40` |
| DA | `DrainRate` | `+0x10` | `DifficultyBindable` | `+0x60` → `BindableFloat` → `f32 @ +0x40` |
| DA | `OverallDifficulty`（mania） | `+0x28` | `DifficultyBindable` | 同上 |
| DA | `ExtendedLimits` | `+0x20` | `BindableBool` | `u8 @ +0x40` |

> 非 mania 的 DA 的 `OverallDifficulty` 在 `+0x18`；本插件只读 osu!mania，表里按
> mania 的 `+0x28` 填写。

## 3. 表段与读取

偏移表新增（纯数据）：

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

- `bindable_value`：普通 `Bindable<T>.Value` 的位移（本构建 `0x40`）；
- `difficulty_current`：`DifficultyBindable` 内部 `CurrentNumber` 的位移（本构建 `0x60`）；
- `settings`：acronym → 设置名 → mod 对象上的字段位移；设置名是闭集
  （`speed_change` / `adjust_pitch` / `drain_rate` / `overall_difficulty` / `extended_limits`），
  表里出现未知名字会被 `validation.rs` 拒绝。

读侧（`frame::read_mod_settings`）：

1. 按表取 mod 对象字段 → bindable；
2. `speed_change` 读 bindable 的 f64；`adjust_pitch` / `extended_limits` 读 u8；
3. `drain_rate` / `overall_difficulty` 先读 `+0x60` 的 `BindableFloat`，再读它的 f32；
4. 任一字段读失败 ⇒ `LazerMod` 的 `complete = false`（反馈签名层 fail-closed），
   已读到的值仍用于展示。

表里没有 `mods` 段（旧表/用户自建表）时：

- DT/NC/HT/DC 仍走旧倍速启发式（可能是默认值）；
- **DA 不再发 `overall_difficulty`**，避免把垃圾值当成 OD。

## 4. 载荷形状

`packet::lazer_mods_value` 现在发与 tosu v2 同形的 `settings`：

```json
{"acronym":"DT","settings":{"speed_change":1.2,"adjust_pitch":false}}
{"acronym":"DA","settings":{"drain_rate":5.5,"overall_difficulty":15,"extended_limits":true}}
```

页面 `modData.js` 仍只消费 `speed_change` / `overall_difficulty`；多出来的键不影响
`modSignature`，但让壳与 tosu 的口径一致。

## 5. 备选来源（未采用）：`ScoreInfo.ModsJson`

lazer 在开局时会把 `APIMod[]` 序列化进 `Score.ScoreInfo.ModsJson`（只含非默认设置）。
它是跨版本最稳的来源，但需要 `Player → Score → ScoreInfo` 三个字段位移；本次没有
采用，因为 `SelectedMods` 的字段布局已在活体上验证，且选歌界面（未开局）也要能显示。
`MMA_LAZER_MODS_PROBE=1` 的探针仍可打印 `ScoreInfo.ModsJson`，留作后续对照。

## 6. 探针用法

1. 用带探针的壳启动 osu!lazer；
2. 设置环境变量 `MMA_LAZER_MODS_PROBE=1` 后启动 `mma-shell.exe`；
3. 在选歌界面配置要验证的 mod；日志里搜 `lazer mods probe`；
4. 每个 mod 对象只打印一次候选字段（`+0x10..+0x40` 的引用、bindable 类型、
   `f64@0x40`、`f32@0x40`、`u8@0x40`）。

真实日志（本次验证）：

```text
mod[0] type=osu.Game.Rulesets.Mania.Mods.ManiaModDoubleTime acronym=DT
  +0x10 ptr=... type=osu.Framework.Bindables.BindableDouble ... f64@40=1.2
mod[1] type=osu.Game.Rulesets.Mania.Mods.ManiaModDifficultyAdjust acronym=DA
  +0x10 ptr=... type=osu.Game.Rulesets.Mods.DifficultyBindable ... cur_type=osu.Framework.Bindables.BindableFloat cur_f32@40=5.5
  +0x20 ptr=... type=osu.Framework.Bindables.BindableBool ... u8@40=1
  +0x28 ptr=... type=osu.Game.Rulesets.Mods.DifficultyBindable ... cur_type=osu.Framework.Bindables.BindableFloat cur_f32@40=15.0
```

## 7. 维护与下一步

- 该布局是**构建相关**的：lazer 更新后若字段顺序变化，需要重新跑活体探针并更新
  当前版本的 `offsets/lazer/*.json` 的 `mods` 段（以及 `manifest.json` 的签名）。
  `tools/lazer-offsets-gen` 目前仍只生成 `types`/`runtime` 段；`results` 与 `mods`
  都是活体探针段，生成器不会自动产出。
- 已有 `AppData` 缓存的同版本表会优先于随包表；升级后若旧缓存没有 `mods` 段，
  读侧会回落（DA 的 OD 为空），可删除/重新生成缓存。
- 后续可把 `ScoreInfo.ModsJson` 固化进 `play` 段，覆盖 DT/HT 之外的所有 mod 设置；
  当前实现只覆盖本次要求的倍速与 DA。

## 8. 红线

- 只读：不写 `WriteProcessMemory`、不改游戏内存。
- 任一字段读不到就降级/不发布，**绝不**用 0、默认值或上一帧占位。
- 表段必须是纯数据 + 通过 `validation.rs` 的范围/8 字节对齐/闭集校验。
