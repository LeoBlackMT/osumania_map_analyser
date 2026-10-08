// lazer-offsets-gen —— osu!lazer 偏移表生成器（dev-time 工具，裸 rustc 编译，零外部依赖）
//
// 构建（在仓库根目录执行）：
//   rustc --edition 2021 -O -A dead_code -o temp/lazer-offsets-gen.exe tools/lazer-offsets-gen/main.rs
//   --edition 2021：裸 rustc 默认 edition 2015。
//   -A dead_code    ：各子命令只用到共享模块的一部分（与 tools/malody4-anchor-check 同法）。
//
// 为什么要入库（DEC-23 取代 DEC-16 ②）：lazer 是**每周级更新**的游戏，每次更新都要重新
// 提取一次偏移；工具放 temp/ 会随验收一起被删，"≤1 天恢复"就变成"先重建工具再计时"。
// 本工具不参与产品运行时：它只在开发者机器上跑，产品侧一律走 ReadProcessMemory（只读）。
//
// 五个子命令（完整手册见同目录 README.md）：
//   collect  —— 找到正在跑的 lazer（x64）进程 → 采一份 dump（dotnet-dump collect）；
//   extract  —— 离线读 dump：扫锚点 → `dumpobj` 逐级解引用 → SOS 中间件（含原始 transcript）；
//   il       —— 读 lazer 安装目录下的托管程序集元数据 → IL 结构清单（+ 跨版本 diff）；
//   emit     —— SOS × IL 双见证校验 → 出 `desktop/src/osu/offsets.rs::load` 认的 JSON；
//   self-test—— 无游戏自测：fixture 输入跑通 extract 之后的全部管线（含失败用例）。
//
// 只读保证：不注入、不写游戏、不改游戏内存、不挂调试器；采集走游戏自带的 .NET 诊断通道
// （`dotnet-dump collect`，会短暂挂起游戏全部线程以写 dump，实测 ~70 s / ~3 GB）。

mod chain;
mod emit;
mod ilmeta;
mod live;
mod minidump;
mod names;
mod runtime;
mod selftest;
mod sos;
mod spec;

use chain::BlockSource;
use emit::{EmitOutcome, Refusal};
use minidump::Dump;
use sos::{AnalyzerRun, SosIntermediate, SosObject};
use spec::{RUNTIME_MIN_PROBES, RUNTIME_PROBES};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const TOOL_VERSION: &str = "0.1.0";

/// 我们在 2026-09-27 真机实测的 dump 体量与挂起时长（README 与 `collect` 输出都引用它）。
const DUMP_BYTES_MEASURED: u64 = 3_174_352_952;
const DUMP_SECONDS_MEASURED: u64 = 70;
/// dump 落盘需要的余量（3 GB 实测 + 峰值余量）。
const DUMP_FREE_SPACE_MIN: u64 = 4 * 1024 * 1024 * 1024;
/// 锚点扫描分块。
const SCAN_CHUNK: usize = 4 * 1024 * 1024;
/// 锚点命中上限（`--max-anchor-hits` 可覆盖）。
const DEFAULT_MAX_ANCHOR_HITS: usize = 8;

const USAGE: &str = "\
lazer-offsets-gen - dev-time osu!lazer offset-table generator (SOS + IL metadata)

USAGE:
  lazer-offsets-gen collect   [--out <dir>] [--dry-run] [--pid <n>] [--analyzer <path>]
                              [--dump-type Heap|Full|Mini] [--force]
  lazer-offsets-gen extract   --dump <dmp> --out <dir> [--analyzer <path>]
                              [--anchor <hex-bytes>] [--anchor-delta <site-delta>]
                              [--max-anchor-hits <n>] [--expect-vtable <hex>]... [--table <json>]
                              [--lazer-version <v>] [--runtime-version <v>]
  lazer-offsets-gen il        --out <dir> (--lazer-dir <dir> | --sos <tsv>)
                              [--assembly <dll>]... [--diff <old-il.tsv>]
  lazer-offsets-gen emit      --sos <tsv> --il <tsv> [--out <json>] [--deploy <shell-exe-dir>]
                              [--report-dir <dir>] [--allow-fixtures] [--allow-version-mismatch]
  lazer-offsets-gen live      [--out <dir>] [--lazer-dir <dir>] [--pid <n>] [--json]
  lazer-offsets-gen self-test --fixtures <dir> [--work <dir>]
  lazer-offsets-gen -h | --help

END-TO-END (lazer running):
  1. collect --out %TEMP%\\lazer-offsets-gen\\run1 --dry-run      # see the exact command first
  2. collect --out %TEMP%\\lazer-offsets-gen\\run1                # ~70 s, ~3 GB
  3. extract --dump %TEMP%\\lazer-offsets-gen\\run1\\lazer-<ts>.dmp --out %TEMP%\\lazer-offsets-gen\\run1
  4. il      --sos <run1>\\sos-intermediate-<ts>.tsv --out <run1>
  5. emit    --sos <run1>\\sos-intermediate-<ts>.tsv --il <run1>\\il-inventory-<ts>.tsv \\
             --deploy <dir with mma-shell.exe>
  OR 1-CLICK LIVE:
  lazer-offsets-gen live --json

ACCEPTANCE (see README.md):
  every emitted offset must have (a) an SOS `dumpobj` line and (b) an IL structural line;
  disagreement => the field is omitted and listed in the emit report.
  A table can only come from a real dump (or from an explicitly marked fixture).

EXIT CODES:
  0  the command ran to completion
  1  self-test reported failures
  2  usage error (unknown/duplicated option, missing value, two commands at once)
  3  precondition refused (no lazer running, analyzer missing, fixture without --allow-fixtures,
     version missing/mismatch, not enough free disk space)
  4  input/format error (not a minidump, malformed intermediate, no anchor hit, transcript unparsable)
  5  nothing published (no field survived the two-witness check; nothing was written)

READ-ONLY / NO-GAME GUARANTEE:
  The product never runs this tool. This tool never attaches a debugger, never injects, never
  writes into the game and never modifies game memory. `collect` uses the game's own .NET
  diagnostics channel (dotnet-dump) which suspends the game's threads while the dump is written.
  `extract` / `il` / `emit` / `self-test` only read files on disk.
";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = run(&args);
    std::process::exit(code);
}

fn run(raw: &[String]) -> i32 {
    match parse(raw) {
        Ok(Cmd::Help) => {
            println!("{USAGE}");
            0
        }
        Ok(Cmd::Collect(options)) => collect(&options),
        Ok(Cmd::Extract(options)) => extract(&options),
        Ok(Cmd::Il(options)) => il(&options),
        Ok(Cmd::Emit(options)) => emit_table(&options),
        Ok(Cmd::Live(options)) => live::run_live(&options),
        Ok(Cmd::SelfTest(options)) => selftest::run(&options),
        Err(message) => {
            eprintln!("usage error: {message}");
            eprintln!();
            eprintln!("{USAGE}");
            2
        }
    }
}

// ------------------------------------------------------------------ 参数 ----

#[derive(Default)]
pub struct Options {
    pub values: BTreeMap<String, Vec<String>>,
    pub flags: BTreeSet<String>,
}

impl Options {
    pub fn value(&self, name: &str) -> Option<&str> {
        self.values
            .get(name)
            .and_then(|list| list.last())
            .map(|s| s.as_str())
    }

    pub fn values(&self, name: &str) -> Vec<&str> {
        self.values
            .get(name)
            .map(|list| list.iter().map(|s| s.as_str()).collect())
            .unwrap_or_default()
    }

    pub fn flag(&self, name: &str) -> bool {
        self.flags.contains(name)
    }

    pub fn required(&self, name: &str) -> Result<&str, String> {
        self.value(name)
            .ok_or_else(|| format!("missing required option {name}"))
    }
}

enum Cmd {
    Collect(Options),
    Extract(Options),
    Il(Options),
    Emit(Options),
    Live(Options),
    SelfTest(Options),
    Help,
}

const COMMANDS: &[&str] = &["collect", "extract", "il", "emit", "self-test", "live"];

/// 每个命令认得的开关（未知开关一律报错：拼错一个字母不该静默变成"没给"）。
fn known_options(command: &str) -> &'static [&'static str] {
    match command {
        "collect" => &["--out", "--pid", "--analyzer", "--dump-type"],
        "extract" => &[
            "--dump",
            "--out",
            "--analyzer",
            "--anchor",
            "--anchor-delta",
            "--max-anchor-hits",
            "--expect-vtable",
            "--table",
            "--lazer-version",
            "--runtime-version",
        ],
        "il" => &["--out", "--lazer-dir", "--sos", "--assembly", "--diff"],
        "emit" => &["--sos", "--il", "--out", "--deploy", "--report-dir"],
        "live" => &["--out", "--lazer-dir", "--pid"],
        "self-test" => &["--fixtures", "--work"],
        _ => &[],
    }
}

fn known_flags(command: &str) -> &'static [&'static str] {
    match command {
        "collect" => &["--dry-run", "--force"],
        "emit" => &["--allow-fixtures", "--allow-version-mismatch"],
        "live" => &["--json", "--dry-run"],
        _ => &[],
    }
}

fn parse(raw: &[String]) -> Result<Cmd, String> {
    if raw.is_empty() {
        return Err("no command given".to_string());
    }
    if raw[0] == "-h" || raw[0] == "--help" {
        return Ok(Cmd::Help);
    }
    let command = raw[0].as_str();
    if !COMMANDS.contains(&command) {
        return Err(format!("unknown command {command:?}"));
    }
    let values = known_options(command);
    let flags = known_flags(command);
    let mut options = Options::default();
    let mut index = 1usize;
    while index < raw.len() {
        let token = raw[index].as_str();
        if token == "-h" || token == "--help" {
            return Ok(Cmd::Help);
        }
        if !token.starts_with("--") {
            return Err(format!("unexpected positional argument {token:?}"));
        }
        if flags.contains(&token) {
            if options.flags.contains(token) {
                return Err(format!("option {token} given twice"));
            }
            options.flags.insert(token.to_string());
            index += 1;
            continue;
        }
        if !values.contains(&token) {
            return Err(format!(
                "unknown option {token:?} for `{command}` (known: {} {})",
                values.join(" "),
                flags.join(" ")
            ));
        }
        let value = raw
            .get(index + 1)
            .ok_or_else(|| format!("option {token} needs a value"))?;
        if value.starts_with("--") {
            return Err(format!("option {token} needs a value, got {value:?}"));
        }
        options
            .values
            .entry(token.to_string())
            .or_default()
            .push(value.clone());
        index += 2;
    }
    Ok(match command {
        "collect" => Cmd::Collect(options),
        "extract" => Cmd::Extract(options),
        "il" => Cmd::Il(options),
        "emit" => Cmd::Emit(options),
        "live" => Cmd::Live(options),
        "self-test" => Cmd::SelfTest(options),
        _ => unreachable!(),
    })
}

// -------------------------------------------------------------- 小工具 ----

/// 本地时间戳 `YYYYMMDD-HHMMSS`（文件名用；非 Windows 退化为 epoch 秒）。
pub fn timestamp() -> String {
    #[cfg(windows)]
    {
        let mut time: SYSTEMTIME = unsafe { std::mem::zeroed() };
        unsafe { GetLocalTime(&mut time) };
        return format!(
            "{:04}{:02}{:02}-{:02}{:02}{:02}",
            time.wYear, time.wMonth, time.wDay, time.wHour, time.wMinute, time.wSecond
        );
    }
    #[allow(unreachable_code)]
    {
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        format!("t{seconds}")
    }
}

pub fn ensure_dir(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|e| format!("create {}: {e}", path.display()))
}

fn default_out_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("MMA_LAZER_GEN_OUT") {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    let temp = std::env::var("TEMP")
        .or_else(|_| std::env::var("TMP"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(temp)
        .join("lazer-offsets-gen")
        .join(format!("run-{}", timestamp()))
}

/// `dotnet-dump` 可执行文件：`--analyzer` > `MMA_DOTNET_DUMP` > `%USERPROFILE%\.dotnet\tools\` > PATH。
fn resolve_analyzer(explicit: Option<&str>) -> Result<PathBuf, String> {
    if let Some(path) = explicit {
        let path = PathBuf::from(path);
        return path
            .is_file()
            .then_some(path.clone())
            .ok_or_else(|| format!("--analyzer {} does not exist", path.display()));
    }
    if let Ok(path) = std::env::var("MMA_DOTNET_DUMP") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
    }
    if let Ok(home) = std::env::var("USERPROFILE") {
        let candidate = PathBuf::from(home)
            .join(".dotnet")
            .join("tools")
            .join("dotnet-dump.exe");
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    if let Some(found) = which("dotnet-dump.exe").or_else(|| which("dotnet-dump")) {
        return Ok(found);
    }
    Err("dotnet-dump not found. Install it once with:\n  \
         & 'C:\\Program Files\\dotnet\\dotnet.exe' tool install --global dotnet-dump\n\
         (measured working version: 10.0.745401; or pass --analyzer <path> / set MMA_DOTNET_DUMP)"
        .to_string())
}

fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var("PATH").ok()?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// `certutil -hashfile <path> SHA256`（**不自己实现密码学**：手写哈希一旦静默出错，
/// 受害的正是"这个构建我验证过"这句话）。输出重定向到文件，不经管道。
fn sha256_of(path: &Path, scratch_dir: &Path) -> Option<String> {
    let scratch = scratch_dir.join(format!("sha256-{}.txt", timestamp()));
    let file = fs::File::create(&scratch).ok()?;
    let _ = std::process::Command::new("certutil")
        .arg("-hashfile")
        .arg(path)
        .arg("SHA256")
        .stdout(std::process::Stdio::from(file))
        .stderr(std::process::Stdio::null())
        .status()
        .ok()?;
    let text = fs::read_to_string(&scratch).ok()?;
    let _ = fs::remove_file(&scratch);
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.len() == 64 && trimmed.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Some(trimmed.to_ascii_lowercase());
        }
    }
    None
}

/// `%APPDATA%\osu\storage.ini` 的 `FullPath`（P7 结论：lazer 存储根）。
fn storage_root() -> Option<PathBuf> {
    let appdata = std::env::var("APPDATA").ok()?;
    let ini = PathBuf::from(appdata).join("osu").join("storage.ini");
    let text = fs::read_to_string(&ini).ok()?;
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim().eq_ignore_ascii_case("FullPath") {
            let value = value.trim();
            if !value.is_empty() {
                return Some(PathBuf::from(value));
            }
        }
    }
    None
}

/// 最新的 `*.runtime.log` 首几行里的 `Running osu <lazer> on .NET <runtime>`。
///
/// 这是计划指定的版本钉法（`D:\Games\osu!lazer\logs\<id>.runtime.log:3-4`）。
fn runtime_log_versions(storage_root: &Path) -> Option<(String, String, PathBuf)> {
    let logs = storage_root.join("logs");
    let entries = fs::read_dir(&logs).ok()?;
    let mut newest: Option<(SystemTime, PathBuf)> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        if path
            .file_name()
            .map(|n| n.to_string_lossy().ends_with(".runtime.log"))
            .unwrap_or(false)
        {
            let modified = entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(UNIX_EPOCH);
            if newest.as_ref().map(|(t, _)| modified > *t).unwrap_or(true) {
                newest = Some((modified, path));
            }
        }
    }
    let (_, path) = newest?;
    let text = fs::read_to_string(&path).ok()?;
    for line in text.lines().take(40) {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("Running osu ") {
            let (lazer, rest) = rest.split_once(" on .NET ")?;
            let runtime = rest.split_whitespace().next()?.to_string();
            return Some((lazer.trim().to_string(), runtime, path));
        }
    }
    None
}

/// `osu!.runtimeconfig.json` 里的 `"version": "x.y.z"`（自包含/includedFrameworks 都是这个键）。
fn runtimeconfig_version(exe_dir: &Path) -> Option<String> {
    let path = exe_dir.join("osu!.runtimeconfig.json");
    let text = fs::read_to_string(&path).ok()?;
    let at = text.find("\"version\"")?;
    let rest = text[at + "\"version\"".len()..].trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    let value = &rest[..end];
    value
        .chars()
        .all(|c| c.is_ascii_digit() || c == '.')
        .then(|| value.to_string())
}

// ------------------------------------------------------------- 进程发现 ----

pub struct Candidate {
    pub pid: u32,
    pub path: PathBuf,
    /// `0x8664` = x64（lazer）、`0x014C` = x86（stable）；`None` = 读不到 PE 头。
    pub machine: Option<u16>,
}

/// 本机跑的 `osu!.exe`（进程名与 stable 完全相同，靠位数区分 —— 计划 A4）。
pub fn discover_osu_processes() -> Result<Vec<Candidate>, String> {
    #[cfg(windows)]
    {
        if let Ok(mode) = std::env::var("MMA_LAZER_GEN_SIMULATE") {
            // dev-only 自检钩子：让"游戏没开/多实例"这两条失败路径可以被**真的跑一遍**，
            // 而不是只写在文档里（默认关闭；产品侧不存在这个开关）。
            match mode.as_str() {
                "no-lazer" => return Ok(Vec::new()),
                "stable-only" => {
                    return Ok(vec![Candidate {
                        pid: 4242,
                        path: PathBuf::from("D:\\Games\\osu!\\osu!.exe"),
                        machine: Some(0x014C),
                    }])
                }
                "two-instances" => {
                    return Ok(vec![
                        Candidate {
                            pid: 4001,
                            path: PathBuf::from("C:\\simulated\\osulazer\\current\\osu!.exe"),
                            machine: Some(0x8664),
                        },
                        Candidate {
                            pid: 4002,
                            path: PathBuf::from("C:\\simulated\\other\\osu!.exe"),
                            machine: Some(0x8664),
                        },
                    ])
                }
                other => eprintln!("[warn] MMA_LAZER_GEN_SIMULATE={other:?} is not a known mode"),
            }
        }
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snapshot == 0 || snapshot == INVALID_HANDLE_VALUE {
            return Err("CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS) failed".to_string());
        }
        let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut candidates = Vec::new();
        let mut seen = 0usize;
        let mut ok = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
        while ok {
            seen += 1;
            let name = wide_to_string(&entry.szExeFile);
            if name.eq_ignore_ascii_case("osu!.exe") {
                let pid = entry.th32ProcessID;
                let path = process_image_path(pid).unwrap_or_default();
                let machine = minidump::pe_machine(&path);
                candidates.push(Candidate { pid, path, machine });
            }
            ok = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
        }
        unsafe { CloseHandle(snapshot) };
        if seen < 10 {
            // 与 B2 真机记录同族的现象：受限上下文里 Toolhelp32 只看得见自己的子进程。
            eprintln!(
                "[warn] Toolhelp32 reported only {seen} process(es) — this context may not see \
                 other users' processes (run the tool from a normal shell / outside the sandbox)"
            );
        }
        Ok(candidates)
    }
    #[cfg(not(windows))]
    {
        Err("process discovery is Windows-only".to_string())
    }
}

/// 从候选表里挑**唯一**的 lazer（x64 `osu!.exe`）。这条逻辑是纯函数，自测直接喂假候选。
pub fn select_lazer(candidates: &[Candidate]) -> Result<&Candidate, String> {
    let x64: Vec<&Candidate> = candidates
        .iter()
        .filter(|c| c.machine == Some(0x8664))
        .collect();
    match x64.len() {
        1 => Ok(x64[0]),
        0 => {
            if candidates.is_empty() {
                Err(no_lazer_message("no running osu!.exe was found"))
            } else {
                Err(no_lazer_message(&format!(
                    "only the 32-bit osu!.exe (osu!stable) is running (pid {})",
                    candidates
                        .iter()
                        .map(|c| c.pid.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )))
            }
        }
        _ => Err(format!(
            "multiple-instances: {} x64 osu!.exe processes are running (pid {}). Close all but \
             one — the generator never guesses which instance to dump.",
            x64.len(),
            x64.iter()
                .map(|c| c.pid.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// "lazer 没在跑"的完整提示（含**将要执行的确切命令**，好让操作员先看到要发生什么）。
fn no_lazer_message(detail: &str) -> String {
    format!(
        "lazer-not-running: {detail}.\n  Start osu!lazer and re-run `collect` — the generator's \
         first step needs the live process (its threads are suspended for ~{DUMP_SECONDS_MEASURED} s \
         while the dump is written).\n  To see the exact command without touching the game: \
         `collect --out <dir> --dry-run`.\n  It would run:\n    \
         <dotnet-dump> collect --process-id <lazer pid> --type Heap --output <dir>\\lazer-<timestamp>.dmp"
    )
}

// ----------------------------------------------------------------- 采集 ----

fn collect(options: &Options) -> i32 {
    let out_dir = options
        .value("--out")
        .map(PathBuf::from)
        .unwrap_or_else(default_out_dir);
    let analyzer = match resolve_analyzer(options.value("--analyzer")) {
        Ok(path) => path,
        Err(message) => {
            eprintln!("[error] {message}");
            return 3;
        }
    };
    if let Err(message) = ensure_dir(&out_dir) {
        eprintln!("[error] {message}");
        return 3;
    }

    // ① 找目标进程（**第一步就失败**：游戏没开时给的是"怎么开"，不是一堆堆栈）。
    let candidates = match discover_osu_processes() {
        Ok(list) => list,
        Err(message) => {
            eprintln!("[error] {message}");
            return 3;
        }
    };
    let target_pid: u32 = match options.value("--pid") {
        Some(raw) => match raw.parse() {
            Ok(pid) => pid,
            Err(_) => {
                eprintln!("[error] --pid expects a number, got {raw:?}");
                return 2;
            }
        },
        None => match select_lazer(&candidates) {
            Ok(candidate) => candidate.pid,
            Err(message) => {
                eprintln!("[error] {message}");
                return 3;
            }
        },
    };
    let target = match candidates.iter().find(|c| c.pid == target_pid) {
        Some(candidate) => candidate,
        None => {
            eprintln!(
                "[error] pid {target_pid} is not a running osu!.exe (found: {})",
                if candidates.is_empty() {
                    "none".to_string()
                } else {
                    candidates
                        .iter()
                        .map(|c| format!("{} ({})", c.pid, c.path.display()))
                        .collect::<Vec<_>>()
                        .join(", ")
                }
            );
            return 3;
        }
    };
    if target.machine != Some(0x8664) {
        eprintln!(
            "[error] pid {} is {} ({}) — lazer offset tables need the x64 build",
            target.pid,
            target.path.display(),
            target
                .machine
                .map(minidump::machine_name)
                .unwrap_or("unknown bitness")
        );
        return 3;
    }

    let dump_type = options.value("--dump-type").unwrap_or("Heap");
    if !["Heap", "Full", "Mini"].contains(&dump_type) {
        eprintln!("[error] --dump-type must be Heap, Full or Mini");
        return 2;
    }
    let dump_path = out_dir.join(format!("lazer-{}.dmp", timestamp()));

    println!("== lazer-offsets-gen collect ==");
    println!("  tool            : v{TOOL_VERSION}");
    if candidates.is_empty() {
        println!("  osu!.exe seen   : none");
    } else {
        for candidate in &candidates {
            println!(
                "  osu!.exe seen   : pid {} bitness {} ({})",
                candidate.pid,
                candidate
                    .machine
                    .map(minidump::machine_name)
                    .unwrap_or("unknown"),
                candidate.path.display()
            );
        }
    }
    println!("  analyzer        : {}", analyzer.display());
    println!("  target pid      : {}", target.pid);
    println!("  target image    : {}", target.path.display());
    println!("  bitness         : {}", minidump::machine_name(0x8664));
    println!("  dump type       : {dump_type}");
    println!("  dump path       : {}", dump_path.display());
    match free_space(&out_dir) {
        Some(free) => println!(
            "  free space      : {:.1} GiB on {}",
            free as f64 / (1024.0 * 1024.0 * 1024.0),
            out_dir.display()
        ),
        None => println!("  free space      : <unknown>"),
    }
    println!(
        "  expected        : ~{:.2} GiB and ~{DUMP_SECONDS_MEASURED} s of suspended game \
         (our own measurement, 2026-09-27; the suspension is the dominant part)",
        DUMP_BYTES_MEASURED as f64 / (1024.0 * 1024.0 * 1024.0)
    );
    if let Some(free) = free_space(&out_dir) {
        if free < DUMP_FREE_SPACE_MIN && !options.flag("--force") {
            eprintln!(
                "[error] only {:.1} GiB free on the dump volume; the dump needs ~{:.2} GiB plus \
                 margin. Free space or pass --force.",
                free as f64 / (1024.0 * 1024.0 * 1024.0),
                DUMP_BYTES_MEASURED as f64 / (1024.0 * 1024.0 * 1024.0)
            );
            return 3;
        }
    }

    println!(
        "  command         : {} collect --process-id {} --type {} --output {}",
        analyzer.display(),
        target.pid,
        dump_type,
        dump_path.display()
    );
    if options.flag("--dry-run") {
        println!("  dry-run         : nothing was executed (the game was not touched)");
        return 0;
    }

    let log_path = out_dir.join(format!("collect-{}.log", timestamp()));
    let log = match fs::File::create(&log_path) {
        Ok(file) => file,
        Err(error) => {
            eprintln!("[error] create {}: {error}", log_path.display());
            return 3;
        }
    };
    let started = Instant::now();
    let status = std::process::Command::new(&analyzer)
        .arg("collect")
        .arg("--process-id")
        .arg(target.pid.to_string())
        .arg("--type")
        .arg(dump_type)
        .arg("--output")
        .arg(&dump_path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::from(
            log.try_clone().expect("clone log handle"),
        ))
        .stderr(std::process::Stdio::from(log))
        .status();
    let elapsed = started.elapsed();
    match status {
        Ok(status) if status.success() => {}
        Ok(status) => {
            eprintln!(
                "[error] dotnet-dump collect exited with {:?} after {:.1} s (log: {})",
                status.code(),
                elapsed.as_secs_f64(),
                log_path.display()
            );
            return 4;
        }
        Err(error) => {
            eprintln!("[error] spawn {}: {error}", analyzer.display());
            return 3;
        }
    }
    let size = fs::metadata(&dump_path).map(|m| m.len()).unwrap_or(0);
    println!(
        "  collected       : {} bytes in {:.1} s (log: {})",
        size,
        elapsed.as_secs_f64(),
        log_path.display()
    );
    // 采完立刻自检一次：不是 minidump 的话，后面所有步骤都是白跑。
    match Dump::open(&dump_path) {
        Ok(dump) => {
            println!(
                "  dump opened     : arch={} pid={} ranges={} modules={}",
                dump.arch,
                dump.pid,
                dump.ranges.len(),
                dump.modules.len()
            );
            if dump.pid != 0 && dump.pid != target.pid {
                eprintln!(
                    "[warn] the dump says pid {} but we asked for pid {} — check the log",
                    dump.pid, target.pid
                );
            }
            println!(
                "  next            : extract --dump \"{}\" --out \"{}\"",
                dump_path.display(),
                out_dir.display()
            );
            0
        }
        Err(error) => {
            eprintln!("[error] the collected file is not a readable minidump: {error}");
            4
        }
    }
}

// --------------------------------------------------------------- 提取 ----

/// 真实路径的对象块来源：`dotnet-dump analyze` + `dumpobj`。
struct AnalyzerBlocks {
    analyzer: PathBuf,
    dump_path: PathBuf,
    out_dir: PathBuf,
    transcripts: Vec<String>,
    unparsed_rows: Vec<String>,
    notes: Vec<String>,
    tag_prefix: String,
}

impl BlockSource for AnalyzerBlocks {
    fn fetch(&mut self, addresses: &[u64]) -> BTreeMap<u64, SosObject> {
        let mut map: BTreeMap<u64, SosObject> = BTreeMap::new();
        if addresses.is_empty() {
            return map;
        }
        let commands: Vec<String> = addresses
            .iter()
            .map(|address| format!("dumpobj 0x{address:X}"))
            .collect();
        let tag = format!("{}-n{}", self.tag_prefix, addresses.len());
        // `dotnet-dump` 的 `dumpobj` **不回显地址**：块与地址只能按命令顺序对应。顺序一旦
        // 对不上（某个地址无效 ⇒ 少一个块）就退化为"一个地址一次分析器调用"，让对齐变成
        // 确定性的（慢，但只在异常时发生）。
        let run: AnalyzerRun =
            match sos::run_analyzer(&self.analyzer, &self.dump_path, &commands, &self.out_dir, &tag)
            {
                Ok(run) => run,
                Err(message) => {
                    self.notes
                        .push(format!("analyzer run `{tag}` failed: {message}"));
                    return map;
                }
            };
        if let Some(name) = run.transcript_path.file_name() {
            self.transcripts.push(name.to_string_lossy().to_string());
        }
        let parsed = sos::parse_transcript(&run.text);
        for row in &parsed.unparsed_rows {
            self.unparsed_rows.push(format!("{tag}: {row}"));
        }
        if parsed.objects.len() == addresses.len() {
            for (address, object) in addresses.iter().zip(parsed.objects.iter()) {
                map.insert(*address, object.clone());
            }
        } else {
            self.notes.push(format!(
                "layer `{tag}`: {} dumpobj command(s) produced {} object block(s) — falling back \
                 to one analyzer run per address to keep the address/block mapping deterministic",
                addresses.len(),
                parsed.objects.len()
            ));
            for (index, address) in addresses.iter().enumerate() {
                let single_tag = format!("{tag}-obj{index}");
                let single = vec![format!("dumpobj 0x{address:X}")];
                match sos::run_analyzer(
                    &self.analyzer,
                    &self.dump_path,
                    &single,
                    &self.out_dir,
                    &single_tag,
                ) {
                    Ok(run) => {
                        if let Some(name) = run.transcript_path.file_name() {
                            self.transcripts.push(name.to_string_lossy().to_string());
                        }
                        let parsed = sos::parse_transcript(&run.text);
                        if let Some(object) = parsed.objects.first() {
                            map.insert(*address, object.clone());
                        }
                    }
                    Err(message) => self.notes.push(format!(
                        "per-address analyzer run for 0x{address:X} failed: {message}"
                    )),
                }
            }
        }
        self.notes.push(format!(
            "layer `{tag}`: {} command(s), analyzer exit {}, {:.1} s",
            commands.len(),
            run.exit_code,
            run.elapsed_ms as f64 / 1000.0
        ));
        map
    }

    fn take_notes(&mut self) -> Vec<String> {
        std::mem::take(&mut self.notes)
    }
}

impl AnalyzerBlocks {
    /// 跑任意一组 SOS 命令（`dumpmt` / `dumpmodule` / `dumparray`）——这些打印**不回显地址**，
    /// 所以真实路径一条命令一次调用（配对的确定性 > 速度；每次约 0.6 s）。
    fn run_commands(&mut self, commands: &[String], tag: &str) -> Option<String> {
        match sos::run_analyzer(
            &self.analyzer,
            &self.dump_path,
            commands,
            &self.out_dir,
            tag,
        ) {
            Ok(run) => {
                if let Some(name) = run.transcript_path.file_name() {
                    self.transcripts.push(name.to_string_lossy().to_string());
                }
                self.notes.push(format!(
                    "runtime `{tag}`: {} command(s), analyzer exit {}, {:.1} s",
                    commands.len(),
                    run.exit_code,
                    run.elapsed_ms as f64 / 1000.0
                ));
                Some(run.text)
            }
            Err(message) => {
                self.notes.push(format!("runtime `{tag}` failed: {message}"));
                None
            }
        }
    }
}

/// 中间件里某个地址上某字段的**指针值**（`instance` 行；读不到 ⇒ `None`）。
fn row_pointer(intermediate: &SosIntermediate, address: u64, field: &str) -> Option<u64> {
    intermediate
        .rows_for(address, field)
        .into_iter()
        .find(|row| row.attr == "instance")
        .and_then(|row| u64::from_str_radix(row.value.trim(), 16).ok())
        .filter(|value| *value != 0)
}

/// 中间件里某个地址上某字段的**偏移**（`instance` 行；读不到 ⇒ `None`）。
fn row_offset(intermediate: &SosIntermediate, address: u64, field: &str) -> Option<i64> {
    intermediate
        .rows_for(address, field)
        .into_iter()
        .find(|row| row.attr == "instance")
        .map(|row| row.offset)
}

/// **Step 10f 的运行期结构阶段**：屏幕栈 → 栈顶元素 → `dumpmt`（RID/Module/名字）→
/// `dumpmodule`（映像基址）→ `dumparray`（元素布局），再把"搜出来的位移"写进中间件。
///
/// 每一跳都"要么给出一个被多探针互证的位移，要么什么都不发布"（fail-closed）：
/// 缺任何一环 ⇒ 该位移不进中间件 ⇒ `emit` 随后不写 `runtime` 段 ⇒ 读侧对 `state.name`
/// 按字段级降级（**绝不**猜一个位移）。
fn runtime_phase(
    dump: &mut Dump,
    source: &mut AnalyzerBlocks,
    intermediate: &mut SosIntermediate,
    resolved: &BTreeMap<String, chain::Resolved>,
) -> Vec<String> {
    let mut report: Vec<String> = Vec::new();
    let Some(screen_stack) = resolved
        .get("screen_stack")
        .filter(|step| step.status.starts_with("ok"))
    else {
        report.push(
            "runtime: the `screen_stack` chain step is not ok — no runtime section can be derived"
                .to_string(),
        );
        return report;
    };
    let Some(stack) = row_pointer(intermediate, screen_stack.address, "stack") else {
        report.push("runtime: `OsuScreenStack.stack` is unreadable — stopping the runtime phase".to_string());
        return report;
    };
    let Some(array) = row_pointer(intermediate, stack, "_array") else {
        report.push("runtime: `Stack<IScreen>._array` is unreadable — stopping the runtime phase".to_string());
        return report;
    };
    let Some(size_offset) = row_offset(intermediate, stack, "_size") else {
        report.push("runtime: `Stack<IScreen>._size` is not in the dumpobj output".to_string());
        return report;
    };
    let size = dump
        .read_at(stack + size_offset as u64, 4)
        .map(|bytes| i32::from_le_bytes(bytes[0..4].try_into().unwrap()))
        .unwrap_or(0);
    report.push(format!(
        "runtime: screen stack 0x{:X} -> Stack 0x{stack:X} -> _array 0x{array:X} (_size={size})",
        screen_stack.address
    ));

    // ① 数组布局（`dumparray` 的 `[i] <addr>` 行 vs dump 字节）。
    let array_text = source.run_commands(
        &[format!("dumparray 0x{array:X}")],
        "sos-runtime-array",
    );
    let Some(array_text) = array_text else {
        report.push("runtime: dumparray failed — no array layout".to_string());
        return report;
    };
    let Some(parsed_array) = sos::parse_dumparray(&array_text) else {
        report.push("runtime: dumparray output unparsable — no array layout".to_string());
        return report;
    };
    report.push(format!(
        "runtime: dumparray {} -> {} element(s), {} printed item(s)",
        parsed_array.name,
        parsed_array.elements,
        parsed_array.items.len()
    ));
    let layout = match runtime::derive_array_layout(
        dump,
        array,
        parsed_array.elements,
        &parsed_array.items,
    ) {
        Ok(layout) => layout,
        Err(reason) => {
            report.push(format!("runtime: array layout FAILED — {reason}"));
            return report;
        }
    };
    report.push(format!(
        "runtime: array layout length@+{:#x}, elements@+{:#x} stride {} — {}",
        layout.length_offset, layout.data_offset, layout.stride, layout.elements_witness
    ));

    // ② 被探的 MT 集合：屏幕栈里的每一枚屏幕（`_array[i]`）+ GameBase 的 MT。
    let mut objects: Vec<(u64, String)> = Vec::new();
    for index in 0..parsed_array.elements.min(parsed_array.items.len()) {
        let Some(Some(element)) = parsed_array.items.get(index).copied() else {
            continue;
        };
        objects.push((element, format!("screen[{index}]")));
    }
    let game_mt = intermediate
        .get("game_base_mt")
        .and_then(|text| u64::from_str_radix(text.trim().trim_start_matches("0x"), 16).ok());
    let Some(game_mt) = game_mt else {
        report.push("runtime: the intermediate carries no game_base_mt — stopping".to_string());
        return report;
    };
    let Some(game_base) = intermediate
        .get("game_base")
        .and_then(|text| u64::from_str_radix(text.trim().trim_start_matches("0x"), 16).ok())
    else {
        report.push("runtime: the intermediate carries no game_base — stopping".to_string());
        return report;
    };
    objects.push((game_base, "gameBase".to_string()));

    // ③ `dumpmt` 每个对象（RID + Module + 名字），逐条落盘。
    let mut token_probes: Vec<runtime::Probe> = Vec::new();
    let mut module_probes: Vec<runtime::Probe> = Vec::new();
    let mut observed: Vec<(String, u32, String, String)> = Vec::new(); // (module, rid, name, witness)
    let mut module_addresses: BTreeMap<u64, String> = BTreeMap::new();
    for (object, label) in &objects {
        let Some(mt) = dump.read_u64(*object) else {
            report.push(format!("runtime: [{label}] at 0x{object:X} is unreadable — skipped"));
            continue;
        };
        let text = source.run_commands(
            &[format!("dumpmt 0x{mt:X}")],
            &format!("sos-runtime-dumpmt-{label}"),
        );
        let Some(text) = text else {
            report.push(format!("runtime: dumpmt 0x{mt:X} failed ({label})"));
            continue;
        };
        let Some(parsed) = sos::parse_dumpmt(&text).into_iter().next() else {
            report.push(format!("runtime: dumpmt 0x{mt:X} output unparsable ({label})"));
            continue;
        };
        let (Some(rid), Some(module)) = (parsed.rid(), parsed.module_address()) else {
            report.push(format!(
                "runtime: dumpmt 0x{mt:X} ({label}) printed no mdToken/Module — skipped"
            ));
            continue;
        };
        let name = parsed.name.clone();
        let module_file = parsed.module_file();
        if name.trim().is_empty() {
            report.push(format!(
                "runtime: dumpmt 0x{mt:X} ({label}) printed an EMPTY type name — the RID→name row is \
                 not published (fail-closed)"
            ));
            continue;
        }
        report.push(format!(
            "runtime: dumpmt 0x{mt:X} ({label}) -> {module_file}#{rid:X} `{name}` Module=0x{module:X} \
             BaseSize={} mdToken={}",
            parsed.base_size, parsed.token
        ));
        token_probes.push(runtime::Probe {
            address: mt,
            expected: rid as u64,
            source: format!("dumpmt 0x{mt:X} ({label} `{name}`)"),
        });
        module_probes.push(runtime::Probe {
            address: mt,
            expected: module,
            source: format!("dumpmt 0x{mt:X} ({label} `{name}`)"),
        });
        observed.push((
            module_file.clone(),
            rid,
            name.clone(),
            format!(
                "dumpmt 0x{mt:X} mdToken {} (RID {rid:X}) Name `{name}` File {} — object 0x{object:X}",
                parsed.token, parsed.file
            ),
        ));
        module_addresses.entry(module).or_insert(module_file);
    }
    if token_probes.len() < RUNTIME_MIN_PROBES {
        report.push(format!(
            "runtime: only {} dumpmt probe(s) — the token/module offsets need >= {} (not published)",
            token_probes.len(),
            RUNTIME_MIN_PROBES
        ));
        return report;
    }

    // ④ `dumpmt` 的 mdToken 与 dump 字节的 RID 位移。
    let token_probe = RUNTIME_PROBES
        .iter()
        .find(|probe| probe.group == "eetype" && probe.name == "token")
        .expect("spec has eetype.token");
    match runtime::find_u32_shifted(dump, game_mt, token_probe, &token_probes) {
        Ok(derived) => {
            report.push(format!(
                "runtime: eetype.token @ +{:#x} (shift {}) — {} probe(s): {}",
                derived.offset, token_probe.shift, derived.probes, derived.witness
            ));
            intermediate.runtime.push(sos::SosRuntimeRow {
                group: "eetype".to_string(),
                name: "token".to_string(),
                offset: derived.offset,
                shift: token_probe.shift,
                stride: 0,
                provenance: "dumpmt+bytes".to_string(),
                witness: derived.witness,
                probes: derived.probes,
            });
        }
        Err(reason) => report.push(format!("runtime: eetype.token FAILED — {reason}")),
    }

    // ⑤ loader Module 位移（跨模块：osu!.dll 与 osu.Game.dll 都要过同一个位移）。
    let module_probe = RUNTIME_PROBES
        .iter()
        .find(|probe| probe.group == "eetype" && probe.name == "loader_module")
        .expect("spec has eetype.loader_module");
    match runtime::find_u64(dump, module_probe, &module_probes) {
        Ok(derived) => {
            report.push(format!(
                "runtime: eetype.loader_module @ +{:#x} — {} probe(s): {}",
                derived.offset, derived.probes, derived.witness
            ));
            intermediate.runtime.push(sos::SosRuntimeRow {
                group: "eetype".to_string(),
                name: "loader_module".to_string(),
                offset: derived.offset,
                shift: 0,
                stride: 0,
                provenance: "dumpmt+bytes".to_string(),
                witness: derived.witness,
                probes: derived.probes,
            });
        }
        Err(reason) => report.push(format!("runtime: eetype.loader_module FAILED — {reason}")),
    }

    // ⑥ `dumpmodule` 每个模块 → 映像基址位移。
    let image_probe = RUNTIME_PROBES
        .iter()
        .find(|probe| probe.group == "module" && probe.name == "image_base")
        .expect("spec has module.image_base");
    let mut image_probes: Vec<runtime::Probe> = Vec::new();
    for (module, module_file) in &module_addresses {
        let text = source.run_commands(
            &[format!("dumpmodule 0x{module:X}")],
            &format!("sos-runtime-dumpmodule-{}", module_file.replace('.', "_")),
        );
        let Some(text) = text else {
            report.push(format!("runtime: dumpmodule 0x{module:X} failed ({module_file})"));
            continue;
        };
        let Some(parsed) = sos::parse_dumpmodule(&text) else {
            report.push(format!(
                "runtime: dumpmodule 0x{module:X} output unparsable ({module_file})"
            ));
            continue;
        };
        let Some(base) = parsed.base_address_value() else {
            report.push(format!(
                "runtime: dumpmodule 0x{module:X} ({module_file}) printed no BaseAddress"
            ));
            continue;
        };
        report.push(format!(
            "runtime: dumpmodule 0x{module:X} -> {module_file} BaseAddress=0x{base:X} \
             MetaDataStart={} Assembly={}",
            parsed.metadata_start, parsed.assembly
        ));
        image_probes.push(runtime::Probe {
            address: *module,
            expected: base,
            source: format!("dumpmodule 0x{module:X} ({module_file})"),
        });
    }
    if image_probes.len() < RUNTIME_MIN_PROBES {
        report.push(format!(
            "runtime: only {} dumpmodule probe(s) — module.image_base needs >= {} (not published)",
            image_probes.len(),
            RUNTIME_MIN_PROBES
        ));
    } else {
        match runtime::find_u64(dump, image_probe, &image_probes) {
            Ok(derived) => {
                report.push(format!(
                    "runtime: module.image_base @ +{:#x} — {} probe(s): {}",
                    derived.offset, derived.probes, derived.witness
                ));
                intermediate.runtime.push(sos::SosRuntimeRow {
                    group: "module".to_string(),
                    name: "image_base".to_string(),
                    offset: derived.offset,
                    shift: 0,
                    stride: 0,
                    provenance: "dumpmodule+bytes".to_string(),
                    witness: derived.witness,
                    probes: derived.probes,
                });
            }
            Err(reason) => report.push(format!("runtime: module.image_base FAILED — {reason}")),
        }
    }

    // ⑦ 数组布局的两行（已在 ① 里导出）。
    intermediate.runtime.push(sos::SosRuntimeRow {
        group: "screen_array".to_string(),
        name: "length".to_string(),
        offset: layout.length_offset,
        shift: 0,
        stride: 0,
        provenance: "dumparray+bytes".to_string(),
        witness: layout.length_witness,
        probes: parsed_array.items.len(),
    });
    intermediate.runtime.push(sos::SosRuntimeRow {
        group: "screen_array".to_string(),
        name: "elements".to_string(),
        offset: layout.data_offset,
        shift: 0,
        stride: layout.stride,
        provenance: "dumparray+bytes".to_string(),
        witness: layout.elements_witness,
        probes: parsed_array
            .items
            .iter()
            .filter(|item| item.is_some())
            .count(),
    });

    // ⑧ RID→类型名（观察行；IL 侧的那一半由 `emit` 对照，两个见证缺一不可）。
    for (module, rid, name, witness) in observed {
        intermediate.typedefs.push(sos::SosTypedefRow {
            module,
            rid,
            name,
            source: "observed".to_string(),
            witness,
        });
    }
    intermediate.runtime.sort_by(|a, b| {
        (a.group.as_str(), a.name.as_str()).cmp(&(b.group.as_str(), b.name.as_str()))
    });
    intermediate
        .typedefs
        .sort_by(|a, b| (a.module.as_str(), a.rid).cmp(&(b.module.as_str(), b.rid)));
    intermediate.typedefs.dedup_by(|a, b| a.module == b.module && a.rid == b.rid);
    report
}

fn extract(options: &Options) -> i32 {
    let dump_path = match options.required("--dump") {
        Ok(value) => PathBuf::from(value),
        Err(message) => {
            eprintln!("[error] {message}");
            return 2;
        }
    };
    let out_dir = options
        .value("--out")
        .map(PathBuf::from)
        .unwrap_or_else(default_out_dir);
    if let Err(message) = ensure_dir(&out_dir) {
        eprintln!("[error] {message}");
        return 3;
    }
    let analyzer = match resolve_analyzer(options.value("--analyzer")) {
        Ok(path) => path,
        Err(message) => {
            eprintln!("[error] {message}");
            return 3;
        }
    };
    let anchor_pattern = options.value("--anchor").unwrap_or(spec::ANCHOR_PATTERN);
    let pattern_bytes = match parse_pattern(anchor_pattern) {
        Ok(bytes) if !bytes.is_empty() => bytes,
        Ok(_) => {
            eprintln!("[error] --anchor is empty");
            return 2;
        }
        Err(message) => {
            eprintln!("[error] {message}");
            return 2;
        }
    };
    let anchor_delta: i64 = match options.value("--anchor-delta") {
        Some(raw) => match raw.parse() {
            Ok(value) => value,
            Err(_) => {
                eprintln!("[error] --anchor-delta expects a number, got {raw:?}");
                return 2;
            }
        },
        None => spec::ANCHOR_SITE_DELTA,
    };
    // delta 表 = `--anchor-delta` 那一项提到最前 + `spec::SITE_DELTAS` 的其余项（逐个试，
    // 由候选验证决定谁对；探针就是这么扫的）。
    let site_deltas: Vec<i64> = {
        let mut list: Vec<i64> = Vec::new();
        list.push(anchor_delta);
        for delta in spec::SITE_DELTAS {
            if *delta != anchor_delta {
                list.push(*delta);
            }
        }
        list
    };
    let expected_vtables = match gather_expected_vtables(options) {
        Ok(value) => value,
        Err(message) => {
            eprintln!("[error] {message}");
            return 3;
        }
    };
    let max_hits: usize = match options.value("--max-anchor-hits") {
        Some(raw) => match raw.parse() {
            Ok(value) => value,
            Err(_) => {
                eprintln!("[error] --max-anchor-hits expects a number, got {raw:?}");
                return 2;
            }
        },
        None => DEFAULT_MAX_ANCHOR_HITS,
    };

    println!("== lazer-offsets-gen extract ==");
    println!("  analyzer        : {}", analyzer.display());
    let mut dump = match Dump::open(&dump_path) {
        Ok(dump) => dump,
        Err(message) => {
            eprintln!("[error] {message}");
            return 4;
        }
    };
    println!("  dump            : {}", dump_path.display());
    println!("  dump size       : {} bytes", dump.bytes);
    println!("  arch / pid      : {} / {}", dump.arch, dump.pid);
    println!(
        "  memory ranges   : {} (modules {})",
        dump.ranges.len(),
        dump.modules.len()
    );

    // ---- 版本钉法：lazer 版本（runtime log 与 dump 模块互证）与 runtime 版本。 ----
    let dump_dir = dump_dir_of(&dump);
    let log_source = storage_root().and_then(|root| runtime_log_versions(&root));
    let module_version = dump_module_version(&dump, "osu!.dll")
        .or_else(|| dump_module_version(&dump, "osu!.exe"));
    let lazer_version = match options.value("--lazer-version") {
        Some(value) => (value.to_string(), "operator (--lazer-version)".to_string()),
        None => match (&log_source, &module_version) {
            (Some((lazer, _, _)), Some(module)) if !versions_agree(lazer, module) => {
                eprintln!(
                    "[warn] runtime log says osu {lazer} but dump module osu!.dll is {module} \
                     — using the runtime log value (pass --lazer-version to override)"
                );
                (lazer.clone(), "runtime-log".to_string())
            }
            (Some((lazer, _, _)), _) => (lazer.clone(), "runtime-log".to_string()),
            (None, Some(module)) => (module.clone(), "dump-module".to_string()),
            (None, None) => {
                eprintln!(
                    "[error] cannot pin the lazer version: no *.runtime.log under the storage \
                     root and no osu!.dll file version in the dump. Pass --lazer-version."
                );
                return 3;
            }
        },
    };
    let runtime_version = match options.value("--runtime-version") {
        Some(value) => (value.to_string(), "operator (--runtime-version)".to_string()),
        None => match &log_source {
            Some((_, runtime, _)) => (runtime.clone(), "runtime-log".to_string()),
            None => match dump_dir.as_ref().and_then(|dir| runtimeconfig_version(dir)) {
                Some(runtime) => (runtime, "runtimeconfig".to_string()),
                None => {
                    eprintln!(
                        "[error] cannot pin the runtime version: no *.runtime.log and no \
                         osu!.runtimeconfig.json next to the dumped exe. Pass --runtime-version."
                    );
                    return 3;
                }
            },
        },
    };
    println!("  lazer version   : {} ({})", lazer_version.0, lazer_version.1);
    println!(
        "  runtime version : {} ({})",
        runtime_version.0, runtime_version.1
    );
    if let Some((_, _, path)) = &log_source {
        println!("  runtime log     : {}", path.display());
    }
    if dump.arch == "unknown" {
        eprintln!("[error] the dump does not say which architecture it is (SystemInfoStream)");
        return 4;
    }
    let arch = dump.arch.clone();
    // 这个 dump 到底是不是 lazer 的：模块表里有没有 osu!.exe / osu!.dll。
    // 不是的话仍然可以跑（锚点扫描会给出结论），但版本钉法只能靠 runtime log，必须说出来。
    let dump_looks_like_lazer = dump
        .modules
        .iter()
        .any(|m| m.file_name().eq_ignore_ascii_case("osu!.dll"))
        || dump
            .modules
            .iter()
            .any(|m| m.file_name().eq_ignore_ascii_case("osu!.exe"));
    if !dump_looks_like_lazer {
        eprintln!(
            "[warn] the dump has no osu!.exe/osu!.dll module — this does not look like an \
             osu!lazer dump (lazer version pinned from {})",
            lazer_version.1
        );
    }

    // ---- 锚点扫描（纯字节匹配；地址完全来自 dump 自己的内存范围表）。 ----
    let started = Instant::now();
    let scan = match dump.scan(&pattern_bytes, max_hits, SCAN_CHUNK) {
        Ok(scan) => scan,
        Err(message) => {
            eprintln!("[error] {message}");
            return 4;
        }
    };
    println!(
        "  anchor scan     : {} hit(s) in {} bytes / {} ms{}",
        scan.hits.len(),
        scan.scanned,
        started.elapsed().as_millis(),
        if scan.capped { " (capped)" } else { "" }
    );
    if scan.hits.is_empty() {
        eprintln!(
            "[error] the anchor pattern was not found in the dump ({}, {} bytes scanned).\n  \
             The anchor is build-specific: on a lazer update verify it again (README, \
             `spec.rs::ANCHOR_PATTERN`) or pass --anchor with a fresh pattern.",
            anchor_pattern, scan.scanned
        );
        return 4;
    }
    println!(
        "  anchor          : {} hit(s) : {}",
        scan.hits.len(),
        scan.hits
            .iter()
            .take(8)
            .map(|hit| format!("{hit:#x}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!(
        "  site deltas     : {} (first = --anchor-delta/spec::ANCHOR_SITE_DELTA)",
        site_deltas
            .iter()
            .map(|d| format!("{d:#x}"))
            .collect::<Vec<_>>()
            .join(" ")
    );
    println!("  resolve hops    : {}", resolve_hops_text());
    match &expected_vtables {
        Some(vtables) => println!(
            "  expected vtable : {}",
            vtables
                .iter()
                .map(|v| format!("{v:#x}"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        None => println!(
            "  expected vtable : <none supplied> -> the `[gameBase] == table vtable` check is \
             recorded as not applicable (`--expect-vtable` / `--table` / $MMA_LAZER_OFFSETS)"
        ),
    }

    // ---- 相位 0：GameBase 解析（anchor → site → … → gameBase），逐个候选 `dumpobj` 验证 ----
    // 这一步是 P4 探针算法的移植（`chain::resolve_game_base`，与自测同一条代码）：
    // **绝不**把 site 当成 gameBase，也绝不接受没通过类型/vtable 验证的候选。
    let mut attempts: Vec<(u64, chain::ResolveAttempt)> = Vec::new();
    let mut candidates: Vec<u64> = Vec::new();
    for hit in &scan.hits {
        let resolution = chain::resolve_game_base(&mut dump, *hit, &site_deltas);
        println!(
            "  anchor {hit:#x}: {} attempt(s), {} candidate(s)",
            resolution.attempts.len(),
            resolution.candidates.len()
        );
        for attempt in &resolution.attempts {
            println!(
                "    anchor={hit:#x} delta={:#x} {} -> {}",
                attempt.delta,
                attempt.path(),
                attempt.verdict
            );
        }
        for candidate in resolution.candidates {
            if !candidates.contains(&candidate) {
                candidates.push(candidate);
            }
        }
        attempts.extend(
            resolution
                .attempts
                .into_iter()
                .map(|attempt| (*hit, attempt)),
        );
    }
    println!(
        "  resolution      : {} candidate address(es) to dumpobj: {}",
        candidates.len(),
        candidates
            .iter()
            .map(|c| format!("{c:#x}"))
            .collect::<Vec<_>>()
            .join(", ")
    );

    let mut source = AnalyzerBlocks {
        analyzer: analyzer.clone(),
        dump_path: dump_path.clone(),
        out_dir: out_dir.clone(),
        transcripts: Vec::new(),
        unparsed_rows: Vec::new(),
        notes: Vec::new(),
        tag_prefix: "sos-L0-game-candidates".to_string(),
    };
    let layer_blocks = source.fetch(&candidates);
    let mut game: Option<(u64, SosObject, i64, u64, u64)> = None;
    for candidate in &candidates {
        let producer = attempts
            .iter()
            .find(|(_, attempt)| attempt.game_base == Some(*candidate));
        let producer_text = producer
            .map(|(hit, attempt)| {
                format!(
                    "delta={:#x} site={:#x} anchor={hit:#x}",
                    attempt.delta,
                    attempt.site.unwrap_or(0)
                )
            })
            .unwrap_or_else(|| "<no producer>".to_string());
        match layer_blocks.get(candidate) {
            None => println!(
                "  candidate {candidate:#x}: <no dumpobj block> ({producer_text}; raw transcript in \
                 the out dir)"
            ),
            Some(object) => {
                let qword = dump.read_u64(*candidate);
                match chain::candidate_verdict(object, qword, expected_vtables.as_deref().unwrap_or(&[])) {
                    Ok(verdict) => {
                        println!(
                            "  candidate {candidate:#x}: Name={} MethodTable={} ({producer_text}) -> \
                             ACCEPTED ({verdict})",
                            object.name, object.method_table
                        );
                        if game.is_none() {
                            game = Some((
                                *candidate,
                                object.clone(),
                                producer.map(|(_, attempt)| attempt.delta).unwrap_or(0),
                                producer
                                    .and_then(|(_, attempt)| attempt.site)
                                    .unwrap_or(0),
                                producer.map(|(hit, _)| *hit).unwrap_or(0),
                            ));
                        }
                    }
                    Err(reason) => println!(
                        "  candidate {candidate:#x}: Name={} MethodTable={} ({producer_text}) -> \
                         REJECTED ({reason})",
                        object.name, object.method_table
                    ),
                }
            }
        }
    }
    let Some((game_address, game_object, resolved_delta, resolved_site, resolved_anchor)) = game else {
        eprintln!(
            "[error] none of the {} GameBase candidate(s) passed the checks (type must contain \
             {:?}{}). Every (anchor, delta) attempt and every candidate verdict is printed above; \
             the raw `dumpobj` transcripts are in {}.",
            candidates.len(),
            spec::GAME_BASE_CONTAINS,
            match &expected_vtables {
                Some(_) => ", and `[gameBase]` must equal the supplied table vtable",
                None => " ; no table vtable was supplied",
            },
            out_dir.display()
        );
        return 4;
    };
    println!(
        "  game base       : {game_address:#x} ({}) via anchor={resolved_anchor:#x} \
         delta={resolved_delta:#x} site={resolved_site:#x}",
        game_object.name
    );
    println!("  game base MT    : {}", game_object.method_table);

    // ---- 逐层解引用（`chain.rs`，与自测走同一条代码）。 ----
    let (resolved, blocks) = chain::walk(&mut source, game_address, &game_object);
    let mut notes = source.take_notes();
    if !dump_looks_like_lazer {
        notes.push(
            "the dump has no osu!.exe/osu!.dll module: this does not look like an osu!lazer dump"
                .to_string(),
        );
    }
    notes.push(chain::offset_base_note(
        &mut dump,
        game_address,
        &game_object.method_table,
    ));
    for (index, row) in source.unparsed_rows.iter().enumerate().take(50) {
        notes.push(format!("unparsed SOS row #{index}: {row}"));
    }

    let mut intermediate = chain::build_intermediate(&mut dump, &blocks, &resolved, notes);
    intermediate
        .provenance
        .insert("provenance".to_string(), "dump".to_string());
    intermediate
        .provenance
        .insert("fixture".to_string(), "false".to_string());
    intermediate
        .provenance
        .insert("dump".to_string(), dump_path.display().to_string());
    intermediate
        .provenance
        .insert("dump_bytes".to_string(), dump.bytes.to_string());
    intermediate
        .provenance
        .insert("dump_pid".to_string(), dump.pid.to_string());
    intermediate
        .provenance
        .insert("dump_arch".to_string(), dump.arch.clone());
    intermediate
        .provenance
        .insert("arch".to_string(), arch.clone());
    intermediate
        .provenance
        .insert("lazer_version".to_string(), lazer_version.0.clone());
    intermediate
        .provenance
        .insert("lazer_version_source".to_string(), lazer_version.1.clone());
    intermediate
        .provenance
        .insert("runtime_version".to_string(), runtime_version.0.clone());
    intermediate
        .provenance
        .insert("runtime_version_source".to_string(), runtime_version.1.clone());
    if let Some((_, _, path)) = &log_source {
        intermediate
            .provenance
            .insert("runtime_log".to_string(), path.display().to_string());
    }
    for module in ["osu!.exe", "osu.Game.dll", "osu.Framework.dll", "osu!.dll"] {
        if let Some(version) = dump_module_version(&dump, module) {
            intermediate
                .provenance
                .insert(format!("dump_module_version:{module}"), version);
        }
    }
    let exe_module = dump
        .modules
        .iter()
        .find(|m| m.file_name().eq_ignore_ascii_case("osu!.exe"))
        .or_else(|| {
            dump.modules
                .iter()
                .find(|m| m.file_name().eq_ignore_ascii_case("osu!.dll"))
        })
        .cloned();
    if let Some(module) = &exe_module {
        intermediate
            .provenance
            .insert("lazer_exe".to_string(), module.path.clone());
        let lazer_dir = Path::new(&module.path)
            .parent()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        if Path::new(&lazer_dir).is_dir() {
            intermediate
                .provenance
                .insert("lazer_dir".to_string(), lazer_dir);
            match sha256_of(Path::new(&module.path), &out_dir) {
                Some(hash) => {
                    intermediate
                        .provenance
                        .insert("lazer_exe_sha256".to_string(), hash);
                }
                None => {
                    intermediate
                        .provenance
                        .insert("lazer_exe_sha256".to_string(), "unavailable".to_string());
                    intermediate.notes.push(
                        "osu!.exe hash unavailable (certutil failed or the file is gone)"
                            .to_string(),
                    );
                }
            }
        } else {
            intermediate.notes.push(format!(
                "the dumped exe path {} is not on this machine (dump from elsewhere?) — `il` \
                 needs --lazer-dir in that case",
                module.path
            ));
        }
    }
    intermediate.provenance.insert(
        "analyzer".to_string(),
        format!(
            "{} (dotnet-dump {})",
            analyzer.display(),
            analyzer_version(&analyzer)
        ),
    );
    intermediate
        .provenance
        .insert("anchor_pattern".to_string(), anchor_pattern.to_string());
    intermediate
        .provenance
        .insert("anchor_hits".to_string(), scan.hits.len().to_string());
    intermediate
        .provenance
        .insert("anchor_scanned_bytes".to_string(), scan.scanned.to_string());
    intermediate
        .provenance
        .insert("anchor".to_string(), format!("0x{resolved_anchor:X}"));
    intermediate
        .provenance
        .insert("anchor_site_delta".to_string(), resolved_delta.to_string());
    intermediate
        .provenance
        .insert("resolve_site".to_string(), format!("0x{resolved_site:X}"));
    intermediate
        .provenance
        .insert("resolve_hops".to_string(), resolve_hops_text());
    intermediate
        .provenance
        .insert("resolve_attempts".to_string(), attempts.len().to_string());
    intermediate
        .provenance
        .insert("resolve_candidates".to_string(), candidates.len().to_string());
    intermediate.provenance.insert(
        "resolve_vtable_check".to_string(),
        match &expected_vtables {
            Some(vtables) => format!(
                "passed: [gameBase]=0x{:X} is one of the supplied table vtable(s) {}",
                dump.read_u64(game_address).unwrap_or(0),
                vtables
                    .iter()
                    .map(|v| format!("0x{v:X}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            None => "not applicable: no table vtable was supplied (--expect-vtable / --table / \
                     $MMA_LAZER_OFFSETS); only the dumpobj type check ran"
                .to_string(),
        },
    );
    intermediate
        .provenance
        .insert("game_base".to_string(), format!("0x{game_address:X}"));
    intermediate
        .provenance
        .insert("game_base_type".to_string(), game_object.name.clone());
    intermediate
        .provenance
        .insert("game_base_mt".to_string(), game_object.method_table.clone());
    intermediate.provenance.insert(
        "transcript".to_string(),
        source.transcripts.join("; "),
    );
    intermediate.provenance.insert(
        "unparsed_sos_rows".to_string(),
        source.unparsed_rows.len().to_string(),
    );

    // ---- Step 10f：运行期结构阶段（`state.name` 那条链的位移 + RID→类型名 的观察行）----
    println!("  runtime phase   : screen stack -> EEType -> dumpmt/dumpmodule/dumparray");
    let runtime_report = runtime_phase(&mut dump, &mut source, &mut intermediate, &resolved);
    for line in &runtime_report {
        println!("    {line}");
    }
    let runtime_rows = intermediate.runtime.len();
    let observed_typedefs = intermediate.typedefs.len();
    for line in &runtime_report {
        intermediate.notes.push(format!("runtime: {line}"));
    }
    intermediate.provenance.insert(
        "runtime_rows".to_string(),
        runtime_rows.to_string(),
    );
    intermediate.provenance.insert(
        "runtime_observed_typedefs".to_string(),
        observed_typedefs.to_string(),
    );

    let stamp = timestamp();
    let intermediate_path = out_dir.join(format!("sos-intermediate-{stamp}.tsv"));
    if let Err(message) = intermediate.write_tsv(&intermediate_path) {
        eprintln!("[error] {message}");
        return 4;
    }
    let report_path = out_dir.join(format!("extract-report-{stamp}.txt"));
    let report = extract_report(&intermediate, &intermediate_path, &dump_path);
    if let Err(error) = fs::write(&report_path, &report) {
        eprintln!("[error] write {}: {error}", report_path.display());
    }

    let deref_mismatch = intermediate
        .get("deref_mismatch")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);
    println!();
    println!(
        "  objects dumped  : {}",
        intermediate.get("dump_objects").unwrap_or("0")
    );
    println!("  field rows      : {}", intermediate.rows.len());
    println!(
        "  deref witness   : {} checked / {} ok / {} mismatch",
        intermediate.get("deref_checked").unwrap_or("0"),
        intermediate.get("deref_ok").unwrap_or("0"),
        deref_mismatch
    );
    if !source.unparsed_rows.is_empty() {
        println!(
            "  unparsed rows   : {} (kept in the intermediate notes)",
            source.unparsed_rows.len()
        );
    }
    let failed: Vec<&sos::SosObjectRecord> = intermediate
        .objects
        .iter()
        .filter(|o| !o.status.starts_with("ok"))
        .collect();
    if !failed.is_empty() {
        println!("  chain steps not ok: {}", failed.len());
        for item in &failed {
            println!("    - {}: {}", item.label, item.status);
        }
    }
    println!("  intermediate    : {}", intermediate_path.display());
    println!("  report          : {}", report_path.display());
    println!("  transcripts     : {}", source.transcripts.join(", "));
    println!(
        "  next            : il --sos \"{}\" --out \"{}\"",
        intermediate_path.display(),
        out_dir.display()
    );
    if deref_mismatch > 0 {
        eprintln!(
            "[warn] {deref_mismatch} field row(s) disagree with the dump bytes — `emit` will \
             omit those fields"
        );
    }
    0
}

fn extract_report(
    intermediate: &SosIntermediate,
    intermediate_path: &Path,
    dump_path: &Path,
) -> String {
    let mut out = String::new();
    out.push_str("=== lazer-offsets-gen extract report ===\n");
    out.push_str(&format!("dump        : {}\n", dump_path.display()));
    out.push_str(&format!("intermediate: {}\n", intermediate_path.display()));
    for (key, value) in &intermediate.provenance {
        out.push_str(&format!("{key} = {value}\n"));
    }
    out.push_str("\n--- objects ---\n");
    for object in &intermediate.objects {
        out.push_str(&format!(
            "  {:<20} 0x{:>12X} {:<60} {}\n",
            object.label, object.address, object.object_type, object.status
        ));
    }
    if !intermediate.runtime.is_empty() {
        out.push_str("\n--- runtime structure (Step 10f) ---\n");
        for row in &intermediate.runtime {
            out.push_str(&format!(
                "  {}.{}\t= {}\tshift {}\tstride {}\t[{} probe(s)]\t{}\t({})\n",
                row.group, row.name, row.offset, row.shift, row.stride, row.probes, row.witness,
                row.provenance
            ));
        }
    }
    if !intermediate.typedefs.is_empty() {
        out.push_str("\n--- observed TypeDef RIDs (Step 10f; IL side corroborates in `emit`) ---\n");
        for row in &intermediate.typedefs {
            out.push_str(&format!(
                "  {}#{:X}\t{}\t({})\t{}\n",
                row.module, row.rid, row.name, row.source, row.witness
            ));
        }
    }
    out.push_str("\n--- notes ---\n");
    for note in &intermediate.notes {
        out.push_str(&format!("  {note}\n"));
    }
    out
}

/// `spec::GAME_BASE_HOPS` 的一行文字（打印与 provenance 共用）。
fn resolve_hops_text() -> String {
    spec::GAME_BASE_HOPS
        .iter()
        .map(|hop| format!("{}(+{:#x} {})", hop.label, hop.offset, hop.field))
        .collect::<Vec<_>>()
        .join(" -> ")
}

/// 解析 `--expect-vtable` 的十六进制值。
fn parse_hex_value(option: &str, raw: &str) -> Result<u64, String> {
    let text = raw.trim().trim_start_matches("0x").trim_start_matches("0X");
    u64::from_str_radix(text, 16)
        .map_err(|_| format!("{option} expects a hex value (e.g. 0x7ff9e7ef8970), got {raw:?}"))
}

/// 一张已生成的表里的 `game_base_vtable`（十进制，与 `emit` 的输出同形；`null` = 没提取到）。
///
/// 只做"找键 + 取一个数"，不引入 JSON 依赖：这张表是我们自己生成的固定形状。
fn table_vtable(path: &Path) -> Result<Option<u64>, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let key = "\"game_base_vtable\"";
    let at = text
        .find(key)
        .ok_or_else(|| format!("{}: no `{key}` key (not a lazer-offsets table?)", path.display()))?;
    let rest = &text[at + key.len()..];
    let rest = rest
        .trim_start()
        .strip_prefix(':')
        .ok_or_else(|| format!("{}: malformed `{key}` entry", path.display()))?
        .trim_start();
    let token: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == 'x' || *c == 'X')
        .collect();
    if token.is_empty() || token.eq_ignore_ascii_case("null") {
        return Ok(None);
    }
    let value = if let Some(hex) = token.strip_prefix("0x").or_else(|| token.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16)
    } else {
        token.parse::<u64>()
    }
    .map_err(|_| format!("{}: `{key}` value {token:?} is not a number", path.display()))?;
    Ok(Some(value))
}

/// 期望的 `[gameBase]`（= 表的 `game_base_vtable`）从哪来：`--expect-vtable` > `--table <json>`
/// > `$MMA_LAZER_OFFSETS`（读取侧同一条环境变量）。都没有 ⇒ `None`（第 2 条判据如实记"不适用"）。
fn gather_expected_vtables(options: &Options) -> Result<Option<Vec<u64>>, String> {
    let mut vtables: Vec<u64> = Vec::new();
    for raw in options.values("--expect-vtable") {
        vtables.push(parse_hex_value("--expect-vtable", raw)?);
    }
    for raw in options.values("--table") {
        let path = PathBuf::from(raw);
        match table_vtable(&path)? {
            Some(value) => {
                println!("  table vtable    : {value:#x} (from --table {raw})");
                vtables.push(value);
            }
            None => eprintln!(
                "[warn] {raw}: `game_base_vtable` is null/missing — it cannot serve as a vtable \
                 expectation"
            ),
        }
    }
    if vtables.is_empty() {
        if let Ok(raw) = std::env::var("MMA_LAZER_OFFSETS") {
            let path = PathBuf::from(&raw);
            if !raw.is_empty() && path.is_file() {
                match table_vtable(&path) {
                    Ok(Some(value)) => {
                        println!("  table vtable    : {value:#x} (from $MMA_LAZER_OFFSETS {raw})");
                        vtables.push(value);
                    }
                    Ok(None) => eprintln!(
                        "[warn] $MMA_LAZER_OFFSETS={raw} has a null/missing `game_base_vtable`"
                    ),
                    Err(message) => eprintln!("[warn] {message}"),
                }
            }
        }
    }
    vtables.dedup();
    Ok(if vtables.is_empty() { None } else { Some(vtables) })
}

fn analyzer_version(analyzer: &Path) -> String {
    let output = std::process::Command::new(analyzer)
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .output();
    match output {
        Ok(output) => String::from_utf8_lossy(&output.stdout)
            .lines()
            .next()
            .unwrap_or("")
            .trim()
            .to_string(),
        Err(_) => "unknown".to_string(),
    }
}

fn versions_agree(left: &str, right: &str) -> bool {
    let parse = |text: &str| -> Vec<u32> {
        text.split('.')
            .map(|part| part.trim().parse::<u32>().unwrap_or(0))
            .collect()
    };
    let left = parse(left);
    let right = parse(right);
    for index in 0..left.len().max(right.len()) {
        if left.get(index).copied().unwrap_or(0) != right.get(index).copied().unwrap_or(0) {
            return false;
        }
    }
    true
}

fn dump_module_version(dump: &Dump, module: &str) -> Option<String> {
    dump.modules
        .iter()
        .find(|m| m.file_name().eq_ignore_ascii_case(module))
        .and_then(|m| m.file_version.clone())
}

/// dump 里 exe 模块所在目录（只有当它在本机存在时才算数）。
fn dump_dir_of(dump: &Dump) -> Option<PathBuf> {
    let module = dump
        .modules
        .iter()
        .find(|m| m.file_name().eq_ignore_ascii_case("osu!.exe"))
        .or_else(|| {
            dump.modules
                .iter()
                .find(|m| m.file_name().eq_ignore_ascii_case("osu!.dll"))
        })?;
    let dir = Path::new(&module.path).parent()?.to_path_buf();
    dir.is_dir().then_some(dir)
}

pub fn parse_pattern(text: &str) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    for token in text.split_whitespace() {
        let token = token.trim_start_matches("0x").trim_start_matches("0X");
        if token.len() > 2 {
            return Err(format!(
                "anchor pattern token {token:?} is more than one byte (write it as hex bytes, \
                 e.g. `{}`)",
                spec::ANCHOR_PATTERN
            ));
        }
        let value = u8::from_str_radix(token, 16)
            .map_err(|_| format!("anchor pattern token {token:?} is not a hex byte"))?;
        bytes.push(value);
    }
    Ok(bytes)
}

// ------------------------------------------------------------------ IL ----

fn il(options: &Options) -> i32 {
    let out_dir = options
        .value("--out")
        .map(PathBuf::from)
        .unwrap_or_else(default_out_dir);
    if let Err(message) = ensure_dir(&out_dir) {
        eprintln!("[error] {message}");
        return 3;
    }
    let lazer_dir = match (options.value("--lazer-dir"), options.value("--sos")) {
        (Some(dir), _) => PathBuf::from(dir),
        (None, Some(sos_path)) => match SosIntermediate::read_tsv(Path::new(sos_path)) {
            Ok(intermediate) => match intermediate.get("lazer_dir") {
                Some(dir) if !dir.is_empty() => PathBuf::from(dir),
                _ => {
                    eprintln!(
                        "[error] {sos_path} has no usable `lazer_dir` (the dumped exe path is not \
                         on this machine). Pass --lazer-dir explicitly."
                    );
                    return 3;
                }
            },
            Err(message) => {
                eprintln!("[error] {message}");
                return 4;
            }
        },
        (None, None) => {
            eprintln!("[error] `il` needs --lazer-dir <dir> (or --sos <sos-intermediate.tsv>)");
            return 2;
        }
    };
    if !lazer_dir.is_dir() {
        eprintln!("[error] {} is not a directory", lazer_dir.display());
        return 3;
    }
    let assemblies: Vec<PathBuf> = options
        .values("--assembly")
        .into_iter()
        .map(PathBuf::from)
        .collect();

    println!("== lazer-offsets-gen il ==");
    println!("  lazer dir       : {}", lazer_dir.display());
    let (inventory, report) = match ilmeta::collect(&lazer_dir, &assemblies) {
        Ok(result) => result,
        Err(message) => {
            eprintln!("[error] {message}");
            return 4;
        }
    };
    for line in &report {
        println!("{line}");
    }
    println!(
        "  managed         : {} assemblies (native skipped {}, failures {})",
        inventory.get("assemblies_managed").unwrap_or("0"),
        inventory.get("assemblies_native").unwrap_or("0"),
        inventory.get("assemblies_failed").unwrap_or("0")
    );
    println!(
        "  field rows      : {}",
        inventory.get("field_rows").unwrap_or("0")
    );
    if inventory.get("assemblies_managed").unwrap_or("0") == "0" {
        eprintln!(
            "[error] no managed assembly found under {}",
            lazer_dir.display()
        );
        return 4;
    }
    let stamp = timestamp();
    let inventory_path = out_dir.join(format!("il-inventory-{stamp}.tsv"));
    if let Err(message) = inventory.write_tsv(&inventory_path) {
        eprintln!("[error] {message}");
        return 4;
    }
    println!("  inventory       : {}", inventory_path.display());
    if let Some(old_path) = options.value("--diff") {
        match ilmeta::IlInventory::read_tsv(Path::new(old_path)) {
            Ok(old) => {
                let diff_path = out_dir.join(format!("il-diff-{stamp}.txt"));
                let text = ilmeta::diff(&old, &inventory);
                if let Err(error) = fs::write(&diff_path, &text) {
                    eprintln!("[error] write {}: {error}", diff_path.display());
                    return 4;
                }
                print!("{text}");
                println!("  diff            : {}", diff_path.display());
            }
            Err(message) => {
                eprintln!("[error] {message}");
                return 4;
            }
        }
    }
    println!(
        "  next            : emit --sos <sos-intermediate.tsv> --il \"{}\" --deploy <dir>",
        inventory_path.display()
    );
    0
}

// ---------------------------------------------------------------- 出表 ----

fn emit_table(options: &Options) -> i32 {
    let sos_path = match options.required("--sos") {
        Ok(value) => PathBuf::from(value),
        Err(message) => {
            eprintln!("[error] {message}");
            return 2;
        }
    };
    let il_path = match options.required("--il") {
        Ok(value) => PathBuf::from(value),
        Err(message) => {
            eprintln!("[error] {message}");
            return 2;
        }
    };
    let sos = match SosIntermediate::read_tsv(&sos_path) {
        Ok(value) => value,
        Err(message) => {
            eprintln!("[error] {message}");
            return 4;
        }
    };
    let il = match ilmeta::IlInventory::read_tsv(&il_path) {
        Ok(value) => value,
        Err(message) => {
            eprintln!("[error] {message}");
            return 4;
        }
    };
    let outcome: EmitOutcome = match emit::emit(
        &sos,
        &il,
        options.flag("--allow-fixtures"),
        options.flag("--allow-version-mismatch"),
    ) {
        Ok(outcome) => outcome,
        Err(Refusal(message)) => {
            eprintln!("[refused] {message}");
            return 3;
        }
    };

    // 表落点：`--out` > `$MMA_LAZER_OFFSETS` > ./lazer-offsets/<ver>__<rt>__<arch>.json
    let table_path = match options.value("--out") {
        Some(path) => PathBuf::from(path),
        None => match std::env::var("MMA_LAZER_OFFSETS") {
            Ok(path) if !path.is_empty() => PathBuf::from(path),
            _ => PathBuf::from("lazer-offsets").join(outcome.file_name()),
        },
    };
    if let Some(parent) = table_path.parent() {
        if !parent.as_os_str().is_empty() {
            if let Err(message) = ensure_dir(parent) {
                eprintln!("[error] {message}");
                return 3;
            }
        }
    }
    if let Err(error) = fs::write(&table_path, outcome.to_json()) {
        eprintln!("[error] write {}: {error}", table_path.display());
        return 4;
    }
    let report_dir = options
        .value("--report-dir")
        .map(PathBuf::from)
        .or_else(|| table_path.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    if let Err(message) = ensure_dir(&report_dir) {
        eprintln!("[error] {message}");
        return 3;
    }
    let report_path = report_dir.join(format!("emit-report-{}.txt", timestamp()));
    let report = outcome.report(
        &sos_path.display().to_string(),
        &il_path.display().to_string(),
    );
    if let Err(error) = fs::write(&report_path, &report) {
        eprintln!("[error] write {}: {error}", report_path.display());
    }
    print!("{report}");
    println!("table           : {}", table_path.display());
    println!("report          : {}", report_path.display());

    if let Some(deploy_dir) = options.value("--deploy") {
        let target_dir = PathBuf::from(deploy_dir).join("lazer-offsets");
        if let Err(message) = ensure_dir(&target_dir) {
            eprintln!("[error] {message}");
            return 3;
        }
        let target = target_dir.join(outcome.file_name());
        if let Err(error) = fs::copy(&table_path, &target) {
            eprintln!("[error] copy to {}: {error}", target.display());
            return 3;
        }
        println!("deployed        : {}", target.display());
    }
    println!(
        "reader ladder   : $MMA_LAZER_OFFSETS (explicit file) -> <shell exe dir>\\lazer-offsets\\{} \
         -> nearest table in that folder (only with an L1 structural proof) -> reason \
         `lazer-offsets-missing:<ver>`",
        outcome.file_name()
    );
    0
}

// ------------------------------------------------------------- Win32 ----

#[cfg(windows)]
pub(crate) type Handle = isize;
#[cfg(windows)]
pub(crate) const INVALID_HANDLE_VALUE: Handle = -1isize;
#[cfg(windows)]
pub(crate) const TH32CS_SNAPPROCESS: u32 = 0x2;
#[cfg(windows)]
pub(crate) const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
#[cfg(windows)]
pub(crate) const PROCESS_VM_READ: u32 = 0x0010;
#[cfg(windows)]
pub(crate) const PROCESS_QUERY_INFORMATION: u32 = 0x0400;

#[cfg(windows)]
pub(crate) const MEM_COMMIT: u32 = 0x1000;
#[cfg(windows)]
pub(crate) const PAGE_READWRITE: u32 = 0x04;
#[cfg(windows)]
pub(crate) const PAGE_EXECUTE_READWRITE: u32 = 0x40;
#[cfg(windows)]
pub(crate) const PAGE_GUARD: u32 = 0x100;
#[cfg(windows)]
pub(crate) const PAGE_NOACCESS: u32 = 0x01;

/// `PROCESSENTRY32W`（MSVC x64 布局，align 8，`size_of == 568`；与
/// `desktop/src/malody4/anchor.rs` 同一份声明口径）。
#[cfg(windows)]
#[repr(C)]
#[allow(non_snake_case)]
pub(crate) struct PROCESSENTRY32W {
    pub(crate) dwSize: u32,
    pub(crate) cntUsage: u32,
    pub(crate) th32ProcessID: u32,
    pub(crate) th32DefaultHeapID: usize,
    pub(crate) th32ModuleID: u32,
    pub(crate) cntThreads: u32,
    pub(crate) th32ParentProcessID: u32,
    pub(crate) pcPriClassBase: i32,
    pub(crate) dwFlags: u32,
    pub(crate) szExeFile: [u16; 260],
}

#[cfg(windows)]
#[repr(C)]
#[allow(non_snake_case)]
pub(crate) struct MEMORY_BASIC_INFORMATION {
    pub(crate) BaseAddress: *mut std::ffi::c_void,
    pub(crate) AllocationBase: *mut std::ffi::c_void,
    pub(crate) AllocationProtect: u32,
    pub(crate) PartitionId: u16,
    pub(crate) RegionSize: usize,
    pub(crate) State: u32,
    pub(crate) Protect: u32,
    pub(crate) Type: u32,
}

/// 布局断言写成编译期常量：布局一旦不对就直接编译失败（`dwSize` 传错会让快照枚举直接失败）。
#[cfg(windows)]
const _PROCESSENTRY32W_LAYOUT_IS_X64: () = assert!(
    std::mem::size_of::<PROCESSENTRY32W>() == 568,
    "PROCESSENTRY32W must use the x64 layout (568 bytes)"
);

/// `SYSTEMTIME`（`GetLocalTime` 用）。
#[cfg(windows)]
#[repr(C)]
#[allow(non_snake_case)]
struct SYSTEMTIME {
    wYear: u16,
    wMonth: u16,
    wDayOfWeek: u16,
    wDay: u16,
    wHour: u16,
    wMinute: u16,
    wSecond: u16,
    wMilliseconds: u16,
}

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    pub(crate) fn CreateToolhelp32Snapshot(dwFlags: u32, th32ProcessID: u32) -> Handle;
    pub(crate) fn Process32FirstW(hSnapshot: Handle, lppe: *mut PROCESSENTRY32W) -> i32;
    pub(crate) fn Process32NextW(hSnapshot: Handle, lppe: *mut PROCESSENTRY32W) -> i32;
    pub(crate) fn OpenProcess(dwDesiredAccess: u32, bInheritHandle: i32, dwProcessId: u32) -> Handle;
    pub(crate) fn CloseHandle(hObject: Handle) -> i32;
    pub(crate) fn QueryFullProcessImageNameW(
        hProcess: Handle,
        dwFlags: u32,
        lpExeName: *mut u16,
        lpdwSize: *mut u32,
    ) -> i32;
    pub(crate) fn VirtualQueryEx(
        hProcess: Handle,
        lpAddress: *const std::ffi::c_void,
        lpBuffer: *mut MEMORY_BASIC_INFORMATION,
        dwLength: usize,
    ) -> usize;
    pub(crate) fn ReadProcessMemory(
        hProcess: Handle,
        lpBaseAddress: *const std::ffi::c_void,
        lpBuffer: *mut std::ffi::c_void,
        nSize: usize,
        lpNumberOfBytesRead: *mut usize,
    ) -> i32;
    pub(crate) fn IsWow64Process(hProcess: Handle, Wow64Process: *mut i32) -> i32;
    fn GetDiskFreeSpaceExW(
        lpDirectoryName: *const u16,
        lpFreeBytesAvailableToCaller: *mut u64,
        lpTotalNumberOfBytes: *mut u64,
        lpTotalNumberOfFreeBytes: *mut u64,
    ) -> i32;
    fn GetLocalTime(lpSystemTime: *mut SYSTEMTIME);
}

#[cfg(windows)]
fn wide_to_string(field: &[u16]) -> String {
    let end = field.iter().position(|&c| c == 0).unwrap_or(field.len());
    String::from_utf16_lossy(&field[..end])
}

#[cfg(windows)]
fn to_wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 进程映像路径（只读：`PROCESS_QUERY_LIMITED_INFORMATION`）。
#[cfg(windows)]
fn process_image_path(pid: u32) -> Option<PathBuf> {
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle == 0 || handle == INVALID_HANDLE_VALUE {
        return None;
    }
    let mut buffer = vec![0u16; 32768];
    let mut length = buffer.len() as u32;
    let ok = unsafe { QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut length) };
    unsafe { CloseHandle(handle) };
    (ok != 0 && length > 0)
        .then(|| PathBuf::from(String::from_utf16_lossy(&buffer[..length as usize])))
}

/// 目标卷空闲字节（拿不到 = `None`）。
#[cfg(windows)]
fn free_space(dir: &Path) -> Option<u64> {
    let wide = to_wide(&dir.to_string_lossy());
    let mut free = 0u64;
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut free,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    (ok != 0).then_some(free)
}

#[cfg(not(windows))]
fn free_space(_dir: &Path) -> Option<u64> {
    None
}

#[cfg(not(windows))]
fn process_image_path(_pid: u32) -> Option<PathBuf> {
    None
}