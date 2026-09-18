//! Deterministic P1 reducer. The shell delivers ordered events and executes effects.
//! Frame animation, gaze, blinking, hit testing and movement stay in the host.
use pet_protocol::{
    AvatarCapabilities, AvatarCommand, AvatarEvent, DesktopCapabilities, DesktopCommand,
    DesktopEvent, Feedback, HitRegion, Permission,
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Intent {
    SetVisible(bool),
    SetExternalSnapEnabled(bool),
    Quit,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Intent(Intent),
    Avatar(AvatarEvent),
    Desktop(DesktopEvent),
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

#[derive(Default)]
pub struct PetCore {
    state: State,
}

impl PetCore {
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
                // First effect is the emergency command, never animation work.
                return vec![Effect::Desktop(DesktopCommand::Shutdown)];
            }
            Event::Intent(Intent::SetVisible(visible)) => {
                self.state.visible = visible;
                effects.push(Effect::Desktop(DesktopCommand::SetVisible(
                    visible && self.state.ready,
                )));
                if !visible {
                    self.state.activity = Activity::Idle;
                    effects.push(Effect::Desktop(DesktopCommand::Detach));
                    effects.push(Effect::Avatar(AvatarCommand::CancelFeedback));
                }
            }
            Event::Intent(Intent::SetExternalSnapEnabled(enabled)) => {
                self.state.external_snap_requested = enabled;
            }
            Event::Avatar(AvatarEvent::Ready(capabilities)) => {
                self.state.ready = true;
                self.state.avatar = capabilities;
                self.state.fault = None;
                self.state.activity = Activity::Idle;
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
                effects.push(Effect::Desktop(DesktopCommand::SetVisible(false)));
                effects.push(Effect::Desktop(DesktopCommand::Detach));
            }
            Event::Avatar(AvatarEvent::Hit(region)) => {
                if self.state.ready
                    && self.state.visible
                    && self.state.activity != Activity::Dragged
                {
                    let feedback = match region {
                        HitRegion::Head if self.state.avatar.head_pat => Some(Feedback::HeadPat),
                        HitRegion::Body if self.state.avatar.body_tap => Some(Feedback::BodyTap),
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
                    self.state.activity = Activity::Dragged;
                    effects.push(Effect::Desktop(DesktopCommand::Detach));
                    effects.push(Effect::Avatar(AvatarCommand::CancelFeedback));
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
                    self.state.activity = if attached {
                        Activity::Perched
                    } else {
                        Activity::Idle
                    };
                }
            }
            Event::Desktop(DesktopEvent::Stopped) => {
                self.state.ready = false;
                self.state.avatar = AvatarCapabilities::default();
                self.state.desktop = DesktopCapabilities::default();
                self.state.permission = Permission::Unknown;
                self.state.activity = Activity::Idle;
            }
        }
        let snap_after = self.state.external_snap_enabled();
        if synchronize || snap_before != snap_after {
            effects.push(Effect::Desktop(DesktopCommand::SetExternalSnapEnabled(
                snap_after,
            )));
            if !snap_after && self.state.activity == Activity::Perched {
                self.state.activity = Activity::Idle;
                effects.push(Effect::Desktop(DesktopCommand::Detach));
            }
        }
        effects
    }
}
