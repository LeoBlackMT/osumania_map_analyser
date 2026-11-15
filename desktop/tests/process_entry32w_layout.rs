//! x64 Win32 FFI layout regression tests for osu! process discovery.
//!
//! Issue #131: `PROCESSENTRY32W` had an extra `_pad1` after `dwFlags`, which pushed
//! `szExeFile` from @44 to @48. Both layouts have `size_of == 568`, so the old
//! size-only assertion passed at compile time while `Process32FirstW` wrote the
//! process name at @44 and Rust read it from @48. Every process name was therefore
//! missing its first two characters, and `list_osu_processes` could never match
//! `osu!.exe`. These tests pin the field offsets (not just the struct size).

#![cfg(windows)]

use mma_shell::osu::win::{
    is_osu_exe_name, MemoryBasicInformation, ModuleEntry32W, ProcessEntry32W,
};
use std::mem::{align_of, offset_of, size_of};

#[test]
fn process_entry32w_layout_matches_x64_caller() {
    assert_eq!(size_of::<ProcessEntry32W>(), 568);
    assert_eq!(align_of::<ProcessEntry32W>(), 8);
    assert_eq!(offset_of!(ProcessEntry32W, dwSize), 0);
    assert_eq!(offset_of!(ProcessEntry32W, cntUsage), 4);
    assert_eq!(offset_of!(ProcessEntry32W, th32ProcessID), 8);
    assert_eq!(offset_of!(ProcessEntry32W, th32DefaultHeapID), 16);
    assert_eq!(offset_of!(ProcessEntry32W, th32ModuleID), 24);
    assert_eq!(offset_of!(ProcessEntry32W, cntThreads), 28);
    assert_eq!(offset_of!(ProcessEntry32W, th32ParentProcessID), 32);
    assert_eq!(offset_of!(ProcessEntry32W, pcPriClassBase), 36);
    assert_eq!(offset_of!(ProcessEntry32W, dwFlags), 40);
    // The regression: MSVC x64 puts szExeFile at @44. @48 truncates every name.
    assert_eq!(offset_of!(ProcessEntry32W, szExeFile), 44);
}

#[test]
fn module_entry32w_layout_matches_x64_caller() {
    assert_eq!(size_of::<ModuleEntry32W>(), 1080);
    assert_eq!(align_of::<ModuleEntry32W>(), 8);
    assert_eq!(offset_of!(ModuleEntry32W, dwSize), 0);
    assert_eq!(offset_of!(ModuleEntry32W, th32ModuleID), 4);
    assert_eq!(offset_of!(ModuleEntry32W, th32ProcessID), 8);
    assert_eq!(offset_of!(ModuleEntry32W, GlblcntUsage), 12);
    assert_eq!(offset_of!(ModuleEntry32W, ProccntUsage), 16);
    assert_eq!(offset_of!(ModuleEntry32W, modBaseAddr), 24);
    assert_eq!(offset_of!(ModuleEntry32W, modBaseSize), 32);
    assert_eq!(offset_of!(ModuleEntry32W, hModule), 40);
    assert_eq!(offset_of!(ModuleEntry32W, szModule), 48);
    assert_eq!(offset_of!(ModuleEntry32W, szExePath), 560);
}

#[test]
fn memory_basic_information_layout_matches_x64_caller() {
    assert_eq!(size_of::<MemoryBasicInformation>(), 48);
    assert_eq!(align_of::<MemoryBasicInformation>(), 8);
    assert_eq!(offset_of!(MemoryBasicInformation, BaseAddress), 0);
    assert_eq!(offset_of!(MemoryBasicInformation, AllocationBase), 8);
    assert_eq!(offset_of!(MemoryBasicInformation, AllocationProtect), 16);
    assert_eq!(offset_of!(MemoryBasicInformation, PartitionId), 20);
    assert_eq!(offset_of!(MemoryBasicInformation, RegionSize), 24);
    assert_eq!(offset_of!(MemoryBasicInformation, State), 32);
    assert_eq!(offset_of!(MemoryBasicInformation, Protect), 36);
    assert_eq!(offset_of!(MemoryBasicInformation, Type), 40);
}

#[test]
fn osu_exe_name_accepts_official_and_alt_names() {
    for name in ["osu!.exe", "OSU!.EXE", "osu.exe", "OSU.EXE"] {
        assert!(is_osu_exe_name(name), "{name} should be accepted");
    }
    for name in ["osu!lazer.exe", "notosu.exe", "osu", ""] {
        assert!(!is_osu_exe_name(name), "{name} should not match");
    }
}
