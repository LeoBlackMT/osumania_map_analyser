// Malody 4.3.7 selection 状态机：纯逻辑，不持有任何阈值。
//
// 输入：锚点身份键（调用方查索引后给出 entry）+ 场景 / 难度名 / 速率 + 通道可用性；
// 输出：`Action::{None, Hidden(原因), Emit(Selection)}`。
//
// 阈值全部来自 `crate::malody4::{DEBOUNCE, HEARTBEAT, RATE_EPS}`（集中在 mod.rs），
// 本文件只持有状态：上次发出的签名与难度名、防抖中的签名与起始时刻、上次发出时刻、帧序号。

use crate::malody4::anchor::IdentityKey;
use crate::malody4::library::LibraryEntry;
use crate::malody4::model::{Screen, Selection, UnavailableReason, SOURCE_ID};
use crate::malody4::{DEBOUNCE, HEARTBEAT, RATE_EPS};
use std::time::{Duration, Instant};

/// 通道可用性。调用方把"整条通道为何不可用"的原因传进来；
/// `Ready` 表示进程 / 内存 / 索引都在线，此时 `key` 或 `entry` 为空由状态机自行判定原因
/// （没选中谱面 → `NoSelection`；索引里没有这张谱 → `ChartNotIndexed`）。
#[derive(Debug, Clone, PartialEq)]
pub enum Availability {
    Ready,
    Unavailable(UnavailableReason),
}

/// 一帧的输出。
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    None,
    Hidden(UnavailableReason),
    Emit(Selection),
}

/// 上一次发出的 `(path, rate, screen)`。
type Signature = (String, f64, Screen);

/// 本帧的解析结果。
enum Resolved<'a> {
    Ready(&'a LibraryEntry),
    Blocked(UnavailableReason),
}

fn resolve<'a>(
    key: &Option<IdentityKey>,
    entry: Option<&'a LibraryEntry>,
    availability: &Availability,
) -> Resolved<'a> {
    match availability {
        Availability::Unavailable(reason) => Resolved::Blocked(reason.clone()),
        Availability::Ready => match (key.is_some(), entry) {
            (true, Some(entry)) => Resolved::Ready(entry),
            (true, None) => Resolved::Blocked(UnavailableReason::ChartNotIndexed),
            (false, _) => Resolved::Blocked(UnavailableReason::NoSelection),
        },
    }
}

/// `rate` 用 `|Δ| < RATE_EPS` 判"同一速率"。
fn same_signature(a: &Signature, b: &Signature) -> bool {
    a.0 == b.0 && a.2 == b.2 && (a.1 - b.1).abs() < RATE_EPS
}

#[derive(Debug, Default)]
pub struct SelectionState {
    /// 上次发出的签名（转不可用时清空，绝不保留残值）。
    last_signature: Option<Signature>,
    /// 上次发出的难度名（转不可用时同样清空）。
    last_version: Option<String>,
    /// 防抖中的候选签名与起始时刻。
    pending_signature: Option<Signature>,
    pending_since: Option<Instant>,
    last_emit_at: Option<Instant>,
    sequence: i64,
    /// 当前是否处于不可用态（"首次转不可用立即发"的判据）。
    unavailable: bool,
}

impl SelectionState {
    pub fn new() -> Self {
        SelectionState::default()
    }

    /// 已发出的帧数（`Action::Hidden` 只带原因，调用方据此组装 `Selection::hidden` 帧）。
    pub fn sequence(&self) -> i64 {
        self.sequence
    }

    /// 上次发出的难度名（诊断用；未发出过或已转不可用时为 `None`）。
    pub fn last_version(&self) -> Option<&str> {
        self.last_version.as_deref()
    }

    /// 推进一帧。
    ///
    /// - 不可用：首次转不可用立即发 `Hidden`（零延迟、无防抖），其后按 `HEARTBEAT` 重发同一条；
    /// - 可用且 `(path, rate, screen)` 与上次发出的不同：防抖 `DEBOUNCE` 后发 `Emit`；
    /// - 可用且签名相同：到 `HEARTBEAT` 边界发心跳，否则 `None`。
    pub fn fold(
        &mut self,
        key: Option<IdentityKey>,
        entry: Option<&LibraryEntry>,
        screen: Screen,
        version: &str,
        rate: f64,
        availability: Availability,
        now: Instant,
    ) -> Action {
        match resolve(&key, entry, &availability) {
            Resolved::Blocked(reason) => self.unavailable_tick(reason, now),
            Resolved::Ready(entry) => self.available_tick(entry, screen, version, rate, now),
        }
    }

    fn due(&self, now: Instant, window: Duration) -> bool {
        match self.last_emit_at {
            Some(last) => now.duration_since(last) >= window,
            None => true,
        }
    }

    fn unavailable_tick(&mut self, reason: UnavailableReason, now: Instant) -> Action {
        if !self.unavailable {
            // 首次转不可用：零延迟，不防抖，并清空全部残值
            self.unavailable = true;
            return self.emit_hidden(reason, now);
        }
        // 持续不可用：只在 HEARTBEAT 边界重发同一条 hidden，不每 tick 刷帧
        if self.due(now, HEARTBEAT) {
            return self.emit_hidden(reason, now);
        }
        Action::None
    }

    fn emit_hidden(&mut self, reason: UnavailableReason, now: Instant) -> Action {
        self.sequence += 1;
        self.last_emit_at = Some(now);
        self.last_signature = None;
        self.last_version = None;
        self.pending_signature = None;
        self.pending_since = None;
        Action::Hidden(reason)
    }

    fn available_tick(
        &mut self,
        entry: &LibraryEntry,
        screen: Screen,
        version: &str,
        rate: f64,
        now: Instant,
    ) -> Action {
        self.unavailable = false;
        let current: Signature = (entry.path.to_string_lossy().to_string(), rate, screen);

        if self
            .last_signature
            .as_ref()
            .is_some_and(|last| same_signature(last, &current))
        {
            // 键未变：到 HEARTBEAT 边界重发当前快照，否则不发
            self.pending_signature = None;
            self.pending_since = None;
            return if self.due(now, HEARTBEAT) {
                self.emit(current, entry, version, "heartbeat", now)
            } else {
                Action::None
            };
        }

        // 键变化：进入防抖；防抖期间再次变化则重新计时（旧值绝不发出）
        if !self
            .pending_signature
            .as_ref()
            .is_some_and(|pending| same_signature(pending, &current))
        {
            self.pending_signature = Some(current.clone());
            self.pending_since = Some(now);
        }
        let since = self.pending_since.unwrap_or(now);
        if now.duration_since(since) < DEBOUNCE {
            return Action::None;
        }

        // 固定映射（页面侧 L2 续约依赖 event != "heartbeat"）
        let event = match self.last_signature.as_ref() {
            None => "anchor-changed",
            Some((last_path, last_rate, last_screen)) => {
                if *last_path != current.0 {
                    "anchor-changed"
                } else if *last_screen != current.2 {
                    "scene-changed"
                } else if (*last_rate - current.1).abs() >= RATE_EPS {
                    "anchor-changed"
                } else {
                    // 三者都不变只可能走心跳分支（上面已处理），此臂保持映射闭合
                    "heartbeat"
                }
            }
        };
        self.emit(current, entry, version, event, now)
    }

    fn emit(
        &mut self,
        signature: Signature,
        entry: &LibraryEntry,
        version: &str,
        event: &str,
        now: Instant,
    ) -> Action {
        self.sequence += 1;
        self.last_emit_at = Some(now);
        self.last_signature = Some(signature.clone());
        self.last_version = Some(version.to_string());
        self.pending_signature = None;
        self.pending_since = None;
        Action::Emit(Selection {
            path: signature.0,
            speed_rate: signature.1,
            screen: signature.2.as_str(),
            sequence: self.sequence,
            event: event.to_string(),
            version: version.to_string(),
            chart_hash: entry.md5.clone(),
            source: SOURCE_ID,
        })
    }
}

#[cfg(test)]
#[path = "../../tests-local/malody4_selection.rs"]
mod tests;
