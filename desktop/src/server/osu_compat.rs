// 24062：tosu 兼容子集 origin —— **实时载荷为默认**（B1 回放降级为测试开关）。
//
// 本模块只做一件事：在 `127.0.0.1:24062` 上假装成 tosu 的三条端点，让页面指向它并渲染
// 卡片（计划 §4 Step 4 / §3.3 端点路径表）。
//
// 数据面（Step 9 起）：
// - **默认 = 实时（live）**：`/websocket/v2` 推壳内读取线程的最新载荷；两条文件路由供奉
//   **当前选中的谱面**。
//   · 未附着 / `unhealthy` ⇒ `packet()` 为 `None` ⇒ **一帧都不发**；两条文件路由 **404**；
//   · **字段级冻结**（DEC-18：`frozen`）⇒ WS 只发 `client` + `state`（**省略 `beatmap`**），
//     两条文件路由 **404**；
//   · **身份保持**（Step 9g：`frozen` + `holding`，见 `invariants::IDENTITY_HOLD_GRACE`）⇒
//     WS 发**最后一张好图**的 `beatmap`/`files`/`directPath`/`folders` + 本帧 `state`，
//     两条文件路由供奉**同一张图**的文件（200）——页面看不到身份变化，不会重抓、不会把
//     24062 的 404 渲染成用户可见的错误；窗口过后回落上面的冻结行为；
//   · 任何情形都**绝不回落到固定回放图**（那会让页面拿到上一张图的假数据）。
// - `MMA_OSU_COMPAT_REPLAY=1` = **B1 回放**（测试用）：固定一张真实 `.osu`（磁盘直供）
//   + 每 150 ms 推一帧手写 tosu-v2 形状包，供"零内存读取"下证伪/证实架构。
//   固定谱面 = `D:\Games\osu!\Songs\2004024 Icon For Hire - Make a Move (Sped Up & Cut Ver)\`
//   下的 `[2000s emo-rock type song]`（4K mania；657 个 hitobject；背景 `SHE IS PLAYING
//   TRIUMPH AND REGRET.jpg`、音频 `audio.mp3`；BeatmapID/SetID = 4167558/2004024；
//   `.osu` = 19 379 B、MD5 `589a91e2c0d7d5f3c96195e39ae05c6a`（**启动时实测**，不硬编码）；
//   firstObject = 4431 ms、lastObject = 48719 ms —— 末对象是 circle，故不需滑条时长推算）。
//   选型与候选清单：`temp/osu-native-memory/evidence/B1-replay/tools/pick-map-shortlist.json`。
//
// 与 tosu 的差异（**有意的、记录在案**）：`play.hits`/`resultsScreen.hits` 只供 §3.3
// 字段表要求的 6 键（真实 tosu 供 10/9 键，多出的 sliderBreaks 等页面不消费）；
// `resultsScreen` 只给 `{hits,mods}`；帧速率固定 150 ms。其余字符串形状照
// `evidence/P8/P8-precapture-notes.md` 的真实帧对齐。

use crate::frames::{
    OSU_COMPAT_BACKGROUND_ROUTE, OSU_COMPAT_FILE_ROUTE, OSU_COMPAT_PORT, OSU_COMPAT_WS_PATH,
};
use crate::server::{http, ws, Shared};
use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

/// 回放节奏：计划 §3.3 端点路径表写死 150 ms。
const REPLAY_INTERVAL: Duration = Duration::from_millis(150);
/// WS 的 read 超时（`ws.read()` 会阻塞，超时即周期性唤醒）。**必须显著小于
/// `REPLAY_INTERVAL`**：出帧判定是"`elapsed() >= 150ms` 就发"，若唤醒粒度也是
/// 100 ms，实际节奏会被量化成 ~200 ms（实测 217 ms）。20 ms ⇒ 实测 153–158 ms。
const READ_TIMEOUT: Duration = Duration::from_millis(20);

/// 24062 是否已绑定成功。绑定失败时壳**不下发** native 端点（`sources.osu` 的决策入参），
/// 否则页面会切到一个没人听的端口。
static BOUND: AtomicBool = AtomicBool::new(false);

/// 24062 是否可用（`server::osu_source` 的唯一入参来源）。
pub fn bound() -> bool {
    BOUND.load(Ordering::Relaxed)
}

// ---- 固定回放谱面（TEMPORARY B1 replay source）----

/// osu!stable 的 Songs 根目录（`folders.songs`，P8 帧同形）。
const SONGS_DIR: &str = r"D:\Games\osu!\Songs";
/// osu!stable 安装目录（`folders.game`）。
const GAME_DIR: &str = r"D:\Games\osu!";
const MAP_FOLDER: &str = "2004024 Icon For Hire - Make a Move (Sped Up & Cut Ver)";
const MAP_FILE: &str =
    "Icon For Hire - Make a Move (Sped Up & Cut Ver.) (MocaLoca) [2000s emo-rock type song].osu";
const MAP_BACKGROUND: &str = "SHE IS PLAYING TRIUMPH AND REGRET.jpg";
const MAP_AUDIO: &str = "audio.mp3";

/// 固定谱面的磁盘绝对路径（`\` 连接，与 `directPath.*` 的 win32 形状一致）。
fn map_dir() -> PathBuf {
    PathBuf::from(format!("{}\\{}", SONGS_DIR, MAP_FOLDER))
}

// ---- 入站请求 ----

/// **回放开关**（测试用）：`MMA_OSU_COMPAT_REPLAY=1` 时回到 B1 的固定谱面回放。
/// 默认关闭（= 实时载荷）。
pub fn replay_enabled() -> bool {
    matches!(std::env::var("MMA_OSU_COMPAT_REPLAY"), Ok(value) if value.trim() == "1")
}

/// 实时载荷（**默认**，Step 9 起）：`/websocket/v2` 推**壳内读取线程的最新载荷**，两条
/// 文件路由改为供奉**当前选中的谱面**。
///
/// 读取器未附着/`unhealthy` ⇒ `packet()` 为 `None` ⇒ **这一帧不发**，文件路由 404；
/// 字段级冻结（`frozen` 且**不**处于身份保持期）⇒ 只发 `client` + `state`、文件路由 404；
/// 身份保持期（Step 9g）⇒ 发最后一张好图的图表块 + 本帧 `state`，文件路由供奉同一张图（200）
/// （都绝不回落到固定回放图——那会变成"页面拿到了上一张图"的假数据）。冻结帧在 `osu::healthy`
/// 口径下**仍算已发布** ⇒ `mode` 保持 `native`（冻结是有界状态、且仍在发 `state`，回落 tosu
/// 只会造成抖动）；`unhealthy` 才是回落条件。
pub fn live_enabled() -> bool {
    !replay_enabled()
}

/// 当前选中的谱面（实时模式的文件路由用）：`(songs_folder, folder, filename, background)`。
///
/// 全部来自读取线程的**载荷**——没附着/停帧/冻结（且不在身份保持期）⇒ `None` ⇒ 404
/// （**绝不**回落到固定回放图，那会变成"页面拿到了上一张图"的假数据）。
///
/// **Step 9g 的身份保持**：读取线程的 I-06 瞬态失败期间（`IDENTITY_HOLD_GRACE` 内）它发布的是
/// **最后一张好图**的图表块（WS 上页面看到的仍是同一张图的身份）。那段时间这里也供奉
/// **同一份**文件（`RoutePayload::Held`）——理由：页面的一切都键在 identity 上，WS 说"还是这张图"
/// 而文件路由 404 会让页面把 `Request failed with status 404` 原样渲染出来（用户报的那条）。
/// 判据是纯函数 `packet::route_payload`：**只有**"保持期 **且** 被供奉那份与正在发布的图身份
/// 逐字一致"才供奉；窗口过后或没有最后一张好图 ⇒ 404（既有行为）。
fn live_map() -> Option<LiveMap> {
    let reader = crate::osu::instance()?;
    let state = reader.latest();
    // 保持的"陈腐度"：读者每处理一帧都会刷新 `frame_at_ms`；停帧超过宽限窗口 ⇒ 不再供奉
    // （无界的陈旧文件与"改动前的 404"是同一种安全侧）。
    let held_fresh = crate::server::now_ms().saturating_sub(state.frame_at_ms)
        <= crate::osu::invariants::IDENTITY_HOLD_GRACE.as_millis() as u64;
    let payload = crate::osu::packet::route_payload(
        live_enabled(),
        state.frozen,
        state.holding,
        held_fresh,
        state.packet.as_ref(),
        state.held_packet.as_ref(),
    )?;
    let payload = match payload {
        crate::osu::packet::RoutePayload::Current(payload) => payload,
        crate::osu::packet::RoutePayload::Held(held) => held,
    };
    let files = crate::osu::packet::beatmap_files(payload)?;
    Some(LiveMap {
        dir: map_dir_for(&files.songs_folder, &files.folder),
        filename: files.filename,
        background: files.background,
    })
}

/// 实时模式下的谱面目录（`songs \ folder`）。
fn map_dir_for(songs: &str, folder: &str) -> PathBuf {
    if folder.is_empty() {
        PathBuf::from(songs)
    } else {
        PathBuf::from(format!("{songs}\\{folder}"))
    }
}

struct LiveMap {
    dir: PathBuf,
    filename: String,
    background: Option<String>,
}

// ---- 响应 ----

/// 一条入站请求的解析结果（只取路由需要的两项）。
struct RequestHead {
    method: String,
    path: String,
}

/// 解析请求行（HTTP 路径不含空白，`split_whitespace` 足够，无需 URL 解析器）。
fn parse_head(raw: &str) -> Option<RequestHead> {
    let mut lines = raw.lines();
    let request_line = lines.next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let method = parts.next()?.to_string();
    let url = parts.next().unwrap_or("/");
    let path = url.split('?').next().unwrap_or("").to_string();
    Some(RequestHead { method, path })
}

/// Host 门禁：只放行本机 `{host}:24062` / 无 Host 头（复用 24061 的 `is_local_host`，
/// 端口按参数传入 ⇒ DNS rebinding 防护与 24061 同一份实现）。
fn host_allowed(head: &str) -> bool {
    http::is_local_host(head, OSU_COMPAT_PORT)
}

// ---- 响应 ----

/// 状态码 → 原因短语（只覆盖本模块真正会发的码）。
fn status_text(code: u16) -> &'static str {
    match code {
        200 => "OK",
        403 => "Forbidden",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "Unknown",
    }
}

/// **所有**响应统一出口：`Access-Control-Allow-Origin: *` 是硬要求——页面在
/// `http://127.0.0.1:24061` 上跨源取 24062 的谱面与背景图（`coverTheme.js` 的
/// `crossOrigin="anonymous"` + `getImageData`），缺 ACAO 会静默退回默认主题；
/// 403/404/500 同样要带（计划 §3.3）。
fn write_response(stream: &mut TcpStream, code: u16, ctype: &str, body: &[u8]) {
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n",
        code,
        status_text(code),
        ctype,
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
}

fn write_json(stream: &mut TcpStream, code: u16, body: &str) {
    write_response(stream, code, "application/json", body.as_bytes());
}

// ---- 固定谱面的解析结果（进程内只解析一次）----

/// 回放包需要的全部谱面事实（全部来自磁盘上的那一份 `.osu`）。
struct ReplayMap {
    /// `.osu` 的 MD5（小写 32 hex）→ `beatmap.checksum`。实测，不硬编码。
    checksum: String,
    id: u64,
    set: u64,
    artist: String,
    title: String,
    version: String,
    mapper: String,
    first_object: i64,
    last_object: i64,
}

impl ReplayMap {
    /// 兜底：元信息全空（200 照常返回，报文里字段为空/z ero）。
    fn empty() -> Self {
        ReplayMap {
            checksum: String::new(),
            id: 0,
            set: 0,
            artist: String::new(),
            title: String::new(),
            version: String::new(),
            mapper: String::new(),
            first_object: 0,
            last_object: 0,
        }
    }
}

static REPLAY_MAP: OnceLock<ReplayMap> = OnceLock::new();

/// 起点：解析固定谱面；**任何失败都不 panic**（24062 是可选端点，不许拖垮壳）。
/// 解析器出 bug 时记 error 并退化成"元信息全空"（HTTP 面照常服务）。
fn replay_map() -> &'static ReplayMap {
    REPLAY_MAP.get_or_init(|| {
        let path = map_dir().join(MAP_FILE);
        let parsed = path.is_file().then(|| parse_map(&path));
        match parsed {
            Some(Ok(map)) => map,
            Some(Err(e)) => {
                crate::server::log::log_at(
                    "error",
                    &format!("osu compat: replay map parse failed ({e}) — serving zeroed metadata"),
                );
                ReplayMap::empty()
            }
            None => {
                crate::server::log::log_at(
                    "error",
                    &format!("osu compat: replay map not found ({})", path.display()),
                );
                ReplayMap::empty()
            }
        }
    })
}

/// 解析一份 `.osu`：头部字段 + `[Events]` 背景 + `[HitObjects]` 时间窗。
///
/// `firstObject` = 文件顺序第一个 hitobject 的起始时间；`lastObject` = 文件顺序最后一个
/// hitobject 的**结束**时间（spinner = 第 6 段 `endTime`；circle/hold = 起始时间；slider 的
/// 时长推算超出 B1 范围 → 退化为起始时间并记 warn）。
fn parse_map(path: &std::path::Path) -> Result<ReplayMap, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut section = String::new();
    let mut header: Vec<(String, String)> = Vec::new();
    let mut background: Option<String> = None;
    let mut first_object: Option<i64> = None;
    let mut last_object: Option<i64> = None;
    let mut last_is_slider = false;
    for raw in text.lines() {
        let line = raw.trim();
        if line.starts_with('[') && line.ends_with(']') {
            section = line.trim_matches(['[', ']']).to_string();
            continue;
        }
        match section.as_str() {
            "Events" => {
                // 形如 `0,0,"bg.jpg",0,0`：取第 3 段并剥引号。**不能**用 `trim_matches('"')`
                // ——尾部的 `,0,0` 会把结果污染成 `bg.jpg",0,0`，而它仍通过"非空"检查。
                if background.is_none() {
                    if let Some(rest) = line.strip_prefix("0,0,") {
                        let name = rest.trim().trim_start_matches('"');
                        let name = name.split('"').next().unwrap_or("");
                        if !name.is_empty() {
                            background = Some(name.to_string());
                        }
                    }
                }
            }
            "HitObjects" => {
                let parts: Vec<&str> = line.split(',').collect();
                if parts.len() < 4 || parts[0].parse::<i64>().is_err() {
                    continue;
                }
                let Ok(time) = parts[2].parse::<i64>() else {
                    continue;
                };
                let Ok(kind) = parts[3].parse::<i64>() else {
                    continue;
                };
                let end = if kind & 8 != 0 {
                    // spinner：EndTime 在第 6 段
                    parts
                        .get(5)
                        .and_then(|v| v.parse::<i64>().ok())
                        .unwrap_or(time)
                } else {
                    if kind & 2 != 0 {
                        last_is_slider = true;
                    }
                    time
                };
                first_object.get_or_insert(time);
                last_object = Some(end);
            }
            "General" | "Metadata" => {
                if let Some((k, v)) = line.split_once(':') {
                    header.push((k.trim().to_string(), v.trim().to_string()));
                }
            }
            _ => {}
        }
    }
    let get = |key: &str| -> String {
        header
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    };
    let num = |key: &str| -> u64 { get(key).parse::<u64>().unwrap_or(0) };
    // 解析器真跑通的标志：`[Events]` 里必须解出背景名。常量与磁盘不一致时照常服务磁盘
    // 上的那张（`/files/beatmap/background` 本就是这个文件名去读盘），只记 warn。
    let background = background
        .filter(|b| !b.is_empty())
        .ok_or_else(|| "[Events] background: no `0,0,\"<file>\"` line found".to_string())?;
    if background != MAP_BACKGROUND {
        crate::server::log::log_at(
            "warn",
            &format!("osu compat: [Events] background is {background:?}, MAP_BACKGROUND is {MAP_BACKGROUND:?}"),
        );
    }
    if last_is_slider {
        crate::server::log::log_at(
            "warn",
            "osu compat: last hit object is a slider — lastObject falls back to its start time",
        );
    }
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(ReplayMap {
        checksum: crate::server::md5_hex_bytes(&bytes),
        id: num("BeatmapID"),
        set: num("BeatmapSetID"),
        artist: get("Artist"),
        title: get("Title"),
        version: get("Version"),
        mapper: get("Creator"),
        first_object: first_object.unwrap_or(0),
        last_object: last_object.unwrap_or(0),
    })
}

// ---- 回放包 ----

/// 全局帧计数器：第 n 个**推给客户端**的帧 ⇒ `live` 前进 150 ms（并发消费者共享它，
/// 故单个连接看到的步长可能是 150 的整数倍；见 `evidence/B1-replay/B1-notes.md`）。
static REPLAY_TICK: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// 手写 tosu-v2 形状包（字段表 = 计划 §3.3；字符串形状照 P8 真实帧）。
/// `live` 从 `firstObject` 起步、每帧 +150 ms，越过 `lastObject` 后回绕——页面的
/// 时间线/暂停逻辑因此一直被驱动，且 live 始终落在磁盘谱面的时间窗内。
fn replay_packet(map: &ReplayMap) -> String {
    let tick = REPLAY_TICK.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let step = REPLAY_INTERVAL.as_millis() as i64;
    let span = (map.last_object - map.first_object).max(step);
    let live = map.first_object + (tick as i64 * step) % span;
    let direct_file = format!("{}\\{}", MAP_FOLDER, MAP_FILE);
    serde_json::json!({
        "client": "stable",
        "state": { "number": 2, "name": "play" },
        "game": { "paused": false, "focused": true },
        "beatmap": {
            "checksum": map.checksum,
            "id": map.id,
            "set": map.set,
            "artist": map.artist,
            "title": map.title,
            "version": map.version,
            "mapper": map.mapper,
            "time": {
                "live": live,
                "firstObject": map.first_object,
                "lastObject": map.last_object,
                "mp3Length": 0,
            },
        },
        "files": {
            "beatmap": MAP_FILE,
            "background": MAP_BACKGROUND,
            "audio": MAP_AUDIO,
        },
        "directPath": {
            "beatmapFile": direct_file,
            "beatmapBackground": format!("{}\\{}", MAP_FOLDER, MAP_BACKGROUND),
            "beatmapAudio": format!("{}\\{}", MAP_FOLDER, MAP_AUDIO),
            "beatmapFolder": MAP_FOLDER,
        },
        "folders": {
            "game": GAME_DIR,
            "songs": SONGS_DIR,
            "beatmap": MAP_FOLDER,
        },
        "menu": { "mods": serde_json::Value::Null },
        "play": {
            "mods": {
                "checksum": crate::server::md5_hex(r#"[{"acronym":"DT"}]"#),
                "number": 64,
                "name": "DT",
                "array": [{ "acronym": "DT" }],
                "rate": 1.5,
            },
            "hits": { "0": 0, "50": 0, "100": 1, "300": 42, "geki": 7, "katu": 0 },
            "combo": 50,
            "score": 100000,
            "accuracy": 0.98,
        },
        "resultsScreen": {
            "hits": { "0": 0, "50": 0, "100": 0, "300": 0, "geki": 0, "katu": 0 },
            "mods": {
                "checksum": "",
                "number": 0,
                "name": "",
                "array": [],
                "rate": 1,
            },
        },
    })
    .to_string()
}

// ---- WS ----

fn is_timeout(err: &tungstenite::Error) -> bool {
    matches!(
        err,
        tungstenite::Error::Io(e)
            if e.kind() == std::io::ErrorKind::WouldBlock
                || e.kind() == std::io::ErrorKind::TimedOut
    )
}

/// 带 loopback Origin 校验的握手（与 24061 的 `ws.rs` 同一份判定）。
fn accept_loopback_ws(stream: TcpStream) -> Option<tungstenite::WebSocket<TcpStream>> {
    tungstenite::accept_hdr(
        stream,
        |req: &tungstenite::handshake::server::Request,
         resp: tungstenite::handshake::server::Response| {
            let origin_ok = req
                .headers()
                .get("Origin")
                .map(|v| ws::is_loopback_origin(v.to_str().unwrap_or("")))
                .unwrap_or(true);
            if origin_ok {
                Ok(resp)
            } else {
                Err(tungstenite::http::Response::builder()
                    .status(403)
                    .body(Some("forbidden".to_string()))
                    .unwrap())
            }
        },
    )
    .ok()
}

/// `/websocket/v2`：每 150 ms 推一帧；入站消息一律忽略；Close 干净收场。
///
/// - 默认（实时）：读取线程的最新载荷；**`None` 时这一帧不发**
///   （未附着/`unhealthy` 不许出帧——这就是"页面无假数据"的实现面）
/// - `MMA_OSU_COMPAT_REPLAY=1`（回放）：固定谱面的手写包（B1 行为，逐字节不变）
fn handle_v2_ws(stream: TcpStream) {
    let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
    let Some(mut ws) = accept_loopback_ws(stream) else {
        return;
    };
    let map = replay_map();
    let live = live_enabled();
    let mut last_push = Instant::now();
    loop {
        if last_push.elapsed() >= REPLAY_INTERVAL {
            last_push = Instant::now();
            let payload = if live {
                match crate::osu::instance().and_then(|reader| reader.packet()) {
                    Some(packet) => packet.to_string(),
                    None => continue,
                }
            } else {
                replay_packet(map)
            };
            if ws.send(tungstenite::Message::Text(payload)).is_err() {
                return;
            }
        }
        match ws.read() {
            Ok(tungstenite::Message::Close(_)) => return,
            Ok(_) => {}
            Err(e) if is_timeout(&e) => {}
            Err(_) => return,
        }
    }
}

/// `/websocket/commands`：**黑洞** —— 接受握手、丢弃全部入站、永不回包。页面每次
/// identity/mod 变化都发 `getSettings`（`socket.js:65-97`），不回包是设计的一部分：
/// 设置的权威保持"壳 settings 帧 / tosu 设置文件"单源。连接保持打开直到对端关闭。
fn handle_commands_ws(stream: TcpStream) {
    let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
    let Some(mut ws) = accept_loopback_ws(stream) else {
        return;
    };
    loop {
        match ws.read() {
            Ok(tungstenite::Message::Close(_)) => return,
            Ok(_) => {}
            Err(e) if is_timeout(&e) => {}
            Err(_) => return,
        }
    }
}

// ---- HTTP ----

/// 24062 的 HTTP 面（§3.3 端点路径表）：Host 门禁 → 两条文件路由 → 其余 404。
///
/// - 默认（实时）：供奉**当前选中的谱面**；没有当前谱面（未附着/冻结/缺路径）⇒ 404
///   （不是回落到固定图 —— 那会让页面拿到上一张图）
/// - `MMA_OSU_COMPAT_REPLAY=1`（回放）：读固定谱面目录下的文件；读失败 = 500
///   （**不是** 404：路由命中过）
///
/// 路由表直接匹配 `frames::OSU_COMPAT_{FILE,BACKGROUND}_ROUTE`：契约公布的 `filesPath`
/// 与实现是同一来源。
fn handle_http(stream: &mut TcpStream, head: &str, method: &str, path: &str) {
    if !host_allowed(head) {
        write_json(stream, 403, r#"{"error":"forbidden host"}"#);
        return;
    }
    if method != "GET" {
        write_json(stream, 405, r#"{"error":"method not allowed"}"#);
        return;
    }
    let live = live_enabled();
    let current = if live { live_map() } else { None };
    let (dir, file, ctype) = match path {
        OSU_COMPAT_FILE_ROUTE => {
            let (dir, file) = match &current {
                Some(map) => (map.dir.clone(), map.filename.clone()),
                None if live => {
                    write_json(stream, 404, r#"{"error":"no current beatmap"}"#);
                    return;
                }
                None => (map_dir(), MAP_FILE.to_string()),
            };
            (dir, file, "text/plain; charset=utf-8".to_string())
        }
        OSU_COMPAT_BACKGROUND_ROUTE => {
            let (dir, file) = match &current {
                Some(map) => match map.background.as_deref() {
                    Some(background) => (map.dir.clone(), background.to_string()),
                    None => {
                        write_json(stream, 404, r#"{"error":"no background"}"#);
                        return;
                    }
                },
                None if live => {
                    write_json(stream, 404, r#"{"error":"no current beatmap"}"#);
                    return;
                }
                None => (map_dir(), MAP_BACKGROUND.to_string()),
            };
            let ctype = crate::server::mime_for(&file);
            (dir, file, ctype)
        }
        _ => {
            write_json(stream, 404, r#"{"error":"not found"}"#);
            return;
        }
    };
    let full = dir.join(&file);
    match std::fs::read(&full) {
        Ok(bytes) => {
            let resolved_ctype = if ctype == "application/octet-stream" {
                if bytes.starts_with(b"\xFF\xD8\xFF") {
                    "image/jpeg".to_string()
                } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
                    "image/png".to_string()
                } else if bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
                    "image/webp".to_string()
                } else {
                    ctype
                }
            } else {
                ctype
            };
            write_response(stream, 200, &resolved_ctype, &bytes)
        }
        Err(e) => {
            crate::server::log::log_at(
                "error",
                &format!("osu compat: cannot read {} ({e})", full.display()),
            );
            write_json(stream, 500, r#"{"error":"file read failed"}"#);
        }
    }
}

/// 一条连接：先 peek 探 WS 升级（照 24061 的 `http::probe_is_ws`），否则按普通 HTTP
/// 读完整请求。**任何分支都不 panic**：解析失败直接关连接。
fn handle_conn(mut stream: TcpStream) {
    if http::probe_is_ws(&stream) {
        // WS 不能在 accept 前消费请求字节 ⇒ Host 门禁落在 accept 回调的 Origin 校验上
        // （rebinding 场景下 Host 与 Origin 必然一起跨源）。这里只 peek。
        let raw = peek_head(&stream);
        if let Some(req) = raw.as_deref().and_then(parse_head) {
            match (req.method.as_str(), req.path.as_str()) {
                ("GET", OSU_COMPAT_WS_PATH) => return handle_v2_ws(stream),
                ("GET", "/websocket/commands") => return handle_commands_ws(stream),
                _ => {
                    write_json(&mut stream, 404, r#"{"error":"not found"}"#);
                    return;
                }
            }
        }
        return;
    }
    let Some((head, _body)) = http::read_request(&mut stream) else {
        return;
    };
    let Some(req) = parse_head(&head) else { return };
    handle_http(&mut stream, &head, &req.method, &req.path);
}

/// peek 出已到达的请求头（Upgrade 请求第一包必然含完整 head；只 peek 不消费）。
fn peek_head(stream: &TcpStream) -> Option<String> {
    let mut probe = [0u8; 4096];
    for _ in 0..250 {
        match stream.peek(&mut probe) {
            Ok(0) => thread::sleep(Duration::from_millis(20)),
            Ok(n) => return Some(String::from_utf8_lossy(&probe[..n]).to_string()),
            Err(_) => return None,
        }
    }
    None
}

// ---- 入口 ----

/// 起 24062 监听器（由 `server::start` 调用，与 `spawn_http_ws`/`spawn_post`/`spawn_bridge`
/// 同形）。**自己 bind**：绑定失败不 panic、不 exit，只记 error 日志 + 入 `state.errors`
/// （页面 status 行可见），照 24060 / 17653 的先例（24061 才是 exit(2) 的那一个）。
pub fn spawn_osu_compat(shared: Arc<Shared>) {
    let listener = match TcpListener::bind(("127.0.0.1", OSU_COMPAT_PORT)) {
        Ok(l) => l,
        Err(e) => {
            crate::server::log::log_at(
                "error",
                &format!(
                    "mma-shell: cannot bind {} ({e}) — the osu! compatible origin is unavailable",
                    OSU_COMPAT_PORT
                ),
            );
            shared
                .shell_errors
                .lock()
                .unwrap()
                .push("osu! 兼容端点端口 24062 被占用，原生传输不可用".to_string());
            return;
        }
    };
    // `shared` 只在绑定失败那一支用到——成功路径不持有它（本模块不发壳帧）。
    drop(shared);
    // 绑定成功才置位：`sources.osu` 的 native 端点只在 24062 真的可用时下发。
    BOUND.store(true, Ordering::Relaxed);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            thread::spawn(move || handle_conn(stream));
        }
    });
}
