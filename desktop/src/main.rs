// mma-shell 桌面壳入口二进制
// 保持极限精炼（≤ 40 行），生命周期编排与窗口事件均已模块化至 `mma_shell::app`。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // GTK 的原生 Wayland 后端不支持置顶；在建窗/启动线程前选用 XWayland。
    // 尊重用户显式指定的 GDK_BACKEND，只调整 KDE Wayland 会话。
    #[cfg(target_os = "linux")]
    if std::env::var_os("GDK_BACKEND").is_none()
        && std::env::var("XDG_SESSION_TYPE").as_deref() == Ok("wayland")
        && std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|desktop| {
            desktop.split(':').any(|name| name.eq_ignore_ascii_case("KDE"))
        })
        && std::env::var_os("DISPLAY").is_some_and(|display| !display.is_empty())
    {
        std::env::set_var("GDK_BACKEND", "x11,wayland");
        // 共享内存传输避开 NVIDIA 的 DMA-BUF 导入问题，保留加速合成。
        if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none()
            && std::env::var_os("WEBKIT_DMABUF_RENDERER_FORCE_SHM").is_none()
        {
            std::env::set_var("WEBKIT_DMABUF_RENDERER_FORCE_SHM", "1");
        }
    }

    // 阶段 0：前置环境准备（WebView2 用户数据便携目录隔离）
    mma_shell::app::ensure_portable_profile();

    // 阶段 1：第二实例快速探针（若已有实例运行，转发参数后瞬间退出，实现零窗口闪烁）
    if mma_shell::app::try_forward_and_exit() {
        return;
    }

    // 阶段 2：启动并运行桌面壳应用生命周期
    mma_shell::app::run();
}