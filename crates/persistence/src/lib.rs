//! One SQLite writer for P2 care state. Every accepted action is committed with
//! its inventory change and request ID before the caller starts an animation.
use pet_core::care::{CareAction, CareError, CareState, Needs};
use pet_core::touch::{PastTouch, TouchDecision, TouchHistory, select_touch_response};
use pet_protocol::HitRegion;
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::Serialize;
use std::{path::Path, time::Duration};

#[derive(Debug)]
pub enum StoreError {
    Database(rusqlite::Error),
    Rules(CareError),
    InvalidRequestId,
    RequestIdConflict,
    InvalidTouchId,
    Json(serde_json::Error),
    UnsupportedSchema(i64),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Database(e) => write!(f, "database: {e}"),
            Self::Rules(e) => write!(f, "care rule: {e:?}"),
            Self::InvalidRequestId => write!(
                f,
                "request ID must contain 1–128 printable ASCII characters"
            ),
            Self::RequestIdConflict => write!(f, "request ID was already used for another action"),
            Self::InvalidTouchId => write!(f, "invalid touch session or event ID"),
            Self::Json(e) => write!(f, "touch event JSON: {e}"),
            Self::UnsupportedSchema(v) => write!(f, "unsupported database schema version {v}"),
        }
    }
}
impl std::error::Error for StoreError {}
impl From<rusqlite::Error> for StoreError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Database(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CareOutcome {
    pub state: CareState,
    pub replayed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TouchOutcome {
    pub state: CareState,
    pub decision: TouchDecision,
    pub occurred_utc_ms: i64,
    pub replayed: bool,
}

/// Keep this on one owner thread; commands from UI and timers queue to that thread.
pub struct CareStore {
    connection: Connection,
}

impl CareStore {
    pub fn open(path: impl AsRef<Path>, now_utc_ms: i64) -> Result<Self, StoreError> {
        let mut connection = Connection::open(path)?;
        connection.busy_timeout(Duration::from_secs(2))?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if !(0..=2).contains(&version) {
            return Err(StoreError::UnsupportedSchema(version));
        }
        if version == 0 {
            let tx = connection.transaction()?;
            tx.execute_batch("CREATE TABLE IF NOT EXISTS schema_migrations (version INTEGER PRIMARY KEY, applied_utc_ms INTEGER NOT NULL);
                CREATE TABLE IF NOT EXISTS pet_state (
                    id INTEGER PRIMARY KEY CHECK(id = 1),
                    satiety INTEGER NOT NULL CHECK(satiety BETWEEN 0 AND 100),
                    energy INTEGER NOT NULL CHECK(energy BETWEEN 0 AND 100),
                    mood INTEGER NOT NULL CHECK(mood BETWEEN 0 AND 100),
                    intimacy INTEGER NOT NULL CHECK(intimacy BETWEEN 0 AND 100),
                    observed_utc_ms INTEGER NOT NULL,
                    decay_remainder_ms INTEGER NOT NULL CHECK(decay_remainder_ms >= 0 AND decay_remainder_ms < 3600000),
                    last_play_utc_ms INTEGER
                );
                CREATE TABLE IF NOT EXISTS inventory (
                    item TEXT PRIMARY KEY, quantity INTEGER NOT NULL CHECK(quantity >= 0)
                );
                CREATE TABLE IF NOT EXISTS care_events (
                    request_id TEXT PRIMARY KEY, action TEXT NOT NULL,
                    occurred_utc_ms INTEGER NOT NULL
                );
                CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);")?;
            let initial = CareState::new(now_utc_ms);
            write_state(&tx, &initial)?;
            tx.execute(
                "INSERT INTO schema_migrations(version, applied_utc_ms) VALUES (1, ?1)",
                [now_utc_ms],
            )?;
            tx.pragma_update(None, "user_version", 1)?;
            tx.commit()?;
        }
        if version <= 1 {
            let tx = connection.transaction()?;
            tx.execute_batch("CREATE TABLE IF NOT EXISTS touch_events (
                host_session TEXT NOT NULL,
                event_id INTEGER NOT NULL,
                pack_id TEXT NOT NULL,
                region TEXT NOT NULL,
                occurred_utc_ms INTEGER NOT NULL,
                rule_id TEXT NOT NULL,
                intimacy_delta INTEGER NOT NULL,
                decision_json TEXT NOT NULL,
                PRIMARY KEY (host_session, event_id)
            );
            CREATE INDEX IF NOT EXISTS touch_events_pack_time ON touch_events(pack_id, occurred_utc_ms);")?;
            tx.execute(
                "INSERT OR IGNORE INTO schema_migrations(version, applied_utc_ms) VALUES (2, ?1)",
                [now_utc_ms],
            )?;
            tx.pragma_update(None, "user_version", 2)?;
            tx.commit()?;
        }
        Ok(Self { connection })
    }

    /// Read and persist capped offline decay. Reopening after a crash sees the
    /// last committed state, even when no care action followed the read.
    pub fn load_at(&mut self, now_utc_ms: i64) -> Result<CareState, StoreError> {
        let tx = self.connection.transaction()?;
        let mut state = read_state(&tx)?;
        state.advance(now_utc_ms);
        write_state(&tx, &state)?;
        tx.commit()?;
        Ok(state)
    }

    pub fn setting(&self, key: &str) -> Result<Option<String>, StoreError> {
        Ok(self
            .connection
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()?)
    }

    pub fn set_setting(&mut self, key: &str, value: &str) -> Result<(), StoreError> {
        let tx = self.connection.transaction()?;
        tx.execute(
            "INSERT INTO settings(key, value) VALUES (?1, ?2)
            ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn apply(
        &mut self,
        request_id: &str,
        action: CareAction,
        now_utc_ms: i64,
    ) -> Result<CareOutcome, StoreError> {
        if request_id.is_empty()
            || request_id.len() > 128
            || !request_id.bytes().all(|b| (0x21..=0x7e).contains(&b))
        {
            return Err(StoreError::InvalidRequestId);
        }
        let tx = self.connection.transaction()?;
        let action_name = serde_json::to_string(&action).expect("care action serialization");
        let previous: Option<String> = tx
            .query_row(
                "SELECT action FROM care_events WHERE request_id = ?1",
                [request_id],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(previous) = previous {
            if previous != action_name {
                return Err(StoreError::RequestIdConflict);
            }
            return Ok(CareOutcome {
                state: read_state(&tx)?,
                replayed: true,
            });
        }
        let mut state = read_state(&tx)?;
        state.apply(action, now_utc_ms).map_err(StoreError::Rules)?;
        write_state(&tx, &state)?;
        tx.execute(
            "INSERT INTO care_events(request_id, action, occurred_utc_ms) VALUES (?1, ?2, ?3)",
            params![request_id, action_name, now_utc_ms],
        )?;
        tx.commit()?;
        Ok(CareOutcome {
            state,
            replayed: false,
        })
    }

    pub fn apply_touch(
        &mut self,
        host_session: &str,
        event_id: u64,
        pack_id: &str,
        region: HitRegion,
        now_utc_ms: i64,
    ) -> Result<TouchOutcome, StoreError> {
        let event_id = i64::try_from(event_id).map_err(|_| StoreError::InvalidTouchId)?;
        if event_id == 0
            || host_session.is_empty()
            || host_session.len() > 128
            || pack_id.is_empty()
            || pack_id.len() > 128
            || !host_session.bytes().all(|b| (0x21..=0x7e).contains(&b))
            || !pack_id.bytes().all(|b| (0x21..=0x7e).contains(&b))
        {
            return Err(StoreError::InvalidTouchId);
        }
        let tx = self.connection.transaction()?;
        let previous: Option<(String, String, String, i64)> = tx.query_row(
            "SELECT pack_id, region, decision_json, occurred_utc_ms FROM touch_events WHERE host_session = ?1 AND event_id = ?2",
            params![host_session, event_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        ).optional()?;
        if let Some((previous_pack, previous_region, json, occurred_utc_ms)) = previous {
            if previous_pack != pack_id
                || previous_region != serde_json::to_string(&region).map_err(StoreError::Json)?
            {
                return Err(StoreError::InvalidTouchId);
            }
            return Ok(TouchOutcome {
                state: read_state(&tx)?,
                decision: serde_json::from_str(&json).map_err(StoreError::Json)?,
                occurred_utc_ms,
                replayed: true,
            });
        }
        let mut state = read_state(&tx)?;
        let last_touch: Option<i64> = tx.query_row(
            "SELECT MAX(occurred_utc_ms) FROM touch_events WHERE pack_id = ?1",
            [pack_id],
            |row| row.get(0),
        )?;
        let effective_now = now_utc_ms
            .max(state.observed_utc_ms)
            .max(last_touch.unwrap_or(i64::MIN));
        state.advance(effective_now);
        let mut history = TouchHistory::default();
        let mut query = tx.prepare(
            "SELECT region, occurred_utc_ms, rule_id, intimacy_delta FROM touch_events
            WHERE pack_id = ?1 AND occurred_utc_ms >= ?2 ORDER BY occurred_utc_ms, event_id",
        )?;
        let rows = query.query_map(
            params![pack_id, effective_now.saturating_sub(10 * 60_000)],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i16>(3)?,
                ))
            },
        )?;
        for row in rows {
            let (region_json, occurred_ms, rule_id, intimacy_delta) = row?;
            history.recent.push(PastTouch {
                region: serde_json::from_str(&region_json).map_err(StoreError::Json)?,
                occurred_ms,
                rule_id,
                intimacy_delta,
            });
        }
        drop(query);
        let day_start = effective_now.div_euclid(86_400_000) * 86_400_000;
        let daily_positive: i64 = tx.query_row(
            "SELECT COALESCE(SUM(CASE WHEN intimacy_delta > 0 THEN intimacy_delta ELSE 0 END), 0)
             FROM touch_events WHERE occurred_utc_ms >= ?1",
            [day_start],
            |row| row.get(0),
        )?;
        history.daily_positive = daily_positive.clamp(0, u8::MAX as i64) as u8;
        let mut decision = select_touch_response(region, state.needs, &history, effective_now);
        decision.apply_intimacy(&mut state.needs);
        write_state(&tx, &state)?;
        tx.execute("INSERT INTO touch_events(host_session, event_id, pack_id, region, occurred_utc_ms, rule_id, intimacy_delta, decision_json)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)", params![
            host_session, event_id, pack_id, serde_json::to_string(&region).map_err(StoreError::Json)?,
            effective_now, decision.rule_id, decision.intimacy_delta,
            serde_json::to_string(&decision).map_err(StoreError::Json)?,
        ])?;
        tx.commit()?;
        Ok(TouchOutcome {
            state,
            decision,
            occurred_utc_ms: effective_now,
            replayed: false,
        })
    }
}

fn checked_u8(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<u8> {
    let n: i64 = row.get(index)?;
    u8::try_from(n).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(index, n))
}

fn read_state(tx: &Transaction<'_>) -> rusqlite::Result<CareState> {
    let (needs, observed_utc_ms, decay_remainder_ms, last_play_utc_ms) = tx.query_row(
        "SELECT satiety, energy, mood, intimacy, observed_utc_ms, decay_remainder_ms, last_play_utc_ms FROM pet_state WHERE id = 1",
        [], |row| Ok((
            Needs { satiety: checked_u8(row, 0)?, energy: checked_u8(row, 1)?, mood: checked_u8(row, 2)?, intimacy: checked_u8(row, 3)? },
            row.get(4)?, row.get(5)?, row.get(6)?
        ))
    )?;
    let food: i64 = tx.query_row(
        "SELECT quantity FROM inventory WHERE item = 'food'",
        [],
        |row| row.get(0),
    )?;
    let food =
        u32::try_from(food).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, food))?;
    Ok(CareState {
        needs,
        food,
        observed_utc_ms,
        decay_remainder_ms,
        last_play_utc_ms,
    })
}

fn write_state(tx: &Transaction<'_>, state: &CareState) -> rusqlite::Result<()> {
    tx.execute("INSERT INTO pet_state(id, satiety, energy, mood, intimacy, observed_utc_ms, decay_remainder_ms, last_play_utc_ms)
        VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7)
        ON CONFLICT(id) DO UPDATE SET satiety=excluded.satiety, energy=excluded.energy,
        mood=excluded.mood, intimacy=excluded.intimacy, observed_utc_ms=excluded.observed_utc_ms,
        decay_remainder_ms=excluded.decay_remainder_ms, last_play_utc_ms=excluded.last_play_utc_ms",
        params![state.needs.satiety, state.needs.energy, state.needs.mood, state.needs.intimacy,
            state.observed_utc_ms, state.decay_remainder_ms, state.last_play_utc_ms])?;
    tx.execute(
        "INSERT INTO inventory(item, quantity) VALUES ('food', ?1)
        ON CONFLICT(item) DO UPDATE SET quantity=excluded.quantity",
        [state.food],
    )?;
    Ok(())
}
