// osu!lazer 的读取路径门面模块
//
// 子模块职责划分：
// - fields: 字段名、结构常量、FieldLookup、降级标记
// - source: 内存抽象（Source trait）及基元读取
// - types: 链状态、证明、诊断、FrameInput/LazerFrame
// - screen: EEType 解析与屏幕状态映射
// - resolve: 站点到 GameBase 多跳解析与 L1 结构证明
// - table: 偏移表查找梯子与加载逻辑
// - frame: 逐帧读取、字段解引用、L0 attach 与 read_tick

pub mod fields;
pub mod source;
pub mod types;
pub mod screen;
pub mod resolve;
pub mod table;
pub mod frame;

pub use fields::*;
pub use source::*;
pub use types::*;
pub use screen::*;
pub use resolve::*;
pub use table::*;
pub use frame::*;

#[cfg(test)]
#[path = "../../tests-local/osu_lazer.rs"]
mod tests_lazer;
