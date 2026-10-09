// osu::invariants - 读数不变量、健康状态机与字段冻结门禁门面

pub mod gate;
pub mod rules;

pub use gate::*;
pub use rules::*;

#[cfg(test)]
#[path = "../../tests-local/osu_invariants.rs"]
mod tests_invariants;
