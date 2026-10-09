// osu::invariants::gate - 门禁状态机、字段冻结与瞬态身份保持

use std::time::Duration;

use crate::osu::keys;
use crate::osu::model::{Hits, Reason, Snapshot};
use super::rules::{
    identity_corroborated, is_holdable_failure, FrameVerdict, IDENTITY_HOLD_GRACE,
};

pub const INVARIANT_STRIKES: u32 = 3;
pub const RECOVERY_CLEAN_FRAMES: u32 = 20;
pub const MIN_DWELL: Duration = Duration::from_secs(10);
pub const FREEZE_WINDOW: Duration = Duration::from_secs(30);
pub const BACKOFF_SECONDS: &[u64] = &[2, 4, 8, 16, 30];

/// 四态（§3.4）：`idle`（没目标）/`healthy`/`degraded`（出帧 + 上报缺字段）/`unhealthy`（不出帧）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HealthState {
    Idle,
    Healthy,
    Degraded,
    Unhealthy,
}

impl HealthState {
    pub fn as_str(&self) -> &'static str {
        match self {
            HealthState::Idle => "idle",
            HealthState::Healthy => "healthy",
            HealthState::Degraded => "degraded",
            HealthState::Unhealthy => "unhealthy",
        }
    }
}

/// 本帧的动作（调用方照此处理载荷）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FrameAction {
    #[default]
    Publish,
    FreezeStateOnly,
    HoldLastGood,
    Stop,
}

impl FrameAction {
    pub fn is_publishable(&self) -> bool {
        matches!(self, FrameAction::Publish | FrameAction::HoldLastGood)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FrameOutcome {
    pub action: FrameAction,
    pub state: Option<HealthState>,
    pub reason: Option<Reason>,
    pub degraded_fields: Vec<String>,
    pub transition: Option<Reason>,
}

pub struct Gate {
    pub state: HealthState,
    pub strikes: u32,
    pub clean_frames: u32,
    pub frozen: bool,
    pub frozen_since_ms: Option<u64>,
    pub freeze_reason: Option<Reason>,
    pub degraded: Vec<String>,
    pub last_reason: Option<Reason>,
    pub holding: bool,
    pub hold_since_ms: Option<u64>,
    pub held_identity: Option<String>,
}

impl Default for Gate {
    fn default() -> Self {
        Self {
            state: HealthState::Idle,
            strikes: 0,
            clean_frames: 0,
            frozen: false,
            frozen_since_ms: None,
            freeze_reason: None,
            degraded: Vec::new(),
            last_reason: None,
            holding: false,
            hold_since_ms: None,
            held_identity: None,
        }
    }
}

impl Gate {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn state(&self) -> HealthState {
        self.state
    }

    pub fn reason(&self) -> Option<&Reason> {
        self.last_reason.as_ref()
    }

    pub fn degraded_fields(&self) -> &[String] {
        &self.degraded
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    pub fn is_holding(&self) -> bool {
        self.holding
    }

    pub fn hold_window_open(&self, now_ms: u64) -> bool {
        match self.hold_since_ms {
            Some(at) => now_ms.saturating_sub(at) < IDENTITY_HOLD_GRACE.as_millis() as u64,
            None => false,
        }
    }

    pub fn holding_active(&self) -> bool {
        self.holding
    }

    pub fn clear_hold(&mut self) {
        self.holding = false;
        self.hold_since_ms = None;
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn on_l0_fail(&mut self, reason: Reason) -> FrameOutcome {
        let previous = self.state;
        self.state = HealthState::Unhealthy;
        self.last_reason = Some(reason.clone());
        self.degraded.clear();
        self.frozen = false;
        self.frozen_since_ms = None;
        self.freeze_reason = None;
        self.strikes = 0;
        self.clean_frames = 0;
        self.clear_hold();
        let transition = (previous != HealthState::Unhealthy).then_some(reason);
        FrameOutcome {
            action: FrameAction::Stop,
            state: Some(HealthState::Unhealthy),
            reason: self.last_reason.clone(),
            degraded_fields: Vec::new(),
            transition,
        }
    }

    pub fn process(
        &mut self,
        snapshot: &Snapshot,
        verdict: FrameVerdict,
        now_ms: u64,
        identity: String,
    ) -> FrameOutcome {
        let previous = self.state;
        if verdict.is_hard_failure() {
            let reason = verdict.to_reason().unwrap_or(Reason::Unhealthy);
            self.strikes = self.strikes.saturating_add(1);
            self.clean_frames = 0;
            self.degraded = verdict.degraded_fields.clone();
            self.last_reason = Some(reason.clone());
            if !self.frozen {
                self.frozen = true;
                self.frozen_since_ms = Some(now_ms);
                self.freeze_reason = Some(reason.clone());
            }

            if is_holdable_failure(&verdict) {
                if self.hold_since_ms.is_none() {
                    self.hold_since_ms = Some(now_ms);
                }
                if self.hold_window_open(now_ms) {
                    self.holding = true;
                    self.state = HealthState::Degraded;
                    let transition = (previous != HealthState::Degraded).then(|| reason.clone());
                    return FrameOutcome {
                        action: FrameAction::HoldLastGood,
                        state: Some(self.state),
                        reason: Some(reason),
                        degraded_fields: verdict.degraded_fields,
                        transition,
                    };
                }
                self.clear_hold();
            } else {
                self.clear_hold();
            }
            self.state = if self.strikes >= INVARIANT_STRIKES {
                HealthState::Unhealthy
            } else {
                HealthState::Degraded
            };
            let transition = (previous != self.state).then(|| reason.clone());
            let action = if self.state == HealthState::Unhealthy {
                FrameAction::Stop
            } else {
                FrameAction::FreezeStateOnly
            };
            return FrameOutcome {
                action,
                state: Some(self.state),
                reason: Some(reason),
                degraded_fields: verdict.degraded_fields,
                transition,
            };
        }

        self.clean_frames += 1;
        self.degraded = verdict.degraded_fields.clone();
        let previous = self.state;

        if self.holding_active() && self.clean_frames < RECOVERY_CLEAN_FRAMES {
            let corroborated_change = !identity.is_empty()
                && identity_corroborated(snapshot)
                && self
                    .held_identity
                    .as_deref()
                    .map_or(false, |held| held != identity);
            if corroborated_change {
                self.clear_hold();
                self.held_identity = Some(identity.clone());
                self.frozen = false;
                self.frozen_since_ms = None;
                self.freeze_reason = None;
                self.last_reason = None;
                self.strikes = 0;
                self.clean_frames = 0;
            } else {
                self.holding = true;
                self.state = HealthState::Degraded;
                let transition = match previous {
                    HealthState::Degraded | HealthState::Healthy | HealthState::Idle => None,
                    _ => self.freeze_reason.clone(),
                };
                return FrameOutcome {
                    action: FrameAction::HoldLastGood,
                    state: Some(self.state),
                    reason: self.last_reason.clone(),
                    degraded_fields: verdict.degraded_fields,
                    transition,
                };
            }
        }

        let (action, state) = if self.frozen {
            if self.clean_frames >= RECOVERY_CLEAN_FRAMES {
                self.frozen = false;
                self.frozen_since_ms = None;
                self.freeze_reason = None;
                self.strikes = 0;
                self.clear_hold();
                let state = if verdict.soft.is_empty() {
                    HealthState::Healthy
                } else {
                    HealthState::Degraded
                };
                self.last_reason = None;
                (FrameAction::Publish, state)
            } else {
                (FrameAction::FreezeStateOnly, HealthState::Degraded)
            }
        } else {
            let state = if verdict.soft.is_empty() {
                HealthState::Healthy
            } else {
                HealthState::Degraded
            };
            self.last_reason = None;
            self.clear_hold();
            (FrameAction::Publish, state)
        };
        self.state = state;
        if action == FrameAction::Publish && !identity.is_empty() {
            self.held_identity = Some(identity);
        }
        let transition = match (previous, state) {
            (HealthState::Healthy | HealthState::Idle, HealthState::Degraded) => None,
            (_, HealthState::Degraded) => self.freeze_reason.clone(),
            (_, HealthState::Healthy) => None,
            (_, HealthState::Idle) => None,
            (_, HealthState::Unhealthy) => self.last_reason.clone(),
        };
        FrameOutcome {
            action,
            state: Some(state),
            reason: self.last_reason.clone(),
            degraded_fields: verdict.degraded_fields,
            transition,
        }
    }

    pub fn should_re_resolve(&self, now_ms: u64) -> bool {
        matches!(self.frozen_since_ms, Some(at) if now_ms.saturating_sub(at) >= FREEZE_WINDOW.as_millis() as u64)
    }

    pub fn re_resolve_due_in_ms(&self, now_ms: u64) -> Option<u64> {
        let at = self.frozen_since_ms?;
        Some((FREEZE_WINDOW.as_millis() as u64).saturating_sub(now_ms.saturating_sub(at)))
    }
}

pub fn backoff(attempt: usize) -> Duration {
    let seconds = BACKOFF_SECONDS[attempt.min(BACKOFF_SECONDS.len() - 1)];
    Duration::from_secs(seconds)
}

pub fn mask_for_state(snapshot: &Snapshot) -> (Option<u32>, Option<u32>, Option<u32>) {
    let play = if (snapshot.is_play_state() || snapshot.is_result_state())
        && super::rules::play_chain_valid(snapshot)
    {
        snapshot.play_mods_mask
    } else {
        None
    };
    let result = if snapshot.is_result_state() && super::rules::result_chain_valid(snapshot) {
        snapshot.result_mods_mask
    } else {
        None
    };
    (snapshot.menu_mods_mask, play, result)
}

pub fn page_mod_codes(snapshot: &Snapshot) -> Vec<&'static str> {
    keys::mod_codes_from_payload(
        &snapshot.to_packet(),
        snapshot.client.unwrap_or(crate::osu::model::Client::Stable),
    )
}

pub fn zero_hits() -> Hits {
    Hits::default()
}
