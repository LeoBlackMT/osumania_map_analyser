// malody4-anchor-check —— Malody 4.3.7 只读观察通道诊断工具（单文件、裸 rustc 编译、零外部依赖）
//
// 构建（在仓库根目录执行）：
//   rustc --edition 2021 -O -A dead_code -o temp/malody4-anchor-check.exe tools/malody4-anchor-check/main.rs
//   --edition 2021：裸 rustc 默认 edition 2015，缺了编译不过。
//   -A dead_code    ：本工具只调用 anchor.rs 的一部分 API，其余是被 lint 判死的 pub 项。
//
// 平台要求：**仅 Windows**。本工具通过 OpenProcess / ReadProcessMemory 只读地观察另一个进程，
// Windows 之外没有任何实现；非 Windows 目标仍可编译（`#[cfg(not(windows))]` 的 main 只打印提示
// 并以退出码 2 结束），平台门写在函数上而不是 `#![cfg(windows)]`：整文件被 cfg 掉会产生
// “no `fn main`”（E0601）。
//
// 依赖注入：用 `#[path]` 直接引入 desktop/src/malody4/anchor.rs —— 版本常量
// （RVA / PE 时间戳 / 文件大小）在仓库里只有那一份，工具不复制、不改写。
// **只注入 anchor.rs**：model.rs 需要 serde、library.rs 需要 md-5 且引用
// `crate::malody4::model::hex16`，注入任何一个都会因缺少 crate 根而编译失败。
//
// 本工具**不含 MD5 实现、不提供 --root 索引查询**（有意为之）：手写的无测试密码学代码一旦
// 静默出错，恰好会误诊“不跟随”这一最需要诊断的场景。索引命中路径由壳提供——壳在签名变化时
// 以 debug 级别打印 `md5 -> path`，见 logs/mma-shell-*.log。
// `--find-md5` 只做 32 位 hex 字符串比较，同样不计算 MD5。
//
// 模式：
//   --exe <path>          离线 PE 版本校验（不碰任何进程）
//   --follow <seconds>    默认模式：200ms 采样，只打印解析后的身份键
//   --dump-identity <s>   50ms 采样，打印**原始字节**（hex + ASCII）+ 指针 + 解析结果
//                         ——撕裂/陈旧读只有原始字节能看出来（文法只验形状）
//   --scan-identity <s>   扫描目标进程可读内存里的全部身份键（地址 + 出现次数）
//   --find-md5 <hex32>    同一个扫描器 + 一个 md5 过滤（该 md5 必须显式给出，工具不预置）
//
// 只读保证：所有模式只用 PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ 打开进程
// （anchor 与本工具各开一个句柄，权限逐字相同）；不注入、不写目标进程、不在目标进程里
// 分配内存，也不需要游戏侧任何文件。

#[path = "../../desktop/src/malody4/anchor.rs"]
mod anchor;

#[cfg(windows)]
use std::io::Read;
#[cfg(windows)]
use std::path::Path;
#[cfg(windows)]
use std::time::{Duration, Instant};

#[cfg(windows)]
const USAGE: &str = "\
malody4-anchor-check - Malody 4.3.7 read-only observation channel diagnostics

USAGE:
  malody4-anchor-check [--follow <seconds>]
  malody4-anchor-check --exe <path>
  malody4-anchor-check --dump-identity <seconds>
  malody4-anchor-check --scan-identity <seconds>
  malody4-anchor-check --find-md5 <hex32>

MODES (at most one per run):
  --exe <path>              Offline PE version check of ANY file. No process is
                            touched: prints e_lfanew, TimeDateStamp, file size and
                            the verdict.
  --follow <seconds>        Sample the anchor pointer and the identity key every
                            200 ms for this many seconds (default 10). Parsed key
                            only - use --dump-identity to see the raw bytes.
  --dump-identity <seconds> Sample every 50 ms (4x the shell's 200 ms poller) and
                            print the RAW 128 bytes of the identity buffer (hex +
                            ASCII), the pointer and the parsed result of every
                            sample, then a summary: Empty / Unparsable counts,
                            distinct md5s, and every pair of consecutive samples
                            whose md5 differs (with the pointer, so an in-place
                            buffer rewrite is distinguishable from a chart switch).
                            This is the torn/stale-read probe: the parser checks
                            grammar only, so a half-rewritten buffer still parses
                            as a \"valid\" md5 that resolves to nothing.
  --scan-identity <seconds> Scan the target process's readable memory for every
                            occurrence of the identity pattern
                            [0-9a-f]{32}_[0-9]{1,9} and report the distinct
                            identities with their addresses and hit counts. Prints
                            regions, bytes scanned and elapsed time, with progress
                            lines. Time budget: this many seconds.
  --find-md5 <hex32>        The same scanner filtered to one md5: whether that md5
                            appears in memory as an identity string <md5>_<slot>
                            and where. The md5 must be given explicitly (32 hex
                            digits) - nothing is hardcoded in the tool. Search
                            budget: 20 s (the 512 MB byte cap still applies).
  -h, --help                Print this help.

EXIT CODES:
  0  the diagnostic ran to completion - read the `verdict` / `reason` / result
     lines. When the game is not running the reason line says process-not-found:
     that IS the diagnosis, so the exit code stays 0 (same as the default mode).
  2  usage error (unknown argument, bad or missing value, two modes at once), or
     unsupported platform (this tool is Windows-only).

BOUNDS OF --scan-identity / --find-md5:
  - only committed, readable, non-guarded regions (VirtualQueryEx), read in 64 KiB
    chunks with ReadProcessMemory;
  - a failed chunk read is retried page by page (4 KiB); unreadable pages are
    skipped and counted, never fatal - a partial read around a page boundary is
    normal;
  - at most 512 MB are read in total, plus the time budget above;
  - an identity must lie fully inside the scanned bytes and end at a non-digit
    byte (a digit run cut off by a chunk boundary is not counted).

NOTES:
  - The tool never injects into the game, never writes to it and never allocates
    memory inside it; every mode opens the process with
    PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ only. It needs no
    game-side file of any kind.
  - It does NOT query the chart index and does NOT implement MD5. The `md5 -> path`
    hit path is logged by the desktop shell (logLevel: debug) in logs/mma-shell-*.log.
";

/// 运行模式：一次只跑一个（同时给两个 = 用法错误）。
#[cfg(windows)]
enum Mode {
    /// 默认模式：200ms 采样，只打印解析后的身份键。
    Follow(u64),
    /// 离线 PE 版本校验，不碰任何进程。
    Exe(String),
    /// 50ms 采样 + 原始字节。
    DumpIdentity(u64),
    /// 全内存找身份键。
    ScanIdentity(u64),
    /// 全内存找身份键 + 一个 md5 过滤。
    FindMd5(String),
}

#[cfg(windows)]
struct Args {
    mode: Mode,
}

#[cfg(windows)]
enum Parsed {
    Run(Args),
    Help,
}

/// 记录模式并拒绝"一次给两个模式"。
#[cfg(windows)]
fn set_mode(
    slot: &mut Option<(&'static str, Mode)>,
    name: &'static str,
    mode: Mode,
) -> Result<(), String> {
    match slot {
        Some((prev, _)) => Err(format!(
            "only one mode per run: {prev} and {name} were both given"
        )),
        None => {
            *slot = Some((name, mode));
            Ok(())
        }
    }
}

/// 秒数参数：必须是 ≥ 1 的整数（0 会让采样/扫描退化成什么都不做，直接判错）。
#[cfg(windows)]
fn parse_secs(flag: &str, raw: &str) -> Result<u64, String> {
    let secs = raw
        .parse::<u64>()
        .map_err(|_| format!("{flag} expects whole seconds, got {raw:?}"))?;
    if secs == 0 {
        return Err(format!("{flag} needs at least 1 second, got 0"));
    }
    Ok(secs)
}

/// md5 参数：必须显式给出且正好 32 位 hex（工具里不预置任何 md5）。
#[cfg(windows)]
fn parse_md5(flag: &str, raw: &str) -> Result<String, String> {
    if raw.len() != 32 {
        return Err(format!(
            "{flag} needs exactly 32 hex digits, got {} character(s): {raw:?}",
            raw.len()
        ));
    }
    if !raw.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!("{flag} accepts hex digits [0-9a-f] only, got {raw:?}"));
    }
    Ok(raw.to_ascii_lowercase())
}

#[cfg(windows)]
fn parse_args(raw: &[String]) -> Result<Parsed, String> {
    let mut mode: Option<(&'static str, Mode)> = None;
    let mut i = 0usize;
    while i < raw.len() {
        match raw[i].as_str() {
            "-h" | "--help" => return Ok(Parsed::Help),
            "--exe" => {
                let value = raw.get(i + 1).ok_or("--exe needs a file path")?;
                set_mode(&mut mode, "--exe", Mode::Exe(value.clone()))?;
                i += 2;
            }
            "--follow" => {
                let value = raw.get(i + 1).ok_or("--follow needs a number of seconds")?;
                set_mode(
                    &mut mode,
                    "--follow",
                    Mode::Follow(parse_secs("--follow", value)?),
                )?;
                i += 2;
            }
            "--dump-identity" => {
                let value = raw
                    .get(i + 1)
                    .ok_or("--dump-identity needs a number of seconds")?;
                set_mode(
                    &mut mode,
                    "--dump-identity",
                    Mode::DumpIdentity(parse_secs("--dump-identity", value)?),
                )?;
                i += 2;
            }
            "--scan-identity" => {
                let value = raw
                    .get(i + 1)
                    .ok_or("--scan-identity needs a number of seconds")?;
                set_mode(
                    &mut mode,
                    "--scan-identity",
                    Mode::ScanIdentity(parse_secs("--scan-identity", value)?),
                )?;
                i += 2;
            }
            "--find-md5" => {
                let value = raw.get(i + 1).ok_or(
                    "--find-md5 needs a 32-hex-digit md5 (required on purpose: the tool hardcodes none)",
                )?;
                set_mode(
                    &mut mode,
                    "--find-md5",
                    Mode::FindMd5(parse_md5("--find-md5", value)?),
                )?;
                i += 2;
            }
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    Ok(Parsed::Run(Args {
        // 没有任何模式开关 = 默认模式（`--follow` 的缺省时长是 10 秒）。
        mode: mode.map(|(_, mode)| mode).unwrap_or(Mode::Follow(10)),
    }))
}

/// 失败原因逐字对齐壳 state 帧 `reason` 的闭集字面量：
/// `AnchorError::reason()` 对 `TargetMismatch` 只返回内部短句（`pe_timestamp_mismatch`），
/// 闭集里的形态是 `target-mismatch:<短句>`，前缀在本工具里补上。
#[cfg(windows)]
fn reason(err: &anchor::AnchorError) -> String {
    match err {
        anchor::AnchorError::TargetMismatch(inner) => format!("target-mismatch:{inner}"),
        other => other.reason().to_string(),
    }
}

#[cfg(windows)]
fn hint(reason: &str) -> &'static str {
    if reason.starts_with("target-mismatch:") {
        return "the exe on disk is not the known Malody 4.3.7 build (see the PE items above); other builds are not supported yet";
    }
    match reason {
        "process-not-found" => {
            "no running malody.exe: start Malody 4.3.7 and re-run; this check is read-only and needs the live process"
        }
        "multiple-instances" => {
            "more than one malody.exe is running: close all but one (the tool never guesses which instance to follow)"
        }
        "access-denied" => {
            "OpenProcess was denied: run this tool as the same user as the game, without lowering its integrity level"
        }
        "bad-read" => {
            "ReadProcessMemory failed or the identity-key grammar did not hold: the client may have exited, or the address is stale"
        }
        "platform-unsupported" => "this channel only exists on Windows",
        _ => "see the reason above",
    }
}

/// 逐项打印 PE 版本门的两个判据（`e_lfanew` / `TimeDateStamp` / 文件大小），只读磁盘文件。
#[cfg(windows)]
fn print_pe_items(path: &Path, spec: &anchor::ClientSpec) {
    let size = match std::fs::metadata(path) {
        Ok(meta) => meta.len(),
        Err(e) => {
            println!("  file size       : <metadata failed: {e}>");
            println!("  e_lfanew        : <unreadable>");
            println!("  TimeDateStamp   : <unreadable>");
            return;
        }
    };
    let mut head = [0u8; 2048];
    match std::fs::File::open(path) {
        Ok(mut file) => {
            // 普通 read（不是 read_exact）：短文件读到多少算多少，其余保持零，
            // 与 anchor::validate_pe_file 的口径一致。
            let _ = file.read(&mut head);
        }
        Err(e) => {
            println!("  file size       : {size} bytes");
            println!("  e_lfanew        : <open failed: {e}>");
            println!("  TimeDateStamp   : <open failed>");
            return;
        }
    }
    let e_lfanew = u32::from_le_bytes([head[0x3C], head[0x3D], head[0x3E], head[0x3F]]);
    println!("  e_lfanew        : 0x{e_lfanew:X}");
    let stamp_at = e_lfanew as usize + 8;
    if stamp_at + 4 <= head.len() {
        let stamp = u32::from_le_bytes([
            head[stamp_at],
            head[stamp_at + 1],
            head[stamp_at + 2],
            head[stamp_at + 3],
        ]);
        println!(
            "  TimeDateStamp   : 0x{stamp:08X} ({}, expected 0x{:08X})",
            if stamp == spec.pe_timestamp { "match" } else { "MISMATCH" },
            spec.pe_timestamp
        );
    } else {
        println!(
            "  TimeDateStamp   : <unreadable: e_lfanew 0x{e_lfanew:X} is outside the first 2048 bytes> (expected 0x{:08X})",
            spec.pe_timestamp
        );
    }
    println!(
        "  file size       : {size} bytes ({}, expected {} bytes)",
        if size == spec.file_size { "match" } else { "MISMATCH" },
        spec.file_size
    );
    if size < 2048 {
        println!("  note            : short file (< 2048 bytes): the header is zero-padded, same rule as anchor.rs");
    }
}

/// `--exe`：离线版本校验。不打开任何进程、不读任何内存，只读磁盘文件。
#[cfg(windows)]
fn check_exe(path: &Path) {
    let spec = anchor::ClientSpec::current();
    println!("== --exe: offline PE version check ==");
    println!("  mode            : file only - no process is opened and no memory is read");
    println!("  path            : {}", path.display());
    println!(
        "  known client    : label {} | rva 0x{:X} | TimeDateStamp 0x{:08X} | file size {}",
        spec.label, spec.rva, spec.pe_timestamp, spec.file_size
    );
    print_pe_items(path, spec);
    match anchor::validate_pe_file(path, spec) {
        Ok(()) => println!("  verdict         : pass (this file matches the known {})", spec.label),
        Err(err) => println!("  verdict         : {}", reason(&err)),
    }
}

/// 默认模式：定位唯一 `malody.exe` → 版本门 → 模块基址与锚点地址 → 200ms 采样 → 结果。
#[cfg(windows)]
fn follow(secs: u64) {
    let spec = anchor::ClientSpec::current();
    println!("== malody4-anchor-check: live read-only check ==");
    println!(
        "  known client    : label {} | rva 0x{:X} | TimeDateStamp 0x{:08X} | file size {}",
        spec.label, spec.rva, spec.pe_timestamp, spec.file_size
    );
    println!("  mode            : read-only (OpenProcess + ReadProcessMemory); no injection, no write, no allocation in the game");
    println!();

    println!("[1/5] locate the single malody.exe");
    let target = match anchor::find_target() {
        Ok(target) => target,
        Err(err) => {
            let r = reason(&err);
            println!("  reason          : {r}");
            println!("  hint            : {}", hint(&r));
            println!();
            println!("  guidance        : start Malody 4.3.7 (malody.exe) and re-run this tool - the anchor check needs a live process;");
            println!("                    if the game is already running, the reason above is the whole diagnosis");
            return;
        }
    };
    println!("  pid             : {}", target.pid);
    println!("  image path      : {}", target.exe_path.display());
    println!("  OpenProcess     : ok (PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, inherit=false)");
    println!();

    println!("[2/5] version gate (anchor::open: live image path cross-check + PE version table)");
    let att = match anchor::open(&target) {
        Ok(att) => {
            println!("  verdict         : pass (live image path matches the module path, PE version matches)");
            att
        }
        Err(err) => {
            let r = reason(&err);
            println!("  reason          : {r}");
            println!("  hint            : {}", hint(&r));
            println!("  PE items of the live image (offline re-check, no memory read):");
            print_pe_items(&target.exe_path, spec);
            return;
        }
    };
    println!();

    println!("[3/5] module base and anchor address");
    let addr = u64::from(att.base_u32) + u64::from(spec.rva);
    println!("  pid             : {}", att.pid);
    println!("  module base     : 0x{:08X} (MODULEENTRY32W.modBaseAddr of malody.exe)", att.base_u32);
    println!("  rva             : 0x{:X}", spec.rva);
    println!(
        "  anchor address  : base + rva = 0x{:08X} + 0x{:X} = 0x{:08X}",
        att.base_u32, spec.rva, addr
    );
    println!("  disk file size  : {} bytes", att.file_size);
    println!();

    // 采样：指针为 0 = 没高亮任何谱面（正常态），非 0 则解析严格文法 `<md5>_<slot>`。
    println!("[4/5] sampling every 200 ms for {} s (use --follow <seconds> to change)", secs);
    let interval = Duration::from_millis(200);
    let window = Duration::from_secs(secs);
    let start = Instant::now();
    let mut index: u32 = 0;
    let mut last: Option<(String, u32)> = None;
    let mut distinct: Vec<(String, u32, u32)> = Vec::new();
    let mut stopped: Option<String> = None;
    loop {
        index += 1;
        let t = start.elapsed().as_secs_f64();
        match anchor::read_identity(&att) {
            Ok(None) => println!("  [{t:>6.1}s] #{index:<3} pointer = 0    -> no selection (nothing highlighted)"),
            Ok(Some(key)) => {
                println!(
                    "  [{t:>6.1}s] #{index:<3} pointer != 0   -> identity md5={} slot={}",
                    key.md5, key.slot
                );
                match distinct
                    .iter_mut()
                    .find(|(md5, slot, _)| *md5 == key.md5 && *slot == key.slot)
                {
                    Some(hit) => hit.2 += 1,
                    None => distinct.push((key.md5.clone(), key.slot, 1)),
                }
                last = Some((key.md5, key.slot));
            }
            Err(err) => {
                let r = reason(&err);
                println!("  [{t:>6.1}s] #{index:<3} read failed    -> {r}");
                println!("             hint: {}", hint(&r));
                stopped = Some(r);
                break;
            }
        }
        if start.elapsed() >= window {
            break;
        }
        std::thread::sleep(interval);
    }
    println!();

    println!("[5/5] result");
    println!("  samples         : {index} in {:.1} s", start.elapsed().as_secs_f64());
    match &last {
        Some((md5, slot)) => {
            println!("  identity key    : md5={md5} slot={slot}");
            println!("  note            : that md5 is the digest of the highlighted .mc file - compare it with md5sum of the file");
        }
        None => println!("  identity key    : none observed (the anchor pointer stayed 0 for the whole window)"),
    }
    if distinct.len() > 1 {
        println!("  distinct keys   : {}", distinct.len());
        for (md5, slot, count) in &distinct {
            println!("                    md5={md5} slot={slot} samples={count}");
        }
    }
    if let Some(r) = &stopped {
        println!("  stopped early   : {r} (sampling aborted, the handle is no longer usable)");
    }
    println!("  index hit path  : the shell logs `md5 -> path` at debug level in logs/mma-shell-*.log (set \"logLevel\": \"debug\"); this tool never queries the index");
}

// ===========================================================================
// 原始字节采样（--dump-identity）与内存扫描（--scan-identity / --find-md5）。
//
// 这一节自己声明 Win32：`anchor.rs` 的 Win32 层是私有的，`Attachment` / `Target` 的
// handle 字段也是私有的，工具既拿不到原始字节也拿不到指针值——而"看原始字节"正是
// --dump-identity 存在的理由。因此工具用**逐字相同的权限**自己开一个句柄：
// PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ，不注入、不写、不在目标进程里分配内存。
// ===========================================================================

/// Win32 句柄（`HANDLE = isize`，与 anchor.rs 同口径）。
#[cfg(windows)]
type Handle = isize;

/// `OpenProcess` / `CreateToolhelp32Snapshot` 的失败句柄。
#[cfg(windows)]
const INVALID_HANDLE_VALUE: Handle = -1isize;

/// `OpenProcess` 权限：查映像路径 + 读内存（同用户下无需提权）。
#[cfg(windows)]
const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
#[cfg(windows)]
const PROCESS_VM_READ: u32 = 0x0010;

/// 身份缓冲字节数（与 `anchor::read_identity_classified` 的 128 字节同口径）。
#[cfg(windows)]
const IDENTITY_BUF: usize = 128;

/// 身份键最长形态：32 位 hex + `_` + 9 位数字。
#[cfg(windows)]
const MAX_IDENTITY_LEN: usize = 32 + 1 + 9;

/// 身份键最短形态：32 位 hex + `_` + 1 位数字。
#[cfg(windows)]
const MIN_IDENTITY_LEN: usize = 32 + 1 + 1;

/// 扫描分块大小（页大小是 4KiB，64KiB 一块足以摊销 ReadProcessMemory 的开销）。
#[cfg(windows)]
const SCAN_CHUNK: usize = 64 * 1024;
#[cfg(windows)]
const PAGE_SIZE: u64 = 4096;

/// 扫描字节上限：512 MB。工具绝不因为"进程很大"而挂住。
#[cfg(windows)]
const MAX_SCAN_BYTES: u64 = 512 * 1024 * 1024;

/// `--find-md5` 的时间预算（该模式不额外要秒数参数：它就是同一个扫描器的封装）。
#[cfg(windows)]
const FIND_MD5_SECS: u64 = 20;

/// 每条身份键最多打印多少个地址（出现次数仍然完整统计）。
#[cfg(windows)]
const MAX_HIT_ADDRS: usize = 16;

/// 用户态地址空间上界（x64 调用方）：防止 VirtualQueryEx 走到内核空间。
#[cfg(windows)]
const MAX_USER_ADDR: u64 = 0x7FFF_FFFF_FFFF;

/// 采样间隔 50ms：比壳的 200ms 轮询快 4 倍，用来抓撕裂读。
#[cfg(windows)]
const DUMP_INTERVAL_MS: u64 = 50;

/// 内存区域状态与保护位。
#[cfg(windows)]
const MEM_COMMIT: u32 = 0x1000;
#[cfg(windows)]
const PAGE_NOACCESS: u32 = 0x01;
#[cfg(windows)]
const PAGE_GUARD: u32 = 0x100;

/// `MEMORY_BASIC_INFORMATION`：**x64 调用方布局**（align 8，`size_of == 48`）。
///
/// 32 位目标进程也用这个尺寸：VirtualQueryEx 是跨位宽的，写回的是调用方布局，
/// 32 位目标只会让地址落在 4GB 以下。
#[cfg(windows)]
#[repr(C)]
#[allow(non_snake_case)]
struct MEMORY_BASIC_INFORMATION {
    BaseAddress: *mut u8,     // @0
    AllocationBase: *mut u8,  // @8
    AllocationProtect: u32,   // @16
    // @20：4 字节填充（RegionSize 是 SIZE_T，需 8 字节对齐）
    RegionSize: usize,  // @24
    State: u32,         // @32
    Protect: u32,       // @36
    Type: u32,          // @40
                        // @44：尾部 ABI 补白 ⇒ size_of == 48
}

/// 布局断言写成编译期常量：布局一旦不对就直接编译失败，绝不让工具静默读错结构。
#[cfg(windows)]
const _MBI_LAYOUT_IS_X64: () = assert!(
    std::mem::size_of::<MEMORY_BASIC_INFORMATION>() == 48,
    "MEMORY_BASIC_INFORMATION must use the x64 caller layout (48 bytes)"
);

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn OpenProcess(dwDesiredAccess: u32, bInheritHandle: i32, dwProcessId: u32) -> Handle;
    fn CloseHandle(hObject: Handle) -> i32;
    fn ReadProcessMemory(
        hProcess: Handle,
        lpBaseAddress: *const u8,
        lpBuffer: *mut u8,
        nSize: usize,
        lpNumberOfBytesRead: *mut usize,
    ) -> i32;
    fn VirtualQueryEx(
        hProcess: Handle,
        lpAddress: *const u8,
        lpBuffer: *mut MEMORY_BASIC_INFORMATION,
        dwLength: usize,
    ) -> usize;
}

/// 读目标进程内存：返回实际读到的字节数；调用失败或 0 字节 → `None`。
#[cfg(windows)]
fn read_proc(handle: Handle, addr: u64, buf: &mut [u8]) -> Option<usize> {
    let mut got: usize = 0;
    let ok = unsafe {
        ReadProcessMemory(
            handle,
            addr as *const u8,
            buf.as_mut_ptr(),
            buf.len(),
            &mut got,
        )
    };
    if ok == 0 || got == 0 {
        None
    } else {
        Some(got)
    }
}

/// 工具自己的只读句柄（`Drop` 时关闭）。
#[cfg(windows)]
struct Probe {
    handle: Handle,
    /// 目标进程 PID（诊断输出用）。
    pid: u32,
    /// 锚点指针槽位地址 = 模块基址 + RVA。
    anchor_addr: u64,
}

#[cfg(windows)]
impl Drop for Probe {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.handle) };
    }
}

/// 新模式公共表头（模式名 / 已知客户端 / 只读声明）。
#[cfg(windows)]
fn mode_header(title: &str) {
    let spec = anchor::ClientSpec::current();
    println!("== {title} ==");
    println!(
        "  known client    : label {} | rva 0x{:X} | TimeDateStamp 0x{:08X} | file size {}",
        spec.label, spec.rva, spec.pe_timestamp, spec.file_size
    );
    println!("  mode            : read-only (OpenProcess + ReadProcessMemory); no injection, no write, no allocation in the game");
}

/// 定位唯一 `malody.exe` → 版本门（`anchor::open`，与壳同一条路径）→ 开工具自己的只读句柄。
///
/// 任何一步失败都按壳 state 帧的闭集打印 `reason` + `hint` 并返回 `None`——新模式的失败形态
/// 与默认模式逐字一致：`process-not-found` / `multiple-instances` / `access-denied` /
/// `target-mismatch:<短句>` / `bad-read`。
#[cfg(windows)]
fn attach() -> Option<Probe> {
    let spec = anchor::ClientSpec::current();
    println!("[1/3] locate the single malody.exe");
    let target = match anchor::find_target() {
        Ok(target) => target,
        Err(err) => {
            let r = reason(&err);
            println!("  reason          : {r}");
            println!("  hint            : {}", hint(&r));
            println!();
            println!("  guidance        : start Malody 4.3.7 (malody.exe) and re-run this tool - this mode needs a live process;");
            println!("                    nothing was read, nothing was written, and no game-side file is involved");
            return None;
        }
    };
    println!("  pid             : {}", target.pid);
    println!("  image path      : {}", target.exe_path.display());
    println!("  OpenProcess     : ok (PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, inherit=false)");
    println!();

    println!("[2/3] version gate (anchor::open: live image path cross-check + PE version table)");
    if let Err(err) = anchor::open(&target) {
        let r = reason(&err);
        println!("  reason          : {r}");
        println!("  hint            : {}", hint(&r));
        println!("  PE items of the live image (offline re-check, no memory read):");
        print_pe_items(&target.exe_path, spec);
        return None;
    }
    println!("  verdict         : pass (live image path matches the module path, PE version matches)");

    // 工具自己的句柄：权限与 anchor 逐字相同，只多一个能读原始字节的入口。
    let handle = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ,
            0,
            target.pid,
        )
    };
    if handle == 0 || handle == INVALID_HANDLE_VALUE {
        let r = anchor::AnchorError::AccessDenied.reason();
        println!("  reason          : {r}");
        println!("  hint            : {}", hint(r));
        return None;
    }
    let anchor_addr = u64::from(target.base_u32) + u64::from(spec.rva);
    println!(
        "  module base     : 0x{:08X} | rva 0x{:X} | anchor address 0x{:08X}",
        target.base_u32, spec.rva, anchor_addr
    );
    println!();
    Some(Probe {
        handle,
        pid: target.pid,
        anchor_addr,
    })
}

// ------------------------------------------------------ --dump-identity --

/// 一次采样的分类（字面量与壳 `IdentityRead` 一致）。
#[cfg(windows)]
#[derive(Clone, Copy, PartialEq)]
enum SampleClass {
    /// 指针为 0：当前没有选中谱面（正常态）。
    Empty,
    /// 解析出身份键。
    Key,
    /// 指针非 0，但目标缓冲当前不是身份键（软失败：陈旧指针 / 半写 / 撕裂）。
    Unparsable,
}

/// 一次采样的产物：指针 + **原始 128 字节** + 解析结果。
#[cfg(windows)]
struct DumpSample {
    ptr: u32,
    /// 第二段（缓冲）是否真的读到；false = 缓冲读失败（软失败）。
    buffer_read: bool,
    raw: [u8; IDENTITY_BUF],
    parsed: Option<anchor::IdentityKey>,
}

#[cfg(windows)]
impl DumpSample {
    fn class(&self) -> SampleClass {
        if self.ptr == 0 {
            SampleClass::Empty
        } else if !self.buffer_read || self.parsed.is_none() {
            SampleClass::Unparsable
        } else {
            SampleClass::Key
        }
    }

    /// 解析结果行：`Empty` / `Key { md5, slot }` / `Unparsable`（并说明是哪一种失败）。
    fn parse_line(&self) -> String {
        if self.ptr == 0 {
            return "Empty (pointer is 0: nothing highlighted - normal state)".to_string();
        }
        if !self.buffer_read {
            return "Unparsable (the 128-byte buffer at the pointer could not be read: stale or half-written pointer)"
                .to_string();
        }
        match &self.parsed {
            Some(key) => format!("Key {{ md5 = {}, slot = {} }}", key.md5, key.slot),
            None => "Unparsable (grammar: not <32 hex>_<1..9 digits>)".to_string(),
        }
    }

    /// 第一个 NUL 之前的字节。壳的读法把缓冲预置零，所以这就是"游戏写进去的字符串"。
    fn c_string(&self) -> String {
        let end = self
            .raw
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(self.raw.len());
        String::from_utf8_lossy(&self.raw[..end]).to_string()
    }
}

/// 采一次：指针槽位 4 字节必须全读到（否则按壳的口径是硬失败 `bad-read`），
/// 指针非 0 再读 128 字节缓冲——缓冲预置零、短读时尾部保持零，与 anchor 逐字相同。
#[cfg(windows)]
fn sample_identity(probe: &Probe) -> Result<DumpSample, anchor::AnchorError> {
    let mut ptr_buf = [0u8; 4];
    if read_proc(probe.handle, probe.anchor_addr, &mut ptr_buf) != Some(ptr_buf.len()) {
        return Err(anchor::AnchorError::BadRead);
    }
    let ptr = u32::from_le_bytes(ptr_buf);
    let mut raw = [0u8; IDENTITY_BUF];
    if ptr == 0 {
        return Ok(DumpSample {
            ptr,
            buffer_read: false,
            raw,
            parsed: None,
        });
    }
    let buffer_read = read_proc(probe.handle, u64::from(ptr), &mut raw).is_some();
    let parsed = if buffer_read {
        anchor::parse_identity_key(&raw)
    } else {
        None
    };
    Ok(DumpSample {
        ptr,
        buffer_read,
        raw,
        parsed,
    })
}

/// 原始字节的 hex + ASCII 双列（每行 32 字节，行首地址 = 指针 + 行偏移）。
#[cfg(windows)]
fn print_hexdump(ptr: u32, raw: &[u8]) {
    for (row, chunk) in raw.chunks(32).enumerate() {
        let addr = u64::from(ptr) + (row * 32) as u64;
        let mut hex = String::with_capacity(32 * 3);
        for b in chunk {
            hex.push_str(&format!("{b:02x} "));
        }
        while hex.len() < 32 * 3 {
            hex.push(' ');
        }
        let ascii: String = chunk
            .iter()
            .map(|&b| {
                if (0x20..0x7F).contains(&b) {
                    char::from(b)
                } else {
                    '.'
                }
            })
            .collect();
        println!("      0x{addr:08X}  {hex}|{ascii}|");
    }
}

/// `--dump-identity <seconds>`：每 50ms 采一次并打印原始字节、指针与解析结果，最后给汇总。
///
/// 为什么需要它：`parse_identity_key` 只验**文法**（32 位 hex + `_` + 1..9 位数字）。游戏原地
/// 复用身份缓冲时，"前半新、后半旧"的撕裂读同样能解析出一个"合法"的 md5，于是壳去索引里查
/// 一个磁盘上根本不存在的谱面。原始字节是唯一能看出半新半旧的地方。
#[cfg(windows)]
fn dump_identity(secs: u64) {
    mode_header("--dump-identity: raw identity-buffer sampling");
    println!(
        "  interval        : {DUMP_INTERVAL_MS} ms (4x faster than the shell's 200 ms poller)"
    );
    println!("  window          : {secs} s");
    println!("  prints          : pointer, raw {IDENTITY_BUF} bytes (hex + ASCII) and the parsed result per sample");
    println!();

    let probe = match attach() {
        Some(probe) => probe,
        None => return,
    };

    println!(
        "[3/3] sampling pid {} every {DUMP_INTERVAL_MS} ms for {secs} s (anchor 0x{:08X} -> buffer)",
        probe.pid, probe.anchor_addr
    );
    let interval = Duration::from_millis(DUMP_INTERVAL_MS);
    let window = Duration::from_secs(secs);
    let start = Instant::now();
    let mut index: u64 = 0;
    let mut empty: u64 = 0;
    let mut unparsable: u64 = 0;
    let mut unparsable_read: u64 = 0;
    let mut keys: u64 = 0;
    let mut distinct: Vec<(String, u32, u64)> = Vec::new();
    let mut prev_class: Option<SampleClass> = None;
    let mut prev_key: Option<(u32, String)> = None;
    let mut unstable: u64 = 0;
    let mut unstable_same_ptr: u64 = 0;
    let mut stopped: Option<String> = None;
    loop {
        index += 1;
        let t = start.elapsed().as_secs_f64();
        match sample_identity(&probe) {
            Ok(sample) => {
                let class = sample.class();
                println!(
                    "  [{t:>6.3}s] #{index:<3} pointer = 0x{:08X}  buffer = {}",
                    sample.ptr,
                    if sample.buffer_read {
                        format!("{IDENTITY_BUF} bytes read")
                    } else {
                        "not read".to_string()
                    }
                );
                if sample.buffer_read {
                    print_hexdump(sample.ptr, &sample.raw);
                    println!("      c-string    : {:?}", sample.c_string());
                }
                println!("      parse       : {}", sample.parse_line());
                match class {
                    SampleClass::Empty => empty += 1,
                    SampleClass::Key => {
                        keys += 1;
                        let key = sample.parsed.as_ref().expect("Key implies a parsed key");
                        match distinct
                            .iter_mut()
                            .find(|(md5, slot, _)| *md5 == key.md5 && *slot == key.slot)
                        {
                            Some(hit) => hit.2 += 1,
                            None => distinct.push((key.md5.clone(), key.slot, 1)),
                        }
                        // 连续两次采样都是身份键但 md5 不同：指针跟着变 = 正常换谱，
                        // 指针不变 = 游戏原地重写了缓冲（撕裂读的前提条件）。
                        if prev_class == Some(SampleClass::Key) {
                            if let Some((prev_ptr, prev_md5)) = &prev_key {
                                if *prev_md5 != key.md5 {
                                    unstable += 1;
                                    let same_ptr = *prev_ptr == sample.ptr;
                                    if same_ptr {
                                        unstable_same_ptr += 1;
                                    }
                                    println!(
                                        "      !! md5 differs from the previous sample: {prev_md5} -> {} (pointer 0x{:08X} -> 0x{:08X}{})",
                                        key.md5,
                                        prev_ptr,
                                        sample.ptr,
                                        if same_ptr {
                                            ", SAME pointer = in-place buffer rewrite (torn read)"
                                        } else {
                                            ", pointer also changed = a chart switch"
                                        }
                                    );
                                }
                            }
                        }
                        prev_key = Some((sample.ptr, key.md5.clone()));
                    }
                    SampleClass::Unparsable => {
                        unparsable += 1;
                        if !sample.buffer_read {
                            unparsable_read += 1;
                        }
                    }
                }
                prev_class = Some(class);
            }
            Err(err) => {
                let r = reason(&err);
                println!("  [{t:>6.3}s] #{index:<3} read failed    -> {r}");
                println!("      hint        : {}", hint(&r));
                stopped = Some(r);
                break;
            }
        }
        if start.elapsed() >= window {
            break;
        }
        std::thread::sleep(interval);
    }
    println!();

    println!("[3/3] result");
    println!(
        "  samples         : {index} in {:.2} s ({DUMP_INTERVAL_MS} ms interval)",
        start.elapsed().as_secs_f64()
    );
    println!("  Empty           : {empty}");
    println!("  Unparsable      : {unparsable} (grammar {}, buffer read failed {unparsable_read})", unparsable - unparsable_read);
    println!("  Key             : {keys}");
    println!("  distinct md5s   : {}", distinct.len());
    for (md5, slot, count) in &distinct {
        println!("                    md5={md5} slot={slot} samples={count}");
    }
    println!(
        "  unstable pairs  : {unstable} consecutive key samples whose md5 differs ({unstable_same_ptr} with the SAME pointer = in-place buffer rewrite)"
    );
    if unstable_same_ptr > 0 {
        println!("  note            : the anchor's buffer is rewritten in place - a read landing between the two writes can return a md5 that exists on no disk (the parser only checks grammar)");
    }
    if let Some(r) = &stopped {
        println!("  stopped early   : {r} (sampling aborted, the handle is no longer usable)");
    }
    println!("  raw bytes       : printed per sample above (hex + ASCII); the shell only keeps the parsed key");
}

// -------------------------------------------------- --scan-identity / --find-md5 --

/// MB 文本（诊断输出用）。
#[cfg(windows)]
fn mb(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
}

/// 一条身份键的命中统计。
#[cfg(windows)]
struct IdentityHit {
    md5: String,
    slot: u32,
    /// 出现次数（完整计数）。
    count: u64,
    /// 出现地址（最多 `MAX_HIT_ADDRS` 个，按扫描顺序 = 升序）。
    addrs: Vec<u64>,
}

/// 扫描统计。
#[cfg(windows)]
struct ScanStats {
    regions_walked: u64,
    regions_read: u64,
    bytes_read: u64,
    skipped_pages: u64,
    elapsed: Duration,
    stop_reason: &'static str,
}

/// 区域是否可读：必须已提交、非 GUARD、非 NOACCESS，且基本保护位在可读集合里。
///
/// 0x100 GUARD / 0x200 NOCACHE / 0x400 WRITECOMBINE 是修饰位，故取低 8 位拿基本保护位；
/// PAGE_EXECUTE(0x10) 单独出现时不可读。
#[cfg(windows)]
fn region_readable(state: u32, protect: u32) -> bool {
    if state != MEM_COMMIT || protect & PAGE_GUARD != 0 || protect & PAGE_NOACCESS != 0 {
        return false;
    }
    matches!(protect & 0xFF, 0x02 | 0x04 | 0x08 | 0x20 | 0x40 | 0x80)
}

/// 内存扫描器：VirtualQueryEx 枚举区域 → 分块 ReadProcessMemory → 在窗口里找身份键。
#[cfg(windows)]
struct Scanner<'a> {
    probe: &'a Probe,
    /// 只保留这个 md5（`--find-md5`）；None = 记录全部身份键（`--scan-identity`）。
    filter: Option<&'a str>,
    hits: Vec<IdentityHit>,
    occurrences: u64,
    stats: ScanStats,
    started: Instant,
    last_progress: Instant,
    budget: Duration,
    cap_hit: bool,
    budget_hit: bool,
}

#[cfg(windows)]
impl<'a> Scanner<'a> {
    fn new(probe: &'a Probe, filter: Option<&'a str>, budget: Duration) -> Self {
        let now = Instant::now();
        Scanner {
            probe,
            filter,
            hits: Vec::new(),
            occurrences: 0,
            stats: ScanStats {
                regions_walked: 0,
                regions_read: 0,
                bytes_read: 0,
                skipped_pages: 0,
                elapsed: Duration::ZERO,
                stop_reason: "walked the whole address space (nothing left to read)",
            },
            started: now,
            last_progress: now,
            budget,
            cap_hit: false,
            budget_hit: false,
        }
    }

    /// 在窗口里找身份键。窗口 = 上一块的尾字节 + 本块新字节，用来接住跨分块的身份键。
    ///
    /// 计数规则：只记"起点落在新字节里、或跨过尾字节边界"的命中——完全落在尾字节里的命中
    /// 上一块已经计过，否则同一处会被相邻两块重复计数。
    fn scan_window(&mut self, window: &[u8], window_addr: u64, carry_len: usize) {
        let mut i = 0usize;
        while i + MIN_IDENTITY_LEN <= window.len() {
            if !window[i].is_ascii_hexdigit()
                || window[i + 32] != b'_'
                || !window[i..i + 32].iter().all(u8::is_ascii_hexdigit)
            {
                i += 1;
                continue;
            }
            let mut j = i + 33;
            while j < window.len() && window[j].is_ascii_digit() {
                j += 1;
            }
            let digits = j - (i + 33);
            // j == window.len()：数字串被分块末尾截断，无法确定后面还有没有第 10 位数字
            // （文法要求 1..=9 位），故不记。它的起点必然落在窗口末尾 42 字节内，
            // 下一块的窗口会带着终止字节重新看到它。
            if digits == 0 || digits > 9 || j >= window.len() {
                i += 1;
                continue;
            }
            if i + (j - i) <= carry_len {
                i += 1; // 完全落在上一块里，已计过
                continue;
            }
            // 文法判定复用壳的解析器：扫描器与壳对"什么是身份键"永远同一套规则。
            if let Some(key) = anchor::parse_identity_key(&window[i..j]) {
                self.record(&key, window_addr + i as u64);
            }
            i += 1;
        }
    }

    fn record(&mut self, key: &anchor::IdentityKey, addr: u64) {
        if let Some(want) = self.filter {
            if key.md5 != want {
                return;
            }
        }
        self.occurrences += 1;
        match self
            .hits
            .iter_mut()
            .find(|h| h.md5 == key.md5 && h.slot == key.slot)
        {
            Some(hit) => {
                hit.count += 1;
                if hit.addrs.len() < MAX_HIT_ADDRS {
                    hit.addrs.push(addr);
                }
            }
            None => self.hits.push(IdentityHit {
                md5: key.md5.clone(),
                slot: key.slot,
                count: 1,
                addrs: vec![addr],
            }),
        }
    }

    /// 到一个区域：64KiB 分块读；整块失败就退到 4KiB 逐页重试，单页失败只跳过该页。
    fn scan_region(&mut self, base: u64, size: u64) {
        self.stats.regions_read += 1;
        let end = base.saturating_add(size);
        let mut cur = base;
        let mut buf = vec![0u8; SCAN_CHUNK];
        let mut page = vec![0u8; PAGE_SIZE as usize];
        // 跨分块保留的尾字节（最多一条身份键的长度）与拼装窗口，避免每块重新分配。
        let mut carry: Vec<u8> = Vec::with_capacity(MAX_IDENTITY_LEN);
        let mut window: Vec<u8> = Vec::with_capacity(SCAN_CHUNK + MAX_IDENTITY_LEN);
        while cur < end && !self.cap_hit && !self.budget_hit {
            let want = SCAN_CHUNK.min((end - cur) as usize);
            let got = read_proc(self.probe.handle, cur, &mut buf[..want]);
            let (data, addr): (&[u8], u64) = match got {
                Some(n) => (&buf[..n], cur),
                None => {
                    // 整块失败多半是块尾跨进了不可读页（页边界的短读是常态）：
                    // 退到 4KiB 逐页重试，只跳过真正读不到的那一页，绝不让整个扫描失败。
                    let page_want = (PAGE_SIZE.min(end - cur)) as usize;
                    match read_proc(self.probe.handle, cur, &mut page[..page_want]) {
                        Some(n) => (&page[..n], cur),
                        None => {
                            self.stats.skipped_pages += 1;
                            // 跳页 ⇒ 内存不连续，尾字节必须丢掉（不能跨过空洞拼接）
                            carry.clear();
                            cur += PAGE_SIZE.min(end - cur);
                            self.check_bounds();
                            continue;
                        }
                    }
                }
            };
            window.clear();
            window.extend_from_slice(&carry);
            window.extend_from_slice(data);
            let window_addr = addr - carry.len() as u64;
            self.scan_window(&window, window_addr, carry.len());
            let keep = MAX_IDENTITY_LEN.min(window.len());
            carry.clear();
            carry.extend_from_slice(&window[window.len() - keep..]);
            self.stats.bytes_read += data.len() as u64;
            cur += data.len() as u64;
            self.check_bounds();
            self.progress(cur);
        }
    }

    /// 字节上限与时间预算（两者任一到达就停，并把原因写进结果块）。
    fn check_bounds(&mut self) {
        if self.stats.bytes_read >= MAX_SCAN_BYTES {
            self.cap_hit = true;
            self.stats.stop_reason = "byte cap reached (512 MB): the scan stopped early";
        }
        if self.started.elapsed() >= self.budget {
            self.budget_hit = true;
            self.stats.stop_reason = "time budget reached: the scan stopped early";
        }
    }

    /// 每秒一条进度行（扫描超过一两秒时能看到进展）。
    fn progress(&mut self, cur: u64) {
        if self.last_progress.elapsed() < Duration::from_secs(1) {
            return;
        }
        self.last_progress = Instant::now();
        println!(
            "  ... {:.1}s | scanned {} | regions {} read of {} walked | pages skipped {} | identities {} ({} occurrences) | at 0x{:X}",
            self.started.elapsed().as_secs_f64(),
            mb(self.stats.bytes_read),
            self.stats.regions_read,
            self.stats.regions_walked,
            self.stats.skipped_pages,
            self.hits.len(),
            self.occurrences,
            cur
        );
    }

    /// 走一遍地址空间（VirtualQueryEx 从 0 递增，直到查不到区域或到达用户态上界）。
    fn run(&mut self) {
        let mut addr: u64 = 0;
        let mut mbi: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
        while addr < MAX_USER_ADDR {
            if self.cap_hit || self.budget_hit {
                break;
            }
            let written = unsafe {
                VirtualQueryEx(
                    self.probe.handle,
                    addr as *const u8,
                    &mut mbi,
                    std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
                )
            };
            if written == 0 {
                break; // 地址空间走完（或查询失败：没有可枚举的区域）
            }
            let base = mbi.BaseAddress as u64;
            let size = mbi.RegionSize as u64;
            if size == 0 {
                break; // 防御：RegionSize 为 0 会让地址不前进
            }
            self.stats.regions_walked += 1;
            if region_readable(mbi.State, mbi.Protect) {
                self.scan_region(base, size);
            }
            match base.checked_add(size) {
                Some(next) if next > addr => addr = next,
                _ => break,
            }
        }
        self.stats.elapsed = self.started.elapsed();
    }
}

/// 结果块（两个扫描模式共用）。
#[cfg(windows)]
fn print_scan_result(scanner: &Scanner) {
    let stats = &scanner.stats;
    println!();
    println!("[3/3] result");
    if let Some(want) = scanner.filter {
        println!("  md5 searched    : {want}");
        println!(
            "  found           : {}",
            if scanner.occurrences > 0 {
                format!(
                    "YES - {} identity string(s) <md5>_<slot> in the scanned bytes",
                    scanner.occurrences
                )
            } else {
                "NO - no identity string <md5>_<slot> in the scanned bytes".to_string()
            }
        );
    }
    println!("  regions walked  : {} (VirtualQueryEx)", stats.regions_walked);
    println!(
        "  regions read    : {} (committed + readable + non-guarded)",
        stats.regions_read
    );
    println!(
        "  bytes scanned   : {} ({} bytes)",
        mb(stats.bytes_read),
        stats.bytes_read
    );
    println!(
        "  byte cap        : {} (512 MB) - {}",
        mb(MAX_SCAN_BYTES),
        if scanner.cap_hit { "REACHED" } else { "not reached" }
    );
    println!(
        "  time budget     : {:.0} s - {}",
        scanner.budget.as_secs_f64(),
        if scanner.budget_hit { "REACHED" } else { "not reached" }
    );
    println!(
        "  skipped pages   : {} (unreadable pages inside readable regions; a partial read around a page boundary is normal and never fails the scan)",
        stats.skipped_pages
    );
    println!("  stopped         : {}", stats.stop_reason);
    println!(
        "  matches         : {} occurrence(s) of {} distinct identity string(s)",
        scanner.occurrences,
        scanner.hits.len()
    );
    for hit in &scanner.hits {
        let shown = hit
            .addrs
            .iter()
            .map(|a| format!("0x{a:08X}"))
            .collect::<Vec<_>>()
            .join(", ");
        let more = if hit.count as usize > hit.addrs.len() {
            format!(" (+{} more)", hit.count as usize - hit.addrs.len())
        } else {
            String::new()
        };
        println!(
            "                    md5={} slot={} seen={} at {}{}",
            hit.md5, hit.slot, hit.count, shown, more
        );
    }
    if scanner.hits.is_empty() {
        println!("                    (none)");
    }
    println!("  elapsed         : {:.2} s", stats.elapsed.as_secs_f64());
    println!("  note            : memory is only read: nothing is written to the game, nothing is injected, no game-side file is used");
}

/// `--scan-identity <seconds>`：全内存找身份键（游戏自己的 UI 里那份也能看见）。
#[cfg(windows)]
fn scan_identity_mode(secs: u64) {
    mode_header("--scan-identity: scan the process memory for identity strings");
    println!("  pattern         : [0-9a-f]{{32}}_[0-9]{{1,9}}, grammar validated by anchor::parse_identity_key (the shell's own parser)");
    println!("  time budget     : {secs} s | byte cap {} (512 MB)", mb(MAX_SCAN_BYTES));
    println!();
    let probe = match attach() {
        Some(probe) => probe,
        None => return,
    };
    println!(
        "[3/3] scanning pid {} memory (committed, readable, non-guarded regions, {SCAN_CHUNK} byte chunks)",
        probe.pid
    );
    let mut scanner = Scanner::new(&probe, None, Duration::from_secs(secs));
    scanner.run();
    print_scan_result(&scanner);
}

/// `--find-md5 <hex32>`：同一个扫描器 + 一个 md5 过滤。
#[cfg(windows)]
fn find_md5_mode(md5: String) {
    mode_header("--find-md5: is that md5 held in memory as an identity string?");
    println!("  md5             : {md5} (given explicitly on the command line; the tool hardcodes none)");
    println!("  searched as     : <md5>_<slot> identity strings, the same pattern --scan-identity uses");
    println!("  time budget     : {FIND_MD5_SECS} s | byte cap {} (512 MB)", mb(MAX_SCAN_BYTES));
    println!();
    let probe = match attach() {
        Some(probe) => probe,
        None => return,
    };
    println!(
        "[3/3] scanning pid {} memory for md5={md5} ({SCAN_CHUNK} byte chunks)",
        probe.pid
    );
    let mut scanner = Scanner::new(&probe, Some(&md5), Duration::from_secs(FIND_MD5_SECS));
    scanner.run();
    print_scan_result(&scanner);
}

#[cfg(windows)]
fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse_args(&raw) {
        Ok(Parsed::Help) => {
            print!("{USAGE}");
            return;
        }
        Ok(Parsed::Run(args)) => args,
        Err(msg) => {
            eprintln!("malody4-anchor-check: {msg}");
            eprintln!();
            eprint!("{USAGE}");
            std::process::exit(2);
        }
    };
    match args.mode {
        Mode::Exe(path) => check_exe(Path::new(&path)),
        Mode::Follow(secs) => follow(secs),
        Mode::DumpIdentity(secs) => dump_identity(secs),
        Mode::ScanIdentity(secs) => scan_identity_mode(secs),
        Mode::FindMd5(md5) => find_md5_mode(md5),
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("malody4-anchor-check: Windows only (platform-unsupported)");
    std::process::exit(2);
}
