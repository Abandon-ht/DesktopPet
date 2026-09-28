use pet_core::{
    care::Needs,
    touch::{PastTouch, TouchHistory, select_touch_response},
};
use pet_protocol::{HitRegion, TouchCue};

fn needs(intimacy: u8) -> Needs {
    Needs {
        intimacy,
        ..Needs::default()
    }
}
fn record(history: &mut TouchHistory, region: HitRegion, at: i64, rule: &str, delta: i16) {
    history.recent.push(PastTouch {
        region,
        occurred_ms: at,
        rule_id: rule.into(),
        intimacy_delta: delta,
    });
}

#[test]
fn relationship_thresholds_and_needs_control_ordinary_reactions() {
    let history = TouchHistory::default();
    let wary = select_touch_response(HitRegion::Head, needs(29), &history, 1000);
    assert_eq!(wary.expression_cue, Some(TouchCue::HeadWary));
    assert_eq!(wary.intimacy_delta, 0);
    let warm = select_touch_response(HitRegion::Head, needs(30), &history, 1000);
    assert_eq!(warm.expression_cue, Some(TouchCue::HeadWarm));
    assert_eq!(warm.intimacy_delta, 1);
    assert_eq!(
        select_touch_response(HitRegion::Face, needs(69), &history, 1000).expression_cue,
        Some(TouchCue::FaceWarm)
    );
    assert_eq!(
        select_touch_response(HitRegion::Face, needs(70), &history, 1000).expression_cue,
        Some(TouchCue::FaceClose)
    );
    let mut tired = needs(70);
    tired.energy = 15;
    let low = select_touch_response(HitRegion::Head, tired, &history, 1000);
    assert_eq!(low.expression_cue, Some(TouchCue::HeadClose));
    assert_eq!(low.intimacy_delta, 0);
    tired.energy = 16;
    tired.mood = 35;
    assert_eq!(
        select_touch_response(HitRegion::Face, tired, &history, 1000).expression_cue,
        Some(TouchCue::FaceClose)
    );
    tired.mood = 36;
    tired.satiety = 15;
    assert_eq!(
        select_touch_response(HitRegion::Head, tired, &history, 1000).intimacy_delta,
        0
    );
}

#[test]
fn both_remaining_regions_share_boundary_escalation_without_unlock() {
    let mut history = TouchHistory::default();
    let first = select_touch_response(HitRegion::UpperBody, needs(100), &history, 100_000);
    assert_eq!(
        (first.rule_id.as_str(), first.intimacy_delta),
        ("boundary_first", -5)
    );
    record(
        &mut history,
        HitRegion::UpperBody,
        100_000,
        &first.rule_id,
        -5,
    );
    let second = select_touch_response(HitRegion::LowerBody, needs(95), &history, 110_000);
    assert_eq!(
        (second.rule_id.as_str(), second.intimacy_delta),
        ("boundary_second", -15)
    );
    record(
        &mut history,
        HitRegion::LowerBody,
        110_000,
        &second.rule_id,
        -15,
    );
    assert_eq!(
        select_touch_response(HitRegion::Head, needs(80), &history, 111_000).intimacy_delta,
        0
    );
    let third = select_touch_response(HitRegion::UpperBody, needs(80), &history, 120_000);
    assert_eq!(third.expression_cue, Some(TouchCue::BoundaryThird));
    assert!(third.set_intimacy_zero);
    record(
        &mut history,
        HitRegion::UpperBody,
        120_000,
        &third.rule_id,
        -80,
    );
    assert_eq!(
        select_touch_response(HitRegion::LowerBody, needs(0), &history, 121_000).rule_id,
        "boundary_withdrawn"
    );
    assert_eq!(
        select_touch_response(HitRegion::Face, needs(70), &history, 121_000).expression_cue,
        None
    );
    assert_eq!(
        select_touch_response(HitRegion::UpperBody, needs(0), &history, 420_000).rule_id,
        "boundary_first"
    );
}

#[test]
fn ordinary_gain_and_discomfort_have_independent_limits() {
    let mut history = TouchHistory::default();
    record(&mut history, HitRegion::Head, 1_000, "head", 1);
    assert_eq!(
        select_touch_response(HitRegion::Head, needs(31), &history, 3_000).intimacy_delta,
        0
    );
    assert_eq!(
        select_touch_response(HitRegion::Head, needs(31), &history, 601_000).intimacy_delta,
        1
    );
    history.daily_positive = 6;
    assert_eq!(
        select_touch_response(HitRegion::Face, needs(31), &history, 601_000).intimacy_delta,
        0
    );
    record(&mut history, HitRegion::LeftLeg, 700_000, "uneasy", 0);
    record(&mut history, HitRegion::RightFoot, 701_000, "uneasy", 0);
    let third = select_touch_response(HitRegion::Abdomen, needs(31), &history, 702_000);
    assert_eq!(
        (third.rule_id.as_str(), third.intimacy_delta),
        ("discomfort", -2)
    );
    record(&mut history, HitRegion::Abdomen, 702_000, "discomfort", -2);
    assert_eq!(
        select_touch_response(HitRegion::LeftFoot, needs(29), &history, 703_000).rule_id,
        "discomfort_pause"
    );
}

#[test]
fn direct_head_face_and_hand_taps_keep_visual_feedback_during_gain_cooldown() {
    let mut history = TouchHistory::default();
    for region in [HitRegion::Head, HitRegion::Face, HitRegion::LeftHand] {
        history.recent.clear();
        record(&mut history, region, 1_000, "ordinary", 1);
        record(&mut history, region, 1_200, "ordinary", 0);
        record(&mut history, region, 1_400, "ordinary", 0);
        let feedback = select_touch_response(region, needs(44), &history, 1_500);
        assert!(feedback.expression_cue.is_some());
        assert_eq!(feedback.intimacy_delta, 0);
    }
}
