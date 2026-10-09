// sys::windows - Windows 平台只读系统访问抽象

pub mod ffi;
pub mod handle;
pub mod memory;
pub mod process;

pub use ffi::*;
pub use handle::ProcessHandle;
pub use memory::{plan_shrink_sequence, LiveProcessReader, MemoryReadError, MemoryReader};
pub use process::{list_modules, list_processes, open_process_read, ModuleItem, ProcessItem};
