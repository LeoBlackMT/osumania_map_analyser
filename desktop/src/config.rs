// 路径与 tosu 探测、壳配置与窗口状态门面与统一重导出（契约 §6/§8）

pub mod common;
pub mod detect;
pub mod json;
pub mod tosu;
pub mod window;

pub use common::*;
pub use detect::*;
pub use json::*;
pub use tosu::*;
pub use window::*;

#[cfg(test)]
#[path = "../tests-local/config.rs"]
mod tests;
