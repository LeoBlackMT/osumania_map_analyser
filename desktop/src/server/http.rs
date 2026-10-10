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

fn status_text(code: u16) -> &'static str {
    match code {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        415 => "Unsupported Media Type",
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
        let cfg = config::read_shell_config();
        if !cfg.is_object() {
            respond_json(&mut stream, 400, r#"{"error":"shell config unreadable"}"#);
            return;
        }
        let win = config::read_window_state();
        let body = serde_json::json!({
            "config": cfg,
            "resolved": {
                "etternaRoot": crate::etterna::etterna_root(&shared),
                "malodyRoot": malody_root(&shared),
                "malody4Root": malody4_effective_root(&shared),
            },
            "window": {
                "topmost": win.topmost,
                "clickThrough": win.click_through
            }
        });
        respond_json(&mut stream, 200, &body.to_string());
        return;
    }

    // GET /offsets/status：当前内存偏移表状态与生成器就绪态（P2 Topic 9 / 10 / 12）
    if path == "/offsets/status" {
        let stable_table = crate::osu::offsets::find_stable_table(None);
        let lazer_default = crate::osu::offsets::default_lazer_table();
        let gen_ready = find_gen_executable(&shared).is_some();
        let shadow_diag = crate::osu::compare::current_diagnostics();
        let shadow_enabled = std::env::var("MMA_SHADOW_DIAG").map(|v| v != "0").unwrap_or(false)
            || std::env::args().any(|arg| arg == "--shadow-diag" || arg == "--debug");
        let body = serde_json::json!({
            "status": "ok",
            "generator_ready": gen_ready,
            "shadow_enabled": shadow_enabled,
            "stable": {
                "client": "stable",
                "version": stable_table.as_ref().map(|t| t.version.clone()).unwrap_or_else(|_| "unknown".to_string()),
                "arch": "x86",
                "anchors_count": stable_table.as_ref().map(|t| t.anchors.len()).unwrap_or(0),
                "loaded": stable_table.is_ok()
            },
            "lazer": {
                "client": "lazer",
                "version": lazer_default.lazer_version,
                "runtime_version": lazer_default.runtime_version,
                "arch": lazer_default.arch,
                "types_count": lazer_default.types.len(),
                "loaded": true
            },
            "shadow": shadow_diag
        });
        respond_json(&mut stream, 200, &body.to_string());
        return;
    }

    // GET /shadow/status：获取当前 L3 影子比对状态与诊断（P2 Topic 12）
    if method == "GET" && path == "/shadow/status" {
        let diag = crate::osu::compare::current_diagnostics();
        respond_json(&mut stream, 200, &serde_json::to_string(&diag).unwrap_or_default());
        return;
    }

    // POST /shadow/reset：重置影子比对计数
    if method == "POST" && path == "/shadow/reset" {
        crate::osu::compare::reset_diagnostics();
        respond_json(&mut stream, 200, r#"{"status":"ok"}"#);
        return;
    }

    // POST /offsets/generate：一键免 SDK 活体自校验生成偏移表（P2 Topic 9）
    if method == "POST" && path == "/offsets/generate" {
        let gen_exe = find_gen_executable(&shared);
        let Some(gen_exe_path) = gen_exe else {
            respond_json(
                &mut stream,
                404,
                r#"{"status":"error","message":"gen.exe not found (neither next to mma-shell.exe nor in dev tree)"}"#,
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
        if let Some(out_dir) = local_lazer {
            cmd.arg("--out").arg(out_dir);
        } else if let Ok(appdata) = std::env::var("APPDATA") {
            let out_dir = std::path::PathBuf::from(appdata)
                .join("ManiaMapAnalyser")
                .join("offsets")
                .join("lazer");
            cmd.arg("--out").arg(out_dir);
        } else if let Ok(home) = std::env::var("HOME") {
            let out_dir = std::path::PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("ManiaMapAnalyser")
                .join("offsets")
                .join("lazer");
            cmd.arg("--out").arg(out_dir);
        }

        match cmd.output() {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                if output.status.success() {
                    respond_json(&mut stream, 200, if stdout.is_empty() { r#"{"status":"ok"}"# } else { &stdout });
                } else {
                    let err_msg = if !stdout.is_empty() {
                        stdout
                    } else if !stderr.is_empty() {
                        format!(r#"{{"status":"error","message":{}}}"#, serde_json::to_string(&stderr).unwrap_or_default())
                    } else {
                        r#"{"status":"error","message":"generator failed or game process not running"}"#.to_string()
                    };
                    respond_json(&mut stream, 400, &err_msg);
                }
            }
            Err(e) => {
                respond_json(
                    &mut stream,
                    500,
                    &format!(r#"{{"status":"error","message":"failed to run gen.exe: {e}"}}"#),
                );
            }
        }
        return;
    }

    // POST /offsets/update：检查并更新远端签名内存表（P2 Topic 10，静默离线回落）
    if method == "POST" && path == "/offsets/update" {
        let manifest_url = "https://raw.githubusercontent.com/LeoBlackMT/osumania_map_analyser/main/desktop/offsets/manifest.json";
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
    let mut cmd = std::process::Command::new("curl");

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd
}

fn check_and_apply_remote_offsets(manifest_url: &str) -> serde_json::Value {
    let mut cmd = create_curl_command();
    cmd.arg("-s").arg("-f").arg("-L").arg("--connect-timeout").arg("5").arg(manifest_url);
    let output = match cmd.output() {
        Ok(out) if out.status.success() => out,
        _ => {
            return serde_json::json!({
                "status": "offline_fallback",
                "message": "Network unavailable or remote host unreachable; keeping existing local offsets",
                "updated": 0
            });
        }
    };

    let manifest_bytes = output.stdout;
    let Ok(manifest) = serde_json::from_slice::<crate::osu::offsets::RemoteManifest>(&manifest_bytes) else {
        return serde_json::json!({
            "status": "error",
            "message": "Remote manifest unreadable or malformed",
            "updated": 0
        });
    };

    let base_url = if let Some((base, _)) = manifest_url.rsplit_once('/') {
        base
    } else {
        manifest_url
    };

    let mut updated_count = 0;
    for table in manifest.tables {
        let table_url = format!("{base_url}/{}/{}", table.client, table.filename);
        let mut t_cmd = create_curl_command();
        t_cmd.arg("-s").arg("-f").arg("-L").arg("--connect-timeout").arg("5").arg(&table_url);
        let Ok(t_out) = t_cmd.output() else { continue };
        if !t_out.status.success() { continue };

        let content = t_out.stdout;
        // 验签
        if crate::osu::offsets::verify_table_signature(
            &content,
            &table.sha256,
            &table.signature,
            &crate::osu::offsets::ED25519_MASTER_PUBLIC_KEY,
        ).is_err() {
            continue;
        }

        // Schema 校验门
        if table.client == "stable" {
            let Ok(stable_t) = crate::osu::offsets::StableTable::load(&content) else { continue };
            if crate::osu::offsets::validate_stable_schema(&stable_t).is_err() { continue };
        } else if table.client == "lazer" {
            let Ok(lazer_t) = crate::osu::offsets::OffsetTable::load(&content) else { continue };
            if crate::osu::offsets::validate_lazer_schema(&lazer_t).is_err() { continue };
        }

        // 原子落盘
        if crate::osu::offsets::save_remote_table(&table.client, &table.filename, &content).is_ok() {
            updated_count += 1;
        }
    }

    serde_json::json!({
        "status": "ok",
        "message": format!("Offsets check complete: {updated_count} table(s) updated"),
        "updated": updated_count
    })
}
