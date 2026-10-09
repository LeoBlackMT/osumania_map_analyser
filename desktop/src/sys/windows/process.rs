// sys::windows::process - 进程枚举、Toolhelp 快照与模块基址探测

use std::path::PathBuf;
use super::ffi::{self, Handle, ModuleEntry32W, ProcessEntry32W, TH32CS_SNAPMODULE, TH32CS_SNAPMODULE32, TH32CS_SNAPPROCESS};
use super::handle::ProcessHandle;

pub struct ProcessItem {
    pub pid: u32,
    pub name: String,
    pub thread_count: u32,
}

pub struct ModuleItem {
    pub name: String,
    pub base: u64,
    pub size: u32,
    pub path: PathBuf,
}

fn wide_to_string(wide: &[u16]) -> String {
    let len = wide.iter().position(|&c| c == 0).unwrap_or(wide.len());
    String::from_utf16_lossy(&wide[..len])
}

/// 枚举当前系统中的所有活动进程。
pub fn list_processes() -> Vec<ProcessItem> {
    let mut out = Vec::new();
    #[cfg(windows)]
    unsafe {
        let snap = ProcessHandle::from_raw(ffi::CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0));
        let Some(snap) = snap else { return out };

        let mut pe: ProcessEntry32W = std::mem::zeroed();
        pe.dwSize = std::mem::size_of::<ProcessEntry32W>() as u32;

        if ffi::Process32FirstW(snap.raw(), &mut pe) != 0 {
            loop {
                let name = wide_to_string(&pe.szExeFile);
                out.push(ProcessItem {
                    pid: pe.th32ProcessID,
                    name,
                    thread_count: pe.cntThreads,
                });
                if ffi::Process32NextW(snap.raw(), &mut pe) == 0 {
                    break;
                }
            }
        }
    }
    out
}

/// 列出目标进程加载的所有模块。
pub fn list_modules(pid: u32) -> Vec<ModuleItem> {
    let mut out = Vec::new();
    #[cfg(windows)]
    unsafe {
        let snap = ProcessHandle::from_raw(ffi::CreateToolhelp32Snapshot(
            TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32,
            pid,
        ));
        let Some(snap) = snap else { return out };

        let mut me: ModuleEntry32W = std::mem::zeroed();
        me.dwSize = std::mem::size_of::<ModuleEntry32W>() as u32;

        if ffi::Module32FirstW(snap.raw(), &mut me) != 0 {
            loop {
                out.push(ModuleItem {
                    name: wide_to_string(&me.szModule),
                    base: me.modBaseAddr as u64,
                    size: me.modBaseSize,
                    path: PathBuf::from(wide_to_string(&me.szExePath)),
                });
                if ffi::Module32NextW(snap.raw(), &mut me) == 0 {
                    break;
                }
            }
        }
    }
    let _ = pid;
    out
}

/// 打开目标进程句柄（只读访问权限）。
pub fn open_process_read(pid: u32) -> Option<ProcessHandle> {
    #[cfg(windows)]
    unsafe {
        ProcessHandle::from_raw(ffi::OpenProcess(ffi::DESIRED_READ_ACCESS, 0, pid))
    }
    #[cfg(not(windows))]
    {
        let _ = pid;
        None
    }
}
