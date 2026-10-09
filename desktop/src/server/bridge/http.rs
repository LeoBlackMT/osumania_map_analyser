// server::bridge::http - 17653 端口 HTTP 监听、POST /selection 门禁流转与 WS 补发

use std::fs;
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::Instant;
use serde::Deserialize;

use crate::config;
use crate::frames::{
    Envelope, BRIDGE_MAX_BODY_BYTES, BRIDGE_PORT, MAX_PAYLOAD_BYTES,
};
use crate::server::log::log_at;
use crate::server::{broadcast, http, json_error, malody_root, md5_hex, next_seq, Shared};
use super::chart::{derive_fields, identity_of, sandbox_chart_path, song_frame, SandboxError};
use super::settings::{
    read_settings, read_settings_with_retry, resolve_judge, settings_log, win_scale_fallback_log,
    win_scale_for,
};
use super::state::{
    bridge_alive, classify, flag_text, judge_text, Dedupe, LogKind,
};

/// `judge_level` 的容错解析（§7.7）：**任何形态都不拒收**——缺失 / `null` / 非整数 /
/// 越界（`>4`）一律 `None`（= 未采集到），绝不猜、绝不夹断。
fn opt_judge_level<'de, D>(deserializer: D) -> Result<Option<u8>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(raw
        .and_then(|value| value.as_u64())
        .and_then(|value| u8::try_from(value).ok())
        .filter(|level| *level <= 4))
}

/// `pro_judge` / `turbo` 的容错解析：非布尔（`1` / `"true"` / 对象）一律 `None`。
fn opt_bool<'de, D>(deserializer: D) -> Result<Option<bool>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(raw.and_then(|value| value.as_bool()))
}

/// `POST /selection` 的载荷（全部 `#[serde(default)]` 容错：缺字段不拒收，由归一化兜底）。
/// **向后兼容**：旧插件只发前 8 个字段，因此新增的 `judge_level` / `pro_judge` / `turbo`
/// 缺省即 `None`（= 未知），绝不因缺字段报 400。
#[derive(Deserialize, Debug, Clone, Default)]
pub struct BridgeSelection {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub speed_rate: f64,
    #[serde(default)]
    pub screen: String,
    #[serde(default)]
    pub sequence: i64,
    #[serde(default)]
    pub event: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub chart_hash: String,
    #[serde(default)]
    pub source: String,
    /// 判定档（`0..=4` ↔ A~E；v5 新增，插件权威）。
    #[serde(default, deserialize_with = "opt_judge_level")]
    pub judge_level: Option<u8>,
    /// Pro（严格组）开关（v5 新增，插件权威）。
    #[serde(default, deserialize_with = "opt_bool")]
    pub pro_judge: Option<bool>,
    /// Turbo 开关（v5 新增，插件权威）。**不得用 UI 开关判**：插件报的是已提交记录。
    #[serde(default, deserialize_with = "opt_bool")]
    pub turbo: Option<bool>,
}

/// 归一化后的请求（门禁之后的唯一分支依据）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Normalized {
    pub screen: String,
    pub path: String,
    pub rate_text: String,
}

/// `speed_rate` 有效性：非有限值或越界（`< 0.05` 或 `> 10`）即无效。
pub fn rate_is_valid(rate: f64) -> bool {
    rate.is_finite() && (0.05..=10.0).contains(&rate)
}

/// `screen` 归一化：闭集外的一切（含空串、未知值）回落 `other`。
pub fn normalize_screen(raw: &str) -> &'static str {
    match raw.trim().to_ascii_lowercase().as_str() {
        "selection" => "selection",
        "playing" => "playing",
        "result" => "result",
        _ => "other",
    }
}

/// 载荷归一化（纯函数）：`speed_rate` 无效 ⇒ **整帧按 `other` 处理**（无谱面、倍率 1.0），
/// `screen` 未知值 ⇒ `other`。`other` 的 `path` 恒为空串、`rate_text` 恒为 `1.00000`
/// ——与上游"`screen=other` 时空 path、rate=1.0"的语义一致。
pub fn normalize(payload: &BridgeSelection) -> Normalized {
    let rate_ok = rate_is_valid(payload.speed_rate);
    let screen = if rate_ok {
        normalize_screen(&payload.screen)
    } else {
        "other"
    };
    if screen == "other" {
        Normalized {
            screen: "other".to_string(),
            path: String::new(),
            rate_text: "1.00000".to_string(),
        }
    } else {
        Normalized {
            screen: screen.to_string(),
            path: config::normalize_path(&payload.path),
            rate_text: format!("{:.5}", payload.speed_rate),
        }
    }
}

/// 内容六元组（`last_event` 的唯一形态）。
///
/// 第 5/6 位（`pro_text` / `turbo_text`）不可省：选曲界面切换 Turbo 时倍率不变，若它们不在
/// 元组里，这类帧会被判成"重复观察"而不广播 song 帧 ⇒ 页面 `winScale` 停在旧值。
pub fn content_key(
    normalized: &Normalized,
    judge: Option<u8>,
    pro: Option<bool>,
    turbo: Option<bool>,
) -> super::state::ContentKey {
    (
        normalized.path.clone(),
        normalized.rate_text.clone(),
        normalized.screen.clone(),
        judge_text(judge),
        flag_text(pro),
        flag_text(turbo),
    )
}

fn header_value<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    head.lines().skip(1).find_map(|line| {
        let (key, value) = line.split_once(':')?;
        if key.trim().eq_ignore_ascii_case(name) {
            Some(value.trim())
        } else {
            None
        }
    })
}

/// `Content-Type` 的 type 部分（`;` 之前）必须是 `application/json`；缺失 ⇒ 非 JSON。
fn is_json_content_type(head: &str) -> bool {
    header_value(head, "content-type")
        .map(|v| {
            v.split(';')
                .next()
                .unwrap_or("")
                .trim()
                .eq_ignore_ascii_case("application/json")
        })
        .unwrap_or(false)
}

fn respond(stream: &mut TcpStream, code: u16, body: &str) {
    http::respond_json(stream, code, body);
}

pub fn spawn_bridge(shared: Arc<Shared>, listener: TcpListener) {
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let shared = shared.clone();
            thread::spawn(move || handle_bridge(shared, stream));
        }
    });
}

/// 门禁顺序**固定**（与上游参考实现 `malody_insight/service.py:283-299` 的 HTTP 语义对齐）：
/// Host → path → Origin → method → Content-Type → body 长度 → JSON 解析。
/// 成功一律 **202**（插件只看 2xx）。
fn handle_bridge(shared: Arc<Shared>, mut stream: TcpStream) {
    let Some((head, body)) = http::read_request(&mut stream) else {
        return;
    };
    if !http::is_local_host(&head, BRIDGE_PORT) {
        respond(&mut stream, 403, &json_error("forbidden host"));
        return;
    }
    let request_line = head.lines().next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("GET");
    let url = parts.next().unwrap_or("/");
    let path = url.split('?').next().unwrap_or("");
    if path != "/selection" {
        respond(&mut stream, 404, &json_error("not found"));
        return;
    }
    // 浏览器页面不可能合法地调用本端点：**任何 Origin 头出现即拒**（非空/空都一样）。
    if header_value(&head, "origin").is_some() {
        respond(&mut stream, 403, &json_error("forbidden origin"));
        return;
    }
    if !method.eq_ignore_ascii_case("POST") {
        respond(&mut stream, 405, &json_error("method not allowed"));
        return;
    }
    if !is_json_content_type(&head) {
        respond(
            &mut stream,
            415,
            &json_error("unsupported media type: application/json required"),
        );
        return;
    }
    if body.is_empty() || body.len() > BRIDGE_MAX_BODY_BYTES {
        respond(
            &mut stream,
            413,
            &json_error("payload too large (0 < len <= 16384)"),
        );
        return;
    }
    let Ok(payload) = serde_json::from_str::<BridgeSelection>(&body) else {
        respond(&mut stream, 400, &json_error("invalid selection json"));
        return;
    };
    let (code, text) = handle_selection(&shared, &payload);
    respond(&mut stream, code, &text);
}

/// 一帧载荷的设置解析产物（纯数据；日志行是否真打由状态里的日志闸按"结论变化"决定）。
struct ResolvedSettings {
    judge: Option<u8>,
    pro: Option<bool>,
    turbo: Option<bool>,
    win_scale: Option<f64>,
    /// 候选日志（级别 + 类别 + 文本）。
    logs: Vec<(&'static str, LogKind, String)>,
}

/// 判定/Pro/Turbo/窗口解析（**插件权威** + `config.json` 兜底与交叉校验）。
///
/// 读盘策略（`config.json` 的唯一读盘点）：
///   * 插件上报了 `judge_level` ⇒ 只做**单次廉价读**做交叉校验（不需要重试窗口）；
///   * 插件没上报 ⇒ 用 `read_settings_with_retry`（≤1s，覆盖"开始游玩"的写盘竞态），
///     把文件值当兜底发布。
///
/// 读盘在锁外（不长时间持 `malody_bridge`）；日志在锁外打；去重由 `LogKind` 各自的闸门保证
/// "值变化时才打"，绝不逐帧刷屏。
fn resolve_settings(shared: &Shared, payload: &BridgeSelection, rate: f64) -> ResolvedSettings {
    let pro = payload.pro_judge;
    let turbo = payload.turbo;
    let win_scale = win_scale_for(turbo, rate);
    let config_path = malody_root(shared).map(|root| root.join("config.json"));
    let fallback = match (payload.judge_level, config_path.as_deref()) {
        (Some(_), Some(path)) => read_settings(path),
        (None, Some(path)) => read_settings_with_retry(path),
        (_, None) => None,
    };
    let file_judge = fallback.and_then(|snapshot| snapshot.judge);
    let (judge, source, cross_check) = resolve_judge(payload.judge_level, file_judge);

    let mut logs: Vec<(&'static str, LogKind, String)> = vec![(
        "info",
        LogKind::Settings,
        settings_log(source, judge, pro, turbo, rate, win_scale),
    )];
    if let Some(line) = cross_check {
        logs.push(("info", LogKind::CrossCheck, line));
    }
    // 插件没值、兜底也没值 ⇒ 一条 warn（页面回落默认 OD，必须留痕）。
    if payload.judge_level.is_none() && judge.is_none() {
        logs.push((
            "warn",
            LogKind::Unavailable,
            super::settings::judge_unavailable_log(config_path.as_deref(), fallback.is_some()),
        ));
    }
    // `winScale` 判不出来 ⇒ 一条 debug 说明原因（`null` 会原样进 song 帧）。
    // 在 1.00x 常规倍率下 winScale=null 是常态，不应在 info 级别刷屏。
    if win_scale.is_none() {
        logs.push((
            "debug",
            LogKind::WinScale,
            win_scale_fallback_log(turbo, rate),
        ));
    }
    ResolvedSettings {
        judge,
        pro,
        turbo,
        win_scale,
        logs,
    }
}

/// 真实事件处理（归一化 → 去重 → 沙箱 → 读谱 → 发帧）。返回 HTTP 状态码与响应体。
fn handle_selection(shared: &Arc<Shared>, payload: &BridgeSelection) -> (u16, String) {
    if !rate_is_valid(payload.speed_rate) {
        log_at(
            "error",
            &format!(
                "malody bridge: invalid speed_rate {:?} — treating the frame as screen=other",
                payload.speed_rate
            ),
        );
    }
    let normalized = normalize(payload);
    let rate = normalized.rate_text.parse::<f64>().unwrap_or(1.0);

    // 判定/Pro/Turbo/窗口解析**必须先于去重**：它们都进内容六元组（第 4/5/6 位），若在去重之后
    // 再解析，基线里的文本会与实际发布值分叉，逐字节重发的心跳会被误判成真实事件。
    let ResolvedSettings {
        judge,
        pro,
        turbo,
        win_scale,
        logs,
    } = resolve_settings(shared, payload, rate);

    // 去重（唯一去重点）：先判分支，再决定是否走真实事件处理。日志闸、发布值写回与去重共用
    // 同一个短锁；真正的 `log_at` 在锁外（文件 IO 不持锁）。
    let mut pending: Vec<(&'static str, String)> = Vec::new();
    let (kind, restart, event_seq) = {
        let mut state = shared.malody_bridge.lock().unwrap();
        for (level, category, line) in logs {
            if state.log_gates.gate_mut(category).due(&line) {
                pending.push((level, line));
            }
        }
        state.judge = judge;
        state.pro = pro;
        state.turbo = turbo;
        state.win_scale = win_scale;
        let content = content_key(&normalized, judge, pro, turbo);
        let outcome = classify(&mut state, content, payload.sequence, Instant::now());
        (outcome.kind, outcome.log, state.event_seq)
    };
    for (level, message) in pending {
        log_at(level, &message);
    }
    if let Some((level, message)) = restart {
        log_at(level, &message);
    }
    if kind != Dedupe::Event {
        return (202, "{}".to_string());
    }

    if normalized.screen == "other" {
        // 没有谱面可分析：只更新桥状态 + 推 state 帧（页面的清空由 state 帧的
        // eventSeq 边沿驱动）。**不发 song 帧**。
        {
            let mut state = shared.malody_bridge.lock().unwrap();
            state.screen = "other".to_string();
            state.chart_path.clear();
            state.rate_text = normalized.rate_text.clone();
        }
        broadcast(shared, "state", Some(crate::server::state_frame(shared)));
        return (202, "{}".to_string());
    }

    let Some(root) = malody_root(shared) else {
        log_at(
            "error",
            "malody bridge: REJECTED — malodyRoot unavailable (set MMA_MALODY_ROOT or malodyRoot in the shell config)",
        );
        return (
            403,
            json_error(SandboxError::RootUnavailable.message()),
        );
    };
    let chart = match sandbox_chart_path(&root, &payload.path) {
        Ok(path) => path,
        Err(err) => {
            log_at(
                "error",
                &format!(
                    "malody bridge: REJECTED {} — {}",
                    payload.path,
                    err.message()
                ),
            );
            return (403, json_error(err.message()));
        }
    };

    // 两道闸显式分开：沙箱允许 50 MB，`song` 帧的 `rawText` 只允许 5 MB。
    // 读之前先 stat——5~50 MB 的谱面必须"显式拒绝并留痕"，绝不读进来又被帧上限静默丢掉。
    let len = fs::metadata(&chart).map(|m| m.len()).unwrap_or(0);
    if len > MAX_PAYLOAD_BYTES as u64 {
        log_at(
            "error",
            &format!(
                "malody bridge: chart {} is {} bytes, exceeds the rawText cap ({} bytes) — NOT broadcast",
                chart.display(),
                len,
                MAX_PAYLOAD_BYTES
            ),
        );
        return (
            504,
            json_error("chart exceeds the rawText cap (>5MB) — not broadcast"),
        );
    }
    let Ok(text) = fs::read_to_string(&chart) else {
        log_at(
            "error",
            &format!("malody bridge: chart read FAILED {}", chart.display()),
        );
        return (500, json_error("chart read failed"));
    };

    let content_md5 = md5_hex(&text);
    let fields = derive_fields(&text, &chart, &payload.version);
    let identity = identity_of(&fields, &content_md5);
    let request_id = format!("b{}", event_seq);
    {
        let mut state = shared.malody_bridge.lock().unwrap();
        state.screen = normalized.screen.clone();
        state.chart_path = chart.to_string_lossy().to_string();
        state.rate_text = normalized.rate_text.clone();
    }
    broadcast(
        shared,
        "song",
        Some(song_frame(
            &request_id,
            &fields,
            &identity,
            &normalized.rate_text,
            text,
            &normalized.screen,
            judge,
            pro,
            turbo,
            win_scale,
        )),
    );
    // 立即推 state 帧（不等 30s 定时帧）：桥事件的场景变化是页面及时跟上的唯一可靠路径。
    broadcast(shared, "state", Some(crate::server::state_frame(shared)));
    log_at(
        "info",
        &format!(
            "malody bridge: event #{} screen={} rate={} chart={} identity={}",
            event_seq,
            normalized.screen,
            normalized.rate_text,
            chart.display(),
            identity
        ),
    );
    (202, "{}".to_string())
}

/// 新 WS 连接建立后的补发（**只发给这个连接**）：先一帧 `state`，再（满足触发条件时）
/// 当前谱面的 `song` 帧。
///
/// 触发条件钉死：`bridge_alive` **且** 已存 `screen ∈ {selection, playing, result}` **且**
/// `chart_path` 非空。⚠️ 不能只判 `last_event.is_some()`——`screen=other` 的真实事件会把
/// `last_event` 覆盖成空 `path` 的六元组，那样会补发出一个空 `path` 的 song 帧，与
/// "`screen=other` 不发 song 帧"直接冲突。
///
/// 补发**不是新事件**：不修改 `last_event` / `last_seq` / `event_seq`，也不参与去重状态机。
/// 内容按当前状态重算（重新读谱、重算 md5 与 identity）；`requestId = b{event_seq}r{k}`，
/// 与真实事件的 `b{seq}` 区分便于日志排查（补发只在注册时发生一次 ⇒ `k` 恒为 1）。
///
/// 为什么要连 state 帧一起补：注册 sink 后壳原本只发 `hello`，而 `state` 帧只由 30s
/// 定时器或真实桥事件触发（`spawn_timers` = 15s ping + 15s sleep ⇒ **周期 30s**）。
/// 页面刷新/壳重启发生在游玩中时，`song` 帧回来了、`state.malodyPlaying` 也置了真，但
/// `state.malodyAlive` 会一直是 `undefined` ⇒ 页面 L1 门控 `malodyPlaying && malodyAlive`
/// 不成立，路由与源圆点在最长 30s 内仍指向 osu/Etterna。
pub fn reconnect_messages(shared: &Shared) -> Vec<String> {
    let (alive, screen, chart_path, rate_text, event_seq, judge, pro, turbo, win_scale) = {
        let state = shared.malody_bridge.lock().unwrap();
        (
            bridge_alive(state.last_seen),
            state.screen.clone(),
            state.chart_path.clone(),
            state.rate_text.clone(),
            state.event_seq,
            state.judge,
            state.pro,
            state.turbo,
            state.win_scale,
        )
    };

    let mut out = Vec::new();
    let state_envelope = Envelope::new(
        "state",
        next_seq(shared),
        Some(crate::server::state_frame(shared)),
    );
    out.push(serde_json::to_string(&state_envelope).unwrap_or_default());

    let screen_ok = matches!(screen.as_str(), "selection" | "playing" | "result");
    if !alive || !screen_ok || chart_path.is_empty() {
        return out;
    }
    let chart = PathBuf::from(&chart_path);
    let Ok(text) = fs::read_to_string(&chart) else {
        log_at(
            "debug",
            &format!(
                "malody bridge: replay skipped, chart unreadable {}",
                chart.display()
            ),
        );
        return out;
    };
    if text.len() > MAX_PAYLOAD_BYTES {
        log_at(
            "error",
            &format!(
                "malody bridge: replay skipped, chart {} exceeds the rawText cap ({} bytes)",
                chart.display(),
                text.len()
            ),
        );
        return out;
    }
    let content_md5 = md5_hex(&text);
    // payload 的 `version` 不落库 ⇒ 补发按 `.mc` 的 `/meta/version` → 文件名 stem 回落
    // （与字段来源表一致：绝不从 `.osu` 里猜 level）。
    let fields = derive_fields(&text, &chart, "");
    let identity = identity_of(&fields, &content_md5);
    let request_id = format!("b{}r1", event_seq);
    let frame = song_frame(
        &request_id,
        &fields,
        &identity,
        &rate_text,
        text,
        &screen,
        judge,
        pro,
        turbo,
        win_scale,
    );
    out.push(
        serde_json::to_string(&Envelope::new("song", next_seq(shared), Some(frame)))
            .unwrap_or_default(),
    );
    log_at(
        "info",
        &format!(
            "malody bridge: replayed {} to a new connection (screen={} rate={})",
            request_id, screen, rate_text
        ),
    );
    out
}
