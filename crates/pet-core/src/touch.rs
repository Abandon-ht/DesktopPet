//! Deterministic region and relationship rules. Storage supplies recent accepted hits.
use crate::care::Needs;
use pet_protocol::{HitRegion, TouchCue};
use serde::{Deserialize, Serialize};

const MINUTE: i64 = 60_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceCue {
    Acknowledge,
    Uncomfortable,
    Boundary,
    Withdrawn,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TouchDecision {
    pub region: HitRegion,
    pub rule_id: String,
    pub expression_cue: Option<TouchCue>,
    pub intimacy_delta: i16,
    pub set_intimacy_zero: bool,
    pub retreat_until_ms: Option<i64>,
    pub voice_cue: Option<VoiceCue>,
    pub reason: String,
}
impl TouchDecision {
    pub fn apply_intimacy(&mut self, needs: &mut Needs) {
        let before = needs.intimacy;
        needs.intimacy = if self.set_intimacy_zero {
            0
        } else if self.intimacy_delta >= 0 {
            before.saturating_add(self.intimacy_delta as u8).min(100)
        } else {
            before.saturating_sub((-self.intimacy_delta) as u8)
        };
        self.intimacy_delta = i16::from(needs.intimacy) - i16::from(before);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PastTouch {
    pub region: HitRegion,
    pub occurred_ms: i64,
    pub rule_id: String,
    pub intimacy_delta: i16,
}

#[derive(Default)]
pub struct TouchHistory {
    pub recent: Vec<PastTouch>,
    pub daily_positive: u8,
}

fn decision(region: HitRegion, rule: &str, cue: Option<TouchCue>, delta: i16) -> TouchDecision {
    TouchDecision {
        region,
        rule_id: rule.into(),
        expression_cue: cue,
        intimacy_delta: delta,
        set_intimacy_zero: false,
        retreat_until_ms: None,
        voice_cue: None,
        reason: String::new(),
    }
}

fn within(now: i64, then: i64, window: i64) -> bool {
    now >= then && now - then < window
}

pub fn select_touch_response(
    region: HitRegion,
    needs: Needs,
    history: &TouchHistory,
    now_ms: i64,
) -> TouchDecision {
    let recent = &history.recent;
    if matches!(region, HitRegion::UpperBody | HitRegion::LowerBody) {
        if let Some(third) = recent
            .iter()
            .rev()
            .find(|hit| hit.rule_id == "boundary_third")
            && within(now_ms, third.occurred_ms, 5 * MINUTE)
        {
            let mut result = decision(region, "boundary_withdrawn", None, 0);
            result.retreat_until_ms = Some(third.occurred_ms + 5 * MINUTE);
            result.voice_cue = Some(VoiceCue::Withdrawn);
            result.reason = "暂时不接受触摸".into();
            return result;
        }
        let count = recent
            .iter()
            .filter(|hit| {
                matches!(hit.region, HitRegion::UpperBody | HitRegion::LowerBody)
                    && within(now_ms, hit.occurred_ms, MINUTE)
            })
            .count();
        let (rule, cue, delta) = match count {
            0 => ("boundary_first", TouchCue::BoundaryFirst, -5),
            1 => ("boundary_second", TouchCue::BoundarySecond, -15),
            _ => ("boundary_third", TouchCue::BoundaryThird, 0),
        };
        let mut result = decision(region, rule, Some(cue), delta);
        result.set_intimacy_zero = count >= 2;
        result.retreat_until_ms = match count {
            1 => Some(now_ms + 30_000),
            n if n >= 2 => Some(now_ms + 5 * MINUTE),
            _ => None,
        };
        result.voice_cue = Some(VoiceCue::Boundary);
        result.reason = "非互动区域，已表达不满".into();
        return result;
    }

    let withdrawn_until = recent
        .iter()
        .rev()
        .find(|hit| hit.rule_id == "boundary_third" && within(now_ms, hit.occurred_ms, 5 * MINUTE))
        .map(|hit| hit.occurred_ms + 5 * MINUTE);
    if let Some(until) = withdrawn_until {
        let mut result = decision(region, "withdrawn", None, 0);
        result.retreat_until_ms = Some(until);
        result.reason = "需要暂时保持距离".into();
        return result;
    }
    let retreat_until = recent
        .iter()
        .rev()
        .find(|hit| hit.rule_id == "boundary_second" && within(now_ms, hit.occurred_ms, 30_000))
        .map(|hit| hit.occurred_ms + 30_000);
    let retreat = retreat_until.is_some();

    if matches!(
        region,
        HitRegion::Abdomen
            | HitRegion::LeftLeg
            | HitRegion::RightLeg
            | HitRegion::LeftFoot
            | HitRegion::RightFoot
    ) {
        if let Some(last) = recent.iter().rev().find(|hit| hit.rule_id == "discomfort")
            && within(now_ms, last.occurred_ms, 30_000)
        {
            let mut result = decision(region, "discomfort_pause", None, 0);
            result.retreat_until_ms = Some(last.occurred_ms + 30_000);
            result.reason = "不适反馈暂时冷却".into();
            return result;
        }
        let count = recent
            .iter()
            .filter(|hit| {
                matches!(
                    hit.region,
                    HitRegion::Abdomen
                        | HitRegion::LeftLeg
                        | HitRegion::RightLeg
                        | HitRegion::LeftFoot
                        | HitRegion::RightFoot
                ) && within(now_ms, hit.occurred_ms, 30_000)
            })
            .count();
        if count >= 2 {
            let mut result = decision(region, "discomfort", Some(TouchCue::Discomfort), -2);
            result.retreat_until_ms = Some(now_ms + 30_000);
            result.voice_cue = Some(VoiceCue::Uncomfortable);
            result.reason = "连续触摸令角色不适".into();
            return result;
        }
    }

    let familiar = needs.intimacy >= 30;
    let close = needs.intimacy >= 70;
    let low = needs.satiety <= 15 || needs.energy <= 15 || needs.mood <= 35;
    let (rule, cue, gain) = match region {
        HitRegion::Head => (
            "head",
            Some(if close {
                TouchCue::HeadClose
            } else if familiar {
                TouchCue::HeadWarm
            } else {
                TouchCue::HeadWary
            }),
            familiar,
        ),
        HitRegion::Face => (
            "face",
            Some(if close {
                TouchCue::FaceClose
            } else if familiar {
                TouchCue::FaceWarm
            } else {
                TouchCue::FaceWary
            }),
            familiar,
        ),
        HitRegion::LeftHand | HitRegion::RightHand => (
            "hand",
            Some(if close {
                TouchCue::HandClose
            } else if familiar {
                TouchCue::HandWarm
            } else {
                TouchCue::HandWary
            }),
            false,
        ),
        HitRegion::LeftArm | HitRegion::RightArm => ("arm", Some(TouchCue::Arm), false),
        HitRegion::Abdomen
        | HitRegion::LeftLeg
        | HitRegion::RightLeg
        | HitRegion::LeftFoot
        | HitRegion::RightFoot => ("uneasy", Some(TouchCue::Uneasy), false),
        HitRegion::Body => ("body_fallback", None, false),
        HitRegion::UpperBody | HitRegion::LowerBody => unreachable!(),
    };
    let direct_expression = matches!(
        region,
        HitRegion::Head | HitRegion::Face | HitRegion::LeftHand | HitRegion::RightHand
    );
    let cue = if low && !direct_expression {
        if needs.energy <= 15 {
            Some(TouchCue::Uneasy)
        } else if matches!(region, HitRegion::Face) {
            Some(TouchCue::FaceWary)
        } else {
            None
        }
    } else {
        cue
    };
    let same_recent: Vec<_> = recent
        .iter()
        .filter(|hit| hit.region == region && within(now_ms, hit.occurred_ms, 10_000))
        .collect();
    let cue = if !direct_expression
        && (same_recent.len() >= 3
            || same_recent
                .last()
                .is_some_and(|hit| within(now_ms, hit.occurred_ms, 2_000)))
    {
        None
    } else {
        cue
    };
    let last_gain = recent.iter().rev().any(|hit| {
        hit.region == region
            && hit.intimacy_delta > 0
            && within(now_ms, hit.occurred_ms, 10 * MINUTE)
    });
    let delta = i16::from(
        gain && !low
            && !retreat
            && !last_gain
            && history.daily_positive < 6
            && needs.intimacy < 100,
    );
    let mut result = decision(region, rule, cue, delta);
    result.retreat_until_ms = retreat_until;
    result.voice_cue = cue.map(|_| VoiceCue::Acknowledge);
    result.reason = if low {
        "当前不想互动"
    } else if retreat {
        "暂时不增加亲密度"
    } else {
        "普通互动"
    }
    .into();
    result
}
