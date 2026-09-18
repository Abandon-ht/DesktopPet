use pet_core::{Activity, Effect, Event, Intent, PetCore};
use pet_protocol::*;

fn ready(core: &mut PetCore) {
    core.update(Event::Avatar(AvatarEvent::Ready(AvatarCapabilities {
        head_pat: true,
        body_tap: false,
    })));
}

fn enable_snap(core: &mut PetCore) {
    ready(core);
    core.update(Event::Intent(Intent::SetExternalSnapEnabled(true)));
    core.update(Event::Desktop(DesktopEvent::Capabilities(
        DesktopCapabilities {
            absolute_position: true,
            external_window_observation: true,
            ..Default::default()
        },
    )));
    core.update(Event::Desktop(DesktopEvent::PermissionChanged(
        Permission::Granted,
    )));
}

#[test]
fn hundred_click_drag_sequences_preserve_interaction_rules() {
    let mut core = PetCore::default();
    ready(&mut core);
    for _ in 0..100 {
        assert_eq!(
            core.update(Event::Avatar(AvatarEvent::Hit(HitRegion::Head))),
            vec![Effect::Avatar(AvatarCommand::PlayFeedback(
                Feedback::HeadPat
            ))]
        );
        // Unsupported character feedback is a no-op, never an action wait.
        assert!(
            core.update(Event::Avatar(AvatarEvent::Hit(HitRegion::Body)))
                .is_empty()
        );
        core.update(Event::Desktop(DesktopEvent::DragStarted));
        assert_eq!(core.state().activity, Activity::Dragged);
        assert!(
            core.update(Event::Avatar(AvatarEvent::Hit(HitRegion::Head)))
                .is_empty()
        );
        assert!(
            core.update(Event::Desktop(DesktopEvent::DragStarted))
                .is_empty()
        );
        core.update(Event::Desktop(DesktopEvent::ExternalAttachmentChanged(
            true,
        )));
        assert_eq!(core.state().activity, Activity::Dragged);
        core.update(Event::Desktop(DesktopEvent::DragEnded));
        assert_eq!(core.state().activity, Activity::Idle);
    }
}

#[test]
fn denied_permission_and_revocation_keep_basic_pet_usable() {
    let mut core = PetCore::default();
    enable_snap(&mut core);
    assert!(core.state().external_snap_enabled());
    core.update(Event::Desktop(DesktopEvent::ExternalAttachmentChanged(
        true,
    )));
    assert_eq!(core.state().activity, Activity::Perched);
    let effects = core.update(Event::Desktop(DesktopEvent::PermissionChanged(
        Permission::Denied,
    )));
    assert!(effects.contains(&Effect::Desktop(DesktopCommand::Detach)));
    assert!(
        effects.contains(&Effect::Desktop(DesktopCommand::SetExternalSnapEnabled(
            false
        )))
    );
    assert!(!core.state().external_snap_enabled());
    assert!(core.state().ready && core.state().visible);
    assert_eq!(core.state().activity, Activity::Idle);
    assert!(
        !core
            .update(Event::Avatar(AvatarEvent::Hit(HitRegion::Head)))
            .is_empty()
    );
    let effects = core.update(Event::Desktop(DesktopEvent::ExternalAttachmentChanged(
        true,
    )));
    assert_eq!(effects, vec![Effect::Desktop(DesktopCommand::Detach)]);
}

#[test]
fn capability_loss_and_target_close_detach() {
    let mut core = PetCore::default();
    enable_snap(&mut core);
    core.update(Event::Desktop(DesktopEvent::ExternalAttachmentChanged(
        true,
    )));
    core.update(Event::Desktop(DesktopEvent::ExternalAttachmentChanged(
        false,
    )));
    assert_eq!(core.state().activity, Activity::Idle);
    core.update(Event::Desktop(DesktopEvent::ExternalAttachmentChanged(
        true,
    )));
    let effects = core.update(Event::Desktop(DesktopEvent::Capabilities(
        Default::default(),
    )));
    assert!(effects.contains(&Effect::Desktop(DesktopCommand::Detach)));
    assert!(!core.state().external_snap_enabled());
}

#[test]
fn permission_alone_does_not_enable_unavailable_platform_features() {
    let mut core = PetCore::default();
    ready(&mut core);
    core.update(Event::Intent(Intent::SetExternalSnapEnabled(true)));
    core.update(Event::Desktop(DesktopEvent::PermissionChanged(
        Permission::Granted,
    )));
    assert!(!core.state().external_snap_enabled());
}

#[test]
fn hide_during_drag_and_restart_preserve_user_intent() {
    let mut core = PetCore::default();
    enable_snap(&mut core);
    core.update(Event::Desktop(DesktopEvent::DragStarted));
    let effects = core.update(Event::Intent(Intent::SetVisible(false)));
    assert_eq!(
        effects.first(),
        Some(&Effect::Desktop(DesktopCommand::SetVisible(false)))
    );
    assert_eq!(core.state().activity, Activity::Idle);
    assert!(
        core.update(Event::Avatar(AvatarEvent::Hit(HitRegion::Head)))
            .is_empty()
    );
    core.update(Event::Desktop(DesktopEvent::Stopped));
    let effects = core.update(Event::Avatar(AvatarEvent::Ready(Default::default())));
    assert!(effects.contains(&Effect::Desktop(DesktopCommand::SetVisible(false))));
    assert!(!core.state().external_snap_enabled());
    assert_eq!(core.state().permission, Permission::Unknown);
}

#[test]
fn fault_is_visible_in_projection_and_quit_works_without_ready_host() {
    let mut core = PetCore::default();
    core.update(Event::Avatar(AvatarEvent::Fault("missing model".into())));
    assert_eq!(core.state().fault.as_deref(), Some("missing model"));
    assert_eq!(
        core.update(Event::Intent(Intent::SetVisible(true))),
        vec![Effect::Desktop(DesktopCommand::SetVisible(false))]
    );
    let effects = core.update(Event::Intent(Intent::Quit));
    assert_eq!(effects, vec![Effect::Desktop(DesktopCommand::Shutdown)]);
    let stopped = core.state().clone();
    ready(&mut core);
    core.update(Event::Intent(Intent::SetVisible(true)));
    assert_eq!(core.state(), &stopped);
}

#[test]
fn ready_after_fault_restores_desired_visibility() {
    let mut core = PetCore::default();
    core.update(Event::Avatar(AvatarEvent::Fault("load failed".into())));
    ready(&mut core);
    assert!(core.state().ready && core.state().visible);
    assert_eq!(core.state().fault, None);
}
