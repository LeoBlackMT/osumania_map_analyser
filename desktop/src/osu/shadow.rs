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

use crate::osu::invariants::{FrameAction, FrameOutcome};
use crate::osu::keys;
use crate::osu::model::{Client, Hits, Snapshot};
use serde_json::{json, Map, Value};

/// 被比较的字段名（**顺序即 JSONL 的 `cmp` 顺序**；`compare.rs` 在"没有 tosu 帧"时
/// 用它生成全 `skipped:no-tosu-frame` 的记录）。
pub const COMPARED_FIELDS: &[&str] = &[
    "client",
    "state_name",
    "checksum",
    "identity",
    "mod_signature",
    "first_object",
    "last_object",
    "songs_folder",
    "play_hits",
    "result_hits",
    "health",
];

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
///
/// `diag` = 该格的**诊断**（不参与判定）：目前只有 `mod_signature` 用（逐槽存在性
/// `mods_by_key` + 缓存键口径的签名串）。留着它是为了让"以后某一格不相等"仍然**可解释**
/// ——C2 矩阵会话（49430 条）的 6789 条 `false` 就是因为当时只有"签名串"一个读数、
/// 看不出"是位置不同还是码不同"。
#[derive(Clone, Debug)]
pub struct FieldDiff {
    pub field: &'static str,
    pub verdict: Verdict,
    pub ours: Side,
    pub theirs: Side,
    pub diag: Option<Value>,
}

impl FieldDiff {
    fn new(field: &'static str, ours: Side, theirs: Side) -> FieldDiff {
        let verdict = verdict(&ours, &theirs);
        FieldDiff {
            field,
            verdict,
            ours,
            theirs,
            diag: None,
        }
    }

    fn with_diag(mut self, diag: Value) -> FieldDiff {
        self.diag = Some(diag);
        self
    }
}

/// 差异集（一条记录的全部字段）。
#[derive(Clone, Debug, Default)]
pub struct ShadowDiff {
    pub fields: Vec<FieldDiff>,
    /// 我们的 mod 代码集合（**页面口径**：`keys::page_mods` ⇒ 状态相关的候选集）。
    pub our_mod_codes: Vec<&'static str>,
    /// tosu 的 mod 代码集合（同一套规则 ⇒ apples-to-apples）。
    pub their_mod_codes: Vec<&'static str>,
    /// 我方逐槽 mod 视图（诊断：`mods.by_key`）。
    pub our_page_mods: keys::PageMods,
    /// tosu 逐槽 mod 视图（诊断：`mods.by_key`）。
    pub their_page_mods: keys::PageMods,
    /// 我方**缓存键口径**的签名串（三槽无条件并集；仅诊断，不参与判定）。
    pub our_signature: Option<String>,
    /// tosu 缓存键口径的签名串（仅诊断）。
    pub their_signature: Option<String>,
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

    /// 差异清单（`diff` 段：只有不等/跳过的那些格子，带两侧原始值与诊断）。
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
            if let Some(diag) = &field.diag {
                entry.insert("diag".to_string(), diag.clone());
            }
            map.insert(field.field.to_string(), Value::Object(entry));
        }
        Value::Object(map)
    }
}

/// 页面会看到的 mod 码集合的**渲染**（`{DT,NC}` / `{}`；排序 + 去重 ⇒ 与集合语义一致）。
///
/// 这是 `mod_signature` 一格的 `our`/`tosu` 取值形态：该格的判据是**码集合**（页面用它派生
/// 签名与缓存键），不是签名串本身（签名串只在 `diag.signature` 里留档）。
pub fn mods_view_text(codes: &[&'static str]) -> String {
    let mut sorted: Vec<&str> = codes.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    format!("{{{}}}", sorted.join(","))
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
    /// **页面口径**的 mod 视图（`keys::page_mods` ⇒ 状态相关的候选集）。
    pub page_mods: keys::PageMods,
    /// `play.hits` 的 6 键称重串（10 键里取页面消费的 6 个）。
    pub play_hits: Option<String>,
    /// `resultsScreen.hits` 的 6 键称重串。
    pub result_hits: Option<String>,
}

/// `{geki,300,katu,100,50,0}` → `Hits::canonical()` 的同形字符串。
///
/// 为什么不直接比较 JSON：tosu 发 10/9 键（多 `sliderBreaks` 等），我们发 6 键——
/// 直接比 JSON 是"形状不同"而不是"计数不同"。两侧都先归一到 6 键再比。
pub fn canonical_hits(value: Option<&Value>) -> Option<String> {
    let object = value?.as_object()?;
    let number = |key: &str| -> u32 {
        object
            .get(key)
            .and_then(|v| v.as_f64())
            .filter(|v| v.is_finite())
            .map(|v| v.max(0.0) as u32)
            .unwrap_or(0)
    };
    let hits = Hits {
        n300: number("300"),
        n100: number("100"),
        n50: number("50"),
        geki: number("geki"),
        katu: number("katu"),
        miss: number("0"),
    };
    Some(hits.canonical())
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
            page_mods: keys::page_mods(payload, client_kind.unwrap_or(Client::Stable)),
            play_hits: canonical_hits(payload.pointer("/play/hits")),
            result_hits: canonical_hits(payload.pointer("/resultsScreen/hits")),
        }
    }
}

/// 我们这一侧的取值（从快照派生；C2 起局内/结算 mods 也有链）。
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
    /// **页面口径**的 mod 视图（`keys::page_mods` ⇒ 状态相关的候选集）。
    pub page_mods: keys::PageMods,
    /// 我方状态是否是 `play`（C2 保留了该字段：**载荷来源**诊断用，不再用于跳过比较）。
    pub in_play_state: bool,
    /// 我方 `play.hits` / `resultsScreen.hits` 的 6 键称重串。
    pub play_hits: Option<String>,
    pub result_hits: Option<String>,
    /// 本帧的健康状态与动作（`health` 一格的来源；`unhealthy` 帧不出载荷，
    /// 与 tosu 的比较按"我们这边不可用"跳过）。
    pub health: &'static str,
    pub frozen: bool,
}

impl OurShadow {
    pub fn from_snapshot(snapshot: &Snapshot) -> OurShadow {
        OurShadow::from_snapshot_with(
            snapshot,
            &FrameOutcome {
                action: FrameAction::Publish,
                state: None,
                reason: None,
                degraded_fields: Vec::new(),
                transition: None,
            },
        )
    }

    /// 带门输出的一侧（门输出决定 `health` 一格与"是否冻结"）。
    pub fn from_snapshot_with(snapshot: &Snapshot, outcome: &FrameOutcome) -> OurShadow {
        let payload = snapshot.to_packet();
        let client = snapshot.client.unwrap_or(Client::Stable);
        // `page_mods` 取**本帧真正会发布出去的载荷**（`compare.rs::record` 的同一套形态）：
        // 冻结帧 = 字段级冻结包、停帧 = 空对象 ⇒ 两者都**没有** mods 对象。页面收到的就是它，
        // 所以 mod 一格的"我方视图"必须照此重建——否则停帧帧会拿快照里的 mods 去比一个
        // 页面根本收不到的载荷（Step 8d 的 `our-gate-only` 跳过就是为这个形态准备的）。
        let frozen_packet;
        let stopped_packet;
        let published: &Value = match outcome.action {
            FrameAction::Stop => {
                stopped_packet = json!({});
                &stopped_packet
            }
            FrameAction::FreezeStateOnly => {
                frozen_packet = snapshot.to_frozen_packet();
                &frozen_packet
            }
            FrameAction::Publish => &payload,
        };
        let page_mods = keys::page_mods(published, client);
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
            in_play_state: page_mods.in_play_state,
            page_mods,
            play_hits: snapshot.play_hits.map(|hits| hits.canonical()),
            result_hits: snapshot.result_hits.map(|hits| hits.canonical()),
            health: outcome.state.map(|s| s.as_str()).unwrap_or("idle"),
            frozen: outcome.action == FrameAction::FreezeStateOnly,
        }
    }

    /// **回放用**（dev-only，`compare::replay_mod_cell`）：从**记录里的我方载荷**重建一侧。
    ///
    /// 记录里的 `our` 对象 = 当时的载荷 + `reason`/`health`/`frozen`/`degraded_fields`
    /// 四个对拍字段（`compare.rs::record`）⇒ 后两者按入参给，其余用与 `tosu` 侧**同一套**
    /// 读法重建，保证回放与真机走同一个 `diff`。
    pub fn from_recorded(payload: &Value, frozen: bool, health: &'static str) -> OurShadow {
        let shared = TosuShadow::from_payload(payload);
        OurShadow {
            client: shared.client,
            state_name: shared.state_name,
            checksum: shared.checksum,
            identity: shared.identity,
            mod_signature: shared.mod_signature,
            first_object: shared.first_object,
            last_object: shared.last_object,
            songs_folder: shared.songs_folder,
            songs_cfg_value: None,
            in_play_state: shared.page_mods.in_play_state,
            page_mods: shared.page_mods,
            play_hits: shared.play_hits,
            result_hits: shared.result_hits,
            health,
            frozen,
        }
    }
}

/// 记录里的 `health` 字符串 → `&'static str`（回放用；未知值归 `idle`）。
pub fn health_from_recorded(value: Option<&str>) -> &'static str {
    match value {
        Some("healthy") => "healthy",
        Some("degraded") => "degraded",
        Some("unhealthy") => "unhealthy",
        _ => "idle",
    }
}

/// 两侧 → 差异集。
///
/// 跳过的口径（每一条都要能在报告里被引用）：
/// - `client`：tosu 报的是**前台焦点客户端**（P1-notes F1），这就是本装置存在的理由；
///   两侧不等 ⇒ `Skipped{focused-client-mismatch}`（我们读的进程与 tosu 供的不是同一个）。
/// - `mod_signature`：见 [`mod_signature_field`]（判据 = **页面可见的 mod 码集合**，
///   状态相关候选；任一侧没有 mods 对象 ⇒ `Skipped`，**不是** `false`）。
/// - `play_hits`/`result_hits`：该态没有 hits（如菜单态）或未过时间门 ⇒ `Skipped`。
/// - 任一字段我方/tosu 侧缺失 ⇒ `Skipped`（各自带原因）。
pub fn diff(ours: &OurShadow, theirs: &TosuShadow) -> ShadowDiff {
    let mut out = ShadowDiff {
        our_mod_codes: ours.page_mods.codes.clone(),
        their_mod_codes: theirs.page_mods.codes.clone(),
        our_page_mods: ours.page_mods.clone(),
        their_page_mods: theirs.page_mods.clone(),
        our_signature: ours.mod_signature.clone(),
        their_signature: theirs.mod_signature.clone(),
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
    out.fields.push(mod_signature_field(ours, theirs));
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
    out.fields.push(FieldDiff::new(
        "play_hits",
        if ours.frozen {
            Side::skipped("frozen-field-level")
        } else {
            option_side(&ours.play_hits, "hits-not-published")
        },
        option_side(&theirs.play_hits, "tosu-play-absent"),
    ));
    out.fields.push(FieldDiff::new(
        "result_hits",
        option_side(&ours.result_hits, "hits-not-published"),
        option_side(&theirs.result_hits, "tosu-results-absent"),
    ));
    out.fields.push(FieldDiff::new(
        "health",
        Side::available(ours.health.to_string()),
        Side::skipped("our-gate-only"),
    ));
    out
}

/// `mod_signature` 一格：**页面可见的 mod 码集合**比较（Step 8d 修正）。
///
/// ## 为什么改（旧判据的两处错）
///
/// 旧判据比的是**两侧各自派生的签名串**（`keys::mod_signature_from_payload`，三槽无条件并集）。
/// 这在 C2 矩阵会话（49430 条）里产出 6789 条 `false`，而其中 **6723 条**是
/// `1.00000|none|none|1` vs `1.50000|none|none|1` 这种"读数位置不同"造成的假差异
/// （`temp/osu-native-memory/evidence/C2-stable-full/probe-mod-cell2.txt`）。判据错在两处：
/// ① 页面**按状态**选候选（游玩态只看 `play.mods`），不是无条件三槽并集；
/// ② "某一侧根本没有 mods 对象"（我们的门冻结/停帧、tosu 帧缺键）被记成 `false`，
///    而它其实是**没得比**（B2 的教训：跳过必须显式）。
///
/// ## 判据（逐行照抄页面）
///
/// ```js
/// // socketHandlers.js:37-44（页面自己的包装）
/// function getModData(data) {
///     return getModDataFromPayload(data, { …, preferPlayMods: state.isInPlayState });
/// }
/// // modData.js:11-26（两套候选集）
/// function collectPlayModsCandidates(data) { return [data?.play?.mods]; }
/// function collectNonPlayModsCandidates(data) { return [data?.menu?.mods, data?.resultsScreen?.mods]; }
/// // modData.js:73-80（选取 + 有没有 mods 对象）
/// const selectedModsCandidates = preferPlayMods ? playModsCandidates
///                                              : [...playModsCandidates, ...nonPlayModsCandidates];
/// const validMods = selectedModsCandidates.filter((mods) => mods !== undefined && mods !== null);
/// const hasModPayload = validMods.length > 0;
/// ```
///
/// 归一与游玩态判定取 `modeLogic.js:5-16`（`normalizeClientStateName` + `isPlayStateName`），
/// 且**每条消息先更新 `state.isInPlayState` 再取值**（`socketHandlers.js:151-160` → `:170`）⇒
/// 每一侧都用**自己这一帧**的 `state.name`。求码与集合语义在 `keys.rs`（同一份页面规则）。
///
/// 三值结局：
/// - 我方冻结帧（字段级）⇒ `Skipped{frozen-field-level}`（载荷里没有 mods，不是"没读到"）；
/// - 我方载荷**一个 mods 对象都没有**（停帧/门产物）⇒ `Skipped{our-gate-only}`；
/// - tosu 侧一个都没有 ⇒ `Skipped{tosu-mods-absent}`；
/// - 两侧都有 ⇒ 比**码集合**（顺序无关）；签名串只进 `diag.signature`（缓存键口径，不判定）。
///
/// “我方有没有 mods 对象”判的是**本帧发出去的**载荷（冻结帧只发 `client`+`state`、
/// 停帧发空对象 ⇒ 两种都没有），不是内存里的快照值。
fn mod_signature_field(ours: &OurShadow, theirs: &TosuShadow) -> FieldDiff {
    let our_side = if ours.frozen {
        Side::skipped("frozen-field-level")
    } else if !ours.page_mods.has_mod_payload() {
        Side::skipped("our-gate-only")
    } else {
        Side::available(mods_view_text(&ours.page_mods.codes))
    };
    let their_side = if theirs.page_mods.has_mod_payload() {
        Side::available(mods_view_text(&theirs.page_mods.codes))
    } else {
        Side::skipped("tosu-mods-absent")
    };
    FieldDiff::new("mod_signature", our_side, their_side).with_diag(json!({
        "mods_by_key": {
            "ours": ours.page_mods.to_json(),
            "tosu": theirs.page_mods.to_json(),
        },
        // 缓存键口径（三槽无条件并集）——**只诊断**：它与页面口径在游玩态下会不同。
        "signature": {
            "ours": ours.mod_signature,
            "tosu": theirs.mod_signature,
        },
    }))
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
