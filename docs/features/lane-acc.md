# LaneAcc（分轨准确度）与 ScoreV1 实时转换

> 目标：AI / 维护者。适用桌面壳 + osu!lazer + 原生 mania 4K–18K；**不计划支持 stable**。

## 1. 功能

卡片主体新增 `contentBar = "LaneAcc"`（也可作为壳内 Tab 或 Combined 可选模块）：

- 顶部：**ScoreV1 转换 Acc**（Stable Classic 窗口；开局 100.00%）+ 单个等效 `OD` 胶囊；不显示倍速、MOD 或原始 OD 箭头，避免窄卡片截断。
- 手牌行：**L / R**（奇数键的中列由 `laneAccSpaceMode` 决定：`independent` 时独立显示为 **M** 手牌块；也可选并入左/右手）；无 Miss 时不显示 Miss 字样。
- 4K–18K 分轨舞台：每轨一条彩条，颜色按**主题色 / 副主题色交替**；条内显示两位小数 Acc，Miss 用右下角红色徽标（11px，窄轨自适应）。
  条高按**动态上下限**放大（上限 = 本局最好轨道，下限 = 本局最差轨道 −1），所以 99.5–100 这种窄区间也能一眼分开。
- 轨道条宽度自适应：≥34px 两位小数、≥26px 一位、≥17px 整数、更窄只留色条 + Miss 圆点。
- 悬浮 / 聚焦 / 点击显示该轨 MAX/300/200/100/50/Miss 明细；弹窗贴在该轨上方、4 秒自动消失，按轨道居中并钳制在卡片内。ScoreV1、L、R 的悬浮明细会在实时渲染时自动刷新，不需要移开再 hover。
- 顶部数字使用固定宽度（ScoreV1 / OD / L/R 手牌），数值变化不推动 OD 位置；所有可变动数字（ScoreV1、L/R、轨道 Acc、Miss）带平滑数值过渡，轨道填充使用 transform 缩放避免布局抖动。
- 单模块卡片布局：Tab 条收成自然高度，舞台固定 190px，轨道条落到卡片底部；4K–18K 自适应，<4K 显示占位提示。
- 局内实时；重试/换图/刷新自动重建；结算后保留终态；失败局分母保持**已判定音符**。
- 切图/跨图包：谱面身份变化时保留卡片结构和旧内容，叠加懒加载态（跨图包更快的扫光/下滑，图包内换难度更慢的呼吸），新谱面准备完成后轨道重新播放入场动画；不播放清空收缩动画，卡片高度保持不变。分析完成即准备谱面模型，切到 LaneAcc 页签时不需要切图重载。
- **LN 口径**：真实 lazer 事件是 `head(1..6) → tail(1..6) → 14(IgnoreHit)` 或 `head(1..6) → 15(ComboBreak) → 13(IgnoreMiss)` 等。非计分事件 `13/14/15` 不推进判定槽位游标与游戏判定计数；尾事件（`part === 1`）只记录释放偏移，长条的**唯一 ScoreV1 判定**等收到 `13/14` 或同列下一条音符头时再结算，避免 body break 信息丢失或非计分事件过早消耗槽位导致后续音符错位。
  游戏口径按**判定事件**计数：LN 的 head 和 tail 各算一个可判定事件（与结果屏一致；判定数 = tap 数 + 2×LN 数），SV1 按每个音符的头尾偏移合并。
- **MaxCombo 口径**：ScoreV1 tooltip 的 MaxCombo 按 stable V1 统计：每个非 Miss 判定 +1 combo，LN 按住期间每 100 ms 再 +1 tick combo；断持（15/掉落）、Miss 或 13 清零 combo。LN 头按下不计数。
- **ScoreV1 口径**：按 osu!stable mania V1 规则对齐：窗口为 stable 原值经 HR/EZ/DT/HT 缩放后**整数截断**；tap 按按下时刻的 `|Δ|` 立即定档（50 档只有早按侧可达，晚按被 100 窗到期闸截断为 Miss）；LN 头按下不计数，尾释放时用 `num2/num3` 合并公式给出唯一判定（含头按过深钉尾、尾镜像、掉落赎回线），尾早于 `tail−50` 窗为掉落，逾 `tail+50` 窗仍在按则以 `now` 结算；MAX 与 300 都按 300 计权。
- **OD 口径**：谱面/DA 的 OD 范围是 `[-15, 15]`；顶部显示的是 Lazer Mania `GetAdjustedDisplayDifficulty` 的等效 OD（HR/EZ 直接缩放 perfect 窗口后反解，例如 `7.8 + HR → 12.04`）。判定窗口仍按 HR/EZ 的 `1.4` / `1/1.4` 缩放，窗口公式内部把 `10-OD` 夹在 `0..10`。
- **对拍**：以 Stable 实机结果为唯一目标。10K Anti Safety Zone（3540 note / 242 LN，stable .osr 重建）
  六档计数与 Acc **完全一致**（[2145,1248,121,9,3,14] / 98.2250%）；9K Memoria（4931 note / 569 LN）
  逐档 ≤2、Acc 误差 0.02%、MaxCombo 误差 4；4K Eviternity（3356 note / 2641 LN）Acc 97.5765%（目标
  97.49%）、MaxCombo 1221（目标 1219）；6K Galgame 的 Lazer 内存事件 Acc 97.4828%（目标 97.4886%）、
  MaxCombo 1169（目标 1170），但 200/50/Miss 仍差 5/5/4——`HitEvent` 不记录 LN 的 regrab/最终释放，
  属于事件流本身的缺口（详见 §7）。mania-hub 的 ScoreV1 实现不作为目标。gameCounts 仍按结果屏
  判定事件（含 LN 尾）。

## 2. 数据链路

```
osu!lazer Player
  +<score.player_score_processor>  → ScoreProcessor
  +<score.hit_events>              → List<HitEvent>
    _items + 0x10 + stride*i       → HitEvent
      time_offset (f64) / result (i32) / hit_object (ptr)
  hit_object + column (i32)        → mania 列号
  ↓ 壳 100ms 轮询（增量；首帧/回退/重试 = reset + 全量）
WebSocket 24061 /ws → mania_hit_events / mania_replay_frames（契约 v10）
  ↓
laneAccAccumulator（纯 JS，O(1)/事件）
  ├─ 游戏口径 305 → 分轨/左右手/Miss
  └─ SV1 Classic 口径 → 顶部转换 Acc
  ↓
laneAccView（#laneacc-wrap）
```

### 2.1 偏移表 `score` 段

```json
"score": {
  "player_score_processor": 1096,
  "hit_events": 656,
  "hit_object": 0,
  "time_offset": 16,
  "result": 24,
  "column": 72,
  "stride": 64,
  "witness": "..."
}
```

- `HitEvent` 是 64 字节顺序值类型；**live 实测**（2026-10-26 18 分钟回放）：
  `TimeOffset@0x10`、`Result@0x18`、`HitObject@0x00`、`LastHitObject@0x08`
  （`Marshal.OffsetOf` 给出的是 unmanaged 布局，不能当 managed 字段偏移）。
- `HitObject + 0x48` = `ManiaHitObject.Column`。
- `player_score_processor` / `hit_events` 是**字段偏移**，由 `lazer-offsets-gen live` 在游玩/回放时复核；
  表缺 `score` 段时读侧不发帧，卡片显示占位。
- live 用 `LastHitObject == 上一事件 HitObject` 的链与列号分布自证（sample=64 / chain=63）；
  见 `temp/laneacc-evidence/CONCLUSION.md`，临时证据不提交。

### 2.2 两个生成入口

`tools/lazer-offsets-gen`（维护者 CLI）与 `gen.exe`（随包 Live Generator）**共用同一 `live` 实现**：
在游玩/回放时扫 `Player → ScoreProcessor → hitEvents`，并读 `Player.Score.Replay.Frames`，用事件样本自证结果域、偏移有限、列号 0..=17
与 `LastHitObject == 上一事件 HitObject` 的链，选命中率最高的一组布局。`extract/emit`（dump 链路）
与 `results` 段一致，**不产出 `score`**。`tools/stable-offsets-gen` 与 stable 表不在范围。

## 3. 事件文法

| result | 含义 | 累积器动作 |
|---|---|---|
| 1..6 | Note/HeadNote/TailNote | 消费该列下一个 slot；记录游戏口径计数；Head 只记录按下偏移，Tail 只记录释放偏移 |
| 13 IgnoreMiss | LN 失败/忽略 | 结算 pending LN（+断连） |
| 14 IgnoreHit | LN 保持成功 | 结算 pending LN |
| 15 ComboBreak | LN body 断持 | 标记 pending LN 断持（+断连），不判 Miss |
| 0/7..12/16 | 非 mania 计分事件 | 忽略（tick 由累积器按 100 ms 自行推算） |

真机顺序：成功 LN `head(1..6) → tail(1..6) → 14 → 14`；断持 `head → tail → 15 → 13`；
完全放手 `head(1) → tail(1) → 15 → 13`。**LN 不能在 tail 上立即终结**：Tail 事件只记录释放偏移，成功/断持都等 13/14（或同列下一条音符头）再结算。Lazer 对 held-through / 未松手的 LN 会发 `result=1, offset=0` 的 Head/Tail 事件；Head 的 `result=6, offset=0` 按“按在头上”处理，`result=1, offset=0` 视为没有按下 → Miss；Tail 的 `result=1, offset=0` 按“一直按住到 tail+50 窗”处理。Lazer 的 result 只用于判断“有没有按下/松开”，**不直接决定 SV1 档位**。

## 4. 双口径公式

窗口（`tm = speedRate / difficultyMultiplier`；HR 1.4、EZ 1/1.4、DT 1.5、HT 0.75，缩放后**整数截断**）：

```
perfect = trunc(16*tm)
great   = trunc((34+3*(10-OD))*tm)
good    = trunc((67+3*(10-OD))*tm)
ok      = trunc((97+3*(10-OD))*tm)
meh     = trunc((121+3*(10-OD))*tm)
miss    = trunc((158+3*(10-OD))*tm)
```

Tap：`|trunc(offset)|` 依次落档；晚于 100 窗的按下由 stable 的到期闸先判 Miss（因此 50 档只有早按侧可达）。

LN 合并（stable mania V1）：
- `press = head + trunc(headOffset)`；`num2` 按 press 相对 head 的位置计算：过深早按（`press < head - 50窗`）钉到 `tail-1`，晚按取 press，浅早按镜像到 `head + (head - press)`；
- `num3`：有释放且不晚于 `tail+50窗` 时取 `tail + trunc(tailOffset)`；否则按“一直按住”取 `tail + 50窗`（超过 50 窗的晚放会被到期闸截到这里）；
- `num4 = |num2 - head|`、`num5 = |num3 - tail|`、`num6 = num4 + num5`；`num3 < tail - 50窗` → Miss；
- `num4 <= 1.2*P && num6 <= 2.4*P → MAX`；`1.1*G/2.2*G → 300`；`Gd/2*Gd → 200`；`Ok/2*Ok → 100`；否则 50；
- 断持/掉落：`max(judgement, 3)`（最多压到 200，不把 200 继续往下压）。

- 游戏口径：`305*N1 + 300*N2 + 200*N3 + 100*N4 + 50*N5` / `305*gameTotal`。
- SV1 口径：`300*(N1+N2) + 200*N3 + 100*N4 + 50*N5` / `300*sv1Total`。
- 0 事件时显示 100.00%。**失败局**：`gameTotal`/`sv1Total` 恒为已判定计数；`finish()` 只冻结显示。

## 5. 设置与降级

| 条件 | 行为 |
|---|---|
| 浏览器/无壳 | 占位：`LaneAcc needs the desktop shell.` |
| 壳契约 < 8 | 占位：提示更新壳 |
| 非 lazer / 非 mania | 占位 |
| 键数 <4 或 >18 | 占位 |
| 表缺 `score` 段 | 占位 |
| 谱面原文缺失 | 占位；切图重载 |

`laneAccSpaceMode`：壳专属 `mma-shell-config.json`（`independent` 默认 = 中列不计入 / `left` / `right`），
设置页下拉、`/shell-config`、预设 shell-only 保存/加载/导入导出；浏览器导入自动剥离。
它只影响 UI 分组，不进 `settings.json`、不进缓存/重算键。

## 5b. 实时判定与回放进度

LaneAcc 不再在进入 play/回放时把全部判定一次性应用。`view.js` 每 200ms 读取
`state.songTimeMs`（tosu/native 的 `beatmap.time.live`，已按 speedRate 归一为谱面时间），
只把 `noteStart/End + offset` 不晚于当前播放位置的事件喂给 accumulator；原始帧也只取
`time <= 当前播放位置` 的子集。效果：

- 开局 0 个判定 → 显示 100.00%；
- 游玩/回放过程中判定数和 ScoreV1 逐步增加；
- 回放不再开局冻结为最终值，seek 回跳时重建并重放到新位置；
- 结算 `finishSession()` 才强制应用全量并冻结。
- 切图/换难度时按 `chart.identity + columnCount + noteCount + OD + 首尾时间` 检测谱面变化，清空 hitEvents/replayFrames 并重建累加器；旧谱面的事件不会按新 note 索引落成 Miss，卡片从 100.00% 重新开始。

## 6. 测试

- `desktop/src/osu/lazer/score.rs`：delta/reset/坏列不推进/快照。
- `temp/laneacc-accumulator.test.mjs`：stable 窗口边界、tap 早/晚 50、LN 全中/断持/超时、tick combo、
  7K 三种 space 分组、失败局分母。
- `temp/laneacc-scorev1-parity.mjs`：6K Lazer 内存事件 / 9K、10K、4K stable .osr 重建事件的 Acc、MaxCombo、
  六档计数对拍（目标来自 Stable 实机）。
- `cargo test --manifest-path desktop/Cargo.toml --lib`。
- 真机矩阵（T16）：4K/7K/10K/18K × NM/DT/HT/HR/EZ/DA × 正常/重试/倒带/刷新/结算/失败退出。


## 7. 原始 ReplayFrame（v10）

`ScoreProcessor.hitEvents` 是 Lazer 已判定事件，LN 的 regrab/最终释放会丢失；只靠它无法还原
Stable 的原始按键时间。v10 起壳同时读取 `Player.Score.Replay.Frames`：

- `Player.Score`（运行时为 `LegacyDatabasedScore`）`+0x480`
- `Score.Replay +0x10`
- `Replay.Frames`（`List<ManiaReplayFrame>`）`+0x08`
- `ManiaReplayFrame.Time +0x10`
- `ManiaReplayFrame.Actions`（`List<ManiaAction>`）`+0x18`
- `List._items +0x08` / `_size +0x10`；`ManiaAction` Key1=0 → `keys` 位掩码

下行帧 `mania_replay_frames` 与 `mania_hit_events` 并列：前者只用于 SV1，后者继续负责 Lazer 305
口径与 gameCounts。页面在 `js/app/laneAcc/rawFrames.js` 把原始帧转换成 Stable 等价的
head/tail/14/15/13 事件，再交给同一套 `accumulator.js` SV1 状态机；没有第二套判定公式。

对拍结果（原始帧 → `rawFrames.js` → accumulator）：

| 谱面 | 六档计数 | Acc | MaxCombo |
|---|---|---|---|
| 10K Anti Safety Zone | `[2145,1248,121,9,3,14]`（与 stored 完全一致） | 98.2250% | —（合成流顺序影响，见下） |
| 9K Memoria | `[3326,1438,132,13,4,18]` | 98.4993% | 1470 |
| 4K Eviternity | `[2047,1150,103,23,8,25]` | 97.5765% | 1221（目标 1219） |
| 6K Galgame | `[1647,921,99,7,17,17]` | 97.4581% | 1170 |
| 4K Rebirth the end（18 分钟，HT+MR，15127 note） | `[8980,5864,250,3,9,21]` | 99.2475% | 真机页面实测 |

最后一行是 **Lazer 正在播放的 18 分钟 4K HT+MR 回放**：壳读 `Score.Replay.Frames` 80,777 帧，
页面 LaneAcc 显示 ScoreV1 99.25%，与原始帧重算结果一致，证明 v10 链路在真实长谱面上工作。

6K 的原始帧 first-release 会多算掉落 Miss；当原始帧 Miss 比 hitEvents Miss 多 5 个以上时，
页面用 hitEvents 的 `15`/`13` 掉落集合重建成 Stable 的 50/100（没有 `14` 的早松手），
并对 `raw=200 / hit=100` 的边界 note 取 100。6K 结果与目标只差一个 50：目标
`[1647,921,99,7,16,17]` 合计 2707，而地图 2708 个物件；这个多出的 50 就是目标里漏掉的物件，
其余五档与 MaxCombo 全部一致。10K/9K/4K 不触发这条重建，保持原始帧逐档结果。
