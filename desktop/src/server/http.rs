// 24061 单 listener：HTTP（静态 /settings /cover）+ WS 分发。
// 逻辑从原 server.rs 拆分；帧循环在 ws.rs。

use crate::config;
use std::sync::Arc;
use crate::frames::{HTTP_PORT, MAX_PAYLOAD_BYTES};
use crate::server::{
    apply_tosu_online_transition, broadcast, malody4_effective_root, malody_root, mime_for,
    percent_decode, Shared, ws,
};
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;

pub fn spawn_http_ws(shared: Arc<Shared>, listener: TcpListener) {
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let shared = shared.clone();
            thread::spawn(move || {
                let mut stream = stream;
                // 先 peek 探测 WS 升级：peek 不消费数据，HTTP 路径随后完整读。
                let is_ws = probe_is_ws(&stream);
                if is_ws {
                    ws::handle_ws(shared, stream);
                } else if let Some((head, body)) = read_request(&mut stream) {
                    handle_http(shared, stream, &head, &body);
                }
            });
        }
    });
}

/// peek 前 1KB 判断是否 WS 升级请求（peek 不消费；accept_hdr 需要原文在流中）。
pub(crate) fn probe_is_ws(stream: &TcpStream) -> bool {
    let mut probe = [0u8; 1024];
    for _ in 0..250 {
        match stream.peek(&mut probe) {
            Ok(0) => std::thread::sleep(Duration::from_millis(20)),
            Ok(n) => {
                return String::from_utf8_lossy(&probe[..n])
                    .to_ascii_lowercase()
                    .contains("upgrade: websocket");
            }
            Err(_) => return false,
        }
    }
    false
}

/// 读完整请求（head + body）：先读至 \r\n\r\n，再按 Content-Length 精确读 body。
pub fn read_request(stream: &mut TcpStream) -> Option<(String, String)> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 8192];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        if buf.len() > 64 * 1024 {
            return None;
        }
        let n = stream.read(&mut tmp).ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&tmp[..n]);
    }
    let head_end = buf.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let content_length = parse_content_length(&head);
    let mut body = String::from_utf8_lossy(&buf[head_end..]).to_string();
    while body.len() < content_length && body.len() <= MAX_PAYLOAD_BYTES {
        let n = stream.read(&mut tmp).ok()?;
        if n == 0 {
            break;
        }
        body.push_str(&String::from_utf8_lossy(&tmp[..n]));
    }
    Some((head, body))
}

fn parse_content_length(head: &str) -> usize {
    head.lines()
        .find(|l| l.to_ascii_lowercase().starts_with("content-length:"))
        .and_then(|l| l.split(':').nth(1))
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(0)
}

pub fn write_response(stream: &mut TcpStream, code: u16, ctype: &str, body: &[u8]) {
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        code,
        status_text(code),
        ctype,
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
}

pub fn respond_json(stream: &mut TcpStream, code: u16, body: &str) {
    write_response(stream, code, "application/json", body.as_bytes());
}

/// 壳页面允许的来源：壳自己的 24061 与 tosu 页面 24050（tosu 在线/`osuTransport:"tosu"` 时壳加载 tosu 页面）。
/// `/feedback` 与 `GET /shell-config` 共用这份白名单。
const SHELL_PAGE_ORIGINS: &[&str] = &[
    "http://127.0.0.1:24061",
    "http://localhost:24061",
    "http://127.0.0.1:24050",
    "http://localhost:24050",
    "http://tauri.localhost",
];

fn header_value<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    head.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        if key.trim().eq_ignore_ascii_case(name) {
            Some(value.trim())
        } else {
            None
        }
    })
}

fn allowed_origin(head: &str) -> Option<String> {
    let origin = header_value(head, "origin")?;
    SHELL_PAGE_ORIGINS
        .iter()
        .find(|allowed| allowed.eq_ignore_ascii_case(origin))
        .map(|_| origin.to_string())
}

fn respond_json_cors(stream: &mut TcpStream, code: u16, body: &str, origin: Option<&str>) {
    let cors = origin
        .map(|value| format!("Access-Control-Allow-Origin: {value}\r\nVary: Origin\r\n"))
        .unwrap_or_default();
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{}Connection: close\r\n\r\n",
        code,
        status_text(code),
        body.len(),
        cors
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body.as_bytes());
}

fn plugin_version(plugin_dir: &std::path::Path) -> String {
    static VERSION: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    VERSION
        .get_or_init(|| {
            std::fs::read_to_string(plugin_dir.join("metadata.txt"))
                .ok()
                .and_then(|text| {
                    text.lines().find_map(|line| {
                        line.trim()
                            .strip_prefix("Version:")
                            .map(|value| value.trim().to_string())
                    })
                })
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| "unknown".to_string())
        })
        .clone()
}

/// POST /feedback：从 native 快照重算客观字段并签名；不直接发后端。
fn handle_feedback(shared: &Arc<Shared>, mut stream: TcpStream, body: &str, origin: Option<String>) {
    if body.len() > 4096 {
        respond_json_cors(&mut stream, 413, r#"{"error":"body_too_large"}"#, origin.as_deref());
        return;
    }
    let page: crate::feedback::PageFeedback = match serde_json::from_str(body) {
        Ok(value) => value,
        Err(_) => {
            respond_json_cors(&mut stream, 400, r#"{"error":"bad_json"}"#, origin.as_deref());
            return;
        }
    };
    let Some(reader) = crate::osu::instance() else {
        respond_json_cors(&mut stream, 409, r#"{"error":"no_reader"}"#, origin.as_deref());
        return;
    };
    let Some(snapshot) = reader.snapshot() else {
        respond_json_cors(&mut stream, 409, r#"{"error":"no_snapshot"}"#, origin.as_deref());
        return;
    };
    let Some(install_id) = crate::feedback::install_id() else {
        respond_json_cors(&mut stream, 500, r#"{"error":"install_id"}"#, origin.as_deref());
        return;
    };
    let version = plugin_version(&shared.plugin_dir);
    let payload = match crate::feedback::build_payload(&snapshot, &page, &version, &install_id) {
        Ok(value) => value,
        Err(reason) => {
            let body = serde_json::json!({ "error": reason }).to_string();
            respond_json_cors(&mut stream, 422, &body, origin.as_deref());
            return;
        }
    };
    let Some(envelope) = crate::feedback::sign(&payload) else {
        respond_json_cors(&mut stream, 500, r#"{"error":"sign_failed"}"#, origin.as_deref());
        return;
    };
    let body = serde_json::to_string(&envelope).unwrap_or_else(|_| "{}".to_string());
    respond_json_cors(&mut stream, 200, &body, origin.as_deref());
}

fn status_text(code: u16) -> &'static str {
    match code {
        200 => "OK",
        202 => "Accepted",
        204 => "No Content",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        413 => "Payload Too Large",
        415 => "Unsupported Media Type",
        422 => "Unprocessable Entity",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "Unknown",
    }
}

/// Host 头仅允许本机 `{host}:{port}`（`127.0.0.1` / `localhost` / `[::1]`）。
///
/// 端口按参数传入：本函数同时服务 24061（静态/设置）与 17653（Malody 选曲桥），
/// 两者各自只有一个 listener，端口是唯一的差异。
pub(crate) fn is_local_host(head: &str, port: u16) -> bool {
    let Some(host) = head.lines().find_map(|l| {
        let lower = l.to_ascii_lowercase();
        if lower.starts_with("host:") {
            Some(l[5..].trim().to_ascii_lowercase())
        } else {
            None
        }
    }) else {
        return true;
    };
    let allowed = [
        format!("127.0.0.1:{}", port),
        format!("localhost:{}", port),
        format!("[::1]:{}", port),
    ];
    allowed.iter().any(|a| a == &host)
}

/// 本地运行期 offsets 缓存根目录，与 `remote_offsets_dir` 的 base_dir 规则一致。
fn local_offsets_cache_root() -> Option<std::path::PathBuf> {
    if let Ok(appdata) = std::env::var("APPDATA") {
        Some(std::path::PathBuf::from(appdata).join("ManiaMapAnalyser").join("offsets"))
    } else if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        Some(std::path::PathBuf::from(xdg).join("ManiaMapAnalyser").join("offsets"))
    } else {
        std::env::var("HOME").ok().map(|home| {
            std::path::PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("ManiaMapAnalyser")
                .join("offsets")
        })
    }
}

/// 当前可用的最新 lazer 表（APPDATA 缓存 / MMA_OFFSETS_DIR / 随包目录）。
///
/// 状态页只需要展示“当前最可能被加载的表”，不依赖正在运行的 osu! 进程；
/// 真正的版本匹配仍由 `osu::lazer::table::load_table` 在读取时完成。
fn latest_lazer_table() -> (crate::osu::offsets::OffsetTable, String) {
    let mut best = crate::osu::offsets::default_lazer_table();
    let mut best_source = "embedded".to_string();

    let mut dirs: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(dir) = std::env::var("MMA_OFFSETS_DIR") {
        dirs.push(std::path::PathBuf::from(dir).join("lazer"));
    }
    if let Some(root) = local_offsets_cache_root() {
        dirs.push(root.join("lazer"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            dirs.push(parent.join("offsets").join("lazer"));
            dirs.push(parent.join("offset").join("lazer"));
        }
    }

    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            let Ok(table) = crate::osu::offsets::OffsetTable::load(&bytes) else {
                continue;
            };
            let candidate_newer = if table.revision() > 0 && best.revision() > 0 {
                table.revision() > best.revision()
            } else {
                crate::osu::offsets::version_is_newer(
                    &table.lazer_version,
                    &best.lazer_version,
                )
            };
            if candidate_newer {
                best = table;
                best_source = path.display().to_string();
            }
        }
    }

    (best, best_source)
}

fn stable_table_source(variant: &str) -> String {
    let is_ppysb = variant.eq_ignore_ascii_case("ppysb");
    let filename = if is_ppysb {
        "ppysb__x86.json"
    } else {
        "stable__x86.json"
    };
    let folders: &[&str] = if is_ppysb {
        &["stable", "ppysb"]
    } else {
        &["stable"]
    };
    let explicit_var = if is_ppysb {
        "MMA_PPYSB_OFFSETS"
    } else {
        "MMA_STABLE_OFFSETS"
    };
    if let Ok(explicit) = std::env::var(explicit_var) {
        if std::path::Path::new(&explicit).is_file() {
            return explicit;
        }
    }

    let check = |base: std::path::PathBuf| -> Option<String> {
        for folder in folders {
            let path = base.join(folder).join(filename);
            if path.is_file() {
                return Some(path.display().to_string());
            }
        }
        None
    };

    if let Ok(dir) = std::env::var("MMA_OFFSETS_DIR") {
        if let Some(path) = check(std::path::PathBuf::from(dir)) {
            return path;
        }
    }
    if let Some(root) = local_offsets_cache_root() {
        if let Some(path) = check(root) {
            return path;
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            if let Some(path) = check(parent.join("offsets")) {
                return path;
            }
        }
    }
    "embedded".to_string()
}

fn handle_http(shared: Arc<Shared>, mut stream: TcpStream, head: &str, body: &str) {
    // Host 头校验：仅接受本机 24061（DNS rebinding 防护——rebind 后的恶意页
    // Host 为攻击者域名，直接 403）。无 Host 头（HTTP/1.0 裸客户端）放行。
    if !is_local_host(head, HTTP_PORT) {
        respond_json(&mut stream, 403, r#"{"error":"forbidden host"}"#);
        return;
    }
    let mut lines = head.lines();
    let request_line = lines.next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("GET").to_string();
    let url = parts.next().unwrap_or("/");
    let path = url.split('?').next().unwrap_or("").to_string();

    // POST /settings：离线可写（请求键优先的读-改-写）；在线只读。三点硬性规定见 §11-Q3。
    if method == "POST" && path == "/settings" {
        // ① 在线复探 + 单一判据：tosu 存在时先复探存活（30s 定时器粒度太粗，见假设 5）；
        //    跳变必须经 `apply_tosu_online_transition` —— 它是"来源切换 + 全量推送"的
        //    **唯一**入口，直接写标志会吃掉切换边沿、跳过全量推送。
        //    403 的条件与 `config::resolve_plugin_settings` 第 1 级**逐字相同**：任一侧
        //    单独改动都会造出"可写但读不回"的窗口。
        if let Some(info) = shared.tosu.as_ref() {
            let alive = config::tosu_online(info);
            let cached = *shared.tosu_online.lock().unwrap();
            if alive != cached {
                apply_tosu_online_transition(&shared, alive);
            }
        }
        if shared.tosu.is_some() && *shared.tosu_online.lock().unwrap() {
            respond_json(&mut stream, 403, r#"{"error":"tosu online: settings are read-only"}"#);
            return;
        }
        // ② 写盘 = 请求键优先的读-改-写：页面发**全量 object**，壳只据此合并——
        //    未知键保留，绝不把 mma-settings.json 截断成请求体。
        let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
            respond_json(&mut stream, 400, r#"{"error":"invalid settings json"}"#);
            return;
        };
        if !value.is_object() {
            respond_json(&mut stream, 400, r#"{"error":"settings must be an object"}"#);
            return;
        }
        let Some(merged) = config::merge_plugin_settings(&value) else {
            respond_json(&mut stream, 500, r#"{"error":"settings write failed"}"#);
            return;
        };
        // ③ 缓存 + 广播：先更新本地缓存（语句结束即释放锁），再推全量合并结果；
        //    响应体是**合并后的全量对象**（Step 9 与冒烟脚本据此读回，不是 `{}`）。
        *shared.plugin_settings.lock().unwrap() = merged.clone();
        broadcast(&shared, "settings", Some(merged.clone()));
        respond_json(&mut stream, 200, &merged.to_string());
        return;
    }

    // POST /shell-config：壳配置（mma-shell-config.json）的请求键优先读-改-写。
    if method == "POST" && path == "/shell-config" {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
            respond_json(&mut stream, 400, r#"{"error":"invalid shell config json"}"#);
            return;
        };
        if !value.is_object() {
            respond_json(&mut stream, 400, r#"{"error":"shell config must be an object"}"#);
            return;
        }

        // 处理窗口置顶与穿透控制（如果有）
        if let Some(win_val) = value.get("window").and_then(|w| w.as_object()) {
            let topmost = win_val.get("topmost").and_then(|v| v.as_bool());
            let click_through = win_val.get("clickThrough").and_then(|v| v.as_bool());
            if topmost.is_some() || click_through.is_some() {
                if let Some(app) = shared.app.lock().unwrap().as_ref() {
                    crate::app::window_state::apply_flag_change(app, topmost, click_through);
                } else {
                    let mut st = crate::app::window_state::WINDOW_STATE.lock().unwrap();
                    let disk = config::read_window_state();
                    st.topmost = topmost.unwrap_or(disk.topmost);
                    st.click_through = click_through.unwrap_or(disk.click_through);
                    crate::app::window_state::persist_window_state();
                }
            }
        }

        // 过滤掉 window 键再传给 patch_shell_config
        let mut cfg_patch = value.clone();
        if let Some(obj) = cfg_patch.as_object_mut() {
            obj.remove("window");
        }
        if !cfg_patch.as_object().map(|m| m.is_empty()).unwrap_or(false) {
            let Some(merged) = config::patch_shell_config(&cfg_patch) else {
                respond_json(&mut stream, 400, r#"{"error":"shell config write failed"}"#);
                return;
            };
            *shared.offline_settings.lock().unwrap() = merged.clone();
            config::clear_detect_caches();
            broadcast(&shared, "settings", Some(merged.clone()));
        }

        let cfg = config::read_shell_config();
        let win = config::read_window_state();
        let body = serde_json::json!({
            "config": cfg,
            "resolved": {
                "etternaRoot": crate::etterna::etterna_root(&shared),
                "malodyRoot": malody_root(&shared),
                "malody4Root": malody4_effective_root(&shared),
                "lazerStorageRoot": lazer_storage_root(),
            },
            "window": {
                "topmost": win.topmost,
                "clickThrough": win.click_through
            }
        });
        respond_json(&mut stream, 200, &body.to_string());
        return;
    }

    // POST /open-settings：打开/聚焦设置窗口。**不等窗口真的建出来**（open_or_focus 只置
    // 标志并起独立线程，建窗是异步的；见 settings_window.rs 线程规则）。
    if method == "POST" && path == "/open-settings" {
        let app = shared.app.lock().unwrap().clone();
        match app {
            Some(app) => {
                crate::settings_window::open_or_focus(&app, &shared.plugin_dir);
                respond_json(&mut stream, 200, "{}");
            }
            // 无窗口模式（app 句柄未注入）→ 503（状态码固定，body 形状不参与判定）。
            None => respond_json(&mut stream, 503, r#"{"error":"no app handle"}"#),
        }
        return;
    }

    // POST /show-main：聚焦/显示主悬浮窗（第二实例启动时无 --settings 则转交此端点）。
    if method == "POST" && path == "/show-main" {
        let app = shared.app.lock().unwrap().clone();
        match app {
            Some(app) => {
                let app2 = app.clone();
                let _ = app.run_on_main_thread(move || {
                    use tauri::Manager;
                    if let Some(main_win) = app2.get_webview_window("main") {
                        let _ = main_win.unminimize();
                        let _ = main_win.show();
                        let _ = main_win.set_focus();
                    }
                });
                respond_json(&mut stream, 200, "{}");
            }
            None => respond_json(&mut stream, 503, r#"{"error":"no app handle"}"#),
        }
        return;
    }

    // POST /feedback：RC 难度反馈的壳侧门控与签名（页面负责把信封转发到后端）。
    if path == "/feedback" {
        let origin = allowed_origin(head);
        if method == "OPTIONS" {
            let Some(origin) = origin else {
                respond_json(&mut stream, 403, r#"{"error":"forbidden origin"}"#);
                return;
            };
            let head = format!(
                "HTTP/1.1 204 No Content\r\nAccess-Control-Allow-Origin: {origin}\r\nVary: Origin\r\nAccess-Control-Allow-Methods: POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\nAccess-Control-Max-Age: 600\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            let _ = stream.write_all(head.as_bytes());
            return;
        }
        if method != "POST" {
            respond_json_cors(&mut stream, 405, r#"{"error":"method not allowed"}"#, origin.as_deref());
            return;
        }
        // 浏览器 POST 必带 Origin（包含跨源页面）：来源不在白名单就直接拒绝，
        // 与 OPTIONS 同一规则。无 Origin 只可能是本机进程直连（端点只绑回环），
        // 见 CONTRACT.md §13。
        if header_value(head, "origin").is_some() && origin.is_none() {
            respond_json(&mut stream, 403, r#"{"error":"forbidden origin"}"#);
            return;
        }
        handle_feedback(&shared, stream, body, origin);
        return;
    }

    if path == "/settings" {
        // 优先级：tosu 在线设置文件 > tosu 设置文件（离线）> mma-settings.json >
        // settings.json 生成默认。见 config::resolve_plugin_settings。
        let settings = config::resolve_plugin_settings(&shared);
        let body = serde_json::to_string(&settings).unwrap_or_default();
        write_response(&mut stream, 200, "application/json", body.as_bytes());
        return;
    }

    // GET /shell-config：壳配置全文 + 三个根目录的**实际采纳**路径（`None` → JSON `null`，
    // 由 `Option<PathBuf>` 序列化而来，不做有损字符串化）。配置不可读 → 400。
    if path == "/shell-config" {
        // 24050#mma-shell 的跨源读取需要 ACAO；白名单与 /feedback 相同，绝不使用 *。
        let origin = allowed_origin(head);
        let cfg = config::read_shell_config();
        if !cfg.is_object() {
            respond_json_cors(
                &mut stream,
                400,
                r#"{"error":"shell config unreadable"}"#,
                origin.as_deref(),
            );
            return;
        }
        let win = config::read_window_state();
        // 结算屏的"这一局来自回放"位：页面在进入结算屏时本来就要取一次本端点（穿透状态），
        // 顺手带上它——回放结果就不会在数据面切换的窗口里先弹成"可评分"。
        let replay = crate::osu::instance()
            .and_then(|reader| reader.snapshot())
            .map(|snapshot| snapshot.is_result_state() && snapshot.replay)
            .unwrap_or(false);
        let body = serde_json::json!({
            "config": cfg,
            "resolved": {
                "etternaRoot": crate::etterna::etterna_root(&shared),
                "malodyRoot": malody_root(&shared),
                "malody4Root": malody4_effective_root(&shared),
                "lazerStorageRoot": lazer_storage_root(),
            },
            "window": {
                "topmost": win.topmost,
                "clickThrough": win.click_through
            },
            "feedback": {
                "replay": replay,
                // 本地联调：用 `MMA_FEEDBACK_ENDPOINT=http://127.0.0.1:8080/api/v1/feedback`
                // 启动壳时，页面把投票发到该地址而不是生产后端（壳窗口没有控制台，
                // 页面自己写不了 window.__MMA_FEEDBACK_ENDPOINT）。生产不设 ⇒ null。
                "endpoint": std::env::var("MMA_FEEDBACK_ENDPOINT")
                    .ok()
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
            }
        });
        respond_json_cors(&mut stream, 200, &body.to_string(), origin.as_deref());
        return;
    }

    // GET /offsets/status：当前内存偏移表状态与生成器就绪态（P2 Topic 9 / 10 / 12）
    if path == "/offsets/status" {
        let stable_table = crate::osu::offsets::find_stable_table(None);
        let ppysb_table = crate::osu::offsets::find_stable_table_variant(None, "ppysb");
        let lazer_default = crate::osu::offsets::default_lazer_table();
        let state = crate::osu::instance().map(|r| r.latest());
        let active_diag = state.as_ref().and_then(|s| s.lazer.as_ref());
        let active = active_diag.and_then(|d| d.active_table.as_ref());
        let bundled = crate::osu::lazer::TableStatus::from(&lazer_default);
        #[allow(unused_mut)]
        let mut lazer = if let Some(table) = active {
            let source = active_diag
                .and_then(|d| d.table_path.as_deref())
                .filter(|path| !path.is_empty() && *path != "<embedded>")
                .unwrap_or("embedded");
            serde_json::json!({
                "client": "lazer",
                "version": table.version,
                "revision": crate::osu::offsets::lazer_revision_from_version(&table.version),
                "runtime_version": table.runtime_version,
                "arch": table.arch,
                "source": source,
                "types_count": table.types_count,
                "score_ready": table.score_ready,
                "loaded": true,
                "bundled": bundled
            })
        } else {
            let (lazer_table, lazer_source) = latest_lazer_table();
            serde_json::json!({
                "client": "lazer",
                "version": lazer_table.lazer_version,
                "revision": lazer_table.revision(),
                "runtime_version": lazer_table.runtime_version,
                "arch": lazer_table.arch,
                "source": lazer_source,
                "types_count": lazer_table.types.len(),
                "score_ready": lazer_table.score().is_some(),
                "loaded": false,
                "bundled": bundled
            })
        };
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        {
            lazer["notice"] = serde_json::json!(state.as_ref().and_then(|s| s.reason.as_deref()));
        }
        let stable_source = stable_table_source("stable");
        let ppysb_source = stable_table_source("ppysb");
        let gen_ready = cfg!(windows) && find_gen_executable(&shared).is_some();
        let body = serde_json::json!({
            "status": "ok",
            "generator_supported": cfg!(windows),
            "generator_ready": gen_ready,
            "ppysb_active": crate::osu::runner::ACTIVE_STABLE_VARIANT.load(std::sync::atomic::Ordering::Relaxed) == 1,
            "stable": {
                "client": stable_table.as_ref().map(|t| t.client.clone()).unwrap_or_else(|_| "stable".to_string()),
                "version": stable_table.as_ref().map(|t| t.version.clone()).unwrap_or_else(|_| "unknown".to_string()),
                "revision": stable_table.as_ref().map(|t| t.revision).unwrap_or(0),
                "arch": "x86",
                "source": stable_source,
                "anchors_count": stable_table.as_ref().map(|t| t.anchors.len()).unwrap_or(0),
                "loaded": stable_table.is_ok(),
                "replay_frames_ready": stable_table.as_ref().map(|t| t.topology.replay.is_some()).unwrap_or(false),
                "notes_ready": stable_table.as_ref().map(|t| t.topology.notes.is_some()).unwrap_or(false)
            },
            "ppysb": {
                "client": "ppysb",
                "version": ppysb_table.as_ref().map(|t| t.version.clone()).unwrap_or_else(|_| "unknown".to_string()),
                "revision": ppysb_table.as_ref().map(|t| t.revision).unwrap_or(0),
                "arch": "x86",
                "source": ppysb_source,
                "anchors_count": ppysb_table.as_ref().map(|t| t.anchors.len()).unwrap_or(0),
                "loaded": ppysb_table.is_ok(),
                "replay_frames_ready": ppysb_table.as_ref().map(|t| t.topology.replay.is_some()).unwrap_or(false),
                "notes_ready": ppysb_table.as_ref().map(|t| t.topology.notes.is_some()).unwrap_or(false)
            },
            "lazer": lazer
        });
        respond_json(&mut stream, 200, &body.to_string());
        return;
    }

    // POST /offsets/generate：一键免 SDK 活体自校验生成偏移表（P2 Topic 9）
    if method == "POST" && path == "/offsets/generate" {
        if !cfg!(windows) {
            respond_json(&mut stream, 400, r#"{"status":"error","code":"unsupported_os","message":"Live generation requires Windows. Use Check Remote Updates on Linux."}"#);
            return;
        }
        let gen_exe = find_gen_executable(&shared);
        let Some(gen_exe_path) = gen_exe else {
            respond_json(
                &mut stream,
                404,
                r#"{"status":"error","code":"gen_not_found","message":"gen.exe not found (neither next to mma-shell.exe nor in dev tree)"}"#,
            );
            return;
        };

        let mut cmd = std::process::Command::new(&gen_exe_path);
        cmd.arg("live").arg("--json");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let local_lazer = gen_exe_path
            .parent()
            .map(|p| p.join("lazer"))
            .filter(|p| p.is_dir());
        cmd.arg("--out").arg(local_lazer.unwrap_or_else(|| crate::osu::offsets::remote_offsets_dir("lazer")));

        match cmd.output() {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                if output.status.success() {
                    respond_json(&mut stream, 200, if stdout.is_empty() { r#"{"status":"ok","code":"ok"}"# } else { &stdout });
                } else {
                    let detail = if !stdout.is_empty() {
                        serde_json::from_str::<serde_json::Value>(&stdout)
                            .ok()
                            .and_then(|value| value.get("message").and_then(|m| m.as_str()).map(str::to_string))
                            .unwrap_or(stdout)
                    } else if !stderr.is_empty() {
                        stderr
                    } else {
                        String::new()
                    };
                    let err_msg = serde_json::json!({
                        "status": "error",
                        "code": "gen_failed",
                        "detail": detail,
                        "message": "generator failed or game process not running"
                    });
                    respond_json(&mut stream, 400, &err_msg.to_string());
                }
            }
            Err(e) => {
                let err_msg = serde_json::json!({
                    "status": "error",
                    "code": "gen_spawn_failed",
                    "detail": e.to_string(),
                    "message": "failed to run gen.exe"
                });
                respond_json(&mut stream, 500, &err_msg.to_string());
            }
        }
        return;
    }

    // POST /offsets/update：检查并更新远端签名内存表（P2 Topic 10，静默离线回落）
    if method == "POST" && path == "/offsets/update" {
        let manifest_url = REMOTE_OFFSETS_MANIFEST;
        let update_outcome = check_and_apply_remote_offsets(manifest_url);
        respond_json(&mut stream, 200, &update_outcome.to_string());
        return;
    }

    if path == "/cover" || path.starts_with("/cover/") {
        let rel = percent_decode(path.trim_start_matches("/cover/"));
        let allowed = shared.cover_whitelist.lock().unwrap().contains(&rel);
        if !allowed {
            respond_json(&mut stream, 404, r#"{"error":"cover not whitelisted"}"#);
            return;
        }
        match fs::read(&rel) {
            Ok(bytes) => {
                let ctype = mime_for(&rel);
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n",
                    ctype,
                    bytes.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&bytes);
            }
            Err(_) => respond_json(&mut stream, 404, r#"{"error":"cover not found"}"#),
        }
        return;
    }

    // 静态：插件目录（防穿越）
    if method != "GET" && method != "HEAD" {
        respond_json(&mut stream, 405, r#"{"error":"method not allowed"}"#);
        return;
    }
    let rel = path.trim_start_matches('/');
    let rel = if rel.is_empty() { "index.html" } else { rel };
    let base = shared.plugin_dir.canonicalize().unwrap_or_else(|_| shared.plugin_dir.clone());
    let candidate = shared.plugin_dir.join(rel);
    let candidate = candidate.canonicalize().unwrap_or(candidate);
    if !candidate.starts_with(&base) || !candidate.is_file() {
        respond_json(&mut stream, 404, r#"{"error":"not found"}"#);
        return;
    }
    match fs::read(&candidate) {
        Ok(bytes) => {
            let ctype = mime_for(rel);
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
                ctype,
                bytes.len()
            );
            let _ = stream.write_all(head.as_bytes());
            if method != "HEAD" {
                let _ = stream.write_all(&bytes);
            }
        }
        Err(_) => respond_json(&mut stream, 404, r#"{"error":"not found"}"#),
    }
}

/// `GET/POST /shell-config` 的 `resolved.lazerStorageRoot`：优先显示读取器实际
/// 采用的存储根（`folders.songs` 的父目录），未附着时回落到配置里的显式值。
fn lazer_storage_root() -> Option<String> {
    if let Some(files) = crate::osu::instance()
        .and_then(|reader| reader.snapshot())
        .and_then(|snapshot| snapshot.songs_folder)
    {
        let path = std::path::Path::new(&files);
        let root = path.parent().unwrap_or(path);
        return Some(root.to_string_lossy().into_owned());
    }
    crate::config::lazer_storage_root()
        .or_else(crate::osu::lazer::detected_lazer_storage_root)
        .map(|root| {
            crate::osu::lazer::normalize_storage_root(&root)
                .to_string_lossy()
                .into_owned()
        })
}

fn find_gen_executable(shared: &Shared) -> Option<std::path::PathBuf> {
    let names: &[&str] = if cfg!(windows) {
        &["gen.exe", "gen"]
    } else {
        &["gen", "gen.exe"]
    };

    // 1. 同级 offsets/gen 或同级 gen
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            for &name in names {
                let in_offsets = parent.join("offsets").join(name);
                if in_offsets.exists() {
                    return Some(in_offsets);
                }
                let candidate = parent.join(name);
                if candidate.exists() {
                    return Some(candidate);
                }
            }
        }
    }
    // 2. 插件上级工作区 temp/gen
    for &name in names {
        let temp_gen = shared.plugin_dir.parent().map(|p| p.join("temp").join(name));
        if let Some(p) = temp_gen {
            if p.exists() {
                return Some(p);
            }
        }
    }
    // 3. 环境变量或 PATH
    if let Ok(path) = std::env::var("MMA_GEN_EXE") {
        let p = std::path::PathBuf::from(path);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

/// 创建跨平台的 curl 命令，在 Windows 上静默运行（CREATE_NO_WINDOW），消除黑框弹出。
fn create_curl_command() -> std::process::Command {
    #[cfg(windows)]
    let mut cmd = std::process::Command::new("curl.exe");
    #[cfg(not(windows))]
    #[allow(unused_mut)]
    let mut cmd = std::process::Command::new("curl");

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd
}

/// 远端偏移表清单地址（手动按钮与每日自动检查共用）。
pub const REMOTE_OFFSETS_MANIFEST: &str =
    "https://raw.githubusercontent.com/LeoBlackMT/osumania_map_analyser/main/desktop/offsets/manifest.json";

#[derive(Clone, Debug, Default)]
struct LocalTableInfo {
    version: String,
    revision: u64,
}

fn embedded_table_info(client: &str) -> LocalTableInfo {
    match client {
        "lazer" => {
            let table = crate::osu::offsets::default_lazer_table();
            let revision = table.revision();
            LocalTableInfo {
                version: table.lazer_version,
                revision,
            }
        }
        "stable" => {
            let table = crate::osu::offsets::default_stable_table();
            LocalTableInfo {
                version: table.version,
                revision: table.revision,
            }
        }
        "ppysb" => {
            let table = crate::osu::offsets::default_ppysb_table();
            LocalTableInfo {
                version: table.version,
                revision: table.revision,
            }
        }
        _ => LocalTableInfo::default(),
    }
}

fn read_table_info(path: &std::path::Path, client: &str) -> Option<LocalTableInfo> {
    let bytes = fs::read(path).ok()?;
    match client {
        "lazer" => crate::osu::offsets::OffsetTable::load(&bytes)
            .ok()
            .map(|table| {
                let revision = table.revision();
                LocalTableInfo {
                    version: table.lazer_version,
                    revision,
                }
            }),
        "stable" | "ppysb" => crate::osu::offsets::StableTable::load(&bytes)
            .ok()
            .map(|table| LocalTableInfo {
                version: table.version,
                revision: table.revision,
            }),
        _ => None,
    }
}

fn table_info_is_newer(candidate: &LocalTableInfo, current: &LocalTableInfo) -> bool {
    if candidate.revision > 0 && current.revision > 0 {
        return candidate.revision > current.revision;
    }
    crate::osu::offsets::version_is_newer(&candidate.version, &current.version)
}

fn local_table_info(client: &str, filename: &str) -> LocalTableInfo {
    let mut best = embedded_table_info(client);
    let mut paths = vec![crate::osu::offsets::remote_table_path(client, filename)];

    if let Ok(dir) = std::env::var("MMA_OFFSETS_DIR") {
        let root = std::path::PathBuf::from(dir);
        if client == "ppysb" {
            paths.push(root.join("stable").join(filename));
            paths.push(root.join("ppysb").join(filename));
        } else {
            paths.push(root.join(client).join(filename));
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            for folder in ["offsets", "offset"] {
                let root = parent.join(folder);
                if client == "ppysb" {
                    paths.push(root.join("stable").join(filename));
                    paths.push(root.join("ppysb").join(filename));
                } else {
                    paths.push(root.join(client).join(filename));
                }
            }
        }
    }

    for path in paths {
        if let Some(candidate) = read_table_info(&path, client) {
            if table_info_is_newer(&candidate, &best) {
                best = candidate;
            }
        }
    }
    best
}

fn read_remote_table_info(client: &str, content: &[u8]) -> Result<LocalTableInfo, String> {
    match client {
        "stable" | "ppysb" => {
            let table = crate::osu::offsets::StableTable::load(content)
                .map_err(|error| format!("stable_table_load: {error}"))?;
            crate::osu::offsets::validate_stable_schema(&table)
                .map_err(|error| format!("stable_schema: {error}"))?;
            Ok(LocalTableInfo {
                version: table.version,
                revision: table.revision,
            })
        }
        "lazer" => {
            let table = crate::osu::offsets::OffsetTable::load(content)
                .map_err(|error| format!("lazer_table_load: {error}"))?;
            crate::osu::offsets::validate_lazer_schema(&table)
                .map_err(|error| format!("lazer_schema: {error}"))?;
            let revision = table.revision();
            Ok(LocalTableInfo {
                version: table.lazer_version,
                revision,
            })
        }
        _ => Err(format!("unsupported_client: {client}")),
    }
}

fn remote_table_url_folder(client: &str) -> &str {
    // ppysb 的随包表放在 offsets/stable 下；运行期缓存仍写 offsets/ppysb。
    if client == "ppysb" {
        "stable"
    } else {
        client
    }
}

fn download_offsets_bytes(url: &str, max_bytes: usize) -> Result<Vec<u8>, String> {
    let max_bytes_arg = max_bytes.to_string();
    let mut cmd = create_curl_command();
    cmd.arg("-sS")
        .arg("-f")
        .arg("-L")
        .arg("--connect-timeout")
        .arg("5")
        .arg("--max-time")
        .arg("20")
        .arg("--retry")
        .arg("2")
        .arg("--retry-delay")
        .arg("1")
        .arg("--max-filesize")
        .arg(&max_bytes_arg)
        .arg(url);
    let output = cmd
        .output()
        .map_err(|error| format!("curl_spawn: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "curl_exit_{}: {}",
            output.status.code().unwrap_or(-1),
            stderr.trim()
        ));
    }
    if output.stdout.len() > max_bytes {
        return Err(format!(
            "response_too_large: {} > {max_bytes}",
            output.stdout.len()
        ));
    }
    Ok(output.stdout)
}

fn offsets_update_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn push_offsets_error(
    errors: &mut Vec<serde_json::Value>,
    filename: &str,
    stage: &str,
    detail: impl Into<String>,
) {
    errors.push(serde_json::json!({
        "filename": filename,
        "stage": stage,
        "detail": detail.into(),
    }));
}

pub fn check_and_apply_remote_offsets(manifest_url: &str) -> serde_json::Value {
    let _guard = match offsets_update_lock().try_lock() {
        Ok(guard) => guard,
        Err(_) => {
            return serde_json::json!({
                "status": "busy",
                "code": "update_in_progress",
                "message": "Another offsets update is already running",
                "updated": 0,
                "skipped": 0,
                "failed": 0,
                "errors": []
            });
        }
    };

    let manifest_bytes = match download_offsets_bytes(manifest_url, 1024 * 1024) {
        Ok(bytes) => bytes,
        Err(detail) => {
            return serde_json::json!({
                "status": "offline_fallback",
                "code": "network_unavailable",
                "message": "Network unavailable or remote host unreachable; keeping existing local offsets",
                "detail": detail,
                "updated": 0,
                "skipped": 0,
                "failed": 0,
                "errors": []
            });
        }
    };

    let manifest = match serde_json::from_slice::<crate::osu::offsets::RemoteManifest>(
        &manifest_bytes,
    ) {
        Ok(manifest) => manifest,
        Err(error) => {
            return serde_json::json!({
                "status": "error",
                "code": "manifest_unreadable",
                "message": "Remote manifest unreadable or malformed",
                "detail": error.to_string(),
                "updated": 0,
                "skipped": 0,
                "failed": 1,
                "errors": [{
                    "filename": "manifest.json",
                    "stage": "manifest_parse",
                    "detail": error.to_string()
                }]
            });
        }
    };

    if manifest.schema_version != crate::osu::offsets::MANIFEST_SCHEMA_VERSION {
        return serde_json::json!({
            "status": "error",
            "code": "manifest_schema_unsupported",
            "message": format!(
                "Unsupported remote manifest schema_version: {} (expected {})",
                manifest.schema_version,
                crate::osu::offsets::MANIFEST_SCHEMA_VERSION
            ),
            "updated": 0,
            "skipped": 0,
            "failed": 1,
            "errors": [{
                "filename": "manifest.json",
                "stage": "manifest_schema",
                "detail": format!("schema_version={}", manifest.schema_version)
            }]
        });
    }

    if let Err(error) = manifest.verify_manifest_signature() {
        return serde_json::json!({
            "status": "error",
            "code": "manifest_signature_invalid",
            "message": "Remote manifest signature verification failed",
            "detail": error,
            "updated": 0,
            "skipped": 0,
            "failed": 1,
            "errors": [{
                "filename": "manifest.json",
                "stage": "manifest_signature",
                "detail": error
            }]
        });
    }

    let base_url = if let Some((base, _)) = manifest_url.rsplit_once('/') {
        base
    } else {
        manifest_url
    };

    let mut updated_count = 0;
    let mut skipped_count = 0;
    let mut errors: Vec<serde_json::Value> = Vec::new();

    for table in manifest.tables {
        if !crate::osu::offsets::is_supported_client(&table.client) {
            push_offsets_error(
                &mut errors,
                &table.filename,
                "client",
                format!("unsupported client: {}", table.client),
            );
            continue;
        }
        if !crate::osu::offsets::is_safe_table_filename(&table.filename) {
            push_offsets_error(
                &mut errors,
                &table.filename,
                "filename",
                "unsafe table filename",
            );
            continue;
        }

        let table_url = format!(
            "{base_url}/{}/{}",
            remote_table_url_folder(&table.client),
            table.filename
        );
        let content = match download_offsets_bytes(
            &table_url,
            crate::osu::offsets::MAX_TABLE_BYTES,
        ) {
            Ok(content) => content,
            Err(detail) => {
                push_offsets_error(&mut errors, &table.filename, "download", detail);
                continue;
            }
        };

        if let Err(error) = crate::osu::offsets::verify_table_signature(
            &content,
            &table.sha256,
            &table.signature,
            &crate::osu::offsets::ED25519_MASTER_PUBLIC_KEY,
        ) {
            push_offsets_error(&mut errors, &table.filename, "signature", error);
            continue;
        }

        let remote_info = match read_remote_table_info(&table.client, &content) {
            Ok(info) => info,
            Err(detail) => {
                push_offsets_error(&mut errors, &table.filename, "schema", detail);
                continue;
            }
        };

        if table.revision > 0
            && remote_info.revision > 0
            && table.revision != remote_info.revision
        {
            push_offsets_error(
                &mut errors,
                &table.filename,
                "revision",
                format!(
                    "manifest revision {} != table revision {}",
                    table.revision, remote_info.revision
                ),
            );
            continue;
        }

        let remote_revision = if table.revision > 0 {
            table.revision
        } else {
            remote_info.revision
        };
        let local_info = local_table_info(&table.client, &table.filename);
        let should_update = if remote_revision > 0 && local_info.revision > 0 {
            crate::osu::offsets::revision_is_newer(remote_revision, local_info.revision)
        } else if table.client == "stable" || table.client == "ppysb" {
            push_offsets_error(
                &mut errors,
                &table.filename,
                "revision",
                "stable/ppysb table has no revision; refusing unordered update",
            );
            continue;
        } else {
            crate::osu::offsets::version_is_newer(&remote_info.version, &local_info.version)
        };

        if !should_update {
            skipped_count += 1;
            continue;
        }

        if let Err(error) =
            crate::osu::offsets::save_remote_table(&table.client, &table.filename, &content)
        {
            push_offsets_error(&mut errors, &table.filename, "write", error.to_string());
            continue;
        }
        updated_count += 1;
    }

    let failed_count = errors.len();
    let status = if failed_count == 0 {
        "ok"
    } else if updated_count > 0 {
        "partial"
    } else {
        "error"
    };
    let message = match status {
        "ok" => format!(
            "Offsets check complete: {updated_count} table(s) updated, {skipped_count} already current"
        ),
        "partial" => format!(
            "Offsets check partially completed: {updated_count} updated, {skipped_count} current, {failed_count} failed"
        ),
        _ => format!(
            "Offsets check failed: {failed_count} table(s) could not be updated ({skipped_count} already current)"
        ),
    };

    serde_json::json!({
        "status": status,
        "code": status,
        "message": message,
        "updated": updated_count,
        "skipped": skipped_count,
        "failed": failed_count,
        "errors": errors
    })
}

#[cfg(test)]
mod cors_tests {
    use super::allowed_origin;

    #[test]
    fn shell_page_origins_allow_only_loopback_shell_pages() {
        assert_eq!(
            allowed_origin("Origin: http://127.0.0.1:24050"),
            Some("http://127.0.0.1:24050".to_string())
        );
        assert_eq!(
            allowed_origin("Origin: http://localhost:24061"),
            Some("http://localhost:24061".to_string())
        );
        assert_eq!(
            allowed_origin("Origin: http://tauri.localhost"),
            Some("http://tauri.localhost".to_string())
        );
        assert_eq!(allowed_origin("Origin: http://evil.example"), None);
        assert_eq!(allowed_origin(""), None);
    }
}

