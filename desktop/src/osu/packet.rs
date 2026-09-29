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
use crate::osu::model::Snapshot;
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

    // `directPath.*` = `path.join(folder, name)` 的 win32 形态（`folder` 为空 ⇒ 名字本身）。
    let joined = |folder: Option<&str>, name: Option<&str>| -> Option<String> {
        name.map(|name| match folder {
            Some(folder) if !folder.is_empty() => format!("{folder}\\{name}"),
            _ => name.to_string(),
        })
    };
    let mut direct = Map::new();
    if let Some(value) = joined(snapshot.folder.as_deref(), snapshot.filename.as_deref()) {
        direct.insert("beatmapFile".to_string(), json!(value));
    }
    if let Some(value) = joined(snapshot.folder.as_deref(), snapshot.background.as_deref()) {
        direct.insert("beatmapBackground".to_string(), json!(value));
    }
    if let Some(value) = joined(snapshot.folder.as_deref(), snapshot.audio.as_deref()) {
        direct.insert("beatmapAudio".to_string(), json!(value));
    }
    if let Some(folder) = snapshot.folder.as_deref() {
        direct.insert("beatmapFolder".to_string(), json!(folder));
    }
    if !direct.is_empty() {
        root.insert("directPath".to_string(), Value::Object(direct));
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
fn insert_mods(root: &mut Map<String, Value>, snapshot: &Snapshot) {
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
