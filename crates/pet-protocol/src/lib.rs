//! P1 semantic DTOs. No renderer, OS handles, transport, or character parameters.
//! The production wire transport and handshake are a separate P1 step.
use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HitRegion {
    Head,
    Body,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Feedback {
    HeadPat,
    BodyTap,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AvatarCapabilities {
    pub head_pat: bool,
    pub body_tap: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopCapabilities {
    pub global_pointer: bool,
    pub absolute_position: bool,
    pub external_window_observation: bool,
    pub input_passthrough: bool,
    pub workspace_tracking: bool,
    pub global_shortcut: bool,
}

/// Capability and permission are independent; a grant alone enables nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    #[default]
    Unknown,
    Granted,
    Denied,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "payload",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum AvatarCommand {
    PlayFeedback(Feedback),
    CancelFeedback,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "payload",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum DesktopCommand {
    SetVisible(bool),
    /// Percentage of the base 500x600 logical viewport (50..=150).
    SetScale(u16),
    /// Normalized window contact height in percent (20..=80).
    SetWindowPerch(u16),
    /// Host chooses a nearby valid target on release; no per-frame IPC.
    SetExternalSnapEnabled(bool),
    Detach,
    Shutdown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "payload",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum AvatarEvent {
    Ready(AvatarCapabilities),
    Hit(HitRegion),
    Fault(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "payload",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum DesktopEvent {
    Capabilities(DesktopCapabilities),
    PermissionChanged(Permission),
    DragStarted,
    DragEnded,
    /// Report attachment state; target identity and geometry stay in the host.
    ExternalAttachmentChanged(bool),
    Stopped,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantic_wire_shape_is_stable() {
        let command = AvatarCommand::PlayFeedback(Feedback::HeadPat);
        let json = r#"{"type":"play_feedback","payload":"head_pat"}"#;
        assert_eq!(serde_json::to_string(&command).unwrap(), json);
        assert_eq!(
            serde_json::from_str::<AvatarCommand>(json).unwrap(),
            command
        );
        assert!(
            serde_json::from_str::<AvatarCommand>(
                r#"{"type":"play_feedback","payload":"ParamEyeLOpen"}"#
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<DesktopCommand>(
                r#"{"type":"set_visible","payload":true,"unexpected":1}"#
            )
            .is_err()
        );
    }
}
