# lazer-offsets-gen —— osu!lazer 偏移表生成器（dev-time 工具）

> **English TL;DR** — A dev-time, in-repo generator for the `osu!lazer` offset table that
> `desktop/src/osu/offsets.rs` loads. It (1) collects a dump of the running lazer process with
> `dotnet-dump collect`, (2) scans the dump for our own anchor and drives
> `dotnet-dump analyze` + SOS (`dumpobj`) along a dereference chain, (3) reads the shipped CLR
> assemblies' metadata as a **second structural witness**, and (4) emits the JSON table only for
> fields that have **both** an SOS line and an IL structural line; any disagreement drops the field
> and lists it in the report. Bare `rustc`, zero dependencies, never part of the product, never
> writes to the game. Manual is Chinese below (this repo's `tools/README.md` convention).

## 这个工具是做什么的（What it does）

lazer 是**每周级更新**的游戏；我们的内存读取依赖 `Type.Field → offset` 的表格，而这张表
**只能从目标构建自己身上提取**（`DECISIONS.md` OPEN-03：ClrMD 读不了本机 lazer 的自包含
dump，走 SOS 提取 + IL 差分 + 结构证明）。本工具就是那次提取：它是**每次 lazer 更新都要
跑一次**的维护工具，所以**入库**（DEC-23 取代 DEC-16 ②；`temp/` 里的工具会随验收一起被删，
"≤1 天恢复"就会变成"先重建工具再计时"）。

它不是产品的一部分：产品侧一律走 `ReadProcessMemory`（只读），dump 采集**只**存在于本工具。

## 前置条件（Prerequisites）

| 项 | 要求 | 怎么满足 |
|---|---|---|
| 平台 | Windows x64 | 只支持 Windows（进程发现 + dump 都是 Windows 形态） |
| `dotnet-dump` | ≥ 10.0.745401（我们验证过的版本） | `& 'C:\Program Files\dotnet\dotnet.exe' tool install --global dotnet-dump`（装到 `%USERPROFILE%\.dotnet\tools\`，工具会自动找；也可 `--analyzer` / `MMA_DOTNET_DUMP` 指定） |
| 游戏 | osu!lazer（x64）**在跑** | `collect` 第一步就要它；stable 是 32 位，工具按位数区分（都叫 `osu!.exe`） |
| 磁盘 | dump 卷 ≥ 4 GB 空闲 | 实测 dump **2.96 GiB**（3,174,352,952 B）；`--force` 可跳过检查 |
| 时间 | 一次采集约 **70 s** 游戏挂起 | 这是 `dotnet-dump collect` 写 dump 的时间（实测 69.97 s），**不是** 1–3 s |
| 权限 | 与游戏同用户 | 读内存/采 dump 都不需要管理员；工具不注入、不写游戏、不挂调试器 |

**⚠️ 运行位置（真机实测）**：在受限的工作区上下文里 `Toolhelp32` 只能看到自己那一族进程
（与 B2 记录同族），表现为"游戏没在跑"。**从工作区外（如 `%TEMP%`）或普通 shell 启动本工具**
即可正常发现游戏 —— 与 `mma-shell.exe` 的 B2 结论一致。

## 端到端命令序列（lazer 正在跑）

```powershell
# 0) 构建（在仓库根目录；--edition 2021 必需，-A dead_code 因为各子命令只用部分模块）
rustc --edition 2021 -O -A dead_code -o temp/lazer-offsets-gen.exe tools/lazer-offsets-gen/main.rs

# 1) 看清要执行的命令（不碰游戏）；会打印发现的 osu!.exe（pid/位数/路径）
temp/lazer-offsets-gen.exe collect --out %TEMP%\lazer-offsets-gen\run1 --dry-run

# 2) 采 dump（~70 s 游戏挂起、~3 GB）
temp/lazer-offsets-gen.exe collect --out %TEMP%\lazer-offsets-gen\run1

# 3) 离线提取：扫锚点 → 站点 → 多跳解出 GameBase → dumpobj 逐级解引用 → SOS 中间件（含原始 transcript）
#    （已经有表时可以加 --table <表.json> / --expect-vtable <hex> / $MMA_LAZER_OFFSETS，
#      让候选验证连 `[gameBase] == 表 vtable` 一起做）
temp/lazer-offsets-gen.exe extract --dump %TEMP%\lazer-offsets-gen\run1\lazer-<ts>.dmp `
                                   --out %TEMP%\lazer-offsets-gen\run1

# 4) IL 结构清单（读 lazer 安装目录里的托管程序集；--sos 可从中间件里取 lazer_dir）
temp/lazer-offsets-gen.exe il --sos %TEMP%\lazer-offsets-gen\run1\sos-intermediate-<ts>.tsv `
                              --out %TEMP%\lazer-offsets-gen\run1 `
                              --diff <上一次的 il-inventory.tsv>      # 跨版本 diff（可选但推荐）

# 5) 双见证校验 + 出表，并直接放到壳 exe 旁的 lazer-offsets\
#    ⚠ 自包含发布的 osu!.dll / osu!.exe 文件版本是占位 0.0.0.0：这个构建上 emit 会先拒一次，
#      按提示加 --allow-version-mismatch（不一致会写进表里的 evidence）
temp/lazer-offsets-gen.exe emit --sos %TEMP%\lazer-offsets-gen\run1\sos-intermediate-<ts>.tsv `
                                --il  %TEMP%\lazer-offsets-gen\run1\il-inventory-<ts>.tsv `
                                --deploy "<放着 mma-shell.exe 的目录>" `
                                --allow-version-mismatch
```

每一步的产物都落在 `--out` 目录里，**每个数字都可回溯**：

```
collect-<ts>.log                       dotnet-dump 的原始输出
sos-commands / sos-<layer>.cmd.txt     逐字命令（-c "dumpobj 0x…"）
sos-<layer>.out.txt / .err.txt         分析器的原始 stdout/stderr
sos-<layer>.txt                        合并 transcript（中间件里按文件名引用它）
sos-intermediate-<ts>.tsv              SOS 中间件（机器可读；见下）
extract-report-<ts>.txt                这次提取的全量读数 + 链步骤状态 + notes
il-inventory-<ts>.tsv                  IL 结构清单（含 #extends 继承链）
il-diff-<ts>.txt                       与上一版清单的字段增删改
EXAMPLE-FIXTURE-*.json / emit-report-*.txt   自测产物（见"自测"）
lazer-offsets/<ver>__<rt>__<arch>.json 出表（默认落点；--out / --deploy 可改）
```

## 输出格式（Output format）

### 1. SOS 中间件 `sos-intermediate-*.tsv`

`#` 行是溯源键值（`#<TAB>key<TAB>value`），数据行两类：

```
# lazer-offsets-gen sos-intermediate v1
#	provenance	dump                ← dump | fixture（fixture 必须显式标记）
#	dump	C:\…\lazer-<ts>.dmp
#	dump_pid	35420
#	arch	x64
#	lazer_version	2026.921.0.0       ← 来源见 lazer_version_source（runtime-log|dump-module|operator）
#	runtime_version	10.0.12            ← runtime-log | runtimeconfig | operator
#	lazer_exe	C:\…\current\osu!.exe
#	lazer_exe_sha256	<64 hex | unavailable>   ← certutil -hashfile（工具不自造密码学）
#	analyzer	C:\…\dotnet-dump.exe (dotnet-dump 10.0.745401)
#	anchor_pattern	01 01 00 00 00 00 80 44 00 00 40 44
#	anchor	0xbf359b84        #	anchor_hits	1
#	anchor_site_delta	36        #	resolve_site	0xbf359b60
#	resolve_hops	external_link_opener(+0x0 …) -> api_access(+0x218 …) -> game(+0x310 …)
#	resolve_attempts	7         #	resolve_candidates	2
#	resolve_vtable_check	passed: [gameBase]=0x… is one of the supplied table vtable(s) 0x…
#	game_base	0xbf359488        #	game_base_mt	0x7ff9e7ef8970
#	deref_checked	214   #	deref_ok	214   #	deref_mismatch	0
#	transcript	sos-L0-…-n1.txt; sos-L1-…-n4.txt; …
object	game	0xBF000FDC	osu.Desktop.OsuGameDesktop	ok	anchor -> game_base (type verified by dumpobj)
field	0xBF000FDC	osu.Desktop.OsuGameDesktop	<Storage>k__BackingField	1088	....Platform.Storage	No	instance	00000000bf000200	osu!.dll	040000ff	ok-ref
```

`field` 行 12 列：`地址 对象类型 字段名 偏移 SOS类型 VT列 Attr列 Value列 模块 token 解引用见证`。
`object` 行的 `status`：`ok` / `chain-type-mismatch:…` / `chain-no-field:…` / `chain-null:…` /
`chain-unreadable:…` / `chain-skipped:<父>-<原因>`。

**GameBase 解析不是"anchor 减一个常量"**：`site = anchor − anchor_site_delta`（`spec::ANCHOR_SITE_DELTA`
= 0x24）只是**中间站点**，`GameBase` 由站点按 `spec::GAME_BASE_HOPS` 多跳解出
（`+0x0` 站点指针 → `+0x218` ExternalLinkOpener.`<api>` → `+0x310` APIAccess.`game`）；
每一个 `(anchor, delta)` 尝试都逐字打印，每个解出的候选都要过**两条硬判据**
（① `dumpobj` 类型含 `osu.Desktop.OsuGameDesktop`/`osu.Game.OsuGameBase`；
② 给了表 vtable 时 `[gameBase]` 必须等于它）——任一不过就拒绝，绝不退而求其次。
`--anchor-delta <n>` 只把某个站点位移提到最前，其余位移仍会被扫；`--expect-vtable <hex>` /
`--table <json>` / `$MMA_LAZER_OFFSETS` 提供第 ② 条要比的 vtable（都不给 ⇒ 如实记"不适用"）。

**中间件只来自 `extract`，绝不手改**：`emit` 只认 `provenance=dump`（且 `deref_checked>0`）
或**显式标记的 fixture**。改 `spec.rs::WANTED` 之后可以**只重跑 `emit`**（中间件里存了
每个被 dump 对象的**全部**字段行，不必重新采 dump）。

### 2. IL 清单 `il-inventory-*.tsv`

```
#	assembly:osu.Game.dll	2026.921.0.0	24227 fields	C:\…\current\osu.Game.dll
extends	osu.Desktop.OsuGameDesktop	osu.Game.OsuGame      ← 继承链（派生 → 基，开放泛型形态）
field	osu.Game.dll	osu.Game.OsuGameBase	<Storage>k__BackingField	instance	class	osu.Framework.Platform.Storage	040000FF	-	22
```

`field` 行 10 列：`程序集 类型 字段 实例/静态 类型种类 类型显示名 token 显式偏移 声明顺序`。
类型名是**开放泛型定义**（元数据里就只有这个名字）。

### 3. 出表 JSON（`offsets.rs::load` 的形状）

```json
{
  "lazer_version": "2026.921.0.0",
  "runtime_version": "10.0.12",
  "arch": "x64",
  "game_base_vtable": 140711314819440,
  "types": {
    "osu.Game.Beatmaps.BeatmapInfo": { "<MD5Hash>k__BackingField": 88 },
    "osu.Framework.Bindables.NonNullableBindable`1<osu.Game.Beatmaps.WorkingBeatmap>": { "value": 32 }
  },
  "verified_build": "osu!lazer 2026.921.0.0 / .NET 10.0.12 / x64 / …\\current\\osu!.exe sha256:<64 hex> / game_base vtable 0x7ff9e7ef8970",
  "evidence": "SOS offsets from dump `…` (pid …, x64, anchor 1 hit(s) at 0x…, game_base 0x…), transcript `…`; SOS offsets proven against the dump bytes (214 checked / 214 ok / 0 mismatch …); IL structure from assemblies under `…` (259 managed); …"
}
```

- **类型键 = 对象的运行时类型**（`dumpobj` 的 `Name:` 行）规范化后的形态；**泛型实例化按实例化
  分开存**（`Bindable<T>.value` 的偏移**依赖 T**：`NonNullableBindable<WorkingBeatmap>` 是
  `+0x20`，`+0x40` 是 `<Description>`；这是 OPEN-03 的修正，IL 侧独立佐证了它）。
- **读取侧要用同一个规范化规则**：`Ns.Type\`1[[Arg, Asm]]` → ``Ns.Type`1<Arg>``（去程序集限定与
  空白；被 SOS 列宽截断的串只保留去空白原文）。IL 侧的对照键是**开放泛型定义**（把 `<…>` 去掉）。
- 泛型 `Type.Field` 缺对应实例化时**必须按字段降级**（`degradedFields`），不得猜另一个实例化。

## 表落点与读取侧的回落梯（reader ladder）

```powershell
# 默认落点：./lazer-offsets/<lazer>__<runtime>__<arch>.json（--out 可改成任意文件）
# 推荐：直接放到壳 exe 旁，读取侧按下面的梯子找
temp/lazer-offsets-gen.exe emit --sos … --il … --deploy "<mma-shell.exe 所在目录>"
#                                                → <该目录>\lazer-offsets\<同名文件>
```

读取侧（Step 10B 的 `osu/lazer.rs`）必须实现同一条梯子：

1. `$MMA_LAZER_OFFSETS`（显式文件路径，开发/运维用）；
2. `<壳 exe 目录>\lazer-offsets\<lazer>__<runtime>__<arch>.json`（精确命中）；
3. 同目录下**任意** `*.json` 表 → 版本键不完全匹配时才走
   `offsets.rs::nearest_table`（**仅在 L1 结构证明通过时**，且**必须大声记日志**；
   默认策略恒拒，所以不显式给谓词就拿不到回落表）；
4. 都没有 ⇒ reason `lazer-offsets-missing:<ver>` + 回落 tosu（绝不静默用错版本的表）。

## 验收判据（Acceptance criteria）

**每一个出表的偏移都必须同时有**

**(a) SOS 行** —— `dumpobj` 打印的 `Offset`（来自真实 dump，`extract` 产出；中间件里带
`transcript` 文件名与原始命令，逐字可回溯）；**且**

**(b) IL 结构行** —— 同一字段在 lazer 安装目录的托管程序集元数据里的结构
（存在性 + `instance`/`static` + 字段类型 + 显式布局偏移）。

**两边对不上 ⇒ 丢弃该字段并在 `emit-report-*.txt` 里逐条列出**（`- label.field  reason  detail`），
绝不"取其一"。第三个见证是**dump 字节解引用**：对每个字段行，用 dump 里的原始字节核对
SOS 打印的值（引用类型比指针、原始类型比内容、结构体比"字段自身地址 = 对象地址 + Offset"）
—— 这同时机械地证明了"**SOS 的 Offset 基准 = 对象地址**"（P4b 只能写"未核实"的那一条）。

`emit` 的完整判据表（全部实现在 `emit.rs`，逐条有自测）：

| 判据 | 不满足时 |
|---|---|
| SOS 中间件里该链步骤的对象存在且状态 `ok` | `chain-missing` / `chain-status`（并**跳过该分支的子树**，fail-closed） |
| 该对象上有这个字段行 | `no-sos-row` |
| 同名行偏移一致（否则歧义） | `ambiguous-sos-row`（列出全部偏移） |
| SOS 的 Attr 列是 `instance` | `sos-non-instance` |
| SOS 的 Type 列非空 | `sos-type-missing` |
| 解引用见证不是 `mismatch-*` | `deref-mismatch` |
| IL 清单里沿**基类链**找到该字段（`#extends`） | `no-il-row` |
| 多个程序集/多个声明不冲突 | `il-ambiguous` |
| IL 说 `instance` | `il-static` |
| 类型一致：非截断 ⇒ 规范名相等；`System.__Canon` ⇒ IL 是泛型参数 `!n`；SOS 截断（含 `…`）⇒ 只比"引用/值类型"这一层 | `il-type-mismatch` |
| 元数据里的**显式布局**偏移（若有）与 SOS 相等 | `il-explicit-offset-mismatch` |
| 同一 `(类型, 字段)` 不会被两个对象以不同偏移发布 | `duplicate-key-conflict` |
| 表来自真实 dump（或**显式** fixture） | 拒绝出表（退出码 3） |
| `provenance=dump` 时 `deref_checked>0` | 拒绝出表（证明解引用那一趟真的跑了） |
| dump 与 IL 的模块版本一致（同名模块逐段比较） | 拒绝出表；`--allow-version-mismatch` 可放行，但**不一致会写进 `evidence`** |

解析阶段的判据（`extract`，实现在 `chain.rs`，逐条有自测）：

| 判据 | 不满足时 |
|---|---|
| `site = anchor − delta`，站点处读出的指针"合理"（非空、在内核分割下、8 字节对齐） | 该 `(anchor, delta)` 尝试被判失败并**逐字打印**（`… implausible (read at 0x…)`），不产生候选 |
| 站点 → `+0x218` → `+0x310` 每一跳都读得出且合理 | 同上（读不到 ⇒ `… unreadable at 0x…`） |
| 候选 `dumpobj` 的类型含 `osu.Desktop.OsuGameDesktop`/`osu.Game.OsuGameBase` | 候选被**拒绝**（打印实际类型），换下一个候选；全不过 ⇒ `extract` 退出码 4 |
| **给了表 vtable 时**：`[gameBase]`（候选地址首 qword = SOS `MethodTable`）等于其中之一，且与 dump 字节一致 | 候选被**拒绝**；没有表可给时第 ② 条记 `not applicable`（绝不假装过了） |

**元数据推导的偏移永不作为偏移发布**（计划 Step 10 原话）：IL 侧只提供结构行与"显式布局"
核对；自动布局的偏移只能由 dump/SOS 给出。

## 自测（不需要 lazer、不需要 dump、不需要分析器）

```powershell
temp/lazer-offsets-gen.exe self-test --fixtures tools/lazer-offsets-gen/fixtures --work temp/lazer-offsets-gen-selftest
```

自测跑的是**真实代码路径**：把 fixture 的 `Offset`/`Value` 列种进一份**合成 minidump**
（`selftest.rs` 里手写的 minidump 写入器）→ `minidump` 解析 → 锚点扫描 → `chain::walk`
（fixture 版 `BlockSource`）→ dump 字节解引用自证 → SOS 中间件装配 → `emit` 双见证校验 →
出表 JSON（再用自带的 JSON 解析器读回来逐字段核对）。29 个用例：

- 解析/规范化：`names/canonical-table`（9 例，含截断与嵌套泛型）、`sos/parse-transcript`；
- dump 层：`minidump/synthetic-roundtrip`（含 `System.String` 布局 +0x08/+0x0C）、
  `minidump/anchor-miss`（0 命中）；
- 链：`chain/fixture-walk`（16 步全 ok）、`chain/type-mismatch`（父步骤失败 ⇒ 子树被跳过）、
  `chain/resolve-multi-hop`（fixture 的 `site → +0x218 → +0x310 → GameBase` 多跳必须解到
  `0xBF000FD8`；错误位移必须挡下）、`chain/resolve-wrong-middle-hop`（中间跳被改坏 ⇒ 无候选、
  判词点名是哪一跳）、`chain/candidate-validation`（类型/vtable/基准自证/只给类型四种情形）；
- 解引用：`deref/witness-clean`（58 行全对）、`deref/witness-mismatch`（改一个字节 ⇒ 报不一致）；
- 出表：`emit/valid-fixture-table`（42 字段 / 14 类型，JSON 可解析、标记齐备）与 9 个失败用例
  （缺 SOS 行/缺 SOS 类型/歧义/缺 IL 行/IL 静态/IL 类型不符/IL 显式偏移不符/解引用不一致/版本不一致/
  空表）、`parse/malformed-intermediate`（列数不对 ⇒ 报错带行号）、`il/diff-structure`、
  `discovery/select-lazer`（没开游戏/只有 stable/多实例/正常各一条）。

**fixture 规则（硬约束）**：fixture 文件名带 `EXAMPLE-fixture-`，文件内带 `#fixture true`；
**只要输入带标记，出的表必然**——文件名前缀 `EXAMPLE-FIXTURE-`、`verified_build` 以
`EXAMPLE-FIXTURE/` 开头、`evidence` 以 `EXAMPLE-FIXTURE EXAMPLE DATA — not a real dump.` 开头。
没有 `--allow-fixtures` 时 fixture 输入直接**拒绝出表**。`emit` 不会、也不能把 fixture 数当成真实偏移表。

## 维护承诺（每次 lazer 更新）

| 触发 | 做什么 | 目标 |
|---|---|---|
| **lazer 小版本更新**（游戏内自动更新，runtime 版本不变） | 重跑 `collect → extract → il --diff → emit`；看 `il-diff` 报出的字段增删改；`emit` 只发布仍有双见证的字段 | ≤ 1 天 |
| **runtime 版本变了**（.NET 升级） | 同上，但键变成新的 `(lazer, runtime, arch)`：**新表另存一份**，旧表保留（回落梯子靠它）；`arch` 变了则一律不回落（`offsets.rs` 的架构不同 ⇒ 永不采纳） | ≤ 1 天 |
| **锚点失效**（`extract` 报 "anchor pattern was not found"） | 锚点是**构建相关**的：用 P4 探针的方法重新找一个 12 字节模式（或先试 `--anchor` 传新串），随后按类型验证会自己把假阳性筛掉 | 半天 |
| **解析链失效**（`extract` 打印的尝试里没有候选，或候选全被 `REJECTED`） | 用 P4 探针的方法重测**站点位移**与**跳序**（`spec.rs::SITE_DELTAS` / `GAME_BASE_HOPS`）；位移是要扫的候选表，不是常量 | 半天 |
| **链上某一步 `chain-type-mismatch` / `chain-no-field`** | 改 `spec.rs::CHAIN`（字段名/类型子串）；**只重跑 `extract`**（要新 dump）或先看 transcript 里那个对象的实际字段表 | 半天 |
| **要发布新字段**（part B 需要更多偏移） | 改 `spec.rs::WANTED` → **只重跑 `emit`**（中间件里已有全部字段行） | 分钟级 |
| **`play.hits` 之类游玩态字段** | 在**游玩中**再采一份 dump（同样的命令），`extract` 会多 dump 出游玩链上能解到的对象；`spec.rs` 里补对应的链步骤与 `WANTED` | 半天 |

要看"这次更新到底变了什么"：`il --diff <上一版清单>` 的 `ADDED/REMOVED/CHANGED` 段就是答案
（字段增删改在元数据层面一目了然），再对照 `emit-report` 的 `omitted` 清单。

## 已知边界（如实记录，不粉饰）

- **真机提取已跑通（2026-09-29，Step 10c）**：同一份真机 dump（2.69 GB，pid 22028，lazer
  `2026.921.0-lazer`）上 `extract → il → emit` 全通——锚点 1 命中（`0xbf35cbb4`），
  `delta=0x24 → site 0xbf35cb90 → elo 0xbbeae208 → api 0xbd222be8 → gameBase 0xbf35c4b8`
  （`dumpobj` 类型 `osu.Desktop.OsuGameDesktop`），第 ② 条判据用同一次 dump 产出的表 vtable
  跑过（`passed: [gameBase]=0x7FFE2F2D8970`）；出表 40 字段 / 14 类型（2 条集级 metadata
  字段因链上 `BeatmapSetInfo.<Metadata>` 缺席而丢弃）。逐字数字见 D-notes §16 与
  `.omo/evidence/osu-native-memory-transport/task-10c-build.txt`。
- **`osu!.dll` / `osu!.exe` 的文件版本是占位 `0.0.0.0`**（自包含发布；`ProductVersion` 才带
  `2026.921.0-lazer+<sha>`）。`emit` 的版本门因此会在真机上拒跑一次；表已产出的那次是用
  `--allow-version-mismatch` 放行的（不一致写进 `evidence`）。要不要把"占位版本"当"不可比对"
  处理，留给下一步决定。
- **候选验证的第 ② 条只在有表时生效**：不给 `--expect-vtable` / `--table` / `$MMA_LAZER_OFFSETS`
  时，`extract` 如实记 `not applicable`（类型判据仍然必须过）——这不是"过了"，是"没得比"。
- **`game_base_vtable` 是运行期 MethodTable 指针**（ASLR 相关）：同一构建的不同进程实例
  数值不同（P4 的 `0x7ff9e7ef8970` vs 本次 `0x7ffe2f2d8970`）。产品侧 L1 结构证明拿实时
  `[game_base]` 跟表里的这一列比 ⇒ **跨进程会失败**（fail-closed 到 tosu，但原生路径在
  重启后就用不了）。这是产品侧（`osu/lazer.rs`）要定的问题，本工具只负责把这一列如实写出来。
- `dotnet-dump analyze` **不回显地址**：块与命令按顺序对应；一旦块数对不上（某个地址无效），
  工具会自动退化为"一个地址一次分析器调用"以保证对齐确定性（更慢，但只在异常时发生）。
- 逐层解引用要**多次调用分析器**（每层一次，链有 5 层），每次都要加载整个 dump ⇒ 分钟级；
  这是"离线读 dump"的固有代价。
- `--type Heap` 就被 `dotnet-dump` 接受（实测），`Full`/`Mini` 是备选链。
- 本工具**不负责**运行时校验：L1 结构证明属于产品侧（`osu/lazer.rs` + `invariants.rs`），
  `offsets.rs` 只提供 `load()` 与"默认拒绝"的回落钩子。
- 工具与自测产物一律在 `temp/`（gitignored）；仓库里只提交 `main.rs`/`*.rs`/`README.md`/`fixtures/`。

## 改哪里（工具的结构）

| 文件 | 内容 |
|---|---|
| `spec.rs` | **lazer 更新时唯一要人工改的文件**：锚点模式、站点位移（`SITE_DELTAS`/`ANCHOR_SITE_DELTA`）、GameBase 跳序（`GAME_BASE_HOPS`）、解引用链（`CHAIN`）、出表字段（`WANTED`） |
| `main.rs` | 五个子命令、进程发现（位数分派）、版本钉法（runtime log/runtimeconfig）、命令驱动与落盘 |
| `minidump.rs` | 最小 minidump 读取器（Memory64List/MemoryList 两种形态、模块表、SystemInfo、MiscInfo、内存扫描、`System.String` 读法） |
| `sos.rs` | `dotnet-dump analyze` 驱动、`dumpobj` 文本解析、dump 字节解引用自证、SOS 中间件读写 |
| `ilmeta.rs` | PE/CLI 元数据解析（ECMA-335 表 + #Strings/#Blob 堆 + 字段签名解码 + `#extends` 继承链）、IL 清单读写、跨版本 diff |
| `emit.rs` | 双见证校验、拒绝门（fixture/溯源/版本）、JSON 出表与报告 |
| `chain.rs` | 解引用链走法（真实路径与自测共用）、**GameBase 多跳解析 + 候选验证**（`resolve_game_base`/`candidate_verdict`）、中间件装配、`Offset` 基准自证 |
| `selftest.rs` | 自测用例 + 合成 minidump 写入器 + 迷你 JSON 解析器 |

参考（我们自己的证据，`temp/` 会在验收后删除，故此处记下要点）：P4b 的 8/8 实读一致
（`<Storage>`@0x440、`<VersionHash>`@0x3d0、`<ScreenStack>`@0x620、`<Host>`@0x338、
`<Beatmap>`@0x450、`<UnderlyingStorage>`@0x10、`BasePath`@0x8、`System.String` len/char
@0x08/@0x0C）与 OPEN-03 的修正（`Bindable<T>.value`@+0x20 且**按实例化**取值）。