// app - 桌面壳应用程序展示层与生命周期管理

pub mod builder;
pub mod instance;
pub mod profile;
pub mod shortcuts;
pub mod url;
pub mod window_events;
pub mod window_state;

pub use builder::AppBuilder;
pub use instance::try_forward_and_exit;
pub use profile::ensure_portable_profile;

/// 启动并运行桌面壳应用程序。
pub fn run() {
    AppBuilder::new().build_and_run();
}
