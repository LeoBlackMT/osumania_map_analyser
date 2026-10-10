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
// - **Step 8d 修正**：`mod_signature` 一格的判据改为"**页面可见的 mod 码集合**"
//   （`shadow::mod_signature_field`：按各侧自己的 `state.name` 选候选槽 ⇒ 游玩态只看
//   `play.mods`，其余状态并集 `play.mods`+`menu.mods`+`resultsScreen.mods`），
//   且任一侧没有 mods 对象时记**跳过**（`frozen-field-level` / `our-gate-only` /
//   `tosu-mods-absent`）。旧判据（比两侧的签名串、缺对象记 `false`）已在 C2 矩阵会话里
//   产出 6789 条假差异，详见 `temp/osu-native-memory/evidence/C2-stable-full/C2-matrix-verdict.md`。
//   逐槽存在性与缓存键签名串只作**诊断**落进 `mods.by_key` / `mods.signature`。
// - `client` 一格按"**焦点客户端**"口径跳过（P1-notes F1：tosu 的 `/json/v2` 只服务前台
//   那个客户端，两侧不等是环境事实、不是读数错误）。
// - 跳过的格子在 JSONL 里是字符串 `"skipped:<why>"`，**不是** `false`（B2 的教训）。

use crate::osu::invariants::{FrameAction, FrameOutcome};
use crate::osu::model::{Client, Reason, Snapshot};
use crate::osu::shadow::{self, OurShadow, TosuShadow, Verdict};
use crate::osu::stable;
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

/// 影子比对诊断报告（P2 Topic 12：供前端页面与诊断端点读取）
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct ShadowDiagnostics {
    pub enabled: bool,
    pub tosu_connected: bool,
    pub total_compared: u64,
    pub matched_frames: u64,
    pub mismatched_frames: u64,
    pub match_rate: f64,
    pub per_field_diffs: std::collections::HashMap<String, u64>,
    pub last_mismatches: Vec<String>,
    pub last_cmp: Option<Value>,
    pub last_diff: Option<Value>,
}

static LIVE_SHADOW_DIAG: Mutex<Option<ShadowDiagnostics>> = Mutex::new(None);

pub fn current_diagnostics() -> ShadowDiagnostics {
    let mut diag = LIVE_SHADOW_DIAG.lock().unwrap().clone().unwrap_or_default();
    diag.enabled = enabled();
    if diag.total_compared > 0 {
        diag.match_rate = (diag.matched_frames as f64 / diag.total_compared as f64) * 100.0;
    }
    diag
}

pub fn reset_diagnostics() {
    let mut guard = LIVE_SHADOW_DIAG.lock().unwrap();
    if let Some(ref mut d) = *guard {
        d.total_compared = 0;
        d.matched_frames = 0;
        d.mismatched_frames = 0;
        d.match_rate = 0.0;
        d.per_field_diffs.clear();
        d.last_mismatches.clear();
        d.last_cmp = None;
        d.last_diff = None;
    }
}

pub fn set_tosu_connected(connected: bool) {
    let mut guard = LIVE_SHADOW_DIAG.lock().unwrap();
    let diag = guard.get_or_insert_with(ShadowDiagnostics::default);
    diag.tosu_connected = connected;
}

fn update_diagnostics(diff: &shadow::ShadowDiff) {
    let mut guard = LIVE_SHADOW_DIAG.lock().unwrap();
    let diag = guard.get_or_insert_with(ShadowDiagnostics::default);
    diag.enabled = enabled();
    diag.tosu_connected = true;
    diag.total_compared += 1;

    let differs: Vec<String> = diff
        .fields
        .iter()
        .filter(|f| f.verdict.is_differ())
        .map(|f| f.field.to_string())
        .collect();

    if differs.is_empty() {
        diag.matched_frames += 1;
    } else {
        diag.mismatched_frames += 1;
        for field in &differs {
            *diag.per_field_diffs.entry(field.clone()).or_insert(0) += 1;
        }
        diag.last_mismatches = differs;
        diag.last_cmp = Some(diff.to_json());
        diag.last_diff = Some(diff.detail_json());
    }
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
                set_tosu_connected(true);
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
                set_tosu_connected(false);
            }
            Err(e) => {
                *feed.connected.lock().unwrap() = false;
                set_tosu_connected(false);
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

/// 我们的快照 + 门输出 + 链诊断 + tosu 帧 → 一条对拍记录。
///
/// 结构（JSONL 每行一条）：
/// - `ts`：毫秒时间戳
/// - `our`：我方**载荷**（`Publish` = 全量；字段级冻结 = 只有 `client`+`state`；
///   `unhealthy` = 空对象）+ `reason` + `health`/`frozen`/`strikes`/`degraded_fields`
/// - `our_meta`：只在对拍侧出现的诊断（`.osu` 解析、链跳中间值、hits 候选槽、
///   结算屏自证字段）
/// - `tosu`：tosu 原始载荷（`null` = 还没收到帧）。**默认是投影后的形态**
///   （`_projected: true` + `_dropped_keys`），`MMA_OSU_COMPARE_FULL=1` 时才是整包。
/// - `cmp`：逐字段 `true` / `false` / `"skipped:<why>"`
/// - `diff`：**只列**不等/跳过的格子（带两侧原始值）
/// - `mods`：两侧 mod 代码集合与是否相等
///
/// C2 起 `diff` 新增 `play_hits` / `result_hits`（6 键称重串）与
/// `mods_mask`（三态掩码），并把 `hits_candidates`（8 个原始 u16）落进 `our_meta`
/// ——这是 `play.hits` 键映射的**唯一**关闭手段（计划 Step 8 的硬要求）。
pub fn record(
    snapshot: &Snapshot,
    outcome: &FrameOutcome,
    probe: Option<&stable::ChainProbe>,
    result: Option<&stable::ResultRead>,
    tosu: Option<&Value>,
) -> Value {
    let frozen = matches!(
        outcome.action,
        FrameAction::FreezeStateOnly | FrameAction::HoldLastGood
    );
    let ours = OurShadow::from_snapshot_with(snapshot, outcome);
    let our_packet = if outcome.action == FrameAction::Stop {
        json!({})
    } else if frozen {
        snapshot.to_frozen_packet()
    } else {
        snapshot.to_packet()
    };
    let tosu_value = tosu.map(|payload| {
        if full_payload() {
            payload.clone()
        } else {
            project_payload(payload)
        }
    });
    let (cmp, diff, mods) = match tosu {
        // 比较永远走**原始载荷**；只有落盘形态受 `MMA_OSU_COMPARE_FULL` 影响
        // （投影不参与判定，否则"少了一个键"会变成"数据变了"）。
        Some(payload) => {
            let theirs = TosuShadow::from_payload(payload);
            let diff = shadow::diff(&ours, &theirs);
            update_diagnostics(&diff);
            // `mods` 段：判据 = `cmp.mod_signature`（页面可见码集合）；`equal` 是它的
            // 布尔投影（`null` = 跳过 ⇒ **没有**可比的 mods 对象，别读成"不相等"）。
            // `by_key`/`signature` 是**诊断**（Step 8d：位置不同 vs 码不同必须分得开）。
            let verdict = diff
                .field("mod_signature")
                .map(|field| field.verdict.clone())
                .unwrap_or(Verdict::Skipped {
                    why: "field-missing".to_string(),
                });
            let mods = json!({
                "equal": match &verdict {
                    Verdict::Equal => Some(true),
                    Verdict::Differ { .. } => Some(false),
                    Verdict::Skipped { .. } => None,
                },
                "verdict": verdict.to_json(),
                "ours": diff.our_mod_codes,
                "theirs": diff.their_mod_codes,
                "by_key": {
                    "ours": diff.our_page_mods.to_json(),
                    "tosu": diff.their_page_mods.to_json(),
                },
                "signature": {
                    "ours": diff.our_signature,
                    "tosu": diff.their_signature,
                },
            });
            (diff.to_json(), diff.detail_json(), mods)
        }
        None => {
            let mut cmp = Map::new();
            let mut diff = Map::new();
            for field in shadow::COMPARED_FIELDS {
                cmp.insert(field.to_string(), json!("skipped:no-tosu-frame"));
                diff.insert(
                    field.to_string(),
                    json!({ "our": Value::Null, "tosu": Value::Null, "verdict": "skipped", "why": "no-tosu-frame" }),
                );
            }
            (
                Value::Object(cmp),
                Value::Object(diff),
                json!({
                    "equal": Value::Null,
                    "verdict": "skipped:no-tosu-frame",
                    "ours": ours.page_mods.codes,
                    "theirs": Vec::<&str>::new(),
                    "by_key": { "ours": ours.page_mods.to_json(), "tosu": Value::Null },
                    "signature": { "ours": ours.mod_signature, "tosu": Value::Null },
                }),
            )
        }
    };
    let mut our = our_packet;
    if let Some(map) = our.as_object_mut() {
        map.insert("reason".to_string(), json!(outcome.reason.as_ref().map(Reason::as_str)));
        map.insert("health".to_string(), json!(outcome.state.map(|s| s.as_str())));
        map.insert("frozen".to_string(), json!(frozen));
        // Step 9g：动作字面量（`publish` / `freeze-state-only` / `hold-last-good` / `stop`）。
        // 保持帧与冻结帧在**本记录**里同形（都只有 `client`+`state`）：对拍侧手里只有本帧
        // 快照，重建不出"上一份验证过的块"（见 `shadow.rs` 的同名注释）。这个字面量是
        // 算子区分两者的唯一线索；页面实际收到的那份载荷由 `ReaderState.holding` 面承担。
        map.insert("action".to_string(), json!(outcome.action.as_str()));
        map.insert("degraded_fields".to_string(), json!(outcome.degraded_fields));
    }
    json!({
        "ts": now_ms(),
        "our": our,
        "our_meta": our_meta_json(snapshot, probe, result),
        "tosu": tosu_value,
        "cmp": cmp,
        "diff": diff,
        "mods": mods,
    })
}

/// 只在证据侧出现的我方诊断（不进发布载荷）：
/// - `songs_cfg_value`：内存里的**原始** cfg 值（`"Songs"`，名字不是路径）
/// - `osu_file`：`.osu` 解析结果（C6 的两个值 + 逐对象计账 + 覆盖率）
/// - `osu_file_mismatches`：头字段与内存值不一致的字段名（空 = 全等）
/// - `chain`：规则集/局内/结算三条链的中间值（C2；含"多一跳"解释的对照值）
/// - `hits_candidates` / `result_hits_candidates`：8 个原始 u16（键映射的关闭手段）
/// - `result`：结算屏的自证字段（score/maxCombo/playerName/onlineId）
fn our_meta_json(
    snapshot: &Snapshot,
    probe: Option<&stable::ChainProbe>,
    result: Option<&stable::ResultRead>,
) -> Value {
    let mut out = Map::new();
    out.insert("songs_cfg_value".to_string(), json!(snapshot.songs_cfg_value));
    out.insert("beatmap_file_path".to_string(), json!(snapshot.beatmap_file_path));
    out.insert("beatmap_file_error".to_string(), json!(snapshot.beatmap_file_error));
    out.insert("beatmap_file_md5".to_string(), json!(snapshot.beatmap_file_md5));
    out.insert(
        "beatmap_file_mismatches".to_string(),
        json!(snapshot.beatmap_file_mismatches),
    );
    out.insert("game_folder".to_string(), json!(snapshot.game_folder));
    out.insert("files_background".to_string(), json!(snapshot.background));
    out.insert("files_audio".to_string(), json!(snapshot.audio));
    out.insert("mp3_length".to_string(), json!(snapshot.mp3_length));
    out.insert("play_time".to_string(), json!(snapshot.play_time));
    out.insert("paused".to_string(), json!(snapshot.paused));
    out.insert("retries".to_string(), json!(snapshot.retries));
    out.insert("plays".to_string(), json!(snapshot.plays));
    out.insert("ruleset_base".to_string(), json!(snapshot.ruleset_base.map(hex)));
    out.insert("gameplay_base".to_string(), json!(snapshot.gameplay_base.map(hex)));
    out.insert("score_base".to_string(), json!(snapshot.score_base.map(hex)));
    out.insert("result_base".to_string(), json!(snapshot.result_base.map(hex)));
    out.insert(
        "mods_masks".to_string(),
        json!({
            "menu": snapshot.menu_mods_mask,
            "play": snapshot.play_mods_mask,
            "resultsScreen": snapshot.result_mods_mask,
        }),
    );
    out.insert(
        "hits_candidates".to_string(),
        json!({
            "offsets": stable::HITS_CANDIDATE_OFFSETS
                .iter()
                .map(|offset| format!("0x{offset:X}"))
                .collect::<Vec<_>>(),
            "values": snapshot.hits_candidates,
            // 8 个槽是否整块读满（不读满 ⇒ 不发布 `play.hits`；见 `invariants::play_hits_publishable`）。
            "complete": snapshot.hits_candidates_complete,
            "mapping_verified": stable::HITS_MAPPING_VERIFIED,
            "mapping": stable::HITS_SLOT_MAPPING
                .iter()
                .map(|(index, key)| format!("{index}:{key:?}"))
                .collect::<Vec<_>>(),
        }),
    );
    out.insert(
        "result_hits_candidates".to_string(),
        json!(snapshot.result_hits_candidates),
    );
    out.insert(
        "result_hits_candidates_complete".to_string(),
        json!(snapshot.result_hits_candidates_complete),
    );
    out.insert(
        "chain".to_string(),
        probe.map(|probe| probe.to_json()).unwrap_or(Value::Null),
    );
    out.insert(
        "result".to_string(),
        result
            .map(|result| {
                json!({
                    "result_base": result.result_base.map(hex),
                    "score": result.score,
                    "max_combo": result.max_combo,
                    "player_name": result.player_name,
                    "online_id": result.online_id,
                    "mods_mask": result.result_mods_mask,
                })
            })
            .unwrap_or(Value::Null),
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

fn hex(value: u32) -> String {
    format!("0x{value:08X}")
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

/// **dev-only 离线回放**（Step 8d，`--replay` 的等价物）：流式重放一条对拍 JSONL，
/// 用**当前**判据重算全部格子，并打印统计。
///
/// 为什么要有它：真机会话的 `cmp`/`diff` 段是**当时那个判据**的产物。判据一改，
/// "改完之后同一批载荷会得到什么"必须能**离线**证明——否则只能再跑一次真机会话
/// （会引入新变量：换图、开关 mod、门抖动），无法把"判据修正的效果"与"环境变化"分开。
///
/// 口径：
/// - 逐行 `BufReader` 流式（336 MB / 49430 条，内存恒定）；
/// - 两侧都从**记录里的原始载荷**重建（`OurShadow::from_recorded` / `TosuShadow::from_payload`），
///   走的是与真机**同一个** `shadow::diff`；
/// - 除 `mod_signature` 之外，其余格子也一并统计（用来核 S8d 交接里那张逐格表）；
/// - `old` 段直接读记录里的 `cmp.mod_signature`（当时判据的原样读数）⇒ 新旧可对照；
/// - `menu+play` 变体：`keys::mods_from_slots(&payload, client, &["play","menu"])`，用来验证
///   "交接里 49165/265 那个口径"与页面口径（含 `resultsScreen.mods`）的差额来源。
///
/// **不参与任何发布路径**：只有 `tests-local/osu_compare.rs` 的 `#[ignore]` 用例调它。
pub fn replay_mod_cell(path: &std::path::Path) -> Result<String, String> {
    use std::io::{BufRead, BufReader};
    use std::collections::{BTreeMap, BTreeSet};

    let file = fs::File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let reader = BufReader::with_capacity(1 << 20, file);

    /// 一格的三种结局计数（跳过按原因分开记）。
    #[derive(Default, Clone)]
    struct CellCount {
        equal: u64,
        differ: u64,
        skipped: BTreeMap<String, u64>,
    }
    impl CellCount {
        fn add(&mut self, verdict: &Verdict) {
            match verdict {
                Verdict::Equal => self.equal += 1,
                Verdict::Differ { .. } => self.differ += 1,
                Verdict::Skipped { why } => *self.skipped.entry(why.clone()).or_insert(0) += 1,
            }
        }
        fn skipped_total(&self) -> u64 {
            self.skipped.values().sum()
        }
        fn line(&self) -> String {
            let mut parts: Vec<String> = self
                .skipped
                .iter()
                .map(|(why, count)| format!("skipped:{why}={count}"))
                .collect();
            parts.sort();
            format!(
                "true={} false={} skipped={}{}{}",
                self.equal,
                self.differ,
                self.skipped_total(),
                if parts.is_empty() { "" } else { " (" },
                if parts.is_empty() {
                    String::new()
                } else {
                    format!("{})", parts.join(", "))
                }
            )
        }
    }

    let mut records = 0u64;
    let mut bad_lines = 0u64;
    let mut cells: BTreeMap<&'static str, CellCount> = BTreeMap::new();
    // 记录里 `cmp` 段的原样计数（= 会话当时那个判据的读数，逐格对照用）。
    let mut recorded_cells: BTreeMap<&'static str, CellCount> = BTreeMap::new();
    let mut old_mod_cell: CellCount = CellCount::default();
    let mut old_mod_values: BTreeMap<String, u64> = BTreeMap::new();
    let mut old_new_cross: BTreeMap<String, u64> = BTreeMap::new();
    let mut variant_menu_play: CellCount = CellCount::default();
    let mut results_slot_attributable = 0u64;
    let mut ts_min = i64::MAX;
    let mut ts_max = i64::MIN;
    let mut unhealthy_ts: Vec<i64> = Vec::new();
    let mut states_ours: BTreeMap<String, u64> = BTreeMap::new();
    let mut states_tosu: BTreeMap<String, u64> = BTreeMap::new();
    let mut healths: BTreeMap<String, u64> = BTreeMap::new();
    let mut frozen_true = 0u64;
    let mut checksums: BTreeSet<String> = BTreeSet::new();
    let mut identities: BTreeSet<String> = BTreeSet::new();
    let mut our_masks: BTreeMap<String, u64> = BTreeMap::new();
    let mut our_slot_masks: BTreeSet<String> = BTreeSet::new();
    let mut mod_pairs: BTreeMap<String, u64> = BTreeMap::new();
    let mut mod_runs: Vec<(u64, u64, String)> = Vec::new();
    let mut prefix_no_tosu = 0u64;
    // 不发布帧（冻结/停帧）的连续段：`(首行, 末行, 冻结数, 停帧数, 首 ts, 末 ts)`
    let mut gaps: Vec<(u64, u64, u64, u64, i64, i64)> = Vec::new();
    // 逐记录的间隔（ms，> 1 s 才算"空档"）
    let mut prev_ts: Option<i64> = None;
    let mut gap_total_ms = 0i64;
    let mut max_gap_ms = 0i64;
    let mut max_gap_at_row = 0u64;
    // hits 来源分解：`(ours_published, tosu_published, both, ours_only, tosu_only)`
    let mut play_hits_sources = (0u64, 0u64, 0u64, 0u64, 0u64);
    let mut result_hits_sources = (0u64, 0u64, 0u64, 0u64, 0u64);

    for line in reader.lines() {
        let Ok(line) = line else {
            bad_lines += 1;
            continue;
        };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(record) = serde_json::from_str::<Value>(&line) else {
            bad_lines += 1;
            continue;
        };
        records += 1;
        let opened = records;
        let ts = record.get("ts").and_then(|v| v.as_i64()).unwrap_or(0);
        ts_min = ts_min.min(ts);
        ts_max = ts_max.max(ts);

        let our_payload = record.get("our").cloned().unwrap_or(Value::Null);
        let frozen = our_payload
            .get("frozen")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if frozen {
            frozen_true += 1;
        }
        let health = shadow::health_from_recorded(
            our_payload.get("health").and_then(|v| v.as_str()),
        );
        *healths.entry(health.to_string()).or_insert(0) += 1;
        if health == "unhealthy" {
            unhealthy_ts.push(ts);
        }
        if let Some(name) = our_payload.pointer("/state/name").and_then(|v| v.as_str()) {
            *states_ours.entry(name.to_string()).or_insert(0) += 1;
        }
        if let Some(mask) = record
            .pointer("/our_meta/mods_masks/menu")
            .and_then(|v| v.as_i64())
        {
            *our_masks.entry(mask.to_string()).or_insert(0) += 1;
        }
        if let Some(masks) = record.pointer("/our_meta/mods_masks").and_then(|v| v.as_object()) {
            for slot in ["menu", "play", "resultsScreen"] {
                if let Some(value) = masks.get(slot).and_then(|v| v.as_i64()) {
                    our_slot_masks.insert(format!("{slot}={value}"));
                }
            }
        }
        // 记录里 `cmp` 段的原样读数（会话当时判据）。
        for field in shadow::COMPARED_FIELDS {
            let Some(value) = record.pointer(&format!("/cmp/{field}")) else {
                continue;
            };
            let count = recorded_cells.entry(field).or_default();
            match value {
                Value::Bool(true) => count.equal += 1,
                Value::Bool(false) => count.differ += 1,
                Value::String(text) => {
                    let why = text.strip_prefix("skipped:").unwrap_or(text).to_string();
                    *count.skipped.entry(why).or_insert(0) += 1;
                }
                Value::Null => *count.skipped.entry("null".to_string()).or_insert(0) += 1,
                _ => {}
            }
        }
        let ours = OurShadow::from_recorded(&our_payload, frozen, health);

        // 不发布帧的连续段（`our` 载荷里一个 mods 对象都没有）。
        let non_publishing = frozen || health == "unhealthy";
        if non_publishing {
            match gaps.last_mut() {
                Some(last) if last.1 + 1 == opened => {
                    last.1 = opened;
                    if frozen {
                        last.2 += 1;
                    } else {
                        last.3 += 1;
                    }
                    last.5 = ts;
                }
                _ => gaps.push((opened, opened, frozen as u64, (!frozen) as u64, ts, ts)),
            }
        }
        if let Some(previous) = prev_ts {
            let gap = ts - previous;
            if gap > 1000 {
                gap_total_ms += gap;
                if gap > max_gap_ms {
                    max_gap_ms = gap;
                    max_gap_at_row = opened;
                }
            }
        }
        prev_ts = Some(ts);
        // hits 来源分解（键存在 = 该侧发布了这一条；`packet.rs` 的"hits 缺席"规则）。
        let ours_play = our_payload.pointer("/play/hits").is_some();
        let ours_result = our_payload.pointer("/resultsScreen/hits").is_some();
        let tosu_play = record.pointer("/tosu/play/hits").is_some();
        let tosu_result = record.pointer("/tosu/resultsScreen/hits").is_some();
        let bump = |sources: &mut (u64, u64, u64, u64, u64), ours: bool, them: bool| {
            if ours {
                sources.0 += 1;
            }
            if them {
                sources.1 += 1;
            }
            match (ours, them) {
                (true, true) => sources.2 += 1,
                (true, false) => sources.3 += 1,
                (false, true) => sources.4 += 1,
                (false, false) => {}
            }
        };
        bump(&mut play_hits_sources, ours_play, tosu_play);
        bump(&mut result_hits_sources, ours_result, tosu_result);

        match record.get("tosu") {
            Some(tosu) if !tosu.is_null() => {
                let theirs = TosuShadow::from_payload(tosu);
                if let Some(name) = tosu.pointer("/state/name").and_then(|v| v.as_str()) {
                    *states_tosu.entry(name.to_string()).or_insert(0) += 1;
                }
                if let Some(checksum) = tosu.pointer("/beatmap/checksum").and_then(|v| v.as_str()) {
                    checksums.insert(checksum.to_string());
                }
                let identity = crate::osu::keys::identity_from_payload(tosu);
                if !identity.is_empty() {
                    identities.insert(identity);
                }
                let diff = shadow::diff(&ours, &theirs);
                for field in shadow::COMPARED_FIELDS {
                    if let Some(entry) = diff.field(field) {
                        cells
                            .entry(field)
                            .or_default()
                            .add(&entry.verdict);
                    }
                }
                // 新口径的 mod 码集合对（诊断：看出"差在哪一槽"）。
                let pair = format!(
                    "ours={} theirs={}",
                    shadow::mods_view_text(&diff.our_mod_codes),
                    shadow::mods_view_text(&diff.their_mod_codes)
                );
                *mod_pairs.entry(pair.clone()).or_insert(0) += 1;
                // 变体口径：只取 menu+play（交接里 49165/265 那个口径）。
                let variant_ours =
                    crate::osu::keys::mods_from_slots(&our_payload, client_kind(&ours), &["play", "menu"]);
                let variant_theirs =
                    crate::osu::keys::mods_from_slots(tosu, client_kind_of_tosu(tosu), &["play", "menu"]);
                let variant = variant_verdict(&variant_ours, frozen, &variant_theirs);
                variant_menu_play.add(&variant);
                // 归因：三槽口径判 false 但"只取 menu+play"判 equal ⇒ 差异**只**来自
                // `resultsScreen.mods` 这一槽（C2 矩阵会话里 = tosu 的残留/垃圾对象）。
                let new_verdict = diff.field("mod_signature").map(|entry| entry.verdict.clone());
                if matches!(new_verdict, Some(Verdict::Differ { .. }))
                    && matches!(variant, Verdict::Equal)
                {
                    results_slot_attributable += 1;
                }
                let old = record.pointer("/cmp/mod_signature").cloned();
                // 新旧口径交叉表（带变体口径 ⇒ 三类 mismatch 各自多少条，不靠推测）。
                let old_text = short_verdict(old.as_ref());
                let new_text = new_verdict
                    .as_ref()
                    .map(short_verdict_of_verdict)
                    .unwrap_or_else(|| "none".to_string());
                let variant_text = short_verdict_of_verdict(&variant);
                let interesting = old_text != "true"
                    || new_text != "true"
                    || variant_text != "true";
                if interesting {
                    let cross = format!(
                        "old={old_text} new={new_text} variant(menu+play)={variant_text}"
                    );
                    *old_new_cross.entry(cross).or_insert(0) += 1;
                }
                // 连续段（new 口径的 false 段）：只看 mod 码集合是否相等。
                let differs = diff
                    .field("mod_signature")
                    .map(|entry| entry.verdict.is_differ())
                    .unwrap_or(false);
                match mod_runs.last_mut() {
                    Some(last) if differs && last.1 + 1 == opened && last.2 == pair => {
                        last.1 = opened;
                    }
                    _ if differs => mod_runs.push((opened, opened, pair)),
                    _ => {}
                }
            }
            _ => {
                prefix_no_tosu += 1;
            }
        }

        if let Some(old) = record.pointer("/cmp/mod_signature") {
            match old {
                Value::Bool(true) => old_mod_cell.equal += 1,
                Value::Bool(false) => old_mod_cell.differ += 1,
                Value::String(text) => {
                    let why = text.strip_prefix("skipped:").unwrap_or(text).to_string();
                    *old_mod_cell.skipped.entry(why).or_insert(0) += 1;
                }
                _ => {}
            }
            *old_mod_values.entry(old.to_string()).or_insert(0) += 1;
        }
    }

    let mut out = String::new();
    out.push_str("== replay_mod_cell (dev-only, Step 8d) ==\n");
    out.push_str(&format!("file    : {}\n", path.display()));
    out.push_str(&format!(
        "records : {records} (bad lines {bad_lines}; tosu 帧前 {} 条无帧)\n",
        prefix_no_tosu
    ));
    out.push_str(&format!(
        "ts span : {ts_min} .. {ts_max} ({} ms ≈ {:.1} min)\n",
        ts_max - ts_min,
        (ts_max - ts_min) as f64 / 60000.0
    ));
    out.push_str(&format!(
        "health  : {:?} ; frozen=true {frozen_true}\n",
        healths
    ));
    out.push_str(&format!("our.state  : {:?}\n", states_ours));
    out.push_str(&format!("tosu.state : {:?}\n", states_tosu));
    out.push_str(&format!(
        "distinct : checksum={} identity={} our_menu_masks={}\n",
        checksums.len(),
        identities.len(),
        our_masks.len()
    ));
    out.push_str(&format!("our menu masks: {:?}\n", our_masks));
    out.push_str(&format!(
        "our 三槽非空掩码取值（去重）: {:?}\n",
        our_slot_masks
    ));
    out.push_str("\n-- 逐格：**会话记录原样**（当时的 `cmp`，= S8d 交接那张表） --\n");
    for field in shadow::COMPARED_FIELDS {
        match recorded_cells.get(field) {
            Some(count) => out.push_str(&format!("  {field:<14} {}\n", count.line())),
            None => out.push_str(&format!("  {field:<14} (无记录)\n")),
        }
    }
    out.push_str("\n-- 逐格（**新判据**重算；我方一侧按**发布出去的载荷**重建） --\n");
    for field in shadow::COMPARED_FIELDS {
        match cells.get(field) {
            Some(count) => out.push_str(&format!("  {field:<14} {}\n", count.line())),
            None => out.push_str(&format!("  {field:<14} (无记录)\n")),
        }
    }
    out.push_str("\n-- mod_signature：旧口径（记录里的 cmp 原样） --\n");
    out.push_str(&format!("  {}\n", old_mod_cell.line()));
    out.push_str(&format!("  原样取值：{:?}\n", old_mod_values));
    out.push_str("\n-- mod_signature：新口径（页面三槽，状态相关） --\n");
    if let Some(count) = cells.get("mod_signature") {
        out.push_str(&format!("  {}\n", count.line()));
    }
    out.push_str("\n-- mod_signature：变体口径（只取 menu+play，交接里的 49165/265 口径） --\n");
    out.push_str(&format!("  {}\n", variant_menu_play.line()));
    out.push_str(&format!(
        "  其中 49165 + 48 + 217 = {}（交接的 49165 true / 265 非 true = 48 false + 217 跳过）\n",
        variant_menu_play.equal + variant_menu_play.differ + variant_menu_play.skipped_total()
    ));
    out.push_str(&format!(
        "\n  新口径 false 中**只**由 `resultsScreen.mods` 一槽造成的：{results_slot_attributable} 条\n"
    ));
    out.push_str("\n-- 新旧口径交叉表（行数） --\n");
    let mut cross: Vec<(&String, &u64)> = old_new_cross.iter().collect();
    cross.sort_by(|a, b| b.1.cmp(a.1));
    for (key, count) in cross {
        out.push_str(&format!("  {count:>7}  {key}\n"));
    }
    out.push_str("\n-- 新口径的 mod 码集合对（前 20） --\n");
    let mut pairs: Vec<(&String, &u64)> = mod_pairs.iter().collect();
    pairs.sort_by(|a, b| b.1.cmp(a.1));
    for (pair, count) in pairs.iter().take(20) {
        out.push_str(&format!("  {count:>7}  {pair}\n"));
    }
    out.push_str("\n-- 新口径 false 的连续段（≥2 行；行号 = 记录序号） --\n");
    let mut runs: Vec<&(u64, u64, String)> = mod_runs.iter().filter(|r| r.1 > r.0).collect();
    runs.sort_by_key(|r| r.0);
    for (start, end, pair) in &runs {
        out.push_str(&format!(
            "  [{start} .. {end}] len={} {pair}\n",
            end - start + 1
        ));
    }
    out.push_str(&format!("  （≥2 行的段：{} 段）\n", runs.len()));
    if let (Some(first), Some(last)) = (unhealthy_ts.first(), unhealthy_ts.last()) {
        out.push_str(&format!(
            "\nunhealthy 帧：{} 条，ts {} .. {}（{} ms）\n",
            unhealthy_ts.len(),
            first,
            last,
            last - first
        ));
    }
    out.push_str(&format!(
        "\n-- 不发布帧（冻结/停帧）的连续段：{} 段，共 {} 帧 --\n",
        gaps.len(),
        gaps.iter().map(|g| g.1 - g.0 + 1).sum::<u64>()
    ));
    for (start, end, frozen_count, stopped_count, ts_start, ts_end) in &gaps {
        out.push_str(&format!(
            "  [{start} .. {end}] len={} frozen={frozen_count} unhealthy={stopped_count} ts={ts_start}..{ts_end}\n",
            end - start + 1
        ));
    }
    out.push_str(&format!(
        "\n记录间隔：>1s 的累计空档 {} ms（≈{:.1} s），最大单次 {} ms（第 {} 条之后）\n",
        gap_total_ms,
        gap_total_ms as f64 / 1000.0,
        max_gap_ms,
        max_gap_at_row
    ));
    out.push_str(&format!(
        "\n-- hits 来源分解（键存在 = 该侧发布了这一条） --\n  play.hits  : ours={} tosu={} both={} ours-only={} tosu-only={}\n  result.hits: ours={} tosu={} both={} ours-only={} tosu-only={}\n",
        play_hits_sources.0,
        play_hits_sources.1,
        play_hits_sources.2,
        play_hits_sources.3,
        play_hits_sources.4,
        result_hits_sources.0,
        result_hits_sources.1,
        result_hits_sources.2,
        result_hits_sources.3,
        result_hits_sources.4
    ));
    Ok(out)
}

/// 回放用：记录里的 `cmp` 值 → 短标签（`true`/`false`/`skipped:<why>`）。
fn short_verdict(value: Option<&Value>) -> String {
    match value {
        Some(Value::Bool(true)) => "true".to_string(),
        Some(Value::Bool(false)) => "false".to_string(),
        Some(Value::String(text)) => text.clone(),
        Some(Value::Null) | None => "none".to_string(),
        Some(other) => other.to_string(),
    }
}

/// 回放用：`Verdict` → 短标签。
fn short_verdict_of_verdict(verdict: &Verdict) -> String {
    match verdict {
        Verdict::Equal => "true".to_string(),
        Verdict::Differ { .. } => "false".to_string(),
        Verdict::Skipped { why } => format!("skipped:{why}"),
    }
}

/// 回放用：我方载荷的 `client` → `Client`（与 `OurShadow` 的读法一致）。
fn client_kind(ours: &OurShadow) -> Client {
    match ours.client.as_deref() {
        Some(value) if value.eq_ignore_ascii_case("lazer") => Client::Lazer,
        _ => Client::Stable,
    }
}

/// 回放用：tosu 载荷的 `client` → `Client`。
fn client_kind_of_tosu(payload: &Value) -> Client {
    match payload.get("client").and_then(|v| v.as_str()) {
        Some(value) if value.eq_ignore_ascii_case("lazer") => Client::Lazer,
        _ => Client::Stable,
    }
}

/// 回放用：变体口径（指定槽）的三值判定——**只用于对照**，与 `shadow` 的判据同构。
fn variant_verdict(
    ours: &crate::osu::keys::PageMods,
    ours_frozen: bool,
    theirs: &crate::osu::keys::PageMods,
) -> Verdict {
    if ours_frozen {
        return Verdict::Skipped {
            why: "frozen-field-level".to_string(),
        };
    }
    if !ours.has_mod_payload() {
        return Verdict::Skipped {
            why: "our-gate-only".to_string(),
        };
    }
    if !theirs.has_mod_payload() {
        return Verdict::Skipped {
            why: "tosu-mods-absent".to_string(),
        };
    }
    if shadow::mods_view_text(&ours.codes) == shadow::mods_view_text(&theirs.codes) {
        Verdict::Equal
    } else {
        Verdict::Differ {
            ours: shadow::mods_view_text(&ours.codes),
            theirs: shadow::mods_view_text(&theirs.codes),
        }
    }
}

#[cfg(test)]
#[path = "../../tests-local/osu_compare.rs"]
mod tests_compare;
