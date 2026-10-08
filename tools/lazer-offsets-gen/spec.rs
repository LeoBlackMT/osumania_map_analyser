// lazer-offsets-gen —— **lazer 更新时唯一需要人工改动的文件**（生成器规格表）
//
// 生成器（`main.rs` 的 `extract` / `il` 子命令）本身是通用机器：它只会照着本文件的
// 三张表干活。lazer 改版时字段改名/挪动，改的就是这里。三张表：
//
// 1. [`ANCHOR_PATTERN`] / [`SITE_DELTAS`] / [`GAME_BASE_HOPS`] / [`GAME_BASE_CONTAINS`]：
//    怎么从 dump 里找到 `GameBase` 对象（纯字节扫描 → 站点 → 多跳解引用 → 类型/vtable 验证；
//    不依赖任何外部表，**绝不**"anchor 减一个常量就是 GameBase"）。
// 2. [`CHAIN`]：从 GameBase 出发**逐级解引用**到我们需要的对象（存储、谱面、规则集……）。
//    每一步都带"期望类型"，类型不符就记 `chain-type-mismatch:<label>` 并跳过该分支——
//    绝不"猜另一个地址"。
// 3. [`WANTED`]：真正要写进偏移表的字段（`(对象标签, 字段名)` + 为什么需要它）。
//    表里每个偏移都必须同时有 SOS 行与 IL 结构行；两边对不上就**丢弃该字段并报告**。
//
// 溯源规则（硬约束，计划 §1 / DEC-02）：这里所有的名字与数字都必须来自**我们自己的
// dump/SOS 提取**（`DECISIONS.md` OPEN-03 的结案路线）或我们自己的 P4/P4b 探针证据
// （`temp/osu-native-memory/evidence/P4/**`、`P4b/**` 与 `probe/lazer-anchors/**`），
// **不是**任何第三方偏移表；任何第三方数字都不得进入本文件或生成的表。
//
// 本文件不含任何**产品偏移值**：产品偏移一律由 `extract` 从 dump 的 SOS 输出里读出来。
// 这里只有**解析链参数**（[`SITE_DELTAS`] 的站点位移、[`GAME_BASE_HOPS`] 的跳数偏移），
// 它们不写进表，只决定"能不能找到 GameBase"。

/// dump 内存里找 `GameBase` 用的锚点字节模式（P4/P4b 实测同一 dump 两种过滤下**各 1 命中**）。
///
/// 语义（我们自己的推导，见 `evidence/P4/lazer-scan-20260927-221455.json` 的 `anchor` 字段）：
/// `osu.Game.OsuGame.ScalingContainerTargetDrawSize` 的 `osuTK.Vector2(1024.0f, 768.0f)` 前
/// 紧跟两个 `0x01` 布尔字节 ⇒ 12 字节模式；命中地址 = 该 Vector2 字段的地址（`anchor`）。
/// 过滤器无关（模式落在 RW 区），所以 dump 里直接扫全部内存范围即可。
/// **命中数不是 1 时**：不要慌，每个命中都会按 [`SITE_DELTAS`] × [`GAME_BASE_HOPS`] 解出候选，
/// 再逐个 `dumpobj` 验证类型（+ 有表时的 vtable）；只有过验证的那一个才算（见 `extract`）。
pub const ANCHOR_PATTERN: &str = "01 01 00 00 00 00 80 44 00 00 40 44";

/// `site` = `anchor - ANCHOR_SITE_DELTA`。**这不是 `GameBase`**：`0x24` 落在中间的**站点**
/// （一个指针字段）上，`GameBase` 要从该站点按 [`GAME_BASE_HOPS`] 多跳解引用才拿得到。
///
/// 实测原话（`evidence/P4/lazer-scan-20260927-220957.txt`）：
/// ```text
/// anchor=0xbf359b84 delta=0x24 site=0xbf359b60 elo=0xbc09aa08 api=0xbec27cf0 gameBase=0xbf359488
///   [gameBase]=0x7ff9e7ef8970 [[gameBase]]=0x73001000000 -> VTABLE MATCH (offsets table for 2026.921.0)
/// resolution summary: candidates_tried=1 deltas_tried=1 resolved=yes gameBase=0xbf359488 delta=0x24
/// external check : expected 0xbf359488 (from the running tosu's own log) vs resolved 0xbf359488 -> EQUAL
/// ```
/// 即：`site = anchor - delta` 读出一个指针（`externalLinkOpener`），再由它跳到 `APIAccess`，
/// 再由 `APIAccess.game` 得到 `GameBase`。
///
/// **历史误读（Step 10A，已纠正）**：旧版把这一行读成"`gameBase = anchor - 0x24 = 0xbf359b90`"、
/// 并写成常量 `GAME_BASE_DELTA = 36`；真机 `extract` 因此对站点本身调 `dumpobj`，SOS 直接回
/// `<Note: this object has an invalid CLASS field>` / `Invalid object`（D-notes §16 有逐字记录）。
/// 误读的根因：P4 那一行里 `site` 与 `gameBase` 同时出现，只看了前者与 delta 的关系。
pub const ANCHOR_SITE_DELTA: i64 = 0x24;

/// 站点位移的候选表（探针逐个试，**由验证决定谁对**，顺序只影响速度）。
///
/// 移植自 `temp/osu-native-memory/probe/lazer-anchors/src/hypotheses.rs::DELTAS`（P4 实测列表）；
/// 第一项 = [`ANCHOR_SITE_DELTA`]。`--anchor-delta <n>` 只把某一项提到最前面，其余仍会被扫。
pub const SITE_DELTAS: &[i64] = &[0x24, 0x28, 0x2c, 0x20, 0x30, 0x1c, 0x34];

/// 从 `site` 到 `GameBase` 的一跳。
pub struct ResolveHop {
    /// 短标签（打印/报告用，与 P4 探针的 `elo`/`api`/`gameBase` 对应）。
    pub label: &'static str,
    /// 在**上一个对象地址**上的字段偏移。第一跳的"上一个对象"就是 `site` 本身：
    /// 偏移 `0` 表示"`site` 处存的就是要解引用的指针"。
    pub offset: u64,
    /// 字段名（照我们自己的证据/探针口径；仅作说明，不参与匹配）。
    pub field: &'static str,
    /// 为什么这么跳（溯源）。
    pub why: &'static str,
}

/// `site → externalLinkOpener → APIAccess.game`（**最后一跳读出 `GameBase`**）。
///
/// 溯源：这两个数字是 P4 探针的假设（`probe/lazer-anchors/src/hypotheses.rs::offsets` 的
/// `EXTERNAL_LINK_OPENER_API = 536` / `API_ACCESS_GAME = 784`），由**我们自己的 P4 运行**
/// 在真机上证明：整条链解通，且解出的 `gameBase` 与运行中的 tosu 自己日志里的地址**逐位相等**
/// （`evidence/P4/lazer-scan-20260927-220957.txt` 的 `resolved=yes` 与 `-> EQUAL` 两行）。
/// 它们是**解析链参数**，不是要发布的产品偏移（产品偏移一律由 `extract` 从 dump 读出）。
/// 改版后若类型/vtable 验证失败：先用 P4 探针的方法重测 delta 与跳数，再改这里。
pub const GAME_BASE_HOPS: &[ResolveHop] = &[
    ResolveHop {
        label: "external_link_opener",
        offset: 0,
        field: "site 处的指针字段（P4 的 `elo`）",
        why: "站点自己就是一个指向 ExternalLinkOpener 的引用字段",
    },
    ResolveHop {
        label: "api_access",
        offset: 536,
        field: "<api>k__BackingField",
        why: "osu.Game.Online.Chat.ExternalLinkOpener.<api> ⇒ osu.Game.Online.API.APIAccess（P4 的 `api`）",
    },
    ResolveHop {
        label: "game",
        offset: 784,
        field: "game",
        why: "osu.Game.Online.API.APIAccess.game ⇒ GameBase（P4 的 `gameBase`，与 tosu 日志 EQUAL）",
    },
];

/// `GameBase` 对象的类型名必须**包含**其中之一（SOS `dumpobj` 的 `Name:` 行）。
///
/// P4b 实测：活对象是 `osu.Desktop.OsuGameDesktop`（`osu!.dll`），`osu.Game.OsuGameBase`
/// 是它的基类名；两者都接受，避免把"派生类改名"当成"锚点失效"。
///
/// **候选验证的两条硬规则**（`chain::candidate_verdict`；任一不过 ⇒ 该候选被拒绝，绝不退而求其次）：
/// 1. `dumpobj` 出来的 `Name:` 必须含本表的某个子串；
/// 2. **给了期望 vtable 时**（`--expect-vtable` / `--table <json>` / `$MMA_LAZER_OFFSETS`）：
///    `[gameBase]`（dump 里 `gameBase` 地址上的首 qword，也就是 SOS 打印的 `MethodTable`）
///    必须等于其中之一——这条与产品侧 `osu/lazer.rs` 的 L1 结构证明同一口径；
///    没有表可给时这一步记成"不适用"，但类型判据仍然必须过。
pub const GAME_BASE_CONTAINS: &[&str] = &["osu.Desktop.OsuGameDesktop", "osu.Game.OsuGameBase"];

/// 解引用链上的一步。
pub struct ChainStep {
    /// 短标签（中间件、报告、表的类型键都按它指代对象）。
    pub label: &'static str,
    /// 父对象标签；`None` = 这一步的对象就是 `GameBase` 本身。
    pub parent: Option<&'static str>,
    /// 在父对象上读的字段名（逐字照 CLR 元数据：SOS 打印的那个名字）。
    pub field: Option<&'static str>,
    /// 解引用出来的对象类型名必须**包含**所有这些子串（SOS `Name:` 行）。
    /// 用"包含"而不是相等：泛型实例化名字里带程序集限定后缀，跨版本可能换写法。
    pub contains: &'static [&'static str],
    /// `true` = 这一步缺席会让"这一层没数据"，必须在报告里显著标出。
    pub required: bool,
    /// 这一步为什么存在（报告与 README 用）。
    pub why: &'static str,
}

/// 从 `GameBase` 出发的解引用链（每一步 = 一次 `dumpobj`）。
///
/// 字段名与类型名**全部**来自我们自己的 P4b 证据
/// （`temp/osu-native-memory/evidence/P4b/dump-offsets-sos-20260927-225957.txt`，
/// 其中的 `sos-dumpobj*.txt` 段）：`<Storage>`@0x440、`<UnderlyingStorage>`@0x10、
/// `<BasePath>`@0x8、`<Beatmap>`@0x450、`NonNullableBindable<WorkingBeatmap>` 的
/// `value`@0x20、`BeatmapManagerWorkingBeatmap.BeatmapInfo`@0x8、
/// `BeatmapInfo.<Metadata>`@0x30 / `<Difficulty>`@0x28 / `<BeatmapSet>`@0x48、
/// `BeatmapMetadata.<Author>`@0x38（`RealmUser`）、`<ScreenStack>`@0x620。
pub const CHAIN: &[ChainStep] = &[
    ChainStep {
        label: "game",
        parent: None,
        field: None,
        contains: GAME_BASE_CONTAINS,
        required: true,
        why: "锚点解出的 GameBase 对象；所有其它对象都从它出发",
    },
    ChainStep {
        label: "storage",
        parent: Some("game"),
        field: Some("<Storage>k__BackingField"),
        contains: &["osu.Game.IO.OsuStorage"],
        required: true,
        why: "lazer 存储根：BasePath(= D:\\Games\\osu!lazer) 与 UnderlyingStorage（P7：folders.songs = BasePath + \"\\files\"）",
    },
    ChainStep {
        label: "desktop_storage",
        parent: Some("storage"),
        field: Some("<UnderlyingStorage>k__BackingField"),
        contains: &["osu.Framework.Platform.DesktopStorage"],
        required: false,
        why: "桌面存储实现；P4b 8/8 验证项之一（BasePath@0x8）",
    },
    ChainStep {
        label: "storage_base_path",
        parent: Some("storage"),
        field: Some("<BasePath>k__BackingField"),
        contains: &["System.String"],
        required: false,
        why: "`System.String` 的实例：_stringLength@0x08 / _firstChar@0x0C 的布局见证（计划 §3.4 的字符串读法）",
    },
    ChainStep {
        label: "desktop_base_path",
        parent: Some("desktop_storage"),
        field: Some("<BasePath>k__BackingField"),
        contains: &["System.String"],
        required: false,
        why: "同上（另一条路径拿到字符串实例；P4b 8/8 的第 7 项）",
    },
    ChainStep {
        label: "beatmap_bindable",
        parent: Some("game"),
        field: Some("<Beatmap>k__BackingField"),
        contains: &[
            "osu.Framework.Bindables.NonNullableBindable`1",
            "osu.Game.Beatmaps.WorkingBeatmap",
        ],
        required: true,
        why: "当前谱面持有者（Bindable<WorkingBeatmap>）；OPEN-03 的修正就在这里：`value`@+0x20（不是 +0x40，且按实例化取值）",
    },
    ChainStep {
        label: "working_beatmap",
        parent: Some("beatmap_bindable"),
        field: Some("value"),
        contains: &["osu.Game.Beatmaps.WorkingBeatmapCache+BeatmapManagerWorkingBeatmap"],
        required: true,
        why: "BeatmapInfo / BeatmapSetInfo 的持有者（嵌套类型，名字里带 `+`）",
    },
    ChainStep {
        label: "beatmap_info",
        parent: Some("working_beatmap"),
        field: Some("BeatmapInfo"),
        contains: &["osu.Game.Beatmaps.BeatmapInfo"],
        required: true,
        why: "identity 的唯一来源：MD5Hash / DifficultyName / Metadata / Difficulty / BeatmapSet",
    },
    ChainStep {
        label: "metadata",
        parent: Some("beatmap_info"),
        field: Some("<Metadata>k__BackingField"),
        contains: &["osu.Game.Beatmaps.BeatmapMetadata"],
        required: true,
        why: "artist / title / author / audio / background（P4b 指出 <Author> 不是字符串而是 RealmUser）",
    },
    ChainStep {
        label: "difficulty",
        parent: Some("beatmap_info"),
        field: Some("<Difficulty>k__BackingField"),
        contains: &["osu.Game.Beatmaps.BeatmapDifficulty"],
        required: false,
        why: "CS/OD/AR/HP（选歌界面之外的兜底）",
    },
    ChainStep {
        label: "beatmap_set",
        parent: Some("beatmap_info"),
        field: Some("<BeatmapSet>k__BackingField"),
        contains: &["osu.Game.Beatmaps.BeatmapSetInfo"],
        required: true,
        why: "beatmap.set（谱面集 id）：BeatmapSetInfo.<OnlineID>",
    },
    ChainStep {
        label: "set_metadata",
        parent: Some("beatmap_set"),
        field: Some("<Metadata>k__BackingField"),
        contains: &["osu.Game.Beatmaps.BeatmapMetadata"],
        required: false,
        why: "谱面集级 artist/title（与单图级 metadata 分开，选歌界面用的是集级）",
    },
    ChainStep {
        label: "realm_user",
        parent: Some("metadata"),
        field: Some("<Author>k__BackingField"),
        contains: &["osu.Game.Models.RealmUser"],
        required: false,
        why: "mapper（tosu 的 beatmap.mapper）：RealmUser.<Username>@0x18",
    },
    ChainStep {
        label: "beatmap_clock",
        parent: Some("game"),
        field: Some("beatmapClock"),
        contains: &["osu.Game.Beatmaps.FramedBeatmapClock"],
        required: false,
        why: "游戏级谱面时钟（`OsuGameBase.beatmapClock`，Step 10e 新增）：它的播放位置就是 \
              `beatmap.time.live` 的来源。字段名与类型名逐字来自**本次 dump 的 dumpobj**\
              （game 的字段表里 `beatmapClock`@0x4D8，对象类型 osu.Game.Beatmaps.FramedBeatmapClock）\
              与同一构建的 IL 清单（osu.Game.dll / osu.Framework.dll）",
    },
    ChainStep {
        label: "beatmap_clock_time",
        parent: Some("beatmap_clock"),
        field: Some("interpolatedTrack"),
        contains: &["osu.Framework.Timing.InterpolatingFramedClock"],
        required: false,
        why: "时钟链上的**插值音轨时钟**（`FramedBeatmapClock.interpolatedTrack`）：\
              它的 `<CurrentTime>k__BackingField`（double，毫秒）就是活的播放位置。\
              同一对象链上还有 `decoupledTrack`（DecouplingFramedClock，未插值）与 \
              `finalClockSource`（FramedOffsetClock，带用户偏移）——三者实测相差几十毫秒，\
              取**插值**这一枚（它才是 tosu 口径的 live 播放位置）",
    },
    ChainStep {
        label: "screen_stack",
        parent: Some("game"),
        field: Some("<ScreenStack>k__BackingField"),
        contains: &["osu.Game.Screens.OsuScreenStack"],
        required: true,
        why: "state.name 的屏幕栈来源（P4b 8/8 的第 3 项）",
    },
    ChainStep {
        label: "screen_stack_stack",
        parent: Some("screen_stack"),
        field: Some("stack"),
        contains: &[
            "System.Collections.Generic.Stack`1",
            "osu.Framework.Screens.IScreen",
        ],
        required: false,
        why: "屏幕栈的**元素容器**（Step 10f）：`OsuScreenStack.stack` 的运行时类型是 \
              `System.Collections.Generic.Stack`1[[osu.Framework.Screens.IScreen, osu.Framework]]`\
              （本次 dump 的 `dumpobj` 逐字），它的 `_array`/`_size` 是「当前屏幕」的唯一来源\
              （`Stack<T>.Push` 写在 `_array[_size++]` ⇒ 栈顶 = `_array[_size-1]`）。\
              IL 侧同名同形：`System.Collections.dll` 的 `Stack`1._array` / `._size`",
    },
    ChainStep {
        label: "screen_array",
        parent: Some("screen_stack_stack"),
        field: Some("_array"),
        contains: &["osu.Framework.Screens.IScreen[]"],
        required: false,
        why: "屏幕对象数组（Step 10f）：`dumpobj` 对数组只回 `Fields: None`，元素布局由 \
              `dumparray` 的 `[i] <addr>` 行见证（见 `runtime.rs` 的 `screen_array` 推导）",
    },
    ChainStep {
        label: "selected_mods",
        parent: Some("game"),
        field: Some("SelectedMods"),
        // 真机元数据（`il` 对 2026.921.0.0 的 osu.Game.dll 实测）：
        // `OsuGameBase.SelectedMods` 的**声明类型**是
        // `Bindable<IReadOnlyList<Mod>>`；运行时对象是它的某个实例（P4b 的 dump 里该行
        // 的类型列被列宽截断成 `...Private.CoreLib]]`，与嵌套泛型实参的尾巴一致）。
        // 所以这里刻意只要求"是 osu.Framework.Bindables.Bindable* 且实参里有 Mod"——
        // `BindableList<T>` 在 2026.921.0.0 里**不**继承 Bindable（它继承 System.Object，
        // 见 il 清单的 `#extends`），写成 `BindableList`1` 会在真机上必然失败。
        contains: &[
            "osu.Framework.Bindables.Bindable",
            "osu.Game.Rulesets.Mods.Mod",
        ],
        required: true,
        why: "当前 mod 的持有者（`value` 字段的按实例化偏移）；声明类型是 Bindable<IReadOnlyList<Mod>>",
    },
    ChainStep {
        label: "ruleset",
        parent: Some("game"),
        field: Some("Ruleset"),
        contains: &["osu.Framework.Bindables.Bindable`1", "osu.Game.Rulesets.RulesetInfo"],
        required: false,
        why: "当前规则集（Bindable<RulesetInfo> 实例）；`value` 字段的按实例化偏移",
    },
];

/// 要写进偏移表的一个字段：`(链上对象标签, 字段名)`。
pub struct Wanted {
    /// [`CHAIN`] 里的标签。
    pub object: &'static str,
    /// 字段名（逐字照 CLR 元数据）。
    pub field: &'static str,
    /// 为什么需要它（进 `emit-report`，也是 README 表格的来源）。
    pub why: &'static str,
}

/// 出表字段清单。
///
/// ### Step 10e：这份 dump **证不出**的字段（逐条留档，绝不写进 WANTED 当"猜"）
///
/// 现场：dump `lazer-20260929-213211.dmp`（pid 22028）采于**主菜单态**——屏幕栈三枚
/// （`MainMenu` / `IntroScreen` / 另一枚，见 `stack._array` 的 dumpobj）里最上面就是
/// `game.menuScreen`（0xB9394868 = `osu.Game.Screens.Menu.MainMenu`），`SelectedMods.value`
/// 是**空表**（`List<Mod>` 的 `_size=0`），`dumpheap` 里没有 `Player` / `ScoreInfo` /
/// `ScoreProcessor`。所以：
///
/// | 字段 | 为什么证不出（本 dump 的机械理由） |
/// |---|---|
/// | `state.name` / `state.number` | 屏幕栈里只有**对象地址**。唯一的字符串候选 `osu.Framework.Graphics.Drawable.Name`（`@0xD8`）在**三枚屏幕对象上都是 `String.Empty`**（实测 0xBF809018，`_stringLength=0`）；运行时类型名只能靠 MethodTable，而 MT 落在**每进程的 loader heap**（本 dump：MT 0x7FFE2F2D8970 的 Module 是 0x7FFE2EFA8B20，**不在** osu!.dll 映像 0x1D707720000 里）⇒ "屏幕 MT → 状态名"的见证表跨进程不可用 |
/// | `play.mods` / `menu.mods` / `resultsScreen.mods` | `acronym` **不是字段**：IL 清单里 `osu.Game.Rulesets.Mods.Mod` 只有 `settingsBacking`，各具体 mod 只有各自的设置 bindable（`<SpeedChange>` 等），全库**没有任何** `Acronym` 字段（唯一同名的是 osu.Game.Tournament 的无关类型）⇒ 只能读 `ScoreInfo.<ModsJson>`（acronym 的 JSON 串），而本 dump 无 `ScoreInfo` |
/// | `play.hits` / `resultsScreen.hits` | 计数域在 `ScoreInfo` / `ScoreProcessor` 上（`<StatisticsJson>` / `ScoreResultCounts`），本 dump 这两个类型**一个活对象都没有**（菜单态） |
/// | `files.background` / `files.audio` | 需要 `BeatmapSetInfo.<Files>`（`List<RealmNamedFileUsage>`）的**元素**：`Filename` → `File.Hash` 才能拼出 `h\hh\<64hex>`。链能走到**列表对象**（`<Files>`@0x20，实测 0x F26D7D08），但元素是**数组槽**（`_items`），`dumpobj` 对数组只回 `Fields: None`⇒ 要读元素就得有"数组元素布局"的见证（表格式扩展），本步不做（见 README「已知边界」） |
///
/// 这三条都是**结构性的**（与"这次 dump 采在哪个态"无关的那一条是 mods）；所以它们不是
/// "再采一份 dump 就好"，而是需要表格式扩展或换见证源。
///
/// ### Step 10f：`state.name` / `state.number`（EEType → 类型名）
///
/// 10B/10e 的结论（逐字留在 [`RUNTIME_PROBES`] 的注释里）是"屏幕对象上的名字字段是
/// `String.Empty`、类型名只能从 MethodTable 出发，而 MT 落在每进程的 loader heap"。
/// **对地址成立，对 MT 的内容不成立**：MT 头部里的 `TypeDef RID` 与"哪个程序集"
/// （loader Module → 映像基址）只依赖**构建**，所以 10f 把这条链拆成
/// "运行期结构位移（dump 见证）" + "构建期 RID→类型名（IL 元数据见证）"两半，
/// 都写进表的 `runtime` 段（见 [`RUNTIME_PROBES`]）。
///
/// 分三块：
/// - **P4b 8/8 验证项**（`<Storage>` / `<VersionHash>` / `<ScreenStack>` / `<Host>` /
///   `<Beatmap>` / `<UnderlyingStorage>` / `DesktopStorage.<BasePath>` / `System.String` 布局）：
///   这些字段的偏移在 P4b 里与运行期实读逐个比对过，是本表的"地基"。
/// - **identity / 元数据**（MD5Hash / OnlineID / DifficultyName / Metadata / Difficulty /
///   BeatmapSet / RealmUser.Username / artist / title / audio / background）：计划 §3.3
///   字段表里 lazer 侧必需的载荷来源。
/// - **持有者布局**（bindable 的 `value`、BindableList 的 `value`、WorkingBeatmap 持有者、
///   屏幕栈的 `stack`）：part B（`osu/lazer.rs`）解引用时要用的每实例化偏移。
pub const WANTED: &[Wanted] = &[
    // ---- P4b 8/8 ----
    Wanted { object: "game", field: "<Storage>k__BackingField", why: "P4b#1：存储根对象（OsuStorage）" },
    Wanted { object: "game", field: "<VersionHash>k__BackingField", why: "P4b#2：本体版本哈希字符串（System.String）" },
    Wanted { object: "game", field: "<ScreenStack>k__BackingField", why: "P4b#3：屏幕栈（state.name 的来源）" },
    Wanted { object: "game", field: "<Host>k__BackingField", why: "P4b#4：GameHost" },
    Wanted { object: "game", field: "<Beatmap>k__BackingField", why: "P4b#5：当前谱面 Bindable" },
    Wanted { object: "storage", field: "<UnderlyingStorage>k__BackingField", why: "P4b#6：底层 Storage" },
    Wanted { object: "storage", field: "<BasePath>k__BackingField", why: "lazer 存储根（P7：folders.songs = BasePath + \"\\files\"）；与 desktop_storage 的 <BasePath> 同址" },
    Wanted { object: "desktop_storage", field: "<BasePath>k__BackingField", why: "P4b#7：存储根路径字符串" },
    Wanted { object: "storage_base_path", field: "_stringLength", why: "P4b#8：System.String 长度字段（计划 §3.4 的字符串读法）" },
    Wanted { object: "storage_base_path", field: "_firstChar", why: "P4b#8：System.String 首字符字段（UTF-16 起点）" },

    // ---- identity / 元数据（计划 §3.3 的 lazer 侧载荷来源）----
    Wanted { object: "beatmap_info", field: "<MD5Hash>k__BackingField", why: "beatmap.md5 / identity 的第一来源（A5/P5：32-hex，不等于文件仓名的 64-hex）" },
    Wanted { object: "beatmap_info", field: "<Hash>k__BackingField", why: "文件仓键（64-hex）= files.beatmap 的来源" },
    Wanted { object: "beatmap_info", field: "<OnlineID>k__BackingField", why: "beatmap.id" },
    Wanted { object: "beatmap_info", field: "<DifficultyName>k__BackingField", why: "beatmap.version" },
    Wanted { object: "beatmap_info", field: "<Metadata>k__BackingField", why: "artist/title/author 的持有者" },
    Wanted { object: "beatmap_info", field: "<Difficulty>k__BackingField", why: "CS/OD/AR/HP" },
    Wanted { object: "beatmap_info", field: "<BeatmapSet>k__BackingField", why: "beatmap.set 的持有者" },
    Wanted { object: "beatmap_info", field: "<StarRating>k__BackingField", why: "星数（诊断/对照用）" },
    Wanted { object: "beatmap_info", field: "<Length>k__BackingField", why: "谱面时长（诊断/对照用）" },
    Wanted { object: "metadata", field: "<Title>k__BackingField", why: "beatmap.title" },
    Wanted { object: "metadata", field: "<TitleUnicode>k__BackingField", why: "beatmap.title 的 unicode 形态（页面按 unicode 优先）" },
    Wanted { object: "metadata", field: "<Artist>k__BackingField", why: "beatmap.artist" },
    Wanted { object: "metadata", field: "<ArtistUnicode>k__BackingField", why: "beatmap.artist 的 unicode 形态" },
    Wanted { object: "metadata", field: "<Author>k__BackingField", why: "mapper 的持有者（RealmUser，不是字符串）" },
    Wanted { object: "metadata", field: "<AudioFile>k__BackingField", why: "音频文件名（directPath.audioFile 的候选；页面不强制）" },
    Wanted { object: "metadata", field: "<BackgroundFile>k__BackingField", why: "背景文件名（封面路由用）" },
    Wanted { object: "metadata", field: "<Source>k__BackingField", why: "来源（诊断用）" },
    Wanted { object: "difficulty", field: "<CircleSize>k__BackingField", why: "键数（4K/7K 判定）" },
    Wanted { object: "difficulty", field: "<OverallDifficulty>k__BackingField", why: "OD" },
    Wanted { object: "difficulty", field: "<ApproachRate>k__BackingField", why: "AR" },
    Wanted { object: "difficulty", field: "<DrainRate>k__BackingField", why: "HP" },
    Wanted { object: "realm_user", field: "<Username>k__BackingField", why: "beatmap.mapper" },
    Wanted { object: "beatmap_set", field: "<OnlineID>k__BackingField", why: "beatmap.set（谱面集 id）" },
    Wanted { object: "set_metadata", field: "<Title>k__BackingField", why: "集级 title（选歌界面用的是它）" },
    Wanted { object: "set_metadata", field: "<Artist>k__BackingField", why: "集级 artist" },

    // ---- part B 解引用用的持有者/绑定布局 ----
    Wanted { object: "beatmap_bindable", field: "value", why: "Bindable<T>.value：**按实例化取值**（OPEN-03 修正：NonNullableBindable<WorkingBeatmap> 是 +0x20，+0x40 是 <Description>）" },
    Wanted { object: "beatmap_bindable", field: "<Description>k__BackingField", why: "同一张表的对照项：证明 +0x40 不是 value（若这里与 value 混了，说明按实例化取表这一步错了）" },
    Wanted { object: "working_beatmap", field: "BeatmapInfo", why: "谱面持有者 → BeatmapInfo" },
    Wanted { object: "working_beatmap", field: "BeatmapSetInfo", why: "谱面持有者 → BeatmapSetInfo" },
    Wanted { object: "selected_mods", field: "value", why: "mods 持有者的 value（`Bindable<T>.value`，按实例化取偏移）：当前 mod 列表" },
    Wanted { object: "ruleset", field: "value", why: "Bindable<RulesetInfo>.value：当前规则集（按实例化偏移）" },
    Wanted { object: "screen_stack", field: "stack", why: "屏幕栈的 List<IScreen>（state.name 合成的起点）" },
    Wanted { object: "screen_stack_stack", field: "_array", why: "屏幕栈的元素数组（Step 10f）：`IScreen[]`，栈顶对象从它取（`_array[_size-1]`）" },
    Wanted { object: "screen_stack_stack", field: "_size", why: "屏幕栈的栈深（Step 10f）：与 .NET `Stack<T>.Push` 的 `_array[_size++]` 语义一起决定「当前屏幕」" },
    Wanted { object: "game", field: "beatmapClock", why: "beatmap.time.live 的第 1 跳（Step 10e）：GameBase → FramedBeatmapClock 的持有者字段" },
    Wanted { object: "beatmap_clock", field: "interpolatedTrack", why: "beatmap.time.live 的第 2 跳（Step 10e）：时钟链上的插值音轨时钟（读过它才拿得到 <CurrentTime>）" },
    Wanted { object: "beatmap_clock_time", field: "<CurrentTime>k__BackingField", why: "beatmap.time.live（Step 10e 新增）：谱面音轨的活播放位置（double 毫秒；按 P3 的裁决原样发布、不自除 rate——音轨位置本身就是谱面时间轴）" },
    Wanted { object: "game", field: "SelectedMods", why: "mods 的第 1 跳（持有者）：`Bindable<IReadOnlyList<Mod>>` 对象（`value` 按实例化 +0x20 才是列表）。**acronym 本身不是字段**（见上面 WANTED 文档里的表缺口说明）⇒ 这一跳只把链路解到持有者，读侧据此对 play/menu/resultsScreen 的 mods 逐条降级并写出精确原因" },
];

// --------------------------------------------------- 运行期结构（Step 10f）----
//
// 这一段是**导出规则**，不是导出值：每个位移都由 `runtime.rs` 在真 dump 上**搜出来**
// （SOS 的打印值 vs dump 字节逐字比对 + 多个探针必须给出同一个位移），然后写进表的
// `runtime` 段。规格里只有"搜什么、在哪搜、凭什么算对"。
//
// ## 为什么必须是"搜"而不是写一个常量
//
// 10B/10e 的结论仍然成立：**屏幕上没有任何字段携带类型名**——三枚屏幕对象的
// `osu.Framework.Graphics.Drawable.Name`（`@0xD8`）都是 `String.Empty`（实测
// `0xBF809018`，`_stringLength=0`）；而 SOS 能印出 `osu.Game.Screens.Menu.MainMenu`
// 是因为它从 **MT → Module → 元数据** 里读的。类型名不是"对象上的字段"，
// 所以只能：**MT 的 TypeDef RID + MT 的 loader Module → 映像基址 → （运行期模块表）模块名**
// + **构建期的 RID→类型名**（IL 元数据，`il` 子命令的 `type` 行）。
//
// ## 这份 dump 证不出什么（逐字留档，避免下一步重复踩）
//
// `lazer-20260929-213211.dmp` 是 `--type Heap` 的 dump：**映像页基本不在 dump 里**。实测：
// 全量扫过 2 691 419 736 字节，`k__BackingField` / `osu.Game.Screens.Menu` /
// `OsuGameDesktop`（类型名的命名空间/名字串）**0 命中**；`osu.Game.dll` 的映像里只有
// 0x40 字节的 PE 头 + 若干零散页（`range_for(0x1D707F60000).size == 0x40`），
// `dumpmodule` 印的 `MetaData start address: 000001D7081BE4CC` 在 dump 里**读不到**
// （"in no range"）。⇒ **"在 dump 里定位名字串再反推指针"这条路在本 dump 上不存在**
// （SOS 打印名字靠的是宿主机上的程序集文件，不是 dump 字节）。
//
// 能见证的是**结构位移**：MT 头部里那个"打包的 TypeDef RID"字段与 loader Module 指针、
// Module 里的映像基址，以及数组对象的元素布局——三样都在 dump 里有 SOS 的独立打印值可比。
//
// ## 探针（`runtime.rs`）
//
// | 组.名 | SOS 打印（互证） | dump 字节比对 | 一致性要求 |
// |---|---|---|---|
// | `eetype.token` | `dumpmt <MT>` 的 `mdToken:`（取 `& 0x00FFFFFF` 得 RID） | `u32@MT+off >> shift == RID` | 同一位移在**全部**被探 MT 上成立，且**唯一** |
// | `eetype.loader_module` | `dumpmt <MT>` 的 `Module:` | `u64@MT+off == Module` | 同上（含**跨模块**：osu!.dll 与 osu.Game.dll 都要过） |
// | `module.image_base` | `dumpmodule <Module>` 的 `BaseAddress:` | `u64@Module+off == 基址` | 同一位移在**全部**被探 Module 上成立，且唯一 |
// | `screen_array.length` | `dumparray <array>` 的 `Array: … Number of elements N` | `u32@array+off == N` | 唯一 |
// | `screen_array.elements` | `dumparray <array>` 的 `[i] <addr>` 行 | `u64@array+data+i*stride == 该地址` | `(data, stride)` 唯一 |
//
// 被探 MT 的集合 = **屏幕栈里的每一枚屏幕**（`stack._array[0.._size]`）＋ `GameBase` 的 MT：
// 屏幕给"很多个 RID、同一个位移"，GameBase 给"另一个模块（osu!.dll）也要过同一个位移"。
// 任何一条不一致 ⇒ **不发布**该位移（并写进报告），读侧随后按字段级降级。
pub struct RuntimeProbe {
    /// 表里 `runtime.<group>.<name>` 的组名。
    pub group: &'static str,
    /// 表里 `runtime.<group>.<name>` 的名字。
    pub name: &'static str,
    /// 在对象里的搜索区间（起始、结束，排他）。
    pub scan: (usize, usize),
    /// 步长（`u32` 按 2、`u64` 按 8）。
    pub align: usize,
    /// 读出来的整数要右移多少位（只有 `token` 是 8；其余为 0）。
    pub shift: u32,
    /// 为什么这么搜（报告与 README 用）。
    pub why: &'static str,
}

/// 运行期结构的导出规则（见上面的表；**位移由 dump 搜出来，不写常量**）。
pub const RUNTIME_PROBES: &[RuntimeProbe] = &[
    RuntimeProbe {
        group: "eetype",
        name: "token",
        scan: (0x00, 0x40),
        align: 2,
        shift: 8,
        why: "MethodTable 头部打包的 TypeDef RID：`dumpmt` 的 `mdToken` 与 `u32@MT+off >> 8` \
              必须相等（实测 off=0x08：`0x0009A204 >> 8 == 0x09A2` == mdToken 的 RID）",
    },
    RuntimeProbe {
        group: "eetype",
        name: "loader_module",
        scan: (0x00, 0x80),
        align: 8,
        shift: 0,
        why: "MethodTable 的 loader Module 指针：`dumpmt` 的 `Module:` 与 `u64@MT+off` 相等\
              （实测 off=0x18，且在 osu!.dll / osu.Game.dll 两个模块的类型上都成立）",
    },
    RuntimeProbe {
        group: "module",
        name: "image_base",
        scan: (0x00, 0x400),
        align: 8,
        shift: 0,
        why: "Module 对象上的映像基址：`dumpmodule` 的 `BaseAddress:` 与 `u64@Module+off` 相等\
              （实测 off=0xC8，osu!.dll 与 osu.Game.dll 两个 Module 都成立）。\
              基址每进程不同 ⇒ 它是**运行期读数**：读侧拿它去目标的模块表（Toolhelp32）反查模块名",
    },
    RuntimeProbe {
        group: "screen_array",
        name: "length",
        scan: (0x08, 0x20),
        align: 4,
        shift: 0,
        why: "数组对象的元素个数：`dumparray` 的 `Array: … Number of elements N` 与 `u32@array+off` 相等",
    },
    RuntimeProbe {
        group: "screen_array",
        name: "elements",
        scan: (0x08, 0x20),
        align: 8,
        shift: 0,
        why: "数组元素的数据起点（`data`）与步长（`stride`）：`dumparray` 的 `[i] <addr>` 行必须\
              满足 `u64@array+data+i*stride == addr`（引用类型数组的步长 = 指针宽度 8）",
    },
];

/// 被探 MT 的数量下限（只有 1 个探针时"唯一"没有意义：任意位移都能凑一个）。
pub const RUNTIME_MIN_PROBES: usize = 2;

/// `runtime.typedefs` 的**收录规则**：只收"屏幕实现"的类型名。
///
/// 为什么不全量收：`state.name` 只可能来自**当前屏幕**的类型，而 lazer 的屏幕实现都在
/// `osu.Game.Screens.*` / `osu.Framework.Screens.*` 两个命名空间下（`IScreen` 的实现者）；
/// 全量收会把表变成"整个 osu.Game 的 TypeDef 清单"（数千条）却不增加任何可读性。
/// **不在收录集里**的类型 ⇒ 读侧按字段级降级（`type-unresolved:<模块>#<RID>`），绝不猜。
/// 另外：**每一次 `dumpmt` 直接印出来的类型都强制收录**（哪怕命名空间不在上面两条里）——
/// 那些是"双见证"的（SOS 打印 + dump 字节）。
pub const RUNTIME_TYPEDEF_NAMESPACES: &[&str] = &["osu.Game.Screens.", "osu.Framework.Screens."];