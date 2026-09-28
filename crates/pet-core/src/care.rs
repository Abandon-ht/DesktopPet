//! Pure P2 care rules. The caller supplies UTC time; storage and animation are external.
use serde::{Deserialize, Serialize};

const HOUR_MS: i64 = 3_600_000;
const MAX_OFFLINE_MS: i64 = 24 * HOUR_MS;
const PLAY_COOLDOWN_MS: i64 = 10 * 60_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Needs {
    pub satiety: u8,
    pub energy: u8,
    pub mood: u8,
    pub intimacy: u8,
}

impl Default for Needs {
    fn default() -> Self {
        Self {
            satiety: 75,
            energy: 75,
            mood: 70,
            intimacy: 20,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CareState {
    pub needs: Needs,
    pub food: u32,
    /// Last wall-clock observation. Live timers must use a monotonic clock.
    pub observed_utc_ms: i64,
    /// Fractional time retained so frequent saves do not erase decay.
    pub decay_remainder_ms: i64,
    pub last_play_utc_ms: Option<i64>,
}

impl CareState {
    pub fn new(now_utc_ms: i64) -> Self {
        Self {
            needs: Needs::default(),
            food: 3,
            observed_utc_ms: now_utc_ms,
            decay_remainder_ms: 0,
            last_play_utc_ms: None,
        }
    }

    /// A clock rollback has no effect. A long absence applies at most 24 hours once.
    pub fn advance(&mut self, now_utc_ms: i64) {
        if now_utc_ms < self.observed_utc_ms {
            // Keep needs and inventory unchanged. Rebase a future cooldown so
            // a clock correction cannot block play until that distant date.
            if self.last_play_utc_ms.is_some_and(|last| last > now_utc_ms) {
                self.last_play_utc_ms = Some(now_utc_ms);
            }
            return;
        }
        if now_utc_ms == self.observed_utc_ms {
            return;
        }
        // One free daily supply, even after a long absence. Existing food is
        // retained; polling frequently cannot postpone or multiply the refill.
        if now_utc_ms.div_euclid(MAX_OFFLINE_MS) > self.observed_utc_ms.div_euclid(MAX_OFFLINE_MS) {
            self.food = self.food.max(3);
        }
        let elapsed = now_utc_ms
            .saturating_sub(self.observed_utc_ms)
            .min(MAX_OFFLINE_MS);
        let hours = (self.decay_remainder_ms + elapsed) / HOUR_MS;
        self.decay_remainder_ms = (self.decay_remainder_ms + elapsed) % HOUR_MS;
        self.needs.satiety = self
            .needs
            .satiety
            .saturating_sub((hours * 2).min(100) as u8);
        self.needs.energy = self.needs.energy.saturating_sub(hours.min(100) as u8);
        self.needs.mood = self.needs.mood.saturating_sub(hours.min(100) as u8);
        self.observed_utc_ms = now_utc_ms;
    }

    pub fn apply(&mut self, action: CareAction, now_utc_ms: i64) -> Result<(), CareError> {
        self.advance(now_utc_ms);
        match action {
            CareAction::Feed => {
                if self.food == 0 {
                    return Err(CareError::NoFood);
                }
                self.food -= 1;
                self.needs.satiety = self.needs.satiety.saturating_add(25).min(100);
                self.needs.mood = self.needs.mood.saturating_add(3).min(100);
                self.needs.intimacy = self.needs.intimacy.saturating_add(2).min(100);
            }
            CareAction::Play => {
                if self.needs.energy < 10 {
                    return Err(CareError::TooTired);
                }
                if self
                    .last_play_utc_ms
                    .is_some_and(|last| now_utc_ms.saturating_sub(last) < PLAY_COOLDOWN_MS)
                {
                    return Err(CareError::Cooldown);
                }
                self.needs.energy = self.needs.energy.saturating_sub(10);
                self.needs.mood = self.needs.mood.saturating_add(12).min(100);
                self.needs.intimacy = self.needs.intimacy.saturating_add(4).min(100);
                self.last_play_utc_ms = Some(now_utc_ms);
            }
            CareAction::Rest => {
                self.needs.energy = self.needs.energy.saturating_add(20).min(100);
                self.needs.mood = self.needs.mood.saturating_add(2).min(100);
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CareAction {
    Feed,
    Play,
    Rest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CareError {
    NoFood,
    TooTired,
    Cooldown,
}
