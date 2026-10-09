// sys::windows::memory - 统一且带缩小重试机制的安全内存读取器

use super::ffi::{self, Handle};

/// 单次 ReadProcessMemory 调用的安全硬上限（1 MiB）。
pub const READ_CALL_MAX: usize = 1024 * 1024;
/// 部分复制错误码（ERROR_PARTIAL_COPY = 299）。
pub const ERROR_PARTIAL_COPY: u32 = 299;

/// 内存读取错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemoryReadError {
    BadAddress,
    PartialCopy,
    AccessDenied,
    SystemError(u32),
    InvalidStringUtf16,
}

impl std::fmt::Display for MemoryReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadAddress => write!(f, "bad-address"),
            Self::PartialCopy => write!(f, "partial-copy"),
            Self::AccessDenied => write!(f, "access-denied"),
            Self::SystemError(code) => write!(f, "win32-error:{}", code),
            Self::InvalidStringUtf16 => write!(f, "invalid-utf16"),
        }
    }
}

impl std::error::Error for MemoryReadError {}

/// 缩小重试计划生成函数：从 want 开始，每次失败减半，直到低于 floor。
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

/// 统一的进程内存读取抽象 Trait。
pub trait MemoryReader {
    /// 精确读满缓冲区；如果发生短读或无法整段读满，返回错误（绝不补零）。
    fn read_exact(&self, addr: u64, buf: &mut [u8]) -> Result<(), MemoryReadError>;

    fn read_u32(&self, addr: u64) -> Result<u32, MemoryReadError> {
        let mut buf = [0u8; 4];
        self.read_exact(addr, &mut buf)?;
        Ok(u32::from_le_bytes(buf))
    }

    fn read_i32(&self, addr: u64) -> Result<i32, MemoryReadError> {
        let mut buf = [0u8; 4];
        self.read_exact(addr, &mut buf)?;
        Ok(i32::from_le_bytes(buf))
    }

    fn read_u64(&self, addr: u64) -> Result<u64, MemoryReadError> {
        let mut buf = [0u8; 8];
        self.read_exact(addr, &mut buf)?;
        Ok(u64::from_le_bytes(buf))
    }

    fn read_f64(&self, addr: u64) -> Result<f64, MemoryReadError> {
        let mut buf = [0u8; 8];
        self.read_exact(addr, &mut buf)?;
        Ok(f64::from_le_bytes(buf))
    }

    fn read_pointer32(&self, addr: u64) -> Result<u32, MemoryReadError> {
        let ptr1 = self.read_u32(addr)?;
        self.read_u32(u64::from(ptr1))
    }

    /// 读取 C# (.NET BCL) 字符串：`[addr + 4]` 为长度，后跟 UTF-16LE 码元。
    fn read_csharp_string(&self, addr: u64, max_units: usize) -> Result<String, MemoryReadError> {
        if addr == 0 {
            return Err(MemoryReadError::BadAddress);
        }
        let len = self.read_i32(addr + 4)? as usize;
        if len == 0 {
            return Ok(String::new());
        }
        if len > max_units {
            return Err(MemoryReadError::BadAddress);
        }
        let byte_len = len * 2;
        let mut raw = vec![0u8; byte_len];
        self.read_exact(addr + 8, &mut raw)?;

        let mut units = Vec::with_capacity(len);
        for chunk in raw.chunks_exact(2) {
            units.push(u16::from_le_bytes([chunk[0], chunk[1]]));
        }
        String::from_utf16(&units).map_err(|_| MemoryReadError::InvalidStringUtf16)
    }
}

/// 基于 Windows 进程句柄的原生内存读取实现。
pub struct LiveProcessReader {
    handle: Handle,
}

impl LiveProcessReader {
    pub fn new(handle: Handle) -> Self {
        Self { handle }
    }
}

impl MemoryReader for LiveProcessReader {
    fn read_exact(&self, addr: u64, buf: &mut [u8]) -> Result<(), MemoryReadError> {
        if buf.is_empty() {
            return Ok(());
        }
        #[cfg(windows)]
        {
            let mut read_bytes: usize = 0;
            let ok = unsafe {
                ffi::ReadProcessMemory(
                    self.handle,
                    addr as *const std::ffi::c_void,
                    buf.as_mut_ptr() as *mut std::ffi::c_void,
                    buf.len(),
                    &mut read_bytes,
                )
            };
            if ok != 0 && read_bytes == buf.len() {
                Ok(())
            } else {
                let err = unsafe { ffi::GetLastError() };
                if err == ERROR_PARTIAL_COPY {
                    Err(MemoryReadError::PartialCopy)
                } else if err == 5 {
                    Err(MemoryReadError::AccessDenied)
                } else {
                    Err(MemoryReadError::SystemError(err))
                }
            }
        }
        #[cfg(not(windows))]
        {
            let _ = (addr, buf);
            Err(MemoryReadError::AccessDenied)
        }
    }
}
