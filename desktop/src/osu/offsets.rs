// lazer 偏移表：**只放数据 + 加载**，不放任何校验逻辑（计划 Step 7 / Step 10 的显式分工）。
//
// 计划里这条分工写了两次（`.omo/plans/osu-native-memory-transport.md` §4.0 模块划分与
// Step 7 的 Action），理由是**校验必须留在读的那一侧**：偏移值本身没有"对不对"的属性，
// 只有"在目标进程 + 目标 runtime 上解引用后结构是否说得通"才能证伪它（L1 结构证明）。
// 所以本文件提供的是**钩子**，不是校验实现：
//
// - `OffsetTable::load(&[u8])`：纯字节 → 表（**不做任何文件 IO**，路径由调用方决定）
// - `OffsetTable::offset()`：查一个 `Type.Field`
// - `OffsetTable::nearest_table()`：就近版本回落——**必须**由调用方给一个校验谓词，且
//   默认谓词**恒拒**（`DefaultPolicy`）。不传谓词就拿不到表，也就没有"静默用错版本"的路径。
//
// ## 来源规则（provenance，硬约束）
//
// 表里的每一个数值**必须来自我们自己的 SOS/dump 提取**（`DECISIONS.md` OPEN-03 的
// 结论：lazer 走 SOS 提取 + IL 差分 + 结构证明；ClrMD 已被 P4b 证伪）。禁止事项：
//
// - **不得**硬编码任何 tosu 推导出来的数字（GPL 洁净，计划 §1 硬约束①/DEC-02）；
// - 每张表**自带 `evidence` 字符串**，写清"哪个构建、怎么提取的、怎么自证的"，
//   不是文件路径（`temp/` 交付后会被删除）；
// - `verified_build` 是**验证过的运行时构建标识**（lazer 版本 + runtime 版本 + 架构），
//   没有验证过的表不许写"看起来像"的值。
//
// 因此本文件**不含任何真实偏移数据**——lazer 的表由 Step 10 的生成器产出并随表落档。
//
// ## 表的 JSON 形状（生成器的输出契约）
//
// ```json
// {
//   "lazer_version": "2099.101.0.0",
//   "runtime_version": "10.0.0",
//   "arch": "x64",
//   "game_base_vtable": 140700000000000,
//   "types": { "BeatmapInfo": { "MD5Hash": 40 } },
//   "verified_build": "<该表的自证记录标识>",
//   "evidence": "<怎么提取的 + 怎么自证的>"
// }
// ```
//
// `game_base_vtable` 可省略（`null` = 本表未提取到）；其余六个必需。
// `types` 的键是 CLR 类型名、字段名逐字照源码/元数据（**不做大小写归一**：字段名错一个
// 字母就该查不到，而不是悄悄命中另一个字段）。
//
// ## `runtime` 段（Step 10f：EEType → 类型名）
//
// `types` 段是"某个已知类型上的某个字段在哪"，它答不了"**这个活对象的类型叫什么**"
// （`state.name` 需要的正是后者）。所以表里另有一段**运行时结构**（同一批双见证规则：
// 每个位移都由生成器从 dump 里的 SOS 打印 + dump 字节逐字比出来）：
//
// ```json
// "runtime": {
//   "eetype":       { "token": {"offset": 8, "shift": 8, "witness": "…"},
//                     "loader_module": {"offset": 24, "shift": 0, "witness": "…"} },
//   "module":       { "image_base": {"offset": 200, "shift": 0, "witness": "…"} },
//   "screen_array": { "length": {"offset": 8, …}, "elements": {"offset": 16, "stride": 8, …} },
//   "typedefs":     { "osu.Game.dll": { "9A2": "osu.Game.Screens.Menu.MainMenu", … } },
//   "observed":     { "osu.Game.dll": { "9A2": "…" } },   // dumpmt 直接印出来的那一部分
//   "witness": "…"
// }
// ```
//
// 读取侧的**完整链**（`lazer.rs` 每一跳都只信表里的位移，绝不写死）：
//
// ```text
//   screenObj   → [screenObj]                        = MethodTable（EEType）
//   rid         → u32[MT + eetype.token.offset] >> eetype.token.shift
//   module      → [MT + eetype.loader_module.offset]              （Module 对象，loader heap）
//   imageBase   → [module + module.image_base.offset]             （该程序集的映像基址）
//   moduleName  → 目标进程的模块表（Toolhelp32）按 imageBase 反查（基址在进程内唯一）
//   typeName    → runtime.typedefs[moduleName][<rid 的十六进制大写>]
// ```
//
// `typedefs` 的键是**模块文件名**（`MODULEENTRY32W.szModule` / IL 清单的 `assembly:<名字>`），
// 值是 `TypeDef RID → 完整类型名`；IL 侧给出全量、`observed` 给出 dumpmt 直接印出的那一部分
// （两者必须一致，否则生成器拒绝出表）。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use crate::osu::model::Reason;

/// 查一个「类型 + 字段」的**结构化查法**（不写死类型名的完整拼写）。
///
/// 为什么不是精确类型名：表的类型键是**运行时类型**（`dumpobj` 的 `Name:` 行规范化后的形态），
/// 而泛型实例化的写法跨构建会变（程序集限定后缀、SOS 列宽截断、C# 侧加不加程序集名）。
/// 所以查法与生成器 `spec.rs::CHAIN` 的 `contains` 判据**同形**：类型键必须**包含**全部子串。
///
/// 硬规则（见 `OffsetTable::offset_for`）：**恰好一个**类型键命中才算找到；0 个 ⇒ `NoType`；
/// ≥2 个 ⇒ `Ambiguous`（拒读并降级，绝不猜另一个实例化——`Bindable<T>.value` 的偏移
/// **依赖 T**，README 的判据表原话是"缺对应实例化时必须按字段降级"）。
#[derive(Clone, Copy, Debug)]
pub struct FieldLookup {
    /// 类型键必须**全部**包含的子串（all-of；全部按 [`canonical_type`] 规范化后比较）。
    pub type_all: &'static [&'static str],
    /// 类型键必须**至少**包含其一的子串（any-of；空 = 不约束）。
    ///
    /// 用在"派生类改名不该被当成结构失效"的地方（`GameBase` 的活对象是
    /// `osu.Desktop.OsuGameDesktop`，而台账里给的是基类族）——与生成器 `spec.rs::CHAIN`
    /// 的 `contains` 判据同义。
    pub type_any: &'static [&'static str],
    /// 字段名（**逐字**，不做大小写归一：错一个字母就该查不到）。
    pub field: &'static str,
}

impl FieldLookup {
    /// all-of 形态（最常用：一个类型子串 + 一个字段）。
    pub const fn new(type_all: &'static [&'static str], field: &'static str) -> FieldLookup {
        FieldLookup {
            type_all,
            type_any: &[],
            field,
        }
    }

    /// any-of 形态（派生类改名容忍；见 `type_any`）。
    pub const fn new_any(type_any: &'static [&'static str], field: &'static str) -> FieldLookup {
        FieldLookup {
            type_all: &[],
            type_any,
            field,
        }
    }

    /// 日志/降级标记用的短名（`类型子串#字段`）。
    pub fn label(&self) -> String {
        let mut parts: Vec<&str> = self.type_all.to_vec();
        parts.extend_from_slice(self.type_any);
        format!("{}#{}", parts.join("+"), self.field)
    }
}

/// 字段查找失败的原因（**字面量**，进 `degradedFields` 与日志）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LookupError {
    /// 没有任何类型键包含全部子串 ⇒ 该类型的字段本表没有。
    NoType(String),
    /// ≥2 个类型键命中 ⇒ 分不出实例化（**拒绝**，不猜）。
    Ambiguous(String, Vec<String>),
    /// 类型找到了但字段不在（改名/被裁掉）。
    NoField(String, String),
    /// 偏移不在合理域（对象头之后、且不超过 [`MAX_FIELD_OFFSET`]）。
    OutOfRange(String, i64),
}

impl std::fmt::Display for LookupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LookupError::NoType(label) => write!(f, "no-type:{label}"),
            LookupError::Ambiguous(label, keys) => {
                write!(f, "ambiguous:{label}({})", keys.join(","))
            }
            LookupError::NoField(key, field) => write!(f, "no-field:{key}.{field}"),
            LookupError::OutOfRange(label, offset) => {
                write!(f, "offset-out-of-range:{label}={offset}")
            }
        }
    }
}

/// 字段偏移的合理下界（x64 对象的前 8 字节是 MethodTable 指针 ⇒ 字段必在 `+0x08` 之后）。
pub const MIN_FIELD_OFFSET: i64 = 0x08;
/// 字段偏移的合理上界：真机最大字段（`OsuScreenStack` 的 `stack`）是 `0x320`；给 16 倍余量。
/// 超出即判"表坏了/查错了键"，**不给假值**。
pub const MAX_FIELD_OFFSET: i64 = 0x4000;

pub const DEFAULT_STABLE_TABLE_JSON: &str = include_str!("../../offsets/stable/stable__x86.json");
pub const DEFAULT_LAZER_TABLE_JSON: &str = include_str!("../../offsets/lazer/2026.1005.0.0__10.0.12__x64.json");

/// lazer 锚点与多跳拓扑段（可选，未配置时使用既有静态默认值）
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct LazerAnchorsSection {
    #[serde(default = "default_lazer_marker_pattern")]
    pub marker_pattern: String,
    #[serde(default = "default_lazer_site_deltas")]
    pub site_deltas: Vec<i64>,
    #[serde(default = "default_lazer_game_base_hops")]
    pub game_base_hops: Vec<(String, u64)>,
}

fn default_lazer_marker_pattern() -> String {
    "01 01 00 00 00 00 80 44 00 00 40 44".to_string()
}

fn default_lazer_site_deltas() -> Vec<i64> {
    vec![0x24, 0x28, 0x2c, 0x20, 0x30, 0x1c, 0x34]
}

fn default_lazer_game_base_hops() -> Vec<(String, u64)> {
    vec![
        ("external_link_opener".to_string(), 0x0),
        ("api_access".to_string(), 0x218),
        ("game".to_string(), 0x310),
    ]
}

/// 一张偏移表（一个 `(lazer 版本, runtime 版本, 架构)` 组合一份）。
///
/// 字段顺序/命名与生成器的 JSON 同形，`load` 直接反序列化。
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct OffsetTable {
    /// lazer 版本（`sq.version` / runtime log 里的 `Running osu <ver>`）。
    pub lazer_version: String,
    /// .NET runtime 版本（`Running osu <lazer> on .NET <runtime>`）。
    pub runtime_version: String,
    /// 架构（`x64` / `x86`；**位数分派**与表键都靠它）。
    pub arch: String,
    /// `GameBase` 的 MethodTable（vtable）。`None` = 本表未提取到（按字段降级）。
    pub game_base_vtable: Option<u64>,
    /// 锚点与多跳拓扑（可选；缺席时回落静态默认值）。
    #[serde(default)]
    pub anchors: Option<LazerAnchorsSection>,
    /// 屏幕类型名 → 状态名映射（可选；缺席时回落静态默认映射）。
    #[serde(default)]
    pub screen_states: Option<BTreeMap<String, String>>,
    /// `Type.Field → offset`（`BTreeMap`：JSON 对象键序不稳定，用有序表保证可复现）。
    pub types: BTreeMap<String, BTreeMap<String, i64>>,
    /// **运行时结构段**（Step 10f；见文件头）：EEType→类型名 这条链的位移与 RID 表。
    /// `None` = 本表没有这一段（旧表）⇒ 依赖它的字段按字段级降级，绝不猜。
    #[serde(default)]
    pub runtime: Option<RuntimeSection>,
    /// 验证过的运行时构建标识（表自证的一部分）。
    pub verified_build: String,
    /// **验证方法**（不是文件路径）：这个偏移是怎么提取的、怎么证明它对。
    pub evidence: String,
}

/// `runtime` 段里的一个位移（`witness` = 生成器留下的证据行；读取侧只读 `offset`/`shift`）。
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct RuntimeEntry {
    /// 对象基准（EEType / Module / 数组对象）上的位移。
    pub offset: i64,
    /// 读出来的整数要右移多少位才是 RID（`token` 用；其余为 0）。
    #[serde(default)]
    pub shift: u32,
    /// 数组元素的步长（`screen_array.elements` 用；其余为 0）。
    #[serde(default)]
    pub stride: i64,
    /// 为什么这个位移是对的（生成器的 SOS 行 + dump 字节比对，逐字留存）。
    #[serde(default)]
    pub witness: String,
}

impl RuntimeEntry {
    /// 位移的合理域判据（与 [`MIN_FIELD_OFFSET`]/[`MAX_FIELD_OFFSET`] 同一口径）。
    pub fn usable(&self) -> bool {
        self.offset >= 0 && self.offset <= MAX_FIELD_OFFSET
    }
}

/// 运行时结构段（见文件头）。每一组都是 `名字 → 位移`。
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct RuntimeSection {
    /// `token`（TypeDef RID 的来源）/ `loader_module`（Module 指针）。
    #[serde(default)]
    pub eetype: BTreeMap<String, RuntimeEntry>,
    /// Module 对象上的 `image_base`。
    #[serde(default)]
    pub module: BTreeMap<String, RuntimeEntry>,
    /// 屏幕栈数组对象上的 `length` / `elements`。
    #[serde(default)]
    pub screen_array: BTreeMap<String, RuntimeEntry>,
    /// `模块文件名 → (RID 十六进制大写 → 完整类型名)`（IL 侧全量）。
    #[serde(default)]
    pub typedefs: BTreeMap<String, BTreeMap<String, String>>,
    /// 同一张映射里**由 dumpmt 直接印出**的那一部分（SOS 见证；与 `typedefs` 必须一致）。
    #[serde(default)]
    pub observed: BTreeMap<String, BTreeMap<String, String>>,
    /// 这一段的整体见证（哪些 MT / 哪些模块 / 哪份 dump）。
    #[serde(default)]
    pub witness: String,
}

impl RuntimeSection {
    /// 取一组位移里的一个（缺 → `None`，调用方按字段级降级处理）。
    pub fn entry(&self, group: &str, name: &str) -> Option<&RuntimeEntry> {
        let map = match group {
            "eetype" => &self.eetype,
            "module" => &self.module,
            "screen_array" => &self.screen_array,
            _ => return None,
        };
        map.get(name).filter(|entry| entry.usable())
    }

    /// `(模块文件名, RID)` → 类型名（键是 RID 的**十六进制大写**，两端同一格式）。
    pub fn type_name(&self, module: &str, rid: u32) -> Option<&str> {
        self.typedefs
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(module))
            .and_then(|(_, map)| map.get(&format!("{rid:X}")))
            .map(|value| value.as_str())
            .filter(|value| !value.trim().is_empty())
    }

    /// 本段覆盖的模块文件名（诊断用）。
    pub fn modules(&self) -> Vec<&str> {
        self.typedefs.keys().map(|key| key.as_str()).collect()
    }
}

/// JSON 根键名（错误消息与测试都引用它，避免两处各写一遍字面量）。
/// `runtime` 之后的这一段是可选的（见 [`OffsetTable::runtime`]）。
pub const TABLE_KEYS: &[&str] = &[
    "lazer_version",
    "runtime_version",
    "arch",
    "game_base_vtable",
    "types",
    "runtime",
    "verified_build",
    "evidence",
];

/// 表的加载错误：**字面量**（调用方把它拼进 reason / 日志，不引入新 reason 形状）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadError {
    /// JSON 本身坏了（含缺字段/类型不符——`serde` 的消息足够定位）。
    Json(String),
    /// 版本/架构/证明缺失（空串也算缺：空版本号落不进任何键）。
    EmptyField(&'static str),
    /// 纯数据 Schema 校验失败（不变量违规/数值越界/危险内容）。
    Validation(String),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Json(message) => write!(f, "offsets-load: {message}"),
            LoadError::EmptyField(field) => write!(f, "offsets-load: empty {field}"),
            LoadError::Validation(message) => write!(f, "offsets-load: {message}"),
        }
    }
}

impl OffsetTable {
    /// JSON **字节** → 表。刻意不吃路径：表的来源（文件/内嵌/生成器管道）由调用方决定，
    /// 本类型只关心内容（也便于用合成 JSON 做单测）。
    pub fn load(bytes: &[u8]) -> Result<OffsetTable, LoadError> {
        let table: OffsetTable =
            serde_json::from_slice(bytes).map_err(|e| LoadError::Json(e.to_string()))?;
        for (field, value) in [
            ("lazer_version", &table.lazer_version),
            ("runtime_version", &table.runtime_version),
            ("arch", &table.arch),
            ("verified_build", &table.verified_build),
            ("evidence", &table.evidence),
        ] {
            if value.trim().is_empty() {
                return Err(LoadError::EmptyField(field));
            }
        }
        validate_lazer_schema(&table).map_err(|e| LoadError::Validation(e.to_string()))?;
        Ok(table)
    }

    /// 标记模式（来自表或默认值）。
    pub fn marker_pattern(&self) -> &str {
        self.anchors
            .as_ref()
            .map(|a| a.marker_pattern.as_str())
            .unwrap_or("01 01 00 00 00 00 80 44 00 00 40 44")
    }

    /// 站点候选位移序列（来自表或默认值）。
    pub fn site_deltas(&self) -> &[i64] {
        self.anchors
            .as_ref()
            .map(|a| a.site_deltas.as_slice())
            .unwrap_or(&[0x24, 0x28, 0x2c, 0x20, 0x30, 0x1c, 0x34])
    }

    /// 站点到 GameBase 的多跳链路（来自表或默认值）。
    pub fn game_base_hops(&self) -> Vec<(&str, u64)> {
        if let Some(anchors) = &self.anchors {
            anchors
                .game_base_hops
                .iter()
                .map(|(label, offset)| (label.as_str(), *offset))
                .collect()
        } else {
            vec![
                ("external_link_opener", 0x0),
                ("api_access", 0x218),
                ("game", 0x310),
            ]
        }
    }

    /// 屏幕类型名 → 状态名（若表定义了自定义映射优先使用）。
    pub fn screen_state_for(&self, type_name: &str) -> Option<&str> {
        if let Some(states) = &self.screen_states {
            if let Some(name) = states.get(type_name) {
                return Some(name.as_str());
            }
        }
        None
    }

    /// 表键（版本 + runtime + 架构）——日志与证据里的唯一标识。
    pub fn key(&self) -> String {
        format!(
            "{}/{}/{}",
            self.lazer_version, self.runtime_version, self.arch
        )
    }

    /// 查一个 `Type.Field` 的偏移。类型或字段不在表里 ⇒ `None`
    /// （**不**回退到别的类型/字段：那会让"读错字段"看起来像"读到了"）。
    pub fn offset(&self, type_name: &str, field: &str) -> Option<i64> {
        self.types.get(type_name)?.get(field).copied()
    }

    /// `runtime` 段（`None` = 旧表没有这一段）。
    pub fn runtime(&self) -> Option<&RuntimeSection> {
        self.runtime.as_ref()
    }

    /// `runtime.<group>.<name>` 的位移（缺段/缺项/越界 ⇒ `None`）。
    pub fn runtime_entry(&self, group: &str, name: &str) -> Option<&RuntimeEntry> {
        self.runtime.as_ref()?.entry(group, name)
    }

    /// `(模块文件名, RID)` → 类型名（`runtime` 段缺失 ⇒ `None`）。
    pub fn runtime_type_name(&self, module: &str, rid: u32) -> Option<&str> {
        self.runtime.as_ref()?.type_name(module, rid)
    }

    /// 按 [`FieldLookup`] 查偏移（唯一类型命中 + 字段存在 + 偏移在合理域）。
    ///
    /// 三种失败各自给出**可进日志/降级标记**的原因；调用方把它们记进 `degradedFields`
    /// 并让该字段**不出现在载荷里**（绝不填 0/占位）。
    pub fn offset_for(&self, lookup: &FieldLookup) -> Result<i64, LookupError> {
        let all: Vec<String> = lookup
            .type_all
            .iter()
            .map(|text| canonical_type(text))
            .collect();
        let any: Vec<String> = lookup
            .type_any
            .iter()
            .map(|text| canonical_type(text))
            .collect();
        let mut matches: Vec<(String, i64)> = Vec::new();
        for (key, fields) in &self.types {
            let canonical = canonical_type(key);
            let all_hit = all.iter().all(|needle| canonical.contains(needle.as_str()));
            let any_hit = any.is_empty() || any.iter().any(|needle| canonical.contains(needle.as_str()));
            if !(all_hit && any_hit) {
                continue;
            }
            if let Some(offset) = fields.get(lookup.field).copied() {
                matches.push((canonical, offset));
            } else {
                // 类型命中但字段不在：这是"字段被改名/裁掉"，比"类型没有"更值得单列。
                matches.push((canonical, i64::MIN));
            }
        }
        match matches.len() {
            0 => Err(LookupError::NoType(lookup.label())),
            1 => {
                let (key, offset) = matches.remove(0);
                if offset == i64::MIN {
                    return Err(LookupError::NoField(key, lookup.field.to_string()));
                }
                if !(MIN_FIELD_OFFSET..=MAX_FIELD_OFFSET).contains(&offset) {
                    return Err(LookupError::OutOfRange(lookup.label(), offset));
                }
                Ok(offset)
            }
            _ => {
                let keys: Vec<String> = matches.into_iter().map(|(key, _)| key).collect();
                Err(LookupError::Ambiguous(lookup.label(), keys))
            }
        }
    }

    /// 与目标环境不一致的**第一处**（`None` = 版本位面一致）。
    ///
    /// 刻意**不**在这里拒绝：不一致的表仍可被接受，但必须**显式**记下来（就近回落要
    /// "大声记日志"，见计划 §5 风险表「就近版本偏移回落」行）。调用方（Step 10）按
    /// L1 结构证明决定是否放行，本文件不代它决定。
    pub fn mismatch(&self, lazer_version: &str, runtime_version: &str, arch: &str) -> Option<String> {
        if self.lazer_version != lazer_version {
            return Some(format!("version:{}", self.lazer_version));
        }
        if self.runtime_version != runtime_version {
            return Some(format!("runtime:{}", self.runtime_version));
        }
        if !self.arch.eq_ignore_ascii_case(arch) {
            return Some(format!("arch:{}", self.arch));
        }
        None
    }

    /// 目标环境：就近回落要匹配的三个键。
    pub fn target(lazer_version: &str, runtime_version: &str, arch: &str) -> Target {
        Target {
            lazer_version: lazer_version.to_string(),
            runtime_version: runtime_version.to_string(),
            arch: arch.to_string(),
        }
    }

    /// **就近版本**回落的唯一入口（计划 §4.0 / Step 10：`offset.rs` 只放数据 + 加载）。
    ///
    /// 规则（写死，实现期不再选）：
    /// 1. `tables` 里挑 `target` **完全匹配**的那张；没有就在 `max_distance` 之内挑最近的
    ///    一张（距离 = 版本距离 + runtime 距离 + 架构权重；架构不同永不采纳）；
    /// 2. 候选**必须**过 `policy.validate`（调用方提供的 L1 结构证明钩子）；
    /// 3. 校验不过 ⇒ 本函数返回 `Err`——**拒绝**，而不是"再试下一个候选"
    ///    （逐个试会把"结构证明不通过"变成"总能找到一张能过的表"）。
    ///
    /// `ValidationPolicy` 的 `validate` 是**默认拒绝**：不显式给出谓词就永远拿不到回落表
    /// （`PLAN.md` §4 「就近回落仅在 L1 结构证明通过时允许」，默认拒绝是这句的机械保证）。
    pub fn nearest_table<'a>(
        tables: &'a [OffsetTable],
        target: &Target,
        max_distance: u32,
        policy: &ValidationPolicy<'_>,
    ) -> Result<&'a OffsetTable, NearestError> {
        if tables.is_empty() {
            return Err(NearestError::NoCandidates);
        }
        let mut ranked: Vec<(u32, &OffsetTable)> = tables
            .iter()
            .filter_map(|table| version_distance(table, target).map(|d| (d, table)))
            .collect();
        if ranked.is_empty() {
            return Err(NearestError::NoCandidates);
        }
        // 距离升序；同距离时按版本串稳定排序（可复现：不给"数组里的先后"当判据）。
        ranked.sort_by(|a, b| {
            a.0.cmp(&b.0)
                .then_with(|| a.1.lazer_version.cmp(&b.1.lazer_version))
                .then_with(|| a.1.runtime_version.cmp(&b.1.runtime_version))
        });
        let (distance, nearest) = ranked[0];
        if distance > max_distance {
            return Err(NearestError::TooFar {
                distance,
                max_distance,
            });
        }
        match policy.validate(nearest) {
            true => Ok(nearest),
            false => Err(NearestError::Refused(nearest.key())),
        }
    }

    /// 本表覆盖的 `Type.Field` 标签（**降级清单的来源**：表缺失/不可用时，读者按它逐字段
    /// 上报"这些字段因此没有来源"，绝不填零/占位）。
    pub fn field_labels(&self) -> Vec<String> {
        let mut labels: Vec<String> = Vec::new();
        for (key, fields) in &self.types {
            for field in fields.keys() {
                labels.push(format!("{key}.{field}"));
            }
        }
        labels
    }
}

/// **CLR 类型名规范化**（读取侧的唯一规则，与生成器 `tools/lazer-offsets-gen/names.rs`
/// 逐字同源——表的类型键就是按它产出的，读的那一侧必须用同一条规则）。
///
/// - 泛型实例化：``Name`1[[Arg, Asm],[Arg2, Asm2]]`` → ``Name`1<Arg,Arg2>``（去程序集限定与空白）
/// - 数组后缀原样保留（`X[]`）、嵌套类型保留 `+`
/// - 被 SOS 列宽截断的形态（含 `...`）不做结构解析：去空白后原样返回
///
/// ⚠️ 两份实现（工具侧 / 读取侧）**必须逐字一致**，否则"类型键查不到"会伪装成"表里没这个
/// 字段"。工具侧的自测（`names/canonical-table`）与读取侧的 `osu_lazer.rs` 用例各自钉住
/// 同一批样本；任何一侧改动都要同时改另一侧。
pub fn canonical_type(display: &str) -> String {
    let trimmed = display.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.contains("...") {
        return trimmed.split_whitespace().collect::<Vec<_>>().join("");
    }
    let mut parser = TypeParser {
        bytes: trimmed.as_bytes(),
        index: 0,
    };
    let mut out = parser.parse_type();
    parser.skip_ws();
    out = out.trim().to_string();
    if out.is_empty() {
        out = trimmed.split_whitespace().collect::<Vec<_>>().join("");
    }
    out
}

struct TypeParser<'a> {
    bytes: &'a [u8],
    index: usize,
}

impl<'a> TypeParser<'a> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.index).copied()
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ') | Some(b'\t')) {
            self.index += 1;
        }
    }

    fn parse_type(&mut self) -> String {
        self.skip_ws();
        let start = self.index;
        while let Some(c) = self.peek() {
            if c == b'[' || c == b']' || c == b',' {
                break;
            }
            self.index += 1;
        }
        let mut name: String = String::from_utf8_lossy(&self.bytes[start..self.index])
            .trim()
            .to_string();
        if self.peek() == Some(b'[') && self.bytes.get(self.index + 1) == Some(&b']') {
            while self.peek() == Some(b'[') && self.bytes.get(self.index + 1) == Some(&b']') {
                name.push_str("[]");
                self.index += 2;
            }
            return name;
        }
        if self.peek() == Some(b'[') && self.bytes.get(self.index + 1) == Some(&b'[') {
            self.index += 2;
            let mut args: Vec<String> = Vec::new();
            loop {
                self.skip_ws();
                if self.peek() == Some(b'[') {
                    self.index += 1;
                }
                let arg = self.parse_type();
                args.push(arg);
                self.skip_to_close();
                self.skip_ws();
                if self.peek() == Some(b',') {
                    self.index += 1;
                    continue;
                }
                break;
            }
            while self.peek() == Some(b']') {
                self.index += 1;
            }
            if !args.is_empty() {
                name = format!("{name}<{}>", args.join(","));
            }
            while self.peek() == Some(b'[') && self.bytes.get(self.index + 1) == Some(&b']') {
                name.push_str("[]");
                self.index += 2;
            }
            return name;
        }
        name
    }

    fn skip_to_close(&mut self) {
        let mut depth = 0usize;
        while let Some(c) = self.peek() {
            match c {
                b'[' => {
                    depth += 1;
                    self.index += 1;
                }
                b']' => {
                    if depth == 0 {
                        self.index += 1;
                        return;
                    }
                    depth -= 1;
                    self.index += 1;
                }
                _ => self.index += 1,
            }
        }
    }
}

/// 目标环境（就回落要匹配的键）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub lazer_version: String,
    pub runtime_version: String,
    pub arch: String,
}

/// 回落被拒的原因（`Display` 直接可进日志）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NearestError {
    /// 表集为空，或没有任何架构匹配的候选。
    NoCandidates,
    /// 最近的候选超出允许距离。
    TooFar { distance: u32, max_distance: u32 },
    /// 校验谓词拒了最近的候选（`String` = 候选的 `key()`）。
    Refused(String),
}

impl std::fmt::Display for NearestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NearestError::NoCandidates => write!(f, "nearest: no candidate"),
            NearestError::TooFar {
                distance,
                max_distance,
            } => write!(f, "nearest: distance {distance} > {max_distance}"),
            NearestError::Refused(key) => write!(f, "nearest: refused by policy ({key})"),
        }
    }
}

/// 校验钩子。**默认实现恒拒**：结构证明是读的那一侧（`lazer.rs`/`invariants.rs`）的事，
/// 本文件不提供"看起来能过"的默认判据（计划 §4.0：`offsets.rs` 只放数据）。
pub struct ValidationPolicy<'a> {
    validate: &'a dyn Fn(&OffsetTable) -> bool,
}

impl Default for ValidationPolicy<'static> {
    fn default() -> Self {
        Self::refuse()
    }
}

impl<'a> ValidationPolicy<'a> {
    /// 用调用方给的谓词构造（谓词 = 该候选表在目标进程上的 L1 结构证明）。
    pub fn new(validate: &'a dyn Fn(&OffsetTable) -> bool) -> Self {
        ValidationPolicy { validate }
    }

    /// 恒拒（默认）。需要它时显式写出来，让"没有校验"在调用点可见。
    pub fn refuse() -> Self {
        ValidationPolicy {
            validate: &|_table: &OffsetTable| false,
        }
    }

    pub fn validate(&self, table: &OffsetTable) -> bool {
        (self.validate)(table)
    }
}

/// 版本距离：`(lazer 距离) + (runtime 距离)`；架构不同 ⇒ `None`（永不跨架构回落）。
///
/// lazer 版本是 `年.月日.修订.构建` 形状（本机实测 `2026.921.0.0`），runtime 是
/// `主.次.修订.构建`（实测 `10.0.12`）——两段都按**数值**逐段比较，缺失的段按 0 算。
/// 架构只做"同/不同"，不参与距离：x86 与 x64 的偏移不可互换。
fn version_distance(table: &OffsetTable, target: &Target) -> Option<u32> {
    if !table.arch.eq_ignore_ascii_case(&target.arch) {
        return None;
    }
    Some(component_distance(&table.lazer_version, &target.lazer_version).saturating_add(
        component_distance(&table.runtime_version, &target.runtime_version),
    ))
}

/// 逐段绝对差之和（缺段按 0；非数字段按 0——表是生成器产物，这里只求"稳定排序"）。
fn component_distance(left: &str, right: &str) -> u32 {
    let left: Vec<u32> = left.split('.').map(parse_component).collect();
    let right: Vec<u32> = right.split('.').map(parse_component).collect();
    let len = left.len().max(right.len());
    let mut total = 0u32;
    for index in 0..len {
        let a = left.get(index).copied().unwrap_or(0);
        let b = right.get(index).copied().unwrap_or(0);
        total = total.saturating_add(a.abs_diff(b));
    }
    total
}

fn parse_component(text: &str) -> u32 {
    text.trim().parse::<u32>().unwrap_or(0)
}

// ---- stable 纯数据表模型与校验 ----

/// stable 锚点定义
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct StableAnchorDef {
    pub pattern: String,
    pub offset: i32,
    #[serde(default)]
    pub derivation: String,
    #[serde(default)]
    pub evidence: String,
}

/// stable 多跳链路与偏移拓扑
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct StableTopology {
    #[serde(default = "default_play_time_from_anchor")]
    pub play_time_from_anchor: u32,
    #[serde(default = "default_beatmap_from_base")]
    pub beatmap_from_base: u32,
    #[serde(default = "default_info_from_base")]
    pub info_from_base: u32,
    #[serde(default = "default_retries_offset")]
    pub retries_offset: u32,
    #[serde(default = "default_plays_offset")]
    pub plays_offset: u32,
    #[serde(default = "default_ruleset_from_anchor")]
    pub ruleset_from_anchor: u32,
    #[serde(default = "default_ruleset_list_offset")]
    pub ruleset_list_offset: u32,
    #[serde(default = "default_gameplay_from_ruleset")]
    pub gameplay_from_ruleset: u32,
    #[serde(default = "default_result_from_ruleset")]
    pub result_from_ruleset: u32,
    #[serde(default = "default_score_from_gameplay")]
    pub score_from_gameplay: u32,
    #[serde(default = "default_mods_container")]
    pub mods_container: u32,
    #[serde(default = "default_mods_xor_high")]
    pub mods_xor_high: u32,
    #[serde(default = "default_mods_xor_low")]
    pub mods_xor_low: u32,
    #[serde(default = "default_score_processor_from_score")]
    pub score_processor_from_score: u32,
    #[serde(default = "default_scorev2_bit")]
    pub scorev2_bit: u32,
    #[serde(default = "default_result_score_offset")]
    pub result_score_offset: u32,
    #[serde(default = "default_result_max_combo_offset")]
    pub result_max_combo_offset: u32,
    #[serde(default = "default_result_player_name_offset")]
    pub result_player_name_offset: u32,
    #[serde(default = "default_result_online_id_offset")]
    pub result_online_id_offset: u32,
    #[serde(default = "default_mp3_length_from_anchor")]
    pub mp3_length_from_anchor: u32,
    #[serde(default = "default_mp3_length_field")]
    pub mp3_length_field: u32,
    #[serde(default = "default_hits_candidate_offsets")]
    pub hits_candidate_offsets: Vec<u32>,
    #[serde(default = "default_hits_slot_mapping")]
    pub hits_slot_mapping: Vec<(usize, String)>,
    #[serde(default = "default_beatmap_md5")]
    pub beatmap_md5: u32,
    #[serde(default = "default_beatmap_filename")]
    pub beatmap_filename: u32,
    #[serde(default = "default_beatmap_folder")]
    pub beatmap_folder: u32,
    #[serde(default = "default_beatmap_version")]
    pub beatmap_version: u32,
    #[serde(default = "default_beatmap_artist")]
    pub beatmap_artist: u32,
    #[serde(default = "default_beatmap_title")]
    pub beatmap_title: u32,
    #[serde(default = "default_beatmap_mapper")]
    pub beatmap_mapper: u32,
    #[serde(default = "default_beatmap_id")]
    pub beatmap_id: u32,
    #[serde(default = "default_beatmap_set_id")]
    pub beatmap_set_id: u32,
}

fn default_play_time_from_anchor() -> u32 { 0x5 }
fn default_beatmap_from_base() -> u32 { 0xC }
fn default_info_from_base() -> u32 { 0x33 }
fn default_retries_offset() -> u32 { 0x8 }
fn default_plays_offset() -> u32 { 0xC }
fn default_ruleset_from_anchor() -> u32 { 0xB }
fn default_ruleset_list_offset() -> u32 { 0x4 }
fn default_gameplay_from_ruleset() -> u32 { 0x64 }
fn default_result_from_ruleset() -> u32 { 0x38 }
fn default_score_from_gameplay() -> u32 { 0x38 }
fn default_mods_container() -> u32 { 0x1C }
fn default_mods_xor_high() -> u32 { 0xC }
fn default_mods_xor_low() -> u32 { 0x8 }
fn default_score_processor_from_score() -> u32 { 0x54 }
fn default_scorev2_bit() -> u32 { 0x2000_0000 }
fn default_result_score_offset() -> u32 { 0x78 }
fn default_result_max_combo_offset() -> u32 { 0x68 }
fn default_result_player_name_offset() -> u32 { 0x28 }
fn default_result_online_id_offset() -> u32 { 0x4 }
fn default_mp3_length_from_anchor() -> u32 { 0x7 }
fn default_mp3_length_field() -> u32 { 0x4 }
fn default_beatmap_md5() -> u32 { 0x6C }
fn default_beatmap_filename() -> u32 { 0x90 }
fn default_beatmap_folder() -> u32 { 0x78 }
fn default_beatmap_version() -> u32 { 0xAC }
fn default_beatmap_artist() -> u32 { 0x18 }
fn default_beatmap_title() -> u32 { 0x24 }
fn default_beatmap_mapper() -> u32 { 0x7C }
fn default_beatmap_id() -> u32 { 0xC8 }
fn default_beatmap_set_id() -> u32 { 0xCC }
fn default_hits_candidate_offsets() -> Vec<u32> {
    vec![0x88, 0x8A, 0x8C, 0x8E, 0x90, 0x92, 0x94, 0x68]
}
fn default_hits_slot_mapping() -> Vec<(usize, String)> {
    vec![
        (0, "100".to_string()),
        (1, "300".to_string()),
        (2, "50".to_string()),
        (3, "geki".to_string()),
        (4, "katu".to_string()),
        (5, "miss".to_string()),
    ]
}

impl Default for StableTopology {
    fn default() -> Self {
        StableTopology {
            play_time_from_anchor: default_play_time_from_anchor(),
            beatmap_from_base: default_beatmap_from_base(),
            info_from_base: default_info_from_base(),
            retries_offset: default_retries_offset(),
            plays_offset: default_plays_offset(),
            ruleset_from_anchor: default_ruleset_from_anchor(),
            ruleset_list_offset: default_ruleset_list_offset(),
            gameplay_from_ruleset: default_gameplay_from_ruleset(),
            result_from_ruleset: default_result_from_ruleset(),
            score_from_gameplay: default_score_from_gameplay(),
            mods_container: default_mods_container(),
            mods_xor_high: default_mods_xor_high(),
            mods_xor_low: default_mods_xor_low(),
            score_processor_from_score: default_score_processor_from_score(),
            scorev2_bit: default_scorev2_bit(),
            result_score_offset: default_result_score_offset(),
            result_max_combo_offset: default_result_max_combo_offset(),
            result_player_name_offset: default_result_player_name_offset(),
            result_online_id_offset: default_result_online_id_offset(),
            mp3_length_from_anchor: default_mp3_length_from_anchor(),
            mp3_length_field: default_mp3_length_field(),
            hits_candidate_offsets: default_hits_candidate_offsets(),
            hits_slot_mapping: default_hits_slot_mapping(),
            beatmap_md5: default_beatmap_md5(),
            beatmap_filename: default_beatmap_filename(),
            beatmap_folder: default_beatmap_folder(),
            beatmap_version: default_beatmap_version(),
            beatmap_artist: default_beatmap_artist(),
            beatmap_title: default_beatmap_title(),
            beatmap_mapper: default_beatmap_mapper(),
            beatmap_id: default_beatmap_id(),
            beatmap_set_id: default_beatmap_set_id(),
        }
    }
}

/// stable 映射段（状态名枚举、mods 映射等）
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct StableMappings {
    #[serde(default = "default_stable_states")]
    pub states: BTreeMap<String, String>,
}

fn default_stable_states() -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    map.insert("0".to_string(), "menu".to_string());
    map.insert("1".to_string(), "edit".to_string());
    map.insert("2".to_string(), "play".to_string());
    map.insert("4".to_string(), "selectEdit".to_string());
    map.insert("5".to_string(), "selectPlay".to_string());
    map.insert("7".to_string(), "resultScreen".to_string());
    map
}

impl Default for StableMappings {
    fn default() -> Self {
        StableMappings {
            states: default_stable_states(),
        }
    }
}

/// stable 偏移表（纯数据模型，对应 stable__x86.json）
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct StableTable {
    #[serde(default = "default_stable_client")]
    pub client: String,
    pub version: String,
    pub arch: String,
    pub anchors: BTreeMap<String, StableAnchorDef>,
    #[serde(default)]
    pub topology: StableTopology,
    #[serde(default)]
    pub mappings: StableMappings,
    pub verified_build: String,
    pub evidence: String,
}

fn default_stable_client() -> String {
    "stable".to_string()
}

impl StableTable {
    pub fn load(bytes: &[u8]) -> Result<StableTable, LoadError> {
        let table: StableTable =
            serde_json::from_slice(bytes).map_err(|e| LoadError::Json(e.to_string()))?;
        for (field, value) in [
            ("client", &table.client),
            ("version", &table.version),
            ("arch", &table.arch),
            ("verified_build", &table.verified_build),
            ("evidence", &table.evidence),
        ] {
            if value.trim().is_empty() {
                return Err(LoadError::EmptyField(field));
            }
        }
        validate_stable_schema(&table).map_err(|e| LoadError::Validation(e.to_string()))?;
        Ok(table)
    }

    pub fn anchor(&self, key: &str) -> Option<&StableAnchorDef> {
        self.anchors.get(key)
    }

    pub fn state_name(&self, index: i32) -> Option<&str> {
        self.mappings.states.get(&index.to_string()).map(|s| s.as_str())
    }
}

// ---- Schema 纯数据校验器 ----

/// Schema 不变量违规错误
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValidationError {
    ExecutableContent(String),
    HopDepthExceeded(usize),
    DisplacementOutOfRange(String, i64),
    StringTooLong(String, usize),
    InvalidPattern(String),
    UnknownStateName(String),
    InvalidAlignment(String, i64),
    MissingRequiredField(String),
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ValidationError::ExecutableContent(desc) => {
                write!(f, "validation-error: executable content detected ({desc})")
            }
            ValidationError::HopDepthExceeded(depth) => {
                write!(f, "validation-error: hop depth {depth} exceeds limit 16")
            }
            ValidationError::DisplacementOutOfRange(field, val) => {
                write!(f, "validation-error: displacement out of range: {field}={val}")
            }
            ValidationError::StringTooLong(field, len) => {
                write!(f, "validation-error: string too long: {field} (len {len} > 512)")
            }
            ValidationError::InvalidPattern(pat) => {
                write!(f, "validation-error: invalid pattern: {pat}")
            }
            ValidationError::UnknownStateName(name) => {
                write!(f, "validation-error: unknown state name '{name}' not in allowed set")
            }
            ValidationError::InvalidAlignment(field, val) => {
                write!(f, "validation-error: unaligned offset: {field}={val}")
            }
            ValidationError::MissingRequiredField(field) => {
                write!(f, "validation-error: missing required field '{field}'")
            }
        }
    }
}

pub const ALLOWED_STATE_NAMES: &[&str] = &[
    "menu",
    "edit",
    "play",
    "selectEdit",
    "selectPlay",
    "resultScreen",
    "resultsScreen",
    "multiplayer",
    "unknown",
    "",
];

fn check_string_safety(field: &str, s: &str) -> Result<(), ValidationError> {
    check_string_safety_bounded(field, s, 512)
}

fn check_long_string_safety(field: &str, s: &str) -> Result<(), ValidationError> {
    check_string_safety_bounded(field, s, 65_536)
}

fn check_string_safety_bounded(field: &str, s: &str, max_len: usize) -> Result<(), ValidationError> {
    if s.len() > max_len {
        return Err(ValidationError::StringTooLong(field.to_string(), s.len()));
    }
    let lower = s.to_ascii_lowercase();
    for needle in ["<script", "javascript:", "eval(", "exec(", "onload=", "onerror="] {
        if lower.contains(needle) {
            return Err(ValidationError::ExecutableContent(format!(
                "{field} contains forbidden token '{needle}'"
            )));
        }
    }
    Ok(())
}

fn check_pattern_safety(field: &str, pattern: &str) -> Result<(), ValidationError> {
    check_string_safety(field, pattern)?;
    let tokens: Vec<&str> = pattern.split_whitespace().collect();
    if tokens.is_empty() || tokens.len() > 64 {
        return Err(ValidationError::InvalidPattern(format!(
            "{field}: token count {} out of range 1..=64",
            tokens.len()
        )));
    }
    for token in tokens {
        if token == "??" {
            continue;
        }
        if token.len() != 2 || !token.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(ValidationError::InvalidPattern(format!(
                "{field}: invalid token '{token}' in pattern"
            )));
        }
    }
    Ok(())
}

pub fn validate_stable_schema(table: &StableTable) -> Result<(), ValidationError> {
    check_string_safety("client", &table.client)?;
    check_string_safety("version", &table.version)?;
    check_string_safety("arch", &table.arch)?;
    check_string_safety("verified_build", &table.verified_build)?;
    check_long_string_safety("evidence", &table.evidence)?;

    if table.anchors.is_empty() || table.anchors.len() > 16 {
        return Err(ValidationError::HopDepthExceeded(table.anchors.len()));
    }

    for (key, def) in &table.anchors {
        check_pattern_safety(&format!("anchor.{key}.pattern"), &def.pattern)?;
        check_long_string_safety(&format!("anchor.{key}.derivation"), &def.derivation)?;
        check_long_string_safety(&format!("anchor.{key}.evidence"), &def.evidence)?;
        if def.offset < -4096 || def.offset > 1_048_576 {
            return Err(ValidationError::DisplacementOutOfRange(
                format!("anchor.{key}.offset"),
                def.offset as i64,
            ));
        }
    }

    let topo = &table.topology;
    for (name, val) in [
        ("play_time_from_anchor", topo.play_time_from_anchor),
        ("beatmap_from_base", topo.beatmap_from_base),
        ("info_from_base", topo.info_from_base),
        ("retries_offset", topo.retries_offset),
        ("plays_offset", topo.plays_offset),
        ("ruleset_from_anchor", topo.ruleset_from_anchor),
        ("ruleset_list_offset", topo.ruleset_list_offset),
        ("gameplay_from_ruleset", topo.gameplay_from_ruleset),
        ("result_from_ruleset", topo.result_from_ruleset),
        ("score_from_gameplay", topo.score_from_gameplay),
        ("mods_container", topo.mods_container),
        ("mods_xor_high", topo.mods_xor_high),
        ("mods_xor_low", topo.mods_xor_low),
        ("score_processor_from_score", topo.score_processor_from_score),
        ("result_score_offset", topo.result_score_offset),
        ("result_max_combo_offset", topo.result_max_combo_offset),
        ("result_player_name_offset", topo.result_player_name_offset),
        ("result_online_id_offset", topo.result_online_id_offset),
        ("mp3_length_from_anchor", topo.mp3_length_from_anchor),
        ("mp3_length_field", topo.mp3_length_field),
        ("beatmap_md5", topo.beatmap_md5),
        ("beatmap_filename", topo.beatmap_filename),
        ("beatmap_folder", topo.beatmap_folder),
        ("beatmap_version", topo.beatmap_version),
        ("beatmap_artist", topo.beatmap_artist),
        ("beatmap_title", topo.beatmap_title),
        ("beatmap_mapper", topo.beatmap_mapper),
        ("beatmap_id", topo.beatmap_id),
        ("beatmap_set_id", topo.beatmap_set_id),
    ] {
        if val > 1_048_576 {
            return Err(ValidationError::DisplacementOutOfRange(name.to_string(), val as i64));
        }
    }

    if topo.hits_candidate_offsets.len() > 16 {
        return Err(ValidationError::HopDepthExceeded(topo.hits_candidate_offsets.len()));
    }
    for (i, offset) in topo.hits_candidate_offsets.iter().enumerate() {
        if *offset > 1_048_576 {
            return Err(ValidationError::DisplacementOutOfRange(
                format!("hits_candidate_offsets[{i}]"),
                *offset as i64,
            ));
        }
        if *offset % 2 != 0 {
            return Err(ValidationError::InvalidAlignment(
                format!("hits_candidate_offsets[{i}]"),
                *offset as i64,
            ));
        }
    }

    if topo.hits_slot_mapping.len() > 16 {
        return Err(ValidationError::HopDepthExceeded(topo.hits_slot_mapping.len()));
    }

    for (idx_str, name) in &table.mappings.states {
        check_string_safety("mappings.states.key", idx_str)?;
        check_string_safety("mappings.states.val", name)?;
        if !ALLOWED_STATE_NAMES.contains(&name.as_str()) {
            return Err(ValidationError::UnknownStateName(name.clone()));
        }
    }

    Ok(())
}

pub fn validate_lazer_schema(table: &OffsetTable) -> Result<(), ValidationError> {
    check_string_safety("lazer_version", &table.lazer_version)?;
    check_string_safety("runtime_version", &table.runtime_version)?;
    check_string_safety("arch", &table.arch)?;
    check_string_safety("verified_build", &table.verified_build)?;
    check_long_string_safety("evidence", &table.evidence)?;

    if let Some(anchors) = &table.anchors {
        check_pattern_safety("anchors.marker_pattern", &anchors.marker_pattern)?;
        if anchors.site_deltas.len() > 16 {
            return Err(ValidationError::HopDepthExceeded(anchors.site_deltas.len()));
        }
        for (i, delta) in anchors.site_deltas.iter().enumerate() {
            if *delta < -4096 || *delta > 4096 {
                return Err(ValidationError::DisplacementOutOfRange(
                    format!("anchors.site_deltas[{i}]"),
                    *delta,
                ));
            }
        }
        if anchors.game_base_hops.len() > 16 {
            return Err(ValidationError::HopDepthExceeded(anchors.game_base_hops.len()));
        }
        for (label, offset) in &anchors.game_base_hops {
            check_string_safety("anchors.game_base_hops.label", label)?;
            if *offset > 16_777_216 {
                return Err(ValidationError::DisplacementOutOfRange(
                    format!("anchors.game_base_hops.{label}"),
                    *offset as i64,
                ));
            }
        }
    }

    if let Some(states) = &table.screen_states {
        if states.len() > 128 {
            return Err(ValidationError::HopDepthExceeded(states.len()));
        }
        for (screen, state) in states {
            check_string_safety("screen_states.key", screen)?;
            check_string_safety("screen_states.val", state)?;
            if !ALLOWED_STATE_NAMES.contains(&state.as_str()) {
                return Err(ValidationError::UnknownStateName(state.clone()));
            }
        }
    }

    // Types sanity checks
    for (type_name, fields) in &table.types {
        check_string_safety("types.key", type_name)?;
        for (field_name, offset) in fields {
            check_string_safety("types.field", field_name)?;
            if *offset < 0 || *offset > MAX_FIELD_OFFSET * 16 {
                return Err(ValidationError::DisplacementOutOfRange(
                    format!("{type_name}.{field_name}"),
                    *offset,
                ));
            }
        }
    }

    // Runtime section checks
    if let Some(runtime) = &table.runtime {
        check_long_string_safety("runtime.witness", &runtime.witness)?;
        for (group_name, map) in [
            ("eetype", &runtime.eetype),
            ("module", &runtime.module),
            ("screen_array", &runtime.screen_array),
        ] {
            for (entry_name, entry) in map {
                check_long_string_safety(&format!("runtime.{group_name}.{entry_name}.witness"), &entry.witness)?;
                if entry.offset < 0 || entry.offset > MAX_FIELD_OFFSET * 16 {
                    return Err(ValidationError::DisplacementOutOfRange(
                        format!("runtime.{group_name}.{entry_name}.offset"),
                        entry.offset,
                    ));
                }
            }
        }
    }

    Ok(())
}

pub fn default_stable_table() -> StableTable {
    StableTable::load(DEFAULT_STABLE_TABLE_JSON.as_bytes())
        .expect("compiled-in stable table must be valid")
}

pub fn default_lazer_table() -> OffsetTable {
    OffsetTable::load(DEFAULT_LAZER_TABLE_JSON.as_bytes())
        .expect("compiled-in lazer table must be valid")
}

/// stable 表发现梯子
pub fn find_stable_table(dir_hint: Option<&Path>) -> Result<StableTable, Reason> {
    // 1. 显式环境变量 $MMA_STABLE_OFFSETS
    if let Ok(env_file) = std::env::var("MMA_STABLE_OFFSETS") {
        let p = PathBuf::from(env_file);
        if p.exists() {
            if let Ok(bytes) = std::fs::read(&p) {
                if let Ok(table) = StableTable::load(&bytes) {
                    return Ok(table);
                }
            }
        }
    }

    // 2. 统一偏移目录环境变量 $MMA_OFFSETS_DIR
    if let Ok(offsets_dir) = std::env::var("MMA_OFFSETS_DIR") {
        let p = PathBuf::from(offsets_dir).join("stable").join("stable__x86.json");
        if p.exists() {
            if let Ok(bytes) = std::fs::read(&p) {
                if let Ok(table) = StableTable::load(&bytes) {
                    return Ok(table);
                }
            }
        }
    }

    // 3. APPDATA 缓存目录
    if let Ok(appdata) = std::env::var("APPDATA") {
        let p = PathBuf::from(appdata)
            .join("ManiaMapAnalyser")
            .join("offsets")
            .join("stable")
            .join("stable__x86.json");
        if p.exists() {
            if let Ok(bytes) = std::fs::read(&p) {
                if let Ok(table) = StableTable::load(&bytes) {
                    return Ok(table);
                }
            }
        }
    }

    // 4. dir_hint 目录（如 exe 同级目录）
    if let Some(hint) = dir_hint {
        let candidates = [
            hint.join("offsets").join("stable").join("stable__x86.json"),
            hint.join("offset").join("stable").join("stable__x86.json"),
        ];
        for p in &candidates {
            if p.exists() {
                if let Ok(bytes) = std::fs::read(p) {
                    if let Ok(table) = StableTable::load(&bytes) {
                        return Ok(table);
                    }
                }
            }
        }
    }

    // 5. 当前可执行文件同级目录
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            let p = exe_dir.join("offsets").join("stable").join("stable__x86.json");
            if p.exists() {
                if let Ok(bytes) = std::fs::read(&p) {
                    if let Ok(table) = StableTable::load(&bytes) {
                        return Ok(table);
                    }
                }
            }
        }
    }

    // 6. 内置编译期兜底（Zero IO 永不失败）
    Ok(default_stable_table())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_tables_are_valid() {
        let stable = default_stable_table();
        assert!(validate_stable_schema(&stable).is_ok());

        let lazer = default_lazer_table();
        assert!(validate_lazer_schema(&lazer).is_ok());
    }

    #[test]
    fn on_disk_stable_table_loads_and_validates() {
        let path = std::path::Path::new("offsets/stable/stable__x86.json");
        if path.exists() {
            let bytes = std::fs::read(path).expect("read stable__x86.json");
            let table = StableTable::load(&bytes).expect("load stable__x86.json");
            assert_eq!(table.client, "stable");
            assert_eq!(table.arch, "x86");
            assert!(table.anchors.contains_key("statusPtr"));
            assert!(table.anchors.contains_key("baseAddr"));
            assert_eq!(table.state_name(2), Some("play"));
            assert_eq!(table.state_name(5), Some("selectPlay"));
            assert_eq!(table.state_name(7), Some("resultScreen"));
        }
    }

    #[test]
    fn stable_schema_rejects_script_injection() {
        let mut table = default_stable_table();
        table.client = "<script>alert(1)</script>".to_string();
        assert!(matches!(
            validate_stable_schema(&table),
            Err(ValidationError::ExecutableContent(_))
        ));

        let mut table2 = default_stable_table();
        table2.evidence = "normal text with javascript:evil() inside".to_string();
        assert!(matches!(
            validate_stable_schema(&table2),
            Err(ValidationError::ExecutableContent(_))
        ));

        let mut table3 = default_stable_table();
        table3.version = "eval(foo)".to_string();
        assert!(matches!(
            validate_stable_schema(&table3),
            Err(ValidationError::ExecutableContent(_))
        ));
    }

    #[test]
    fn stable_schema_rejects_out_of_range_displacement() {
        let mut table = default_stable_table();
        table.topology.beatmap_from_base = 2_000_000;
        assert!(matches!(
            validate_stable_schema(&table),
            Err(ValidationError::DisplacementOutOfRange(..))
        ));

        let mut table2 = default_stable_table();
        table2.topology.hits_candidate_offsets[0] = 5_000_000;
        assert!(matches!(
            validate_stable_schema(&table2),
            Err(ValidationError::DisplacementOutOfRange(..))
        ));
    }

    #[test]
    fn stable_schema_rejects_unaligned_hits_offset() {
        let mut table = default_stable_table();
        table.topology.hits_candidate_offsets[0] = 0x89; // odd offset
        assert!(matches!(
            validate_stable_schema(&table),
            Err(ValidationError::InvalidAlignment(..))
        ));
    }

    #[test]
    fn stable_schema_rejects_hop_depth_exceeded() {
        let mut table = default_stable_table();
        for i in 0..20 {
            table.anchors.insert(
                format!("extra_anchor_{i}"),
                StableAnchorDef {
                    pattern: "90 90".to_string(),
                    offset: 0,
                    derivation: String::new(),
                    evidence: String::new(),
                },
            );
        }
        assert!(matches!(
            validate_stable_schema(&table),
            Err(ValidationError::HopDepthExceeded(_))
        ));
    }

    #[test]
    fn stable_schema_rejects_unknown_state_names() {
        let mut table = default_stable_table();
        table
            .mappings
            .states
            .insert("99".to_string(), "malicious_state".to_string());
        assert!(matches!(
            validate_stable_schema(&table),
            Err(ValidationError::UnknownStateName(_))
        ));
    }

    #[test]
    fn lazer_schema_rejects_out_of_range_displacement() {
        let mut table = default_lazer_table();
        let mut fields = std::collections::BTreeMap::new();
        fields.insert("evil_field".to_string(), 100_000_000);
        table.types.insert("Some.Type".to_string(), fields);
        assert!(matches!(
            validate_lazer_schema(&table),
            Err(ValidationError::DisplacementOutOfRange(..))
        ));
    }

    #[test]
    fn find_stable_table_returns_valid_table() {
        let table = find_stable_table(None).expect("must find table or compile-time fallback");
        assert_eq!(table.client, "stable");
        assert!(validate_stable_schema(&table).is_ok());
    }
}

