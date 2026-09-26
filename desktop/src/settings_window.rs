// 设置窗口（壳内第二个 OS 窗口，承载壳本机 HTTP 端口上的 settings.html）。
//
// ---- 线程规则（必须遵守；改动本文件前先读）----
// 除原子标志与文件 I/O 外，**一切** Tauri 窗口 API（`get_webview_window` / `show` /
// `unminimize` / `set_focus` / `build` / `close` / `set_always_on_top`）只在
// `std::thread::spawn` 的闭包内调用；快捷键回调、`on_window_event`、HTTP 线程只碰
// 原子标志与文件。理由（三条均已按源码核实）：
//   * `tauri-runtime-wry-2.11.4/src/lib.rs:235-255`：`send_user_message` 在
//     `current_thread().id() == context.main_thread_id` 时**直接内联**调用
//     `handle_user_message`——主线程调用不是"排队延迟"，故 `run_on_main_thread`
//     也不是解法；
//   * `tauri-runtime-wry-2.11.4/src/lib.rs:2757-2768`（上游注释，`create_window` /
//     `create_webview`）："must be called from a separate thread, otherwise the
//     channel will introduce a deadlock"；
//   * `wry-0.55.1/src/webview2/mod.rs:367-415`：WebView2 控制器的创建阻塞在
//     `webview2_com::wait_with_pump(rx)?`（等异步回调 + 泵消息）。
// 三条相加：主线程里 `build()` 是自锁死（主线程正等自己派发/泵的消息）；独立线程里
// 调用则请求经 event loop proxy 派发、主线程继续跑事件循环，等待才会返回。
//
// ---- 几何约定 ----
// 设置窗口几何（`mma-shell-settings-window.json` 的 x/y/w/h）来自 `Moved`/`Resized` 的
// **physical** 载荷（px）；恢复必须用 `tauri::PhysicalPosition` / `tauri::PhysicalSize`
// 构造——builder 的 `position`/`inner_size` 是 **logical**，混用会在高 DPI 下逐次漂移
// （同 `main.rs:126-128` 主窗的既有约定）。

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

/// 设置窗口的 Tauri label（`main.rs` 的 `on_window_event` 按它分流）。
pub const LABEL: &str = "settings";
/// 设置窗口标题。
pub const TITLE: &str = "ManiaMapAnalyser — Settings";

/// 窗口是否打开（回调/HTTP 线程只读写这个原子标志，不碰窗口 API）。
static SETTINGS_OPEN: AtomicBool = AtomicBool::new(false);
/// 建窗串行化锁：同一时刻只允许一个线程进入"查存在 → 建窗"段。
static CREATE_LOCK: Mutex<()> = Mutex::new(());

/// 设置页 URL（与端点同源的本机端口，无需 CORS）。
pub fn url() -> tauri::WebviewUrl {
    WebviewUrl::External(
        tauri::Url::parse(&format!(
            "http://127.0.0.1:{}/settings.html",
            crate::frames::HTTP_PORT
        ))
        .expect("settings url"),
    )
}

/// 窗口是否已打开（仅读原子标志；任何线程可调）。
pub fn is_open() -> bool {
    SETTINGS_OPEN.load(Ordering::Relaxed)
}

/// 打开或聚焦设置窗口（幂等；快捷键 / `POST /open-settings` / `--settings` 共用）。
///
/// 本函数自身只做"文件检查 + 置标志 + 起线程"，不直接调用任何窗口 API。
pub fn open_or_focus(app: &AppHandle, plugin_dir: &Path) {
    // ① 页面不存在 → 只记警告并返回（**不置标志**，避免"显示已开"却没有窗口）。
    let page = plugin_dir.join("settings.html");
    if !page.exists() {
        crate::server::log::log_at(
            "warn",
            &format!("settings window skipped: {} missing", page.display()),
        );
        return;
    }
    // ② 先置标志（HTTP/快捷键线程据此判定"设置窗口在前"），再异步建/聚焦窗口。
    SETTINGS_OPEN.store(true, Ordering::Relaxed);
    let app = app.clone();
    std::thread::spawn(move || {
        let _guard = CREATE_LOCK.lock().unwrap();
        // 以下全部是窗口 API——只能在这个独立线程里调用（见文件头线程规则）。
        if let Some(window) = app.get_webview_window(LABEL) {
            let _ = window.show();
            let _ = window.unminimize();
            let _ = window.set_focus();
            return;
        }
        // 几何约定：磁盘上的 x/y/w/h 来自 Moved/Resized 的 **physical** 载荷，恢复必须
        // 用 Physical* 构造（builder 的 position/inner_size 是 logical，混用会在高 DPI
        // 下逐次漂移；同 main.rs:126-128 的既有约定）。
        let state = crate::config::read_settings_window_state();
        let fresh = state.x == i32::MIN;
        let mut builder = WebviewWindowBuilder::new(&app, LABEL, url())
            .title(TITLE)
            .min_inner_size(720.0, 480.0)
            .resizable(true)
            .decorations(true)
            .always_on_top(false)
            .skip_taskbar(false);
        if fresh {
            // 首次打开（无记忆几何）：默认 logical 尺寸 + 居中。
            builder = builder.inner_size(state.w as f64, state.h as f64).center();
        }
        match builder.build() {
            Ok(window) => {
                if !fresh {
                    // 记忆几何是 physical → 用 Physical* 恢复（见文件头几何约定）。
                    let _ = window.set_position(tauri::PhysicalPosition::new(state.x, state.y));
                    let _ = window.set_size(tauri::PhysicalSize::new(state.w, state.h));
                }
                crate::server::log::log_line("settings window opened");
                // 设置窗口在前期间取消主窗置顶（不写盘；关闭/失败时按磁盘恢复）。
                if let Some(main) = app.get_webview_window("main") {
                    let _ = main.set_always_on_top(false);
                }
            }
            Err(e) => {
                // 自愈：复位标志 + 按磁盘把主窗置顶恢复，否则主窗会永久失去置顶。
                SETTINGS_OPEN.store(false, Ordering::Relaxed);
                crate::server::log::log_at("error", &format!("settings window build FAILED: {e}"));
                restore_main_topmost(&app);
            }
        }
    });
}

/// 关闭设置窗口（幂等；只负责发起关闭，窗口的 `CloseRequested`/`Destroyed`
/// 由 `main.rs` 的事件分流复位标志）。
pub fn close(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        if let Some(window) = app.get_webview_window(LABEL) {
            let _ = window.close();
        }
    });
}

/// 按磁盘状态恢复主窗置顶（设置窗口关闭/建窗失败后调用）。
pub fn restore_main_topmost(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        if let Some(main) = app.get_webview_window("main") {
            let _ = main.set_always_on_top(crate::config::read_window_state().topmost);
        }
    });
}

/// 设置窗口已关闭/已销毁：复位标志并按磁盘恢复主窗置顶。
/// 事件回调内调用（只做原子标志 + 起线程；窗口 API 在独立线程里执行）。
pub fn on_window_closed(app: &AppHandle) {
    SETTINGS_OPEN.store(false, Ordering::Relaxed);
    restore_main_topmost(app);
}
