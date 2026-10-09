// app::instance - 第二实例零闪烁探测与命令行参数转移

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::thread;
use std::time::Duration;
use crate::server;

/// 探测地址：本机壳端口（24061）。
const PROBE_ADDR: &str = "127.0.0.1:24061";

/// 既有实例探测结果。
pub enum ExistingInstance {
    /// 端口上是本壳（`GET /settings` → 200 + 以 `{` 开头的响应体）。
    OurShell,
    /// 端口被别的进程占用（非 200 或响应体不是 JSON 对象）。
    Foreign,
    /// 端口没人监听 → 正常启动。
    None,
}

/// 向本机壳端口发一个最小 HTTP/1.1 请求并读回全部响应（lossy UTF-8）。
pub fn shell_http_request(method: &str, path: &str, extra_headers: &str) -> std::io::Result<String> {
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

/// 最多 3 次探测 24061 上的既有实例。
pub fn probe_existing_instance() -> ExistingInstance {
    for attempt in 0..3 {
        if attempt > 0 {
            thread::sleep(Duration::from_millis(200));
        }
        let Ok(response) = shell_http_request("GET", "/settings", "") else {
            continue;
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

/// 检查并处理既有实例。
/// 若存在既有实例，转发参数并退出进程，返回 true；若为首个实例，返回 false 允许继续启动。
pub fn try_forward_and_exit() -> bool {
    let want_settings = std::env::args().any(|arg| arg == "--settings");
    match probe_existing_instance() {
        ExistingInstance::OurShell => {
            server::log::log_line("existing mma-shell instance detected on 24061");
            if want_settings {
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
            } else {
                match shell_http_request("POST", "/show-main", "Content-Length: 0\r\n") {
                    Ok(response) => server::log::log_line(&format!(
                        "existing instance: show-main forwarded ({})",
                        response.lines().next().unwrap_or("(no status line)")
                    )),
                    Err(e) => server::log::log_at(
                        "error",
                        &format!("existing instance: show-main FAILED: {}", e),
                    ),
                }
            }
            std::process::exit(0);
        }
        ExistingInstance::Foreign => {
            server::log::log_at("error", "port 24061 is occupied by another process");
            std::process::exit(2);
        }
        ExistingInstance::None => false,
    }
}
