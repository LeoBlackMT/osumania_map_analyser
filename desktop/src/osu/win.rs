// osu! 原生读取层的 Win32 直接 FFI + RAII 目标进程。
//
// 与 `malody4/anchor.rs` 的关系（计划 §4.0 单选结论 ②：**不抽公共 win32.rs**）：
// 本文件是**孪生实现**，只共享"断言与约定"，不共享代码——`malody4/` 侧从不调用
// `VirtualQueryEx`、也没有 64 位地址空间/位数一等概念的需求，抽取会让
// `tools/malody4-anchor-check/main.rs`（用 `#[path]` 注入 `anchor.rs`）永久注入两个文件。
//
// 只读契约（与本仓库既有先例一致，且**只多不少**）：
// - 句柄权限恒为 `PROCESS_VM_READ | PROCESS_QUERY_INFORMATION`（`VirtualQueryEx`
//   文档要求后者；该对权限不含任何写/挂起/注入位）。本文件不声明
//   `WriteProcessMemory` / `VirtualAllocEx` / `CreateRemoteThread` / `SuspendThread` /
//   `NtXxx` 中的任何一个——**没有声明就没有误用面**。
// - 每个句柄都在 `OwnedHandle`/`Target` 的 `Drop` 里 `CloseHandle`（RAII，无泄漏路径）。
// - `ReadProcessMemory` 必须**整段读满**才算成功：短读（`ERROR_PARTIAL_COPY = 299`
//   的典型形态）一律当失败返回，**绝不零填充**（零填充会把"读不到"伪装成"值是 0"）。
// - **单次调用硬上限 1 MiB**（计划 §3.4「按区截断 + 单次硬上限」）：`read_exact_at`
//   一次最多读 `READ_CALL_MAX` 字节，更大的请求由 `read_exact_chunked_at` 切块下发；
//   299 的处置是"**缩小重试**"（见 `plan_shrink_sequence`），不是零填充、也不是静默短读。

#![allow(non_snake_case, non_camel_case_types)]

use crate::osu::model::Reason;

/// 目标进程名：stable 与 lazer 都用这个名字，靠 PE machine 分派（计划 §3.4）。
pub const OSU_EXE: &str = "osu!.exe";

/// `OpenProcess` 权限：读内存 + 查询（`VirtualQueryEx` / `QueryFullProcessImageNameW`）。
pub const DESIRED_ACCESS: u32 = 0x0010 | 0x0400; // PROCESS_VM_READ | PROCESS_QUERY_INFORMATION

/// PE machine 字面量（磁盘映像读出的 `IMAGE_FILE_HEADER.Machine`）。
pub const PE_MACHINE_I386: u16 = 0x014C;
pub const PE_MACHINE_AMD64: u16 = 0x8664;

// ---- 非 Windows 桩 ----
//
// 与 `malody4/anchor.rs:779-821` 同形：同名同签名的 stub，让 crate 在 Linux 上照常
// 编译（CI/交叉检查），行为恒为 "platform-unsupported" 式的空目标。

#[cfg(not(windows))]
pub struct Target;

#[cfg(not(windows))]
impl Target {
    pub fn pid(&self) -> u32 {
        0
    }
}

#[cfg(not(windows))]
pub fn select_target() -> Result<Target, Reason> {
    Err(Reason::ProcessNotFound)
}

/// C# 字符串长度硬上限（码元数）：4096 足够覆盖最长路径/文件名（MAX_PATH=260），
/// 又不足以让一个坏头把内存读放大成 MB 级（计划 §3.4：先夹取上限再读正文）。
pub const MAX_CSHARP_STRING_UNITS: u32 = 4096;

// ---- Windows ----

#[cfg(windows)]
pub use win32::*;

#[cfg(windows)]
mod win32 {
    use super::*;
    use std::path::{Path, PathBuf};

    /// ⚠️ 必须与 `malody4/anchor.rs` 的 `Handle = isize` 与结构体拼写**逐字一致**：
    /// 两个 `#[link(name = "kernel32")]` 块里的同名符号会被链接器合并，参数类型不一致时
    /// 编译器会给 "redeclared with a different signature" 警告（且类型不同是真实的 UB 面）。
    /// 孪生实现的代价就在这里——共享的是约定，不是代码。
    pub type Handle = isize;
    const INVALID_HANDLE_VALUE: Handle = -1isize;

    const TH32CS_SNAPPROCESS: u32 = 0x0000_0002;
    const TH32CS_SNAPMODULE: u32 = 0x0000_0008;
    /// 壳是 64 位、stable 是 32 位：**两个模块标志都要给**，否则看不见 32 位模块。
    const TH32CS_SNAPMODULE32: u32 = 0x0000_0010;

    const MEM_COMMIT: u32 = 0x1000;
    const PAGE_NOACCESS: u32 = 0x01;
    const PAGE_READONLY: u32 = 0x02;
    const PAGE_READWRITE: u32 = 0x04;
    const PAGE_WRITECOPY: u32 = 0x08;
    const PAGE_EXECUTE_READ: u32 = 0x20;
    const PAGE_EXECUTE_READWRITE: u32 = 0x40;
    const PAGE_EXECUTE_WRITECOPY: u32 = 0x80;
    const PAGE_GUARD: u32 = 0x100;

    /// 参考过滤器（P1 的 filter A）：`MEM_COMMIT & (PAGE_READWRITE|PAGE_EXECUTE_READWRITE)`。
    pub const FILTER_RW: u32 = PAGE_READWRITE | PAGE_EXECUTE_READWRITE;
    /// 宽过滤器（P1 的 filter B）：任何有读权限的已提交区。
    pub const FILTER_READABLE: u32 = PAGE_READONLY
        | PAGE_READWRITE
        | PAGE_WRITECOPY
        | PAGE_EXECUTE_READ
        | PAGE_EXECUTE_READWRITE
        | PAGE_EXECUTE_WRITECOPY;

    /// `MEMORY_BASIC_INFORMATION`（x64 调用方布局，`size_of == 48`）。
    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    pub struct MemoryBasicInformation {
        pub BaseAddress: *mut u8,    // @0
        pub AllocationBase: *mut u8, // @8
        pub AllocationProtect: u32,  // @16
        pub PartitionId: u16,        // @20（x64 上真名如此；x86 侧是 4 字节对齐补白）
        pub _pad0: u16,              // @22
        pub RegionSize: usize,       // @24
        pub State: u32,              // @32
        pub Protect: u32,            // @36
        pub Type: u32,               // @40
        pub _pad1: u32,              // @44 ⇒ 48
    }

    /// `PROCESSENTRY32W`（x64 调用方布局，`size_of == 568`）。
    #[repr(C)]
    #[allow(non_snake_case)]
    pub struct ProcessEntry32W {
        pub dwSize: u32,              // @0
        pub cntUsage: u32,            // @4
        pub th32ProcessID: u32,       // @8
        pub _pad0: u32,               // @12（th32DefaultHeapID 是 ULONG_PTR）
        pub th32DefaultHeapID: usize, // @16
        pub th32ModuleID: u32,        // @24
        pub cntThreads: u32,          // @28
        pub th32ParentProcessID: u32, // @32
        pub pcPriClassBase: i32,      // @36
        pub dwFlags: u32,             // @40
        pub _pad1: u32,               // @44
        pub szExeFile: [u16; 260],    // @48 ⇒ 568
    }

    /// `MODULEENTRY32W`（x64 调用方布局，`size_of == 1080`）——字段名与
    /// `malody4/anchor.rs` 的同名结构体**逐字一致**（见本模块头部的说明）。
    #[repr(C)]
    #[allow(non_snake_case)]
    pub struct ModuleEntry32W {
        pub dwSize: u32,           // @0
        pub th32ModuleID: u32,     // @4
        pub th32ProcessID: u32,    // @8
        pub GlblcntUsage: u32,     // @12
        pub ProccntUsage: u32,     // @16
        pub _pad0: u32,            // @20
        pub modBaseAddr: *mut u8,  // @24
        pub modBaseSize: u32,      // @32（SizeOfImage，不是磁盘文件大小）
        pub _pad1: u32,            // @36
        pub hModule: *mut u8,      // @40
        pub szModule: [u16; 256],  // @48
        pub szExePath: [u16; 260], // @560 ⇒ 1080
    }

    // `clashing_extern_declarations`：`malody4/anchor.rs` 早已声明同名的
    // `Process32FirstW` / `Module32FirstW` 等符号，两侧结构体名字不同（孪生实现的
    // 有意结果）。两边布局逐字节一致，且 `malody4` 的 `#[repr(C)]` 结构体是模块私有的
    // （无法 import），所以这里显式记录差异而不是"重命名到看不出区别"：
    // - 见 `MemoryBasicInformation` / `ProcessEntry32W` / `ModuleEntry32W` 的字段注释
    // - 布局由下方 `const _` 断言（48 / 568 / 1080）在编译期钉死
    // 一旦某个字段真被改坏，四个结构体里有一个先炸断言，不会静默错位。
    #[allow(clashing_extern_declarations)]
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
            lpBuffer: *mut MemoryBasicInformation,
            dwLength: usize,
        ) -> usize;
        fn CreateToolhelp32Snapshot(dwFlags: u32, th32ProcessID: u32) -> Handle;
        fn Process32FirstW(hSnapshot: Handle, lppe: *mut ProcessEntry32W) -> i32;
        fn Process32NextW(hSnapshot: Handle, lppe: *mut ProcessEntry32W) -> i32;
        fn Module32FirstW(hSnapshot: Handle, lpme: *mut ModuleEntry32W) -> i32;
        fn Module32NextW(hSnapshot: Handle, lpme: *mut ModuleEntry32W) -> i32;
        fn QueryFullProcessImageNameW(
            hProcess: Handle,
            dwFlags: u32,
            lpExeName: *mut u16,
            lpdwSize: *mut u32,
        ) -> i32;
        fn GetLastError() -> u32;
    }

    /// 进程/模块快照与内存句柄的 RAII 包装：`Drop` 关闭。
    pub struct OwnedHandle(Handle);

    impl OwnedHandle {
        fn raw(&self) -> Handle {
            self.0
        }
        fn is_valid(&self) -> bool {
            self.0 != 0 && self.0 != INVALID_HANDLE_VALUE
        }
    }

    impl Drop for OwnedHandle {
        fn drop(&mut self) {
            if self.is_valid() {
                unsafe { CloseHandle(self.0) };
            }
        }
    }

    /// 编译期布局断言：这些 `size_of` 就是 Win32 要求的 `dwLength` / `dwSize` 实参，
    /// 一旦 x64 调用方布局被改坏，这里先炸。
    const _: () = {
        assert!(std::mem::size_of::<MemoryBasicInformation>() == 48);
        assert!(std::mem::size_of::<ProcessEntry32W>() == 568);
        assert!(std::mem::size_of::<ModuleEntry32W>() == 1080);
    };

    fn wide_to_string(wide: &[u16]) -> String {
        let len = wide.iter().position(|&c| c == 0).unwrap_or(wide.len());
        String::from_utf16_lossy(&wide[..len])
    }

    /// 诊断开关：`MMA_OSU_DEBUG=1` 时把发现阶段的细节打到 stderr（默认关，避免刷屏）。
    pub fn debug_enabled() -> bool {
        matches!(std::env::var("MMA_OSU_DEBUG"), Ok(v) if v.trim() != "0" && !v.trim().is_empty())
    }

    /// 诊断用：原始 Toolhelp32 枚举结果（句柄有效性 / 首项调用返回值 / 总数 / osu! 名）。
    ///
    /// 存在的理由：`list_osu_processes` 在"快照句柄无效"与"枚举为空"两种情况下都返回
    /// 空表，产品侧不打日志（免刷屏），于是真机上无法区分。本函数只在 dev 诊断工具里调。
    pub fn diag_toolhelp() {
        let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        let valid = snap != 0 && snap != INVALID_HANDLE_VALUE;
        println!(
            "diag: CreateToolhelp32Snapshot handle=0x{:X} valid={} GetLastError={}",
            snap,
            valid,
            unsafe { GetLastError() }
        );
        if !valid {
            return;
        }
        let mut entry: ProcessEntry32W = unsafe { std::mem::zeroed() };
        entry.dwSize = std::mem::size_of::<ProcessEntry32W>() as u32;
        let first = unsafe { Process32FirstW(snap, &mut entry) };
        println!(
            "diag: Process32FirstW -> {first} GetLastError={} dwSize={} size_of={}",
            unsafe { GetLastError() },
            entry.dwSize,
            std::mem::size_of::<ProcessEntry32W>()
        );
        let mut total = 0u32;
        let mut osu_names: Vec<String> = Vec::new();
        let mut more = first != 0;
        while more {
            total += 1;
            let name = wide_to_string(&entry.szExeFile);
            if name.to_lowercase().contains("osu") {
                osu_names.push(format!("{}(pid={})", name, entry.th32ProcessID));
            }
            entry.dwSize = std::mem::size_of::<ProcessEntry32W>() as u32;
            more = unsafe { Process32NextW(snap, &mut entry) } != 0;
        }
        println!("diag: total processes seen = {total}");
        println!("diag: names containing 'osu' = {:?}", osu_names);
        unsafe { CloseHandle(snap) };
    }

    /// Toolhelp32 进程枚举 → `(pid, exe 文件名)`。
    pub fn list_osu_processes() -> Result<Vec<(u32, String)>, Reason> {
        let snap = OwnedHandle(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) });
        if !snap.is_valid() {
            return Err(Reason::from_win32_error(unsafe { GetLastError() }));
        }
        let mut out = Vec::new();
        let mut entry: ProcessEntry32W = unsafe { std::mem::zeroed() };
        entry.dwSize = std::mem::size_of::<ProcessEntry32W>() as u32;
        let mut more = unsafe { Process32FirstW(snap.raw(), &mut entry) } != 0;
        while more {
            if wide_to_string(&entry.szExeFile).eq_ignore_ascii_case(OSU_EXE) {
                out.push((entry.th32ProcessID, wide_to_string(&entry.szExeFile)));
            }
            entry.dwSize = std::mem::size_of::<ProcessEntry32W>() as u32;
            more = unsafe { Process32NextW(snap.raw(), &mut entry) } != 0;
        }
        Ok(out)
    }

    /// 目标进程模块的 32 位基址（`MODULEENTRY32W.modBaseAddr`），按模块名匹配。
    pub fn module_base(pid: u32, module_name: &str) -> Result<u32, Reason> {
        let snap = OwnedHandle(unsafe {
            CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid)
        });
        if !snap.is_valid() {
            return Err(Reason::from_win32_error(unsafe { GetLastError() }));
        }
        let mut entry: ModuleEntry32W = unsafe { std::mem::zeroed() };
        entry.dwSize = std::mem::size_of::<ModuleEntry32W>() as u32;
        let mut more = unsafe { Module32FirstW(snap.raw(), &mut entry) } != 0;
        while more {
            if wide_to_string(&entry.szModule).eq_ignore_ascii_case(module_name) {
                return Ok(entry.modBaseAddr as usize as u32);
            }
            entry.dwSize = std::mem::size_of::<ModuleEntry32W>() as u32;
            more = unsafe { Module32NextW(snap.raw(), &mut entry) } != 0;
        }
        Err(Reason::SignatureMiss("module-base"))
    }

    /// 活进程的映像路径（`PROCESS_QUERY_INFORMATION` 足够）。
    pub fn live_image_path(handle: Handle, pid: u32) -> Option<PathBuf> {
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        if unsafe { QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut len) } != 0
            && len > 0
        {
            return Some(PathBuf::from(String::from_utf16_lossy(
                &buf[..len as usize],
            )));
        }
        let _ = pid;
        None
    }

    /// 磁盘映像的 PE machine（决定 stable/lazer 分派）。只读文件，不 attach。
    pub fn pe_machine_from_file(path: &Path) -> Result<u16, String> {
        use std::io::{Read, Seek, SeekFrom};
        let mut f =
            std::fs::File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
        let mut dos = [0u8; 64];
        f.read_exact(&mut dos)
            .map_err(|e| format!("read DOS: {e}"))?;
        if &dos[0..2] != b"MZ" {
            return Err("not an MZ image".to_string());
        }
        let e_lfanew = u32::from_le_bytes([dos[0x3C], dos[0x3D], dos[0x3E], dos[0x3F]]) as u64;
        f.seek(SeekFrom::Start(e_lfanew))
            .map_err(|e| format!("seek e_lfanew: {e}"))?;
        let mut pe = [0u8; 6];
        f.read_exact(&mut pe)
            .map_err(|e| format!("read PE header: {e}"))?;
        if &pe[0..4] != b"PE\0\0" {
            return Err("no PE signature".to_string());
        }
        Ok(u16::from_le_bytes([pe[4], pe[5]]))
    }

    /// 已附着（或待附着）的 target。`handle` 由本结构体独占，`Drop` 时 `CloseHandle`。
    pub struct Target {
        handle: Handle,
        pub pid: u32,
        pub image_path: PathBuf,
        pub bitness: u16,
        pub module_base: u32,
    }

    impl Drop for Target {
        fn drop(&mut self) {
            if self.handle != 0 && self.handle != INVALID_HANDLE_VALUE {
                unsafe { CloseHandle(self.handle) };
            }
        }
    }

    impl Target {
        pub fn handle(&self) -> Handle {
            self.handle
        }

        /// 该 target 的可扫区域：**命中缓存即复用，未命中才走 `VirtualQueryEx`**，
        /// 结果写回 `cache`（缓存的生命周期 = 本 `Target`，见 `scan::RegionCache`）。
        pub fn regions_cached(
            &self,
            cache: &mut crate::osu::scan::RegionCache,
            access_mask: u32,
            limit: usize,
        ) -> Vec<crate::osu::scan::Region> {
            if let Some(hit) = cache.get(access_mask) {
                return hit.to_vec();
            }
            let regions = walk_regions(self.handle, access_mask, limit);
            cache.refresh(access_mask, regions.clone());
            regions
        }
    }

    /// 以 `PROCESS_VM_READ | PROCESS_QUERY_INFORMATION` 打开进程，成功即返回 RAII target。
    pub fn open_target(pid: u32, image_path: PathBuf, bitness: u16) -> Result<Target, Reason> {
        let handle = unsafe { OpenProcess(DESIRED_ACCESS, 0, pid) };
        if handle == 0 || handle == INVALID_HANDLE_VALUE {
            return Err(Reason::from_win32_error(unsafe { GetLastError() }));
        }
        let module_base = module_base(pid, OSU_EXE).unwrap_or(0);
        Ok(Target {
            handle,
            pid,
            image_path,
            bitness,
            module_base,
        })
    }

    /// 进程选择（计划 §3.4 分派规则）：**32 位（PE machine 0x014C）的 `osu!.exe` 是
    /// 稳定目标**；同名的 64 位进程（0x8664）是 lazer ⇒ 本步只记录其存在、不读它。
    ///
    /// 判据表：
    /// - 恰好 1 个 32 位实例 ⇒ `Ok(Target)`；存在 64 位实例 ⇒ 只记录（不进 reason）
    /// - 0 个 32 位实例 + 有 64 位实例 ⇒ `client-ambiguous`（分派不出 stable）
    /// - 0 个 `osu!.exe` ⇒ `process-not-found`
    /// - ≥2 个 32 位实例 ⇒ `multiple-instances`（绝不猜跟哪一个）
    ///
    /// ⚠️ 发现阶段用 Toolhelp32（快，~1ms）**并且**保留一条 PID 扫描兜底：
    /// 实测本机存在"Toolhelp32 看不到 `osu!.exe`、但 `OpenProcess` + 读内存完全正常"
    /// 的上下文（沙箱/受限令牌下的子进程，见 B2 证据 `diag-*.txt`：枚举 391 个进程里
    /// 一个 osu! 都没有，而同一进程 `--pid=15936` 打开读 `MZ` 头成功）。
    /// 发现不到就彻底不可用 ⇒ **枚举不是门，打不开才是门**；因此枚举空表时退化为
    /// 逐个 PID 试开 + 读映像路径判名（只多花几秒，且只在快路径失败时才发生）。
    pub fn select_target() -> Result<Target, Reason> {
        if let Some(found) = discover_by_toolhelp()? {
            SWEPT.with(|slot| slot.set(false));
            eprintln!(
                "[osu] discovery: toolhelp32 stable={} lazer={}",
                found.stable.len(),
                found.lazer_running
            );
            return finish_selection(found);
        }
        let swept = discover_by_pid_sweep();
        SWEPT.with(|slot| slot.set(true));
        eprintln!(
            "[osu] discovery: pid-sweep stable={} lazer={} (toolhelp32 returned no osu!.exe)",
            swept.stable.len(),
            swept.lazer_running
        );
        finish_selection(swept)
    }

    /// 上一次目标发现是否用了兜底扫描。
    pub fn last_discovery_used_sweep() -> bool {
        SWEPT.with(|slot| slot.get())
    }

    /// 候选进程指纹：只读（`PROCESS_QUERY_INFORMATION` 够用）。
    #[derive(Clone, Debug)]
    pub struct Fingerprint {
        pub pid: u32,
        pub parent_pid: u32,
        /// `PROCESSENTRY32W.cntThreads`（0 = 该 PID 不在 Toolhelp32 进程表里）。
        pub threads: u32,
        pub image_path: PathBuf,
        pub bitness: u16,
        /// `VirtualQueryEx` walk 选中的 RW/RWX 区域数（内存规模的直接证据）。
        pub regions: usize,
        /// 私有提交字节数（`GetProcessMemoryInfo` 的 `PrivateUsage`）。
        pub private_bytes: u64,
    }

    /// 目标选择判据（写死，避免"看起来合理就选"）：私有提交内存 ≥ 128 MiB 即"像游戏本体"。
    ///
    /// 依据：本机真机实测的 stable 主进程私有提交约 600 MiB 量级，而同映像的辅助进程
    /// （PID 相近的那几个）都在 10 MiB 以下（见 B2 证据 `diag-fingerprint.txt`）。
    pub const GAME_MIN_PRIVATE_BYTES: u64 = 128 * 1024 * 1024;

    /// 诊断用：兜底 PID 扫描的原始结果（已按坐标去重，与 `discover_by_pid_sweep` 同一判据）。
    pub fn sweep_osu_pids() -> Vec<u32> {
        let mut found: Vec<(u32, PathBuf, u64, usize)> = Vec::new();
        for pid in PID_SWEEP_RANGE {
            if let Seen::Stable(path, private, regions) = see_pid(pid) {
                found.push((pid, path, private, regions));
            } else if matches!(see_pid(pid), Seen::Lazer) {
                found.push((pid, PathBuf::from("<lazer>"), 0, 0));
            }
        }
        let (kept, _) = dedupe_by_coordinate(found);
        kept.into_iter().map(|(pid, _, _)| pid).collect()
    }

    /// 诊断用：未去重的全部候选坐标（看清"试开幻觉"的规模）。
    pub fn sweep_osu_coordinates() -> Vec<(u32, String, u64, usize)> {
        let mut out = Vec::new();
        for pid in PID_SWEEP_RANGE {
            if let Seen::Stable(path, private, regions) = see_pid(pid) {
                out.push((pid, path.to_string_lossy().to_string(), private, regions));
            }
        }
        out
    }

    #[repr(C)]
    #[allow(non_snake_case)]
    struct ProcessMemoryCountersEx {
        cb: u32,
        PageFaultCount: u32,
        PeakWorkingSetSize: usize,
        WorkingSetSize: usize,
        QuotaPeakPagedPoolUsage: usize,
        QuotaPagedPoolUsage: usize,
        QuotaPeakNonPagedPoolUsage: usize,
        QuotaNonPagedPoolUsage: usize,
        PagefileUsage: usize,
        PeakPagefileUsage: usize,
        PrivateUsage: usize,
    }

    #[link(name = "psapi")]
    extern "system" {
        fn GetProcessMemoryInfo(
            Process: Handle,
            ppsmemCounters: *mut ProcessMemoryCountersEx,
            cb: u32,
        ) -> i32;
    }

    /// 读候选进程的规模指纹（只读）。
    pub fn fingerprint(pid: u32) -> Option<Fingerprint> {
        let handle = unsafe { OpenProcess(DESIRED_ACCESS, 0, pid) };
        if handle == 0 || handle == INVALID_HANDLE_VALUE {
            return None;
        }
        let h = OwnedHandle(handle);
        let image_path = live_image_path(h.raw(), pid)?;
        let bitness = pe_machine_from_file(&image_path).unwrap_or(0);
        let regions = walk_regions_count(h.raw());
        let private_bytes = private_usage(h.raw()).unwrap_or(0);
        let (threads, parent_pid) = toolhelp_entry(pid);
        Some(Fingerprint {
            pid,
            parent_pid,
            threads,
            image_path,
            bitness,
            regions,
            private_bytes,
        })
    }

    /// 该 PID 在 Toolhelp32 进程表里的 `(cntThreads, th32ParentProcessID)`；不在表里 → `(0, 0)`。
    pub fn toolhelp_entry(pid: u32) -> (u32, u32) {
        let snap = OwnedHandle(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) });
        if !snap.is_valid() {
            return (0, 0);
        }
        let mut entry: ProcessEntry32W = unsafe { std::mem::zeroed() };
        entry.dwSize = std::mem::size_of::<ProcessEntry32W>() as u32;
        let mut more = unsafe { Process32FirstW(snap.raw(), &mut entry) } != 0;
        while more {
            if entry.th32ProcessID == pid {
                return (entry.cntThreads, entry.th32ParentProcessID);
            }
            entry.dwSize = std::mem::size_of::<ProcessEntry32W>() as u32;
            more = unsafe { Process32NextW(snap.raw(), &mut entry) } != 0;
        }
        (0, 0)
    }

    fn private_usage(handle: Handle) -> Option<u64> {
        let mut counters: ProcessMemoryCountersEx = unsafe { std::mem::zeroed() };
        counters.cb = std::mem::size_of::<ProcessMemoryCountersEx>() as u32;
        let ok = unsafe {
            GetProcessMemoryInfo(
                handle,
                &mut counters,
                std::mem::size_of::<ProcessMemoryCountersEx>() as u32,
            )
        };
        if ok == 0 {
            None
        } else {
            Some(counters.PrivateUsage as u64)
        }
    }

    fn walk_regions_count(handle: Handle) -> usize {
        walk_regions(handle, FILTER_RW, REGION_WALK_LIMIT).len()
    }

    /// 指纹 walk 的区域上限（只用于规模判断，不需要走完整个地址空间）。
    const REGION_WALK_LIMIT: usize = 4096;

    /// 快路径：Toolhelp32 枚举 + 磁盘 PE machine 分派。
    fn discover_by_toolhelp() -> Result<Option<Candidates>, Reason> {
        let procs = list_osu_processes()?;
        if procs.is_empty() {
            return Ok(None);
        }
        let mut stable: Vec<(u32, PathBuf, u64)> = Vec::new();
        // 3 个进程的规模，逐个开句柄取映像路径 + 磁盘 PE machine 完全够用；
        // 开不上的（权限/退出竞争）记下错误码，但不阻断其它候选。
        for (pid, _name) in procs {
            match see_pid(pid) {
                Seen::Stable(path, private, regions) => {
                    stable.push((pid, path, private));
                    let _ = regions;
                }
                Seen::Lazer | Seen::Other | Seen::Unreadable => {}
            }
        }
        Ok(Some(Candidates {
            stable,
            lazer_running: lazer_seen(),
            fallback_error: None,
        }))
    }

    /// 兜底路径：逐个 PID 试开（范围 4..=65535），读映像路径判 `osu!.exe` 再分派。
    ///
    /// 只在 Toolhelp32 空表时跑；本机实测 30–50 ms 扫完全程（见 B2 证据 `diag-*.txt`）。
    ///
    /// ⚠️ 该上下文里有两个真相，都必须处理（本机实测）：
    /// ① Toolhelp32 的表可能**被截断**（`mma-shell` 只看到 35 个进程，全是它自己的子进程；
    ///    `osu!.exe` 不在其中）⇒ 不能把"表里有没有它"当判据；
    /// ② "试开相邻 PID"会拿到**同一进程的其它坐标**——15937/15938/15939 的映像路径与内存规模
    ///    与真进程 15936 一模一样（该模式在本机是系统性的：几乎每个进程都有 n+1..n+3 三个
    ///    这样"指向同一个进程"的坐标）。
    /// 判据因此是**坐标去重**：同一（映像路径, 区域数, 私有提交）只留最小 PID。
    /// 它只依赖读到的内容，与调用方上下文无关（`GetProcessId` 在本机不可靠，不用）。
    fn discover_by_pid_sweep() -> Candidates {
        let mut found: Vec<(u32, PathBuf, u64, usize)> = Vec::new();
        let mut opened = 0u32;
        for pid in PID_SWEEP_RANGE {
            match see_pid(pid) {
                Seen::Stable(path, private, regions) => found.push((pid, path, private, regions)),
                Seen::Unreadable => {}
                Seen::Other | Seen::Lazer => opened += 1,
            }
        }
        if debug_enabled() {
            eprintln!(
                "[osu] pid-sweep detail: readable-but-not-osu={} candidates={:?}",
                opened,
                found
                    .iter()
                    .map(|(pid, path, private, regions)| format!(
                        "{pid}:{}:{}MB:{regions}",
                        path.display(),
                        private / (1024 * 1024)
                    ))
                    .collect::<Vec<_>>()
            );
        }
        let (stable, dropped) = dedupe_by_coordinate(found);
        if !dropped.is_empty() {
            eprintln!(
                "[osu] pid-sweep: ignored {} duplicate coordinate(s) of the same process: {:?}",
                dropped.len(),
                dropped
            );
        }
        // lazer 的存在由 `lazer_seen()` 单独记录（本步不读 lazer）。
        Candidates {
            stable,
            lazer_running: lazer_seen(),
            fallback_error: Some(Reason::ProcessNotFound),
        }
    }

    fn finish_selection(candidates: Candidates) -> Result<Target, Reason> {
        // 没有 32 位 osu!.exe ⇒ 分不出 stable；`fallback_error` 区分"根本没有 osu! 进程"
        // 与"有 64 位进程（lazer）但本步不读它"。lazer 的存在另由 `lazer_seen()` 记录。
        if candidates.stable.is_empty() {
            return Err(match candidates.fallback_error {
                Some(reason) if !candidates.lazer_running => reason,
                _ => Reason::ClientAmbiguous,
            });
        }
        // 唯一候选：直接用。
        if candidates.stable.len() == 1 {
            let (pid, path, _private) = candidates.stable.into_iter().next().expect("len == 1");
            return open_target(pid, path, PE_MACHINE_I386);
        }
        // 多个候选：**按规模判定**——同一份 `osu!.exe` 映像会同时存在若干辅助进程
        // （本机实测 4 个 PID 共用该映像），游戏本体是唯一一个私有提交在 100 MiB 量级的。
        let mut ranked = candidates.stable;
        ranked.sort_by(|a, b| b.2.cmp(&a.2));
        let top = ranked[0].clone();
        let second = ranked[1].clone();
        let top_looks_like_game = top.2 >= GAME_MIN_PRIVATE_BYTES;
        let separated = top.2 >= second.2.saturating_mul(2);
        if top_looks_like_game && separated {
            return open_target(top.0, top.1, PE_MACHINE_I386);
        }
        // 两个都像游戏（或都比不出高低）⇒ 不猜：reason `multiple-instances`。
        Err(Reason::MultipleInstances)
    }

    struct Candidates {
        /// `(pid, 映像路径, 私有提交字节数)`。
        stable: Vec<(u32, PathBuf, u64)>,
        lazer_running: bool,
        fallback_error: Option<Reason>,
    }

    thread_local! {
        /// 本次目标发现里是否见到了 64 位 `osu!.exe`（lazer）。本步**不读** lazer，
        /// 只记录它的存在（计划 §3.4：lazer 走另一条链，E 步才实现）。
        static LAZER_SEEN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
        /// 上一次 `select_target()` 是否走了 PID 扫描兜底。
        static SWEPT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }

    /// 上一次目标发现里是否见到 lazer（记录用，不参与分派）。
    pub fn lazer_seen() -> bool {
        LAZER_SEEN.with(|slot| slot.get())
    }

    /// 一次打开、一次判定（句柄只在本次调用内存在，RAII 关闭）。
    enum Seen {
        Stable(PathBuf, u64, usize),
        Lazer,
        Other,
        Unreadable,
    }

    fn see_pid(pid: u32) -> Seen {
        let handle = unsafe { OpenProcess(DESIRED_ACCESS, 0, pid) };
        if handle == 0 || handle == INVALID_HANDLE_VALUE {
            return Seen::Unreadable;
        }
        let h = OwnedHandle(handle);
        let Some(path) = live_image_path(h.raw(), pid) else {
            return Seen::Unreadable;
        };
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        if !name.eq_ignore_ascii_case(OSU_EXE) {
            return Seen::Other;
        }
        let private = private_usage(h.raw()).unwrap_or(0);
        let regions = walk_regions_count(h.raw());
        match pe_machine_from_file(&path) {
            Ok(PE_MACHINE_I386) => Seen::Stable(path, private, regions),
            Ok(PE_MACHINE_AMD64) => {
                LAZER_SEEN.with(|slot| slot.set(true));
                Seen::Lazer
            }
            _ => Seen::Other,
        }
    }

    /// 候选的一把"坐标锁"：同一份映像 + 相同区域数 + 相同私有提交 ⇒ 同一进程的不同坐标。
    ///
    /// **为什么不用 `GetProcessId` 判**：本机实测它并不能可靠区分（`osu-diag` 在部分上下文里
    /// 对所有 PID 都返回"就是它自己"，而在另一个上下文里能正确报出 15937→15936）。
    /// 指纹比较只依赖"读到的内容"，与调用方处在哪个上下文无关。
    fn coordinate_key(candidate: &(u32, PathBuf, u64, usize)) -> (String, u64, usize) {
        (
            candidate.1.to_string_lossy().to_lowercase(),
            candidate.2,
            candidate.3,
        )
    }

    /// 同一坐标去重：同键只保留**最小 PID**（同组里第一个被分配的那个）。
    fn dedupe_by_coordinate(
        mut candidates: Vec<(u32, PathBuf, u64, usize)>,
    ) -> (Vec<(u32, PathBuf, u64)>, Vec<u32>) {
        candidates.sort_by_key(|c| c.0);
        let mut seen: Vec<((String, u64, usize), u32)> = Vec::new();
        let mut kept: Vec<(u32, PathBuf, u64)> = Vec::new();
        let mut dropped: Vec<u32> = Vec::new();
        for candidate in candidates {
            let key = coordinate_key(&candidate);
            if let Some((_, first)) = seen.iter().find(|(k, _)| *k == key) {
                dropped.push(candidate.0);
                let _ = first;
                continue;
            }
            seen.push((key, candidate.0));
            kept.push((candidate.0, candidate.1, candidate.2));
        }
        (kept, dropped)
    }

    /// PID 扫描范围：Windows 的 PID 上限是 2^32/4，但实际分配几乎总在 65535 以下。
    pub const PID_SWEEP_RANGE: std::ops::RangeInclusive<u32> = 4..=65_535;

    /// `VirtualQueryEx` walk：按 `access_mask` 过滤出可扫区域。
    ///
    /// 与 P1 探针同一份判据（可对齐证据）：`MEM_COMMIT && (protect & mask) != 0`，
    /// 并且 `PAGE_GUARD` 区域**先跳过再读**（guard 页是已提交但读取会触发异常）。
    pub fn walk_regions(
        handle: Handle,
        access_mask: u32,
        limit: usize,
    ) -> Vec<crate::osu::scan::Region> {
        let mut out = Vec::new();
        let mut addr: usize = 0;
        while out.len() < limit {
            let mut mbi = MemoryBasicInformation::default();
            let n = unsafe {
                VirtualQueryEx(
                    handle,
                    addr as *const u8,
                    &mut mbi,
                    std::mem::size_of::<MemoryBasicInformation>(),
                )
            };
            if n == 0 {
                break;
            }
            let size = mbi.RegionSize;
            if size == 0 {
                break;
            }
            let readable = (mbi.State & MEM_COMMIT) != 0
                && (mbi.Protect & access_mask) != 0
                && (mbi.Protect & PAGE_GUARD) == 0
                && (mbi.Protect & PAGE_NOACCESS) == 0;
            if readable {
                out.push(crate::osu::scan::Region { base: addr, size });
            }
            match addr.checked_add(size) {
                Some(next) => addr = next,
                None => break,
            }
        }
        out
    }

    /// 单次 `ReadProcessMemory` 的**硬上限**（1 MiB）。与 `scan::CHUNK_MAX` **同值**
    /// （层内常量按 §3.4 的"单次读上限 1 MiB"取整；`scan.rs` 直接引用本常量）。
    ///
    /// 为什么要在这里再夹一次：`scan.rs` 的分块是"扫描策略"（它还要带尾接、要按区推进），
    /// 而本函数是**产品侧唯一的下发点**——字符串/字段读若哪天被传进一个大缓冲（例如
    /// 64 MiB 的 lazer 扫描），没有这道夹取就会变成"一次请求 64 MiB"，
    /// 与 §3.4 的约定不符。
    pub const READ_CALL_MAX: usize = 1024 * 1024;

    /// `ERROR_PARTIAL_COPY`：请求的区间**不是整段可访问**（跨区/页尾）。
    /// `ReadProcessMemory` 的典型失败形态，处置 = 缩小重试（见 `plan_shrink_sequence`）。
    pub const ERROR_PARTIAL_COPY: u32 = 299;

    /// 单次 `ReadProcessMemory`：**必须整段读满**，否则报错（含 `ERROR_PARTIAL_COPY`）。
    ///
    /// 绝不零填充：失败时调用方拿到 `Err`，而不是一段被 0 污染的缓冲。短读（Win32 当成功
    /// 但 `lpNumberOfBytesRead < nSize`）与失败同义（fail-closed），同样不补齐。
    ///
    /// `buf.len()` 超过 [`READ_CALL_MAX`] 时返回 [`ERROR_PARTIAL_COPY`] 风格的错误码
    /// （`ERROR_INVALID_PARAMETER = 87`）而**不是**悄悄截断——静默截断会让调用方拿着半段
    /// 缓冲当整段用（比报错危险得多）。
    pub fn read_exact_at(handle: Handle, addr: u32, buf: &mut [u8]) -> Result<(), u32> {
        if buf.is_empty() {
            return Ok(());
        }
        if buf.len() > READ_CALL_MAX {
            return Err(87); // ERROR_INVALID_PARAMETER：超单次上限，调用方必须走 chunked
        }
        let mut got: usize = 0;
        let ok = unsafe {
            ReadProcessMemory(
                handle,
                addr as usize as *const u8,
                buf.as_mut_ptr(),
                buf.len(),
                &mut got,
            )
        };
        if ok == 0 {
            return Err(unsafe { GetLastError() });
        }
        if got != buf.len() {
            // 短读：Win32 把它当成功但 `lpNumberOfBytesRead < nSize`；这是"读不满"，
            // 与失败同义（fail-closed），且**不能**用 0 补齐剩余部分冒充成功。
            return Err(ERROR_PARTIAL_COPY);
        }
        Ok(())
    }

    /// 超长读：**同一次读取**语义，但按 [`READ_CALL_MAX`] 切块下达，每一块仍然必须读满。
    ///
    /// 与"缩小重试"的分工：本函数不重试、不缩小——任何一块读不满就整体失败（fail-closed），
    /// 由调用方决定是否缩小区间重来（`scan.rs::read_chunk` 就是这么做的）。
    /// `addr` 在这里按 `usize` 传递：x64 目标的地址放不进 `u32`（lazer 在 E 步）。
    pub fn read_exact_chunked_at(handle: Handle, addr: usize, buf: &mut [u8]) -> Result<(), u32> {
        for (index, chunk) in buf.chunks_mut(READ_CALL_MAX).enumerate() {
            let at = addr.wrapping_add(index * READ_CALL_MAX);
            read_exact_at(handle, at as u32, chunk)?;
        }
        Ok(())
    }

    /// 缩小重试的计划（**纯函数**，可单测；`scan.rs` 的读取策略就是它的产物）。
    ///
    /// 语义：从 `want` 起，每次失败就把请求长度**减半**再试，直到小于 `floor` 为止；
    /// 返回的是"依次尝试的请求长度"。`want` 先被 [`READ_CALL_MAX`] 夹取（单次硬上限）。
    ///
    /// - 空计划 = 不尝试（`floor == 0` 或 `want == 0`）；
    /// - 计划里**不会出现 0 长度**的请求（0 长度读毫无意义，且会掩盖"读不到"）。
    pub fn plan_shrink_sequence(want: usize, floor: usize) -> Vec<usize> {
        let mut plan = Vec::new();
        if floor == 0 {
            return plan;
        }
        let mut len = want.min(READ_CALL_MAX);
        while len >= floor {
            plan.push(len);
            len /= 2;
        }
        plan
    }

    fn read_array<const N: usize>(handle: Handle, addr: u32) -> Result<[u8; N], Reason> {
        let mut buf = [0u8; N];
        read_exact_at(handle, addr, &mut buf).map_err(Reason::from_win32_error)?;
        Ok(buf)
    }

    pub fn read_u32(target: &Target, addr: u32) -> Result<u32, Reason> {
        read_array::<4>(target.handle(), addr).map(u32::from_le_bytes)
    }

    /// 16 位读（C2：hits 的计数域是 u16；`maxCombo` 同样是 u16）。
    pub fn read_u16(target: &Target, addr: u32) -> Result<u16, Reason> {
        read_array::<2>(target.handle(), addr).map(u16::from_le_bytes)
    }

    pub fn read_i32(target: &Target, addr: u32) -> Result<i32, Reason> {
        read_u32(target, addr).map(|v| v as i32)
    }

    pub fn read_f64(target: &Target, addr: u32) -> Result<f64, Reason> {
        read_array::<8>(target.handle(), addr).map(f64::from_le_bytes)
    }

    /// `readPointer`（P1 探针 `read.rs:45-48` 的同一形状）：**两次解引用**——
    /// 读 `addr` 处的 u32 得到对象地址，再读该对象地址处的 u32。
    /// 32 位目标两次读都是 4 字节（x64 目标要换成 8 字节，见 §3.4；lazer 在 E 步）。
    ///
    /// ⚠️ 与 `patterns::Anchor::offset` 的分工：`offset` 只在 `scan.rs` 定址时施加
    /// **一次**。若某条链需要"先加偏移再解引用"，请显式写 `addr + off`，不要再动 offset。
    pub fn read_pointer(target: &Target, addr: u32) -> Result<u32, Reason> {
        let inner = read_u32(target, addr)?;
        read_u32(target, inner)
    }

    /// 32 位 C# 字符串（P1 `semantics-*.txt` 自证：`tosu-hits 11 : 0` 压倒性地选了本约定）。
    ///
    /// 布局：`[+0x00]` MethodTable 指针、`[+0x04]` int32 **UTF-16 码元数**、`[+0x08]` 起正文。
    /// 防护（计划 §3.4）：
    /// - 先读长度再读正文，**长度必须 ∈ 1..=4096**（0 或过大直接判无效，绝不按长度分配巨缓冲）
    /// - 正文按 `length * 2` 字节一次读满（短读即失败），转 UTF-16 失败即无效
    /// - 不把尾随 NUL 当分隔符：正文里出现 NUL 一律判无效
    pub fn read_csharp_string(target: &Target, addr: u32) -> Result<String, Reason> {
        if addr == 0 {
            return Err(Reason::InvariantFailed("string-ptr-null"));
        }
        let len_addr = addr.checked_add(4).ok_or(Reason::ReadError)?;
        let len = read_u32(target, len_addr)? as i64;
        if len <= 0 || len > MAX_CSHARP_STRING_UNITS as i64 {
            return Err(Reason::InvariantFailed("string-length"));
        }
        let mut raw = vec![0u8; len as usize * 2];
        let body_addr = addr.checked_add(8).ok_or(Reason::ReadError)?;
        read_exact_at(target.handle(), body_addr, &mut raw).map_err(Reason::from_win32_error)?;
        let units: Vec<u16> = raw
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        if units.contains(&0) {
            return Err(Reason::InvariantFailed("string-nul"));
        }
        String::from_utf16(&units).map_err(|_| Reason::InvariantFailed("string-utf16"))
    }
}
