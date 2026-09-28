// 影子比对（**纯函数**）：我们的快照 + 一份 tosu 载荷 → 逐字段差异集。
//
// 分工（计划 §4.0 Step 6 / §3.4 L3）：
// - 本文件只做"两份**已解析**的普通结构体"的比较，**不碰 IO**（不连 WS、不读内存、不落盘）；
// - `compare.rs` 是 IO 驱动：抓 tosu 载荷、喂快照、把结果写成 JSONL。
// 这样比较规则可以被单测覆盖，也不会让"证据仪器"混进发布路径。
//
// 三值语义（为什么不是 bool）：`true/false` 无法区分"比了且不等"与"根本没得比"。
// B2 的教训（`B2-session-verification.md` §2/§3）是：**跳过必须显式**，否则
// `skipped` 会被读成 `false`，把"设计上不比"误判成"读数错了"。
// 因此每格只有三种结局：`Equal` / `Differ{ours,theirs}` / `Skipped{why}`。
//
// 第一断言（§3.4）：跨传输键**逐字节相等** —— `identity` / `mod_signature` / `state_name`；
// 数值（`first_object`/`last_object`）只做"相等性记录"（不设容差：两侧都是整毫秒）。

use crate::osu::keys;
use crate::osu::model::{Client, Snapshot};
use serde_json::{json, Map, Value};

/// 一侧的取值：`Available` = 读到了；`Skipped` = 结构性缺失/按设计不比（**必须带原因**）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Side {
    Available(String),
    Skipped(String),
}

impl Side {
    pub fn available(value: impl Into<String>) -> Side {
        Side::Available(value.into())
    }

    pub fn skipped(why: impl Into<String>) -> Side {
        Side::Skipped(why.into())
    }

    pub fn value(&self) -> Option<&str> {
        match self {
            Side::Available(value) => Some(value),
            Side::Skipped(_) => None,
        }
    }

    pub fn is_available(&self) -> bool {
        matches!(self, Side::Available(_))
    }
}

/// 一格的结局。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    Equal,
    Differ { ours: String, theirs: String },
    Skipped { why: String },
}

impl Verdict {
    pub fn is_equal(&self) -> bool {
        matches!(self, Verdict::Equal)
    }

    pub fn is_differ(&self) -> bool {
        matches!(self, Verdict::Differ { .. })
    }

    pub fn is_skipped(&self) -> bool {
        matches!(self, Verdict::Skipped { .. })
    }

    /// JSONL 里的紧凑形态：`true` / `false` / `"skipped:<why>"`。
    pub fn to_json(&self) -> Value {
        match self {
            Verdict::Equal => json!(true),
            Verdict::Differ { .. } => json!(false),
            Verdict::Skipped { why } => json!(format!("skipped:{why}")),
        }
    }
}

/// 一格比较：两侧都可用才比；任一不可用 ⇒ `Skipped`（**不是** `false`）。
pub fn verdict(ours: &Side, theirs: &Side) -> Verdict {
    match (ours, theirs) {
        (Side::Available(ours), Side::Available(theirs)) => {
            if ours == theirs {
                Verdict::Equal
            } else {
                Verdict::Differ {
                    ours: ours.clone(),
                    theirs: theirs.clone(),
                }
            }
        }
        (Side::Skipped(a), Side::Skipped(b)) => Verdict::Skipped {
            why: if a == b {
                a.clone()
            } else {
                format!("{a}+{b}")
            },
        },
        (Side::Skipped(why), _) => Verdict::Skipped { why: why.clone() },
        (_, Side::Skipped(why)) => Verdict::Skipped { why: why.clone() },
    }
}

/// 一条字段的比较记录。
#[derive(Clone, Debug)]
pub struct FieldDiff {
    pub field: &'static str,
    pub verdict: Verdict,
    pub ours: Side,
    pub theirs: Side,
}

impl FieldDiff {
    fn new(field: &'static str, ours: Side, theirs: Side) -> FieldDiff {
        let verdict = verdict(&ours, &theirs);
        FieldDiff {
            field,
            verdict,
            ours,
            theirs,
        }
    }
}

/// 差异集（一条记录的全部字段）。
#[derive(Clone, Debug, Default)]
pub struct ShadowDiff {
    pub fields: Vec<FieldDiff>,
    /// 我们的 mod 代码集合（页面 `modData.js` 的同一套规则）。
    pub our_mod_codes: Vec<&'static str>,
    /// tosu 的 mod 代码集合（同一套规则 ⇒ apples-to-apples）。
    pub their_mod_codes: Vec<&'static str>,
    /// tosu 报的 `client`（"焦点客户端"，见 `client_diff_skipped`）。
    pub their_client: Option<String>,
}

impl ShadowDiff {
    pub fn field(&self, name: &str) -> Option<&FieldDiff> {
        self.fields.iter().find(|f| f.field == name)
    }

    pub fn equal_count(&self) -> usize {
        self.fields.iter().filter(|f| f.verdict.is_equal()).count()
    }

    pub fn differ_count(&self) -> usize {
        self.fields.iter().filter(|f| f.verdict.is_differ()).count()
    }

    pub fn skipped_count(&self) -> usize {
        self.fields
            .iter()
            .filter(|f| f.verdict.is_skipped())
            .count()
    }

    /// mod 代码集合是否相等（集合语义：顺序无关 —— 页面用 `Set`）。
    pub fn mod_codes_equal(&self) -> bool {
        let mut ours: Vec<&str> = self.our_mod_codes.clone();
        let mut theirs: Vec<&str> = self.their_mod_codes.clone();
        ours.sort_unstable();
        theirs.sort_unstable();
        ours.dedup();
        theirs.dedup();
        ours == theirs
    }

    /// 紧凑 JSON（`cmp` 段：字段名 → `true`/`false`/`"skipped:<why>"`）。
    pub fn to_json(&self) -> Value {
        let mut map = Map::new();
        for field in &self.fields {
            map.insert(field.field.to_string(), field.verdict.to_json());
        }
        Value::Object(map)
    }

    /// 差异清单（`diff` 段：只有不等/跳过的那些格子，带两侧原始值）。
    pub fn detail_json(&self) -> Value {
        let mut map = Map::new();
        for field in &self.fields {
            let mut entry = Map::new();
            entry.insert("our".to_string(), side_json(&field.ours));
            entry.insert("tosu".to_string(), side_json(&field.theirs));
            match &field.verdict {
                Verdict::Differ { .. } => {
                    entry.insert("verdict".to_string(), json!("differ"));
                }
                Verdict::Skipped { why } => {
                    entry.insert("verdict".to_string(), json!("skipped"));
                    entry.insert("why".to_string(), json!(why));
                }
                Verdict::Equal => continue,
            }
            map.insert(field.field.to_string(), Value::Object(entry));
        }
        Value::Object(map)
    }
}

fn side_json(side: &Side) -> Value {
    match side {
        Side::Available(value) => json!(value),
        Side::Skipped(why) => json!({ "skipped": why }),
    }
}

/// tosu 侧的载荷 → 比对所需的普通结构体。
#[derive(Clone, Debug, Default)]
pub struct TosuShadow {
    pub client: Option<String>,
    pub state_name: Option<String>,
    pub state_number: Option<i64>,
    pub checksum: Option<String>,
    pub identity: Option<String>,
    pub mod_signature: Option<String>,
    pub first_object: Option<i64>,
    pub last_object: Option<i64>,
    pub songs_folder: Option<String>,
    pub mod_codes: Vec<&'static str>,
}

impl TosuShadow {
    /// 从一帧 tosu v2 载荷解析。字段缺失一律 `None`（**不**用空串冒充）。
    pub fn from_payload(payload: &Value) -> TosuShadow {
        let text = |pointer: &str| -> Option<String> {
            payload
                .pointer(pointer)
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        };
        let number =
            |pointer: &str| -> Option<i64> { payload.pointer(pointer).and_then(|v| v.as_i64()) };
        let client = text("/client");
        let client_kind = client.as_deref().map(|value| {
            if value.eq_ignore_ascii_case("lazer") {
                Client::Lazer
            } else {
                Client::Stable
            }
        });
        let checksum = payload
            .pointer("/beatmap/checksum")
            .and_then(|v| v.as_str())
            .or_else(|| payload.pointer("/beatmap/md5").and_then(|v| v.as_str()))
            .map(|s| s.to_string());
        TosuShadow {
            client,
            state_name: text("/state/name"),
            state_number: number("/state/number"),
            checksum,
            identity: Some(keys::identity_from_payload(payload)),
            mod_signature: Some(keys::mod_signature_from_payload(
                payload,
                client_kind.unwrap_or(Client::Stable),
            )),
            first_object: number("/beatmap/time/firstObject"),
            last_object: number("/beatmap/time/lastObject"),
            songs_folder: text("/folders/songs"),
            mod_codes: keys::mod_codes_from_payload(payload, client_kind.unwrap_or(Client::Stable)),
        }
    }
}

/// 我们这一侧的取值（从快照派生；`play` 态的 mods 链属 C2，见下）。
pub struct OurShadow {
    pub client: Option<String>,
    pub state_name: Option<String>,
    pub checksum: Option<String>,
    pub identity: Option<String>,
    pub mod_signature: Option<String>,
    pub first_object: Option<i64>,
    pub last_object: Option<i64>,
    pub songs_folder: Option<String>,
    pub songs_cfg_value: Option<String>,
    pub mod_codes: Vec<&'static str>,
    /// 我方状态是否是 `play`（局内 mods 链未实现 ⇒ mod 签名按设计跳过）。
    pub in_play_state: bool,
}

impl OurShadow {
    pub fn from_snapshot(snapshot: &Snapshot) -> OurShadow {
        let payload = snapshot.to_packet();
        let client = snapshot.client.unwrap_or(Client::Stable);
        OurShadow {
            client: snapshot.client.map(|c| c.as_str().to_string()),
            state_name: snapshot.state_name.clone(),
            checksum: snapshot.checksum.clone(),
            identity: Some(keys::identity_from_payload(&payload)),
            mod_signature: Some(keys::mod_signature_from_payload(&payload, client)),
            first_object: snapshot.first_object().map(|v| v as i64),
            last_object: snapshot.last_object().map(|v| v as i64),
            songs_folder: snapshot.songs_folder.clone(),
            songs_cfg_value: snapshot.songs_cfg_value.clone(),
            mod_codes: keys::mod_codes_from_payload(&payload, client),
            in_play_state: snapshot.state_name.as_deref() == Some("play"),
        }
    }
}

/// 两侧 → 差异集。
///
/// 跳过的口径（每一条都要能在报告里被引用）：
/// - `client`：tosu 报的是**前台焦点客户端**（P1-notes F1），这就是本装置存在的理由；
///   两侧不等 ⇒ `Skipped{focused-client-mismatch}`（我们读的进程与 tosu 供的不是同一个）。
/// - `mod_signature`：仅当**我方**处于 `play` 态时跳过（局内 mods 链 = Step 8 / C2）；
///   非 play 态照比。
/// - 任一字段我方/tosu 侧缺失 ⇒ `Skipped`（各自带原因）。
pub fn diff(ours: &OurShadow, theirs: &TosuShadow) -> ShadowDiff {
    let mut out = ShadowDiff {
        our_mod_codes: ours.mod_codes.clone(),
        their_mod_codes: theirs.mod_codes.clone(),
        their_client: theirs.client.clone(),
        ..Default::default()
    };

    // client：焦点客户端口径（不是"读数对不对"）。
    let client_ours = ours
        .client
        .clone()
        .map(Side::available)
        .unwrap_or_else(|| Side::skipped("our-client-unset"));
    let client_theirs = match theirs.client.clone() {
        Some(value) => {
            if ours
                .client
                .as_deref()
                .map(|ours| ours.eq_ignore_ascii_case(&value))
                .unwrap_or(false)
            {
                Side::available(value)
            } else {
                Side::skipped("focused-client-mismatch")
            }
        }
        None => Side::skipped("tosu-client-absent"),
    };
    out.fields
        .push(FieldDiff::new("client", client_ours, client_theirs));

    out.fields.push(FieldDiff::new(
        "state_name",
        option_side(&ours.state_name, "state.read-failed"),
        option_side(&theirs.state_name, "tosu-state-absent"),
    ));
    out.fields.push(FieldDiff::new(
        "checksum",
        option_side(&ours.checksum, "beatmap.read-failed"),
        option_side(&theirs.checksum, "tosu-beatmap-absent"),
    ));
    out.fields.push(FieldDiff::new(
        "identity",
        option_side(&ours.identity, "packet.empty"),
        option_side(&theirs.identity, "tosu-frame-absent"),
    ));
    out.fields.push(FieldDiff::new(
        "mod_signature",
        if ours.in_play_state {
            Side::skipped("play-state-mods-chain-not-implemented")
        } else {
            option_side(&ours.mod_signature, "packet.empty")
        },
        if theirs.mod_signature.is_none() {
            Side::skipped("tosu-mod-signature-absent")
        } else {
            option_side(&theirs.mod_signature, "tosu-mod-signature-absent")
        },
    ));
    out.fields.push(FieldDiff::new(
        "first_object",
        number_side(ours.first_object, "osu-file-not-parsed"),
        number_side(theirs.first_object, "tosu-time-absent"),
    ));
    out.fields.push(FieldDiff::new(
        "last_object",
        number_side(ours.last_object, "osu-file-not-parsed"),
        number_side(theirs.last_object, "tosu-time-absent"),
    ));
    out.fields.push(FieldDiff::new(
        "songs_folder",
        optional_text_side(
            &ours.songs_folder,
            format!(
                "songs-chain:{}",
                ours.songs_cfg_value.as_deref().unwrap_or("unavailable")
            ),
        ),
        option_side(&theirs.songs_folder, "tosu-folders-absent"),
    ));
    out
}

fn option_side(value: &Option<String>, why: &str) -> Side {
    match value {
        Some(value) => Side::available(value.clone()),
        None => Side::skipped(why),
    }
}

fn optional_text_side(value: &Option<String>, why: String) -> Side {
    match value {
        Some(value) => Side::available(value.clone()),
        None => Side::skipped(why),
    }
}

fn number_side(value: Option<i64>, why: &str) -> Side {
    match value {
        Some(value) => Side::available(value.to_string()),
        None => Side::skipped(why),
    }
}

#[cfg(test)]
#[path = "../../tests-local/osu_shadow.rs"]
mod tests_shadow;
