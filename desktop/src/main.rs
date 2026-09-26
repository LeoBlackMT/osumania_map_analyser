// mma-shell 桌面壳入口：
// 在线（tosu.env 命中且存活）→ 导航到 tosu 插件页（设置/静态零适配）；
// 离线 → 24061 本地页。窗口属性（透明/置顶/无边框）由 tauri.conf.json 配置。
//
// ---- 线程规则（改动本文件前先读）----
// * **窗口 API 一律只在 `std::thread::spawn` 的独立线程里调用**（`settings_window.rs`
//   文件头列了三条源码依据：`send_user_message` 在主线程上是**内联执行**而非排队延迟，
//   上游明言建窗/建 webview "must be called from a separate thread"，且 wry 建窗阻塞在
//   `webview2_com::wait_with_pump`）。因此 `run_on_main_thread` **不是**建窗的解法。
// * 快捷键回调、`on_window_event`、HTTP 线程只碰**原子标志与文件**（内存缓存 / 读改写
//   JSON）：不调用窗口 API，避免"回调里等自己派发的消息"自死锁。
// * 既有例外仅两处（本轮未改动）：`apply_flag_change` 与 5s 兜底 persister 仍用
//   `run_on_main_thread` 做 `set_always_on_top` / `outer_position` 等**非建窗**调用。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use mma_shell::{config, server, settings_window};
use tauri::Manager;
use tauri_plugin_global_shortcut::{
    Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState,
};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

// 窗口状态内存缓存（位置/尺寸由窗口事件维护；标志位由快捷键切换）。
static WINDOW_STATE: Mutex<config::WindowState> = Mutex::new(config::WindowState {
    x: i32::MIN,
    y: 0,
    w: 520,
    h: 680,
    topmost: true,
    click_through: false,
});

fn startup_url(tosu: &Option<config::TosuInfo>) -> String {
    let Some(info) = tosu else {
        return "http://127.0.0.1:24061/".to_string();
    };
    if !config::tosu_online(info) {
        return "http://127.0.0.1:24061/".to_string();
    }
    // 用实际插件目录名（用户可能自定义如 "ManiaMapAnalyser-PR1"）。
    let folder = config::plugin_dir()
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| config::PLUGIN_FOLDER.to_string());
    let encoded = folder.replace(' ', "%20");
    format!("{}/{}/", info.base_url(), encoded)
}

/// 把当前内存状态写盘（仅文件 IO，安全；任何线程可调）。
fn persist_window_state() {
    config::write_window_state(&WINDOW_STATE.lock().unwrap());
}

// ---- 第二实例探测（Step 4）----
//
// 必须在**任何窗口创建之前**执行：tauri.conf.json 里的窗口在 `RuntimeRunEvent::Ready`
// 时就建好了（早于 `setup` 回调），只有把探测放在 `main()` 开头才可能做到第二实例
// 零窗口闪烁（假设 1/15）。

/// 探测地址：本机壳端口（`frames::HTTP_PORT`）。这里用 `host:port` 字面量是因为
/// HTTP 请求行/Host 头需要该形式；端口值仍由 `frames::HTTP_PORT` 唯一声明。
const PROBE_ADDR: &str = "127.0.0.1:24061";

/// 既有实例探测结果。
enum ExistingInstance {
    /// 端口上是本壳（`GET /settings` → 200 + 以 `{` 开头的响应体）。
    OurShell,
    /// 端口被别的进程占用（非 200 或响应体不是 JSON 对象）。
    Foreign,
    /// 端口没人监听 → 正常启动。
    None,
}

/// 向本机壳端口发一个最小 HTTP/1.1 请求并读回全部响应（lossy UTF-8）。
/// 对端响应带 `Connection: close` 并随即关连接 ⇒ `read_to_end` 拿到 EOF；读超时也
/// 保留已读字节（分类只看状态行与响应体首字符）。
fn shell_http_request(method: &str, path: &str, extra_headers: &str) -> std::io::Result<String> {
    let addr: SocketAddr = PROBE_ADDR.parse().expect("probe addr");
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_millis(300))?;
    stream.set_read_timeout(Some(Duration::from_millis(1500)))?;
    stream.set_write_timeout(Some(Duration::from_millis(1500)))?;
    let request = format!(
        "{} {} HTTP/1.1\r\nHost: {}\r\n{}Connection: close\r\n\r\n",
        method, path, PROBE_ADDR, extra_headers
    );
    stream.write_all(request.as_bytes())?;
    stream.flush()?;
    let mut raw = Vec::new();
    let _ = stream.read_to_end(&mut raw);
    Ok(String::from_utf8_lossy(&raw).to_string())
}

/// 最多 3 次（间隔 200ms、连接超时 300ms）探测 24061 上的既有实例。
/// 连上后发 `GET /settings`：**200 且响应体以 `{` 开头** = 本壳；其余一律 `Foreign`
/// （别的 HTTP 服务、非 HTTP 进程、以及"本壳但设置不可读"的边界）。
fn probe_existing_instance() -> ExistingInstance {
    for attempt in 0..3 {
        if attempt > 0 {
            thread::sleep(Duration::from_millis(200));
        }
        let Ok(response) = shell_http_request("GET", "/settings", "") else {
            continue; // 连接/写失败 ⇒ 端口无人监听（或监听还没就绪）→ 再试一次
        };
        let (head, body) = match response.find("\r\n\r\n") {
            Some(i) => (&response[..i], &response[i + 4..]),
            None => (response.as_str(), ""),
        };
        let is_200 = head
            .lines()
            .next()
            .map(|line| line.split_whitespace().nth(1) == Some("200"))
            .unwrap_or(false);
        return if is_200 && body.trim_start().starts_with('{') {
            ExistingInstance::OurShell
        } else {
            ExistingInstance::Foreign
        };
    }
    ExistingInstance::None
}

/// 判定一次性探针开关（`=1` 才算开启；未设置/其他值 = 默认行为）。
fn probe_env_on(name: &str) -> bool {
    std::env::var(name).ok().as_deref() == Some("1")
}

/// 设置窗口的打开入口（快捷键路径）：默认只调 `settings_window::open_or_focus`；
/// 下面两个**临时探针**（Step 7 验收后连同本函数一起删除）只按环境变量分流。
fn open_settings_with_probes(app: &tauri::AppHandle, plugin_dir: &std::path::Path) {
    if probe_env_on("MMA_SETTINGS_WINDOW_INLINE") {
        probe_inline_build(app);
        return;
    }
    if probe_env_on("MMA_SETTINGS_WINDOW_FORCE_FAIL") {
        probe_force_fail_build(app);
        return;
    }
    settings_window::open_or_focus(app, plugin_dir);
}

/// **临时测试脚手架**（`MMA_SETTINGS_WINDOW_INLINE=1`，Step 7 后删除）。
///
/// 故意在**主线程**内联 `build()`（违反文件头的线程规则），把假设 2 变成可见的 A/B：
/// 主线程内联建窗会自锁死（事件循环不再泵消息）。看门狗线程 3s 后判定"主线程没回来"
/// 并记 `INLINE_BUILD_WATCHDOG: main thread did not return` + `exit(9)`（避免留下一个
/// 无响应的进程）；若 3s 内主线程已返回（本函数末尾置标志）则只记一条 warn 供 A/B 对比。
fn probe_inline_build(app: &tauri::AppHandle) {
    static INLINE_RETURNED: AtomicBool = AtomicBool::new(false);
    INLINE_RETURNED.store(false, Ordering::Relaxed);
    thread::spawn(move || {
        thread::sleep(Duration::from_secs(3));
        if INLINE_RETURNED.load(Ordering::Relaxed) {
            server::log::log_at(
                "warn",
                "INLINE probe: main thread returned from the inline build within 3s (assumption 2 NOT reproduced)",
            );
            return;
        }
        server::log::log_at("error", "INLINE_BUILD_WATCHDOG: main thread did not return");
        std::process::exit(9);
    });
    server::log::log_at(
        "error",
        "INLINE probe: building the settings window on the main thread (deliberate rule violation)",
    );
    // 用独立 label，避免与真实设置窗口（LABEL="settings"）的标志/事件分流相互干扰。
    let result = tauri::WebviewWindowBuilder::new(app, "settings-inline", settings_window::url())
        .title(settings_window::TITLE)
        .build();
    INLINE_RETURNED.store(true, Ordering::Relaxed);
    match result {
        Ok(window) => {
            server::log::log_at("warn", "INLINE probe: build() returned Ok on the main thread");
            let _ = window.close();
        }
        Err(e) => {
            server::log::log_at("error", &format!("INLINE probe: build() returned Err: {}", e));
        }
    }
}

/// **临时测试脚手架**（`MMA_SETTINGS_WINDOW_FORCE_FAIL=1`，Step 7 后删除）。
///
/// 复现 `settings_window::open_or_focus` 建窗失败分支的动作：错误日志 +
/// `restore_main_topmost`（按磁盘恢复主窗置顶）。失败用**已被占用的 label**（`main`）
/// 制造：`tauri::WindowManager::prepare_window` 在创建任何 OS 窗口之前就返回
/// `Err(WindowLabelAlreadyExists)`（`tauri-2.11.5/src/manager/window.rs:70-72`，
/// `window/mod.rs:390` 调用点在前、运行时建窗在后）——本版本 tauri 没有"空 label
/// 非法"校验，空 label 反而可能真的建出窗口，故不用它。
fn probe_force_fail_build(app: &tauri::AppHandle) {
    let app = app.clone();
    thread::spawn(move || {
        server::log::log_at(
            "error",
            "FORCE_FAIL probe: building with a duplicate label (main) to force Err",
        );
        match tauri::WebviewWindowBuilder::new(&app, "main", settings_window::url()).build() {
            Ok(window) => {
                server::log::log_at("error", "FORCE_FAIL probe: build unexpectedly SUCCEEDED");
                let _ = window.close();
            }
            Err(e) => {
                server::log::log_at(
                    "error",
                    &format!("FORCE_FAIL probe: build FAILED as designed: {}", e),
                );
                settings_window::restore_main_topmost(&app);
                server::log::log_at(
                    "info",
                    &format!("FORCE_FAIL probe: is_open()={}", settings_window::is_open()),
                );
            }
        }
    });
}

/// 位置/尺寸事件后持久化：置顶/穿透以磁盘为权威（control toggle 可能刚改过），
/// 只把磁盘上的两标志合并进内存再写盘，避免本通道旧快照覆盖对侧通道的切换。
fn merge_disk_flags_and_persist() {
    let disk = config::read_window_state();
    {
        let mut st = WINDOW_STATE.lock().unwrap();
        st.topmost = disk.topmost;
        st.click_through = disk.click_through;
    }
    persist_window_state();
}

/// 切换置顶/穿透：入参为目标值（快捷键回调已算好 next）；应用窗口 API 并写盘。
/// 写盘前从磁盘读回对侧标志——两通道（全局快捷键 / 页面 control toggle）共用
/// mma-shell-state.json 为权威，防止各自内存副本互相覆盖（曾出现 control toggle
/// 写盘后 5s 定时器用快捷键通道的旧内存值覆盖回滚）。
fn apply_flag_change(app: &tauri::AppHandle, topmost: Option<bool>, click_through: Option<bool>) {
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

fn main() {
    // ① 探测既有实例——**先于任何窗口创建**（见上方注释）：命中的两个分支都在建窗
    //    之前就退出，第二进程因此零窗口闪烁。
    let want_settings = std::env::args().any(|arg| arg == "--settings");
    // 只在 `None` 分支赋值（另两个分支 `process::exit` 发散）→ 无需初值。
    let open_settings_on_start;
    match probe_existing_instance() {
        ExistingInstance::OurShell => {
            server::log::log_line("existing mma-shell instance detected on 24061");
            if want_settings {
                // 把 --settings 转交给既有实例（端点只负责发起建/聚焦，不等窗口）。
                match shell_http_request("POST", "/open-settings", "Content-Length: 0\r\n") {
                    Ok(response) => server::log::log_line(&format!(
                        "existing instance: open-settings forwarded ({})",
                        response.lines().next().unwrap_or("(no status line)")
                    )),
                    Err(e) => server::log::log_at(
                        "error",
                        &format!("existing instance: open-settings FAILED: {}", e),
                    ),
                }
            }
            // 无论是否带 --settings：既有实例在前，本进程一个窗口都不建。
            std::process::exit(0);
        }
        ExistingInstance::Foreign => {
            server::log::log_at("error", "port 24061 is occupied by another process");
            std::process::exit(2);
        }
        ExistingInstance::None => {
            open_settings_on_start = want_settings;
        }
    }

    let plugin_dir = config::plugin_dir();
    let tosu = config::probe_tosu_env();
    // 在线判定提到 main()：启动时的骨架生成与 HTTP 层的权威链共用同一个判据，
    // 避免两个位置各探一次得出不同结论（单一在线门控）。
    let online = tosu.as_ref().map(config::tosu_online).unwrap_or(false);
    let url_text = startup_url(&tosu);
    println!("plugin dir: {}", plugin_dir.display());
    println!("startup url: {}", url_text);

    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(move |app| {
            // 插件版本从 metadata.txt 读取（不再硬编码，避免版本漂移）。
            let plugin_version = std::fs::read_to_string(plugin_dir.join("metadata.txt"))
                .ok()
                .and_then(|text| {
                    text.lines().find_map(|line| {
                        line.trim().strip_prefix("Version:").map(|v| v.trim().to_string())
                    })
                })
                .unwrap_or_else(|| "?".to_string());
            server::log::log_line(&format!(
                "mma-shell start (crate v{}, plugin v{}, ts={})",
                env!("CARGO_PKG_VERSION"),
                plugin_version,
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0)
            ));
            // 启动时生成 mma-shell-config.json 骨架（无 tosu 用户可发现并编辑）。
            config::ensure_shell_config();
            // 仅离线且无本地设置文件时生成 mma-settings.json 骨架（全量插件设置）。
            config::ensure_plugin_settings(&tosu, online);

            let shared = server::start(plugin_dir.clone(), tosu);
            // App 句柄进 Shared：`server::start` 内 24061 已经在服务请求（`/open-settings`
            // 需要它），所以必须在拿到 shared 后**立即**注入，先于 `set_main_window`。
            server::set_app_handle(&shared, app.handle().clone());
            if let Some(window) = app.get_webview_window("main") {
                server::set_main_window(&shared, window.clone());
                // 恢复记忆的窗口状态（位置/尺寸/置顶/穿透）——setup 内同步调用安全。
                // 注意：存取均为 physical 坐标（outer_position/outer_size），
                // 恢复必须用 Physical* 构造，否则与 logical 混用会在高 DPI
                // 下逐次漂移放大。
                let saved = config::read_window_state();
                *WINDOW_STATE.lock().unwrap() = saved;
                if saved.x != i32::MIN {
                    let _ = window.set_position(tauri::PhysicalPosition::new(saved.x, saved.y));
                }
                let _ = window.set_size(tauri::PhysicalSize::new(saved.w, saved.h));
                let _ = window.set_always_on_top(saved.topmost);
                if saved.click_through {
                    let _ = window.set_ignore_cursor_events(true);
                }
                let url = url_text.parse().expect("invalid startup url");
                let _ = window.navigate(url);
            }

            // `--settings`：单实例启动时直接打开设置窗口（双实例已在 main() 开头退出）。
            // 此处是 setup 内同步调用，`open_or_focus` 自身只置标志 + 起独立线程建窗。
            if open_settings_on_start {
                settings_window::open_or_focus(app.handle(), &plugin_dir);
            }

            // 全局快捷键（页面焦点/点击穿透无关）。回调只改内存+异步应用窗口 API，
            // 绝不在回调内同步调用窗口方法（线程规则见文件头注）。
            // 默认键位：Ctrl+Shift+T 置顶 / Ctrl+Shift+C 穿透 / Ctrl+Shift+S 设置 /
            // Ctrl+Q 关闭；可在 mma-shell-config.json 的 hotkeys 配置（"Ctrl+Shift+T" 等字符串）。
            // 解析配置键位（失败回落默认）。
            let cfg = config::read_shell_config();
            let hot = |k: &str, def: &str| {
                cfg.get("hotkeys")
                    .and_then(|h| h.get(k))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| def.to_string())
            };
            let h_top = hot("topmost", "Ctrl+Shift+T");
            let h_click = hot("clickThrough", "Ctrl+Shift+C");
            let h_settings = hot("settings", "Ctrl+Shift+S");
            let h_close = hot("close", "Ctrl+Q");
            let parse_hot = |s: &str| -> Option<Shortcut> {
                // 简单解析："Ctrl+Shift+T" / "Alt+C" / "Ctrl+Q"（仅单键+修饰符组合）。
                let mut mods = Modifiers::empty();
                let mut key: Option<Code> = None;
                for part in s.split('+') {
                    match part.trim() {
                        "Ctrl" | "CTRL" | "ctrl" => mods |= Modifiers::CONTROL,
                        "Shift" | "SHIFT" | "shift" => mods |= Modifiers::SHIFT,
                        "Alt" | "ALT" | "alt" => mods |= Modifiers::ALT,
                        "Super" | "Win" | "Meta" | "CMD" => mods |= Modifiers::SUPER,
                        other => {
                            key = match other {
                                "T" => Some(Code::KeyT),
                                "C" => Some(Code::KeyC),
                                "Q" => Some(Code::KeyQ),
                                "A" => Some(Code::KeyA),
                                "B" => Some(Code::KeyB),
                                "D" => Some(Code::KeyD),
                                "E" => Some(Code::KeyE),
                                "F" => Some(Code::KeyF),
                                "G" => Some(Code::KeyG),
                                "H" => Some(Code::KeyH),
                                "I" => Some(Code::KeyI),
                                "J" => Some(Code::KeyJ),
                                "K" => Some(Code::KeyK),
                                "L" => Some(Code::KeyL),
                                "M" => Some(Code::KeyM),
                                "N" => Some(Code::KeyN),
                                "O" => Some(Code::KeyO),
                                "P" => Some(Code::KeyP),
                                "R" => Some(Code::KeyR),
                                "S" => Some(Code::KeyS),
                                "U" => Some(Code::KeyU),
                                "V" => Some(Code::KeyV),
                                "W" => Some(Code::KeyW),
                                "X" => Some(Code::KeyX),
                                "Y" => Some(Code::KeyY),
                                "Z" => Some(Code::KeyZ),
                                _ => None,
                            };
                        }
                    }
                }
                key.map(|k| Shortcut::new(if mods.is_empty() { None } else { Some(mods) }, k))
            };
            if let Some(sc) = parse_hot(&h_top) {
                match app.global_shortcut().on_shortcut(
                    sc,
                    move |app, _shortcut, event| {
                        if event.state() == ShortcutState::Pressed {
                            // 设置窗口打开期间置顶被取消 → 两个开关回调一律失效
                            // （不写盘、不变更视觉状态），避免把设置窗口盖住。
                            if settings_window::is_open() {
                                server::log::log_at(
                                    "debug",
                                    "shortcut ignored: settings window is open (topmost)",
                                );
                                return;
                            }
                            server::log::log_at("debug", "shortcut pressed: topmost");
                            let next = !config::read_window_state().topmost;
                            apply_flag_change(app, Some(next), None);
                        }
                    },
                ) {
                    Ok(_) => server::log::log_line(&format!("shortcut registered: topmost={}", h_top)),
                    Err(e) => server::log::log_at("error", &format!("shortcut FAILED topmost={}: {}", h_top, e)),
                }
            }
            if let Some(sc) = parse_hot(&h_click) {
                match app.global_shortcut().on_shortcut(
                    sc,
                    move |app, _shortcut, event| {
                        if event.state() == ShortcutState::Pressed {
                            if settings_window::is_open() {
                                server::log::log_at(
                                    "debug",
                                    "shortcut ignored: settings window is open (clickThrough)",
                                );
                                return;
                            }
                            server::log::log_at("debug", "shortcut pressed: clickThrough");
                            let next = !config::read_window_state().click_through;
                            apply_flag_change(app, None, Some(next));
                        }
                    },
                ) {
                    Ok(_) => server::log::log_line(&format!("shortcut registered: clickThrough={}", h_click)),
                    Err(e) => server::log::log_at("error", &format!("shortcut FAILED clickThrough={}: {}", h_click, e)),
                }
            }
            if let Some(sc) = parse_hot(&h_settings) {
                match app.global_shortcut().on_shortcut(
                    sc,
                    move |app, _shortcut, event| {
                        if event.state() == ShortcutState::Pressed {
                            server::log::log_at("debug", "shortcut pressed: settings");
                            // 回调内只做标志/文件与"起独立线程建窗"（见文件头线程规则）。
                            open_settings_with_probes(app, &plugin_dir);
                        }
                    },
                ) {
                    Ok(_) => server::log::log_line(&format!("shortcut registered: settings={}", h_settings)),
                    Err(e) => server::log::log_at("error", &format!("shortcut FAILED settings={}: {}", h_settings, e)),
                }
            }
            if let Some(sc) = parse_hot(&h_close) {
                match app.global_shortcut().on_shortcut(
                    sc,
                    move |app, _shortcut, event| {
                        if event.state() == ShortcutState::Pressed {
                            server::log::log_at("debug", "shortcut pressed: close");
                            let app2 = app.clone();
                            let _ = app.run_on_main_thread(move || {
                                persist_window_state();
                                if let Some(window) = app2.get_webview_window("main") {
                                    let _ = window.close();
                                }
                            });
                        }
                    },
                ) {
                    Ok(_) => server::log::log_line(&format!("shortcut registered: close={}", h_close)),
                    Err(e) => server::log::log_at("error", &format!("shortcut FAILED close={}: {}", h_close, e)),
                }
            }
            // 窗口状态兜底：主线程每 5s 查询一次位置/尺寸并写盘（部分透明窗口
            // Moved/Resized 事件可能不触发；查询必须经 run_on_main_thread）。
            {
                let handle = app.handle().clone();
                thread::spawn(move || loop {
                    thread::sleep(std::time::Duration::from_secs(5));
                    let handle2 = handle.clone();
                    let _ = handle.run_on_main_thread(move || {
                        let Some(window) = handle2.get_webview_window("main") else {
                            return;
                        };
                        if let Ok(position) = window.outer_position() {
                            WINDOW_STATE.lock().unwrap().x = position.x;
                            WINDOW_STATE.lock().unwrap().y = position.y;
                        }
                        if let Ok(size) = window.outer_size() {
                            WINDOW_STATE.lock().unwrap().w = size.width;
                            WINDOW_STATE.lock().unwrap().h = size.height;
                        }
                        // 置顶/穿透以磁盘为权威（control toggle / 快捷键都可能改），
                        // 仅刷新位置尺寸，避免用本线程的旧快照覆盖对侧通道的切换。
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

            Ok(())
        })
        .on_window_event(|window, event| {
            // 事件回调在主线程，且**每个窗口都会进来**（设置窗口也走这里）→ 必须按
            // label 分流。回调内只碰原子标志与文件（内存缓存 / 读改写 JSON），
            // 绝不调用窗口 API（线程规则见文件头注）。
            match window.label() {
                "main" => match event {
                    tauri::WindowEvent::Moved(position) => {
                        WINDOW_STATE.lock().unwrap().x = position.x;
                        WINDOW_STATE.lock().unwrap().y = position.y;
                        merge_disk_flags_and_persist();
                    }
                    tauri::WindowEvent::Resized(size) => {
                        WINDOW_STATE.lock().unwrap().w = size.width;
                        WINDOW_STATE.lock().unwrap().h = size.height;
                        merge_disk_flags_and_persist();
                    }
                    tauri::WindowEvent::CloseRequested { .. } => {
                        merge_disk_flags_and_persist();
                        // 主窗关闭 ⇒ 连带关闭设置窗口（子窗不该独自存活）。
                        settings_window::close(window.app_handle());
                    }
                    _ => {}
                },
                // 设置窗口：几何写**独立文件** mma-shell-settings-window.json。
                // 事件载荷是 **Physical** 坐标/尺寸（tauri-runtime-2.11.3/src/window.rs:32,34），
                // 与 settings_window.rs 的 Physical* 建窗参数同一约定：**原样存储**，
                // 绝不做 logical/DPI 换算。
                settings_window::LABEL => match event {
                    tauri::WindowEvent::Moved(position) => {
                        let mut state = config::read_settings_window_state();
                        state.x = position.x;
                        state.y = position.y;
                        config::write_settings_window_state(&state);
                    }
                    tauri::WindowEvent::Resized(size) => {
                        let mut state = config::read_settings_window_state();
                        state.w = size.width;
                        state.h = size.height;
                        config::write_settings_window_state(&state);
                    }
                    // 关闭与销毁都要复位标志并恢复主窗置顶（两条路径都可能先到）。
                    tauri::WindowEvent::CloseRequested { .. } | tauri::WindowEvent::Destroyed => {
                        settings_window::on_window_closed(window.app_handle());
                    }
                    _ => {}
                },
                _ => {}
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}