// 快照 → tosu v2 形状载荷（**纯函数**）——§3.3 的完整字段表（C2 起）。
//
// 字段表（实现时不得再改；每行的消费点见计划 §3.3）：
//
// | 键 | 来源 | 出现条件 |
// |---|---|---|
// | `client` | 进程分派 | 恒 |
// | `state.{number,name}` | `statusPtr` 链 + P8 观测集 | 恒 |
// | `game.paused` | 停滞推导（`previousPlayTime == playTime`） | 有播放时间时 |
// | `beatmap.{id,set,checksum,md5,artist,title,version,mapper}` | Beatmap 对象 | 有图 |
// | `beatmap.time.{live,firstObject,lastObject,mp3Length}` | 内存 / `.osu` 解析 | 各自可得时 |
// | `files.{beatmap,background,audio}` | 内存 / `.osu` 解析 | 有图 |
// | `directPath.{beatmapFile,beatmapBackground,beatmapAudio,beatmapFolder}` | 派生 | 有图 |
// | `folders.{game,songs,beatmap}` | 目录 / cfg 链 / 内存 | 各自可得时 |
// | `menu.mods` / `play.mods` / `resultsScreen.mods` | 三条 mod 链 | **恒在**（值或 `null`） |
// | `play.hits` / `resultsScreen.hits` | 两条 hits 链 | 状态门 + 链有效性门 + 新鲜度门全过时（见 `mod.rs::apply_hits`） |
//
// 三条**形状规则**（计划 §3.3 的加粗警告）：
// 1. **载荷路径字符串是"名字/相对键"**，绝不是绝对路径：`files.beatmap` = 文件名、
//    `folders.beatmap` = 谱面**文件夹名**、`directPath.beatmapFile` = `folder\filename`
//    （win32 分隔符）。写错会让 `identity` 的 `path:` 段与 tosu 不等 ⇒ 静默丢缓存。
//    我们**自己读盘**时才另行拼绝对路径（`songs_folder\folder\filename`）。
// 2. 缺失字段一律**不出现**（而不是给 `null`）——页面靠"键在不在"走降级链。
//    唯一例外是三个 mods 键：tosu 恒发（无信息时为 `null`），页面用 `!== null` 过滤。
// 3. **mods 是缓存键**：本帧读不出来就发 `null`，**绝不重发上一帧的掩码**（§3.4）。

use crate::osu::invariants;
use crate::osu::keys;
use crate::osu::model::{Client, LazerMod, Snapshot};
use serde_json::{json, Map, Value};

/// 合成 v2 包（§3.3 字段表）。
pub fn packet_from_snapshot(snapshot: &Snapshot) -> Value {
    let mut root = Map::new();
    if let Some(client) = snapshot.client {
        root.insert("client".to_string(), json!(client.as_str()));
    }

    match (snapshot.state_number, snapshot.state_name.clone()) {
        (Some(number), Some(name)) => {
            root.insert("state".to_string(), json!({ "number": number, "name": name }));
        }
        (Some(number), None) => {
            root.insert("state".to_string(), json!({ "number": number }));
        }
        _ => {}
    }

    // `game.paused`：停滞推导（`previousPlayTime == playTime`）。tosu 还发 `focused`，
    // 我们不做前台窗口判定 ⇒ 只发 `paused`（页面只读这一项，§3.3）。
    if let Some(paused) = snapshot.paused {
        root.insert("game".to_string(), json!({ "paused": paused }));
    }

    insert_beatmap(&mut root, snapshot);
    insert_files(&mut root, snapshot);
    insert_folders(&mut root, snapshot);
    insert_mods(&mut root, snapshot);
    insert_hits(&mut root, snapshot);

    Value::Object(root)
}

/// **字段级冻结**的载荷（§3.4）：只发 `client` + `state`。
///
/// 为什么保留 `state`：页面 L1 的 `isInPlayState` 与 60 s 的 L2 窗口都靠它维生
/// （`sourceManager.js:27`、`socketHandlers.js:192-194` 的 early-return 已支持该形态）；
/// 为什么**必须**省略其余全部：图表数据不可信时任何数值都可能串局，而 `state.name`
/// 是唯一"即使图表坏了也仍然正确"的字段。
pub fn frozen_packet_from_snapshot(snapshot: &Snapshot) -> Value {
    let mut root = Map::new();
    if let Some(client) = snapshot.client {
        root.insert("client".to_string(), json!(client.as_str()));
    }
    if let (Some(number), Some(name)) = (snapshot.state_number, snapshot.state_name.clone()) {
        root.insert("state".to_string(), json!({ "number": number, "name": name }));
    } else if let Some(number) = snapshot.state_number {
        root.insert("state".to_string(), json!({ "number": number }));
    }
    Value::Object(root)
}

/// **保持帧**（Step 9g）：以上一份**验证过**的载荷为底，只把 `client`/`state` 换成本帧读数。
///
/// 与 [`frozen_packet_from_snapshot`] 的分工（这是本步的核心）：冻结帧**省略**整个图表块，
/// 页面据此重抓 `/files/beatmap/file` 并把 404 渲染给用户；保持帧**保留**它——那是同一张图上
/// 一次验证过的值（[`invariants::IDENTITY_HOLD_GRACE`]）。页面把一切都键在 identity 上，
/// 保持 ⇒ 看不到任何身份变化 ⇒ 不重抓、不报错；而它**不可能**变成另一张图（沿用同一份块）。
///
/// 本帧真正流动的只有 `state`（页面 L1 的 `isInPlayState` 与 60 s 路由窗口靠它维生）。
/// 图表块（含 `beatmap.time.live`）在整个保持期内是**上一份**的读数——这是有意的：那几帧的
/// 图表读数正是**不可信**的那部分（身份指针都读不到）。窗口期满或换成扣留行为后，冻结帧
/// （无 `beatmap`）照旧。
pub fn held_packet_from(held: &Value, snapshot: &Snapshot) -> Value {
    let mut root = held.as_object().cloned().unwrap_or_default();
    match snapshot.client {
        Some(client) => {
            root.insert("client".to_string(), json!(client.as_str()));
        }
        None => {
            root.remove("client");
        }
    }
    match (snapshot.state_number, snapshot.state_name.clone()) {
        (Some(number), Some(name)) => {
            root.insert("state".to_string(), json!({ "number": number, "name": name }));
        }
        (Some(number), None) => {
            root.insert("state".to_string(), json!({ "number": number }));
        }
        // 本帧连状态都没读到 ⇒ 不沿用旧 `state`（保持的**只有**验证过的图表块）。
        _ => {
            root.remove("state");
        }
    }
    Value::Object(root)
}

/// 文件路由可供奉的载荷（Step 9g §2；纯数据）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoutePayload<'a> {
    /// 正常态：**本帧**发布的载荷。
    Current(&'a Value),
    /// 身份保持期：**最后一张好图**的载荷（见 [`invariants::IDENTITY_HOLD_GRACE`]）。
    Held(&'a Value),
}

/// 文件路由的决策（**纯函数**；表驱动单测 `tests-local/osu_packet.rs`）。
///
/// 规则表（`live` = 24062 处于实时模式，见 `osu_compat::live_enabled`；
/// `held_fresh` = 距读者**最后一次处理帧**仍在 [`invariants::IDENTITY_HOLD_GRACE`] 之内）：
///
/// | live | frozen | holding | held_fresh | 条件 | 结果 |
/// |---|---|---|---|---|---|
/// | false | * | * | * | 回放模式 | `None`（调用方走固定回放图，与本函数无关） |
/// | true | * | * | * | 没有已发布载荷（未附着 / `unhealthy` 停帧） | `None` |
/// | true | false | * | * | 正常态 | `Current` |
/// | true | true | **true** | **true** | 保持期 + 身份逐字一致 | `Held`（200 供奉最后一张好图） |
/// | true | true | **true** | **true** | 保持期但身份对不上 | `None`（**绝不**供奉别的图） |
/// | true | true | * | **false** | 读者已停帧超过保持窗口（例如冻结窗口到期的重解析期） | `None` |
/// | true | true | false | * | 冻结且不保持（窗口已过 / 非身份类冻结） | `None`（既有行为） |
///
/// 为什么身份要"逐字一致"才算：文件路由供的是**磁盘上的文件**，页面拿着它去解析成卡片。
/// 只要被供奉的那份与"我们正在发布的图"不是同一张，页面就会把 A 图的分析挂到 B 图的
/// 身份上（缓存键污染 + 用户看到的图不对）——宁可 404（缺数据的既有语义）。
///
/// 为什么要 `held_fresh`：保持是**逐帧刷新**的事实（读者每次处理帧都会重写 `packet`/`holding`）。
/// 一旦读者不再出帧（冻结窗口到期的强制重解析、`unhealthy` 之前的空档），那份"最后一张好图"
/// 就不再是"上一次发布"，而是一个**无界的陈旧值**——这条把它限死在同一个宽限窗口内，
/// 窗口过后逐字回到 404（与改动前的冻结语义一致）。
pub fn route_payload<'a>(
    live: bool,
    frozen: bool,
    holding: bool,
    held_fresh: bool,
    published: Option<&'a Value>,
    held: Option<&'a Value>,
) -> Option<RoutePayload<'a>> {
    if !live {
        return None;
    }
    let published = published?;
    if !frozen {
        return Some(RoutePayload::Current(published));
    }
    if !holding || !held_fresh {
        return None;
    }
    let held = held?;
    if keys::identity_from_payload(published) != keys::identity_from_payload(held) {
        return None;
    }
    Some(RoutePayload::Held(held))
}

/// 两条文件路由需要的事实（**相对名**）。
///
/// `songs_folder` 是壳自己解析出的**绝对**目录（`folders.songs`），`folder` 只是谱面
/// **文件夹名**（`folders.beatmap`），`filename` 是**文件名**（`files.beatmap`）——
/// 三者拼法见 `osu_compat::map_dir_for`（与 `directPath.*` 的 win32 形状同一份规则）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BeatmapFiles {
    pub songs_folder: String,
    pub folder: String,
    pub filename: String,
    pub background: Option<String>,
}

/// 从**载荷**取两条文件路由要供奉的那份谱面（缺 `songs`/`filename` ⇒ `None` ⇒ 404，绝不猜）。
///
/// 从载荷取（而不是从当帧快照）是刻意的：供给页面的必须是"**我们正在发布的那份**"——
/// 保持期用的是最后一张好图的载荷，正常态用的是本帧载荷，两者都走这一份读法。
pub fn beatmap_files(payload: &Value) -> Option<BeatmapFiles> {
    let text = |pointer: &str| {
        payload
            .pointer(pointer)
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or("")
    };
    let songs_folder = text("/folders/songs");
    let filename = text("/files/beatmap");
    if songs_folder.is_empty() || filename.is_empty() {
        return None;
    }
    Some(BeatmapFiles {
        songs_folder: songs_folder.to_string(),
        folder: text("/folders/beatmap").to_string(),
        filename: filename.to_string(),
        background: payload
            .pointer("/files/background")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

fn insert_beatmap(root: &mut Map<String, Value>, snapshot: &Snapshot) {
    let has_beatmap = snapshot.map_id.is_some()
        || snapshot.set_id.is_some()
        || snapshot.checksum.is_some()
        || snapshot.filename.is_some()
        || snapshot.folder.is_some()
        || snapshot.version.is_some()
        || snapshot.artist.is_some()
        || snapshot.title.is_some()
        || snapshot.mapper.is_some();
    if has_beatmap {
        let mut beatmap = Map::new();
        if let Some(id) = snapshot.map_id {
            beatmap.insert("id".to_string(), json!(id));
        }
        if let Some(set) = snapshot.set_id {
            beatmap.insert("set".to_string(), json!(set));
        }
        if let Some(checksum) = snapshot.checksum.as_deref() {
            // v2 的权威键是 `checksum`（P8 校准：v2 没有 `md5`）；`md5` 是 §3.3 表里的
            // 兼容键（页面读 `md5 || checksum`），两个都给 ⇒ 两条兜底链都命中同一值。
            beatmap.insert("checksum".to_string(), json!(checksum));
            beatmap.insert("md5".to_string(), json!(checksum));
        }
        for (key, value) in [
            ("artist", snapshot.artist.as_deref()),
            ("title", snapshot.title.as_deref()),
            ("version", snapshot.version.as_deref()),
            ("mapper", snapshot.mapper.as_deref()),
        ] {
            if let Some(text) = value {
                beatmap.insert(key.to_string(), json!(text));
            }
        }
        if !beatmap.is_empty() {
            root.insert("beatmap".to_string(), Value::Object(beatmap));
        }
    }

    // `beatmap.time`：四个键**各自独立**（页面每一段都有自己的兜底），
    // `firstObject`/`lastObject` 来自 `.osu` 解析（C6 口径：文件顺序的末对象 endTime）。
    let mut time = Map::new();
    if let Some(live) = snapshot.play_time {
        time.insert("live".to_string(), json!(live));
    }
    if let Some(first) = snapshot.first_object() {
        time.insert("firstObject".to_string(), json!(first));
    }
    if let Some(last) = snapshot.last_object() {
        time.insert("lastObject".to_string(), json!(last));
    }
    if let Some(mp3) = snapshot.mp3_length {
        time.insert("mp3Length".to_string(), json!(mp3));
    }
    if !time.is_empty() {
        let beatmap = root
            .entry("beatmap".to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        if let Value::Object(map) = beatmap {
            map.insert("time".to_string(), Value::Object(time));
        }
    }
}

fn insert_files(root: &mut Map<String, Value>, snapshot: &Snapshot) {
    let mut files = Map::new();
    if let Some(filename) = snapshot.filename.as_deref() {
        files.insert("beatmap".to_string(), json!(filename));
    }
    if let Some(background) = snapshot.background.as_deref() {
        files.insert("background".to_string(), json!(background));
    }
    if let Some(audio) = snapshot.audio.as_deref() {
        files.insert("audio".to_string(), json!(audio));
    }
    if !files.is_empty() {
        root.insert("files".to_string(), Value::Object(files));
    }

    // `directPath.*` = `path.join(folder, name)` 的 win32 形态（`folder` 为空/`.` ⇒ 名字本身）。
    let mut direct = Map::new();
    if let Some(value) = snapshot
        .filename
        .as_deref()
        .map(|name| direct_path_join(snapshot.folder.as_deref(), name))
    {
        direct.insert("beatmapFile".to_string(), json!(value));
    }
    if let Some(value) = snapshot
        .background
        .as_deref()
        .map(|name| direct_path_join(snapshot.folder.as_deref(), name))
    {
        direct.insert("beatmapBackground".to_string(), json!(value));
    }
    if let Some(value) = snapshot
        .audio
        .as_deref()
        .map(|name| direct_path_join(snapshot.folder.as_deref(), name))
    {
        direct.insert("beatmapAudio".to_string(), json!(value));
    }
    if let Some(folder) = snapshot.folder.as_deref() {
        direct.insert("beatmapFolder".to_string(), json!(folder));
    }
    if !direct.is_empty() {
        root.insert("directPath".to_string(), Value::Object(direct));
    }
}

/// `path.join(folder, name)` 的 win32 形态（**唯一**实现，`invariants::i03` 也用它）。
///
/// 为什么不是朴素的 `folder\name`：lazer 的 `folders.beatmap` 逐字是 `"."`
/// （参照实现 `safeJoin('')` = `path.join('')` = `'.'`，P8 golden 帧实测），而 Node 的
/// `path.join('.', key)` 会把 `'.'` 段吃掉 ⇒ `directPath.beatmapFile == files.beatmap`
/// （P5 的逐字节断言）。所以这里把空段与 `'.'` 段丢掉后拼接；**`..` 不做规约**
/// （两个客户端的 `folder` 只会是真实目录名或 `'.'`，没有 `..` 形态——多写一条规约等于
/// 凭空发明 tosu 没有的行为）。
pub fn direct_path_join(folder: Option<&str>, name: &str) -> String {
    let segments: Vec<&str> = folder
        .unwrap_or("")
        .split(['\\', '/'])
        .filter(|segment| !segment.is_empty() && *segment != ".")
        .collect();
    if segments.is_empty() {
        name.to_string()
    } else {
        format!("{}\\{}", segments.join("\\"), name)
    }
}

fn insert_folders(root: &mut Map<String, Value>, snapshot: &Snapshot) {
    let mut folders = Map::new();
    if let Some(game) = snapshot.game_folder.as_deref() {
        folders.insert("game".to_string(), json!(game));
    }
    if let Some(songs) = snapshot.songs_folder.as_deref() {
        folders.insert("songs".to_string(), json!(songs));
    }
    if let Some(beatmap) = snapshot.folder.as_deref() {
        folders.insert("beatmap".to_string(), json!(beatmap));
    }
    if !folders.is_empty() {
        root.insert("folders".to_string(), Value::Object(folders));
    }
}

/// 三个 mods 键：**恒发**（值或 `null`），且按状态选取（`invariants::mask_for_state`）。
///
/// **lazer 分支**（Step 10B）：mods 不是位掩码（`ModsJson` 是 acronym 列表），所以走
/// [`lazer_mods_value`] 的 v2 形状；槽位照 P8 golden 帧的形状——`play` 与 `resultsScreen`
/// 发**同一份当前态** mods，`menu.mods` 恒 `null`（lazer 没有"菜单 mod"这个对象；
/// 帧实测 `menu.mods: null`）。读不到 mods ⇒ 两个槽都发 `null`（**绝不**沿用上一帧签名）。
fn insert_mods(root: &mut Map<String, Value>, snapshot: &Snapshot) {
    if snapshot.client == Some(Client::Lazer) {
        let current = snapshot.lazer_mods.as_deref().map(lazer_mods_value);
        root.insert(
            "menu".to_string(),
            json!({ "mods": Value::Null }),
        );
        root.insert(
            "play".to_string(),
            json!({ "mods": current.clone().unwrap_or(Value::Null) }),
        );
        root.insert(
            "resultsScreen".to_string(),
            json!({ "mods": current.unwrap_or(Value::Null) }),
        );
        return;
    }
    let (menu, play, result) = invariants::mask_for_state(snapshot);
    root.insert(
        "menu".to_string(),
        json!({ "mods": menu.map(keys::menu_mods_value) }),
    );
    root.insert(
        "play".to_string(),
        json!({ "mods": play.map(keys::menu_mods_value) }),
    );
    root.insert(
        "resultsScreen".to_string(),
        json!({ "mods": result.map(keys::menu_mods_value) }),
    );
}

/// lazer 的 mods 对象（v2 形状）：`{number, name, array:[{acronym, settings?}], rate}`。
///
/// - `acronym` 逐字来自内存里的 mod（大写化后原样发出，**不过已知码白名单**——
///   页面在 lazer 分支也是 `modCodes.add(acronym)`，见 `js/app/modData.js:164-167`）；
/// - `settings.speed_change` / `settings.overall_difficulty` 是页面 `speedRate`/`odFlag`
///   在 lazer 分支的**唯一**来源（`modData.js:150-180`）⇒ 有值就发；
/// - `number` 由 acronym 反算**页面已知的那 6 位**（`keys::MOD_BIT_FLAGS`）：它只可能补出
///   acronym 已经表达过的码，不可能引入新码；`name` 是 acronym 的拼接（页面按已知码做前缀
///   匹配 ⇒ 码集与逐个 acronym 一致）；`rate` 取第一个 `speed_change`（页面不读它，仅供
///   与 tosu 的 v2 对象同形）。
pub fn lazer_mods_value(mods: &[LazerMod]) -> Value {
    let acronyms: Vec<String> = mods
        .iter()
        .map(|mod_| mod_.acronym.trim().to_uppercase())
        .filter(|acronym| !acronym.is_empty())
        .collect();
    let array: Vec<Value> = mods
        .iter()
        .filter(|mod_| !mod_.acronym.trim().is_empty())
        .map(|mod_| {
            let mut item = Map::new();
            item.insert(
                "acronym".to_string(),
                json!(mod_.acronym.trim().to_uppercase()),
            );
            let mut settings = Map::new();
            if let Some(speed_change) = mod_.speed_change {
                if speed_change.is_finite() && speed_change > 0.0 {
                    settings.insert("speed_change".to_string(), json!(speed_change));
                }
            }
            if let Some(overall_difficulty) = mod_.overall_difficulty {
                if overall_difficulty.is_finite() {
                    settings.insert(
                        "overall_difficulty".to_string(),
                        json!(overall_difficulty),
                    );
                }
            }
            if !settings.is_empty() {
                item.insert("settings".to_string(), Value::Object(settings));
            }
            Value::Object(item)
        })
        .collect();
    let mut number = 0u32;
    for (bit, code) in keys::MOD_BIT_FLAGS {
        if acronyms.iter().any(|acronym| acronym == code) {
            number |= bit;
        }
    }
    let rate = mods
        .iter()
        .filter_map(|mod_| mod_.speed_change)
        .find(|value| value.is_finite() && *value > 0.0)
        .unwrap_or(1.0);
    json!({
        "number": number,
        "name": acronyms.concat(),
        "array": Value::Array(array),
        "rate": rate,
    })
}

/// hits：只发**该态**的那一条（`play` / `resultsScreen`），键集 = 页面消费的 6 键。
fn insert_hits(root: &mut Map<String, Value>, snapshot: &Snapshot) {
    if let Some(hits) = snapshot.play_hits {
        let play = root
            .entry("play".to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        if let Value::Object(map) = play {
            map.insert("hits".to_string(), hits.to_json());
        }
    }
    if let Some(hits) = snapshot.result_hits {
        let results = root
            .entry("resultsScreen".to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        if let Value::Object(map) = results {
            map.insert("hits".to_string(), hits.to_json());
        }
    }
}

#[cfg(test)]
#[path = "../../tests-local/osu_packet.rs"]
mod tests_packet;
