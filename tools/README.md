# tools/ — 开发工具集 / Development Tools

> 本目录存放仓库的辅助开发脚本，不参与插件运行时（插件运行不依赖 Node 等任何外部运行时）。
> This folder holds helper scripts for repository maintenance. They are NOT part of the plugin runtime (the plugin itself never requires Node or any external runtime).

## patch-minaclac-msd-cap.mjs（MSD 上限突破补丁）

### 这个脚本是做什么的（What it does）

MinaCalc（Etterna 的官方难度计算器）内部把技能值上限（SSR cap）钳制在 **40.0**，导致超高难谱面（高速叠键 / 高密度）的 MSD 结果被压平在 40 左右。本脚本把该上限从 40.0 提升到 **100.0**，让 MSD 可以突破 42 并继续显示真实难度。

MinaCalc (Etterna's official difficulty calculator) clamps its internal skill cap (SSR cap) at **40.0**, flattening MSD results for ultra-hard charts (fast jacks / extreme density). This script raises the cap from 40.0 to **100.0** so MSD can break past 42 and show true difficulty.

### 原理（How it works）

- WASM 字节码中 `f32.const 40.0` 编码为 `43 00 00 20 42`（IEEE754 单精度小端）。
- `f32.const 100.0` 编码为 `43 00 00 c8 42`。
- 脚本扫描 `ManiaMapAnalyser by Leo_Black/js/ett/versions/` 下的所有 `.wasm`，把每一处 `43 00 00 20 42` 等长替换为 `43 00 00 c8 42`。
- 这是 5 字节 → 5 字节的等长替换，不改变 wasm 的 section 偏移、函数表与导入导出表，结构完全安全。

- In WASM bytecode, `f32.const 40.0` is encoded as `43 00 00 20 42` (IEEE754 little-endian single precision).
- `f32.const 100.0` is encoded as `43 00 00 c8 42`.
- The script scans every `.wasm` under `ManiaMapAnalyser by Leo_Black/js/ett/versions/` and replaces each `43 00 00 20 42` with `43 00 00 c8 42`.
- The replacement is equal-length (5 bytes → 5 bytes), so section offsets, function tables and import/export tables stay intact — structurally safe.

### 使用方法（Usage）

```bash
node tools/patch-minaclac-msd-cap.mjs
```

- 从仓库根目录或任意位置运行均可（路径按脚本自身位置解析）。
- 脚本是幂等的：某个文件里已没有 40.0 常量时会提示并跳过。
- 修改前会先把原始二进制复制一份到仓库的本地备份目录（该目录不入库）。
- 仓库内的 `.wasm` 由 git 跟踪，原始字节随时可以从 git 历史恢复。

- Run from the repository root or anywhere (paths resolve relative to the script itself).
- The script is idempotent: files with no remaining 40.0 constant are reported and skipped.
- Before modifying, a copy of each original binary is saved to the repository's local backup folder (never committed).
- The shipped `.wasm` files are git-tracked, so pristine bytes can always be restored from git history.

### 何时需要重新执行（When to re-run）

向 `ManiaMapAnalyser by Leo_Black/js/ett/versions/` 添加新的 MinaCalc 版本（新 `.wasm`）之后，请重新运行本脚本，确保新版本同样突破上限。

After adding a new MinaCalc version (a new `.wasm`) to `ManiaMapAnalyser by Leo_Black/js/ett/versions/`, re-run this script so the new version gets the cap lift too.

### 注意事项（Notes）

- **`minaclac-68.0-unofficial.wasm` 不参与 patch**：该版本中的 40.0 常量并非技能上限（实测其技能值本就可超过 40），patch 会改变普通谱面的输出，因此保持原样。
- **上层 JS 无需改动**：`calc.js` 的 `mapOutputValues` 直接透传 wasm 输出的 8 个技能值，上限突破后数值自然更高。
- **修改 wasm 字节后**，请同步递增 `ManiaMapAnalyser by Leo_Black/js/ett/constants.js` 中的 `WASM_ASSET_VERSION`（浏览器缓存失效用），避免用户加载到旧的 wasm。

- **`minaclac-68.0-unofficial.wasm` is NOT patched**: its 40.0 constants are not a skill cap (its skill values already exceed 40 in practice), and patching it would alter normal-map outputs.
- **No JS glue changes needed**: `calc.js`'s `mapOutputValues` passes through the 8 skill values from wasm directly; higher outputs flow through naturally.
- **After changing wasm bytes**, bump `WASM_ASSET_VERSION` in `ManiaMapAnalyser by Leo_Black/js/ett/constants.js` (used for browser cache busting) so users do not load a stale wasm.

## malody4-anchor-check（Malody 4.3.7 只读观察通道诊断）

### 这个工具是做什么的（What it does）

Malody 4.3.7 是本插件的第四类数据源，桌面壳以**零注入的只读观察**接入它：定位唯一的 `malody.exe` → 校验 PE 版本（`TimeDateStamp` + 文件大小）→ 读锚点指针（`模块基址 + 锚点 RVA`，RVA 取自 `anchor.rs` 的版本表）→ 解析身份键 `<md5>_<slot>`。当卡片里的谱面“不跟随”游戏高亮时，成因可能是进程没找到、多实例、权限不足、版本不符、内存读取失败或索引里没有这张谱，肉眼无法区分。本工具把这几环逐项打印出来，一行命令就能看出断在哪一环；它也能对任意 PE 文件做**离线版本校验**（不需要游戏在跑）。

在“跟随链路没错、但索引里就是没有这张谱”的情况下，还有三种可能：身份缓冲被游戏**原地重写**（读到半新半旧的“撕裂值”），游戏内存里其实另有正确的身份串，或者游戏给出的 md5 本就不是磁盘上那个文件的 md5。为此工具提供三种进一步模式：`--dump-identity <秒>`（每 50ms 采样一次，打印身份缓冲的**原始字节** hex + ASCII、指针值与解析结果，并汇总 Empty / Unparsable / 不同 md5 / 相邻两次采样 md5 不稳的情况）、`--scan-identity <秒>`（扫描目标进程可读内存里的**全部**身份串，报告地址与出现次数）、`--find-md5 <32位hex>`（同一个扫描器 + 一个 md5 过滤，回答“这个 md5 到底在不在内存里、在哪”）。

Malody 4.3.7 is the plugin's fourth data source, attached by the desktop shell through zero-injection read-only observation: find the single `malody.exe`, verify the PE version (`TimeDateStamp` + file size), read the anchor pointer (`module base + anchor RVA`, the RVA comes from the version table in `anchor.rs`) and parse the identity key `<md5>_<slot>`. When the card does not follow the highlighted chart, the cause can be a missing process, multiple instances, missing rights, a version mismatch, a failed memory read or a chart missing from the index. This tool prints every stage so one command shows which stage broke, and it can validate ANY PE file offline (no game running).

When the follow chain itself is fine but the index still misses the chart, three further causes remain: the game rewrites the identity buffer **in place** (so a read can return a torn, half-old/half-new value), the game holds another identity string somewhere in memory, or the md5 the game reports simply is not the md5 of the file on disk. Three further modes cover them: `--dump-identity <seconds>` (samples every 50 ms and prints the **raw bytes** of the identity buffer, hex + ASCII, plus the pointer and the parsed result, then a summary of Empty / Unparsable / distinct md5s / consecutive samples whose md5 differs), `--scan-identity <seconds>` (scans the target's readable memory for **every** identity string and reports addresses and hit counts) and `--find-md5 <32 hex digits>` (the same scanner filtered to one md5: whether it is in memory at all, and where).

### 原理（How it works）

- 单文件、零依赖、不属于 cargo workspace：`main.rs` 用 `#[path = "../../desktop/src/malody4/anchor.rs"] mod anchor;` **直接注入壳里的那一份 `anchor.rs`**，所以版本常量（RVA / PE 时间戳 / 文件大小）在仓库里仍然只有一份，工具既不复制也不改写。
- 只注入 `anchor.rs`：`model.rs` 需要 `serde`、`library.rs` 需要 `md-5` 且引用 crate 根，注入任何一个都无法用裸 `rustc` 编译。
- 平台门写在函数上（`#[cfg(windows)] fn main` / `#[cfg(not(windows))] fn main`）而不是整文件 `#![cfg(windows)]`，这样非 Windows 目标也能编译出可执行文件（运行即打印平台提示并以退出码 2 结束）。
- `--exe` 模式只读磁盘文件（`anchor::validate_pe_file`），不打开任何进程；默认模式只做只读观察（`OpenProcess` + `ReadProcessMemory`），不注入、不写入、不在游戏内分配内存。
- 三个新模式（`--dump-identity` / `--scan-identity` / `--find-md5`）也**只读**：只用 `PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ` 打开进程，不注入、不写入、不在目标进程里分配内存；工具需要**游戏侧零文件**（不读也不写游戏目录）。
- Win32 全部手写 `extern "system"` 声明（`VirtualQueryEx` / `ReadProcessMemory` / `OpenProcess` / `CloseHandle`），零外部 crate；`anchor.rs` 的 handle 字段是私有的，故工具自己以相同权限再开一个句柄来读原始字节，`anchor.rs` 的版本常量仍然只有一份。
- `--scan-identity` / `--find-md5` 的边界：只用 `VirtualQueryEx` 枚举**已提交、可读、非 GUARD** 的区域，按 64KiB 分块读；整块读失败就退到 4KiB 逐页重试，读不到的页跳过并计数（页边界处的短读是常态，绝不让扫描失败）；总读取量上限 512MB（`--find-md5` 另有 20 秒预算），扫描超过 1 秒会打印进度行，结果块报告区域数、实际扫描字节数、跳过页数与耗时。
- 命中判定复用壳的解析器（`anchor::parse_identity_key`），并要求整个身份串落在已扫描字节内、以非数字字节结尾——被分块边界截断的数字串不计数（否则无法区分 9 位与 10 位数字）。

- Single file, no dependencies, not part of the cargo workspace: `main.rs` injects the shell's own `anchor.rs` through `#[path = "../../desktop/src/malody4/anchor.rs"] mod anchor;`, so the version constants (RVA / PE timestamp / file size) still live in exactly one place and are never copied or duplicated.
- Only `anchor.rs` is injected: `model.rs` needs `serde`, and `library.rs` needs `md-5` plus the crate root, so injecting either one cannot compile with bare `rustc`.
- The platform gate sits on the functions (`#[cfg(windows)] fn main` / `#[cfg(not(windows))] fn main`) instead of a whole-file `#![cfg(windows)]`, so non-Windows targets still build a binary that prints a platform notice and exits with code 2.
- `--exe` mode reads a file on disk only (`anchor::validate_pe_file`) and opens no process; the default mode observes read-only (`OpenProcess` + `ReadProcessMemory`): no injection, no writes, no allocation inside the game.
- The three new modes (`--dump-identity` / `--scan-identity` / `--find-md5`) are read-only in exactly the same way: they open the process with `PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ` only (no injection, no writes, no allocation inside the target), and they need **no game-side files at all** (nothing under the game directory is read or written).
- Every Win32 call is a hand-written `extern "system"` declaration (`VirtualQueryEx` / `ReadProcessMemory` / `OpenProcess` / `CloseHandle`) with zero external crates. The handle fields of `anchor.rs` are private, so the tool opens its own handle with identical rights to read the raw bytes - the version constants still live only in `anchor.rs`.
- Bounds of `--scan-identity` / `--find-md5`: only committed, readable, non-guarded regions are enumerated with `VirtualQueryEx`, read in 64 KiB chunks; a failed chunk read is retried page by page (4 KiB) and unreadable pages are skipped and counted (a partial read around a page boundary is normal and never fails the scan); total reads are capped at 512 MB (`--find-md5` also gets a 20 s budget), progress lines appear once a scan runs longer than a second, and the result block reports regions, bytes actually scanned, skipped pages and elapsed time.
- Matching reuses the shell's own parser (`anchor::parse_identity_key`) and requires the whole identity to lie inside the scanned bytes and to end at a non-digit byte - a digit run cut off by a chunk boundary is not counted (otherwise 9 and 10 digits cannot be told apart).

### 使用方法（Usage）

```bash
# 构建（在仓库根目录执行；--edition 2021 必需，-A dead_code 因为只用到 anchor.rs 的一部分 API）
rustc --edition 2021 -O -A dead_code -o temp/malody4-anchor-check.exe tools/malody4-anchor-check/main.rs

# 离线版本校验：对任意 PE 文件打印 e_lfanew / TimeDateStamp / 文件大小与结论（不需要游戏在跑）
temp/malody4-anchor-check.exe --exe "C:\Windows\System32\notepad.exe"
temp/malody4-anchor-check.exe --exe "D:\Games\Malody-4.3.7\malody.exe"

# 只读观察：定位进程 → 版本门 → 模块基址与锚点地址 → 每 200ms 采样（默认 10 秒，用 --follow 改）
temp/malody4-anchor-check.exe
temp/malody4-anchor-check.exe --follow 30

# 原始字节采样：每 50ms 一次，打印身份缓冲的原始 hex + ASCII、指针与解析结果，最后给汇总
temp/malody4-anchor-check.exe --dump-identity 10

# 全内存找身份串：报告每条身份串的地址与出现次数（不需要先知道 md5）
temp/malody4-anchor-check.exe --scan-identity 15

# 只找指定的 md5：回答“这个 md5 在不在内存里、在哪”（md5 必须显式给出，工具不预置任何值）
temp/malody4-anchor-check.exe --find-md5 5a6d5fe073b0b2a8fe5bac4167cff5c8
```

- 失败原因逐字取壳 state 帧 `reason` 的闭集：`process-not-found` / `multiple-instances` / `access-denied` / `target-mismatch:…` / `bad-read` / `platform-unsupported`，可以直接与壳日志、页面提示对照。
- 退出码：`0` = 诊断已跑完（结论看 `verdict` / `reason` 行；游戏没在跑时 `reason` 就是 `process-not-found`，那本身就是诊断结论，故仍为 `0`），`2` = 参数错误或平台不支持。
- 采样期间锚点指针为 0 表示游戏当前没有高亮任何谱面（正常态）；指针非 0 时打印解析出的 `md5` 与 `slot`。
- 三个新模式都不是“一次采样即结论”：`--dump-identity` 看的是**原始字节**（撕裂读只有原始字节能看出来，因为文法只验形状），`--scan-identity` 看的是**全内存**（游戏自己的列表 / UI / 查找表里那份身份串也一并列出），`--find-md5` 是前两者的单点查询。
- 采样期间若要观察换谱，请在游戏里切换高亮谱面：`--dump-identity` 会在相邻两次采样 md5 不同时打印 `!!` 行，并区分“指针也变了（正常换谱）”与“指针没变（游戏原地重写了缓冲）”。

- Failure reasons are the shell's closed-set `reason` literals verbatim: `process-not-found` / `multiple-instances` / `access-denied` / `target-mismatch:...` / `bad-read` / `platform-unsupported`, so they can be compared directly with the shell log and the page notice.
- Exit codes: `0` = the diagnostic ran to completion (read the `verdict` / `reason` lines; when the game is not running the reason line is `process-not-found`, which IS the diagnosis, so the code stays 0), `2` = bad arguments or unsupported platform.
- While sampling, an anchor pointer of 0 means the game currently highlights no chart (a normal state); a non-zero pointer prints the parsed `md5` and `slot`.
- The three new modes are not single-shot verdicts: `--dump-identity` shows the **raw bytes** (only raw bytes can expose a torn read, because the parser checks grammar only), `--scan-identity` covers **all of memory** (the game's own list / UI / lookup-table copy of an identity is listed too), and `--find-md5` is a single-point query on top of that scanner.
- To watch a chart switch, highlight another chart in the game while sampling: `--dump-identity` prints a `!!` line whenever the md5 of two consecutive samples differs, and distinguishes "the pointer changed too (a normal chart switch)" from "the pointer stayed the same (the game rewrote the buffer in place)".

### 何时需要重新编译（When to rebuild）

`desktop/src/malody4/anchor.rs` 的版本表（`KNOWN_CLIENTS` 的 RVA / `TimeDateStamp` / 文件大小）或锚点读取逻辑变更后，请重新执行上面的构建命令，让工具与壳用的是同一份常量。工具不随插件版本号变化而需要重建。

Rebuild with the command above whenever the version table in `desktop/src/malody4/anchor.rs` (`KNOWN_CLIENTS`: RVA / `TimeDateStamp` / file size) or the anchor read logic changes, so the tool and the shell share the same constants. The tool does not need rebuilding when the plugin version changes.

### 注意事项（Notes）

- **本工具不查索引、不含 MD5 实现**（有意为之：手写的无测试密码学代码一旦静默出错，恰好会误诊“不跟随”这一最需要诊断的场景）。**索引命中路径请看壳日志 `logs/mma-shell-*.log`（把壳配置里的 `logLevel` 设为 `debug`）**：壳在签名变化时会打印 `md5 -> path`，拿工具打印的 `md5` 去比对，即可判断是“索引里没有这张谱”还是“跟随链路断了”。`--find-md5` 只做 32 位 hex 字符串比较，同样不计算 MD5。
- **工具绝不写入游戏**：不注入、不写目标进程内存、不在目标进程里分配内存、不要求管理员权限（与游戏同用户即可），也不需要**任何游戏侧文件**（不读也不写游戏目录，`--scan-identity` 只读目标进程自己的内存）。
- `--scan-identity` / `--find-md5` 是**内存快照式**结论：游戏在跑、内存在变，同一条命令两次运行的地址可能不同；只回答“这次扫描看到什么”。
- `--find-md5` 找的是**身份串** `<md5>_<slot>`，不是裸的 32 位 hex：若要看“游戏里是不是存在某个 md5，但与 slot 无关”，请用 `--scan-identity` 的完整列表比对。
- `temp/malody4-anchor-check.exe` 是临时产物，验收后按 CLAUDE.md 的约定删除，不要提交。
- 工具只读：不注入游戏、不写任何文件、不需要管理员权限（与游戏同用户即可）。

- **This tool does not query the index and contains no MD5 implementation** (deliberate: untested hand-written crypto that silently misbehaves would misdiagnose exactly the "not following" case this tool exists for). **For the index hit path, read the shell log `logs/mma-shell-*.log` (set `logLevel` to `debug` in the shell config)**: on every signature change the shell logs `md5 -> path`, so comparing it with the `md5` printed by this tool tells you whether the chart is missing from the index or the follow chain broke. `--find-md5` only compares 32 hex characters; it computes no MD5 either.
- **The tool never writes to the game**: no injection, no writes into the target's memory, no allocation inside the target, no administrator rights (same user as the game is enough) and **no game-side file whatsoever** (nothing under the game directory is read or written; `--scan-identity` only reads the target's own memory).
- `--scan-identity` / `--find-md5` are **snapshot** answers: the game is running and its memory changes, so two runs of the same command can report different addresses; they only answer "what was visible during this scan".
- `--find-md5` looks for the **identity string** `<md5>_<slot>`, not for a bare 32-hex string: to ask whether some md5 exists regardless of the slot, compare against the full list printed by `--scan-identity`.
- `temp/malody4-anchor-check.exe` is a scratch artifact: delete it after acceptance per CLAUDE.md, never commit it.
- The tool is read-only: no injection, no file writes, no administrator rights needed (same user as the game is enough).
