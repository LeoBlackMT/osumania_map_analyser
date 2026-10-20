//! RC 难度反馈：壳侧客观字段重算 + Ed25519 签名。
//!
//! 页面只提交 `vote` 与显示值等"报告字段"；客观字段（bid/checksum/rate/mods/hits/
//! weight/passed）全部从 native 快照重算。签名字符串与后端 `CanonicalSignString`
//! 必须逐字节一致。
//!
//! 壳不直接发 HTTPS：`server::http` 返回签名信封，由页面转发到后端。

use ed25519_compact::{KeyPair, Seed};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::osu::model::{Client, Hits, Snapshot};

pub const SIGNING_SEED_HEX: &str = env!("MMA_FEEDBACK_SIGNING_SEED");
pub const KEY_ID: &str = env!("MMA_FEEDBACK_KEY_ID");

pub const RC_LN_MAX: f64 = 0.15;
pub const ACC_MIN: f64 = 90.0;
pub const ACC_MAX: f64 = 100.0;
pub const TOTAL_HITS_MIN: u32 = 200;
pub const COMPLETION_MIN: f64 = 0.85;
pub const WEIGHT_BASE: f64 = 0.30;
pub const WEIGHT_SPAN: f64 = 0.70;
pub const WEIGHT_PEAK: f64 = 96.0;
pub const WEIGHT_SIGMA_LEFT: f64 = 3.2;
pub const WEIGHT_SIGMA_RIGHT: f64 = 4.0;
pub const WEIGHT_MIN: f64 = 0.40;
pub const WEIGHT_MAX: f64 = 1.0;
pub const WEIGHT_VERSION: u32 = 3;

/// 允许的倍速：NM/MR 类 1.0，DT/NC 1.5，HT/DC 0.75。
pub const ALLOWED_RATES: &[f64] = &[0.75, 1.0, 1.5];

/// mod 白名单（NM 用空集合表示）。
pub const ALLOWED_MODS: &[&str] = &[
    "HD", "FI", "FL", "DT", "NC", "HT", "DC",
    "MR", "SD", "PF", "SV2", "TD", "CS",
];

/// 旧版独立文件（其内容已合并进 `mma-shell-state.json`，仅用于一次性迁移）。
pub const INSTALL_ID_FILE: &str = "mma-shell-feedback.json";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct FeedbackPayload {
    pub bid: i64,
    pub checksum: String,
    pub rate: f64,
    pub mods: Vec<String>,
    pub key_count: i32,
    pub ln_ratio: f64,
    pub pattern: String,
    pub mode_tag: String,
    pub od: Option<f64>,
    pub actual_algorithm: String,
    pub selected_algorithm: String,
    pub displayed_val: f64,
    pub vote: f64,
    pub weight: f64,
    pub weight_version: u32,
    pub passed: bool,
    pub client: String,
    pub classic: bool,
    pub plugin_version: String,
    pub settings_hash: String,
    pub install_id: String,
    pub timestamp: u64,
    pub nonce: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct FeedbackAuth {
    pub key_id: String,
    pub signature: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct SignedEnvelope {
    pub payload: FeedbackPayload,
    pub auth: FeedbackAuth,
}

/// 页面提交的报告字段（客观字段不在这里）。
#[derive(Deserialize, Clone, Debug)]
pub struct PageFeedback {
    pub vote: f64,
    pub displayed_val: f64,
    pub actual_algorithm: String,
    pub selected_algorithm: String,
    pub key_count: i32,
    pub ln_ratio: f64,
    pub pattern: String,
    pub mode_tag: String,
    pub od: Option<f64>,
    pub settings_hash: String,
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn hex_decode(text: &str) -> Option<Vec<u8>> {
    if text.len() % 2 != 0 {
        return None;
    }
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks(2) {
        let hi = (pair[0] as char).to_digit(16)?;
        let lo = (pair[1] as char).to_digit(16)?;
        out.push(((hi << 4) | lo) as u8);
    }
    Some(out)
}

fn keypair() -> Option<&'static KeyPair> {
    static KEYPAIR: OnceLock<Option<KeyPair>> = OnceLock::new();
    KEYPAIR
        .get_or_init(|| {
            let bytes = hex_decode(SIGNING_SEED_HEX)?;
            if bytes.len() != 32 {
                return None;
            }
            Seed::from_slice(&bytes).ok().map(KeyPair::from_seed)
        })
        .as_ref()
}

/// 与后端 `CanonicalSignString` 逐字节一致（v1）。
pub fn build_sign_string(payload: &FeedbackPayload) -> String {
    let mods = if payload.mods.is_empty() {
        "NM".to_string()
    } else {
        let mut sorted: Vec<String> = payload.mods.iter().map(|m| m.to_uppercase()).collect();
        sorted.sort();
        sorted.join(",")
    };
    let od = payload
        .od
        .map(|value| format!("{value:.2}"))
        .unwrap_or_else(|| "-".to_string());
    format!(
        "v1|{}|{}|{:.3}|{}|{}|{:.4}|{}|{}|{}|{}|{}|{:.2}|{:.1}|{:.3}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
        payload.bid,
        payload.checksum,
        payload.rate,
        mods,
        payload.key_count,
        payload.ln_ratio,
        payload.pattern,
        payload.mode_tag,
        od,
        payload.actual_algorithm,
        payload.selected_algorithm,
        payload.displayed_val,
        payload.vote,
        payload.weight,
        payload.weight_version,
        if payload.passed { 1 } else { 0 },
        payload.client,
        if payload.classic { 1 } else { 0 },
        payload.plugin_version,
        payload.settings_hash,
        payload.install_id,
        payload.timestamp,
        payload.nonce,
    )
}

pub fn sign(payload: &FeedbackPayload) -> Option<SignedEnvelope> {
    let keypair = keypair()?;
    let message = build_sign_string(payload);
    let signature = keypair.sk.sign(message.as_bytes(), None);
    Some(SignedEnvelope {
        payload: payload.clone(),
        auth: FeedbackAuth {
            key_id: KEY_ID.to_string(),
            signature: hex_encode(signature.as_ref()),
        },
    })
}

/// ScoreV1 准度（与页面 `scoreV1` 同式）。
pub fn score_v1(hits: &Hits) -> f64 {
    let total = total_hits(hits) as f64;
    if total <= 0.0 {
        return 0.0;
    }
    // 用 u64 求和：六个 u32 相加在 debug 下会 panic、release 下回绕（坏指针可能给出巨大计数）。
    let weighted = 300u64 * (hits.geki as u64 + hits.n300 as u64)
        + 200 * hits.katu as u64
        + 100 * hits.n100 as u64
        + 50 * hits.n50 as u64;
    100.0 * weighted as f64 / (300.0 * total)
}

pub fn total_hits(hits: &Hits) -> u32 {
    let sum = hits.geki as u64
        + hits.n300 as u64
        + hits.katu as u64
        + hits.n100 as u64
        + hits.n50 as u64
        + hits.miss as u64;
    sum.min(u32::MAX as u64) as u32
}

pub fn weight_for_acc(acc: f64) -> f64 {
    // 峰值 96 的左右不对称高斯：低 acc 掉得快，96 之后缓慢回落。
    let value = acc.clamp(90.0, 100.0);
    let sigma = if value <= WEIGHT_PEAK { WEIGHT_SIGMA_LEFT } else { WEIGHT_SIGMA_RIGHT };
    let z = (value - WEIGHT_PEAK) / sigma;
    (WEIGHT_BASE + WEIGHT_SPAN * (-0.5 * z * z).exp()).clamp(WEIGHT_MIN, WEIGHT_MAX)
}

fn is_hex32(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

fn sanitize_text(value: &str, max_len: usize) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == ' ' || *c == '_' || *c == '-' || *c == '+' || *c == '/')
        .take(max_len)
        .collect::<String>()
        .trim()
        .to_string()
}

fn is_allowed_actual(value: &str) -> bool {
    matches!(
        value,
        "Sunny" | "Daniel" | "Azusa" | "Roxy" | "Companella" | "Azusa+Companella"
    )
}

fn is_allowed_selected(value: &str) -> bool {
    matches!(
        value,
        "Mixed" | "Sunny" | "Daniel" | "Azusa" | "Roxy" | "Companella"
    )
}

/// lazer 的 mods/rate：来自原生 `SelectedMods`（`lazer_mods`，`{acronym, speed_change}`）。
///
/// 与 stable 的位掩码是两条独立来源：lazer 没有 stable 的 mod 位表，自定义倍速（DA/CL）
/// 也只能从 `speed_change` 读出来。`None` = 原生没读到 ⇒ 调用方按 `no_mods` 拒绝。
fn lazer_mods_and_rate(snapshot: &Snapshot) -> Option<(Vec<String>, f64)> {
    let lazer_mods = snapshot.lazer_mods.as_ref()?;
    // fail-closed：SelectedMods 里只要有一个元素没读全（对象/类型名/acronym/数量超限），
    // 就不能签名——把 DT 静默丢成 NM 会写入错误的客观字段。展示层不受影响。
    if !snapshot.lazer_mods_complete {
        return None;
    }
    let mods: Vec<String> = lazer_mods
        .iter()
        .map(|entry| entry.acronym.trim().to_uppercase())
        .filter(|code| !code.is_empty())
        .collect();
    let rate = lazer_mods
        .iter()
        .filter_map(|entry| entry.speed_change)
        .find(|value| value.is_finite() && *value > 0.0)
        .unwrap_or(1.0);
    Some((mods, rate))
}

/// 从 native 快照 + 页面报告字段构造待签名 payload；任一 gate 失败返回原因码。
pub fn build_payload(
    snapshot: &Snapshot,
    page: &PageFeedback,
    plugin_version: &str,
    install_id: &str,
) -> Result<FeedbackPayload, &'static str> {
    if snapshot.state_name.as_deref() != Some("resultScreen") {
        return Err("not_result_screen");
    }
    if snapshot.replay {
        return Err("replay");
    }
    let Some(hits) = snapshot.result_hits.as_ref() else {
        return Err("no_hits");
    };
    let Some(bid) = snapshot.map_id.filter(|id| *id > 0) else {
        return Err("no_bid");
    };
    let checksum = snapshot.checksum.as_deref().unwrap_or("").to_lowercase();
    if !is_hex32(&checksum) {
        return Err("no_checksum");
    }

    // mods / rate 的来源按客户端分：
    // - stable：`mask_for_state` 的结算槽（它已做了"状态 = resultScreen ∧ 结算链可信"的门控，
    //   直接读 raw mask 会把结算动画期间的残留指针当成 mod）；
    // - lazer：没有 stable 的位掩码，用原生读到的 `SelectedMods` 列表（`lazer_mods`：
    //   acronym + speed_change）。它与页面 `resultsScreen.mods` 同源，自定义倍速也在这里。
    let (mods, rate) = match snapshot.client {
        Some(Client::Lazer) => {
            let Some(lazer) = lazer_mods_and_rate(snapshot) else {
                return Err("no_mods");
            };
            lazer
        }
        _ => {
            let Some(mask) = crate::osu::invariants::mask_for_state(snapshot).2 else {
                return Err("no_mods");
            };
            let mods: Vec<String> = crate::osu::keys::menu_mod_codes(mask)
                .iter()
                .map(|code| (*code).to_string())
                .collect();
            let rate = crate::osu::keys::menu_mods_value(mask)
                .get("rate")
                .and_then(Value::as_f64)
                .unwrap_or(1.0);
            (mods, rate)
        }
    };
    if mods.iter().any(|code| !ALLOWED_MODS.contains(&code.as_str())) {
        return Err("mods");
    }
    if !ALLOWED_RATES.iter().any(|allowed| (rate - allowed).abs() < 1e-9) {
        return Err("rate");
    }

    let total = total_hits(hits);
    if total < TOTAL_HITS_MIN {
        return Err("total_hits");
    }
    let objects = snapshot
        .beatmap_file
        .as_ref()
        .map(|file| file.first_last.object_count)
        .unwrap_or(0);
    if objects == 0 || (total as f64) / (objects as f64) < COMPLETION_MIN {
        return Err("completion");
    }
    let acc = score_v1(hits);
    if !(ACC_MIN..=ACC_MAX).contains(&acc) {
        return Err("accuracy");
    }

    if !(page.vote >= -1.0 && page.vote <= 1.0) || (page.vote * 2.0).fract() != 0.0 {
        return Err("bad_vote");
    }
    if !page.displayed_val.is_finite() || !(-3.0..=25.0).contains(&page.displayed_val) {
        return Err("bad_displayed");
    }
    if !is_allowed_actual(page.actual_algorithm.trim()) {
        return Err("bad_algorithm");
    }
    if !is_allowed_selected(page.selected_algorithm.trim()) {
        return Err("bad_algorithm");
    }
    if page.key_count != 4 && page.key_count != 6 && page.key_count != 7 {
        return Err("bad_keycount");
    }
    if !page.ln_ratio.is_finite() || page.ln_ratio < 0.0 || page.ln_ratio > RC_LN_MAX {
        return Err("not_rc");
    }
    if page.mode_tag.trim() != "RC" {
        return Err("not_rc");
    }
    // OD 是签名的客观字段：页面必须给出（页面从本地 .osu 解析）；缺失即拒绝，
    // 不用服务端 API 的值回填——否则 OD 校验失去意义。
    let Some(od) = page.od else {
        return Err("bad_od");
    };
    if !od.is_finite() || !(0.0..=10.0).contains(&od) {
        return Err("bad_od");
    }
    let settings_hash = sanitize_text(&page.settings_hash, 16);
    if settings_hash.len() != 8 {
        return Err("bad_settings_hash");
    }

    let client = snapshot.client.unwrap_or(Client::Stable).as_str().to_string();
    let classic = !mods.iter().any(|code| code == "SV2")
        && (client != "lazer" || mods.iter().any(|code| code == "CL"));

    Ok(FeedbackPayload {
        bid: bid as i64,
        checksum,
        rate,
        mods,
        key_count: page.key_count,
        ln_ratio: page.ln_ratio,
        pattern: {
            let text = sanitize_text(&page.pattern, 32);
            if text.is_empty() { "unknown".to_string() } else { text }
        },
        mode_tag: "RC".to_string(),
        od: Some(od),
        actual_algorithm: page.actual_algorithm.trim().to_string(),
        selected_algorithm: page.selected_algorithm.trim().to_string(),
        displayed_val: page.displayed_val,
        vote: page.vote,
        weight: (weight_for_acc(acc) * 1000.0).round() / 1000.0,
        weight_version: WEIGHT_VERSION,
        passed: true,
        client,
        classic,
        plugin_version: plugin_version.to_string(),
        settings_hash,
        install_id: install_id.to_string(),
        timestamp: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        nonce: new_nonce(),
    })
}

/// 16 hex 的随机性来源：时间 nanos + pid + 进程内单调计数器，经 SHA-256 取前 8 字节。
pub fn new_nonce() -> String {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let count = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut hasher = Sha256::new();
    hasher.update(nanos.to_le_bytes());
    hasher.update(std::process::id().to_le_bytes());
    hasher.update(count.to_le_bytes());
    hex_encode(&hasher.finalize()[..8])
}

/// 读/建本地匿名 install id（32 hex）。与窗口状态同存 `mma-shell-state.json`，
/// 旧的 `mma-shell-feedback.json` 会被一次性迁移并删除。
pub fn install_id() -> Option<String> {
    let dir = crate::config::exe_dir()?;
    install_id_in(&dir)
}

pub fn install_id_in(dir: &Path) -> Option<String> {
    let mut state = crate::config::window::read_window_state_in(dir);
    if let Some(id) = state.install_id.as_deref().filter(|id| is_hex32(id)) {
        return Some(id.to_string());
    }

    // 迁移：旧文件存在且合法 → 写入 state 文件**成功之后**才删旧文件。
    // 写失败就返回 None（调用方拒绝签名），而不是每次调用都拿一个新的随机 id。
    let legacy = dir.join(INSTALL_ID_FILE);
    if let Ok(text) = std::fs::read_to_string(&legacy) {
        if let Ok(value) = serde_json::from_str::<Value>(&text) {
            if let Some(id) = value.get("install_id").and_then(Value::as_str) {
                if is_hex32(id) {
                    state.install_id = Some(id.to_string());
                    if crate::config::window::write_window_state_in(dir, &state) {
                        let _ = std::fs::remove_file(&legacy);
                        return Some(id.to_string());
                    }
                    return None;
                }
            }
        }
    }

    let id = new_install_id();
    state.install_id = Some(id.clone());
    if crate::config::window::write_window_state_in(dir, &state) {
        Some(id)
    } else {
        None
    }
}

fn new_install_id() -> String {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let count = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut hasher = Sha256::new();
    hasher.update(nanos.to_le_bytes());
    hasher.update(std::process::id().to_le_bytes());
    hasher.update(count.to_le_bytes());
    hash_with_process_salt(hasher)
}

fn hash_with_process_salt(mut hasher: Sha256) -> String {
    // 同一进程内多一次不可预测输入（栈地址），避免纯时间戳被枚举。
    let local = 0u8;
    hasher.update((&local as *const u8 as usize).to_le_bytes());
    hex_encode(&hasher.finalize()[..16])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> FeedbackPayload {
        FeedbackPayload {
            bid: 4182931,
            checksum: "7c9e6679742540de944bbed07fc1f90a".to_string(),
            rate: 1.0,
            mods: vec!["MR".to_string()],
            key_count: 4,
            ln_ratio: 0.015,
            pattern: "Jacks".to_string(),
            mode_tag: "RC".to_string(),
            od: Some(8.5),
            actual_algorithm: "Roxy".to_string(),
            selected_algorithm: "Mixed".to_string(),
            displayed_val: 12.2,
            vote: 0.5,
            weight: 0.995,
            weight_version: 3,
            passed: true,
            client: "stable".to_string(),
            classic: true,
            plugin_version: "2.2.0".to_string(),
            settings_hash: "a1b2c3d4".to_string(),
            install_id: "6f1c2a9e6f1c2a9e6f1c2a9e6f1c2a9e".to_string(),
            timestamp: 1760000000,
            nonce: "a4f19c08e21b7d33".to_string(),
        }
    }

    #[test]
    fn sign_string_is_stable() {
        let text = build_sign_string(&sample());
        assert_eq!(
            text,
            "v1|4182931|7c9e6679742540de944bbed07fc1f90a|1.000|MR|4|0.0150|Jacks|RC|8.50|Roxy|Mixed|12.20|0.5|0.995|3|1|stable|1|2.2.0|a1b2c3d4|6f1c2a9e6f1c2a9e6f1c2a9e6f1c2a9e|1760000000|a4f19c08e21b7d33"
        );
    }

    #[test]
    fn nm_is_used_for_empty_mods() {
        let mut payload = sample();
        payload.mods.clear();
        assert!(build_sign_string(&payload).contains("|1.000|NM|4|"));
    }

    #[test]
    fn missing_od_uses_dash() {
        let mut payload = sample();
        payload.od = None;
        assert!(build_sign_string(&payload).contains("|RC|-|Roxy|"));
    }

    #[test]
    fn signature_roundtrip_and_field_sensitivity() {
        let payload = sample();
        let signed = sign(&payload).expect("sign");
        let message = build_sign_string(&payload);
        let signature = ed25519_compact::Signature::from_slice(
            &hex_decode(&signed.auth.signature).expect("hex"),
        )
        .expect("signature");
        let public = keypair().expect("keypair").pk;
        public
            .verify(message.as_bytes(), &signature)
            .expect("verify");

        let mut changed = payload.clone();
        changed.vote = 1.0;
        assert_ne!(build_sign_string(&changed), message);
    }

    #[test]
    fn weight_curve_matches_table() {
        assert!((weight_for_acc(90.0) - 0.4207).abs() < 0.001);
        assert!((weight_for_acc(92.0) - 0.6205).abs() < 0.001);
        assert!((weight_for_acc(94.0) - 0.8758).abs() < 0.001);
        assert!((weight_for_acc(96.0) - 1.0).abs() < 1e-9);
        assert!((weight_for_acc(98.0) - 0.9177).abs() < 0.001);
        assert!((weight_for_acc(100.0) - 0.7246).abs() < 0.001);
        assert!((weight_for_acc(80.0) - 0.4207).abs() < 0.001);
    }

    #[test]
    fn score_v1_all_max_is_100() {
        let hits = Hits { n300: 500, n100: 0, n50: 0, geki: 0, katu: 0, miss: 0 };
        assert!((score_v1(&hits) - 100.0).abs() < 1e-9);
    }

    #[test]
    fn lazer_mods_and_rate_come_from_selected_mods() {
        let mut snapshot = Snapshot {
            client: Some(Client::Lazer),
            ..Snapshot::default()
        };
        snapshot.lazer_mods = Some(vec![
            crate::osu::model::LazerMod {
                acronym: "DT".to_string(),
                speed_change: Some(1.5),
                adjust_pitch: None,
                drain_rate: None,
                overall_difficulty: None,
                extended_limits: None,
            },
            crate::osu::model::LazerMod {
                acronym: "MR".to_string(),
                speed_change: None,
                adjust_pitch: None,
                drain_rate: None,
                overall_difficulty: None,
                extended_limits: None,
            },
        ]);
        // 读全了才允许签名（fail-closed 见 `lazer_mods_and_rate`）。
        snapshot.lazer_mods_complete = true;
        let (mods, rate) = lazer_mods_and_rate(&snapshot).expect("lazer mods");
        assert_eq!(mods, vec!["DT".to_string(), "MR".to_string()]);
        assert!((rate - 1.5).abs() < 1e-9);

        // 列表读不全（任一元素失败/超限）⇒ 拒绝签名，绝不回落成 NM。
        snapshot.lazer_mods_complete = false;
        assert!(lazer_mods_and_rate(&snapshot).is_none(), "不完整的 mods 不能签名");
        snapshot.lazer_mods_complete = true;

        // 自定义倍速（CL 0.75）也走同一个来源；没有速度 mod 时 rate = 1.0。
        snapshot.lazer_mods = Some(vec![crate::osu::model::LazerMod {
            acronym: "cl".to_string(),
            speed_change: Some(0.75),
            adjust_pitch: None,
            drain_rate: None,
            overall_difficulty: None,
            extended_limits: None,
        }]);
        assert_eq!(
            lazer_mods_and_rate(&snapshot),
            Some((vec!["CL".to_string()], 0.75))
        );
        snapshot.lazer_mods = None;
        assert!(lazer_mods_and_rate(&snapshot).is_none(), "原生读不到就不猜");
    }

    #[test]
    fn install_id_persists() {
        let dir = std::env::temp_dir().join(format!("mma-fb-test-{}", new_nonce()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let first = install_id_in(&dir).expect("first");
        let second = install_id_in(&dir).expect("second");
        assert_eq!(first, second);
        assert_eq!(first.len(), 32);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn nonce_is_16_hex_and_unique() {
        let a = new_nonce();
        let b = new_nonce();
        assert_eq!(a.len(), 16);
        assert_ne!(a, b);
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit()));
    }
}
