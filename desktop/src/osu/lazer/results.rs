// results.rs —— 结算屏的判定数（`resultsScreen.hits`）
//
// 数据来源与形状（2026.1005.0.0 真机读到的）：
//   ScreenStack 栈顶（`SoloResultsScreen`）
//     +<table.results.score_offset>       → `osu.Game.Scoring.ScoreInfo`
//     +<table.results.player_offset>      → `osu.Game.Screens.Play.ReplayPlayer`（回放时）/ `SoloPlayer`（正常）
//   ScoreInfo
//     +<table.results.statistics_offset>  → `<StatisticsJson>`（`System.String`）
//     +<table.results.statistics_dict_offset> → `statistics`（`Dictionary<HitResult,int>`）
//
// 两个来源都要读，因为它们的**可用时机不同**：
//   · `<StatisticsJson>` 是入库时序列化的字符串（回放 / 历史成绩有值；刚打完的活成绩是空串）；
//   · `statistics` 字典是记分处理器逐个判定累加的活对象（活成绩只有它有值）。
// 先读 JSON（名字自描述、跨版本稳），读不到再枚举字典（整型键按表里的
// `hit_result_values` 映射成判定名）。
//
// 真实 JSON（回放一局 mania）：
//   {"none":0,"miss":23,"meh":1,"ok":9,"good":118,"great":633,"perfect":1399,
//    "small_tick_miss":0,...,"ignore_hit":34,"combo_break":0}
// 真实字典（刚打完的一局 mania）：17 个 entry，`great=15`、`perfect=793`、`ignore_hit=20`。
//
// 键是 lazer `HitResult`（枚举序号或 snake_case 名）。页面只消费 stable 六键
// （`geki/300/katu/100/50/0`，见 `livePpCounts.extractCounts`），所以这里做一次映射：
//   geki=perfect(320) · 300=great · katu=good(200) · 100=ok · 50=meh · 0=miss
// 与 osu!stable mania 的 API 计数口径一致（V1 里 320 与 300 同权，见 `feedbackMath.scoreV1`）。

use std::collections::BTreeMap;

use crate::osu::model::Hits;
use crate::osu::offsets::{OffsetTable, ResultsSection};
use super::screen::read_eetype;
use super::source::{read_i32, read_ptr_field, read_string, Source, StringLayout};

/// 降级标记用的字段名（与 `TABLE_BACKED_FIELDS` 里的键一致）。
pub const FIELD: &str = "resultsScreen.hits";

/// lazer `HitResult` 的六个 mania 判定键。其它键（tick/bonus/ignore/combo_break）不参与映射。
const JUDGEMENT_KEYS: &[&str] = &["perfect", "great", "good", "ok", "meh", "miss"];

/// .NET 数组布局（与表的 `runtime.screen_array` 见证的是同一个布局）：长度在 `+0x08`、元素在 `+0x10`。
const ARRAY_LENGTH_OFFSET: u64 = 0x08;
const ARRAY_ELEMENTS_OFFSET: u64 = 0x10;
/// `Dictionary<TKey,TValue>.Entry`（`HitResult` → `int`）= 4 个 int：`hashCode/next/key/value`。
///
/// 键值都是 4 字节，所以步长 16；`hashCode < 0` = 已释放的槽（枚举时跳过）。
const DICTIONARY_ENTRY_STRIDE: u64 = 16;
const DICTIONARY_ENTRY_KEY: u64 = 8;
const DICTIONARY_ENTRY_VALUE: u64 = 12;
/// 判定字典的 entry 上限（`HitResult` 只有十几个键；防御坏指针）。
const STATISTICS_ENTRY_MAX: i32 = 64;

/// 一次结算屏读取的结果。
#[derive(Clone, Debug, Default)]
pub struct ResultsRead {
    pub hits: Option<Hits>,
    /// 结算屏上的 `<player>` 是 `ReplayPlayer` ⇒ 这一局的成绩来自回放，不是真实游玩。
    pub replay: bool,
    pub gaps: Vec<String>,
}

impl ResultsRead {
    fn gap(&mut self, why: &str) {
        self.gaps.push(format!("{FIELD}:{why}"));
    }
}

/// 读一帧结算屏。`screen` = 屏幕栈顶对象地址。
///
/// 失败一律**不发布**（`hits = None` + 逐字 gap）：绝不用 0 或上一帧的值占位。
pub fn read_results(
    source: &dyn Source,
    table: &OffsetTable,
    screen: u64,
    layout: Option<StringLayout>,
    modules: &[(u64, String)],
) -> ResultsRead {
    let mut read = ResultsRead::default();
    let Some(results) = table.results() else {
        read.gap("offsets-missing-results-section");
        return read;
    };
    if screen == 0 {
        read.gap("screen-top-unreadable");
        return read;
    }

    // 回放判定先做：即使 hits 读不出来，"这是回放"也必须能判（否则回放成绩会被当成真实游玩）。
    if let Some(player_offset) = results.player_offset {
        if let Some(player) = read_ptr_field(source, screen, player_offset) {
            if let Ok(info) = read_eetype(source, table, player, modules) {
                read.replay = info
                    .type_name
                    .as_deref()
                    .map_or(false, |name| name.contains("ReplayPlayer"));
            }
        }
    }

    let Some(layout) = layout else {
        read.gap("string-layout");
        return read;
    };
    let Some(score) = read_ptr_field(source, screen, results.score_offset) else {
        read.gap("read-Score");
        return read;
    };

    // ① 入库过的成绩：`<StatisticsJson>`（回放 / 历史成绩）。
    let mut json_reason = "read-StatisticsJson";
    if let Some(statistics_offset) = results.statistics_offset {
        if let Some(pointer) = read_ptr_field(source, score, statistics_offset) {
            match read_string(source, pointer, layout) {
                Some(json) if !json.trim().is_empty() => match hits_from_statistics_json(&json) {
                    Some(hits) => {
                        read.hits = Some(hits);
                        return read;
                    }
                    None => json_reason = "parse-statistics-json",
                },
                Some(_) => json_reason = "statistics-json-empty",
                None => json_reason = "statistics-json-unreadable",
            }
        }
    } else {
        json_reason = "no-statistics-json-offset";
    }

    // ② 活成绩：`statistics` 字典（`<StatisticsJson>` 在入库前是空串）。
    match read_statistics_dict(source, score, results) {
        Some(hits) => read.hits = Some(hits),
        None => read.gap(&format!("{json_reason}+statistics-dict-unreadable")),
    }
    read
}

/// `StatisticsJson` → 载荷六键。
///
/// 判据（任一不过 ⇒ `None`，绝不 0 占位）：
/// 1. 是一个 JSON object；
/// 2. 至少带一个 `HitResult` 判定键（否则任意 `{...}` 字符串都会被当成判定数）；
/// 3. 六个键里至少有一个非零（全零 = 空成绩，不是有效结算）。
pub fn hits_from_statistics_json(text: &str) -> Option<Hits> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let object = value.as_object()?;
    if !JUDGEMENT_KEYS.iter().any(|key| object.contains_key(*key)) {
        return None;
    }
    let count = |key: &str| {
        object
            .get(key)
            .and_then(|value| value.as_u64())
            .unwrap_or(0)
            .min(u32::MAX as u64) as u32
    };
    let hits = Hits {
        geki: count("perfect"),
        n300: count("great"),
        katu: count("good"),
        n100: count("ok"),
        n50: count("meh"),
        miss: count("miss"),
    };
    if hits.is_zero() {
        return None;
    }
    Some(hits)
}

/// 活成绩的判定字典（`Dictionary<HitResult,int>`）。
///
/// 读法：`ScoreInfo.statistics_dict_offset` → 字典对象；`dict_entries_offset` → entry 数组；
/// 数组长度 @ `+0x08`、元素 @ `+0x10`、每个 entry 16 字节（`hashCode/next/key/value`）。
/// 跳过 `hashCode < 0` 的释放槽；key 按表里的 `hit_result_values` 映射成判定名。
fn read_statistics_dict(
    source: &dyn Source,
    score: u64,
    results: &ResultsSection,
) -> Option<Hits> {
    let dict_offset = results.statistics_dict_offset?;
    let entries_offset = results.dict_entries_offset?;
    if results.hit_result_values.is_empty() {
        return None;
    }
    let dict = read_ptr_field(source, score, dict_offset)?;
    let entries = read_ptr_field(source, dict, entries_offset)?;
    let length = read_i32(source, entries + ARRAY_LENGTH_OFFSET)?;
    if length <= 0 || length > STATISTICS_ENTRY_MAX {
        return None;
    }
    let mut counts: BTreeMap<u32, u32> = BTreeMap::new();
    for index in 0..length as u64 {
        let entry = entries + ARRAY_ELEMENTS_OFFSET + index * DICTIONARY_ENTRY_STRIDE;
        let hash = read_i32(source, entry)?;
        if hash < 0 {
            continue;
        }
        let key = read_i32(source, entry + DICTIONARY_ENTRY_KEY)?;
        let value = read_i32(source, entry + DICTIONARY_ENTRY_VALUE)?;
        if key < 0 || value < 0 {
            return None;
        }
        counts.insert(key as u32, value as u32);
    }
    if counts.is_empty() {
        return None;
    }
    let value_for = |name: &str| -> u32 {
        results
            .hit_result_values
            .get(name)
            .and_then(|key| counts.get(key))
            .copied()
            .unwrap_or(0)
    };
    let hits = Hits {
        geki: value_for("perfect"),
        n300: value_for("great"),
        katu: value_for("good"),
        n100: value_for("ok"),
        n50: value_for("meh"),
        miss: value_for("miss"),
    };
    if hits.is_zero() {
        return None;
    }
    Some(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statistics_json_maps_to_the_six_payload_keys() {
        let hits = hits_from_statistics_json(
            r#"{"none":0,"miss":23,"meh":1,"ok":9,"good":118,"great":633,"perfect":1399,
                "small_tick_miss":0,"small_tick_hit":0,"large_tick_miss":0,"large_tick_hit":0,
                "small_bonus":0,"large_bonus":0,"ignore_miss":0,"ignore_hit":34,"combo_break":0}"#,
        )
        .expect("有效统计 JSON");
        assert_eq!(hits.geki, 1399, "perfect(320) -> geki");
        assert_eq!(hits.n300, 633, "great -> 300");
        assert_eq!(hits.katu, 118, "good(200) -> katu");
        assert_eq!(hits.n100, 9, "ok -> 100");
        assert_eq!(hits.n50, 1, "meh -> 50");
        assert_eq!(hits.miss, 23);
    }

    #[test]
    fn non_statistics_payloads_are_rejected() {
        for text in [
            "",
            "not json",
            "[]",
            r#"{"acronym":"DT"}"#,
            r#"{"perfect":0,"great":0,"good":0,"ok":0,"meh":0,"miss":0}"#,
        ] {
            assert!(
                hits_from_statistics_json(text).is_none(),
                "{text:?} 不该被当成判定数"
            );
        }
    }

    #[test]
    fn unknown_keys_are_ignored_and_string_numbers_are_not_accepted() {
        let hits = hits_from_statistics_json(
            r#"{"great":12,"perfect":3,"combo_break":0,"future_key":7,"100":"9"}"#,
        )
        .expect("至少一个已知判定键");
        assert_eq!(hits.n300, 12);
        assert_eq!(hits.geki, 3);
        assert_eq!(hits.n100, 0, "字符串形式的数字不算计数");
    }

    // ---- 判定字典（活成绩） ----

    struct Mem {
        base: u64,
        bytes: Vec<u8>,
    }

    impl Source for Mem {
        fn read(&self, addr: u64, len: usize) -> Option<Vec<u8>> {
            let start = addr.checked_sub(self.base)? as usize;
            let end = start.checked_add(len)?;
            if end > self.bytes.len() {
                return None;
            }
            Some(self.bytes[start..end].to_vec())
        }
    }

    fn put_q(image: &mut [u8], base: u64, addr: u64, value: u64) {
        let at = (addr - base) as usize;
        image[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }

    fn put_i(image: &mut [u8], base: u64, addr: u64, value: i32) {
        let at = (addr - base) as usize;
        image[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn dict_results() -> ResultsSection {
        let mut hit_result_values = BTreeMap::new();
        for (name, value) in [
            ("miss", 1u32),
            ("meh", 2),
            ("ok", 3),
            ("good", 4),
            ("great", 5),
            ("perfect", 6),
        ] {
            hit_result_values.insert(name.to_string(), value);
        }
        ResultsSection {
            score_offset: 0x30,
            statistics_offset: Some(0x58),
            player_offset: None,
            statistics_dict_offset: Some(0x80),
            dict_entries_offset: Some(0x10),
            hit_result_values,
            witness: "test".to_string(),
        }
    }

    /// 合成一段内存：`ScoreInfo` → `statistics` 字典 → entry 数组（17 个 entry，与真机同形）。
    fn dict_memory() -> (Mem, u64, ResultsSection) {
        const BASE: u64 = 0x1000_0000;
        let mut image = vec![0u8; 0x4000];
        let score = BASE + 0x100;
        let dict = BASE + 0x1000;
        let entries = BASE + 0x2000;
        put_q(&mut image, BASE, score, BASE + 0x3000); // ScoreInfo 的 MT（这里不参与读取）
        put_q(&mut image, BASE, score + 0x80, dict);
        put_q(&mut image, BASE, dict + 0x10, entries);
        put_i(&mut image, BASE, entries + 8, 17);
        // 真机实测：key=5(great)=15、key=6(perfect)=793、key=14(ignore_hit)=20，其余 0。
        for index in 0..17u64 {
            let entry = entries + 0x10 + index * 16;
            put_i(&mut image, BASE, entry, index as i32); // hashCode（枚举 hash == key）
            put_i(&mut image, BASE, entry + 4, -1); // next
            put_i(&mut image, BASE, entry + 8, index as i32); // key
            let value = match index {
                5 => 15,
                6 => 793,
                14 => 20,
                _ => 0,
            };
            put_i(&mut image, BASE, entry + 12, value);
        }
        (Mem { base: BASE, bytes: image }, score, dict_results())
    }

    #[test]
    fn live_statistics_dictionary_maps_to_the_six_payload_keys() {
        let (memory, score, results) = dict_memory();
        let hits = read_statistics_dict(&memory, score, &results).expect("字典判定数");
        assert_eq!(hits.geki, 793, "perfect -> geki");
        assert_eq!(hits.n300, 15, "great -> 300");
        assert_eq!(hits.katu, 0);
        assert_eq!(hits.n100, 0);
        assert_eq!(hits.n50, 0);
        assert_eq!(hits.miss, 0);
    }

    #[test]
    fn dictionary_without_a_valid_entries_offset_is_rejected() {
        let (memory, score, results) = dict_memory();
        let mut broken = results;
        broken.dict_entries_offset = Some(0x28); // 指到别处：长度字段不成立
        assert!(read_statistics_dict(&memory, score, &broken).is_none());    }

    #[test]
    fn dictionary_without_the_key_mapping_is_rejected() {
        let (memory, score, results) = dict_memory();
        let mut broken = results;
        broken.hit_result_values.clear();
        assert!(read_statistics_dict(&memory, score, &broken).is_none());
    }
}
