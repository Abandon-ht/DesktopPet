//! Versioned semantic DTOs. No renderer, OS handles, transport, or character parameters.
//! The production wire transport and handshake are a separate P1 step.
use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 6;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BaselineExpression {
    #[default]
    Neutral,
    Cheerful,
    Sad,
    Irritable,
    Tired,
    Depleted,
    Starving,
    Affectionate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HitRegion {
    Head,
    Body,
    Face,
    /// Screen-left hand in the character viewport.
    LeftHand,
    /// Screen-right hand in the character viewport.
    RightHand,
    LeftArm,
    RightArm,
    Abdomen,
    LeftLeg,
    RightLeg,
    LeftFoot,
    RightFoot,
    UpperBody,
    LowerBody,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HitDetail {
    pub region: HitRegion,
    /// Top-left viewport coordinates in thousandths (0–1000).
    pub point: [u16; 2],
    /// Monotonic within one rendering-host session.
    pub event_id: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TouchCue {
    HeadWary,
    HeadWarm,
    HeadClose,
    FaceWary,
    FaceWarm,
    FaceClose,
    HandWary,
    HandWarm,
    HandClose,
    Arm,
    Uneasy,
    Discomfort,
    BoundaryFirst,
    BoundarySecond,
    BoundaryThird,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Feedback {
    HeadPat,
    BodyTap,
    Feed,
    Play,
    Rest,
    Greet,
    Peek,
    Invite,
    Celebrate,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AvatarCapabilities {
    pub head_pat: bool,
    pub body_tap: bool,
    pub feed: bool,
    pub play: bool,
    pub rest: bool,
    pub greet: bool,
    pub peek: bool,
    pub invite: bool,
    #[serde(default)]
    pub celebrate: bool,
    #[serde(default)]
    pub baseline: bool,
    #[serde(default)]
    pub touch_reactions: bool,
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
    PlayTouchCue(TouchCue),
    CancelFeedback,
    SetBaseline(BaselineExpression),
    PreviewExpression(String),
    EndPreview,
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
    /// Gaze activation radius around the pet face in logical pixels (150..=1200).
    SetGazeRadius(u16),
    /// Host chooses a nearby valid target on release; no per-frame IPC.
    SetExternalSnapEnabled(bool),
    StartScreenPlay,
    StopScreenPlay,
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
    Hit(HitDetail),
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
        let hit = AvatarEvent::Hit(HitDetail {
            region: HitRegion::LeftHand,
            point: [295, 560],
            event_id: 7,
        });
        let json =
            r#"{"type":"hit","payload":{"region":"left_hand","point":[295,560],"event_id":7}}"#;
        assert_eq!(serde_json::to_string(&hit).unwrap(), json);
        assert_eq!(serde_json::from_str::<AvatarEvent>(json).unwrap(), hit);
        assert_eq!(
            serde_json::to_string(&HitRegion::LeftFoot).unwrap(),
            "\"left_foot\""
        );
    }
}
