//! Deterministic P1 reducer. The shell delivers ordered events and executes effects.
//! Frame animation, gaze, blinking, hit testing and movement stay in the host.
pub mod care;
pub mod touch;
use care::{CareAction, Needs};
use pet_protocol::{
    AvatarCapabilities, AvatarCommand, AvatarEvent, BaselineExpression, DesktopCapabilities,
    DesktopCommand, DesktopEvent, Feedback, HitRegion, Permission,
};

/// Adapters enqueue commands without blocking the UI/event loop. An Ok means
/// accepted for delivery, not executed. Delivery failures must reach the shell;
/// runtime readiness/failures return as events. These traits do not implement IPC.
pub trait AvatarPort {
    type Error;
    fn send(&mut self, command: AvatarCommand) -> Result<(), Self::Error>;
}

/// Hide/shutdown must also be callable directly by the shell's emergency path,
/// independently of PetCore. The production adapter must prioritize them.
pub trait DesktopPort {
    type Error;
    fn send(&mut self, command: DesktopCommand) -> Result<(), Self::Error>;
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Activity {
    #[default]
    Idle,
    Dragged,
    Perched,
    Eating,
    Playing,
    Sleeping,
    Greeting,
    Peeking,
    Inviting,
    ScreenPlay,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Intent {
    SetVisible(bool),
    SetExternalSnapEnabled(bool),
    SetCompanionEnabled(bool),
    SetDoNotDisturb(bool),
    SetScreenPlayEnabled(bool),
    StopActivity,
    Quit,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Intent(Intent),
    Avatar(AvatarEvent),
    Desktop(DesktopEvent),
    CareCommitted {
        action: CareAction,
        now_ms: u64,
        needs: Needs,
    },
    Tick {
        now_ms: u64,
        needs: Needs,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    Avatar(AvatarCommand),
    Desktop(DesktopCommand),
}

/// UI projection. Visibility is user intent, not a native-window acknowledgement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct State {
    pub visible: bool,
    pub ready: bool,
    pub stopping: bool,
    pub activity: Activity,
    pub baseline: BaselineExpression,
    pub companion_enabled: bool,
    pub do_not_disturb: bool,
    pub screen_play_enabled: bool,
    pub external_snap_requested: bool,
    pub permission: Permission,
    pub desktop: DesktopCapabilities,
    pub avatar: AvatarCapabilities,
    pub fault: Option<String>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            visible: true,
            ready: false,
            stopping: false,
            activity: Activity::Idle,
            baseline: BaselineExpression::Neutral,
            companion_enabled: false,
            do_not_disturb: false,
            screen_play_enabled: false,
            external_snap_requested: false,
            permission: Permission::Unknown,
            desktop: DesktopCapabilities::default(),
            avatar: AvatarCapabilities::default(),
            fault: None,
        }
    }
}

impl State {
    pub fn external_snap_enabled(&self) -> bool {
        self.external_snap_requested
            && self.permission == Permission::Granted
            && self.desktop.external_window_observation
            && self.desktop.absolute_position
            && self.ready
            && self.visible
            && !self.stopping
    }
}

pub struct PetCore {
    state: State,
    attached: bool,
    activity_deadline_ms: Option<u64>,
    last_proactive_ms: Option<u64>,
    proactive_hour: std::collections::VecDeque<u64>,
    now_ms: u64,
    proactive_interval_ms: u64,
    proactive_hour_limit: usize,
    baseline_changed_ms: Option<u64>,
}

impl Default for PetCore {
    fn default() -> Self {
        Self {
            state: State::default(),
            attached: false,
            activity_deadline_ms: None,
            last_proactive_ms: None,
            proactive_hour: Default::default(),
            now_ms: 0,
            proactive_interval_ms: 15 * 60_000,
            proactive_hour_limit: 2,
            baseline_changed_ms: None,
        }
    }
}

impl PetCore {
    fn desired_baseline(needs: Needs) -> BaselineExpression {
        use BaselineExpression::*;
        if needs.satiety <= 15 {
            Starving
        } else if needs.energy <= 15 {
            Depleted
        } else if needs.satiety <= 30 && needs.mood <= 35 {
            Irritable
        } else if needs.energy <= 35 {
            Tired
        } else if needs.mood <= 35 {
            Sad
        } else if needs.intimacy >= 70 && needs.mood >= 60 {
            Affectionate
        } else if needs.mood >= 80 && needs.satiety >= 40 && needs.energy >= 40 {
            Cheerful
        } else {
            Neutral
        }
    }

    fn keep_baseline(baseline: BaselineExpression, needs: Needs) -> bool {
        use BaselineExpression::*;
        match baseline {
            Starving => needs.satiety <= 20,
            Depleted => needs.energy <= 20,
            Irritable => needs.satiety <= 35 && needs.mood <= 40,
            Tired => needs.energy <= 40,
            Sad => needs.mood <= 40,
            Affectionate => needs.intimacy >= 65 && needs.mood >= 55,
            Cheerful => needs.mood >= 75 && needs.satiety >= 35 && needs.energy >= 35,
            Neutral => false,
        }
    }

    fn update_baseline(&mut self, needs: Needs, effects: &mut Vec<Effect>) {
        let desired = Self::desired_baseline(needs);
        let current = self.state.baseline;
        if desired == current {
            return;
        }
        let urgent = matches!(
            desired,
            BaselineExpression::Starving | BaselineExpression::Depleted
        );
        if !urgent && Self::keep_baseline(current, needs) {
            return;
        }
        if !urgent
            && self
                .baseline_changed_ms
                .is_some_and(|at| self.now_ms.saturating_sub(at) < 10_000)
        {
            return;
        }
        self.state.baseline = desired;
        self.baseline_changed_ms = Some(self.now_ms);
        if self.state.ready && self.state.avatar.baseline {
            effects.push(Effect::Avatar(AvatarCommand::SetBaseline(desired)));
        }
    }

    pub fn set_companion_limits(&mut self, interval_minutes: u16, hourly_limit: u8) {
        self.proactive_interval_ms = u64::from(interval_minutes.clamp(1, 60)) * 60_000;
        self.proactive_hour_limit = usize::from(hourly_limit.clamp(1, 4));
    }

    fn resting_activity(&self) -> Activity {
        if self.attached && self.state.external_snap_enabled() {
            Activity::Perched
        } else {
            Activity::Idle
        }
    }

    fn cancel_activity(&mut self, effects: &mut Vec<Effect>) {
        if self.activity_deadline_ms.take().is_some() {
            if self.state.activity == Activity::ScreenPlay {
                effects.push(Effect::Desktop(DesktopCommand::StopScreenPlay));
            } else {
                effects.push(Effect::Avatar(AvatarCommand::CancelFeedback));
            }
            self.state.activity = self.resting_activity();
        }
    }

    fn start_activity(
        &mut self,
        activity: Activity,
        duration_ms: u64,
        requested: Option<Feedback>,
        effects: &mut Vec<Effect>,
    ) {
        if self.state.activity == Activity::ScreenPlay {
            effects.push(Effect::Desktop(DesktopCommand::StopScreenPlay));
        } else {
            effects.push(Effect::Avatar(AvatarCommand::CancelFeedback));
        }
        self.state.activity = activity;
        self.activity_deadline_ms = Some(self.now_ms.saturating_add(duration_ms));
        if activity == Activity::ScreenPlay {
            effects.push(Effect::Desktop(DesktopCommand::StartScreenPlay));
            return;
        }
        let preferred = requested.unwrap_or(match activity {
            Activity::Eating => Feedback::Feed,
            Activity::Playing => Feedback::Play,
            Activity::Sleeping => Feedback::Rest,
            Activity::Greeting => Feedback::Greet,
            Activity::Peeking => Feedback::Peek,
            Activity::Inviting => Feedback::Invite,
            _ => return,
        });
        let feedback = match preferred {
            Feedback::Feed if self.state.avatar.feed => Some(Feedback::Feed),
            Feedback::Play if self.state.avatar.play => Some(Feedback::Play),
            Feedback::Rest if self.state.avatar.rest => Some(Feedback::Rest),
            Feedback::Greet if self.state.avatar.greet => Some(Feedback::Greet),
            Feedback::Peek if self.state.avatar.peek => Some(Feedback::Peek),
            Feedback::Invite if self.state.avatar.invite => Some(Feedback::Invite),
            Feedback::Celebrate if self.state.avatar.celebrate => Some(Feedback::Celebrate),
            Feedback::HeadPat if self.state.avatar.head_pat => Some(Feedback::HeadPat),
            Feedback::BodyTap if self.state.avatar.body_tap => Some(Feedback::BodyTap),
            _ if matches!(preferred, Feedback::Feed | Feedback::Rest | Feedback::Peek)
                && self.state.avatar.body_tap =>
            {
                Some(Feedback::BodyTap)
            }
            _ if self.state.avatar.head_pat => Some(Feedback::HeadPat),
            _ if self.state.avatar.body_tap => Some(Feedback::BodyTap),
            _ => None,
        };
        if let Some(feedback) = feedback {
            effects.push(Effect::Avatar(AvatarCommand::PlayFeedback(feedback)));
        }
    }

    fn tick(&mut self, needs: Needs, effects: &mut Vec<Effect>) {
        if self
            .activity_deadline_ms
            .is_some_and(|deadline| self.now_ms >= deadline)
        {
            self.cancel_activity(effects);
            return;
        }
        while self
            .proactive_hour
            .front()
            .is_some_and(|at| self.now_ms.saturating_sub(*at) >= 3_600_000)
        {
            self.proactive_hour.pop_front();
        }
        if !self.state.companion_enabled
            || self.state.do_not_disturb
            || !self.state.ready
            || !self.state.visible
            || self.state.activity == Activity::Dragged
            || self.activity_deadline_ms.is_some()
            || needs.energy < 20
            || self.proactive_hour.len() >= self.proactive_hour_limit
        {
            return;
        }
        let anchor = self.last_proactive_ms.unwrap_or(0);
        if self.now_ms.saturating_sub(anchor) < self.proactive_interval_ms {
            return;
        }
        let activity = if self.state.screen_play_enabled && self.proactive_hour.len() == 1 {
            Activity::ScreenPlay
        } else if needs.intimacy >= 30 && needs.mood >= 40 {
            Activity::Inviting
        } else if needs.mood < 50 {
            Activity::Peeking
        } else {
            Activity::Greeting
        };
        self.start_activity(
            activity,
            if activity == Activity::ScreenPlay {
                8_000
            } else {
                4_000
            },
            None,
            effects,
        );
        self.last_proactive_ms = Some(self.now_ms);
        self.proactive_hour.push_back(self.now_ms);
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    pub fn update(&mut self, event: Event) -> Vec<Effect> {
        let mut effects = Vec::new();
        // Quit is terminal for this instance. Late host events cannot revive it.
        if self.state.stopping {
            return effects;
        }
        let snap_before = self.state.external_snap_enabled();
        let mut synchronize = false;
        match event {
            Event::Intent(Intent::Quit) => {
                self.state.stopping = true;
                self.state.visible = false;
                self.state.ready = false;
                self.state.activity = Activity::Idle;
                self.activity_deadline_ms = None;
                // First effect is the emergency command, never animation work.
                return vec![Effect::Desktop(DesktopCommand::Shutdown)];
            }
            Event::Intent(Intent::SetVisible(visible)) => {
                self.state.visible = visible;
                effects.push(Effect::Desktop(DesktopCommand::SetVisible(
                    visible && self.state.ready,
                )));
                if !visible {
                    if self.state.activity == Activity::ScreenPlay {
                        effects.push(Effect::Desktop(DesktopCommand::StopScreenPlay));
                    }
                    self.state.activity = Activity::Idle;
                    self.activity_deadline_ms = None;
                    self.attached = false;
                    effects.push(Effect::Desktop(DesktopCommand::Detach));
                    effects.push(Effect::Avatar(AvatarCommand::CancelFeedback));
                }
            }
            Event::Intent(Intent::SetExternalSnapEnabled(enabled)) => {
                self.state.external_snap_requested = enabled;
            }
            Event::Intent(Intent::SetCompanionEnabled(enabled)) => {
                if enabled && !self.state.companion_enabled {
                    self.last_proactive_ms = Some(self.now_ms);
                }
                self.state.companion_enabled = enabled;
                if !enabled
                    && matches!(
                        self.state.activity,
                        Activity::Greeting
                            | Activity::Peeking
                            | Activity::Inviting
                            | Activity::ScreenPlay
                    )
                {
                    self.cancel_activity(&mut effects);
                }
            }
            Event::Intent(Intent::SetDoNotDisturb(enabled)) => {
                self.state.do_not_disturb = enabled;
                if enabled
                    && matches!(
                        self.state.activity,
                        Activity::Greeting
                            | Activity::Peeking
                            | Activity::Inviting
                            | Activity::ScreenPlay
                    )
                {
                    self.cancel_activity(&mut effects);
                }
            }
            Event::Intent(Intent::SetScreenPlayEnabled(enabled)) => {
                self.state.screen_play_enabled = enabled;
                if !enabled && self.state.activity == Activity::ScreenPlay {
                    self.cancel_activity(&mut effects);
                }
            }
            Event::Intent(Intent::StopActivity) => {
                if self.activity_deadline_ms.is_some() {
                    self.cancel_activity(&mut effects);
                } else {
                    effects.push(Effect::Avatar(AvatarCommand::CancelFeedback));
                }
                self.last_proactive_ms = Some(self.now_ms);
            }
            Event::CareCommitted {
                action,
                now_ms,
                needs,
            } => {
                self.now_ms = self.now_ms.max(now_ms);
                self.update_baseline(needs, &mut effects);
                if self.state.ready
                    && self.state.visible
                    && self.state.activity != Activity::Dragged
                {
                    let (activity, duration) = match action {
                        CareAction::Feed => (Activity::Eating, 3_000),
                        CareAction::Play => (Activity::Playing, 4_000),
                        CareAction::Rest => (Activity::Sleeping, 8_000),
                    };
                    let requested = if action == CareAction::Play
                        && needs.mood >= 80
                        && self.state.avatar.celebrate
                    {
                        Some(Feedback::Celebrate)
                    } else {
                        None
                    };
                    self.start_activity(activity, duration, requested, &mut effects);
                }
            }
            Event::Tick { now_ms, needs } => {
                self.now_ms = self.now_ms.max(now_ms);
                self.update_baseline(needs, &mut effects);
                self.tick(needs, &mut effects);
            }
            Event::Avatar(AvatarEvent::Ready(capabilities)) => {
                self.state.ready = true;
                self.state.avatar = capabilities;
                if capabilities.baseline {
                    effects.push(Effect::Avatar(AvatarCommand::SetBaseline(
                        self.state.baseline,
                    )));
                }
                self.state.fault = None;
                self.state.activity = Activity::Idle;
                self.activity_deadline_ms = None;
                self.attached = false;
                effects.push(Effect::Desktop(DesktopCommand::SetVisible(
                    self.state.visible,
                )));
                synchronize = true;
            }
            Event::Avatar(AvatarEvent::Fault(message)) => {
                self.state.ready = false;
                self.state.avatar = AvatarCapabilities::default();
                self.state.fault = Some(message);
                self.state.activity = Activity::Idle;
                self.activity_deadline_ms = None;
                self.attached = false;
                effects.push(Effect::Desktop(DesktopCommand::SetVisible(false)));
                effects.push(Effect::Desktop(DesktopCommand::Detach));
            }
            Event::Avatar(AvatarEvent::Hit(hit)) => {
                if self.state.ready
                    && self.state.visible
                    && self.state.activity != Activity::Dragged
                {
                    if self.activity_deadline_ms.is_some() {
                        self.cancel_activity(&mut effects);
                    }
                    let feedback = match hit.region {
                        HitRegion::Head | HitRegion::Face if self.state.avatar.head_pat => {
                            Some(Feedback::HeadPat)
                        }
                        HitRegion::Body
                        | HitRegion::LeftHand
                        | HitRegion::RightHand
                        | HitRegion::LeftArm
                        | HitRegion::RightArm
                        | HitRegion::Abdomen
                        | HitRegion::LeftLeg
                        | HitRegion::RightLeg
                        | HitRegion::LeftFoot
                        | HitRegion::RightFoot
                        | HitRegion::UpperBody
                        | HitRegion::LowerBody
                            if self.state.avatar.body_tap =>
                        {
                            Some(Feedback::BodyTap)
                        }
                        _ => None,
                    };
                    if let Some(feedback) = feedback {
                        effects.push(Effect::Avatar(AvatarCommand::PlayFeedback(feedback)));
                    }
                }
            }
            Event::Desktop(DesktopEvent::Capabilities(capabilities)) => {
                self.state.desktop = capabilities;
            }
            Event::Desktop(DesktopEvent::PermissionChanged(permission)) => {
                self.state.permission = permission;
            }
            Event::Desktop(DesktopEvent::DragStarted) => {
                if self.state.ready
                    && self.state.visible
                    && self.state.activity != Activity::Dragged
                {
                    let screen_play = self.state.activity == Activity::ScreenPlay;
                    self.state.activity = Activity::Dragged;
                    self.activity_deadline_ms = None;
                    self.attached = false;
                    effects.push(Effect::Desktop(DesktopCommand::Detach));
                    if screen_play {
                        effects.push(Effect::Desktop(DesktopCommand::StopScreenPlay));
                    } else {
                        effects.push(Effect::Avatar(AvatarCommand::CancelFeedback));
                    }
                }
            }
            Event::Desktop(DesktopEvent::DragEnded) => {
                if self.state.activity == Activity::Dragged {
                    self.state.activity = Activity::Idle;
                }
            }
            Event::Desktop(DesktopEvent::ExternalAttachmentChanged(attached)) => {
                if attached
                    && (!self.state.external_snap_enabled()
                        || self.state.activity == Activity::Dragged)
                {
                    effects.push(Effect::Desktop(DesktopCommand::Detach));
                } else if self.state.activity != Activity::Dragged {
                    self.attached = attached;
                    if self.activity_deadline_ms.is_none() {
                        self.state.activity = self.resting_activity();
                    }
                }
            }
            Event::Desktop(DesktopEvent::Stopped) => {
                self.state.ready = false;
                self.state.avatar = AvatarCapabilities::default();
                self.state.desktop = DesktopCapabilities::default();
                self.state.permission = Permission::Unknown;
                self.state.activity = Activity::Idle;
                self.activity_deadline_ms = None;
                self.attached = false;
            }
        }
        let snap_after = self.state.external_snap_enabled();
        if synchronize || snap_before != snap_after {
            effects.push(Effect::Desktop(DesktopCommand::SetExternalSnapEnabled(
                snap_after,
            )));
            if !snap_after && self.state.activity == Activity::Perched {
                self.state.activity = Activity::Idle;
                self.attached = false;
                effects.push(Effect::Desktop(DesktopCommand::Detach));
            }
        }
        effects
    }
}
