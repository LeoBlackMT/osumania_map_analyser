// mods.rs —— lazer 的 mod 自定义设置（自定义倍速 / DA 自定义 OD）的读取准备。
//
// 现状（2026-10）：`frame::read_lazer_mods` 从 `SelectedMods` 的 mod 对象上按固定
// 候选位移猜 bindable 值。倍速（DT/HT 的 `BindableNumber<double>`）能读；但 DA 的
// `OverallDifficulty` 是 `DifficultyBindable : Bindable<float?>`（可空 float），
// 按 f64 读 `+0x20` 既读不到 OD，也无法区分 `DrainRate` 与 `OverallDifficulty`
// （基类字段顺序里 DrainRate 在前）——所以 DA 自定义 OD 目前不可靠。
//
// 权威来源是 `ScoreInfo.ModsJson`：`Player.load` 开局时执行
// `Score.ScoreInfo.Mods = gameplayMods`，setter 调 `updateModsJson()`，把
// `APIMod[]` 序列化成 JSON；`APIMod(Mod)` 只收录**非默认**设置
// （`if (!bindable.IsDefault)`），所以自定义倍速/OD 一定在里面，默认值一定不在。
// 它名字自描述（`speed_change` / `overall_difficulty`）、与 lazer 版本解耦，
// 且能覆盖任意 ruleset 的 mod——比逐个解 bindable 稳。
//
// 本文件是准备工作，不改变现有读取行为：
// 1. `lazer_mods_from_mods_json`：JSON → `Vec<LazerMod>`（单元测试覆盖）；
// 2. `probe_once`：`MMA_LAZER_MODS_PROBE=1` 时，从屏幕栈顶（Player）扫出
//    `<Score>` → `<ScoreInfo>` → `ModsJson`，把结果写进壳日志。只读、只记日志，
//    找不到就静默返回，不参与载荷。
//
// 下一步（拿到一次真机日志后）：把 Score/ScoreInfo 的位移固化进偏移表的 `play` 段
// （与 `results` 段同形），让 `read_lazer_mods` 优先读 `ModsJson`，`SelectedMods`
// 的启发式降为兜底。

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::osu::model::LazerMod;
use crate::osu::offsets::OffsetTable;

use super::screen::read_eetype;
use super::source::{read_f64, read_ptr_field, read_string, read_u8, Source, StringLayout};

/// `APIMod[]` 的 JSON 形状：
/// `[{"acronym":"DT","settings":{"speed_change":1.2}}, {"acronym":"DA","settings":{"overall_difficulty":8.5}}]`
///
/// 判据（任一不过 ⇒ `None`）：顶层是数组；每项是对象且 `acronym` 是非空字符串。
/// `settings` 缺失/不是对象都按"没有非默认设置"处理；未知键忽略。
/// 空数组是合法的"无 mod"（lazer 自身会写空串而不是 `[]`，这里只做容错）。
pub fn lazer_mods_from_mods_json(text: &str) -> Option<Vec<LazerMod>> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let array = value.as_array()?;
    let mut mods = Vec::with_capacity(array.len());
    for item in array {
        let object = item.as_object()?;
        let acronym = object.get("acronym")?.as_str()?.trim();
        if acronym.is_empty() {
            return None;
        }
        let settings = object.get("settings").and_then(|value| value.as_object());
        let number = |key: &str| {
            settings
                .and_then(|settings| settings.get(key))
                .and_then(|value| value.as_f64())
                .filter(|value| value.is_finite())
        };
        mods.push(LazerMod {
            acronym: acronym.to_uppercase(),
            // 速率必须为正；0/负数不是有效倍速。
            speed_change: number("speed_change").filter(|value| *value > 0.0),
            // OD 0 合法。
            overall_difficulty: number("overall_difficulty"),
        });
    }
    Some(mods)
}

// 屏幕对象上找 `<Score>k__BackingField`；Score 上找 `ScoreInfo`；ScoreInfo 上找
// `ModsJson`。三个字段的精确位移都依赖 lazer 构建，探针先用"逐个引用字段 + 类型名"
// 找到对象，再由字符串内容确认，不写死任何位移。
const SCAN_START: u64 = 0x8;
const SCREEN_SCAN_END: u64 = 0x800;
const SCORE_SCAN_END: u64 = 0x40;
const SCOREINFO_SCAN_END: u64 = 0x400;
const PROBE_INTERVAL_MS: u64 = 1000;

static LAST_ATTEMPT_MS: AtomicU64 = AtomicU64::new(0);
static LAST_DONE_SCREEN: AtomicU64 = AtomicU64::new(0);
static LAST_REPORTED_SCREEN: AtomicU64 = AtomicU64::new(0);

/// 探针入口：只在 `MMA_LAZER_MODS_PROBE` 设置时工作；每个屏幕对象最多每秒尝试一次。
/// 成功后不再重复；失败时每个屏幕对象只报一次（仍会节流重试，等 `Player` 加载完）。
pub fn probe_once(
    source: &dyn Source,
    table: &OffsetTable,
    screen_top: u64,
    layout: Option<StringLayout>,
    modules: &[(u64, String)],
) {
    if std::env::var_os("MMA_LAZER_MODS_PROBE").is_none() || screen_top == 0 {
        return;
    }
    if LAST_DONE_SCREEN.load(Ordering::Relaxed) == screen_top {
        return;
    }
    let now = now_ms();
    let previous = LAST_ATTEMPT_MS.load(Ordering::Relaxed);
    if now.saturating_sub(previous) < PROBE_INTERVAL_MS {
        return;
    }
    LAST_ATTEMPT_MS.store(now, Ordering::Relaxed);

    match read_play_mods_json(source, table, screen_top, layout, modules) {
        Some(read) => {
            LAST_DONE_SCREEN.store(screen_top, Ordering::Relaxed);
            crate::server::log::log_at(
                "info",
                &format!("[osu] lazer mods probe: {}", read.summary),
            );
        }
        None => {
            if LAST_REPORTED_SCREEN.swap(screen_top, Ordering::Relaxed) != screen_top {
                crate::server::log::log_at(
                    "info",
                    &format!(
                        "[osu] lazer mods probe: no ScoreInfo.ModsJson under screen=0x{screen_top:016X} (will retry)"
                    ),
                );
            }
        }
    }
}

struct ProbeRead {
    summary: String,
}

static PROBED_MODS: Mutex<Option<HashSet<u64>>> = Mutex::new(None);

pub fn probe_enabled() -> bool {
    std::env::var_os("MMA_LAZER_MODS_PROBE").is_some()
}

/// 逐个 mod 对象打印候选引用字段与三种解码（u8 / f32 / f64），用于确认
/// `SpeedChange`（double）与 `DifficultyBindable`（可空 float）的真实布局。
/// 每个对象只打印一次；只在 `MMA_LAZER_MODS_PROBE` 设置时工作。
pub fn probe_mod_object(
    source: &dyn Source,
    table: &OffsetTable,
    object: u64,
    type_name: &str,
    acronym: &str,
    modules: &[(u64, String)],
) {
    if !probe_enabled() {
        return;
    }
    {
        let mut guard = PROBED_MODS
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let set = guard.get_or_insert_with(HashSet::new);
        if !set.insert(object) {
            return;
        }
    }

    let mut fields = Vec::new();
    for offset in [0x10u64, 0x18, 0x20, 0x28, 0x30, 0x38, 0x40] {
        let Some(pointer) = read_ptr_field(source, object, offset as i64) else {
            continue;
        };
        let bindable_type = read_eetype(source, table, pointer, modules)
            .ok()
            .and_then(|info| info.type_name)
            .unwrap_or_else(|| "?".to_string());
        let u8_value = read_u8(source, pointer + 0x20)
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".to_string());
        let f32_value = read_f32(source, pointer + 0x24)
            .map(|value| format!("{value:?}"))
            .unwrap_or_else(|| "-".to_string());
        let f64_value = read_f64(source, pointer + 0x20)
            .map(|value| format!("{value:?}"))
            .unwrap_or_else(|| "-".to_string());
        fields.push(format!(
            "+0x{offset:X} -> 0x{pointer:016X} type={bindable_type} \
             u8@0x20={u8_value} f32@0x24={f32_value} f64@0x20={f64_value}"
        ));
    }
    crate::server::log::log_at(
        "info",
        &format!(
            "[osu] lazer mod probe: type={type_name} acronym={acronym} fields: {}",
            fields.join("; ")
        ),
    );
}

fn read_f32(source: &dyn Source, addr: u64) -> Option<f32> {
    let bytes = source.read(addr, 4)?;
    Some(f32::from_le_bytes(bytes.try_into().ok()?))
}

fn read_play_mods_json(
    source: &dyn Source,
    table: &OffsetTable,
    screen_top: u64,
    layout: Option<StringLayout>,
    modules: &[(u64, String)],
) -> Option<ProbeRead> {
    let layout = layout?;
    let (score_offset, score) = find_object_of_type(
        source,
        table,
        screen_top,
        SCAN_START,
        SCREEN_SCAN_END,
        "osu.Game.Scoring.Score",
        modules,
    )?;
    let (score_info_offset, score_info) = find_object_of_type(
        source,
        table,
        score,
        SCAN_START,
        SCORE_SCAN_END,
        "osu.Game.Scoring.ScoreInfo",
        modules,
    )?;
    let json = find_mods_json(source, layout, score_info, SCOREINFO_SCAN_END)?;
    let mods = lazer_mods_from_mods_json(&json)?;
    let summary = format!(
        "screen=0x{screen_top:016X} score_offset=0x{score_offset:X} score=0x{score:016X} \
         scoreinfo_offset=0x{score_info_offset:X} scoreinfo=0x{score_info:016X} mods=[{}] mods_json={}",
        summarize(&mods),
        truncate(&json, 600)
    );
    Some(ProbeRead { summary })
}

fn find_object_of_type(
    source: &dyn Source,
    table: &OffsetTable,
    base: u64,
    start: u64,
    end: u64,
    wanted: &str,
    modules: &[(u64, String)],
) -> Option<(u64, u64)> {
    for offset in (start..end).step_by(8) {
        let Some(candidate) = read_ptr_field(source, base, offset as i64) else {
            continue;
        };
        let Ok(info) = read_eetype(source, table, candidate, modules) else {
            continue;
        };
        if info.type_name.as_deref() == Some(wanted) {
            return Some((offset, candidate));
        }
    }
    None
}

fn find_mods_json(
    source: &dyn Source,
    layout: StringLayout,
    score_info: u64,
    end: u64,
) -> Option<String> {
    for offset in (SCAN_START..end).step_by(8) {
        let Some(pointer) = read_ptr_field(source, score_info, offset as i64) else {
            continue;
        };
        let Some(text) = read_string(source, pointer, layout) else {
            continue;
        };
        if !text.trim_start().starts_with('[') || !text.contains("\"acronym\"") {
            continue;
        }
        if lazer_mods_from_mods_json(&text).is_some() {
            return Some(text);
        }
    }
    None
}

fn summarize(mods: &[LazerMod]) -> String {
    mods.iter()
        .map(|mod_| match (mod_.speed_change, mod_.overall_difficulty) {
            (Some(rate), _) => format!("{}:{rate:.2}x", mod_.acronym),
            (_, Some(od)) => format!("{}:OD{od}", mod_.acronym),
            _ => mod_.acronym.clone(),
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    text.chars().take(max_chars).collect::<String>() + "…"
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mods_json_maps_custom_rate_and_od() {
        let mods = lazer_mods_from_mods_json(
            r#"[
                {"acronym":"DT","settings":{"speed_change":1.2}},
                {"acronym":"DA","settings":{"overall_difficulty":8.5}},
                {"acronym":"MR"}
            ]"#,
        )
        .expect("有效 ModsJson");

        assert_eq!(mods.len(), 3);
        assert_eq!(mods[0].acronym, "DT");
        assert_eq!(mods[0].speed_change, Some(1.2));
        assert_eq!(mods[0].overall_difficulty, None);
        assert_eq!(mods[1].acronym, "DA");
        assert_eq!(mods[1].speed_change, None);
        assert_eq!(mods[1].overall_difficulty, Some(8.5));
        assert_eq!(mods[2].acronym, "MR");
        assert_eq!(mods[2].speed_change, None);
        assert_eq!(mods[2].overall_difficulty, None);
    }

    #[test]
    fn default_mods_have_no_settings_and_od_zero_is_kept() {
        let mods = lazer_mods_from_mods_json(
            r#"[{"acronym":"DT"},{"acronym":"DA","settings":{"overall_difficulty":0}}]"#,
        )
        .expect("有效 ModsJson");
        assert_eq!(
            mods[0].speed_change, None,
            "默认 1.5x 不应产生 speed_change"
        );
        assert_eq!(mods[0].overall_difficulty, None);
        assert_eq!(mods[1].overall_difficulty, Some(0.0), "OD 0 是合法值");
    }

    #[test]
    fn invalid_settings_do_not_poison_the_rest() {
        let mods = lazer_mods_from_mods_json(
            r#"[
                {"acronym":"DT","settings":{"speed_change":0}},
                {"acronym":"HT","settings":{"speed_change":"0.75"}},
                {"acronym":"DA","settings":[]},
                {"acronym":"MR","settings":{"unknown":1}}
            ]"#,
        )
        .expect("结构仍合法");
        assert_eq!(mods[0].speed_change, None, "0 不是有效倍速");
        assert_eq!(mods[1].speed_change, None, "字符串不是数字");
        assert_eq!(mods[2].overall_difficulty, None, "settings 不是对象");
        assert_eq!(mods[3].acronym, "MR");
    }

    #[test]
    fn empty_array_is_no_mods() {
        assert_eq!(
            lazer_mods_from_mods_json("[]").unwrap(),
            Vec::<LazerMod>::new()
        );
    }

    #[test]
    fn malformed_payloads_are_rejected() {
        for text in [
            "",
            "not json",
            "{}",
            r#"[{"settings":{}}]"#,
            r#"[{"acronym":""}]"#,
            r#"[{"acronym":1}]"#,
            r#"[{"acronym":"DT"},]"#,
        ] {
            assert!(
                lazer_mods_from_mods_json(text).is_none(),
                "{text:?} 不该被当成 ModsJson"
            );
        }
    }
}
