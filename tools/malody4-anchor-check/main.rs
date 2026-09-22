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
  malody4-anchor-check [--exe <path>] [--follow <seconds>]

OPTIONS:
  --exe <path>        Offline PE version check of ANY file. No process is touched:
                      prints e_lfanew, TimeDateStamp, file size and the verdict.
  --follow <seconds>  Sample the anchor pointer and the identity key every 200 ms
                      for this many seconds (default 10).
  -h, --help          Print this help.

EXIT CODES:
  0  the diagnostic ran to completion (read the `verdict` / `reason` lines)
  2  usage error, or unsupported platform (this tool is Windows-only)

NOTES:
  - The tool never injects into the game, never writes to it and never allocates
    memory inside it; it only enumerates processes/modules and reads memory.
  - It does NOT query the chart index and does NOT implement MD5. The `md5 -> path`
    hit path is logged by the desktop shell (logLevel: debug) in logs/mma-shell-*.log.
";

#[cfg(windows)]
struct Args {
    exe: Option<String>,
    follow_secs: u64,
}

#[cfg(windows)]
enum Parsed {
    Run(Args),
    Help,
}

#[cfg(windows)]
fn parse_args(raw: &[String]) -> Result<Parsed, String> {
    let mut exe: Option<String> = None;
    let mut follow_secs: u64 = 10;
    let mut i = 0usize;
    while i < raw.len() {
        match raw[i].as_str() {
            "-h" | "--help" => return Ok(Parsed::Help),
            "--exe" => {
                let value = raw.get(i + 1).ok_or("--exe needs a file path")?;
                exe = Some(value.clone());
                i += 2;
            }
            "--follow" => {
                let value = raw.get(i + 1).ok_or("--follow needs a number of seconds")?;
                follow_secs = value
                    .parse::<u64>()
                    .map_err(|_| format!("--follow expects whole seconds, got {value:?}"))?;
                i += 2;
            }
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    Ok(Parsed::Run(Args { exe, follow_secs }))
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
fn follow(args: &Args) {
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
    println!("[4/5] sampling every 200 ms for {} s (use --follow <seconds> to change)", args.follow_secs);
    let interval = Duration::from_millis(200);
    let window = Duration::from_secs(args.follow_secs);
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
    match &args.exe {
        Some(path) => check_exe(Path::new(path)),
        None => follow(&args),
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("malody4-anchor-check: Windows only (platform-unsupported)");
    std::process::exit(2);
}
