use pet_core::{
    Activity, Effect, Event, Intent, PetCore,
    care::{CareAction, Needs},
};
use pet_protocol::{
    AvatarCapabilities, AvatarCommand, AvatarEvent, BaselineExpression, DesktopCommand,
    DesktopEvent, Feedback,
};

fn ready() -> PetCore {
    let mut core = PetCore::default();
    core.update(Event::Avatar(AvatarEvent::Ready(AvatarCapabilities {
        head_pat: true,
        body_tap: true,
        ..Default::default()
    })));
    core
}

#[test]
fn needs_choose_stable_baseline_and_reconnect_replays_it() {
    let mut core = PetCore::default();
    let capabilities = AvatarCapabilities {
        baseline: true,
        celebrate: true,
        play: true,
        ..Default::default()
    };
    let ready = core.update(Event::Avatar(AvatarEvent::Ready(capabilities)));
    assert!(ready.contains(&Effect::Avatar(AvatarCommand::SetBaseline(
        BaselineExpression::Neutral
    ))));
    let mut needs = Needs {
        mood: 85,
        ..Default::default()
    };
    let effects = core.update(Event::Tick { now_ms: 100, needs });
    assert!(effects.contains(&Effect::Avatar(AvatarCommand::SetBaseline(
        BaselineExpression::Cheerful
    ))));
    needs.mood = 78;
    assert!(
        !core
            .update(Event::Tick {
                now_ms: 2_000,
                needs
            })
            .iter()
            .any(|e| matches!(e, Effect::Avatar(AvatarCommand::SetBaseline(_))))
    );
    needs.mood = 70;
    assert!(
        !core
            .update(Event::Tick {
                now_ms: 5_000,
                needs
            })
            .iter()
            .any(|e| matches!(e, Effect::Avatar(AvatarCommand::SetBaseline(_))))
    );
    let effects = core.update(Event::Tick {
        now_ms: 11_000,
        needs,
    });
    assert!(effects.contains(&Effect::Avatar(AvatarCommand::SetBaseline(
        BaselineExpression::Neutral
    ))));
    needs.satiety = 10;
    let effects = core.update(Event::Tick {
        now_ms: 11_001,
        needs,
    });
    assert!(effects.contains(&Effect::Avatar(AvatarCommand::SetBaseline(
        BaselineExpression::Starving
    ))));
    core.update(Event::Desktop(DesktopEvent::Stopped));
    let ready = core.update(Event::Avatar(AvatarEvent::Ready(capabilities)));
    assert!(ready.contains(&Effect::Avatar(AvatarCommand::SetBaseline(
        BaselineExpression::Starving
    ))));
}

#[test]
fn high_mood_play_uses_celebration_and_ordinary_play_does_not() {
    let mut core = PetCore::default();
    core.update(Event::Avatar(AvatarEvent::Ready(AvatarCapabilities {
        play: true,
        celebrate: true,
        ..Default::default()
    })));
    let mut needs = Needs {
        mood: 85,
        ..Default::default()
    };
    let effects = core.update(Event::CareCommitted {
        action: CareAction::Play,
        now_ms: 100,
        needs,
    });
    assert!(
        effects.contains(&Effect::Avatar(AvatarCommand::PlayFeedback(
            Feedback::Celebrate
        )))
    );
    core.update(Event::Intent(Intent::StopActivity));
    needs.mood = 60;
    let effects = core.update(Event::CareCommitted {
        action: CareAction::Play,
        now_ms: 10_100,
        needs,
    });
    assert!(effects.contains(&Effect::Avatar(AvatarCommand::PlayFeedback(Feedback::Play))));
}

#[test]
fn care_drag_stop_and_hide_resolve_without_mutual_exclusion_conflicts() {
    let mut core = ready();
    for n in 0..100_u64 {
        let now = n * 20_000;
        let care = [CareAction::Feed, CareAction::Play, CareAction::Rest][(n % 3) as usize];
        let effects = core.update(Event::CareCommitted {
            action: care,
            now_ms: now,
            needs: Needs::default(),
        });
        assert!(effects.contains(&Effect::Avatar(AvatarCommand::CancelFeedback)));
        assert!(matches!(
            core.state().activity,
            Activity::Eating | Activity::Playing | Activity::Sleeping
        ));
        let drag = core.update(Event::Desktop(DesktopEvent::DragStarted));
        assert_eq!(core.state().activity, Activity::Dragged);
        assert!(drag.contains(&Effect::Avatar(AvatarCommand::CancelFeedback)));
        core.update(Event::Intent(Intent::StopActivity));
        assert_eq!(core.state().activity, Activity::Dragged);
        core.update(Event::Desktop(DesktopEvent::DragEnded));
        assert_eq!(core.state().activity, Activity::Idle);
        core.update(Event::Tick {
            now_ms: now + 10_000,
            needs: Needs::default(),
        });
        assert_eq!(core.state().activity, Activity::Idle);
    }
    core.update(Event::CareCommitted {
        action: CareAction::Rest,
        now_ms: 2_100_000,
        needs: Needs::default(),
    });
    core.update(Event::Intent(Intent::SetVisible(false)));
    assert_eq!(core.state().activity, Activity::Idle);
    core.update(Event::Tick {
        now_ms: 2_200_000,
        needs: Needs::default(),
    });
    assert_eq!(core.state().activity, Activity::Idle);
}

#[test]
fn proactive_behaviors_obey_interval_hourly_limit_dnd_and_expiry() {
    let mut core = ready();
    core.set_companion_limits(1, 2);
    core.update(Event::Intent(Intent::SetCompanionEnabled(true)));
    assert!(
        core.update(Event::Tick {
            now_ms: 59_999,
            needs: Needs::default()
        })
        .is_empty()
    );
    let effects = core.update(Event::Tick {
        now_ms: 60_000,
        needs: Needs::default(),
    });
    assert_eq!(core.state().activity, Activity::Greeting);
    assert!(
        effects.contains(&Effect::Avatar(AvatarCommand::PlayFeedback(
            Feedback::HeadPat
        )))
    );
    core.update(Event::Tick {
        now_ms: 64_000,
        needs: Needs::default(),
    });
    assert_eq!(core.state().activity, Activity::Idle);
    core.update(Event::Tick {
        now_ms: 120_000,
        needs: Needs::default(),
    });
    assert_eq!(core.state().activity, Activity::Greeting);
    core.update(Event::Tick {
        now_ms: 124_000,
        needs: Needs::default(),
    });
    core.update(Event::Tick {
        now_ms: 180_000,
        needs: Needs::default(),
    });
    assert_eq!(core.state().activity, Activity::Idle); // two in the rolling hour
    core.update(Event::Tick {
        now_ms: 3_660_000,
        needs: Needs::default(),
    });
    assert_eq!(core.state().activity, Activity::Greeting);
    let effects = core.update(Event::Intent(Intent::SetDoNotDisturb(true)));
    assert_eq!(core.state().activity, Activity::Idle);
    assert!(effects.contains(&Effect::Avatar(AvatarCommand::CancelFeedback)));
    core.update(Event::Tick {
        now_ms: 4_000_000,
        needs: Needs::default(),
    });
    assert_eq!(core.state().activity, Activity::Idle);
}

#[test]
fn explicit_care_preempts_invitation_and_low_energy_blocks_proactive() {
    let mut core = ready();
    core.set_companion_limits(1, 2);
    core.update(Event::Intent(Intent::SetCompanionEnabled(true)));
    let mut needs = Needs {
        energy: 10,
        ..Default::default()
    };
    core.update(Event::Tick {
        now_ms: 60_000,
        needs,
    });
    assert_eq!(core.state().activity, Activity::Idle);
    needs.energy = 75;
    needs.intimacy = 40;
    core.update(Event::Tick {
        now_ms: 61_000,
        needs,
    });
    assert_eq!(core.state().activity, Activity::Inviting);
    core.update(Event::CareCommitted {
        action: CareAction::Feed,
        now_ms: 61_001,
        needs: Needs::default(),
    });
    assert_eq!(core.state().activity, Activity::Eating);
    core.update(Event::Tick {
        now_ms: 64_001,
        needs,
    });
    assert_eq!(core.state().activity, Activity::Idle);
}

#[test]
fn dedicated_pack_expression_is_preferred_and_stop_delays_next_prompt() {
    let mut core = PetCore::default();
    core.update(Event::Avatar(AvatarEvent::Ready(AvatarCapabilities {
        head_pat: true,
        greet: true,
        ..Default::default()
    })));
    core.set_companion_limits(1, 2);
    core.update(Event::Intent(Intent::SetCompanionEnabled(true)));
    let effects = core.update(Event::Tick {
        now_ms: 60_000,
        needs: Needs::default(),
    });
    assert!(
        effects.contains(&Effect::Avatar(AvatarCommand::PlayFeedback(
            Feedback::Greet
        )))
    );
    core.update(Event::Intent(Intent::StopActivity));
    assert_eq!(core.state().activity, Activity::Idle);
    core.update(Event::Tick {
        now_ms: 60_001,
        needs: Needs::default(),
    });
    assert_eq!(core.state().activity, Activity::Idle);
    core.update(Event::Tick {
        now_ms: 120_000,
        needs: Needs::default(),
    });
    assert_eq!(core.state().activity, Activity::Greeting);
}

#[test]
fn screen_play_is_opt_in_and_stop_or_deadline_always_exits() {
    let mut core = ready();
    core.set_companion_limits(1, 2);
    core.update(Event::Intent(Intent::SetCompanionEnabled(true)));
    core.update(Event::Tick {
        now_ms: 60_000,
        needs: Needs::default(),
    });
    core.update(Event::Tick {
        now_ms: 64_000,
        needs: Needs::default(),
    });
    core.update(Event::Tick {
        now_ms: 120_000,
        needs: Needs::default(),
    });
    assert_ne!(core.state().activity, Activity::ScreenPlay);

    let mut core = ready();
    core.set_companion_limits(1, 2);
    core.update(Event::Intent(Intent::SetScreenPlayEnabled(true)));
    core.update(Event::Intent(Intent::SetCompanionEnabled(true)));
    core.update(Event::Tick {
        now_ms: 60_000,
        needs: Needs::default(),
    });
    core.update(Event::Tick {
        now_ms: 64_000,
        needs: Needs::default(),
    });
    let effects = core.update(Event::Tick {
        now_ms: 120_000,
        needs: Needs::default(),
    });
    assert_eq!(core.state().activity, Activity::ScreenPlay);
    assert!(effects.contains(&Effect::Desktop(DesktopCommand::StartScreenPlay)));
    let effects = core.update(Event::Tick {
        now_ms: 128_000,
        needs: Needs::default(),
    });
    assert_eq!(core.state().activity, Activity::Idle);
    assert!(effects.contains(&Effect::Desktop(DesktopCommand::StopScreenPlay)));

    let mut core = ready();
    core.set_companion_limits(1, 2);
    core.update(Event::Intent(Intent::SetScreenPlayEnabled(true)));
    core.update(Event::Intent(Intent::SetCompanionEnabled(true)));
    core.update(Event::Tick {
        now_ms: 60_000,
        needs: Needs::default(),
    });
    core.update(Event::Tick {
        now_ms: 64_000,
        needs: Needs::default(),
    });
    core.update(Event::Tick {
        now_ms: 120_000,
        needs: Needs::default(),
    });
    let effects = core.update(Event::Intent(Intent::StopActivity));
    assert_eq!(core.state().activity, Activity::Idle);
    assert!(effects.contains(&Effect::Desktop(DesktopCommand::StopScreenPlay)));
}
