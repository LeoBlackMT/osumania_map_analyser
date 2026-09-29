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

/// 一张偏移表（一个 `(lazer 版本, runtime 版本, 架构)` 组合一份）。
///
/// 字段顺序/命名与生成器的 JSON 同形，`load` 直接反序列化。
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
pub struct OffsetTable {
    /// lazer 版本（`sq.version` / runtime log 里的 `Running osu <ver>`）。
    pub lazer_version: String,
    /// .NET runtime 版本（`Running osu <lazer> on .NET <runtime>`）。
    pub runtime_version: String,
    /// 架构（`x64` / `x86`；**位数分派**与表键都靠它）。
    pub arch: String,
    /// `GameBase` 的 MethodTable（vtable）。`None` = 本表未提取到（按字段降级）。
    pub game_base_vtable: Option<u64>,
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
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
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
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Deserialize)]
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
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Json(message) => write!(f, "offsets-load: {message}"),
            LoadError::EmptyField(field) => write!(f, "offsets-load: empty {field}"),
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
        Ok(table)
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

#[cfg(test)]
#[path = "../../tests-local/osu_offsets.rs"]
mod tests_offsets;

