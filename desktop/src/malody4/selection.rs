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
mod tests {
    use super::*;
    use crate::malody4::TICK;
    use std::path::PathBuf;

    fn mk_entry(path: &str, md5: &str) -> LibraryEntry {
        LibraryEntry {
            path: PathBuf::from(path),
            slot: 0,
            md5: md5.to_string(),
        }
    }

    fn mk_key(md5: &str) -> IdentityKey {
        IdentityKey {
            md5: md5.to_string(),
            slot: 7,
        }
    }

    /// 通道在线 + 锚点命中 + 索引命中。
    fn tick(
        state: &mut SelectionState,
        entry: &LibraryEntry,
        screen: Screen,
        rate: f64,
        now: Instant,
    ) -> Action {
        state.fold(
            Some(mk_key(&entry.md5)),
            Some(entry),
            screen,
            "Hard",
            rate,
            Availability::Ready,
            now,
        )
    }

    fn emitted(action: Action) -> Selection {
        match action {
            Action::Emit(selection) => selection,
            other => panic!("expected Emit, got {other:?}"),
        }
    }

    /// 先让一个签名出现（`t0`，只进防抖），再在 `t0 + DEBOUNCE` 取出发出的帧。
    fn prime(
        state: &mut SelectionState,
        entry: &LibraryEntry,
        screen: Screen,
        rate: f64,
        t0: Instant,
    ) -> (Selection, Instant) {
        assert_eq!(tick(state, entry, screen, rate, t0), Action::None);
        let emit_at = t0 + DEBOUNCE;
        (emitted(tick(state, entry, screen, rate, emit_at)), emit_at)
    }

    fn outage(state: &mut SelectionState, reason: UnavailableReason, now: Instant) -> Action {
        state.fold(
            None,
            None,
            Screen::Other,
            "",
            1.0,
            Availability::Unavailable(reason),
            now,
        )
    }

    #[test]
    fn first_appearance_emits_only_after_debounce() {
        let entry = mk_entry("Maps/x/0/y.mc", "ab");
        let mut state = SelectionState::new();
        let t0 = Instant::now();

        assert_eq!(tick(&mut state, &entry, Screen::Selection, 1.2, t0), Action::None);
        assert_eq!(
            tick(&mut state, &entry, Screen::Selection, 1.2, t0 + TICK),
            Action::None,
            "200ms < DEBOUNCE：不发"
        );
        let selection = emitted(tick(&mut state, &entry, Screen::Selection, 1.2, t0 + 2 * TICK));
        assert_eq!(selection.event, "anchor-changed");
        assert_eq!(selection.path, "Maps/x/0/y.mc");
        assert_eq!(selection.speed_rate, 1.2);
        assert_eq!(selection.screen, "selection");
        assert_eq!(selection.version, "Hard");
        assert_eq!(selection.chart_hash, "ab");
        assert_eq!(selection.source, "malody4-native");
        assert_eq!(selection.sequence, 1);
        assert_eq!(state.sequence(), 1);
        assert_eq!(state.last_version(), Some("Hard"));
    }

    #[test]
    fn debounce_boundary_emits_at_exactly_debounce() {
        let entry = mk_entry("Maps/x/0/y.mc", "ab");
        let mut state = SelectionState::new();
        let t0 = Instant::now();
        assert_eq!(tick(&mut state, &entry, Screen::Selection, 1.0, t0), Action::None);
        assert_eq!(
            tick(
                &mut state,
                &entry,
                Screen::Selection,
                1.0,
                t0 + DEBOUNCE - Duration::from_millis(1)
            ),
            Action::None
        );
        let selection = emitted(tick(&mut state, &entry, Screen::Selection, 1.0, t0 + DEBOUNCE));
        assert_eq!(selection.event, "anchor-changed");
    }

    #[test]
    fn unchanged_key_emits_heartbeat_only_at_the_boundary() {
        let entry = mk_entry("Maps/x/0/y.mc", "ab");
        let mut state = SelectionState::new();
        let t0 = Instant::now();
        let (first, first_emit_at) = prime(&mut state, &entry, Screen::Selection, 1.0, t0);
        assert_eq!(first.event, "anchor-changed");

        assert_eq!(
            tick(&mut state, &entry, Screen::Selection, 1.0, first_emit_at + TICK),
            Action::None,
            "同键 200ms 内不发"
        );
        assert_eq!(
            tick(
                &mut state,
                &entry,
                Screen::Selection,
                1.0,
                first_emit_at + HEARTBEAT - Duration::from_millis(1)
            ),
            Action::None
        );
        let heartbeat = emitted(tick(
            &mut state,
            &entry,
            Screen::Selection,
            1.0,
            first_emit_at + HEARTBEAT,
        ));
        assert_eq!(heartbeat.event, "heartbeat");
        assert_eq!(heartbeat.sequence, 2);
        assert_eq!(heartbeat.path, "Maps/x/0/y.mc");
        assert_eq!(heartbeat.screen, "selection");
        assert_eq!(heartbeat.chart_hash, "ab");
        assert_eq!(heartbeat.source, "malody4-native");
    }

    #[test]
    fn unchanged_path_with_new_screen_emits_scene_changed() {
        let entry = mk_entry("Maps/x/0/y.mc", "ab");
        let mut state = SelectionState::new();
        let t0 = Instant::now();
        let (_, first_emit_at) = prime(&mut state, &entry, Screen::Selection, 1.0, t0);

        let scene_change = first_emit_at + TICK;
        assert_eq!(
            tick(&mut state, &entry, Screen::Playing, 1.0, scene_change),
            Action::None
        );
        let selection = emitted(tick(
            &mut state,
            &entry,
            Screen::Playing,
            1.0,
            scene_change + DEBOUNCE,
        ));
        assert_eq!(selection.event, "scene-changed");
        assert_eq!(selection.screen, "playing");
        assert_eq!(selection.path, "Maps/x/0/y.mc");
        assert_eq!(selection.sequence, 2);
    }

    #[test]
    fn changed_key_debounces_again_and_never_emits_the_old_value() {
        let first_entry = mk_entry("Maps/a/0/a.mc", "aa");
        let second_entry = mk_entry("Maps/b/0/b.mc", "bb");
        let mut state = SelectionState::new();
        let t0 = Instant::now();
        let (_, first_emit_at) = prime(&mut state, &first_entry, Screen::Selection, 1.0, t0);

        let changed = first_emit_at + TICK;
        assert_eq!(
            tick(&mut state, &second_entry, Screen::Selection, 1.0, changed),
            Action::None,
            "键变后重新防抖：这一刻不发任何东西（尤其不发旧值）"
        );
        assert_eq!(
            tick(
                &mut state,
                &second_entry,
                Screen::Selection,
                1.0,
                changed + DEBOUNCE - Duration::from_millis(1)
            ),
            Action::None
        );
        // 防抖期间切回已发出的旧值：不得再发（等心跳边界即可）
        assert_eq!(
            tick(&mut state, &first_entry, Screen::Selection, 1.0, changed + DEBOUNCE),
            Action::None
        );
        // 再次切到 B：重新计时
        let again = changed + DEBOUNCE + TICK;
        assert_eq!(
            tick(&mut state, &second_entry, Screen::Selection, 1.0, again),
            Action::None
        );
        let selection = emitted(tick(
            &mut state,
            &second_entry,
            Screen::Selection,
            1.0,
            again + DEBOUNCE,
        ));
        assert_eq!(selection.path, "Maps/b/0/b.mc");
        assert_eq!(selection.event, "anchor-changed");
        assert_eq!(selection.sequence, 2);
    }

    #[test]
    fn rate_epsilon_decides_same_or_changed_key() {
        let entry = mk_entry("Maps/x/0/y.mc", "ab");
        let mut state = SelectionState::new();
        let t0 = Instant::now();
        let (_, first_emit_at) = prime(&mut state, &entry, Screen::Selection, 1.0, t0);

        // |Δ| = 1e-6 < RATE_EPS：同一速率 → 200ms 内不发；到 2s 边界发心跳
        assert_eq!(
            tick(
                &mut state,
                &entry,
                Screen::Selection,
                1.0 + 1e-6,
                first_emit_at + TICK
            ),
            Action::None
        );
        let heartbeat = emitted(tick(
            &mut state,
            &entry,
            Screen::Selection,
            1.0 + 1e-6,
            first_emit_at + HEARTBEAT,
        ));
        assert_eq!(heartbeat.event, "heartbeat");
        assert!((heartbeat.speed_rate - 1.0).abs() < RATE_EPS);

        // |Δ| = 1e-2 ≥ RATE_EPS：变键 → 重新防抖后发 anchor-changed
        let changed = first_emit_at + HEARTBEAT + TICK;
        assert_eq!(
            tick(&mut state, &entry, Screen::Selection, 1.0 + 1e-2, changed),
            Action::None
        );
        let selection = emitted(tick(
            &mut state,
            &entry,
            Screen::Selection,
            1.0 + 1e-2,
            changed + DEBOUNCE,
        ));
        assert_eq!(selection.event, "anchor-changed");
        assert!((selection.speed_rate - 1.01).abs() < 1e-9);
        assert_eq!(selection.sequence, 3);
    }

    #[test]
    fn becoming_unavailable_emits_hidden_immediately_with_empty_fields() {
        let entry = mk_entry("Maps/x/0/y.mc", "ab");
        let mut state = SelectionState::new();
        let t0 = Instant::now();
        let (_, first_emit_at) = prime(&mut state, &entry, Screen::Selection, 1.2, t0);

        let outage_at = first_emit_at + TICK;
        assert_eq!(
            outage(&mut state, UnavailableReason::ProcessNotFound, outage_at),
            Action::Hidden(UnavailableReason::ProcessNotFound),
            "首次转不可用：零延迟立即 hidden"
        );
        assert_eq!(state.sequence(), 2);
        assert_eq!(state.last_version(), None, "不可用态不得保留残值");

        let record = Selection::hidden(state.sequence(), "hidden");
        assert_eq!(record.path, "");
        assert_eq!(record.version, "");
        assert_eq!(record.chart_hash, "");
        assert_eq!(record.screen, "other");
        assert_eq!(record.speed_rate, 1.0);
        assert_eq!(record.event, "hidden");
        assert_eq!(record.sequence, 2);
        assert_eq!(record.source, "malody4-native");

        // 持续不可用：200ms 内不重复
        assert_eq!(
            outage(&mut state, UnavailableReason::ProcessNotFound, outage_at + TICK),
            Action::None
        );
        assert_eq!(
            outage(
                &mut state,
                UnavailableReason::ProcessNotFound,
                outage_at + HEARTBEAT - Duration::from_millis(1)
            ),
            Action::None
        );
        // 只在 2s 边界重发
        assert_eq!(
            outage(&mut state, UnavailableReason::ProcessNotFound, outage_at + HEARTBEAT),
            Action::Hidden(UnavailableReason::ProcessNotFound)
        );
        assert_eq!(state.sequence(), 3);
    }

    #[test]
    fn entry_missing_is_chart_not_indexed_and_key_missing_is_no_selection() {
        let entry = mk_entry("Maps/x/0/y.mc", "ab");
        let mut state = SelectionState::new();
        let t0 = Instant::now();
        assert_eq!(
            state.fold(
                Some(mk_key("ab")),
                None,
                Screen::Selection,
                "Hard",
                1.0,
                Availability::Ready,
                t0
            ),
            Action::Hidden(UnavailableReason::ChartNotIndexed)
        );

        let mut other_state = SelectionState::new();
        assert_eq!(
            other_state.fold(
                None,
                Some(&entry),
                Screen::Selection,
                "Hard",
                1.0,
                Availability::Ready,
                t0
            ),
            Action::Hidden(UnavailableReason::NoSelection)
        );
    }

    #[test]
    fn hidden_then_reappearing_emits_anchor_changed_with_new_rate() {
        let entry = mk_entry("Maps/x/0/y.mc", "ab");
        let mut state = SelectionState::new();
        let t0 = Instant::now();
        assert_eq!(
            state.fold(
                Some(mk_key("ab")),
                None,
                Screen::Other,
                "",
                1.0,
                Availability::Ready,
                t0
            ),
            Action::Hidden(UnavailableReason::ChartNotIndexed)
        );

        // 重新出现：速率取新值，且必须重新防抖
        assert_eq!(
            tick(&mut state, &entry, Screen::Selection, 1.5, t0 + TICK),
            Action::None
        );
        let selection = emitted(tick(
            &mut state,
            &entry,
            Screen::Selection,
            1.5,
            t0 + TICK + DEBOUNCE,
        ));
        assert_eq!(selection.event, "anchor-changed");
        assert_eq!(selection.speed_rate, 1.5);
        assert_eq!(selection.path, "Maps/x/0/y.mc");
        assert_eq!(selection.sequence, 2);
    }

    #[test]
    fn different_unavailable_causes_share_the_hidden_event() {
        let t0 = Instant::now();
        let causes = [
            UnavailableReason::NoSelection,
            UnavailableReason::ChartNotIndexed,
            UnavailableReason::ProcessNotFound,
            UnavailableReason::MultipleInstances,
            UnavailableReason::AccessDenied,
            UnavailableReason::BadRead,
            UnavailableReason::TargetMismatch("pe_timestamp_mismatch"),
            UnavailableReason::TargetMismatch("file_size_mismatch"),
            UnavailableReason::TargetMismatch("pe_header_out_of_range"),
            UnavailableReason::TargetMismatch("other"),
            UnavailableReason::RootNotConfigured,
            UnavailableReason::NoLibrary,
            UnavailableReason::PlatformUnsupported,
        ];
        let mut seen_strings: Vec<String> = Vec::new();
        for cause in causes {
            let mut state = SelectionState::new();
            let action = outage(&mut state, cause.clone(), t0);
            match action {
                Action::Hidden(reason) => {
                    assert_eq!(reason, cause);
                    // 线上记录与成因无关：event 恒为 "hidden"，只有原因不同
                    assert_eq!(Selection::hidden(state.sequence(), "hidden").event, "hidden");
                }
                other => panic!("expected Hidden, got {other:?}"),
            }
            assert_eq!(state.sequence(), 1);
            seen_strings.push(cause.as_str());
        }
        assert_eq!(seen_strings.len(), 13);
        let distinct: std::collections::HashSet<&String> = seen_strings.iter().collect();
        assert_eq!(distinct.len(), 13, "13 个成因的原因串两两不同");
        assert!(seen_strings.contains(&String::new()), "NoSelection 的原因串为空");
        assert!(seen_strings.contains(&"target-mismatch:unknown".to_string()));
    }

    #[test]
    fn ten_consecutive_unavailable_ticks_emit_hidden_only_once() {
        // §7：同一 hidden 连续 10 次 tick 只发一次（去重）。10 × 200ms = 1.8s < HEARTBEAT(2s)。
        let mut state = SelectionState::new();
        let t0 = Instant::now();
        let mut hidden = 0;
        for step in 0u32..10 {
            let action = outage(
                &mut state,
                UnavailableReason::ProcessNotFound,
                t0 + TICK * step,
            );
            match action {
                Action::Hidden(reason) => {
                    assert_eq!(reason, UnavailableReason::ProcessNotFound);
                    hidden += 1;
                }
                Action::None => {}
                other => panic!("不可用态只应产生 Hidden/None，实际 {other:?}"),
            }
        }
        assert_eq!(hidden, 1, "10 个 tick 只发一条 hidden");
        assert_eq!(state.sequence(), 1);
        // 跨过 2s 边界后才是下一条
        assert_eq!(
            outage(&mut state, UnavailableReason::ProcessNotFound, t0 + HEARTBEAT),
            Action::Hidden(UnavailableReason::ProcessNotFound)
        );
        assert_eq!(state.sequence(), 2);
    }

    #[test]
    fn unavailable_state_never_leaks_residual_values() {
        let entry = mk_entry("Maps/x/0/y.mc", "ab");
        let mut state = SelectionState::new();
        let t0 = Instant::now();
        let (playing, first_emit_at) = prime(&mut state, &entry, Screen::Playing, 1.5, t0);
        assert_eq!(playing.screen, "playing");

        let outage_at = first_emit_at + TICK;
        assert_eq!(
            outage(&mut state, UnavailableReason::BadRead, outage_at),
            Action::Hidden(UnavailableReason::BadRead)
        );
        // 立刻恢复：残值已清空 → 当作全新锚点（anchor-changed），速率取新值
        assert_eq!(
            tick(&mut state, &entry, Screen::Selection, 0.8, outage_at + TICK),
            Action::None
        );
        let selection = emitted(tick(
            &mut state,
            &entry,
            Screen::Selection,
            0.8,
            outage_at + TICK + DEBOUNCE,
        ));
        assert_eq!(selection.event, "anchor-changed");
        assert_eq!(selection.speed_rate, 0.8);
        assert_eq!(selection.screen, "selection");
        assert_eq!(selection.version, "Hard");
        assert_eq!(selection.sequence, 3);
        assert_eq!(state.last_version(), Some("Hard"));
    }
}
