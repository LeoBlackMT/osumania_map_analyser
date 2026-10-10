// sys::windows::ffi - 严格受限的只读 Win32 API 声明与类型

#![allow(non_snake_case, non_camel_case_types)]

pub type Handle = isize;
pub const INVALID_HANDLE_VALUE: Handle = -1;

// 访问权限：仅允许读内存与有限信息查询，绝不包含写入或注入权限
pub const PROCESS_VM_READ: u32 = 0x0010;
pub const PROCESS_QUERY_INFORMATION: u32 = 0x0400;
pub const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
pub const DESIRED_READ_ACCESS: u32 = PROCESS_VM_READ | PROCESS_QUERY_INFORMATION;

// PE Machine 架构标识
pub const PE_MACHINE_I386: u16 = 0x014C;
pub const PE_MACHINE_AMD64: u16 = 0x8664;

// Toolhelp 快照标志
pub const TH32CS_SNAPPROCESS: u32 = 0x00000002;
pub const TH32CS_SNAPMODULE: u32 = 0x00000008;
pub const TH32CS_SNAPMODULE32: u32 = 0x00000010;

// 内存分页与提交状态
pub const MEM_COMMIT: u32 = 0x1000;
pub const PAGE_NOACCESS: u32 = 0x01;
pub const PAGE_READONLY: u32 = 0x02;
pub const PAGE_READWRITE: u32 = 0x04;
pub const PAGE_WRITECOPY: u32 = 0x08;
pub const PAGE_EXECUTE_READ: u32 = 0x20;
pub const PAGE_EXECUTE_READWRITE: u32 = 0x40;
pub const PAGE_EXECUTE_WRITECOPY: u32 = 0x80;
pub const PAGE_GUARD: u32 = 0x100;

pub const FILTER_READABLE: u32 = PAGE_READONLY
    | PAGE_READWRITE
    | PAGE_WRITECOPY
    | PAGE_EXECUTE_READ
    | PAGE_EXECUTE_READWRITE
    | PAGE_EXECUTE_WRITECOPY;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ProcessEntry32W {
    pub dwSize: u32,
    pub cntUsage: u32,
    pub th32ProcessID: u32,
    pub th32DefaultHeapID: usize,
    pub th32ModuleID: u32,
    pub cntThreads: u32,
    pub th32ParentProcessID: u32,
    pub pcPriClassBase: i32,
    pub dwFlags: u32,
    pub szExeFile: [u16; 260],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ModuleEntry32W {
    pub dwSize: u32,
    pub th32ModuleID: u32,
    pub th32ProcessID: u32,
    pub GlblcntUsage: u32,
    pub ProccntUsage: u32,
    pub modBaseAddr: *mut u8,
    pub modBaseSize: u32,
    pub hModule: Handle,
    pub szModule: [u16; 256],
    pub szExePath: [u16; 260],
}

#[repr(C)]
pub struct MemoryBasicInformation {
    pub BaseAddress: *mut std::ffi::c_void,
    pub AllocationBase: *mut std::ffi::c_void,
    pub AllocationProtect: u32,
    pub PartitionId: u16,
    pub RegionSize: usize,
    pub State: u32,
    pub Protect: u32,
    pub Type: u32,
}

#[cfg(windows)]
#[allow(clashing_extern_declarations)]
extern "system" {
    pub fn OpenProcess(dwDesiredAccess: u32, bInheritHandle: i32, dwProcessId: u32) -> Handle;
    pub fn CloseHandle(hObject: Handle) -> i32;
    pub fn ReadProcessMemory(
        hProcess: Handle,
        lpBaseAddress: *const std::ffi::c_void,
        lpBuffer: *mut std::ffi::c_void,
        nSize: usize,
        lpNumberOfBytesRead: *mut usize,
    ) -> i32;
    pub fn VirtualQueryEx(
        hProcess: Handle,
        lpAddress: *const std::ffi::c_void,
        lpBuffer: *mut MemoryBasicInformation,
        dwLength: usize,
    ) -> usize;
    pub fn CreateToolhelp32Snapshot(dwFlags: u32, th32ProcessID: u32) -> Handle;
    pub fn Process32FirstW(hSnapshot: Handle, lppe: *mut ProcessEntry32W) -> i32;
    pub fn Process32NextW(hSnapshot: Handle, lppe: *mut ProcessEntry32W) -> i32;
    pub fn Module32FirstW(hSnapshot: Handle, lpme: *mut ModuleEntry32W) -> i32;
    pub fn Module32NextW(hSnapshot: Handle, lpme: *mut ModuleEntry32W) -> i32;
    pub fn QueryFullProcessImageNameW(
        hProcess: Handle,
        dwFlags: u32,
        lpExeName: *mut u16,
        lpdwSize: *mut u32,
    ) -> i32;
    pub fn GetLastError() -> u32;
}
