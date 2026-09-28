// `.osu` 文件解析（**纯函数**）：契约项 **C6** 的两个值 + 头字段交叉校验。
//
// 为什么在这里解析（计划 §3.3 字段表）：
// - `beatmap.time.live` 来自内存（playTime 链，C2）；
// - `beatmap.time.firstObject` / `lastObject` **不是内存字段**——它们是"谱面全长的首/末时间"，
//   参照实现由它自己的谱面处理器从 `.osu` 算出（P1-notes F2 同族结论；B1-notes §8-1 把 C6
//   留到本步用"自己解析 vs tosu 输出"逐条对照关闭）。故本模块是这两个值的**唯一来源**。
//
// 口径（冻结，实现不得再选）：
// - `firstObject` = `round(首个 hit object 的 startTime)`，**文件顺序**（不是最小时间）；
//   同时报出 `min_start_time`，两者不等即说明文件不是时间升序 ⇒ 影子比对时一眼可见。
// - `lastObject` = `round(文件顺序**最后一个**对象的 endTime)`，**不是** `max(endTime)`。逐类型：
//   - circle（`type & 0b1`）      → `startTime`（endTime == startTime）
//   - mania hold（`type & 0b1000_0000`）→ 第 6 段 `endTime`
//   - spinner（`type & 0b1000`） → 第 6 段 `endTime`
//   - slider（`type & 0b10`）    → `startTime + duration`，`duration = pixelLength / velocity`，
//     `velocity = 100 * SliderMultiplier * legacyMultiplier * sv / beatLength`（见下）
//   **为什么是文件顺序的末对象**（2026-09-28 由操作员在 6 张真实谱面上逐张核对，100% 一致）：
//   页面消费的 `beatmap.time.lastObject` 口径 = **末对象的结束时间**；而 `max(endTime)`
//   在"前面某条长条比末对象结束得更晚"的图上会**超过**该值（典型是 LN 谱：一条早开始的长条
//   尾端压在最后几个对象之后）。6 张反例见下方台账，差值 39…2285 ms。
//   ⇒ 旧口径（`max`）已废弃；`max_end_time` 作为**诊断字段**保留该值，让两者的差在 JSONL 里可见。
//
// | 谱面（checksum 前 10 位） | 旧口径 `max(end)` | 新口径（文件末对象） | 差 |
// |---|---|---|---|
// | `413c9fb4e6` | 222223 | **222184** | 39 |
// | `63fc7278e5` | 331104 | **330014** | 1090 |
// | `b830429863` | 447591 | **445900** | 1691 |
// | `c2980bd6dc` | 745985 | **743166** | 2819 |
// | `1328a34083` | 148862 | **148777** | 85 |
// | `8362f42b4b` | 144322 | **144099** | 223 |
// 回归由 `tests-local/osu_beatmap_corpus.rs::real_charts_last_object_is_the_file_order_last_object`
// 守着（文件缺失时跳过）。
// - `[General] SliderMultiplier`：缺省 **1.4**（osu! 的默认值）。
// - `sv`（绿线段）：`-100 / beatLength`，**夹取到 `[0.1, 10]`**（osu! 对 slider velocity 的
//   硬夹取）；红线段 `beatLength` 由"slider 时刻之前最近的红线"给出。
// - 路径长度：Linear（折线）/ Bezier（任意控制点数，采样积分）/ PerfectCircle（三点定圆解析弧长）/
//   Catmull（标准 Catmull-Rom，有限差分切线）。Bezier 用 1024 段采样（弦长和，误差 O(1/N²)，
//   对 ms 级时长无影响），Catmull 用 256 段采样。
//
// 覆盖率三态（`SliderCoverage`）：没有 slider ⇒ `None`（**exact**）；全部 slider 都算出来了
// ⇒ `Full`；出现退化 slider（缺红线段、beatLength 非正、路径长度非有限）⇒ `Partial` 且
// 该 slider 的 endTime 退化为 startTime（**保守**：只会低估 `lastObject`）。
//
// ⚠️ 真机语料实测（2026-09-28，本机 `D:\Games\osu!\Songs`，135 个目录 / 1424 个 `.osu` /
// 65831 个 slider）：**文件第 8 段的 `length` 不总是几何路径长度**——有 63 个目录（47%）
// 存在"长度锁定"（同目录里多个几何不同的 slider 共享同一 `length`，例如从 (9,21) 出发的
// `L|59:29` 声明 27.3 而几何长度 50.636）。osu! 官方 issue `ppy/osu#24798` 记录了同一现象
// （"the length specified in the .osu file is different than the length calculated using the
// path points"，举例：文件 330 / 几何 222.004），且**游戏用几何长度算时长**。
// ⇒ 本实现的 `duration` 一律用**几何长度**（`path_pixel_length`），`length` 只在几何长度
// 为 0（旧格式/退化）时兜底；语料里"几何长度 vs 声明长度"一致率 90.3%（排除锁定目录后），
// 中位比值 1.0004 ≈ 1。见 `tests-local/osu_beatmap_corpus.rs` 的两个语料用例。
//
// 头字段（`Title/Artist/Creator/Version/BeatmapID/BeatmapSetID`）用于与内存值交叉校验：
// 这是 C6 的"解析器本身没读错文件"的自证（P2/P5 已单独证明 `checksum == 磁盘 MD5`）。

/// 解析结果的覆盖率（见文件头说明）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SliderCoverage {
    /// 谱面里没有 slider（mania 原生谱的常态）⇒ 两个值都是精确值。
    None,
    /// 所有 slider 的时长都算出来了。
    Full,
    /// 至少一个 slider 时长算不出来（退化处理，可能低估 `lastObject`）。
    Partial,
}

impl SliderCoverage {
    pub fn as_str(&self) -> &'static str {
        match self {
            SliderCoverage::None => "none",
            SliderCoverage::Full => "full",
            SliderCoverage::Partial => "partial",
        }
    }
}

/// 一个时间点（`[TimingPoints]` 的一行；只保留算时长需要的两种语义）。
#[derive(Clone, Copy, Debug)]
pub struct TimingPoint {
    /// `time`（ms）。
    pub time: f64,
    /// 红线段 = 每拍的毫秒数（> 0）；绿线段 = 负的"SV 倍数编码"（`-100 / sv`）。
    pub beat_length: f64,
    /// 第 8 段：`1` = 红线段（uninherited），`0`/缺省 = 绿线段（inherited）。
    pub uninherited: bool,
}

/// 头字段（与内存值交叉校验用；空串 = 文件里没有该键）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Header {
    pub title: String,
    pub artist: String,
    pub creator: String,
    pub version: String,
    /// `[Editor] BeatmapID`。旧图可能整段缺失 ⇒ `None`。
    pub beatmap_id: Option<i32>,
    /// `[Editor] BeatmapSetID`。旧图可能整段缺失 ⇒ `None`。
    pub beatmap_set_id: Option<i32>,
}

/// 首/末对象的结果（C6 的两个值）。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FirstLast {
    /// `round(文件顺序首个对象的 startTime)`。
    pub first_object: i32,
    /// `round(文件顺序**末个**对象的 endTime)`；没有对象 ⇒ `None`（**不是** `max_end_time`）。
    pub last_object: Option<i32>,
    /// **诊断字段**：`round(max(各对象 endTime))`——旧口径，只用于观察"长条越过末对象"的差值。
    pub max_end_time: Option<i32>,
    /// 所有对象的 `min(startTime)`（与 `first_object` 不等即"文件非升序"）。
    pub min_start_time: i32,
    /// 解析出的 hit object 行数。
    pub object_count: usize,
    /// `circle` / `slider` / `spinner` / `hold` 各自的数量（逐对象计账）。
    pub circles: usize,
    pub sliders: usize,
    pub spinners: usize,
    pub holds: usize,
    /// slider 路径长度的**累计和**（诊断用；没有 slider = 0）。
    pub slider_pixel_total: f64,
    /// 文件里**声明**的 `pixelLength` 累计和（同上，用于"算出来的长度 vs 文件声明"交叉校验）。
    pub slider_declared_total: f64,
    /// 时长算不出来的 slider 数（> 0 ⇒ `SliderCoverage::Partial`）。
    pub sliders_uncomputed: usize,
}

/// 解析结果。
#[derive(Clone, Debug, Default)]
pub struct BeatmapFile {
    pub header: Header,
    /// 实际使用的 `SliderMultiplier`（缺省 1.4）。
    pub slider_multiplier: f64,
    /// 是否在文件里显式读到 `SliderMultiplier`（否则为默认值）。
    pub slider_multiplier_explicit: bool,
    pub timing_points: Vec<TimingPoint>,
    pub first_last: FirstLast,
    pub slider_coverage: Option<SliderCoverage>,
}

impl BeatmapFile {
    pub fn first_object(&self) -> i32 {
        self.first_last.first_object
    }

    pub fn last_object(&self) -> Option<i32> {
        self.first_last.last_object
    }

    /// 诊断值 `max(endTime)`（口径见 `FirstLast::max_end_time`；不进发布载荷）。
    pub fn max_end_time(&self) -> Option<i32> {
        self.first_last.max_end_time
    }

    /// 头字段与内存值逐字段比较，返回**不一致的字段名**（空 = 全等）。
    ///
    /// 口径：文件里缺该键（旧图的 `[Editor]`）不算不一致；大小写与首尾空白不做归一
    /// （`.osu` 与内存是同一份文本，任何差别都是真实信号）。
    pub fn header_mismatches(
        &self,
        title: Option<&str>,
        artist: Option<&str>,
        creator: Option<&str>,
        version: Option<&str>,
        map_id: Option<i32>,
        set_id: Option<i32>,
    ) -> Vec<&'static str> {
        let mut out = Vec::new();
        let mut check = |field: &'static str, file: &str, memory: Option<&str>| {
            if let Some(memory) = memory {
                if file != memory {
                    out.push(field);
                }
            }
        };
        check("title", &self.header.title, title);
        check("artist", &self.header.artist, artist);
        check("creator", &self.header.creator, creator);
        check("version", &self.header.version, version);
        if let (Some(file), Some(memory)) = (self.header.beatmap_id, map_id) {
            if file != memory {
                out.push("beatmap_id");
            }
        }
        if let (Some(file), Some(memory)) = (self.header.beatmap_set_id, set_id) {
            if file != memory {
                out.push("beatmap_set_id");
            }
        }
        out
    }
}

/// 一条 slider 的路径信息（诊断/语料自证用：把"算出来的长度"与"文件声明的长度"按
/// **曲线类型**分组比对，才能定位是哪一种曲线数学不成立）。
#[derive(Clone, Debug)]
pub struct SliderPathInfo {
    /// 曲线类型字母（`L`/`B`/`P`/`C`）。
    pub kind: char,
    pub computed: f64,
    pub declared: f64,
    /// 路径串（出问题时可直接复算）。
    pub path: String,
    /// 对象坐标与原始行（诊断用）。
    pub start: (f64, f64),
    pub line: String,
}

/// 逐 slider 的路径信息（`parse` 的产物里不保留原始对象，故单独走一遍）。
pub fn slider_paths(text: &str) -> Vec<SliderPathInfo> {
    let mut out = Vec::new();
    let mut section = String::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = line.to_string();
            continue;
        }
        if section != "[HitObjects]" {
            continue;
        }
        let Some(object) = parse_object(line) else {
            continue;
        };
        let Some(slider) = object.slider.as_ref() else {
            continue;
        };
        out.push(SliderPathInfo {
            kind: slider.path.chars().next().unwrap_or('L'),
            computed: path_pixel_length(&slider.path, object.x, object.y),
            declared: slider.pixel_length,
            path: slider.path.clone(),
            start: (object.x, object.y),
            line: object.line.clone(),
        });
    }
    out
}

/// 解析文本（UTF-8；容忍 BOM 与 CRLF）。**永不 panic、永不返回 Err**：坏行跳过并在
/// `object_count` 等处体现（解析器只服务对拍证据，不参与门控）。
pub fn parse(text: &str) -> BeatmapFile {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut out = BeatmapFile {
        slider_multiplier: DEFAULT_SLIDER_MULTIPLIER,
        ..Default::default()
    };
    let mut section = String::new();
    let mut raw_objects: Vec<RawObject> = Vec::new();
    let mut slider_multiplier_seen: Option<f64> = None;

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = line.to_string();
            continue;
        }
        match section.as_str() {
            "[General]" => {
                if let Some((key, value)) = key_value(line) {
                    if key == "SliderMultiplier" {
                        if let Ok(parsed) = value.parse::<f64>() {
                            if parsed.is_finite() && parsed > 0.0 {
                                slider_multiplier_seen = Some(parsed);
                            }
                        }
                    }
                }
            }
            "[Metadata]" => {
                if let Some((key, value)) = key_value(line) {
                    match key {
                        "Title" => out.header.title = value.to_string(),
                        "Artist" => out.header.artist = value.to_string(),
                        "Creator" => out.header.creator = value.to_string(),
                        "Version" => out.header.version = value.to_string(),
                        _ => {}
                    }
                }
            }
            "[Editor]" => {
                if let Some((key, value)) = key_value(line) {
                    match key {
                        "BeatmapID" => out.header.beatmap_id = value.parse::<i32>().ok(),
                        "BeatmapSetID" => out.header.beatmap_set_id = value.parse::<i32>().ok(),
                        _ => {}
                    }
                }
            }
            "[TimingPoints]" => {
                let fields: Vec<&str> = line.split(',').collect();
                if fields.len() < 2 {
                    continue;
                }
                let (Ok(time), Ok(beat_length)) = (
                    fields[0].trim().parse::<f64>(),
                    fields[1].trim().parse::<f64>(),
                ) else {
                    continue;
                };
                // 旧格式只有两段 ⇒ 第 8 段缺省为"红线段"（`1`）。
                let uninherited = fields.get(6).map(|v| v.trim() != "0").unwrap_or(true);
                out.timing_points.push(TimingPoint {
                    time,
                    beat_length,
                    uninherited,
                });
            }
            "[HitObjects]" => {
                if let Some(object) = parse_object(line) {
                    raw_objects.push(object);
                }
            }
            _ => {}
        }
    }

    out.slider_multiplier_explicit = slider_multiplier_seen.is_some();
    if let Some(value) = slider_multiplier_seen {
        out.slider_multiplier = value;
    }
    out.first_last = compute_first_last(&raw_objects, &out.timing_points, out.slider_multiplier);
    out.slider_coverage = Some(if out.first_last.sliders == 0 {
        SliderCoverage::None
    } else if out.first_last.sliders_uncomputed == 0 {
        SliderCoverage::Full
    } else {
        SliderCoverage::Partial
    });
    out
}

/// `SliderMultiplier` 的 osu! 默认值（`[General]` 缺该键时）。
pub const DEFAULT_SLIDER_MULTIPLIER: f64 = 1.4;

/// 标准难度倍率（`Difficulty.SliderMultiplier` = `SliderMultiplier * 1.4`）。
pub const LEGACY_SLIDER_MULTIPLIER: f64 = 1.4;

/// osu! 对 slider velocity 倍率的硬夹取区间。
const SV_MIN: f64 = 0.1;
const SV_MAX: f64 = 10.0;

fn key_value(line: &str) -> Option<(&str, &str)> {
    let (key, value) = line.split_once(':')?;
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some((key.trim(), value))
    }
}

/// 一行 `[HitObjects]`（只留算时长需要的部分）。
#[derive(Clone, Debug)]
struct RawObject {
    kind: ObjectKind,
    time: f64,
    /// hit object 第 1/2 段（x/y）：osu! 把**对象坐标**当作路径的第 1 个控制点。
    x: f64,
    y: f64,
    /// 原始行（诊断用）。
    line: String,
    /// 第 6 段（spinner/hold 的 endTime；slider 的旧格式尾巴）。
    tail: Option<f64>,
    /// slider 的 `pixelLength` / `path`（第 6/8 段）。
    slider: Option<RawSlider>,
}

#[derive(Clone, Debug)]
struct RawSlider {
    pixel_length: f64,
    /// 第 8 段的路径串（`X|L|...`；也可能缺省）。
    path: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ObjectKind {
    Circle,
    Slider,
    Spinner,
    Hold,
}

fn parse_object(line: &str) -> Option<RawObject> {
    let fields: Vec<&str> = line.split(',').collect();
    if fields.len() < 4 {
        return None;
    }
    let type_byte: i32 = fields[3].trim().parse().ok()?;
    let time: f64 = fields[2].trim().parse().ok()?;
    let x: f64 = fields[0].trim().parse().ok()?;
    let y: f64 = fields[1].trim().parse().ok()?;
    // 位序照 osu!：circle > slider > spinner > mania hold（maniahitobject 带 128 位）。
    let kind = if type_byte & 0b0000_0001 != 0 {
        ObjectKind::Circle
    } else if type_byte & 0b0000_0010 != 0 {
        ObjectKind::Slider
    } else if type_byte & 0b0000_1000 != 0 {
        ObjectKind::Spinner
    } else if type_byte & 0b1000_0000 != 0 {
        ObjectKind::Hold
    } else {
        return None;
    };
    // 第 6 段（hitSound/endTime）的语义**按类型分派**（本机 `D:\Games\osu!\Songs` 真实样本
    // 与计划 §3.3 的 `+p[5]` 口径）：
    // - mania 长条（maniahitobject）：`endTime:…`（`64,192,2000,128,0,12000:0:0:0:0:`）
    //   ⇒ 数值在**首段**；
    // - spinner：简写形态 `256,192,3000,12,0,9000`（endTime 就在第 6 段）与展开形态
    //   `…,12,0,9000:0:0:0:0:`（数值仍在**首段**）都成立。
    // 早期版本"统一取首段而未排除自己的 hitSound 段"才读成 0——现在**只在 spinner/hold 上
    // 取 endTime**，circle/slider 不解析第 6 段（它只是 hitSound）。
    let tail = match kind {
        ObjectKind::Hold | ObjectKind::Spinner => fields.get(5).and_then(|v| {
            v.split(':')
                .find(|part| !part.trim().is_empty())
                .and_then(|part| part.trim().parse::<f64>().ok())
        }),
        _ => None,
    };
    let slider = if kind == ObjectKind::Slider {
        // 真实 v14 布局（本机 `D:\Games\osu!\Songs` 39445 行滑块样本核对）：
        // `x,y,time,type,hitSound,curve,slides,length,edgeSounds,edgeSets,hitSample`
        // ⇒ 第 6 段 = 曲线串（`P|256:40|175:157`）、第 8 段 = pixelLength。
        Some(RawSlider {
            pixel_length: fields
                .get(7)
                .and_then(|v| v.split(':').next())
                .and_then(|v| v.trim().parse::<f64>().ok())
                .unwrap_or(0.0),
            path: fields
                .get(5)
                .map(|v| v.trim().to_string())
                .unwrap_or_default(),
        })
    } else {
        None
    };
    Some(RawObject {
        kind,
        time,
        x,
        y,
        line: line.to_string(),
        tail,
        slider,
    })
}

/// 逐对象算 endTime，再汇总成 C6 的两个值。
///
/// - `last_object` = **文件顺序末对象**的 endTime（口径见文件头；不是 `max`）；
/// - `max_end_time` = `max(endTime)`（诊断字段，反映"最长长条压过末对象"的差）。
fn compute_first_last(
    objects: &[RawObject],
    timing_points: &[TimingPoint],
    slider_multiplier: f64,
) -> FirstLast {
    let mut out = FirstLast::default();
    if objects.is_empty() {
        return out;
    }
    out.object_count = objects.len();
    // 文件顺序的首个对象（不是最小时间）。
    out.first_object = round_ms(objects[0].time);
    out.min_start_time = out.first_object;
    let mut max_end = f64::NEG_INFINITY;
    // 文件顺序末对象（不是最晚时间）的 endTime。
    let mut last_end: Option<f64> = None;

    for object in objects {
        out.min_start_time = out.min_start_time.min(round_ms(object.time));
        let end = match object.kind {
            ObjectKind::Circle => {
                out.circles += 1;
                object.time
            }
            ObjectKind::Spinner => {
                out.spinners += 1;
                object.tail.unwrap_or(object.time)
            }
            ObjectKind::Hold => {
                out.holds += 1;
                object.tail.unwrap_or(object.time)
            }
            ObjectKind::Slider => {
                out.sliders += 1;
                let (duration, pixels) = slider_duration(object, timing_points, slider_multiplier);
                if let Some(slider) = object.slider.as_ref() {
                    out.slider_declared_total += slider.pixel_length.max(0.0);
                }
                match pixels {
                    Some(pixels) => out.slider_pixel_total += pixels,
                    None => out.sliders_uncomputed += 1,
                }
                object.time + duration
            }
        };
        if end.is_finite() && end > max_end {
            max_end = end;
        }
        // 每轮覆盖 ⇒ 循环结束时留下的就是文件顺序末对象的值——连"坏值"也照样覆盖（`round_ms`
        // 把非有限值给 0），不因为"这个值看起来更合理"而换成别的对象（那正是旧口径的错）。
        last_end = Some(end);
    }
    if max_end.is_finite() {
        out.max_end_time = Some(round_ms(max_end));
    }
    out.last_object = last_end.map(round_ms);
    out
}

/// `duration = pixelLength / velocity`；返回 `(duration_ms, 用到的像素长度)`——
/// 后者为 `None` = 时长算不出来（调用方计 `sliders_uncomputed`）。
fn slider_duration(
    object: &RawObject,
    timing_points: &[TimingPoint],
    slider_multiplier: f64,
) -> (f64, Option<f64>) {
    let Some(slider) = object.slider.as_ref() else {
        return (0.0, None);
    };
    let Some(beat_length) = beat_length_at(timing_points, object.time) else {
        return (0.0, None);
    };
    if !(beat_length > 0.0) || !beat_length.is_finite() {
        return (0.0, None);
    }
    let sv = sv_at(timing_points, object.time);
    let velocity = 100.0 * slider_multiplier * LEGACY_SLIDER_MULTIPLIER * sv / beat_length;
    if !(velocity > 0.0) || !velocity.is_finite() {
        return (0.0, None);
    }
    // 长度优先由路径串算出（osu! 的做法）；路径串缺失/为空时退回第 8 段的 `pixelLength`。
    let from_path = path_pixel_length(&slider.path, object.x, object.y);
    let pixels = if from_path > 0.0 {
        from_path
    } else {
        slider.pixel_length.max(0.0)
    };
    let duration = pixels / velocity;
    if !duration.is_finite() || duration < 0.0 {
        return (0.0, None);
    }
    (duration, Some(pixels))
}

/// slider 时刻之前**最近的红线段**的 `beatLength`（没有 ⇒ `None`）。
fn beat_length_at(timing_points: &[TimingPoint], time: f64) -> Option<f64> {
    let mut best: Option<&TimingPoint> = None;
    for point in timing_points {
        if !point.uninherited || !point.time.is_finite() || !(point.beat_length > 0.0) {
            continue;
        }
        if point.time <= time && best.map(|b| point.time >= b.time).unwrap_or(true) {
            best = Some(point);
        }
    }
    best.map(|p| p.beat_length)
}

/// `sv`：slider 时刻之前最近的**绿线段**的 `-100 / beatLength`，夹取到 `[0.1, 10]`；
/// 没有绿线段（或绿线不合法）⇒ `1.0`。
fn sv_at(timing_points: &[TimingPoint], time: f64) -> f64 {
    let mut best: Option<&TimingPoint> = None;
    for point in timing_points {
        if point.uninherited || !point.time.is_finite() {
            continue;
        }
        if point.time <= time && best.map(|b| point.time >= b.time).unwrap_or(true) {
            best = Some(point);
        }
    }
    match best {
        Some(point) if point.beat_length < 0.0 => {
            (-100.0 / point.beat_length).clamp(SV_MIN, SV_MAX)
        }
        _ => 1.0,
    }
}

fn round_ms(value: f64) -> i32 {
    if !value.is_finite() {
        return 0;
    }
    value.round() as i32
}

// ---- 路径长度 ----

#[derive(Clone, Copy, Debug)]
struct Point {
    x: f64,
    y: f64,
}

impl Point {
    fn distance(&self, other: &Point) -> f64 {
        ((self.x - other.x).powi(2) + (self.y - other.y).powi(2)).sqrt()
    }
}

/// `path` 串（`B|x:y|x:y|...`；第 1 段是曲线类型字母）→ 总长。
///
/// **对象坐标是路径的第 1 个控制点**（osu! `ConvertHitObjectParser`：先 `controlPoints.Add(
/// startPosition)`，再逐个加曲线串里的坐标；与首点重复时不重复添加）。这一条早期版本漏了，
/// 导致"L|140:0"这种单点路径长度为 0（被单测抓住）。
fn path_pixel_length(path: &str, start_x: f64, start_y: f64) -> f64 {
    let mut parts = path.split('|');
    let Some(kind_text) = parts.next() else {
        return 0.0;
    };
    let kind = kind_text.chars().next().unwrap_or('L');
    let tokens: Vec<&str> = parts.collect();
    if tokens.is_empty() {
        return 0.0;
    }
    let mut groups: Vec<Vec<Point>> = Vec::new();
    let mut current: Vec<Point> = Vec::new();
    let first_token = tokens.first().and_then(|token| parse_point(token));
    let start = Point {
        x: start_x,
        y: start_y,
    };
    // 与路径串首点**不相同**时才把对象坐标计入（相同则它是同一段的首点，计两次会多算一个 0 长度段）。
    let duplicate_start = first_token
        .map(|point| point.x == start.x && point.y == start.y)
        .unwrap_or(false);
    if !duplicate_start {
        current.push(start);
    }
    for index in 0..tokens.len() {
        let Some(point) = parse_point(tokens[index]) else {
            continue;
        };
        // osu! 的路径解析（`ConvertHitObjectParser`）：相邻两个坐标**完全相同时**
        // （`|x:y|x:y`）前者是上一段的最后一个控制点、后者是下一段的第一个控制点；
        // 同一曲线段内的重复坐标（`x:y|x+1:y|x:y`）不触发分段。
        if index + 1 < tokens.len() && tokens[index] == tokens[index + 1] {
            current.push(point);
            groups.push(std::mem::take(&mut current));
            continue;
        }
        current.push(point);
    }
    if !current.is_empty() {
        groups.push(current);
    }
    groups
        .iter()
        .filter(|group| !group.is_empty())
        .map(|group| segment_length(kind, group))
        .sum::<f64>()
}

fn parse_point(token: &str) -> Option<Point> {
    let mut coordinates = token.split(':');
    let x = coordinates.next()?.trim().parse::<f64>().ok()?;
    let y = coordinates.next()?.trim().parse::<f64>().ok()?;
    if x.is_finite() && y.is_finite() {
        Some(Point { x, y })
    } else {
        None
    }
}

/// 一段曲线的长度。
fn segment_length(kind: char, points: &[Point]) -> f64 {
    if points.is_empty() {
        return 0.0;
    }
    if points.is_empty() {
        return 0.0;
    }
    match kind {
        'L' | 'l' => {
            if points.len() < 2 {
                0.0
            } else {
                polyline_length(points)
            }
        }
        'P' | 'p' => perfect_circle_length(points).unwrap_or_else(|| polyline_length(points)),
        'C' | 'c' => catmull_length(points),
        // 'B'、缺失、未知一律按 Bezier（osu! 的默认曲线类型）。
        _ => bezier_length(points),
    }
}

fn polyline_length(points: &[Point]) -> f64 {
    points
        .windows(2)
        .map(|pair| pair[0].distance(&pair[1]))
        .sum()
}

/// Bezier：按段升阶求值 + 1024 段采样积分（弦长和）。任意控制点数都成立
/// （osu! 的 4+ 控制点是高次 Bezier，不是分段三次）。
fn bezier_length(points: &[Point]) -> f64 {
    let n = points.len();
    if n < 2 {
        return 0.0;
    }
    const SAMPLES: usize = 1024;
    let control = points.to_vec();
    let mut previous = control[0];
    let mut total = 0.0;
    for step in 1..=SAMPLES {
        let t = step as f64 / SAMPLES as f64;
        // de Casteljau：每层把点数减 1（`level` 层有 `n - level` 个点），最后一层就是曲线上的点。
        let mut current = control.clone();
        for level in 1..=n - 1 {
            for index in 0..n - level {
                current[index] = Point {
                    x: current[index].x * (1.0 - t) + current[index + 1].x * t,
                    y: current[index].y * (1.0 - t) + current[index + 1].y * t,
                };
            }
        }
        let point = current[0];
        total += previous.distance(&point);
        previous = point;
    }
    total
}

/// PerfectCircle：三点定圆 → 解析弧长；三点共线/退化 ⇒ `None`（调用方退化为折线）。
///
/// 注意 osu! 的坐标系 y 向下（屏幕坐标），但"圆 + 圆心角"在任意仿射一致的坐标系下
/// 长度不变（边长的欧氏度量就是像素度量），所以无需翻转 y。
fn perfect_circle_length(points: &[Point]) -> Option<f64> {
    if points.len() < 3 {
        return None;
    }
    let (a, b, c) = (points[0], points[1], points[2]);
    let d = 2.0 * (a.x * (b.y - c.y) + b.x * (c.y - a.y) + c.x * (a.y - b.y));
    if d.abs() < 1e-9 {
        return None;
    }
    let sum_a = a.x * a.x + a.y * a.y;
    let sum_b = b.x * b.x + b.y * b.y;
    let sum_c = c.x * c.x + c.y * c.y;
    let center = Point {
        x: (sum_a * (b.y - c.y) + sum_b * (c.y - a.y) + sum_c * (a.y - b.y)) / d,
        y: (sum_a * (c.x - b.x) + sum_b * (a.x - c.x) + sum_c * (b.x - a.x)) / d,
    };
    let radius = center.distance(&a);
    if !radius.is_finite() || radius <= 0.0 {
        return None;
    }
    let angle = |p: &Point| (p.y - center.y).atan2(p.x - center.x);
    let normalize = |mut value: f64| {
        while value < 0.0 {
            value += std::f64::consts::TAU;
        }
        while value >= std::f64::consts::TAU {
            value -= std::f64::consts::TAU;
        }
        value
    };
    let theta_a = normalize(angle(&a));
    let theta_b = normalize(angle(&b));
    let theta_c = normalize(angle(&c));
    // 两个候选弧：从 A 逆时针到 C，或顺时针到 C；取**经过 B** 的那个。
    let ccw = normalize(theta_c - theta_a);
    let contains = |span: f64| {
        let offset = normalize(theta_b - theta_a);
        offset <= span
    };
    let delta = if contains(ccw) {
        ccw
    } else {
        -(std::f64::consts::TAU - ccw)
    };
    Some(radius * delta.abs())
}

/// Catmull（标准 Catmull-Rom，端点用有限差分切线），256 段/段采样。
fn catmull_length(points: &[Point]) -> f64 {
    let n = points.len();
    if n < 2 {
        return 0.0;
    }
    const SAMPLES: usize = 256;
    let mut total = 0.0;
    let mut previous = points[0];
    for index in 0..n - 1 {
        let p0 = if index == 0 {
            points[0]
        } else {
            points[index - 1]
        };
        let p1 = points[index];
        let p2 = points[index + 1];
        let p3 = if index + 2 < n {
            points[index + 2]
        } else {
            points[n - 1]
        };
        // osu! 的切线（`CatmullApproximator`）：内部点用 (p2-p0)/2，端点的第 1 段用 p2-p1、
        // 末段用 p2-p1（与参照实现一致的有限差分）。
        let t1 = Point {
            x: (p2.x - p0.x) / 2.0,
            y: (p2.y - p0.y) / 2.0,
        };
        let t2 = Point {
            x: (p3.x - p1.x) / 2.0,
            y: (p3.y - p1.y) / 2.0,
        };
        for step in 1..=SAMPLES {
            let t = step as f64 / SAMPLES as f64;
            let point = catmull_point(p1, p2, t1, t2, t);
            total += previous.distance(&point);
            previous = point;
        }
    }
    total
}

fn catmull_point(p1: Point, p2: Point, t1: Point, t2: Point, t: f64) -> Point {
    let t2v = t * t;
    let t3 = t2v * t;
    let h1 = 2.0 * t3 - 3.0 * t2v + 1.0;
    let h2 = -2.0 * t3 + 3.0 * t2v;
    let h3 = t3 - 2.0 * t2v + t;
    let h4 = t3 - t2v;
    Point {
        x: h1 * p1.x + h2 * p2.x + h3 * t1.x + h4 * t2.x,
        y: h1 * p1.y + h2 * p2.y + h3 * t1.y + h4 * t2.y,
    }
}

#[cfg(test)]
#[path = "../../tests-local/osu_beatmap_file.rs"]
mod tests_beatmap_file;
