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

Malody 4.3.7 is the plugin's fourth data source, attached by the desktop shell through zero-injection read-only observation: find the single `malody.exe`, verify the PE version (`TimeDateStamp` + file size), read the anchor pointer (`module base + anchor RVA`, the RVA comes from the version table in `anchor.rs`) and parse the identity key `<md5>_<slot>`. When the card does not follow the highlighted chart, the cause can be a missing process, multiple instances, missing rights, a version mismatch, a failed memory read or a chart missing from the index. This tool prints every stage so one command shows which stage broke, and it can validate ANY PE file offline (no game running).

### 原理（How it works）

- 单文件、零依赖、不属于 cargo workspace：`main.rs` 用 `#[path = "../../desktop/src/malody4/anchor.rs"] mod anchor;` **直接注入壳里的那一份 `anchor.rs`**，所以版本常量（RVA / PE 时间戳 / 文件大小）在仓库里仍然只有一份，工具既不复制也不改写。
- 只注入 `anchor.rs`：`model.rs` 需要 `serde`、`library.rs` 需要 `md-5` 且引用 crate 根，注入任何一个都无法用裸 `rustc` 编译。
- 平台门写在函数上（`#[cfg(windows)] fn main` / `#[cfg(not(windows))] fn main`）而不是整文件 `#![cfg(windows)]`，这样非 Windows 目标也能编译出可执行文件（运行即打印平台提示并以退出码 2 结束）。
- `--exe` 模式只读磁盘文件（`anchor::validate_pe_file`），不打开任何进程；默认模式只做只读观察（`OpenProcess` + `ReadProcessMemory`），不注入、不写入、不在游戏内分配内存。

- Single file, no dependencies, not part of the cargo workspace: `main.rs` injects the shell's own `anchor.rs` through `#[path = "../../desktop/src/malody4/anchor.rs"] mod anchor;`, so the version constants (RVA / PE timestamp / file size) still live in exactly one place and are never copied or duplicated.
- Only `anchor.rs` is injected: `model.rs` needs `serde`, and `library.rs` needs `md-5` plus the crate root, so injecting either one cannot compile with bare `rustc`.
- The platform gate sits on the functions (`#[cfg(windows)] fn main` / `#[cfg(not(windows))] fn main`) instead of a whole-file `#![cfg(windows)]`, so non-Windows targets still build a binary that prints a platform notice and exits with code 2.
- `--exe` mode reads a file on disk only (`anchor::validate_pe_file`) and opens no process; the default mode observes read-only (`OpenProcess` + `ReadProcessMemory`): no injection, no writes, no allocation inside the game.

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
```

- 失败原因逐字取壳 state 帧 `reason` 的闭集：`process-not-found` / `multiple-instances` / `access-denied` / `target-mismatch:…` / `bad-read` / `platform-unsupported`，可以直接与壳日志、页面提示对照。
- 退出码：`0` = 诊断已跑完（结论看 `verdict` / `reason` 行），`2` = 参数错误或平台不支持。
- 采样期间锚点指针为 0 表示游戏当前没有高亮任何谱面（正常态）；指针非 0 时打印解析出的 `md5` 与 `slot`。

- Failure reasons are the shell's closed-set `reason` literals verbatim: `process-not-found` / `multiple-instances` / `access-denied` / `target-mismatch:...` / `bad-read` / `platform-unsupported`, so they can be compared directly with the shell log and the page notice.
- Exit codes: `0` = the diagnostic ran to completion (read the `verdict` / `reason` lines), `2` = bad arguments or unsupported platform.
- While sampling, an anchor pointer of 0 means the game currently highlights no chart (a normal state); a non-zero pointer prints the parsed `md5` and `slot`.

### 何时需要重新编译（When to rebuild）

`desktop/src/malody4/anchor.rs` 的版本表（`KNOWN_CLIENTS` 的 RVA / `TimeDateStamp` / 文件大小）或锚点读取逻辑变更后，请重新执行上面的构建命令，让工具与壳用的是同一份常量。工具不随插件版本号变化而需要重建。

Rebuild with the command above whenever the version table in `desktop/src/malody4/anchor.rs` (`KNOWN_CLIENTS`: RVA / `TimeDateStamp` / file size) or the anchor read logic changes, so the tool and the shell share the same constants. The tool does not need rebuilding when the plugin version changes.

### 注意事项（Notes）

- **本工具不查索引、不含 MD5 实现**（有意为之：手写的无测试密码学代码一旦静默出错，恰好会误诊“不跟随”这一最需要诊断的场景）。**索引命中路径请看壳日志 `logs/mma-shell-*.log`（把壳配置里的 `logLevel` 设为 `debug`）**：壳在签名变化时会打印 `md5 -> path`，拿工具打印的 `md5` 去比对，即可判断是“索引里没有这张谱”还是“跟随链路断了”。
- `temp/malody4-anchor-check.exe` 是临时产物，验收后按 CLAUDE.md 的约定删除，不要提交。
- 工具只读：不注入游戏、不写任何文件、不需要管理员权限（与游戏同用户即可）。

- **This tool does not query the index and contains no MD5 implementation** (deliberate: untested hand-written crypto that silently misbehaves would misdiagnose exactly the "not following" case this tool exists for). **For the index hit path, read the shell log `logs/mma-shell-*.log` (set `logLevel` to `debug` in the shell config)**: on every signature change the shell logs `md5 -> path`, so comparing it with the `md5` printed by this tool tells you whether the chart is missing from the index or the follow chain broke.
- `temp/malody4-anchor-check.exe` is a scratch artifact: delete it after acceptance per CLAUDE.md, never commit it.
- The tool is read-only: no injection, no file writes, no administrator rights needed (same user as the game is enough).
