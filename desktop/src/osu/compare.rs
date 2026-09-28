// 影子对拍的 **IO 驱动**（**dev-only**，本步的验收仪器）。
//
// 分工（B3 重构）：比较规则在 `osu/shadow.rs`（纯函数，独立单测）；本文件只负责
// ① 连 tosu 取载荷（tungstenite WS 客户端线程）② 每 tick 调 `record()` 落一条 JSONL。
//
// 关开关（不重编译）：环境变量 `MMA_OSU_COMPARE=0` 关闭整套装置；
// `MMA_OSU_COMPARE_INTERVAL_MS` 覆盖对拍节奏（默认 **100 ms**，夹取 20..=5000）。
//
// 口径说明（**必须照记**）：
// - `play` 态下 tosu 的 mod 来自它的局内链（ScoreV2 位 XOR 还原），本步**没有**实现那条链
//   ⇒ 局内 `mod_signature` 记为 `"skipped:play-state-mods-chain-not-implemented"`
//   （C2/Step 8 落地局内链后转为比较）。`state_name` / `checksum` / `identity` /
//   `first_object` / `last_object` / `songs_folder` 在**所有**状态都比较。
// - `client` 一格按"**焦点客户端**"口径跳过（P1-notes F1：tosu 的 `/json/v2` 只服务前台
//   那个客户端，两侧不等是环境事实、不是读数错误）。
// - 跳过的格子在 JSONL 里是字符串 `"skipped:<why>"`，**不是** `false`（B2 的教训）。

use crate::osu::model::{Client, Snapshot};
use crate::osu::shadow::{self, OurShadow, TosuShadow};
use serde_json::{json, Map, Value};
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// 对拍装置的开关：`MMA_OSU_COMPARE=0` 关闭（其它值/未设置 = 开启）。
pub fn enabled() -> bool {
    match std::env::var("MMA_OSU_COMPARE") {
        Ok(value) => value.trim() != "0",
        Err(_) => true,
    }
}

/// 默认落盘的 tosu 载荷白名单（**C1 变更**）。
///
/// 为什么要有它：B3 的真机会话是 2271 条 / **37 MB**，而其中
/// `performance.graph.series`（逐帧星数曲线）一条就占 ~8 KB——对拍从不比较它。
/// 默认只留"被比较的字段 + 一小撮诊断字段"（下面这张表逐条给出理由），
/// 需要整包时用 `MMA_OSU_COMPARE_FULL=1` 恢复今天的行为。
///
/// 保留项与理由：
/// - `client`/`state`/`beatmap`/`files`/`directPath`/`folders`：`shadow.rs` 的比较面（必需）
/// - `menu`/`play`/`resultsScreen`：mod 码集合的来源（`play` 态另有 hits，livePP 相关的证据）
/// - `game`：`game.paused` 是停滞推导的对照面（B2/B3 的对拍口径都引用它）
/// - `server`/`session`/`profile`/`leaderboard`：**小**（合计 <1 KB/条）且 `beatmap.md5` /
///   `state.name` 的兜底链在 tosu 侧就长在这里（页面 `socketHandlers.js` 的回退读取）
const KEPT_TOP_LEVEL: &[&str] = &[
    "client",
    "state",
    "beatmap",
    "files",
    "directPath",
    "folders",
    "menu",
    "play",
    "resultsScreen",
    "game",
    "server",
    "session",
    "profile",
    "leaderboard",
];

/// 大型诊断块（**默认丢弃**，`MMA_OSU_COMPARE_FULL=1` 时原样保留）：
/// `performance`（逐帧星数曲线，~8 KB/条）、`settings`（tosu 全量设置字典，~1.5 KB/条）、
/// `tourney`（锦标赛客户端列表，~200 B/条；仅 mod 候选来源）。它们既不参与比较，
/// 也不被证据分析引用，却占了 2271 条会话 37 MB 里的 ~80%。
///
/// 整包模式：`MMA_OSU_COMPARE_FULL=1`（其它值 = 默认的裁剪模式）。
pub fn full_payload() -> bool {
    match std::env::var("MMA_OSU_COMPARE_FULL") {
        Ok(value) => value.trim() == "1",
        Err(_) => false,
    }
}

/// **纯函数**：把 tosu 载荷裁成"被比较的字段 + 诊断子集"（默认落盘形态）。
///
/// 非对象（`null`/数组/标量）原样返回；对象按 [`KEPT_TOP_LEVEL`] 投影，
/// 并额外放一个 `_projected: true` 标记 + `_dropped_keys` 名单——**绝不能**让读者
/// 把"被裁掉的键"误读成"tosu 没发这个键"（那会把设备事实读成读数缺陷）。
pub fn project_payload(payload: &Value) -> Value {
    let Some(map) = payload.as_object() else {
        return payload.clone();
    };
    let mut out = Map::new();
    for key in KEPT_TOP_LEVEL {
        if let Some(value) = map.get(*key) {
            out.insert((*key).to_string(), value.clone());
        }
    }
    out.insert("_projected".to_string(), json!(true));
    out.insert(
        "_dropped_keys".to_string(),
        json!(map
            .keys()
            .filter(|key| !KEPT_TOP_LEVEL.contains(&key.as_str()))
            .cloned()
            .collect::<Vec<String>>()),
    );
    Value::Object(out)
}

/// 影子比对节奏：默认 **100 ms（10 Hz）**；`MMA_OSU_COMPARE_INTERVAL_MS` 覆盖。
///
/// 夹取到 `20..=5000` ms：低于 20 ms 会让 WS 客户端与内存 tick 抢同一段时间片，
/// 高于 5 s 就失去"跨传输键同刻相等"的意义。**注意**：读取线程的**内存** tick 仍是
/// `mod::TICK`（250 ms）——本间隔只改对拍记录频率，不改发布节奏（那是 Step 8 的事）。
pub fn interval() -> Duration {
    let millis = std::env::var("MMA_OSU_COMPARE_INTERVAL_MS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(|value| value.clamp(20, 5_000))
        .unwrap_or(100);
    Duration::from_millis(millis)
}

/// tosu 端点（`host:port`）。默认 24050（tosu 默认端口）；`MMA_OSU_TOSU` 覆盖。
fn tosu_host_port() -> String {
    std::env::var("MMA_OSU_TOSU")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "127.0.0.1:24050".to_string())
}

/// `ws://{host}:{port}/websocket/v2?l=<label>`。
pub fn tosu_ws_url() -> String {
    format!(
        "ws://{}/websocket/v2?l=ManiaMapAnalyser%20by%20Leo_Black",
        tosu_host_port()
    )
}

/// 证据文件落点：`MMA_OSU_COMPARE_OUT` 覆盖 → 否则 `<workspace>/temp/osu-native-memory/
/// evidence/B3-shadow/samples.jsonl` → 否则系统临时目录。**只写文件，不改产品状态。**
pub fn samples_path() -> PathBuf {
    if let Ok(over) = std::env::var("MMA_OSU_COMPARE_OUT") {
        if !over.trim().is_empty() {
            return PathBuf::from(over);
        }
    }
    if let Ok(ws) = std::env::var("MMA_WORKSPACE_ROOT") {
        if !ws.trim().is_empty() {
            return PathBuf::from(ws)
                .join("temp/osu-native-memory/evidence/B3-shadow/samples.jsonl");
        }
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let candidate = cwd.join("temp/osu-native-memory/evidence/B3-shadow/samples.jsonl");
    if candidate.parent().map(|p| p.is_dir()).unwrap_or(false)
        || cwd.join("desktop").is_dir()
        || cwd.join("temp").is_dir()
    {
        return candidate;
    }
    std::env::temp_dir().join("samples.jsonl")
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// tosu 最新载荷（WS 客户端的产物）。
#[derive(Default)]
pub struct TosuFeed {
    latest: Mutex<Option<Value>>,
    connected: Mutex<bool>,
    last_error: Mutex<Option<String>>,
    frames: Mutex<u64>,
}

impl TosuFeed {
    pub fn latest(&self) -> Option<Value> {
        self.latest.lock().unwrap().clone()
    }

    pub fn connected(&self) -> bool {
        *self.connected.lock().unwrap()
    }

    pub fn last_error(&self) -> Option<String> {
        self.last_error.lock().unwrap().clone()
    }

    pub fn frames(&self) -> u64 {
        *self.frames.lock().unwrap()
    }

    fn set(&self, value: Value) {
        *self.frames.lock().unwrap() += 1;
        *self.latest.lock().unwrap() = Some(value);
    }
}

/// 起 tosu WS 客户端线程（断线 2s 重连；读超时 250ms 以免线程卡死无法退出）。
pub fn spawn_tosu_client(feed: Arc<TosuFeed>) {
    thread::spawn(move || loop {
        let url = tosu_ws_url();
        match tungstenite::connect(url.as_str()) {
            Ok((mut socket, _response)) => {
                *feed.connected.lock().unwrap() = true;
                *feed.last_error.lock().unwrap() = None;
                eprintln!("[osu] compare: connected to {url}");
                if let tungstenite::stream::MaybeTlsStream::Plain(stream) = socket.get_ref() {
                    let _ = stream.set_read_timeout(Some(Duration::from_millis(250)));
                }
                loop {
                    match socket.read() {
                        Ok(tungstenite::Message::Text(text)) => {
                            match serde_json::from_str::<Value>(&text) {
                                Ok(value) => feed.set(value),
                                Err(e) => {
                                    *feed.last_error.lock().unwrap() =
                                        Some(format!("bad json: {e}"));
                                }
                            }
                        }
                        Ok(tungstenite::Message::Close(_)) => break,
                        Ok(_) => {}
                        Err(tungstenite::Error::Io(e))
                            if e.kind() == std::io::ErrorKind::WouldBlock
                                || e.kind() == std::io::ErrorKind::TimedOut => {}
                        Err(e) => {
                            *feed.last_error.lock().unwrap() = Some(format!("{e}"));
                            break;
                        }
                    }
                }
                *feed.connected.lock().unwrap() = false;
            }
            Err(e) => {
                *feed.connected.lock().unwrap() = false;
                *feed.last_error.lock().unwrap() = Some(format!("connect: {e}"));
            }
        }
        thread::sleep(Duration::from_secs(2));
    });
}

/// 保留的兼容入口（B2 的证据工具按这个形状取字段）；现在它只是 `packet_from_snapshot`
/// 的薄包装，**比较本身走 `shadow`**。
pub fn fields_from_payload(payload: &Value, client: Option<Client>) -> Value {
    let _ = client;
    payload.clone()
}

/// 我们的快照 + tosu 帧 → 一条对拍记录。
///
/// 结构（JSONL 每行一条）：
/// - `ts`：毫秒时间戳
/// - `our`：我方载荷 + `reason`（`reason` = 壳侧原因字面量，`None` = 正常发布）
/// - `our_meta`：只在对拍侧出现的诊断（`.osu` 解析结果、头字段交叉校验、songs 原始 cfg 值）
/// - `tosu`：tosu 原始载荷（`null` = 还没收到帧）。**默认是投影后的形态**
///   （`_projected: true` + `_dropped_keys`），`MMA_OSU_COMPARE_FULL=1` 时才是整包。
/// - `cmp`：逐字段 `true` / `false` / `"skipped:<why>"`
/// - `diff`：**只列**不等/跳过的格子（带两侧原始值）
/// - `mods`：两侧 mod 代码集合与是否相等
pub fn record(snapshot: &Snapshot, reason: Option<String>, tosu: Option<&Value>) -> Value {
    let ours = OurShadow::from_snapshot(snapshot);
    match tosu {
        Some(payload) => {
            // 比较永远走**原始载荷**；只有落盘形态受 `MMA_OSU_COMPARE_FULL` 影响
            // （投影不参与判定，否则"少了一个键"会变成"数据变了"）。
            let theirs = TosuShadow::from_payload(payload);
            let diff = shadow::diff(&ours, &theirs);
            let mut our = snapshot.to_packet();
            if let Some(map) = our.as_object_mut() {
                map.insert("reason".to_string(), json!(reason));
            }
            let tosu_value = if full_payload() {
                payload.clone()
            } else {
                project_payload(payload)
            };
            json!({
                "ts": now_ms(),
                "our": our,
                "our_meta": our_meta_json(snapshot),
                "tosu": tosu_value,
                "cmp": diff.to_json(),
                "diff": diff.detail_json(),
                "mods": {
                    "ours": diff.our_mod_codes,
                    "theirs": diff.their_mod_codes,
                    "equal": diff.mod_codes_equal(),
                },
            })
        }
        None => {
            let mut cmp = Map::new();
            let mut diff = Map::new();
            for field in [
                "client",
                "state_name",
                "checksum",
                "identity",
                "mod_signature",
                "first_object",
                "last_object",
                "songs_folder",
            ] {
                cmp.insert(field.to_string(), json!("skipped:no-tosu-frame"));
                diff.insert(
                    field.to_string(),
                    json!({ "our": Value::Null, "tosu": Value::Null, "verdict": "skipped", "why": "no-tosu-frame" }),
                );
            }
            let mut our = snapshot.to_packet();
            if let Some(map) = our.as_object_mut() {
                map.insert("reason".to_string(), json!(reason));
            }
            json!({
                "ts": now_ms(),
                "our": our,
                "our_meta": our_meta_json(snapshot),
                "tosu": Value::Null,
                "cmp": Value::Object(cmp),
                "diff": Value::Object(diff),
                "mods": { "ours": ours.mod_codes, "theirs": Vec::<&str>::new(), "equal": Value::Null },
            })
        }
    }
}

/// 只在证据侧出现的我方诊断（不进发布载荷）：
/// - `songs_cfg_value`：内存里的**原始** cfg 值（`"Songs"`，名字不是路径）
/// - `osu_file`：`.osu` 解析结果（C6 的两个值 + 逐对象计账 + 覆盖率）
/// - `osu_file_mismatches`：头字段与内存值不一致的字段名（空 = 全等）
fn our_meta_json(snapshot: &Snapshot) -> Value {
    let mut out = Map::new();
    out.insert(
        "songs_cfg_value".to_string(),
        json!(snapshot.songs_cfg_value),
    );
    out.insert(
        "beatmap_file_path".to_string(),
        json!(snapshot.beatmap_file_path),
    );
    out.insert(
        "beatmap_file_error".to_string(),
        json!(snapshot.beatmap_file_error),
    );
    out.insert(
        "beatmap_file_mismatches".to_string(),
        json!(snapshot.beatmap_file_mismatches),
    );
    match snapshot.beatmap_file.as_ref() {
        Some(file) => {
            out.insert(
                "osu_file".to_string(),
                json!({
                    "first_object": file.first_object(),
                    "last_object": file.last_object(),
                    // 诊断：旧口径 `max(endTime)`——与 `last_object` 的差就是"长条压过末对象"的量。
                    "max_end_time": file.max_end_time(),
                    "min_start_time": file.first_last.min_start_time,
                    "object_count": file.first_last.object_count,
                    "circles": file.first_last.circles,
                    "sliders": file.first_last.sliders,
                    "spinners": file.first_last.spinners,
                    "holds": file.first_last.holds,
                    "sliders_uncomputed": file.first_last.sliders_uncomputed,
                    "slider_pixel_total": file.first_last.slider_pixel_total,
                    "slider_coverage": file.slider_coverage.map(|c| c.as_str()),
                    "slider_multiplier": file.slider_multiplier,
                    "timing_points": file.timing_points.len(),
                    "header": {
                        "title": file.header.title,
                        "artist": file.header.artist,
                        "creator": file.header.creator,
                        "version": file.header.version,
                        "beatmap_id": file.header.beatmap_id,
                        "beatmap_set_id": file.header.beatmap_set_id,
                    },
                }),
            );
        }
        None => {
            out.insert("osu_file".to_string(), Value::Null);
        }
    }
    Value::Object(out)
}

/// JSONL 追加器（进程内唯一写者；首次写入时建目录）。
pub struct SampleWriter {
    path: PathBuf,
    ready: bool,
}

impl SampleWriter {
    pub fn new(path: PathBuf) -> SampleWriter {
        SampleWriter { path, ready: false }
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    pub fn write(&mut self, record: &Value) {
        use std::io::Write;
        if !self.ready {
            if let Some(parent) = self.path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            // 每次运行都从空文件开始：追加到旧运行会让"首个样本"与计数不可解释。
            let _ = fs::remove_file(&self.path);
            self.ready = true;
        }
        let Ok(mut file) = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        else {
            return;
        };
        let _ = writeln!(file, "{record}");
    }
}

#[cfg(test)]
#[path = "../../tests-local/osu_compare.rs"]
mod tests_compare;
