// osu!lazer 的读取路径（Step 10B / Wave E）：**全部字段由偏移表驱动**，绝不写死偏移。
//
// 本文件与 `stable.rs` 的分工逐字相同（`stable.rs` 是 32 位 stable 的语义层，本文件是
// 64 位 lazer 的语义层）：`win.rs` 是读原语（含 64 位读），`scan.rs` 是掩码扫描（含
// `find_in_regions64`），`patterns.rs` 是 stable 的锚点台账，本文件是 lazer 的
// **锚点 + 多跳解析 + L1 结构证明 + 解引用链 + 字段级降级**。
//
// ## 三个来源，三层职责（不可混）
//
// | 层 | 来源 | 内容 |
// |---|---|---|
// | 结构常数 | 我们自己的 P4/P4b 证据（`evidence/P4/lazer-scan-20260927-220957.txt`） | 标记模式、站点位移 `0x24`、跳序 `+0x0`/`+0x218`/`+0x310`、表目录名/环境变量名、`x64` 架构串 |
// | 偏移表 | 生成器 `tools/lazer-offsets-gen/`（SOS + IL 双见证） | 每个 `Type.Field → 偏移`、`game_base_vtable`、版本键 |
// | 运行时 | 只读内存（`Source`）+ 只读文件（`TableFiles`） | 目标进程的字节、`sq.version`/`osu!.runtimeconfig.json`/`storage.ini` |
//
// ## 表的梯子（`load_table`，写死；README「表落点与读取侧的回落梯」逐条对应）
//
// 1. `$MMA_LAZER_OFFSETS`（显式文件；开发/运维用）
// 2. `<壳 exe 目录>\lazer-offsets\<lazer>__<runtime>__<arch>.json`（文件命名约定的精确命中；
//    与生成器 `emit --deploy <壳 exe 目录>` 的落点**同一处**——表是随壳分发的产物，
//    不放进游戏安装目录）
// 3. 同目录下**任意** `*.json` 且 `mismatch() == None`（键完全相同、只是文件名被人改过）
// 4. 同目录下**最近**的表（`offsets::nearest_table`）——**必须**过 L1 结构证明（见下：链解通
//    + [`table_probe`] 的表侧字段探针）且**大声记日志**；`nearest_table` 的默认策略恒拒，
//    所以"没有证明"就没有回落表
// 5. 都没有 ⇒ `Reason::LazerOffsetsMissing(<版本>)` + 受影响的字段清单（壳上报，**绝不编造**）
//
// **`game_base_vtable` 不是准入条件**：它是见证/溯源值（运行期 MethodTable 指针，跨进程必然
// 不同——P4 那次 `0x7ff9e7ef8970`、本次提取 `0x7ffe2f2d8970`）。没有这一列的表照样可用；
// 有这一列也只用来记一行确认（同进程实例内重新生成时才会相等）。
//
// ## 链（每 tick 重走；地址都是 64 位）
//
// ```text
//   anchor        = 标记模式（12 字节）在 RW/RWX 区域里的命中地址（P4：两过滤器各 1 命中）
//   site          = anchor - ANCHOR_SITE_DELTA(0x24)  ← **站点**（一个指针字段），不是 GameBase
//   elo           = [site + 0x0]                      → ExternalLinkOpener（P4 探针的 `elo`）
//   api           = [elo  + 0x218]                    → APIAccess（`<api>k__BackingField`）
//   gameBase      = [api  + 0x310]                    → GameBase（`APIAccess.game`）← 多跳解引用（P4 溯源）
//   [gameBase]    = MethodTable ⇒ (b) 对齐且落在可读的已提交区域里（**不是**与表 vtable 比较）
//   storage       = [gameBase + 表.storage]           → BasePath 字符串（folders.songs 的来源之一）
//   bindable      = [gameBase + 表.<Beatmap>]         → value（**按实例化**的偏移！）→ WorkingBeatmap
//   beatmapInfo   = [working + 表.BeatmapInfo]        → MD5Hash / Hash / OnlineID / DifficultyName /
//                                                        Metadata / BeatmapSet
//   metadata      = [info + 表.<Metadata>]            → Title / Artist / Author(RealmUser) / 背景 / 音频
//   screenStack   = [gameBase + 表.<ScreenStack>]     → stack（state.name 的来源；见下面的降级）
//   selectedMods  = [gameBase + 表.SelectedMods]      → value（mods 的来源；见下面的降级）
// ```
//
// 站点位移是**候选表**（[`SITE_DELTAS`]，第一项 = [`ANCHOR_SITE_DELTA`]，与生成器
// `spec.rs::SITE_DELTAS` 逐字同源）：逐条试、每跳都要过指针合理性判据（[`plausible_ptr`]）；
// 只有走完全部跳**且** `[gameBase]` 过 (b) 的尝试才算**候选**，第一个候选就是本次识别的对象。
// 历史误读（Step 10A/10C）：旧版把 `0x24` 当常量直接算 `gameBase` ⇒ 拿**站点**去比 vtable，
// 真机上必然失败（D-notes §16.1 有逐字记录）。
//
// ⚠️ `Bindable<T>.value` 的偏移**依赖 T**（P4b/OPEN-03 的修正：`value` 在
// `NonNullableBindable<WorkingBeatmap>` 上是 `+0x20`，`+0x40` 是 `<Description>`），所以查法
// 是"类型键必须包含实例化实参"（`offsets::FieldLookup`），多个实例化命中 ⇒ **拒读并降级**。
//
// ## L1 结构证明（**重新定义**：跨进程可用的那一版）
//
// 旧定义（"`[gameBase]` == 表里的 `game_base_vtable`"）**跨进程不可能通过**：MethodTable 是
// 运行期地址（ASLR），同一构建的不同进程实例必然不同（P4 `0x7ff9e7ef8970` vs 本次提取
// `0x7ffe2f2d8970`，见 D-notes §16.3）。新证明是**结构性**的，四条并列：
//
// | 条 | 判据 | 在哪做 |
// |---|---|---|
// | (a) | 站点链 `site → [site+0x0] → [+0x218] → [+0x310]` 每跳指针都合理（非空、≥64 KiB、<内核分割、8 字节对齐） | [`resolve_game_base`]（L0 与"重识别"时） |
// | (b) | `[gameBase]` **对齐**且落在**可读的已提交区域**里（读 8 字节能读满）——廉价合理性，**不是**相等 | [`method_table_plausible`]（每帧） |
// | (c) | 会话内 MT **稳定**：第一帧记下它，之后变了 ⇒ 按"对象被重新识别"处理（**重跑解析** + 记日志），不是停帧 | [`SessionProof`]（每帧） |
// | (d) | 字段级读取仍要过既有结构/取值不变量（`invariants::i06` 的 lazer 分支 + `degradedFields`） | `invariants.rs` |
//
// 表里的 `game_base_vtable` 留作**见证/溯源值**（"这张表的 GameBase 是哪个对象类型"、生成侧的
// dump 产物），只用来记一行（[`vtable_witness_note`]，每个 attach 至多一次）：相等 ⇒
// "same-process regeneration"；不等 ⇒ `vtable witness differs (expected across launches)`；
// **永不**因此判失败，也**不**用它挑候选或挑表。
//
// ## 字段级降级（**与 stable 同一条规则**）
//
// `degraded`（仍出帧、缺失字段**不出现**在载荷里 + `degradedFields` 上报）≠ `unhealthy`（不出帧）。
// 本文件只在**两处**判失败，都在 L1 这一层：① 站点链**一个候选都解不出**（`signature-miss:gameBase`；
// `attach` 的 L0 与帧内"重识别"共用同一条判据）；② 帧内 `[gameBase]` 读不到/不合理**且**重跑解析
// 仍无候选。其余一切（链断、字段改名、实例化分不出、表里根本没有该字段的见证）都走
// **字段级降级**：该字段从载荷里消失，`degradedFields` 里留一条
// `<载荷字段>:<原因>`（原因字面量见下），**绝不填 0/占位、绝不沿用上一帧**。
//
// 本步（10B）如实记录的**表缺口**（每一条都能用 `OffsetTable::field_labels()` 核对，
// 也都写进了 §15 的"part A 待补"清单）；Step 10e/10f 把能证的那两条补上了（下表前两行
// 是**已关闭**的，其余每一条都带着"为什么在这个 dump 上证不出"的机械理由）：
//
// | 载荷字段 | 状态 | 为什么 / 需要什么 |
// |---|---|---|
// | `beatmap.time.live` | **Step 10e 已接通** | 生成器 `spec.rs::CHAIN` 增 `beatmap_clock`（`game.beatmapClock`：`OsuGameBase.beatmapClock`）→ `beatmap_clock_time`（`interpolatedTrack`：`osu.Framework.Timing.InterpolatingFramedClock`），`WANTED` 增 `<CurrentTime>k__BackingField`（double 毫秒）；两条见证都来自同一份 dump 的 `dumpobj` + 同一构建的 IL 清单 |
// | `state.number` / `state.name` | **Step 10f 已接通**（本条取代 10B 的"仍缺"） | 见下方「state.{name,number} 的来源」：屏幕栈 → 栈顶屏幕对象 → **MethodTable（EEType）→ TypeDef RID + loader Module → Module.image_base → 进程模块表 → `runtime.typedefs[模块][RID]` → 类型名 → 我们自己的屏幕类型映射**。10B 的"没有可移植的名字源"**仍然成立**（`Drawable.Name` 是 `String.Empty`；类型名不是对象上的字段），Step 10f 改走**运行期结构 + 构建期 RID 表**：每个位移都由生成器从同一份 dump 里比出来（`dumpmt`/`dumpmodule`/`dumparray` 打印值 vs dump 字节），RID→名字来自我们自己的 IL 元数据解析 |
// | `play.mods` / `menu.mods` / `resultsScreen.mods` | 仍缺 | `Mod.acronym` **不是字段**（IL 清单：`osu.Game.Rulesets.Mods.Mod` 只有 `settingsBacking`，具体 mod 只有各自的设置 bindable，全库无 `Acronym` 字段）⇒ 只能读 `ScoreInfo.<ModsJson>`（acronym 的 JSON 串），而这需要一个**游玩/结算态**的 `ScoreInfo`（本步的 dump 采在主菜单：没有 `Player`/`ScoreInfo`）；且参照实现在 lazer 上也不发 `menu.mods` |
// | `play.hits` / `resultsScreen.hits` | 仍缺 | 计数域在 `ScoreInfo.<StatisticsJson>` / `ScoreProcessor.ScoreResultCounts` 上，本步的 dump 里这两个类型**一个活对象都没有**（菜单态）；路径（`Player.<Score>` → `Score.ScoreInfo`；`ResultsScreen.Score`）需要走屏幕栈元素（10f 已经把"屏幕栈元素"这条路打通了：`_array`/`_size` + 数组元素布局都有表项），但 `ScoreInfo` 字段本身仍要在游玩态 dump 里见证 |
// | `files.background` / `files.audio` / `directPath.beatmap*`（两者） | 仍缺 | 要 `BeatmapSetInfo.<Files>`（`List<RealmNamedFileUsage>`）的**元素**：`Filename` → `File.Hash` 才能拼出 `h\hh\<64hex>`。链能走到**列表对象**（`<Files>`@+0x20），但元素是**数组槽**（`_items`），`dumpobj` 对数组只回 `Fields: None` ⇒ 需要"数组元素布局"（数据起点/元素步长）的见证项（表格式扩展），本步不做 |
//
// ## `state.{name,number}` 的来源（Step 10f）
//
// ```text
//   screenStack   = [gameBase + 表.<ScreenStack>]      → OsuScreenStack
//   stack         = [screenStack + 表.stack]           → System.Collections.Generic.Stack<IScreen>
//   array         = [stack + 表._array]                → osu.Framework.Screens.IScreen[]
//   size          = (i32)[stack + 表._size]            → 栈深（`Stack<T>` 的 Push 写在 _array[_size++]）
//   current       = (u64)[array + 表(screen_array).elements.offset + (size-1)*stride]
//   methodTable   = [current]                          → EEType（MethodTable）
//   rid           = (u32)[methodTable + 表(eetype).token.offset] >> 表(eetype).token.shift
//   module        = [methodTable + 表(eetype).loader_module.offset]
//   imageBase     = [module + 表(module).image_base.offset]
//   moduleName    = 目标进程的模块表（Toolhelp32）里 imageBase 对应的 `szModule`
//   typeName      = 表(runtime).typedefs[moduleName][RID 大写十六进制]
//   state.name    = SCREEN_STATE_MAP(typeName)          ← 我们自己的映射（见下）
//   state.number  = model::OBSERVED_STATE_NAMES 里同名状态的编号
// ```
//
// **为什么不是"读屏幕对象上的某个名字字段"**（10B 的结论仍然成立，逐字留着）：三枚屏幕对象的
// `Drawable.Name`（`@0xD8`）都是 `String.Empty`；类型名不是对象上的字段，只能从 MethodTable 出发。
// 10B 认为"MT 落在每进程 loader heap ⇒ 跨进程不可用"——对**地址**成立，但 **MT 的头部内容**
// 是可移植的：`TypeDef RID` 与"哪个程序集"（loader Module → image_base）都只依赖构建，
// 所以 10f 把**结构位移**与**构建期的 RID→名字**分开发布（前者由 dump 见证，后者由 IL 元数据见证）。
//
// **映射规则（我们自己的，不是抄来的）**：见 [`SCREEN_STATE_MAP`]——只有"屏幕类型名 → 观测过的
// 状态名"这一张表；编号来自 `model::OBSERVED_STATE_NAMES`（P8 台账：stable 六态 + lazer 的
// 0/2/5），**不另编一套编号**。表里有、规则里没有的类型 ⇒ `state.name = ""` + 降级
// （v2 约定：观测集外不发假名，也**绝不**判 `unhealthy`），降级原因里带上**实际类型名**。
//
// 这些缺口**不影响**已实现的字段：`client` / `state.{name,number}` /
// `beatmap.{id,set,md5,version,artist,title,mapper}` / `files.beatmap` /
// `folders.{songs,beatmap,game}` / `directPath.beatmapFile` /
// `beatmap.time.{firstObject,lastObject}`（`.osu` 解析，与 stable 同一条链）+ Step 10e 的
// `beatmap.time.live`。

use crate::osu::model::{Client, Reason, Snapshot, OBSERVED_STATE_NAMES};
use crate::osu::offsets::{FieldLookup, LookupError, OffsetTable, Target, ValidationPolicy};
use std::path::{Path, PathBuf};

// ---- 结构常数（来自我们自己的 P4/P4b 证据；**不是**偏移表的内容）----

/// 标记模式（P4 实测：`ScalingContainerTargetDrawSize` 的 `Vector2(1024.0f, 768.0f)` 紧跟两个
/// `0x01` 布尔字节；两种过滤器下**各 1 命中**）。
pub const MARKER_PATTERN: &str = "01 01 00 00 00 00 80 44 00 00 40 44";
/// **站点**位移：`site = anchor - ANCHOR_SITE_DELTA`。
///
/// ⚠️ `site` **不是** `GameBase`：`0x24` 落在中间的**站点**（一个指针字段）上，`GameBase` 要从
/// 站点按 [`GAME_BASE_HOPS`] 多跳解引用才拿得到（P4 实测 `anchor 0xbf359b84 → site 0xbf359b60
/// → … → gameBase 0xbf359488`）。与生成器 `spec.rs::ANCHOR_SITE_DELTA` 同值、同语义。
pub const ANCHOR_SITE_DELTA: i64 = 0x24;
/// 站点位移的**候选表**（生成器 `spec.rs::SITE_DELTAS` / P4 探针 `hypotheses.rs::DELTAS` 的逐字移植）：
/// 逐条试，**由验证决定谁对**（顺序只影响速度）。第一项 = [`ANCHOR_SITE_DELTA`]。
pub const SITE_DELTAS: &[i64] = &[0x24, 0x28, 0x2c, 0x20, 0x30, 0x1c, 0x34];
/// 站点 → `GameBase` 的跳序（生成器 `spec.rs::GAME_BASE_HOPS` 的逐字移植）：
/// `(标签, 在**上一个对象**上的字段偏移)`。第一跳的"上一个对象"就是站点本身（偏移 `0`）。
pub const GAME_BASE_HOPS: &[(&str, u64)] = &[
    ("external_link_opener", 0x0),
    ("api_access", 0x218),
    ("game", 0x310),
];
/// 锚点键名（进 `signature-miss:<key>`；与 stable 的 `patterns::Anchor::key` 同一命名风格）。
pub const ANCHOR_KEY: &str = "gameBase";
/// 架构串（表键的一段；位数分派 DEC-19 的产物）。
pub const ARCH_X64: &str = "x64";
pub const ARCH_X86: &str = "x86";
/// 表目录名（照生成器 README 的落点约定：`<壳 exe 目录>\lazer-offsets\`）。
pub const TABLE_DIR: &str = "lazer-offsets";
/// 显式表路径的环境变量（梯子第 1 级）。
pub const ENV_TABLE: &str = "MMA_LAZER_OFFSETS";
/// 就近回落的版本距离上限（"最近"仍要够近才敢用；版本号是 `年.月日.修订.构建` 形状）。
pub const NEAREST_MAX_DISTANCE: u32 = 64;
/// 标记命中的候选上限（P4 实测 1 命中；给 16 是"同形状代码"的余量，且每个命中都要解出候选）。
pub const MARKER_HIT_LIMIT: usize = 16;
/// 解析失败时逐条打印判词的**行数上限**（尝试数 = 锚点数 × delta 数，最坏上百条；超出只报总数）。
pub const MAX_RESOLUTION_LINES: usize = 16;
/// lazer 的 `folders.beatmap` / `directPath.beatmapFolder` 逐字值。
///
/// **来源**（P8 golden 帧 + tosu `states/menu.ts:100` 的 `safeJoin`）：lazer 的内存
/// `folder` 是空串，参照实现用 `path.join('')` 归一 ⇒ `'.'`；而 `path.join('.', key)`
/// 又把 `'.'` 吃掉 ⇒ `directPath.beatmapFile == files.beatmap`（P5 逐字节相等的那条断言）。
pub const FOLDER_DOT: &str = ".";
/// lazer 存储根下的文件仓目录名（P7：`folders.songs = FullPath + "\files"`）。
pub const FILES_DIR: &str = "files";
/// md5（`beatmap.md5`）与文件仓键（`files.beatmap`）的十六进制长度。
pub const MD5_HEX_LEN: usize = 32;
pub const STORE_KEY_HEX_LEN: usize = 64;
/// `state.name` / `state.number` 的降级原因（Step 10f 起：链上每一步都点名，见文件头）。
pub const GAP_MODS: &str = "play.mods:offsets-missing-ScoreInfo.ModsJson";
pub const GAP_PLAY_HITS: &str = "play.hits:offsets-missing-score-chain";
pub const GAP_RESULTS_HITS: &str = "resultsScreen.hits:offsets-missing-score-chain";
pub const GAP_BACKGROUND: &str = "files.background:offsets-missing-BeatmapSetInfo.Files";
pub const GAP_AUDIO: &str = "files.audio:offsets-missing-BeatmapSetInfo.Files";
pub const GAP_NO_BEATMAP: &str = "beatmap:none";
/// 屏幕栈深度的合理上界（越界 = 读到了垃圾 `_size`；fail-closed 到降级，不跟着走）。
pub const SCREEN_STACK_MAX: i32 = 64;
/// `state.name` 里"类型名读到了、但不在观测集里"的降级原因（v2 约定：发 `""` + 降级）。
pub const UNMAPPED_SCREEN_SUFFIX: &str = "not-in-observed-set";

/// **屏幕类型名 → `state.name`**（我们自己的映射规则；溯源见文件头）。
///
/// 只用**观测过的**名字（`model::OBSERVED_STATE_NAMES` 的六个），编号也从那一份观测集取
/// （`OBSERVED_STATE_NAMES` 的 `(编号, 名字)`；不另编一套）。每条的类型名都来自 lazer 自己的
/// 屏幕类（`osu.Game.Screens.*`），逐条溯源：
///
/// | 屏幕类型 | 状态 | 溯源 |
/// |---|---|---|
/// | `osu.Game.Screens.Menu.MainMenu` | `menu` | **本步 dump 直接见证**：该 dump 的屏幕栈顶就是它（`dumpmt 0x7FFE304DCCC0` → `Name: osu.Game.Screens.Menu.MainMenu`），P4b/P5 的 lazer 帧在同一态上报 `(0, menu)` |
/// | `osu.Game.Screens.Select.SoloSongSelect` | `selectPlay` | **本步真机见证**：Step 10f 的 live 对拍里，我们的链解出的栈顶类型就是它，而同一时刻 tosu 的 `/json/v2` 报 `(5, selectPlay)`（§19 逐字记录）。类型名本身来自 IL 元数据的 TypeDef 行（`String` 精确相等） |
/// | `osu.Game.Screens.Select.SongSelect` | `selectPlay` | 它的**基类**（lazer 把 `SoloSongSelect` 推上栈；基类留作同一状态的容忍项，类型名精确相等才命中） |
/// | `osu.Game.Screens.Play.Player` / `SoloPlayer` / `ReplayPlayer` | `play` | lazer 的游玩屏（单人/回放），P8 的 lazer 帧在游玩态上报 `(2, play)`；本步未直接观测（用户当时在选歌） |
/// | `osu.Game.Screens.Play.PlayerLoader` | `play` | 进图加载器（`Player` 之前推入，属"游玩中"）——**本步未验证**，留给下一步与 tosu 对拍（§19 已列） |
/// | `osu.Game.Screens.Ranking.ResultsScreen` / `SoloResultsScreen` | `resultScreen` | lazer 的结算屏（单人/多人共用基类）；stable 侧观测到 `(7, resultScreen)`，lazer 侧本步未观测 |
/// | `osu.Game.Screens.Edit.Editor` | `edit` | lazer 的编辑器本体；stable 侧观测到 `(1, edit)`，lazer 侧本步未观测 |
/// | `osu.Game.Screens.Edit.EditorLoader` | `selectEdit` | 编辑器的**进入/选图**阶段（stable 的 `selectEdit` 语义）；**本步未验证**，留给下一步与 tosu 对拍（§19 已列） |
///
/// 表里出现、规则里没有的屏幕类型 ⇒ `state.name = ""` + 降级（**绝不**猜一个近似状态）；
/// 降级原因里带**实际类型名**（`state.name:osu.Game.Screens.X-not-in-observed-set`）。
/// 这也正是 Step 10f 的 live 对拍关掉的第一条：第一次跑出的是
/// `state.name:osu.Game.Screens.Select.SoloSongSelect-not-in-observed-set`（链解对了、规则还没收它），
/// 补上 `SoloSongSelect` 之后同一秒 tosu 的 `(5, selectPlay)` 与我们逐字相等。
pub const SCREEN_STATE_MAP: &[(&str, &str)] = &[
    ("osu.Game.Screens.Menu.MainMenu", "menu"),
    ("osu.Game.Screens.Select.SoloSongSelect", "selectPlay"),
    ("osu.Game.Screens.Select.SongSelect", "selectPlay"),
    ("osu.Game.Screens.Play.Player", "play"),
    ("osu.Game.Screens.Play.SoloPlayer", "play"),
    ("osu.Game.Screens.Play.ReplayPlayer", "play"),
    ("osu.Game.Screens.Play.PlayerLoader", "play"),
    ("osu.Game.Screens.Ranking.ResultsScreen", "resultScreen"),
    ("osu.Game.Screens.Ranking.SoloResultsScreen", "resultScreen"),
    ("osu.Game.Screens.Edit.Editor", "edit"),
    ("osu.Game.Screens.Edit.EditorLoader", "selectEdit"),
];

/// 类型名 → 状态名（**逐字相等**：类型名整个来自表的 `runtime.typedefs`，不做模糊匹配）。
pub fn screen_state_for(type_name: &str) -> Option<&'static str> {
    SCREEN_STATE_MAP
        .iter()
        .find(|(screen, _)| *screen == type_name)
        .map(|(_, state)| *state)
}
/// `beatmap.time.live` 的取值域（毫秒）：时钟是 double，读错字段会给出天文数字。
/// ±24 h 之外的读数一律判"不是播放位置"并降级（`MP3_LENGTH_MAX_MS` 是同一量级的域）。
pub const LIVE_TIME_MAX_MS: f64 = 86_400_000.0;

// ---- 表驱动的字段查法（类型键的写法与生成器 `spec.rs::CHAIN` 的 `contains` 判据同形）----

/// `GameBase` 的类型键（**任选其一**：派生类改名不该被当成"锚点失效"，
/// 与 `spec.rs::GAME_BASE_CONTAINS` 同义）。
const GAME_TYPE_ANY: &[&str] = &["osu.Desktop.OsuGameDesktop", "osu.Game.OsuGameBase"];

pub const F_STORAGE: FieldLookup = FieldLookup::new_any(GAME_TYPE_ANY, "<Storage>k__BackingField");
pub const F_BEATMAP: FieldLookup = FieldLookup::new_any(GAME_TYPE_ANY, "<Beatmap>k__BackingField");
pub const F_SCREEN_STACK: FieldLookup =
    FieldLookup::new_any(GAME_TYPE_ANY, "<ScreenStack>k__BackingField");
pub const F_SELECTED_MODS: FieldLookup = FieldLookup::new_any(GAME_TYPE_ANY, "SelectedMods");

pub const F_STORAGE_BASE_PATH: FieldLookup = FieldLookup::new(
    &["osu.Game.IO.OsuStorage"],
    "<BasePath>k__BackingField",
);
/// `Bindable<T>.value`：**按实例化取值**（OPEN-03 的修正；`+0x40` 是 `<Description>`）。
pub const F_BEATMAP_BINDABLE_VALUE: FieldLookup = FieldLookup::new(
    &[
        "osu.Framework.Bindables.NonNullableBindable`1",
        "osu.Game.Beatmaps.WorkingBeatmap",
    ],
    "value",
);
pub const F_WORKING_BEATMAP_INFO: FieldLookup = FieldLookup::new(
    &["osu.Game.Beatmaps.WorkingBeatmapCache+BeatmapManagerWorkingBeatmap"],
    "BeatmapInfo",
);
pub const F_BEATMAP_INFO_MD5: FieldLookup = FieldLookup::new(
    &["osu.Game.Beatmaps.BeatmapInfo"],
    "<MD5Hash>k__BackingField",
);
pub const F_BEATMAP_INFO_HASH: FieldLookup = FieldLookup::new(
    &["osu.Game.Beatmaps.BeatmapInfo"],
    "<Hash>k__BackingField",
);
pub const F_BEATMAP_INFO_ONLINE_ID: FieldLookup = FieldLookup::new(
    &["osu.Game.Beatmaps.BeatmapInfo"],
    "<OnlineID>k__BackingField",
);
pub const F_BEATMAP_INFO_DIFFICULTY_NAME: FieldLookup = FieldLookup::new(
    &["osu.Game.Beatmaps.BeatmapInfo"],
    "<DifficultyName>k__BackingField",
);
pub const F_BEATMAP_INFO_METADATA: FieldLookup = FieldLookup::new(
    &["osu.Game.Beatmaps.BeatmapInfo"],
    "<Metadata>k__BackingField",
);
pub const F_BEATMAP_INFO_SET: FieldLookup = FieldLookup::new(
    &["osu.Game.Beatmaps.BeatmapInfo"],
    "<BeatmapSet>k__BackingField",
);
pub const F_SET_ONLINE_ID: FieldLookup = FieldLookup::new(
    &["osu.Game.Beatmaps.BeatmapSetInfo"],
    "<OnlineID>k__BackingField",
);
pub const F_METADATA_TITLE: FieldLookup =
    FieldLookup::new(&["osu.Game.Beatmaps.BeatmapMetadata"], "<Title>k__BackingField");
pub const F_METADATA_ARTIST: FieldLookup =
    FieldLookup::new(&["osu.Game.Beatmaps.BeatmapMetadata"], "<Artist>k__BackingField");
pub const F_METADATA_AUTHOR: FieldLookup =
    FieldLookup::new(&["osu.Game.Beatmaps.BeatmapMetadata"], "<Author>k__BackingField");
pub const F_REALM_USERNAME: FieldLookup =
    FieldLookup::new(&["osu.Game.Models.RealmUser"], "<Username>k__BackingField");
pub const F_SCREEN_STACK_LIST: FieldLookup =
    FieldLookup::new(&["osu.Game.Screens.OsuScreenStack"], "stack");
/// 屏幕栈的**元素来源**（Step 10f）：`Stack<IScreen>._array`（`IScreen[]`）与 `._size`（i32）。
///
/// 运行时类型名是 `System.Collections.Generic.Stack\`1[[osu.Framework.Screens.IScreen, osu.Framework]]`
/// （本步 dump 的 `dumpobj` 逐字），所以按"两个子串都在"查（泛型实例化的写法跨版本会变）。
/// `_array`/`_size` 的名字与 `System.Collections.dll` 的 IL 行逐字一致（`Stack\`1._array`/`._size`）。
pub const F_SCREEN_STACK_ARRAY: FieldLookup = FieldLookup::new(
    &["System.Collections.Generic.Stack`1", "osu.Framework.Screens.IScreen"],
    "_array",
);
pub const F_SCREEN_STACK_SIZE: FieldLookup = FieldLookup::new(
    &["System.Collections.Generic.Stack`1", "osu.Framework.Screens.IScreen"],
    "_size",
);
/// `beatmap.time.live` 的三跳（Step 10e；生成器 `spec.rs::CHAIN` 的 `beatmap_clock` /
/// `beatmap_clock_time` 与 `WANTED` 的 `<CurrentTime>k__BackingField` 逐字对应）：
///
/// ```text
///   beatmapClock = [gameBase + 表.beatmapClock]                  → osu.Game.Beatmaps.FramedBeatmapClock
///   trackClock   = [beatmapClock + 表.interpolatedTrack]         → osu.Framework.Timing.InterpolatingFramedClock
///   live         = (double)[trackClock + 表.<CurrentTime>]        → 播放位置（毫秒）
/// ```
///
/// 为什么取**插值**那一枚（而不是 `decoupledTrack` 或 `finalClockSource`）：三者在同一份 dump 上
/// 实测相差几十毫秒（141460.17 / 141460.27 / 141387.17），插值钟就是"当前播放位置"的平滑值；
/// 另两枚是它的源与带用户偏移的下游（`FramedOffsetClock.offset` 为用户偏移）。
pub const F_BEATMAP_CLOCK: FieldLookup =
    FieldLookup::new_any(GAME_TYPE_ANY, "beatmapClock");
pub const F_BEATMAP_TRACK_CLOCK: FieldLookup = FieldLookup::new(
    &["osu.Game.Beatmaps.FramedBeatmapClock"],
    "interpolatedTrack",
);
pub const F_BEATMAP_CLOCK_TIME: FieldLookup = FieldLookup::new(
    &["osu.Framework.Timing.InterpolatingFramedClock"],
    "<CurrentTime>k__BackingField",
);
/// CoreCLR 字符串的布局见证（P4b#8：`_stringLength`@`+0x08`、`_firstChar`@`+0x0C`）。
/// **这两个偏移也从表里查**（缺任一 ⇒ 全部字符串类字段降级，绝不退回写死的 `0x08/0x0C`）。
pub const F_STRING_LENGTH: FieldLookup = FieldLookup::new(&["System.String"], "_stringLength");
pub const F_STRING_CHARS: FieldLookup = FieldLookup::new(&["System.String"], "_firstChar");

/// 表缺失/不可用时**因此没有来源**的载荷字段（壳在下发 `lazer-offsets-missing:<ver>` 的同时
/// 上报这一份清单；§3.3 的 lazer 侧字段里，凡来源是表的一律在内）。
pub const TABLE_BACKED_FIELDS: &[&str] = &[
    "state.number",
    "state.name",
    "beatmap.id",
    "beatmap.set",
    "beatmap.md5",
    "beatmap.version",
    "beatmap.artist",
    "beatmap.title",
    "beatmap.mapper",
    "beatmap.time.live",
    "files.beatmap",
    "files.background",
    "files.audio",
    "folders.songs",
    "directPath.beatmapFile",
    "menu.mods",
    "play.mods",
    "resultsScreen.mods",
    "play.hits",
    "resultsScreen.hits",
];

/// 表缺失时上报的降级清单（**唯一**实现，`mod.rs` 直接用）。
pub fn missing_table_degraded_fields() -> Vec<String> {
    TABLE_BACKED_FIELDS
        .iter()
        .map(|field| (*field).to_string())
        .collect()
}

// ---- 只读内存视图（产品 = `win::Target`；测试 = 合成字节）----

/// 一块只读内存视图的**唯一**抽象（等价物：`__try/ReadProcessMemory` 与一个 `Vec<u8>`）。
///
/// 契约（fail-closed，与 `win::read_exact_at64` 逐字同义）：**必须整段读满**才算成功；
/// 读不满/跨区/不可读 ⇒ `None`——**绝不零填充**（零填充会把"读不到"伪装成"值是 0"）。
pub trait Source {
    fn read(&self, addr: u64, len: usize) -> Option<Vec<u8>>;
}

#[cfg(windows)]
impl Source for crate::osu::win::Target {
    fn read(&self, addr: u64, len: usize) -> Option<Vec<u8>> {
        let mut buf = vec![0u8; len];
        crate::osu::win::read_exact_at64(self.handle(), addr, &mut buf).ok()?;
        Some(buf)
    }
}

#[cfg(not(windows))]
impl Source for crate::osu::win::Target {
    fn read(&self, _addr: u64, _len: usize) -> Option<Vec<u8>> {
        None
    }
}

/// 8 字节小端读（指针/`long`）。
pub fn read_u64(source: &dyn Source, addr: u64) -> Option<u64> {
    let bytes = source.read(addr, 8)?;
    Some(u64::from_le_bytes(bytes.try_into().ok()?))
}

/// 4 字节小端读（`int`）。
pub fn read_i32(source: &dyn Source, addr: u64) -> Option<i32> {
    let bytes = source.read(addr, 4)?;
    Some(i32::from_le_bytes(bytes.try_into().ok()?))
}

/// 4 字节小端读（`uint`；EEType 的 TypeDef RID 打包字段用）。
pub fn read_u32(source: &dyn Source, addr: u64) -> Option<u32> {
    let bytes = source.read(addr, 4)?;
    Some(u32::from_le_bytes(bytes.try_into().ok()?))
}

/// 8 字节小端读（`double`；`beatmap.time.live` 用——时钟位置是 double 毫秒）。
pub fn read_f64(source: &dyn Source, addr: u64) -> Option<f64> {
    let bytes = source.read(addr, 8)?;
    Some(f64::from_le_bytes(bytes.try_into().ok()?))
}

/// **对象字段**的指针读：`base + offset`（`offset` 来自表）、非空、8 字节对齐。
///
/// 三条判据合起来就是"这一个字段的 L1 最小证明"：地址算得出（表有这一行）、值非空
/// （null 是合法的"没有对象"，调用方按缺字段处理）、指针对齐（x64 的引用字段必然对齐；
/// 不对齐只可能是"偏移读错了一位"）。
pub fn read_ptr_field(source: &dyn Source, base: u64, offset: i64) -> Option<u64> {
    let addr = field_addr(base, offset)?;
    let pointer = read_u64(source, addr)?;
    if pointer == 0 || pointer % 8 != 0 {
        return None;
    }
    Some(pointer)
}

/// `base + offset`（`offset` 必须先被表查过；负偏移在表里不该出现，这里也拒掉）。
pub fn field_addr(base: u64, offset: i64) -> Option<u64> {
    if offset < 0 {
        return None;
    }
    base.checked_add(offset as u64)
}

/// `System.String` 的两个位移（从表里查出来的，不是写死的常数）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StringLayout {
    pub length: i64,
    pub chars: i64,
}

/// CoreCLR（x64）字符串读法：`[+length]` int32 码元数 + `[+chars]` 起 UTF-16LE 正文。
///
/// 防护与 32 位版本（`win::read_csharp_string`）逐字相同：先读长度并夹取上限、正文一次读满、
/// 出现 NUL 即判无效（不把尾随 NUL 当分隔符）。**绝不**把读失败当成空串。
pub fn read_string(source: &dyn Source, addr: u64, layout: StringLayout) -> Option<String> {
    if addr == 0 {
        return None;
    }
    let length_addr = field_addr(addr, layout.length)?;
    let len = read_i32(source, length_addr)?;
    if len <= 0 || len > crate::osu::win::MAX_CSHARP_STRING_UNITS as i32 {
        return None;
    }
    let body_addr = field_addr(addr, layout.chars)?;
    let bytes = source.read(body_addr, len as usize * 2)?;
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    if units.contains(&0) {
        return None;
    }
    String::from_utf16(&units).ok()
}

/// 文件仓键 → 载荷里那句相对路径：`h[0]\h[0:2]\<hash>`（win32 反斜杠）。
///
/// 来源（P5 逐字节比对）：`files.beatmap == directPath.beatmapFile ==
/// '8\8c\8c3af1d6…011b'`，且 `folders.beatmap == '.'`（见 [`FOLDER_DOT`]）。非法形状
/// （不是 64 hex）⇒ `None` ⇒ 该字段降级（**不**构造半个路径）。
pub fn to_lazer_path(hash: &str) -> Option<String> {
    let trimmed = hash.trim();
    if trimmed.len() != STORE_KEY_HEX_LEN
        || !trimmed
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b) || (b'A'..=b'F').contains(&b))
    {
        return None;
    }
    let lower = trimmed.to_lowercase();
    Some(format!(
        "{}\\{}\\{}",
        &lower[0..1],
        &lower[0..2],
        lower
    ))
}

/// `beatmap.md5` 的形状（I-02 的同一判据，读的那一侧先用一次以免发出坏值）。
pub fn is_md5_hex(text: &str) -> bool {
    text.len() == MD5_HEX_LEN
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// 文件仓键的形状（I-03 的 lazer 分支）：`h\hh\<64hex>` 且三段自洽。
///
/// 判据与 [`to_lazer_path`] **互为往返**（`to_lazer_path(叶) == Some(整串)`）：形状与派生
/// 不可能各写一套（写歪了一边，另一边立刻报不变量软失败）。
pub fn is_store_key(text: &str) -> bool {
    let lower = text.to_lowercase();
    let mut parts = lower.split('\\');
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(head), Some(prefix), Some(leaf), None) => {
            is_hex(head)
                && is_hex(prefix)
                && is_hex(leaf)
                && head.len() == 1
                && prefix.len() == 2
                && leaf.len() == STORE_KEY_HEX_LEN
                && prefix.starts_with(head)
                && leaf.starts_with(prefix)
        }
        _ => false,
    }
}

fn is_hex(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// 从 BeatmapSetInfo.<Files> 读取文件表：`Vec<(filename, lazer_store_path)>`。
pub fn read_beatmap_set_files(
    source: &dyn Source,
    beatmap_set: u64,
    layout: StringLayout,
) -> Vec<(String, String)> {
    let mut files = Vec::new();
    let Some(files_list) = read_ptr_field(source, beatmap_set, 0x20) else {
        return files;
    };
    let Some(size_addr) = field_addr(files_list, 0x10) else {
        return files;
    };
    let Some(size) = read_i32(source, size_addr) else {
        return files;
    };
    if size <= 0 || size > 1000 {
        return files;
    }
    let Some(items_array) = read_ptr_field(source, files_list, 0x08) else {
        return files;
    };
    let Some(len_addr) = field_addr(items_array, 0x08) else {
        return files;
    };
    let Some(arr_len) = read_i32(source, len_addr) else {
        return files;
    };
    if arr_len <= 0 {
        return files;
    }
    let count = (size.min(arr_len) as usize).min(1000);
    for i in 0..count {
        let elem_offset = 0x10 + (i as i64) * 8;
        let Some(usage_ptr) = read_ptr_field(source, items_array, elem_offset) else {
            continue;
        };
        let ptr_a = read_ptr_field(source, usage_ptr, 0x18);
        let ptr_b = read_ptr_field(source, usage_ptr, 0x20);
        let (filename_opt, file_ptr_opt) = match (ptr_a, ptr_b) {
            (Some(a), Some(b)) => {
                if let Some(s) = read_string(source, a, layout) {
                    (Some(s), Some(b))
                } else if let Some(s) = read_string(source, b, layout) {
                    (Some(s), Some(a))
                } else {
                    (None, None)
                }
            }
            _ => (None, None),
        };
        let Some(filename) = filename_opt else {
            continue;
        };
        let Some(file_ptr) = file_ptr_opt else {
            continue;
        };
        let hash_opt = read_ptr_field(source, file_ptr, 0x18)
            .and_then(|h_ptr| read_string(source, h_ptr, layout))
            .or_else(|| {
                read_ptr_field(source, file_ptr, 0x20)
                    .and_then(|h_ptr| read_string(source, h_ptr, layout))
            });
        let Some(hash) = hash_opt else {
            continue;
        };
        if let Some(lazer_path) = to_lazer_path(&hash) {
            files.push((filename, lazer_path));
        }
    }
    files
}

/// 在文件表里按背景图文件名检索仓库路径；未指定或未匹配时按图像后缀回退。
pub fn find_background_file(files: &[(String, String)], target_name: Option<&str>) -> Option<String> {
    if let Some(target) = target_name {
        let clean = target.trim().trim_matches('"');
        if !clean.is_empty() {
            for (filename, path) in files {
                if filename.eq_ignore_ascii_case(clean) {
                    return Some(path.clone());
                }
            }
            let leaf = std::path::Path::new(clean)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(clean);
            for (filename, path) in files {
                if filename.eq_ignore_ascii_case(leaf) {
                    return Some(path.clone());
                }
            }
        }
    }
    for (filename, path) in files {
        let lower = filename.to_lowercase();
        if lower.ends_with(".jpg") || lower.ends_with(".png") || lower.ends_with(".jpeg") || lower.ends_with(".webp") {
            return Some(path.clone());
        }
    }
    None
}

// ---- EEType → 类型名（Step 10f）----
//
// 这一段的唯一职责：把一个**对象地址**变成**类型名**（或一条逐字原因）。位移全部来自表的
// `runtime` 段（`eetype.token` / `eetype.loader_module` / `module.image_base`），
// RID→名字来自 `runtime.typedefs[模块文件名]`（IL 侧见证）；模块文件名由**目标进程的模块表**
// （`Toolhelp32`，基址在进程内唯一）按 image base 反查——基址是每进程的，所以它是**运行期读数**，
// 不是表里的常量。

/// 一次 EEType→类型名 解析的全部中间读数（诊断/证据用；每一步都可缺）。
#[derive(Clone, Debug, Default)]
pub struct EetypeRead {
    /// 对象地址（`[对象]` = MethodTable）。
    pub object: u64,
    pub method_table: u64,
    /// `TypeDef RID`（`u32[MT + token.offset] >> token.shift`）。
    pub rid: u32,
    /// loader Module 对象（`[MT + loader_module.offset]`）。
    pub module: u64,
    /// 该 Module 的映像基址（`[module + image_base.offset]`）。
    pub image_base: u64,
    /// 映像基址在目标进程模块表里对应的模块名（`osu.Game.dll`）。
    pub module_name: Option<String>,
    /// 最终类型名（`runtime.typedefs[模块][RID]`）。
    pub type_name: Option<String>,
}

impl EetypeRead {
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "object": format!("0x{:016X}", self.object),
            "method_table": format!("0x{:016X}", self.method_table),
            "rid": self.rid,
            "module": format!("0x{:016X}", self.module),
            "image_base": format!("0x{:016X}", self.image_base),
            "module_name": self.module_name,
            "type_name": self.type_name,
        })
    }
}

/// EEType→类型名 的**唯一**实现（表驱动；每一跳失败都给一条可进 `degradedFields` 的原因）。
///
/// 失败原因字面量（`<载荷字段>:<原因>` 的 `<原因>` 段）：
/// `offsets-missing-runtime:<组>.<项>` / `read-methodtable` / `methodtable-implausible` /
/// `read-token` / `token-domain` / `read-loader-module` / `module-implausible` /
/// `read-image-base` / `image-base-implausible` / `module-unresolved:0x…` /
/// `type-unresolved:<模块>#<RID>`。
pub fn read_eetype(
    source: &dyn Source,
    table: &OffsetTable,
    object: u64,
    modules: &[(u64, String)],
) -> Result<EetypeRead, String> {
    let token = table
        .runtime_entry("eetype", "token")
        .ok_or_else(|| "offsets-missing-runtime:eetype.token".to_string())?;
    if token.shift > 24 {
        return Err("token-domain".to_string());
    }
    let loader_module = table
        .runtime_entry("eetype", "loader_module")
        .ok_or_else(|| "offsets-missing-runtime:eetype.loader_module".to_string())?;
    let image_base_entry = table
        .runtime_entry("module", "image_base")
        .ok_or_else(|| "offsets-missing-runtime:module.image_base".to_string())?;

    let mut read = EetypeRead {
        object,
        ..EetypeRead::default()
    };
    read.method_table = read_u64(source, object).ok_or_else(|| "read-methodtable".to_string())?;
    if !plausible_ptr(read.method_table) {
        return Err("methodtable-implausible".to_string());
    }
    let token_addr = field_addr(read.method_table, token.offset).ok_or_else(|| "token-domain".to_string())?;
    let packed = read_u32(source, token_addr).ok_or_else(|| "read-token".to_string())?;
    read.rid = packed >> token.shift;
    if read.rid == 0 {
        return Err("token-domain".to_string());
    }
    let module_addr =
        field_addr(read.method_table, loader_module.offset).ok_or_else(|| "module-implausible".to_string())?;
    read.module = read_u64(source, module_addr).ok_or_else(|| "read-loader-module".to_string())?;
    if !plausible_ptr(read.module) {
        return Err("module-implausible".to_string());
    }
    let image_addr =
        field_addr(read.module, image_base_entry.offset).ok_or_else(|| "image-base-implausible".to_string())?;
    read.image_base = read_u64(source, image_addr).ok_or_else(|| "read-image-base".to_string())?;
    if !plausible_ptr(read.image_base) {
        return Err("image-base-implausible".to_string());
    }
    let module_name = modules
        .iter()
        .find(|(base, _)| *base == read.image_base)
        .map(|(_, name)| name.clone())
        .ok_or_else(|| format!("module-unresolved:0x{:016X}", read.image_base))?;
    let type_name = table
        .runtime_type_name(&module_name, read.rid)
        .ok_or_else(|| format!("type-unresolved:{module_name}#{:X}", read.rid))?
        .to_string();
    read.module_name = Some(module_name);
    read.type_name = Some(type_name);
    Ok(read)
}

/// `state.{name,number}` 的读数（Step 10f；`number` 只在映射命中时才有）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScreenState {
    /// 观测集里的编号（`model::OBSERVED_STATE_NAMES`）。
    pub number: i32,
    /// 观测过的状态名（`menu`/`selectPlay`/…）。
    pub name: String,
    /// 屏幕的**实际类型名**（降级报告里也带它，便于对拍）。
    pub type_name: String,
}

/// [`read_screen_state`] 的三种结论（**没有第四种**：绝不猜一个近似状态）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScreenOutcome {
    /// 类型名命中了观测集 ⇒ `state.{name,number}` 都有值。
    Mapped(ScreenState),
    /// 类型名读到了、但规则里没有这个屏幕 ⇒ v2 约定：`state.name = ""` + 降级（带实际类型名）。
    Unmapped(String),
    /// 链上某一步失败 ⇒ 逐字原因（调用方按字段级降级处理，两个字段都点名）。
    Unresolved(String),
}

/// 屏幕栈 → 栈顶屏幕 → EEType → 类型名 → `SCREEN_STATE_MAP`（**唯一**实现）。
///
/// 返回 [`ScreenOutcome`]：命中观测集 / 类型名读到了但不在映射里（调用方按 v2 约定发
/// `""` + 降级，并把类型名写进原因）/ 链上某一步失败（字段级降级）。
fn read_screen_state(
    source: &dyn Source,
    table: &OffsetTable,
    game_base: u64,
    resolved: &Resolved,
    chain: &mut ChainAddrs,
    modules: &[(u64, String)],
) -> ScreenOutcome {
    let unresolved = |reason: String| ScreenOutcome::Unresolved(reason);
    let lookup = |offset: &Result<i64, LookupError>| -> Result<i64, String> {
        offset.clone().map_err(|error| format!("offsets-{error}"))
    };
    let stack_offset = match lookup(&resolved.screen_stack) {
        Ok(value) => value,
        Err(reason) => return unresolved(reason),
    };
    let list_offset = match lookup(&resolved.screen_stack_list) {
        Ok(value) => value,
        Err(reason) => return unresolved(reason),
    };
    let array_offset = match lookup(&resolved.screen_stack_array) {
        Ok(value) => value,
        Err(reason) => return unresolved(reason),
    };
    let size_offset = match lookup(&resolved.screen_stack_size) {
        Ok(value) => value,
        Err(reason) => return unresolved(reason),
    };

    chain.screen_stack = read_ptr_field(source, game_base, stack_offset);
    let Some(screen_stack) = chain.screen_stack else {
        return unresolved("read-<ScreenStack>".to_string());
    };
    chain.screen_stack_list = read_ptr_field(source, screen_stack, list_offset);
    let Some(stack) = chain.screen_stack_list else {
        return unresolved("read-OsuScreenStack.stack".to_string());
    };
    chain.screen_stack_array = read_ptr_field(source, stack, array_offset);
    let Some(array) = chain.screen_stack_array else {
        return unresolved("read-Stack._array".to_string());
    };
    // 数组元素布局只在真的要到元素时才查（诊断里的链地址因此不会因为表缺 `runtime` 段而消失）。
    let array_entry = match table.runtime_entry("screen_array", "elements") {
        Some(entry) => entry,
        None => return unresolved("offsets-missing-runtime:screen_array.elements".to_string()),
    };
    let Some(size_addr) = field_addr(stack, size_offset) else {
        return unresolved("read-Stack._size".to_string());
    };
    let Some(size) = read_i32(source, size_addr) else {
        return unresolved("read-Stack._size".to_string());
    };
    if size <= 0 || size > SCREEN_STACK_MAX {
        return unresolved(format!("screen-stack-empty:{size}"));
    }
    let index = (size - 1) as i64;
    let stride = array_entry.stride.max(8);
    let Some(element_addr) = field_addr(array, array_entry.offset + index.saturating_mul(stride)) else {
        return unresolved("element-address".to_string());
    };
    let Some(element) = read_u64(source, element_addr) else {
        return unresolved("read-screen-element".to_string());
    };
    if !plausible_ptr(element) {
        return unresolved("screen-element-implausible".to_string());
    }
    chain.screen_top = Some(element);
    let read = match read_eetype(source, table, element, modules) {
        Ok(read) => read,
        Err(reason) => {
            // 中间读数能填多少填多少（诊断里能看出链走到哪一步断的）。
            chain.screen_top_vtable = read_u64(source, element);
            return unresolved(reason);
        }
    };
    chain.screen_top_vtable = Some(read.method_table);
    chain.screen_top_module = Some(read.module);
    chain.screen_top_image_base = Some(read.image_base);
    let type_name = read.type_name.clone().unwrap_or_default();
    match screen_state_for(&type_name) {
        Some(state) => match OBSERVED_STATE_NAMES
            .iter()
            .find(|(_, name)| *name == state)
            .map(|(number, _)| *number)
        {
            Some(number) => ScreenOutcome::Mapped(ScreenState {
                number,
                name: state.to_string(),
                type_name,
            }),
            None => unresolved(format!("state-not-in-observed-set:{state}")),
        },
        None => ScreenOutcome::Unmapped(type_name),
    }
}

/// 表侧/进程侧的**运行期结构证明**（Step 10f 的第二条 L1 判据，与 [`table_probe`] 并列）：
/// 用这张表的 `runtime` 段在目标进程上把 `[gameBase]`（GameBase 的 EEType）解成**类型名**，
/// 并要求它等于我们自己的 `GAME_TYPE_ANY`（`osu.Desktop.OsuGameDesktop` / 基类名）。
///
/// 这条证明同时覆盖：EEType 位移、Module 位移、image base 位移、进程模块表、以及
/// `runtime.typedefs` 里那条 RID 记录——**任何一环错位都过不了**（比"读一个字段"强得多）。
pub fn runtime_probe(
    source: &dyn Source,
    table: &OffsetTable,
    game_base: u64,
    modules: &[(u64, String)],
) -> Result<String, String> {
    let read = read_eetype(source, table, game_base, modules)?;
    let name = read.type_name.clone().unwrap_or_default();
    if !GAME_TYPE_ANY.iter().any(|needle| name.contains(needle)) {
        return Err(format!(
            "`[gameBase]` (EEType 0x{:016X}) resolves to `{name}` in {}# {:X} — expected one of {:?}",
            read.method_table, read.module_name.as_deref().unwrap_or("<unknown>"), read.rid, GAME_TYPE_ANY
        ));
    }
    Ok(format!(
        "[gameBase] EEType 0x{:016X} -> {}# {:X} -> `{name}` (image base 0x{:016X})",
        read.method_table,
        read.module_name.as_deref().unwrap_or("<unknown>"),
        read.rid,
        read.image_base
    ))
}

// ---- (a) GameBase 解析：`anchor → site → … → gameBase`（生成器 `chain.rs` 的逐字移植）----
//
// 与工具**同一批判据、同一批失败文案**，只把"读 dump 字节"换成"读活进程内存"（`Source`）。
// 真机与自测走**同一份代码**（真实路径喂 `win::Target`，自测喂合成镜像）。

/// x64 用户态指针的合理性判据（生成器 `chain.rs::plausible_ptr` 的逐字移植：非空、在空页之上、
/// 在内核分割之下、8 字节对齐）。只用来在解引用前挡掉垃圾值，**不是**类型判据。
pub fn plausible_ptr(pointer: u64) -> bool {
    pointer >= 0x1_0000 && pointer < 0x0000_8000_0000_0000 && pointer % 8 == 0
}

/// (b) 判据：`[gameBase]` 必须是**结构上像** MethodTable 的值——非空、8 字节对齐，且地址落在
/// **可读的已提交区域**里（读 8 字节能读满；`Source` 的 fail-closed 契约把"未提交/无权限"变成
/// `None`）。
///
/// 刻意**不是**与表里 `game_base_vtable` 的相等比较：那一列是运行期地址，跨进程必然不同
/// （ASLR，见文件头「L1 结构证明」）。
pub fn method_table_plausible(source: &dyn Source, method_table: u64) -> bool {
    plausible_ptr(method_table) && source.read(method_table, 8).is_some()
}

/// 一次 `(anchor, delta)` 解析尝试的**完整记录**（**没有静默失败**：每个字段都可为 `None`，
/// `verdict` 逐字说明被哪一步挡下、当时读到了什么）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResolveAttempt {
    pub anchor: u64,
    pub delta: i64,
    /// `anchor - delta`（P4 探针的 `site`）。
    pub site: Option<u64>,
    /// 站点读出的指针（P4 探针的 `elo`）。
    pub external_link_opener: Option<u64>,
    /// `APIAccess`（P4 探针的 `api`）。
    pub api_access: Option<u64>,
    /// 最后一跳读出的值（P4 探针的 `gameBase`）。被合理性判据挡下的值也记在这里（报告逐字复现），
    /// 但**只有 `candidate` 为真时它才是候选**。
    pub game_base: Option<u64>,
    /// `[gameBase]`（MethodTable）；只有读到时才有值。
    pub method_table: Option<u64>,
    /// (a)：走完全部跳且每一跳的指针都过了合理性判据。
    pub resolved: bool,
    /// (a) **且** (b)：`[gameBase]` 对齐且可读 ⇒ 这是一个**候选**。
    pub candidate: bool,
    pub verdict: String,
}

impl ResolveAttempt {
    /// 打印一行用的通路描述（`site -> … -> game`，读不到的部分写 `<none>`）。
    pub fn path(&self) -> String {
        let mut parts: Vec<String> = vec![format!(
            "site={}",
            self.site
                .map(|value| format!("0x{value:016X}"))
                .unwrap_or_else(|| "<none>".into())
        )];
        for (index, (label, _)) in GAME_BASE_HOPS.iter().enumerate() {
            let value = match index {
                0 => self.external_link_opener,
                1 => self.api_access,
                _ => self.game_base,
            };
            parts.push(format!(
                "{label}={}",
                value
                    .map(|value| format!("0x{value:016X}"))
                    .unwrap_or_else(|| "<none>".into())
            ));
        }
        parts.join(" -> ")
    }
}

/// 一趟解析的全部尝试 + 去重后的**候选**（生成器 `chain::Resolution` 的移植）。
#[derive(Clone, Debug, Default)]
pub struct Resolution {
    /// 逐条尝试（顺序 = 锚点序 × delta 表序），报告**逐字**列出。
    pub attempts: Vec<ResolveAttempt>,
    /// 所有过了 (a)+(b) 的尝试（按 `game_base` 去重、按发现顺序）；第一个就是本次识别的对象。
    pub candidates: Vec<ResolveAttempt>,
}

impl Resolution {
    /// 第一个候选（**验收的唯一入口**：没有它 ⇒ 调用方报 `signature-miss:gameBase`）。
    pub fn accepted(&self) -> Option<&ResolveAttempt> {
        self.candidates.first()
    }
}

/// `site = anchor - delta`（`delta` 为负 ⇒ `anchor + |delta|`；下溢/上溢 ⇒ `None`，**绝不回绕**）。
pub fn site_from_anchor(anchor: u64, delta: i64) -> Option<u64> {
    if delta >= 0 {
        anchor.checked_sub(delta as u64)
    } else {
        anchor.checked_add((-delta) as u64)
    }
}

/// 对单个 `(anchor, delta)` 走一遍 `site → … → gameBase`，再做 (b) 判据。
pub fn try_site(source: &dyn Source, anchor: u64, delta: i64) -> ResolveAttempt {
    let mut attempt = ResolveAttempt {
        anchor,
        delta,
        ..ResolveAttempt::default()
    };
    let Some(site) = site_from_anchor(anchor, delta) else {
        attempt.verdict = "anchor-delta underflow".to_string();
        return attempt;
    };
    attempt.site = Some(site);

    let mut current = site;
    for (index, (label, offset)) in GAME_BASE_HOPS.iter().enumerate() {
        let field_address = current.wrapping_add(*offset);
        match read_u64(source, field_address) {
            Some(value) if plausible_ptr(value) => {
                attempt.record_hop(index, value);
                current = value;
            }
            Some(value) => {
                attempt.record_hop(index, value);
                attempt.verdict =
                    format!("{label}={value:#x} implausible (read at {field_address:#x})");
                return attempt;
            }
            None => {
                attempt.verdict = format!(
                    "{label} unreadable at {field_address:#x} (outside the target's readable regions)"
                );
                return attempt;
            }
        }
    }
    attempt.resolved = true;
    attempt.game_base = Some(current);
    // (b)：`[gameBase]` 的结构合理性（对齐 + 可读）。**不是**与表 vtable 的相等比较。
    match read_u64(source, current) {
        Some(method_table) if method_table_plausible(source, method_table) => {
            attempt.method_table = Some(method_table);
            attempt.candidate = true;
            attempt.verdict = format!(
                "candidate: {} hop(s) from site 0x{site:016X}; [gameBase]=0x{method_table:016X} is aligned and readable",
                GAME_BASE_HOPS.len()
            );
        }
        Some(method_table) => {
            attempt.method_table = Some(method_table);
            attempt.verdict = format!(
                "[gameBase]=0x{method_table:016X} is not a plausible MethodTable (aligned + readable)"
            );
        }
        None => {
            attempt.verdict = format!("`[gameBase]` unreadable at 0x{current:016X}");
        }
    }
    attempt
}

impl ResolveAttempt {
    fn record_hop(&mut self, index: usize, value: u64) {
        match index {
            0 => self.external_link_opener = Some(value),
            1 => self.api_access = Some(value),
            _ => self.game_base = Some(value),
        }
    }
}

/// 扫完整个 `(锚点 × delta)` 笛卡尔积（**不提前收手**：每条尝试都留在 `attempts` 里）。
///
/// 候选 = 过了 (a)（每跳合理）**且** (b)（`[gameBase]` 对齐 + 可读）的尝试，按地址去重。
/// 没有候选 ⇒ [`Resolution::candidates`] 为空 ⇒ 调用方必须**拒绝**（`signature-miss:gameBase`），
/// **绝不**退而求其次地拿一个"看起来像"的地址。
pub fn resolve_game_base(source: &dyn Source, anchors: &[u64], deltas: &[i64]) -> Resolution {
    let mut resolution = Resolution::default();
    for anchor in anchors {
        for delta in deltas {
            let attempt = try_site(source, *anchor, *delta);
            if attempt.candidate {
                let known = resolution
                    .candidates
                    .iter()
                    .any(|candidate| candidate.game_base == attempt.game_base);
                if !known {
                    resolution.candidates.push(attempt.clone());
                }
            }
            resolution.attempts.push(attempt);
        }
    }
    resolution
}

/// 解析过程的日志（**没有静默失败**）：有候选 ⇒ 一行说清通路 + 尝试/候选条数；
/// 没有候选 ⇒ 逐条打印判词（上限 [`MAX_RESOLUTION_LINES`]，超出只报总数）。
fn log_resolution(resolution: &Resolution) {
    match resolution.accepted() {
        Some(accepted) => eprintln!(
            "[osu] lazer: L1 resolution — {} -> gameBase ({} attempt(s), {} candidate(s))",
            accepted.path(),
            resolution.attempts.len(),
            resolution.candidates.len()
        ),
        None => {
            eprintln!(
                "[osu] lazer: L1 resolution failed — no candidate among {} attempt(s):",
                resolution.attempts.len()
            );
            for attempt in resolution.attempts.iter().take(MAX_RESOLUTION_LINES) {
                eprintln!(
                    "    anchor=0x{:016X} delta={:#x} {} -> {}",
                    attempt.anchor,
                    attempt.delta,
                    attempt.path(),
                    attempt.verdict
                );
            }
            if resolution.attempts.len() > MAX_RESOLUTION_LINES {
                eprintln!(
                    "    … {} more attempt(s) not printed",
                    resolution.attempts.len() - MAX_RESOLUTION_LINES
                );
            }
        }
    }
}

/// 表侧的结构探针（**就近回落**的校验钩子；L1 结构证明的表侧一半）：用**这张表的偏移**在目标
/// 进程上走两跳，每一跳都必须是合理指针（`<Storage>` 必须在表里且读得出；`<BasePath>` 在表里时
/// 也必须读得出）。它是"这张表在这个进程上说得通"的最小证据面，与 `game_base_vtable` 无关
/// （那一列跨进程不可比——正是它不能当判据的原因）。
pub fn table_probe(
    source: &dyn Source,
    table: &OffsetTable,
    game_base: u64,
) -> Result<String, String> {
    let resolved = Resolved::from_table(table);
    let storage_offset = resolved
        .storage
        .map_err(|error| format!("offsets-{error} for `<Storage>`"))?;
    let storage = read_ptr_field(source, game_base, storage_offset).ok_or_else(|| {
        format!("`[gameBase + {storage_offset:#x}]` (<Storage>) is not a plausible pointer")
    })?;
    let mut hops = format!("<Storage>@+{storage_offset:#x}=0x{storage:016X}");
    match resolved.base_path {
        Ok(offset) => {
            let base_path = read_ptr_field(source, storage, offset).ok_or_else(|| {
                format!("`[storage + {offset:#x}]` (<BasePath>) is not a plausible pointer")
            })?;
            hops.push_str(&format!(" -> <BasePath>@+{offset:#x}=0x{base_path:016X}"));
        }
        Err(error) => hops.push_str(&format!(" (<BasePath> not in the table: offsets-{error})")),
    }
    Ok(hops)
}

/// 表里的 `game_base_vtable` 与**观测到的** MethodTable 的关系（确认 / 差异 / 缺席）。
///
/// **不是判据**：这一列是运行期地址（P4 与本次提取差一个模块装载基址），跨进程必然不同。
/// 它留在表里是**见证/溯源**（"这张表的 GameBase 是哪个对象类型"），也是"同一进程实例内重新
/// 生成"时的确认物；读取侧**永不**因为它判失败。
pub fn vtable_witness_note(table: &OffsetTable, observed: u64) -> String {
    match table.game_base_vtable {
        Some(expected) if expected == observed => format!(
            "vtable witness confirmed — table game_base_vtable=0x{expected:016X} == observed \
             MethodTable (same-process regeneration)"
        ),
        Some(expected) => format!(
            "vtable witness differs (expected across launches) — table 0x{expected:016X} vs \
             observed 0x{observed:016X}"
        ),
        None => "table carries no game_base_vtable witness — the structural proof carries the \
                 identification"
            .to_string(),
    }
}

// ---- 表的落点与梯子 ----

/// 只读文件视图（梯子的每一级都可能缺席 ⇒ `read` 用 `Option`，**不**用 `Result`）。
pub trait TableFiles {
    fn read(&self, path: &Path) -> Option<Vec<u8>>;
    /// 目录里的 `*.json`（不存在 ⇒ 空表；顺序无关，梯子自己会排序）。
    fn list_json(&self, dir: &Path) -> Vec<PathBuf>;
}

/// 真实文件系统（产品路径）。
pub struct RealFiles;

impl TableFiles for RealFiles {
    fn read(&self, path: &Path) -> Option<Vec<u8>> {
        std::fs::read(path).ok()
    }

    fn list_json(&self, dir: &Path) -> Vec<PathBuf> {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut out: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .map(|ext| ext.eq_ignore_ascii_case("json"))
                    .unwrap_or(false)
            })
            .collect();
        out.sort();
        out
    }
}

/// 目标环境（版本键 + 两个只读文件的落点）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TargetEnv {
    /// `osu!.exe` 所在目录（`folders.game`；表目录与版本文件都在它下面）。
    pub exe_dir: PathBuf,
    /// `%APPDATA%\osu\storage.ini`（缺失 ⇒ `None`；它给出存储根，P7）。
    pub storage_ini: Option<PathBuf>,
    /// 架构串（位数分派的产物）。
    pub arch: String,
}

/// 目标环境的**实测**读数（版本键 + 存储根）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TargetInfo {
    /// `sq.version` 的 `<version>`（实测 `2026.921.0-lazer`）；读不到 ⇒ 空串。
    pub lazer_version: String,
    /// `osu!.runtimeconfig.json` 的 `includedFrameworks[].version`（实测 `10.0.12`）；读不到 ⇒ 空串。
    pub runtime_version: String,
    pub arch: String,
    /// `storage.ini` 的 `FullPath`（实测 `D:\Games\osu!lazer`）；读不到 ⇒ `None`。
    pub storage_root: Option<String>,
}

impl TargetInfo {
    /// 表键（`mismatch()`/日志用）。
    pub fn target(&self) -> Target {
        OffsetTable::target(&self.lazer_version, &self.runtime_version, &self.arch)
    }

    /// `folders.songs` = `存储根 + "\files"`（P7 的固定后缀规则；没有存储根 ⇒ `None`）。
    pub fn songs_folder(&self) -> Option<String> {
        self.storage_root
            .as_deref()
            .map(|root| format!("{root}\\{}", FILES_DIR))
    }
}

/// 从只读文件推导目标环境（**全部可选**：读不到就留空/`None`，梯子随后按版本键处理）。
pub fn target_from_files(files: &dyn TableFiles, env: &TargetEnv) -> TargetInfo {
    let lazer_version = files
        .read(&env.exe_dir.join("sq.version"))
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|text| tag_value(&text, "version"))
        .unwrap_or_default();
    let runtime_version = files
        .read(&env.exe_dir.join("osu!.runtimeconfig.json"))
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|text| runtime_version_from_config(&text))
        .unwrap_or_default();
    let storage_root = env.storage_ini.as_deref().and_then(|path| {
        let bytes = files.read(path)?;
        let text = String::from_utf8(bytes).ok()?;
        ini_value(&text, "FullPath")
    });
    TargetInfo {
        lazer_version,
        runtime_version,
        arch: env.arch.clone(),
        storage_root,
    }
}

/// `<version>…</version>` 的取值（`sq.version` 是 XML；不引入 XML 依赖）。
fn tag_value(text: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = text.find(&open)? + open.len();
    let end = text[start..].find(&close)? + start;
    let value = text[start..end].trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

/// `runtimeconfig.json` 的运行时版本：`includedFrameworks[]` 优先（自包含发布），
/// 没有就退到 `framework`（框架依赖发布）。两处都读不到 ⇒ `None`。
fn runtime_version_from_config(text: &str) -> Option<String> {
    for marker in ["includedFrameworks", "\"framework\""] {
        let Some(at) = text.find(marker) else {
            continue;
        };
        let rest = &text[at..];
        let Some(key_at) = rest.find("\"version\"") else {
            continue;
        };
        let after = &rest[key_at + "\"version\"".len()..];
        let colon = after.find(':')?;
        let quoted = after[colon + 1..].trim_start();
        if let Some(value) = quoted_string(quoted) {
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    None
}

/// 行内取值（`FullPath = D:\Games\osu!lazer`）：`key` 之后第一个 `=` 的右侧，去空白。
fn ini_value(text: &str, key: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        let Some((left, right)) = line.split_once('=') else {
            continue;
        };
        if left.trim().eq_ignore_ascii_case(key) {
            let value = right.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn quoted_string(text: &str) -> Option<String> {
    let text = text.strip_prefix('"')?;
    let end = text.find('"')?;
    Some(text[..end].to_string())
}

/// 表的出处（日志/证据/降级报告用）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableOrigin {
    /// `$MMA_LAZER_OFFSETS`。
    Env,
    /// `<exe 目录>\lazer-offsets\<lazer>__<runtime>__<arch>.json`。
    Exact,
    /// 同目录里的**任意**同键表（文件名被改过）。
    SameKey,
    /// 就近版本回落（**已过 L1 结构证明**）。
    Nearest,
}

impl TableOrigin {
    pub fn as_str(self) -> &'static str {
        match self {
            TableOrigin::Env => "env",
            TableOrigin::Exact => "exe-dir",
            TableOrigin::SameKey => "same-key",
            TableOrigin::Nearest => "nearest",
        }
    }
}

/// 梯子产出的表（+ 它是怎么被选中的）。
#[derive(Clone, Debug)]
pub struct LoadedTable {
    pub table: OffsetTable,
    pub origin: TableOrigin,
    pub path: PathBuf,
    /// `mismatch()` 的非空结果（同键/就近回落的证据：**必须**记日志）。
    pub mismatch: Option<String>,
}

/// 表文件名（照生成器的落点约定；`sanitize` 规则与 `emit.rs::sanitize` 逐字同源）。
pub fn table_file_name(lazer_version: &str, runtime_version: &str, arch: &str) -> String {
    format!(
        "{}__{}__{}.json",
        sanitize(lazer_version),
        sanitize(runtime_version),
        sanitize(arch)
    )
}

fn sanitize(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// **表的梯子**（唯一实现；顺序写死，见文件头）。
///
/// `table_dir` = **壳 exe 目录**（生成器 `emit --deploy <壳 exe 目录>` 与 README/D-notes 的落点
/// 约定：`<壳 exe 目录>\lazer-offsets\`）。刻意**不是**游戏 exe 目录——那张表是随壳分发的产物，
/// 且 lazer 的安装目录会被更新覆盖；版本文件（`sq.version`/`osu!.runtimeconfig.json`）仍然从
/// **游戏** exe 目录读（见 `TargetEnv::exe_dir`）。
///
/// `prove` = 调用方提供的 **L1 结构证明**（"这张表在目标进程上说得通吗"：站点链解通 + 表侧
/// 字段探针 [`table_probe`]；**不是** `game_base_vtable` 的相等比较——那一列跨进程必然不同）。
/// 只有**就近回落**这一步需要它；`offsets::nearest_table` 的默认策略恒拒，所以"没有证明"
/// 就没有回落表（机械保证，不是约定）。
pub fn load_table(
    files: &dyn TableFiles,
    env_path: Option<&Path>,
    table_dir: &Path,
    info: &TargetInfo,
    prove: &dyn Fn(&OffsetTable) -> bool,
) -> Result<LoadedTable, Reason> {
    let missing = || Reason::LazerOffsetsMissing(info.lazer_version.clone());

    // ① 显式路径（`$MMA_LAZER_OFFSETS`）。
    if let Some(path) = env_path {
        match files.read(path).map(|bytes| OffsetTable::load(&bytes)) {
            Some(Ok(table)) => match usable(&table) {
                Ok(()) => {
                    let mismatch = table.mismatch(
                        &info.lazer_version,
                        &info.runtime_version,
                        &info.arch,
                    );
                    if let Some(detail) = &mismatch {
                        eprintln!(
                            "[osu] lazer offsets: table from ${ENV_TABLE} ({}) does not match the target key ({detail}) — accepting because the operator pointed at it explicitly",
                            table.key()
                        );
                    }
                    return Ok(LoadedTable {
                        table,
                        origin: TableOrigin::Env,
                        path: path.to_path_buf(),
                        mismatch,
                    });
                }
                Err(why) => eprintln!(
                    "[osu] lazer offsets: table from ${ENV_TABLE} rejected: {why} ({})",
                    table.key()
                ),
            },
            Some(Err(error)) => eprintln!(
                "[osu] lazer offsets: table from ${ENV_TABLE} unreadable: {error} ({})",
                path.display()
            ),
            None => eprintln!(
                "[osu] lazer offsets: ${ENV_TABLE} points at {} but it is not readable — falling through the ladder",
                path.display()
            ),
        }
    }

    let dir = table_dir.join(TABLE_DIR);
    let expected = dir.join(table_file_name(
        &info.lazer_version,
        &info.runtime_version,
        &info.arch,
    ));

    // ② 文件命名约定的精确命中。
    if let Some(Ok(table)) = files.read(&expected).map(|bytes| OffsetTable::load(&bytes)) {
        if usable(&table).is_ok() {
            return Ok(LoadedTable {
                mismatch: table.mismatch(&info.lazer_version, &info.runtime_version, &info.arch),
                table,
                origin: TableOrigin::Exact,
                path: expected,
            });
        }
        eprintln!(
            "[osu] lazer offsets: {} rejected: {}",
            expected.display(),
            usable(&table).err().unwrap_or_default()
        );
    }

    // ③ 同目录下**任意**同键表（文件名被改过；键完全相同 ⇒ 不需要回落）。
    // ④ 同一批候选里挑最近的（**必须**过 L1 证明，且大声记日志）。
    let mut tables: Vec<(PathBuf, OffsetTable)> = Vec::new();
    for path in files.list_json(&dir) {
        if path == expected {
            continue;
        }
        let Some(Ok(table)) = files.read(&path).map(|bytes| OffsetTable::load(&bytes)) else {
            continue;
        };
        if usable(&table).is_ok() {
            tables.push((path, table));
        }
    }
    let target = info.target();
    if let Some((path, table)) = tables
        .iter()
        .find(|(_, table)| table.mismatch(&info.lazer_version, &info.runtime_version, &info.arch).is_none())
    {
        return Ok(LoadedTable {
            table: table.clone(),
            origin: TableOrigin::SameKey,
            path: path.clone(),
            mismatch: None,
        });
    }
    let candidates: Vec<OffsetTable> = tables.iter().map(|(_, table)| table.clone()).collect();
    let distance_labels: Vec<String> = candidates.iter().map(|table| table.key()).collect();
    let policy = ValidationPolicy::new(prove);
    match OffsetTable::nearest_table(&candidates, &target, NEAREST_MAX_DISTANCE, &policy) {
        Ok(nearest) => {
            let key = nearest.key();
            // 候选里同键的那一份就是它的落点（表可能来自任意文件名）。
            let path = tables
                .iter()
                .find(|(_, table)| table.key() == key)
                .map(|(path, _)| path.clone())
                .unwrap_or_else(|| dir.clone());
            let mismatch = nearest.mismatch(&info.lazer_version, &info.runtime_version, &info.arch);
            // **大声记日志**（计划 §4.0 / Step 10 的硬要求）：回落是有代价的选择。
            eprintln!(
                "[osu] lazer offsets: FALLBACK TABLE — target key {}/{}/{} not present; using nearest {} (distance<= {}) because the L1 structural proof (site chain + table field probe) passed. candidates=[{}]",
                info.lazer_version,
                info.runtime_version,
                info.arch,
                key,
                NEAREST_MAX_DISTANCE,
                distance_labels.join(", ")
            );
            Ok(LoadedTable {
                table: nearest.clone(),
                origin: TableOrigin::Nearest,
                path,
                mismatch,
            })
        }
        Err(error) => {
            if !distance_labels.is_empty() {
                eprintln!(
                    "[osu] lazer offsets: nearest fallback refused ({error}); candidates=[{}]",
                    distance_labels.join(", ")
                );
            }
            Err(missing())
        }
    }
}

/// 表的可用性（**唯一**的准入门）：`types` 必须非空（没有字段的表读不出任何东西）。
///
/// `game_base_vtable` **不再是**准入条件：它是见证/溯源值（运行期指针，跨进程不可比），
/// 没有它的表照样可用——识别由结构证明承担（见文件头「L1 结构证明」）。
fn usable(table: &OffsetTable) -> Result<(), String> {
    if table.types.is_empty() {
        return Err("types is empty".to_string());
    }
    Ok(())
}

// ---- 锚点 + L1 结构证明 + 解引用链 ----

/// lazer 解引用链的地址（诊断/证据用；**不进载荷**）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChainAddrs {
    pub game_base: u64,
    /// `[game_base]`（MethodTable）——每帧过 (b) 判据，并受 (c) 的会话内稳定性约束
    /// （变了 ⇒ 重识别 + 重跑解析，**不是**停帧；见文件头「L1 结构证明」）。
    pub vtable: u64,
    pub storage: Option<u64>,
    pub base_path: Option<u64>,
    pub beatmap_bindable: Option<u64>,
    pub working_beatmap: Option<u64>,
    pub beatmap_info: Option<u64>,
    pub beatmap_set: Option<u64>,
    pub metadata: Option<u64>,
    pub realm_user: Option<u64>,
    /// `OsuScreenStack` 对象（`state.name` 的起点）。
    pub screen_stack: Option<u64>,
    /// `OsuScreenStack.stack`（`Stack<IScreen>` 对象；屏幕名映射的来源）。
    pub screen_stack_list: Option<u64>,
    /// `Stack<IScreen>._array`（`osu.Framework.Screens.IScreen[]`；栈元素所在的对象）。
    pub screen_stack_array: Option<u64>,
    /// 栈顶的**屏幕对象**（`_array[_size-1]`；`state.name` 的直接来源）。
    pub screen_top: Option<u64>,
    /// 栈顶屏幕对象的 MethodTable（EEType）。
    pub screen_top_vtable: Option<u64>,
    /// 栈顶屏幕对象的 loader Module（`runtime.eetype.loader_module` 读出来的）。
    pub screen_top_module: Option<u64>,
    /// 上面那个 Module 的 image base（`runtime.module.image_base` 读出来的）。
    pub screen_top_image_base: Option<u64>,
    pub selected_mods: Option<u64>,
    /// `OsuGameBase.beatmapClock`（`FramedBeatmapClock` 对象；`beatmap.time.live` 的起点）。
    pub beatmap_clock: Option<u64>,
    /// 时钟链上的插值音轨时钟（`InterpolatingFramedClock`；live 值的直接来源）。
    pub beatmap_track_clock: Option<u64>,
}

impl ChainAddrs {
    pub fn to_json(&self) -> serde_json::Value {
        let hex = |value: Option<u64>| value.map(|v| format!("0x{v:016X}"));
        serde_json::json!({
            "game_base": format!("0x{:016X}", self.game_base),
            "vtable": format!("0x{:016X}", self.vtable),
            "storage": hex(self.storage),
            "base_path": hex(self.base_path),
            "beatmap_bindable": hex(self.beatmap_bindable),
            "working_beatmap": hex(self.working_beatmap),
            "beatmap_info": hex(self.beatmap_info),
            "beatmap_set": hex(self.beatmap_set),
            "metadata": hex(self.metadata),
            "realm_user": hex(self.realm_user),
            "screen_stack": hex(self.screen_stack),
            "screen_stack_list": hex(self.screen_stack_list),
            "screen_stack_array": hex(self.screen_stack_array),
            "screen_top": hex(self.screen_top),
            "screen_top_vtable": hex(self.screen_top_vtable),
            "screen_top_module": hex(self.screen_top_module),
            "screen_top_image_base": hex(self.screen_top_image_base),
            "selected_mods": hex(self.selected_mods),
            "beatmap_clock": hex(self.beatmap_clock),
            "beatmap_track_clock": hex(self.beatmap_track_clock),
        })
    }
}

/// L0 的产物：表 + `gameBase` + 会话内的 L1 证明状态（每 tick 复用；进程内的 `GameBase` 地址
/// 不随换图变化；变了就走"重识别"，见 [`SessionProof`]）。
#[derive(Clone, Debug)]
pub struct Attach {
    pub table: OffsetTable,
    pub origin: TableOrigin,
    pub table_path: PathBuf,
    pub table_mismatch: Option<String>,
    pub target: TargetInfo,
    /// 当前识别的对象地址（重识别后跟着新对象走；`read_tick` 每帧回写）。
    pub game_base: u64,
    /// L0 的标记命中（**只**用于重识别时重跑解析，不是每帧重扫）。
    pub anchors: Vec<u64>,
    /// 会话内的 L1 证明状态（(c) 的 MT 稳定性）。
    pub proof: SessionProof,
    /// **目标进程的模块表**（`(image base, 模块文件名)`；Step 10f 起 `state.name` 需要它）。
    /// 只在 attach 时取一次；帧内出现 `module-unresolved` 时刷新（新 DLL 会被加载）。
    pub modules: Vec<(u64, String)>,
    pub marker_hits: usize,
    pub scan_ms: u128,
}

/// 逐帧诊断（`ReaderState.lazer` 的来源；`state_json` 会带上它）。
#[derive(Clone, Debug, Default)]
pub struct Diagnostics {
    pub table_key: Option<String>,
    pub table_origin: Option<String>,
    pub table_path: Option<String>,
    pub table_mismatch: Option<String>,
    pub game_base: Option<u64>,
    pub marker_hits: Option<usize>,
    pub scan_ms: Option<u128>,
    pub chain: Option<ChainAddrs>,
    /// 目标进程模块表里的模块数（`state.name` 的链要靠它把 image base 反查成模块名）。
    pub modules: Option<usize>,
    pub gaps: Vec<String>,
}

impl Diagnostics {
    pub fn from_attach(attach: &Attach) -> Diagnostics {
        Diagnostics {
            table_key: Some(attach.table.key()),
            table_origin: Some(attach.origin.as_str().to_string()),
            table_path: Some(attach.table_path.to_string_lossy().to_string()),
            table_mismatch: attach.table_mismatch.clone(),
            game_base: Some(attach.game_base),
            marker_hits: Some(attach.marker_hits),
            scan_ms: Some(attach.scan_ms),
            chain: None,
            modules: Some(attach.modules.len()),
            gaps: Vec::new(),
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "table_key": self.table_key,
            "table_origin": self.table_origin,
            "table_path": self.table_path,
            "table_mismatch": self.table_mismatch,
            "game_base": self.game_base.map(|value| format!("0x{value:016X}")),
            "marker_hits": self.marker_hits,
            "scan_ms": self.scan_ms,
            "chain": self.chain.map(|chain| chain.to_json()),
            "modules": self.modules,
            "gaps": self.gaps,
        })
    }
}

/// 会话内的 L1 证明状态（(c)：**MT 必须在一次会话内稳定**）。
///
/// 第一帧记下 `[gameBase]`；之后每次都拿它比：一致 ⇒ `Stable`；
/// 变了（或 `[gameBase]` 读不到/不合理）⇒ [`ProofStep::Changed`] ⇒ 调用方按"对象被重新识别"
/// 处理：**重跑解析**（[`resolve_game_base`]）并记日志，然后用 [`SessionProof::adopt`] 认下新
/// 对象的 MT。**绝不**因为 MT 变了就停帧——跨进程/跨 GC 的地址变化是正常事件，
/// 而"读错对象"由 (a)(b)(d) 三条判据挡。
#[derive(Clone, Debug, Default)]
pub struct SessionProof {
    /// 本会话当前对象的 MethodTable；`None` = 还没有成功帧。
    vtable: Option<u64>,
}

/// [`SessionProof::observe`] 的结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProofStep {
    /// 本会话的第一次成功观察（记下了 MT）。
    First,
    /// 与记录的 MT 一致 ⇒ 稳定。
    Stable,
    /// 与记录的 MT 不同 ⇒ 需要重新识别。
    Changed { previous: u64 },
}

impl SessionProof {
    /// 本会话当前记录的 MT（诊断/日志用）。
    pub fn session_vtable(&self) -> Option<u64> {
        self.vtable
    }

    /// 观察一帧的 `[gameBase]`。
    pub fn observe(&mut self, method_table: u64) -> ProofStep {
        match self.vtable {
            None => {
                self.vtable = Some(method_table);
                ProofStep::First
            }
            Some(current) if current == method_table => ProofStep::Stable,
            Some(current) => ProofStep::Changed {
                previous: current,
            },
        }
    }

    /// 重识别：把会话记录的 MT 换成新对象的值（重跑解析成功后调用）。
    pub fn adopt(&mut self, method_table: u64) {
        self.vtable = Some(method_table);
    }
}

/// 表里的字段偏移（一次解析；每个字段各自的失败原因都留着，供逐字段降级）。
#[derive(Clone, Debug)]
pub struct Resolved {
    storage: Result<i64, LookupError>,
    base_path: Result<i64, LookupError>,
    beatmap: Result<i64, LookupError>,
    bindable_value: Result<i64, LookupError>,
    working_beatmap_info: Result<i64, LookupError>,
    beatmap_md5: Result<i64, LookupError>,
    beatmap_hash: Result<i64, LookupError>,
    beatmap_online_id: Result<i64, LookupError>,
    beatmap_difficulty_name: Result<i64, LookupError>,
    beatmap_metadata: Result<i64, LookupError>,
    beatmap_set: Result<i64, LookupError>,
    set_online_id: Result<i64, LookupError>,
    metadata_title: Result<i64, LookupError>,
    metadata_artist: Result<i64, LookupError>,
    metadata_author: Result<i64, LookupError>,
    realm_username: Result<i64, LookupError>,
    screen_stack: Result<i64, LookupError>,
    screen_stack_list: Result<i64, LookupError>,
    /// 屏幕栈的元素来源（Step 10f）：`Stack<IScreen>._array` / `._size`。
    screen_stack_array: Result<i64, LookupError>,
    screen_stack_size: Result<i64, LookupError>,
    selected_mods: Result<i64, LookupError>,
    /// `beatmap.time.live` 的三跳（Step 10e）。
    beatmap_clock: Result<i64, LookupError>,
    beatmap_track_clock: Result<i64, LookupError>,
    beatmap_clock_time: Result<i64, LookupError>,
    string_length: Result<i64, LookupError>,
    string_chars: Result<i64, LookupError>,
}

impl Resolved {
    pub fn from_table(table: &OffsetTable) -> Resolved {
        Resolved {
            storage: table.offset_for(&F_STORAGE),
            base_path: table.offset_for(&F_STORAGE_BASE_PATH),
            beatmap: table.offset_for(&F_BEATMAP),
            bindable_value: table.offset_for(&F_BEATMAP_BINDABLE_VALUE),
            working_beatmap_info: table.offset_for(&F_WORKING_BEATMAP_INFO),
            beatmap_md5: table.offset_for(&F_BEATMAP_INFO_MD5),
            beatmap_hash: table.offset_for(&F_BEATMAP_INFO_HASH),
            beatmap_online_id: table.offset_for(&F_BEATMAP_INFO_ONLINE_ID),
            beatmap_difficulty_name: table.offset_for(&F_BEATMAP_INFO_DIFFICULTY_NAME),
            beatmap_metadata: table.offset_for(&F_BEATMAP_INFO_METADATA),
            beatmap_set: table.offset_for(&F_BEATMAP_INFO_SET),
            set_online_id: table.offset_for(&F_SET_ONLINE_ID),
            metadata_title: table.offset_for(&F_METADATA_TITLE),
            metadata_artist: table.offset_for(&F_METADATA_ARTIST),
            metadata_author: table.offset_for(&F_METADATA_AUTHOR),
            realm_username: table.offset_for(&F_REALM_USERNAME),
            screen_stack: table.offset_for(&F_SCREEN_STACK),
            screen_stack_list: table.offset_for(&F_SCREEN_STACK_LIST),
            screen_stack_array: table.offset_for(&F_SCREEN_STACK_ARRAY),
            screen_stack_size: table.offset_for(&F_SCREEN_STACK_SIZE),
            selected_mods: table.offset_for(&F_SELECTED_MODS),
            beatmap_clock: table.offset_for(&F_BEATMAP_CLOCK),
            beatmap_track_clock: table.offset_for(&F_BEATMAP_TRACK_CLOCK),
            beatmap_clock_time: table.offset_for(&F_BEATMAP_CLOCK_TIME),
            string_length: table.offset_for(&F_STRING_LENGTH),
            string_chars: table.offset_for(&F_STRING_CHARS),
        }
    }

    /// 字符串布局（两个位移都查到才算；缺任一 ⇒ 全部字符串类字段降级）。
    pub fn string_layout(&self) -> Result<StringLayout, LookupError> {
        let length = self.string_length.clone()?;
        let chars = self.string_chars.clone()?;
        Ok(StringLayout { length, chars })
    }
}

/// 会话内"对象被重新识别"的记录（(c)：MT 变了 / 读不到 ⇒ 重跑解析）。诊断与日志用。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VtableChange {
    /// 重识别前的会话 MT（`None` = 本会话还没有成功帧）。
    pub previous: Option<u64>,
    /// 触发重识别的观测值（`None` = `[gameBase]` 读不到）。
    pub observed: Option<u64>,
}

/// 一帧的读取结果（快照 + 链 + 降级清单；三者的 `degraded` 是同一份）。
#[derive(Clone, Debug)]
pub struct LazerFrame {
    pub snapshot: Snapshot,
    pub chain: ChainAddrs,
    pub gaps: Vec<String>,
    /// 本帧发生了重识别时记下旧/新 MT（否则 `None`）。
    pub reidentified: Option<VtableChange>,
}

/// 一次采样的入参（普通值，便于把读取逻辑做成纯函数）。
pub struct FrameInput<'a> {
    pub table: &'a OffsetTable,
    pub source: &'a dyn Source,
    pub pid: u32,
    pub game_base: u64,
    /// L0 的标记命中（重识别时重跑解析用的锚点）。
    pub anchors: &'a [u64],
    /// 会话内的 L1 证明状态（(c) 的 MT 稳定性）。
    pub proof: &'a mut SessionProof,
    /// `storage.ini` 派生的绝对 songs 目录（P7；`None` = 没有这个文件）。
    pub songs_folder: Option<String>,
    /// `dirname(osu!.exe)`（`folders.game`）。
    pub game_folder: Option<String>,
    /// **目标进程的模块表**（`(image base, 模块文件名)`；Step 10f）：`state.name` 的链需要它把
    /// 运行期的 image base 反查成模块名（基址每进程不同 ⇒ 它是运行期读数，不是表里的常量）。
    /// 空表 ⇒ 该字段降级（`module-unresolved:…`），**绝不**猜模块。
    pub modules: &'a [(u64, String)],
}

/// 降级标记：`<载荷字段>:<原因>`（**唯一**的构造点，避免各处写法漂移）。
fn gap(field: &str, why: &str) -> String {
    format!("{field}:{why}")
}

/// 查表失败的降级标记（原因字面量 = `LookupError` 的 `Display`）。
fn offsets_gap(field: &str, error: &LookupError) -> String {
    gap(field, &format!("offsets-{error}"))
}

/// 每帧的 L1 结构证明：(b) `[gameBase]` 对齐且可读 → (c) 会话内 MT 稳定。
///
/// MT 变了 / `[gameBase]` 读不到 / 不合理 ⇒ **重跑解析**（[`resolve_game_base`]，锚点来自 L0）
/// 并按新对象继续；重跑仍无候选 ⇒ `Err(signature-miss:gameBase)`（调用方 detach）。
/// 返回 `(game_base, method_table, 本帧的重识别记录)`——`game_base` 在重识别后会变。
fn prove_object(
    source: &dyn Source,
    table: &OffsetTable,
    game_base: u64,
    anchors: &[u64],
    proof: &mut SessionProof,
) -> Result<(u64, u64, Option<VtableChange>), Reason> {
    let observed = read_u64(source, game_base);
    if let Some(method_table) = observed.filter(|value| method_table_plausible(source, *value)) {
        match proof.observe(method_table) {
            ProofStep::First | ProofStep::Stable => return Ok((game_base, method_table, None)),
            ProofStep::Changed { previous } => eprintln!(
                "[osu] lazer: L1 object re-identified — MethodTable at gameBase=0x{game_base:016X} \
                 changed 0x{previous:016X} -> 0x{method_table:016X}; re-resolving GameBase from {} \
                 anchor(s)",
                anchors.len()
            ),
        }
    } else {
        eprintln!(
            "[osu] lazer: L1 object re-identified — `[gameBase]` at 0x{game_base:016X} is {}; \
             re-resolving GameBase from {} anchor(s)",
            observed
                .map(|value| format!("0x{value:016X} (not an aligned+readable MethodTable)"))
                .unwrap_or_else(|| "unreadable".to_string()),
            anchors.len()
        );
    }

    let previous = proof.session_vtable();
    let resolution = resolve_game_base(source, anchors, SITE_DELTAS);
    log_resolution(&resolution);
    let Some(candidate) = resolution.accepted() else {
        eprintln!(
            "[osu] lazer: L1 re-identification failed — no GameBase candidate for {} anchor(s) \
             (signature-miss:{ANCHOR_KEY})",
            anchors.len()
        );
        return Err(Reason::SignatureMiss(ANCHOR_KEY));
    };
    let (new_base, new_vtable) = (
        candidate.game_base.unwrap_or(0),
        candidate.method_table.unwrap_or(0),
    );
    proof.adopt(new_vtable);
    eprintln!(
        "[osu] lazer: L1 re-identified object — gameBase=0x{new_base:016X} delta={:#x} \
         [gameBase]=0x{new_vtable:016X} (table={})",
        candidate.delta,
        table.key()
    );
    Ok((
        new_base,
        new_vtable,
        Some(VtableChange { previous, observed }),
    ))
}

/// 单次采样：(a)+(b)+(c) 的 L1 结构证明 → 解引用链 → §3.3 的 lazer 侧字段。
///
/// **失败出口只有一条**：站点链解不出候选（`signature-miss:gameBase`；含帧内重识别后的重跑）。
/// 其余一切走字段级降级（见文件头），包括 `[gameBase]` 的 MT 与表里 `game_base_vtable` 不等
/// ——那一列是见证物，**不是**判据。
pub fn read_frame(input: FrameInput<'_>) -> Result<LazerFrame, Reason> {
    let FrameInput {
        table,
        source,
        pid,
        game_base,
        anchors,
        proof,
        songs_folder,
        game_folder,
        modules,
    } = input;
    let resolved = Resolved::from_table(table);
    let mut gaps: Vec<String> = Vec::new();
    let mut snapshot = Snapshot {
        client: Some(Client::Lazer),
        pid,
        ..Default::default()
    };

    // ① L1 结构证明（**每帧**重做；(a) 在 L0/重识别时做，(b)(c) 在这里做——定义见文件头）。
    let (game_base, vtable, reidentified) = prove_object(source, table, game_base, anchors, proof)?;
    let mut chain = ChainAddrs {
        game_base,
        vtable,
        ..Default::default()
    };

    // ② 字符串布局（表里的 `System.String` 见证项；缺任一 ⇒ 字符串类字段整体降级）。
    let layout = match resolved.string_layout() {
        Ok(layout) => Some(layout),
        Err(error) => {
            gaps.push(gap("strings", &format!("offsets-{error}")));
            None
        }
    };

    // ③ 存储链：`[gameBase+off] → OsuStorage` → `<BasePath>`（`folders.songs` 的内存来源）。
    if let Ok(offset) = resolved.storage {
        if let Some(storage) = read_ptr_field(source, game_base, offset) {
            chain.storage = Some(storage);
            if let Ok(base_path_offset) = resolved.base_path {
                chain.base_path = read_ptr_field(source, storage, base_path_offset);
            }
        }
    }
    let memory_songs = chain
        .base_path
        .and_then(|pointer| layout.and_then(|layout| read_string(source, pointer, layout)))
        .map(|root| format!("{root}\\{}", FILES_DIR));
    snapshot.folder = Some(FOLDER_DOT.to_string());
    // P7：`folders.songs` 优先 `storage.ini` 的 FullPath + `\files`；两者都有且不一致时**大声**报警
    // （载荷字符串必须与 tosu 一致，静默分歧会让页面缓存键漂移）。
    snapshot.songs_folder = match (&songs_folder, &memory_songs) {
        (Some(ini), Some(memory)) if ini != memory => {
            eprintln!(
                "[osu] lazer: folders.songs mismatch — storage.ini says '{ini}', memory <BasePath> says '{memory}' (using storage.ini per P7)"
            );
            Some(ini.clone())
        }
        (Some(ini), _) => Some(ini.clone()),
        (None, Some(memory)) => Some(memory.clone()),
        (None, None) => {
            gaps.push(gap("folders.songs", "offsets-missing-BasePath"));
            None
        }
    };
    snapshot.game_folder = game_folder;

    // ④ 谱面链：`<Beatmap>` → `value`（**按实例化**）→ WorkingBeatmap → BeatmapInfo。
    let mut beatmap_info = None;
    match (
        resolved.beatmap.clone(),
        resolved.bindable_value.clone(),
        resolved.working_beatmap_info.clone(),
    ) {
        (Ok(beatmap), Ok(value), Ok(info)) => {
            chain.beatmap_bindable = read_ptr_field(source, game_base, beatmap);
            chain.working_beatmap = chain
                .beatmap_bindable
                .and_then(|bindable| read_ptr_field(source, bindable, value));
            chain.beatmap_info = chain
                .working_beatmap
                .and_then(|working| read_ptr_field(source, working, info));
            beatmap_info = chain.beatmap_info;
            if beatmap_info.is_none() {
                gaps.push(GAP_NO_BEATMAP.to_string());
            }
        }
        (beatmap, value, info) => {
            for (offset, field) in [
                (beatmap, "beatmap.object"),
                (value, "beatmap.bindable.value"),
                (info, "beatmap.working"),
            ] {
                if let Err(error) = offset {
                    gaps.push(offsets_gap(field, &error));
                }
            }
        }
    }

    // ⑤ identity / 元数据（逐字段降级）。
    if let Some(info) = beatmap_info {
        // md5：形状不对 ⇒ 该字段降级（绝不发半个哈希）。
        if let Some(md5) = read_text_field(source, info, &resolved.beatmap_md5, "beatmap.md5", layout, &mut gaps) {
            if is_md5_hex(&md5) {
                snapshot.checksum = Some(md5);
            } else {
                gaps.push(gap("beatmap.md5", "shape"));
            }
        }
        // 文件仓键（64 hex）→ 载荷里的相对路径。
        if let Some(hash) = read_text_field(source, info, &resolved.beatmap_hash, "files.beatmap", layout, &mut gaps) {
            match to_lazer_path(&hash) {
                Some(path) => snapshot.filename = Some(path),
                None => gaps.push(gap("files.beatmap", "shape")),
            }
        }
        snapshot.map_id = field_i32(source, info, &resolved.beatmap_online_id, "beatmap.id", &mut gaps);
        snapshot.version = read_text_field(
            source,
            info,
            &resolved.beatmap_difficulty_name,
            "beatmap.version",
            layout,
            &mut gaps,
        );
        chain.metadata = resolved
            .beatmap_metadata
            .as_ref()
            .ok()
            .and_then(|offset| read_ptr_field(source, info, *offset));
        chain.beatmap_set = resolved
            .beatmap_set
            .as_ref()
            .ok()
            .and_then(|offset| read_ptr_field(source, info, *offset));
        if let Some(set) = chain.beatmap_set {
            snapshot.set_id = field_i32(source, set, &resolved.set_online_id, "beatmap.set", &mut gaps);
            if let Some(layout) = layout {
                snapshot.lazer_files = read_beatmap_set_files(source, set, layout);
            }
        } else if let Err(error) = &resolved.beatmap_set {
            gaps.push(offsets_gap("beatmap.set", error));
        } else {
            gaps.push(gap("beatmap.set", "read"));
        }
        let mut bg_name_from_meta = None;
        if let Some(metadata) = chain.metadata {
            snapshot.title = read_text_field(source, metadata, &resolved.metadata_title, "beatmap.title", layout, &mut gaps);
            snapshot.artist =
                read_text_field(source, metadata, &resolved.metadata_artist, "beatmap.artist", layout, &mut gaps);
            if let Some(layout) = layout {
                // <BackgroundFile>k__BackingField offset 96 (0x60)
                bg_name_from_meta = read_ptr_field(source, metadata, 96)
                    .and_then(|ptr| read_string(source, ptr, layout));
            }
            // mapper：`<Author>` 是 RealmUser（**不是字符串**，P4b 的纠正）⇒ 再跳一跳。
            chain.realm_user = resolved
                .metadata_author
                .as_ref()
                .ok()
                .and_then(|offset| read_ptr_field(source, metadata, *offset));
            if let Some(user) = chain.realm_user {
                snapshot.mapper =
                    read_text_field(source, user, &resolved.realm_username, "beatmap.mapper", layout, &mut gaps);
            } else if let Err(error) = &resolved.metadata_author {
                gaps.push(offsets_gap("beatmap.mapper", error));
            } else {
                gaps.push(gap("beatmap.mapper", "read"));
            }
        } else if let Err(error) = &resolved.beatmap_metadata {
            gaps.push(offsets_gap("beatmap.title", error));
            gaps.push(offsets_gap("beatmap.artist", error));
            gaps.push(offsets_gap("beatmap.mapper", error));
        } else {
            gaps.push(gap("beatmap.title", "read"));
            gaps.push(gap("beatmap.artist", "read"));
            gaps.push(gap("beatmap.mapper", "read"));
        }
        if !snapshot.lazer_files.is_empty() {
            if let Some(bg_path) = find_background_file(&snapshot.lazer_files, bg_name_from_meta.as_deref()) {
                snapshot.background = Some(bg_path);
            }
        }
    }

    // ⑥ 屏幕栈 → 栈顶屏幕 → EEType → 类型名 → `state.{name,number}`（Step 10f）。
    //
    // 三种结论（见 [`ScreenOutcome`]）：命中观测集 ⇒ 两个字段都有值；类型名读到了但不在规则里
    // ⇒ `state.name = ""` + 降级（v2 约定，原因里带**实际类型名**）；链上失败 ⇒ 两个字段都按
    // 字段级降级（逐字原因），**绝不**填 0/占位。
    match read_screen_state(source, table, game_base, &resolved, &mut chain, modules) {
        ScreenOutcome::Mapped(state) => {
            snapshot.state_number = Some(state.number);
            snapshot.state_name = Some(state.name.clone());
        }
        ScreenOutcome::Unmapped(type_name) => {
            snapshot.state_name = Some(String::new());
            gaps.push(gap(
                "state.name",
                &format!("{type_name}-{UNMAPPED_SCREEN_SUFFIX}"),
            ));
            gaps.push(gap("state.number", "screen-type-unmapped"));
        }
        ScreenOutcome::Unresolved(reason) => {
            gaps.push(gap("state.name", &reason));
            gaps.push(gap("state.number", &reason));
        }
    }
    match resolved.selected_mods.clone() {
        Ok(offset) => {
            chain.selected_mods = read_ptr_field(source, game_base, offset);
            snapshot.lazer_mods = None;
            if chain.selected_mods.is_none() {
                gaps.push(gap("play.mods", "read"));
                gaps.push(gap("menu.mods", "read"));
                gaps.push(gap("resultsScreen.mods", "read"));
            } else {
                // 持有者读到了，但**acronym 不是字段**（IL 清单里没有任何 `Acronym` 字段，
                // 见文件头的表缺口表）⇒ 三枚 mods 字段仍然没有来源，逐条如实上报：
                // 不是一个"读不到"，而是"这类数据的**唯一**来源是 ScoreInfo.<ModsJson>"。
                for field in ["play.mods", "menu.mods", "resultsScreen.mods"] {
                    gaps.push(gap(field, "offsets-missing-ScoreInfo.ModsJson"));
                }
            }
        }
        Err(error) => {
            gaps.push(offsets_gap("play.mods", &error));
            gaps.push(offsets_gap("menu.mods", &error));
            gaps.push(offsets_gap("resultsScreen.mods", &error));
        }
    }
    // ⑦ 播放时钟（Step 10e）：`beatmap.time.live` —— 表里的三跳
    // （`beatmapClock` → `interpolatedTrack` → `<CurrentTime>k__BackingField`）。
    snapshot.play_time = read_live_time(source, game_base, &resolved, &mut chain, &mut gaps);
    // hits / 背景 / 音频：表里没有来源（见文件头的表缺口表）——**如实降级**。
    gaps.push(GAP_PLAY_HITS.to_string());
    gaps.push(GAP_RESULTS_HITS.to_string());
    gaps.push(GAP_BACKGROUND.to_string());
    gaps.push(GAP_AUDIO.to_string());

    snapshot.lazer_chain = Some(chain);
    gaps.sort();
    gaps.dedup();
    snapshot.degraded_fields = gaps.clone();
    Ok(LazerFrame {
        snapshot,
        chain,
        gaps,
        reidentified,
    })
}

/// 对象字段的**字符串**读（`[base+offset] → System.String`），四处共用同一份降级口径：
/// 布局缺失（`strings` 已上报）/ 偏移查不到 / 指针为 0 或读失败 / 正文不合法各记一条。
fn read_text_field(
    source: &dyn Source,
    base: u64,
    offset: &Result<i64, LookupError>,
    field: &str,
    layout: Option<StringLayout>,
    gaps: &mut Vec<String>,
) -> Option<String> {
    let layout = layout?;
    let offset = match offset {
        Ok(offset) => *offset,
        Err(error) => {
            gaps.push(offsets_gap(field, error));
            return None;
        }
    };
    let Some(pointer) = read_ptr_field(source, base, offset) else {
        gaps.push(gap(field, "read"));
        return None;
    };
    match read_string(source, pointer, layout) {
        Some(text) => Some(text),
        None => {
            gaps.push(gap(field, "string-layout"));
            None
        }
    }
}

/// `beatmap.time.live` 的三跳读法（Step 10e；**唯一**实现，与生成器 `spec.rs` 的三跳逐字对应）：
/// `[gameBase + beatmapClock]` → `[clock + interpolatedTrack]` → `(double)[track + <CurrentTime>]`。
///
/// 每一跳的失败都按**字段级降级**处理，并把逐字原因写进 `gaps`
/// （`<载荷字段>:<原因>`：`offsets-missing-*` / `read-*` / `domain`），绝不填 0、绝不沿用上一帧。
/// 值域判据用 [`LIVE_TIME_MAX_MS`]：double 读错字段会给出天文数字，那种读数不是"播放位置"。
fn read_live_time(
    source: &dyn Source,
    game_base: u64,
    resolved: &Resolved,
    chain: &mut ChainAddrs,
    gaps: &mut Vec<String>,
) -> Option<i32> {
    const FIELD: &str = "beatmap.time.live";
    // 三跳**按顺序**查：第一枚缺的偏移就是唯一一条降级原因（比"三条同义记录"可读，
    // 也点名了该补哪一跳）。
    let mut offsets = [0i64; 3];
    for (index, lookup) in [
        resolved.beatmap_clock.clone(),
        resolved.beatmap_track_clock.clone(),
        resolved.beatmap_clock_time.clone(),
    ]
    .into_iter()
    .enumerate()
    {
        match lookup {
            Ok(offset) => offsets[index] = offset,
            Err(error) => {
                gaps.push(offsets_gap(FIELD, &error));
                return None;
            }
        }
    }
    let (clock_offset, track_offset, time_offset) = (offsets[0], offsets[1], offsets[2]);
    chain.beatmap_clock = read_ptr_field(source, game_base, clock_offset);
    let Some(clock) = chain.beatmap_clock else {
        gaps.push(gap(FIELD, "read-beatmapClock"));
        return None;
    };
    chain.beatmap_track_clock = read_ptr_field(source, clock, track_offset);
    let Some(track) = chain.beatmap_track_clock else {
        gaps.push(gap(FIELD, "read-interpolatedTrack"));
        return None;
    };
    let Some(address) = field_addr(track, time_offset) else {
        gaps.push(gap(FIELD, "read"));
        return None;
    };
    match read_f64(source, address) {
        Some(value) if value.is_finite() && value.abs() <= LIVE_TIME_MAX_MS => {
            Some(value.round() as i32)
        }
        _ => {
            gaps.push(gap(FIELD, "domain"));
            None
        }
    }
}

/// `int` 字段读（失败即降级；**不**返回 0 占位）。
fn field_i32(
    source: &dyn Source,
    base: u64,
    offset: &Result<i64, LookupError>,
    field: &str,
    gaps: &mut Vec<String>,
) -> Option<i32> {
    let offset = match offset {
        Ok(offset) => *offset,
        Err(error) => {
            gaps.push(offsets_gap(field, error));
            return None;
        }
    };
    match field_addr(base, offset).and_then(|addr| read_i32(source, addr)) {
        Some(value) => Some(value),
        None => {
            gaps.push(gap(field, "read"));
            None
        }
    }
}

// ---- 产品装配（Windows）：L0 = 扫描标记 → GameBase 解析（候选表）→ 梯子（结构证明）→ 表落点 ----

/// 扫描标记模式（`FILTER_READY` 逐档；命中即止），返回候选 `anchor` 地址。
#[cfg(windows)]
pub fn scan_markers(
    target: &crate::osu::win::Target,
    regions: &mut crate::osu::scan::RegionCache,
    limit: usize,
) -> Result<(Vec<u64>, u128), Reason> {
    use crate::osu::scan;
    let pattern = scan::Pattern::parse(MARKER_PATTERN).map_err(|_| Reason::SignatureMiss(ANCHOR_KEY))?;
    let started = std::time::Instant::now();
    let mut stats = scan::ScanStats::default();
    for mask in crate::osu::FILTER_READY {
        let list = target.regions_cached(regions, *mask, crate::osu::REGION_LIMIT);
        let hits = scan::find_in_regions64(target.handle(), &list, &pattern, 0, limit, &mut stats);
        if !hits.is_empty() {
            eprintln!(
                "[osu] lazer scan: filter#{} regions={} bytes={} hits={} elapsed={}ms",
                mask,
                list.len(),
                stats.bytes_read,
                hits.len(),
                started.elapsed().as_millis()
            );
            return Ok((hits, started.elapsed().as_millis()));
        }
    }
    Err(Reason::SignatureMiss(ANCHOR_KEY))
}

/// L0 全流程：目标环境读数 → 扫描标记 → **相位 0 的 GameBase 解析** → 表梯子（结构证明）
/// → 多个候选里取第一个（**没有候选就失败，绝不猜**）。
///
/// `env_path` = `$MMA_LAZER_OFFSETS`（`None` = 没有这个环境变量）。
#[cfg(windows)]
pub fn attach(
    target: &crate::osu::win::Target,
    regions: &mut crate::osu::scan::RegionCache,
    env_path: Option<PathBuf>,
) -> Result<Attach, Reason> {
    let env = TargetEnv {
        exe_dir: target
            .image_path
            .parent()
            .map(|dir| dir.to_path_buf())
            .unwrap_or_default(),
        storage_ini: storage_ini_path(),
        arch: arch_for_bitness(target.bitness).to_string(),
    };
    let files = RealFiles;
    let info = target_from_files(&files, &env);
    // 表的落点 = **壳 exe 目录**（生成器 `--deploy` 的约定；见 `load_table` 的说明）。
    let table_dir = shell_exe_dir();
    eprintln!(
        "[osu] lazer target: version={:?} runtime={:?} arch={} storage_root={:?} table_dir={}",
        info.lazer_version,
        info.runtime_version,
        info.arch,
        info.storage_root,
        table_dir.display()
    );
    let source: &dyn Source = target;
    let (anchors, scan_ms) = scan_markers(target, regions, MARKER_HIT_LIMIT)?;
    // 相位 0：`anchor → site → … → gameBase`（与生成器 `chain.rs` 同一批判据）。
    let resolution = resolve_game_base(source, &anchors, SITE_DELTAS);
    log_resolution(&resolution);
    // **目标进程的模块表**（Step 10f）：`state.name` 的链要用 image base 反查模块名。
    // 取不到时**不失败**：那条字段随后按字段级降级（`module-unresolved:…`），其余字段照常。
    let modules = match crate::osu::win::module_list(target.pid) {
        Ok(list) => {
            eprintln!(
                "[osu] lazer modules: {} module(s) enumerated for pid {} (state.name resolves the \
                 assembly by image base)",
                list.len(),
                target.pid
            );
            list
        }
        Err(reason) => {
            eprintln!(
                "[osu] lazer modules: enumeration failed ({reason:?}) — state.name will degrade to \
                 `module-unresolved:…` until the module table is readable"
            );
            Vec::new()
        }
    };
    // 梯子第 4 级（就近回落）的校验钩子 = **同一份结构证明**：链解通 + 表侧字段探针
    // + 运行期结构探针（EEType→类型名 必须解出 `osu.Desktop.OsuGameDesktop` 那一族）。
    // 见证物（`game_base_vtable`）只作确认/记录，**不**参与放行（跨进程必然不同）。
    let prove = |table: &OffsetTable| -> bool {
        let Some(candidate) = resolution.accepted() else {
            eprintln!(
                "[osu] lazer offsets: structural proof for {} refused — no GameBase candidate \
                 resolved from {} anchor(s)",
                table.key(),
                anchors.len()
            );
            return false;
        };
        let game_base = candidate.game_base.unwrap_or(0);
        match table_probe(source, table, game_base) {
            Ok(detail) => eprintln!(
                "[osu] lazer offsets: structural proof for {} passed — {detail}",
                table.key()
            ),
            Err(why) => {
                eprintln!(
                    "[osu] lazer offsets: structural proof for {} refused — {why}",
                    table.key()
                );
                return false;
            }
        }
        match runtime_probe(source, table, game_base, &modules) {
            Ok(detail) => {
                eprintln!(
                    "[osu] lazer offsets: runtime structure proof for {} passed — {detail}",
                    table.key()
                );
                true
            }
            Err(why) => {
                eprintln!(
                    "[osu] lazer offsets: runtime structure proof for {} refused — {why}",
                    table.key()
                );
                false
            }
        }
    };
    let loaded = load_table(&files, env_path.as_deref(), &table_dir, &info, &prove)?;
    let Some(candidate) = resolution.accepted() else {
        eprintln!(
            "[osu] lazer: L1 resolution failed — no GameBase candidate for {} anchor(s); \
             detaching (signature-miss:{ANCHOR_KEY})",
            anchors.len()
        );
        return Err(Reason::SignatureMiss(ANCHOR_KEY));
    };
    let game_base = candidate.game_base.unwrap_or(0);
    let vtable = candidate.method_table.unwrap_or(0);
    // 见证物：**确认/记录**，绝不是放行条件（同一构建的同一进程实例才会相等）。
    eprintln!("[osu] lazer: {}", vtable_witness_note(&loaded.table, vtable));
    if let Some(runtime) = loaded.table.runtime() {
        eprintln!(
            "[osu] lazer: runtime section present ({} typedef name(s) over {} module(s), \
             {} observed): {}",
            runtime.typedefs.values().map(|map| map.len()).sum::<usize>(),
            runtime.typedefs.len(),
            runtime.observed.values().map(|map| map.len()).sum::<usize>(),
            runtime.witness
        );
    } else {
        eprintln!(
            "[osu] lazer: the table carries NO runtime section — state.name/state.number will \
             degrade to field level"
        );
    }
    eprintln!(
        "[osu] lazer: L1 proof passed (structural) — gameBase=0x{game_base:016X} delta={:#x} \
         site=0x{:016X} [gameBase]=0x{vtable:016X} anchors={} scan_ms={scan_ms} table={} origin={}",
        candidate.delta,
        candidate.site.unwrap_or(0),
        anchors.len(),
        loaded.table.key(),
        loaded.origin.as_str()
    );
    let marker_hits = anchors.len();
    Ok(Attach {
        table: loaded.table,
        origin: loaded.origin,
        table_path: loaded.path,
        table_mismatch: loaded.mismatch,
        target: info,
        game_base,
        anchors,
        proof: SessionProof::default(),
        modules,
        marker_hits,
        scan_ms,
    })
}

/// **壳 exe 目录**（表梯子的落点根；生成器 `emit --deploy <壳 exe 目录>` 的同一处）。
/// 读不到自身路径 ⇒ 空（梯子随后只看 `$MMA_LAZER_OFFSETS`，找不到就如实报缺表）。
pub fn shell_exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.to_path_buf()))
        .unwrap_or_default()
}

/// `arch` 串（位数分派；未识别的 machine ⇒ `x86` 之外的保守值）。
pub fn arch_for_bitness(pe_machine: u16) -> &'static str {
    match Client::from_bitness(pe_machine) {
        Some(Client::Lazer) => ARCH_X64,
        _ => ARCH_X86,
    }
}

/// `%APPDATA%\osu\storage.ini`（P7 的存储根来源；只为 `folders.songs` 的优先来源）。
#[cfg(windows)]
pub fn storage_ini_path() -> Option<PathBuf> {
    std::env::var("APPDATA")
        .ok()
        .map(|appdata| PathBuf::from(appdata).join("osu").join("storage.ini"))
}

/// 显式表路径（`$MMA_LAZER_OFFSETS`；空串当没有）。
pub fn env_table_path() -> Option<PathBuf> {
    std::env::var(ENV_TABLE)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// 便捷组合（`mod.rs` 用）：一次采样；重识别后把 L0 的地址跟着新对象走。
#[cfg(windows)]
pub fn read_tick(
    attach: &mut Attach,
    target: &crate::osu::win::Target,
) -> Result<LazerFrame, Reason> {
    let source: &dyn Source = target;
    if attach.modules.is_empty() {
        attach.modules = crate::osu::win::module_list(target.pid).unwrap_or_default();
        eprintln!(
            "[osu] lazer modules: (re-)enumerated {} module(s) — state.name needs the module table",
            attach.modules.len()
        );
    }
    let mut frame = read_frame(FrameInput {
        table: &attach.table,
        source,
        pid: target.pid,
        game_base: attach.game_base,
        anchors: &attach.anchors,
        proof: &mut attach.proof,
        songs_folder: attach.target.songs_folder(),
        game_folder: crate::osu::stable::game_folder(&target.image_path),
        modules: &attach.modules,
    })?;
    // 目标里加载了新 DLL（或第一次枚举不全）⇒ 模块表变了：重取一次再读这一帧。
    if frame
        .gaps
        .iter()
        .any(|gap| gap.contains("module-unresolved"))
    {
        let refreshed = crate::osu::win::module_list(target.pid).unwrap_or_default();
        if refreshed.len() != attach.modules.len() {
            eprintln!(
                "[osu] lazer modules: module table changed ({} -> {}) — re-reading this frame",
                attach.modules.len(),
                refreshed.len()
            );
            attach.modules = refreshed;
            frame = read_frame(FrameInput {
                table: &attach.table,
                source,
                pid: target.pid,
                game_base: attach.game_base,
                anchors: &attach.anchors,
                proof: &mut attach.proof,
                songs_folder: attach.target.songs_folder(),
                game_folder: crate::osu::stable::game_folder(&target.image_path),
                modules: &attach.modules,
            })?;
        }
    }
    attach.game_base = frame.chain.game_base;
    Ok(frame)
}

// ---- 非 Windows 桩（同 `mod.rs` 约定：同名同签名，行为为空）----

#[cfg(not(windows))]
pub fn attach(
    _target: &crate::osu::win::Target,
    _regions: &mut crate::osu::scan::RegionCache,
    _env_path: Option<PathBuf>,
) -> Result<Attach, Reason> {
    Err(Reason::PlatformUnsupported)
}

#[cfg(not(windows))]
pub fn read_tick(
    _attach: &mut Attach,
    _target: &crate::osu::win::Target,
) -> Result<LazerFrame, Reason> {
    Err(Reason::PlatformUnsupported)
}

#[cfg(not(windows))]
pub fn storage_ini_path() -> Option<PathBuf> {
    None
}

#[cfg(test)]
#[path = "../../tests-local/osu_lazer.rs"]
mod tests_lazer;
