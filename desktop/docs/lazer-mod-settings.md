# lazer 模组自定义设置（自定义倍速 / DA 自定义 OD）读取预研

> 状态：**预研（2026-10-19）**。本文记录的链路尚未接入载荷；现有行为仍是
> `SelectedMods` + 位移启发式读取。探针只写日志，不改变任何发布字段。
>
> 目标：把 lazer 的**自定义倍速**（DT/HT 的 `speed_change`）与 **DA 自定义 OD**
> （`overall_difficulty`）从"猜 bindable 位移"换成有名字、可校验的来源。

---

## 1. 现状与问题

`desktop/src/osu/lazer/frame.rs` 的 `read_lazer_mods` 从 `SelectedMods` 链
（`Bindable<IReadOnlyList<Mod>>`）逐元素解析运行时类型名得到 acronym，然后在
mod 对象的候选位移（`0x10..0x38`）上找 bindable，按 `+0x20` 读 f64：

- **倍速可用**：`ModDoubleTime` / `ModNightcore` 等的 `SpeedChange` 是
  `BindableNumber<double>`，值就在 `+0x20`。自定义倍速能读出来，但仍是"扫描候选
  位移 + 值域过滤"，不是按字段名读。
- **DA 自定义 OD 不可靠**：`ModDifficultyAdjust.OverallDifficulty` 是
  `DifficultyBindable : Bindable<float?>`（**可空 float**），不是 double。
  `+0x20` 处是 `Nullable<float>` 的 `hasValue` 字节，真正的值在 `+0x24`；
  按 f64 读会得到无意义的值或 0。更麻烦的是基类字段顺序里 `DrainRate` 在
  `OverallDifficulty` 之前，扫描会先命中 HP，无法区分两者。

结论：DA 的 OD 需要换来源，而不是继续调启发式。

## 2. 权威来源：`ScoreInfo.ModsJson`

lazer 在**开局时**就把当前 mod 配置序列化进成绩对象：

```text
Player.load
  → Score = CreateScore(playableBeatmap)
  → Score.ScoreInfo.Mods = gameplayMods        // Player.cs
  → ScoreInfo.Mods setter
  → updateModsJson()
  → ModsJson = JsonConvert.SerializeObject(APIMods)
```

`APIMod(Mod)` 只收录**非默认**设置：

```csharp
foreach (var (_, property) in mod.GetSettingsSourceProperties())
{
    var bindable = (IBindable)property.GetValue(mod)!;
    if (!bindable.IsDefault)
        Settings.Add(property.Name.ToSnakeCase(), bindable.GetUnderlyingSettingValue());
}
```

所以：

- `DT` 用默认 1.5x → `ModsJson` 里没有 `settings`；
- `DT` 改成 1.2x → `{"acronym":"DT","settings":{"speed_change":1.2}}`；
- `DA` 改 OD 8.5 → `{"acronym":"DA","settings":{"overall_difficulty":8.5}}`。

这条来源的优点：

1. **名字自描述**：`speed_change` / `overall_difficulty` 直接对应页面
   `modData.js` 读的键，不需要猜字段顺序；
2. **跨版本稳**：JSON 是 lazer 自己的序列化格式，不随对象布局变化；
3. **覆盖任意 ruleset**：acronym 由游戏给出，不需要维护类型名白名单；
4. **默认值免费**：缺失键就是默认，和现有 `modData.js` 的语义一致。

## 3. 读取链路（待固化）

```text
ScreenStack 栈顶（Player / SoloPlayer）
  + <Score>k__BackingField      → osu.Game.Scoring.Score
Score
  + ScoreInfo                   → osu.Game.Scoring.ScoreInfo
ScoreInfo
  + <ModsJson>k__BackingField   → System.String（JSON）
```

三个位移都随 lazer 构建变化，因此分两步：

1. **探针**（已实现）：不写死位移，从屏幕对象开始逐个引用字段找类型名
   `osu.Game.Scoring.Score` → `osu.Game.Scoring.ScoreInfo`，再在 `ScoreInfo`
   的字符串字段里找以 `[` 开头、带 `"acronym"` 且能解析的 JSON。只读、只记日志。
2. **固化**（待办）：拿到一次真机日志后，把 Score/ScoreInfo 位移写进偏移表的
   `play` 段（与结算屏的 `results` 段同形），读侧直接按表读，不再扫描。

`results` 段已经证明 `ScoreInfo` 可以按表读（`results.rs`），`play` 段只是同一类
结构在 Player 上的入口。

## 4. 探针用法

1. 用带探针的壳（本地 `cargo build --release` 或 CI 产物）启动 osu!lazer；
2. 设置环境变量 `MMA_LAZER_MODS_PROBE=1` 后启动 `mma-shell.exe`；
3. 在选歌界面配置 `DT` 自定义倍速（例如 1.2x）+ `DA` 自定义 OD（例如 8.5）：
   此时每个 mod 对象会打印一次候选字段；进图后再看 `ScoreInfo.ModsJson` 行；
4. 查看壳 exe 旁的 `logs/mma-shell-YYYYMMDD.log`，搜 `lazer mods probe`。

成功时一行：

```text
[osu] lazer mods probe: screen=0x... score_offset=0x... score=0x... scoreinfo_offset=0x... scoreinfo=0x... mods=[DT:1.20x,DA:OD8.5] mods_json=[{"acronym":"DT","settings":{"speed_change":1.2}},...]
```

失败时：

```text
[osu] lazer mods probe: no ScoreInfo.ModsJson under screen=0x... (will retry)
```

探针每个屏幕对象每秒最多扫一次；成功后不再重复。日志里只有 mod 配置，没有用户名/
成绩/谱面标识。

同一次探针还会对 `SelectedMods` 里的每个 mod 对象打印一次候选字段（`+0x10..+0x40`
的引用、bindable 运行时类型、`u8@+0x20`、`f32@+0x24`、`f64@+0x20`）：

```text
[osu] lazer mod probe: type=osu.Game.Rulesets.Mania.Mods.ManiaModDifficultyAdjust acronym=DA fields: +0x10 -> 0x... type=osu.Game.Rulesets.Mods.DifficultyBindable u8@0x20=1 f32@0x24=8.5 f64@0x20=1.6e-...; ...
```

它用来核对两件事：`SpeedChange`（`BindableNumber<double>`）的 `f64@+0x20` 是不是
自定义倍速；`DifficultyBindable`（`Bindable<float?>`）的 `u8@+0x20`/`f32@+0x24`
是不是"有值 + OD"，以及 `DrainRate` 与 `OverallDifficulty` 谁在前。

## 5. 下一步

- [ ] 用一次真机日志确认 `Score` / `ScoreInfo` 的可达性与位移；
- [ ] 在偏移表增加 `play` 段（`score_offset`、`score_info_offset`、`mods_json_offset`），
      校验规则与 `results` 段一致（范围、8 字节对齐、缺段降级）；
- [ ] `read_lazer_mods` 优先读 `ModsJson`，`SelectedMods` 启发式只做兜底；
- [ ] 反馈签名层的 `lazer_mods_and_rate` 改用同一份 mods（避免展示与签名口径分叉）；
- [ ] 为 `ModsJson` 解析补一条跨版本快照测试（真实 JSON 样本进 `tests-local/`）。

## 6. 红线

- 探针与后续读取都**只读**：不写 `WriteProcessMemory`、不改游戏内存。
- 任何一步读不到就降级/不发布，**绝不**用 0、默认值或上一帧占位。
- `ModsJson` 为空串（无 mod）或解析失败时，不能把空列表当成"确定无 mod"发出去——
  仍按现有 `SelectedMods` 路径兜底。
- 偏移表新增段必须是纯数据 + 通过 `validation.rs` 的范围/对齐校验；不把扫描逻辑
  藏进数据表。
