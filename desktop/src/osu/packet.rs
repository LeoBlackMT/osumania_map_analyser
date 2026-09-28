// 快照 → tosu v2 形状载荷（**纯函数**）。
//
// 本步（B2）只需要 §3.3 字段表里"能由 `baseAddr` + `statusPtr` 填出来"的那一部分：
// `client` / `state.{number,name}` / `beatmap.{id,set,checksum,artist,title,version,mapper}`
// / `files.beatmap` / `directPath.beatmapFile` / `folders.beatmap` / `menu.mods`。
//
// 明确留空（**C2 的 TODO**，不是遗漏）：
// - `beatmap.time.live`（playTime 链在 C2）；`firstObject`/`lastObject` 已由 B3 的
//   `.osu` 解析给出（见下），`mp3Length` 仍缺
// - `play.mods` / `resultsScreen.mods`（局内 mods 需要 ScoreV2 位 XOR 还原来还原 mod 位，
//   属 C2；**菜单 mods 已实现**，见 `menu_mods_value`）
// - `play.hits` / `resultsScreen.hits`、`session`、`profile`、`settings`、`leaderboard`、
//   `performance`、`tourney`（§3.3"明确不提供"或 C2 起）
//
// 形状依据：`evidence/P8/golden-*.json` 的 tosu v4.25.1 实帧（键名/类型/大小写逐字对齐）。
// `files.beatmap` 是**文件名**（stable）/相对键（lazer），`folders.beatmap` 是**名字**
// 而不是绝对目录（计划 §3.3 的加粗警告：写错会让 `identity` 的 `path:` 段与 tosu 不同）。

use crate::osu::keys;
use crate::osu::model::Snapshot;
use serde_json::{json, Map, Value};

/// 合成最小 v2 包。缺失字段一律**不出现**（而不是给 `null`）——与 tosu 的
/// "缺哪个键就不发哪个键"一致，页面的降级读取依赖这一点。
pub fn packet_from_snapshot(snapshot: &Snapshot) -> Value {
    let mut root = Map::new();
    if let Some(client) = snapshot.client {
        root.insert("client".to_string(), json!(client.as_str()));
    }

    match (snapshot.state_number, snapshot.state_name.clone()) {
        (Some(number), Some(name)) => {
            root.insert(
                "state".to_string(),
                json!({ "number": number, "name": name }),
            );
        }
        (Some(number), None) => {
            root.insert("state".to_string(), json!({ "number": number }));
        }
        _ => {}
    }

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
            // 页面的 `beatmap.md5 || beatmap.checksum`：tosu 的两个键都给同一个值，
            // 这里也两个都给（`:216`，缺一段就会走不同的兜底分支）。
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

    // `beatmap.time.firstObject` / `lastObject`：**来自 `.osu` 解析**（C6），不是内存。
    // 与 tosu 同形：整段 `beatmap.time` 在同一次插入里给出（缺一个键就不发整段，
    // 避免页面读到半截时间窗；`live` 属 C2，本步不发）。
    match (snapshot.first_object(), snapshot.last_object()) {
        (Some(first), Some(last)) => {
            let beatmap = root
                .entry("beatmap".to_string())
                .or_insert_with(|| Value::Object(Map::new()));
            if let Value::Object(map) = beatmap {
                map.insert(
                    "time".to_string(),
                    json!({ "firstObject": first, "lastObject": last }),
                );
            }
        }
        _ => {}
    }

    if let Some(filename) = snapshot.filename.as_deref() {
        root.insert("files".to_string(), json!({ "beatmap": filename }));
    }
    match (snapshot.folder.as_deref(), snapshot.filename.as_deref()) {
        (Some(folder), Some(filename)) => {
            // `path.join(folder, filename)` 的 Win32 形态（反斜杠；lazer 的 folder 恒空）。
            let joined = if folder.is_empty() {
                filename.to_string()
            } else {
                format!("{folder}\\{filename}")
            };
            root.insert(
                "directPath".to_string(),
                json!({ "beatmapFile": joined, "beatmapFolder": folder }),
            );
            root.insert("folders".to_string(), json!({ "beatmap": folder }));
        }
        (Some(folder), None) => {
            root.insert("folders".to_string(), json!({ "beatmap": folder }));
        }
        (None, Some(filename)) => {
            root.insert("directPath".to_string(), json!({ "beatmapFile": filename }));
        }
        (None, None) => {}
    }

    // 绝对 songs 目录（B3 新增链的派生结果；**不是**内存里的原始字符串——原始 cfg 值见
    // `Snapshot::songs_cfg_value`，只在证据里出现）。tosu 的 `folders.songs` 就是这个值。
    if let Some(songs) = snapshot.songs_folder.as_deref() {
        let folders = root
            .entry("folders".to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        if let Value::Object(map) = folders {
            map.insert("songs".to_string(), json!(songs));
        }
    }
    // 菜单 mods：**掩码有值就发**（含 0 = NM）。`0` 必须发（`number: 0` 是页面
    // `hasExplicitNoMod` 的来源之一，`modData.js:109`），缺失才是"没有这条信息"。
    if let Some(mask) = snapshot.menu_mods_mask {
        root.insert(
            "menu".to_string(),
            json!({ "mods": keys::menu_mods_value(mask) }),
        );
    }

    Value::Object(root)
}
