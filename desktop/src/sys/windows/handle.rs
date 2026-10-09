// sys::windows::handle - 安全的 RAII 进程与系统句柄管理

use super::ffi::{self, Handle, INVALID_HANDLE_VALUE};

/// 安全的只读进程/快照句柄包装器。
/// 在 Drop 时自动调用 CloseHandle，防止系统句柄泄漏。
#[derive(Debug)]
pub struct ProcessHandle(Handle);

impl ProcessHandle {
    /// 从原生 Handle 创建安全包装。如果句柄无效（<= 0 或 INVALID_HANDLE_VALUE），则返回 None。
    pub fn from_raw(raw: Handle) -> Option<Self> {
        if raw <= 0 || raw == INVALID_HANDLE_VALUE {
            None
        } else {
            Some(Self(raw))
        }
    }

    /// 获取底层原生句柄值。
    pub fn raw(&self) -> Handle {
        self.0
    }

    /// 检查句柄是否有效。
    pub fn is_valid(&self) -> bool {
        self.0 > 0 && self.0 != INVALID_HANDLE_VALUE
    }
}

impl Drop for ProcessHandle {
    fn drop(&mut self) {
        #[cfg(windows)]
        if self.is_valid() {
            unsafe {
                ffi::CloseHandle(self.0);
            }
        }
    }
}
