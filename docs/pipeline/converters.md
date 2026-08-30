# 谱面转换器（sm/ssc/mc → osu）

## 模块

- `js/parser/smSscToOsuConverter.js` — `.sm`/`.ssc` → `.osu` v14（mania）
- `js/parser/mcToOsuConverter.js` — `.mc`（Malody chart JSON）→ `.osu` v14（移植自
  [mc_to_osu.py](https://github.com/LeoBlackMT/nonebot_plugin_osumania_toolkit)（MIT，
  源自 Jakads/malody2osu））
- `js/parser/vendor/simfile-parser/` — simfile-parser v0.9.0（MIT）vendored 副本，
  来源与本地补丁见 `NOTICE.md`

两种转换器的输出都直接进入既有 `OsuFileParser`（管线零改动）。固定参数：
OD=9 / HP=8 / AR=5 / Mode=3。

## 转换语义（时间轴）

- **单位**：simfile-parser 的 offset 均为 measure；`1 measure = 4 beats`。
- **STOPS/DELAYS 烘焙**：事件先按 BPM 分段换算毫秒，再把位于该拍（含同拍）之前
  所有 stop 的时长累加后移（stop 时长按该拍 BPM 换算）。osu 无 stop 概念，此为
  Etterna TimingData 语义的社区惯例。
- **WARPS**：罕见，按折叠处理（忽略其时间影响，近似标注）。
- **键数**：从 note 行宽推导（vendor 已打列宽补丁，支持 6K/7K 变宽行）；
  `CircleSize = 键数`。
- **hold/roll**：`2`/`4` 头 → type 128 LN（roll 同化为 LN）；`3` 尾配对。
- **LN 尾冲突修复**：同列下一条起始前 1ms 截尾；尾 ≤ 头时压为头 + 1ms。
- **mine/lift/keysound**（`M`/`L`/`K`）：丢弃（osu 无对应概念）。
- **.mc**：`time[]` → BPM 红线；`effect[]`（scroll）→ 负斜率线（`100/|scroll|`，
  scroll=0 → `1E+308`）；`endbeat` → type 128；LN 尾同毫秒自动微调；仅 Key 模式。

## 测试

运行（Node 22+ 无类型翻转问题；插件目录无 `package.json type:module`，
Node 需按 ESM 解析 `.js`）：

```
node --experimental-detect-module tests/run-converter-tests.mjs
```

- 测试脚本与合成生成器**均不进仓库**（`tests/` 整体 gitignored，纯本地验证）；
  合成样本由 `tests/generate-fixtures.mjs` 生成（可随时重建）；
- **真实谱面样本仅本机私有**：放入 `tests/fixtures/real/`（目录 gitignored），
  存在即自动冒烟；示例来源（本机）：`D:\Games\Etterna\Songs`、
  `D:\Steam\steamapps\common\MalodyV\chart` 下的 `.sm`/`.mc`。
- 仓库内唯一 golden 载体 = 本文档 + 测试脚本内联断言，**不提交任何真实谱面文件**。

### Golden 摘要（分类断言基准）

| 样本 | 类别 | 关键断言 |
|---|---|---|
| sm-simple-4k（120bpm 2 measures） | 无 STOPS | keys=4；BPM 点 1；taps 0/500/1000/1500ms；hold 2000→4000ms；noteCount 5/1 |
| sm-stops-4k（`#STOPS:0.500=0.500`） | STOPS 烘焙 | 第二批 tap = 2000+250=2250ms（stop 0.5 拍@120bpm=250ms） |
| sm-7k（7 列变宽行） | 列宽 | keys=7、noteCount=4（vendor 列宽补丁） |
| ssc-multi（双 #NOTEDATA 块） | 多难度 | difficulty=Medium → `difficult 8`；默认 → 第一块 `expert 12`；归一化别名（hard→expert 等） |
| mc-simple（tap+hold+SV 1.5） | .mc | keys=4；BPM 点 1；SV 红线 `0,-66.666666…`；hold 1000→2000ms |
| mc-ln-collision | LN 尾冲突 | hold 尾修复为 1999ms（下一条 2000ms 前 1ms） |
| sm-rests-only（无 note） | 边界 | 抛出「未找到难度块」而非崩溃 |

## 参考

- simfile-parser: https://github.com/noahm/simfile-parser (MIT)
- reamber（列映射/骨架思路）: https://github.com/Evrey/reamber (MIT)
- ETT2OSU（sm→osu 语义对照）: https://github.com/Icey0111/ETT2OSU (MIT)
- Etterna TimingData（时间轴权威语义）: https://github.com/etternagame/etterna