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

use std::collections::BTreeMap;

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
    /// 验证过的运行时构建标识（表自证的一部分）。
    pub verified_build: String,
    /// **验证方法**（不是文件路径）：这个偏移是怎么提取的、怎么证明它对。
    pub evidence: String,
}

/// JSON 根键名（错误消息与测试都引用它，避免两处各写一遍字面量）。
pub const TABLE_KEYS: &[&str] = &[
    "lazer_version",
    "runtime_version",
    "arch",
    "game_base_vtable",
    "types",
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

