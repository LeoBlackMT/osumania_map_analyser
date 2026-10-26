// app::window_state - 窗口几何状态内存缓存与双向磁盘持久化

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};
use tauri::Manager;
use crate::config;

// 窗口状态内存缓存（位置/尺寸由窗口事件维护；标志位由快捷键切换）。
pub static WINDOW_STATE: Mutex<config::WindowState> = Mutex::new(config::WindowState {
    x: i32::MIN,
    y: 0,
    w: 520,
    h: 680,
    topmost: true,
    click_through: false,
    settings: None,
    install_id: None,
});

static DIRTY: AtomicBool = AtomicBool::new(false);
static LAST_CHANGE: Mutex<Option<Instant>> = Mutex::new(None);

/// 把当前内存状态写盘（仅文件 IO，安全；任何线程可调）。
pub fn persist_window_state() {
    let mut state = WINDOW_STATE.lock().unwrap().clone();
    state.sanitize();
    config::write_window_state(&state);
}

/// 标记几何发生变动（防抖写盘：避免拖拽/缩放时每秒几十次高频磁盘 IO）。
pub fn mark_geometry_dirty() {
    *LAST_CHANGE.lock().unwrap() = Some(Instant::now());
    DIRTY.store(true, Ordering::Release);
}

/// 立即将未写入的变动刷新落盘（退出或关键操作时调用）。
pub fn flush_window_state() {
    if DIRTY.swap(false, Ordering::AcqRel) {
        persist_window_state();
    }
}

/// 位置/尺寸事件后持久化：置顶/穿透以磁盘为权威（control toggle 可能刚改过）。
pub fn merge_disk_flags_and_persist() {
    let disk = config::read_window_state();
    {
        let mut st = WINDOW_STATE.lock().unwrap();
        st.topmost = disk.topmost;
        st.click_through = disk.click_through;
    }
    // 标记变动走防抖落盘，避免高频 I/O 轰炸
    mark_geometry_dirty();
}

/// 切换置顶/穿透：入参为目标值；应用窗口 API 并立即写盘。
pub fn apply_flag_change(app: &tauri::AppHandle, topmost: Option<bool>, click_through: Option<bool>) {
    {
        let mut st = WINDOW_STATE.lock().unwrap();
        let disk = config::read_window_state();
        st.topmost = topmost.unwrap_or(disk.topmost);
        st.click_through = click_through.unwrap_or(disk.click_through);
    }
    persist_window_state();
    let app2 = app.clone();
    let _ = app.run_on_main_thread(move || {
        let Some(window) = app2.get_webview_window("main") else {
            return;
        };
        if let Some(v) = topmost {
            let _ = window.set_always_on_top(v);
        }
        if let Some(v) = click_through {
            let _ = window.set_ignore_cursor_events(v);
        }
    });
}

/// 窗口状态兜底看门狗：
/// 1. 主线程每 5s 查询一次位置/尺寸并写盘（带最小化与非法值过滤）。
/// 2. 同时承担防抖定时器职责（拖拽停止 500ms 后自动落盘）。
pub fn spawn_window_state_watchdog(app: &tauri::AppHandle) {
    let handle = app.clone();
    thread::spawn(move || loop {
        thread::sleep(Duration::from_millis(250));

        // ① 检查防抖队列：如果自上次变动已静止超过 500ms，则执行落盘
        let should_flush = {
            if DIRTY.load(Ordering::Acquire) {
                let last = *LAST_CHANGE.lock().unwrap();
                last.map(|t| t.elapsed() >= Duration::from_millis(500))
                    .unwrap_or(true)
            } else {
                false
            }
        };
        if should_flush {
            flush_window_state();
        }

        // ② 每 5s 周期性采样并维护置顶/穿透磁盘状态（用递增计数器实现 20 次 250ms = 5s）
        thread_local! {
            static TICK: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
        }
        let tick = TICK.with(|t| {
            let next = (t.get() + 1) % 20;
            t.set(next);
            next
        });
        if tick != 0 {
            continue;
        }

        let handle2 = handle.clone();
        let _ = handle.run_on_main_thread(move || {
            let Some(window) = handle2.get_webview_window("main") else {
                return;
            };
            // 关键守卫：最小化时绝对不采样，避免 Win+D 负坐标与零尺寸篡改状态！
            if window.is_minimized().unwrap_or(false) {
                return;
            }
            if let Ok(position) = window.outer_position() {
                if config::is_valid_position(position.x, position.y) {
                    WINDOW_STATE.lock().unwrap().x = position.x;
                    WINDOW_STATE.lock().unwrap().y = position.y;
                }
            }
            // 关键守卫：统一使用 inner_size，与 window.set_size (映射为 set_inner_size)
            // 和 WindowEvent::Resized 保持口径一致，杜绝重启尺寸膨胀循环！
            if let Ok(size) = window.inner_size() {
                if config::is_valid_main_window_size(size.width, size.height) {
                    WINDOW_STATE.lock().unwrap().w = size.width;
                    WINDOW_STATE.lock().unwrap().h = size.height;
                }
            }
            let disk = config::read_window_state();
            {
                let mut st = WINDOW_STATE.lock().unwrap();
                st.topmost = disk.topmost;
                st.click_through = disk.click_through;
            }
            persist_window_state();
        });
    });
}
