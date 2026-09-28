// 页面同形的两个派生字符串：`identity` 与 `modSignature`（**纯函数**）。
//
// 这是缓存键的断言面：与 tosu 载荷算出来的值必须**逐字节相等**，否则页面会
// 静默丢缓存（计划 SC2 / DEC-01）。因此本文件的规则是**逐行照抄页面语义**，
// 不是"重新设计一个等价物"——任何改动都要回去核对下面引用的行号。
//
// 来源（只读页面代码）：
// - `js/app/socketHandlers.js:196-268`（normalizeText / normalizePathText /
//   normalizeNumberText / identity 四段降级链 `id:` → `hash:` → `path:` → `meta:`）
// - `js/app/modData.js:184-229`（speedRate / odFlag / cvtFlag / classic 与 4 段签名）
// - `config.js:121-131`（knownCodes 12 个 + bitFlags 6 个）

use crate::osu::model::{Client, DerivedKeys, Snapshot};
use serde_json::{json, Value};

/// `config.js:122` 的 knownCodes（顺序即页面 `Array.prototype.sort()` 之后的字典序）。
pub const KNOWN_MOD_CODES: &[&str] = &[
    "CL", "DA", "DC", "DT", "EZ", "HO", "HR", "IN", "MR", "NC", "SV2",
];

/// `config.js:123-130` 的 bitFlags（stable 位掩码 → 代码）。
pub const MOD_BIT_FLAGS: &[(u32, &str)] = &[
    (1 << 1, "EZ"),   // 2
    (1 << 4, "HR"),   // 16
    (1 << 6, "DT"),   // 64
    (1 << 8, "HT"),   // 256
    (1 << 9, "NC"),   // 512
    (1 << 29, "SV2"), // 536870912
];

/// `socketHandlers.js:217` 的路径规范化与 `:202` 的 `String(value)` 组合：
/// 原样字符串 → 先 trim → 反斜杠转斜杠 → 连续的 `/` 折叠成一个 → **整串小写**。
pub fn normalize_path_text(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let slash = trimmed.replace('\\', "/");
    let mut collapsed = String::with_capacity(slash.len());
    let mut last_was_slash = false;
    for ch in slash.chars() {
        if ch == '/' {
            if last_was_slash {
                continue;
            }
            last_was_slash = true;
        } else {
            last_was_slash = false;
        }
        collapsed.push(ch);
    }
    collapsed.to_lowercase()
}

/// `socketHandlers.js:207-213` 的 `normalizeNumberText`：有限且 > 0 才输出
/// `String(Math.trunc(num))`，否则空串。`f64` 的 `trunc()` 与 `Math.trunc` 同义。
pub fn normalize_number_text(value: f64) -> String {
    if !value.is_finite() || value <= 0.0 {
        return String::new();
    }
    let truncated = value.trunc();
    // JS 的 String(Number) 对整数值不给小数点；这里 id/set 都是整数域。
    format!("{}", truncated as i64)
}

/// `modData.js:28-50` 的 `addCodesFromString`：大写、剥掉 `[^A-Z0-9]`，然后按
/// **已排序**的 knownCodes 逐一做前缀匹配（匹配不上就前进 1 个字符）。
pub fn add_codes_from_string(codes: &mut Vec<&'static str>, value: &str) {
    let normalized: String = value
        .chars()
        .flat_map(|c| c.to_uppercase())
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    let mut index = 0usize;
    while index < normalized.len() {
        let mut matched = false;
        for code in KNOWN_MOD_CODES {
            if normalized[index..].starts_with(code) {
                if !codes.contains(code) {
                    codes.push(code);
                }
                index += code.len();
                matched = true;
                break;
            }
        }
        if !matched {
            index += 1;
        }
    }
}

/// `modData.js:164-167`（lazer 专属）：把单个 acronym 原样大写后收进代码集，
/// **不过 knownCodes 白名单**（页面在 lazer 分支里是 `modCodes.add(acronym)`）。
/// 返回值借用 `KNOWN_MOD_CODES` 里的字面量；白名单外的代码无法表达为 `&'static str`
/// 时返回 `None`——stable 侧永远不会走到这里。
pub fn add_code_acronym(codes: &mut Vec<&'static str>, acronym: &str) -> Option<()> {
    let upper = acronym.to_uppercase();
    let code = KNOWN_MOD_CODES.iter().find(|c| **c == upper)?;
    if !codes.contains(code) {
        codes.push(code);
    }
    Some(())
}

/// `modData.js:52-62` 的 `addCodesFromNumber`：按 bitFlags 逐位判定。
pub fn add_codes_from_number(codes: &mut Vec<&'static str>, number: u32) {
    for (bit, code) in MOD_BIT_FLAGS {
        if number & bit != 0 && !codes.contains(code) {
            codes.push(code);
        }
    }
}

/// 从一帧载荷（**我们的快照包或 tosu 的原始载荷都行**——这是 apples-to-apples 的关键）
/// 提取页面会看到的 mod 代码集合。
///
/// 取用顺序照 `modData.js:11-26,73-77`：`play.mods` 先，然后 `menu.mods`、
/// `resultsScreen.mods`（`preferPlayMods` 为 false 时的默认顺序）。`null` 一律跳过
/// （`:79`）。
pub fn mod_codes_from_payload(payload: &Value, client: Client) -> Vec<&'static str> {
    let mut codes: Vec<&'static str> = Vec::new();
    for pointer in ["/play/mods", "/menu/mods", "/resultsScreen/mods"] {
        let Some(mods) = payload.pointer(pointer).filter(|v| !v.is_null()) else {
            continue;
        };
        // 裸数组（`modData.js:128-135`）与对象（`:87-127`）两种形态。
        let object = if mods.is_array() { None } else { Some(mods) };
        if let Some(obj) = object {
            for key in ["name", "str", "acronym"] {
                if let Some(text) = obj.get(key).and_then(|v| v.as_str()) {
                    add_codes_from_string(&mut codes, text);
                }
            }
            for key in ["number", "num"] {
                if let Some(num) = obj.get(key).and_then(|v| v.as_f64()) {
                    if num.is_finite() {
                        add_codes_from_number(&mut codes, num as u32);
                    }
                }
            }
        }
        let array = if mods.is_array() {
            mods.as_array()
        } else {
            mods.get("array").and_then(|v| v.as_array())
        };
        if let Some(items) = array {
            for item in items {
                if let Some(text) = item.as_str() {
                    add_codes_from_string(&mut codes, text);
                    continue;
                }
                if let Some(obj) = item.as_object() {
                    // lazer 专属：array[].acronym 与 array[].settings（`:164-180`）。
                    if let Some(acronym) = obj.get("acronym").and_then(|v| v.as_str()) {
                        add_codes_from_string(&mut codes, acronym);
                        if client == Client::Lazer {
                            add_code_acronym(&mut codes, acronym);
                        }
                    }
                }
            }
        }
    }
    codes
}

/// `modData.js:184-206` 的 speedRate / odFlag / cvtFlag。
///
/// stable（本步唯一实现的客户端）语义：
/// - `speedRate = 1.5`（NC|DT）、`0.75`（HT|DC）、否则 `1.0`
/// - `odFlag = "HR"` → `"EZ"` → 否则 `"none"`
/// - `cvtFlag` 恒 `"none"`（`IN`/`HO` 是 lazer 专属分支）
/// lazer 的 `settings.speed_change` / `settings.overall_difficulty` 取值路径一并实现，
/// 但 lazer 帧本步不产出（E 步接入）。
fn mod_dimensions(
    payload: &Value,
    client: Client,
    codes: &[&str],
) -> (f64, Option<String>, Option<&'static str>) {
    let mut speed_rate = 1.0f64;

    if client == Client::Lazer {
        let mut lazer_speed_change: Option<f64> = None;
        let mut da_overall_difficulty: Option<f64> = None;
        for pointer in ["/play/mods", "/menu/mods", "/resultsScreen/mods"] {
            let Some(mods) = payload.pointer(pointer) else {
                continue;
            };
            let items = if mods.is_array() {
                mods.as_array()
            } else {
                mods.get("array").and_then(|v| v.as_array())
            };
            let Some(items) = items else { continue };
            for item in items {
                let Some(obj) = item.as_object() else {
                    continue;
                };
                let acronym = obj
                    .get("acronym")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_uppercase();
                if acronym == "DA" {
                    if let Some(od) = obj
                        .get("settings")
                        .and_then(|s| s.get("overall_difficulty"))
                        .and_then(|v| v.as_f64())
                    {
                        if od.is_finite() {
                            da_overall_difficulty = Some(od);
                        }
                    }
                }
                if let Some(sc) = obj
                    .get("settings")
                    .and_then(|s| s.get("speed_change"))
                    .and_then(|v| v.as_f64())
                {
                    if sc.is_finite() && sc > 0.0 {
                        lazer_speed_change = Some(sc);
                    }
                }
            }
        }
        if let Some(sc) = lazer_speed_change {
            speed_rate = sc;
        } else if codes.contains(&"NC") || codes.contains(&"DT") {
            speed_rate = 1.5;
        } else if codes.contains(&"HT") || codes.contains(&"DC") {
            speed_rate = 0.75;
        }
        // `modData.js:192-198`：lazer 下 DA 的 overall_difficulty 优先于 HR/EZ，
        // 输出的是**数值本身**（`String(daOverallDifficulty)`），不是字面量 "DA"。
        let od_flag: Option<String> =
            da_overall_difficulty.map(|od| format!("{od}")).or_else(|| {
                if codes.contains(&"HR") {
                    Some("HR".to_string())
                } else if codes.contains(&"EZ") {
                    Some("EZ".to_string())
                } else {
                    None
                }
            });
        let cvt_flag: Option<&'static str> = if codes.contains(&"IN") {
            Some("IN")
        } else if codes.contains(&"HO") {
            Some("HO")
        } else {
            None
        };
        return (speed_rate, od_flag, cvt_flag);
    }

    if codes.contains(&"NC") || codes.contains(&"DT") {
        speed_rate = 1.5;
    } else if codes.contains(&"HT") || codes.contains(&"DC") {
        speed_rate = 0.75;
    }
    let od_flag: Option<String> = if codes.contains(&"HR") {
        Some("HR".to_string())
    } else if codes.contains(&"EZ") {
        Some("EZ".to_string())
    } else {
        None
    };
    // stable 的 cvtFlag 恒 none（`IN`/`HO` 是 lazer 专属，见 `modData.js:200-206`）。
    (speed_rate, od_flag, None)
}

/// `modData.js:224-229` 的四段签名：`"{speedRate:.5}|{odFlag|none}|{cvtFlag|none}|{classic}"`。
///
/// `classic = !modCodes.has("SV2") && (client !== "lazer" || modCodes.has("CL"))`
/// （`modData.js:219-220`）——stable 无 SV2 即 `classic=true`。
pub fn mod_signature_from_payload(payload: &Value, client: Client) -> String {
    let codes = mod_codes_from_payload(payload, client);
    let (speed_rate, od_flag, cvt_flag) = mod_dimensions(payload, client, &codes);
    let classic = !codes.contains(&"SV2") && (client != Client::Lazer || codes.contains(&"CL"));
    format!(
        "{:.5}|{}|{}|{}",
        speed_rate,
        od_flag.as_deref().unwrap_or("none"),
        cvt_flag.unwrap_or("none"),
        if classic { "1" } else { "0" }
    )
}

/// `socketHandlers.js:251-267` 的身份字符串：
/// `id:<trunc(id)>`（有限且 > 0）→ `hash:<md5|checksum 小写+trim>` →
/// `path:<文件名/相对键 归一化>` 依次追加，`|` 连接；**三段全空**时才用
/// `meta:<artist::title::version::mapper 小写>`。
pub fn identity_from_payload(payload: &Value) -> String {
    let beatmap = payload.get("beatmap");
    let text = |value: Option<&Value>| -> String {
        match value {
            None | Some(Value::Null) => String::new(),
            Some(Value::String(s)) => s.trim().to_string(),
            Some(other) => other.to_string().trim().to_string(),
        }
    };
    let id = normalize_number_text(
        beatmap
            .and_then(|b| b.get("id"))
            .and_then(|v| v.as_f64())
            .unwrap_or(f64::NAN),
    );
    // `socketHandlers.js:216`：`beatmap.md5 || beatmap.checksum`（前者非空才用前者）。
    let md5 = text(beatmap.and_then(|b| b.get("md5")));
    let hash_raw = if md5.is_empty() {
        text(beatmap.and_then(|b| b.get("checksum")))
    } else {
        md5
    };
    let hash = hash_raw.to_lowercase();
    // `socketHandlers.js:217`：`files.beatmap || directPath.beatmapFile`。
    let files_beatmap = text(payload.pointer("/files/beatmap"));
    let direct_beatmap_file = text(payload.pointer("/directPath/beatmapFile"));
    let path = normalize_path_text(if files_beatmap.is_empty() {
        &direct_beatmap_file
    } else {
        &files_beatmap
    });

    let mut parts: Vec<String> = Vec::new();
    if !id.is_empty() {
        parts.push(format!("id:{id}"));
    }
    if !hash.is_empty() {
        parts.push(format!("hash:{hash}"));
    }
    if !path.is_empty() {
        parts.push(format!("path:{path}"));
    }

    if parts.is_empty() {
        let meta = [
            text(beatmap.and_then(|b| b.get("artist"))),
            text(beatmap.and_then(|b| b.get("title"))),
            text(beatmap.and_then(|b| b.get("version"))),
            text(beatmap.and_then(|b| b.get("mapper"))),
        ]
        .join("::")
        .to_lowercase();
        // `:262`：去掉冒号后仍有内容才算"有元信息身份"。
        if meta.replace(':', "").len() > 0 {
            parts.push(format!("meta:{meta}"));
        }
    }
    parts.join("|")
}

/// 快照 → `{identity, mod_signature}`：先按 `packet.rs` 的载荷形态折叠，再走上面两条
/// **同一份**规则。对拍时 tosu 侧也走这里（喂 tosu 原始载荷），保证是 apples-to-apples。
pub fn derive(snapshot: &Snapshot) -> DerivedKeys {
    let payload = snapshot.to_packet();
    let client = snapshot.client.unwrap_or(Client::Stable);
    DerivedKeys {
        identity: identity_from_payload(&payload),
        mod_signature: mod_signature_from_payload(&payload, client),
    }
}

/// 菜单 mod 位掩码对应的代码集合（本步只用于对照展示/日志；页面语义在 `mod_codes_from_payload`）。
pub fn menu_mod_codes(mask: u32) -> Vec<&'static str> {
    let mut codes = Vec::new();
    add_codes_from_number(&mut codes, mask);
    codes
}

/// 稳定版 mod 位值表（tosu `utils/osuMods.types.ts` 的 `bitValues`，为"名字生成"服务）。
///
/// 与 `MOD_BIT_FLAGS` 的分工：`MOD_BIT_FLAGS` 是**页面**的已知代码白名单（只 6 位），
/// 这张表是**载荷名字**的编码表（bit 值 → 短名），后者决定 `menu.mods.name` 长什么样。
/// 只读研究参照实现的这一张常量表，不抄任何代码。
const MOD_BIT_NAMES: &[(u32, &str)] = &[
    (1 << 0, "NF"),
    (1 << 1, "EZ"),
    (1 << 2, "TD"),
    (1 << 3, "HD"),
    (1 << 4, "HR"),
    (1 << 5, "SD"),
    (1 << 6, "DT"),
    (1 << 7, "RX"),
    (1 << 8, "HT"),
    (1 << 9, "NC"),
    (1 << 10, "FL"),
    (1 << 11, "AT"),
    (1 << 12, "SO"),
    (1 << 13, "AP"),
    (1 << 14, "PF"),
    (1 << 15, "4K"),
    (1 << 16, "5K"),
    (1 << 17, "6K"),
    (1 << 18, "7K"),
    (1 << 19, "8K"),
    (1 << 20, "FI"),
    (1 << 21, "RD"),
    (1 << 22, "CN"),
    (1 << 23, "TG"),
    (1 << 24, "9K"),
    (1 << 25, "10K"),
    (1 << 26, "1K"),
    (1 << 27, "2K"),
    (1 << 28, "3K"),
    (1 << 29, "SV2"),
    (1 << 30, "CO"),
];

/// 位掩码 → 短名（参照实现 `modsName` 的位序 + 三条合并规则 `DTNC→NC`、`SDPF→PF`、`ATCN→CN`）。
///
/// `0` → `"NM"`（页面靠 `number: 0` 触发 `hasExplicitNoMod`，名字只是为了与参照实现同形）。
pub fn mods_name_from_mask(mask: u32) -> String {
    if mask == 0 {
        return "NM".to_string();
    }
    let mut parts = String::new();
    for (bit, name) in MOD_BIT_NAMES {
        if mask & bit != 0 {
            parts.push_str(name);
        }
    }
    parts
        .replace("DTNC", "NC")
        .replace("SDPF", "PF")
        .replace("ATCN", "CN")
}

/// 位掩码 → v2 形状的 mods 对象（`{checksum, number, name, array, rate}`）。
///
/// `checksum = md5(JSON.stringify(array))`（参照实现 `osuMods.ts` 的 `textMD5(JSON.stringify(array))`）；
/// `array = name.match(/.{1,2}/g).map(r => ({acronym: r}))`——**按 2 字符切分**，所以三字母的
/// `SV2` 会变成 `[{acronym:"SV"},{acronym:"2"}]`（这是参照实现自身的形状怪癖：`name` 才是
/// 权威，`number` 是位掩码本体）。页面**不读** `checksum`/`array`，但保持同形让 §3.3 的
/// "逐字节同形"可检验、也让 `modSignature` 的计算路径完全一致（`SV2` 的判定走 `number` 位）。
pub fn menu_mods_value(mask: u32) -> Value {
    let name = mods_name_from_mask(mask);
    let array: Vec<Value> = name
        .as_bytes()
        .chunks(2)
        .map(|pair| {
            json!({
                "acronym": String::from_utf8_lossy(pair).to_string()
            })
        })
        .collect();
    let array_value = Value::Array(array);
    let checksum = crate::server::md5_hex(&array_value.to_string());
    let number = mask as i64;
    let rate = if mask & ((1 << 6) | (1 << 9)) != 0 {
        1.5
    } else if mask & ((1 << 10) | (1 << 8)) != 0 {
        // HT(1<<8) | DC(1<<10)：注意 DC 在 stable 位表里没有独立位，
        // 参照实现对 HT/DC 的统一判断在这里落到 HT 位上。
        0.75
    } else {
        1.0
    };
    json!({
        "checksum": checksum,
        "number": number,
        "name": name,
        "array": array_value,
        "rate": rate,
    })
}
