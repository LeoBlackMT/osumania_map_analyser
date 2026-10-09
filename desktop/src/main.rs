// mma-shell 桌面壳入口二进制
// 保持极限精炼（≤ 40 行），生命周期编排与窗口事件均已模块化至 `mma_shell::app`。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // 阶段 0：前置环境准备（WebView2 用户数据便携目录隔离）
    mma_shell::app::ensure_portable_profile();

    // 阶段 1：第二实例快速探针（若已有实例运行，转发参数后瞬间退出，实现零窗口闪烁）
    if mma_shell::app::try_forward_and_exit() {
        return;
    }

    // 阶段 2：启动并运行桌面壳应用生命周期
    mma_shell::app::run();
}