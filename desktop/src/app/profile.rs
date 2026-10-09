// app::profile - WebView2 用户数据便携目录配置

/// 把 WebView2 的用户数据目录放到 exe 旁（与 `mma-shell-*.json` / `logs/` 同一约定）。
///
/// WebView2 默认用 `%LOCALAPPDATA%\<identifier>`；该目录在部分机器上会被拒绝访问
/// （实测：`Failed to setup app: 拒绝访问。 (os error 5)` → 建窗失败 → 进程无窗口退出）。
/// exe 旁既保证可写、又与壳的其它状态文件一致；用户显式设置
/// `WEBVIEW2_USER_DATA_FOLDER` 时完全尊重之；exe 目录不可写（如 Program Files）时
/// 不设置该变量，退回 WebView2 默认行为。
pub fn ensure_portable_profile() {
    if std::env::var_os("WEBVIEW2_USER_DATA_FOLDER").is_some() {
        return;
    }
    let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
    else {
        return;
    };
    let profile = dir.join("webview2-profile");
    if std::fs::create_dir_all(&profile).is_err() {
        return;
    }
    std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", &profile);
}
