use pet_persistence::{CareStore, StoreError};
use pet_protocol::HitRegion;
use rusqlite::Connection;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct TestDb(PathBuf);
impl TestDb {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "desktop-pet-touch-{}-{}.sqlite",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        Self(path)
    }
}
impl Drop for TestDb {
    fn drop(&mut self) {
        for path in [
            &self.0,
            &self.0.with_extension("sqlite-wal"),
            &self.0.with_extension("sqlite-shm"),
        ] {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[test]
fn boundary_penalty_is_atomic_idempotent_and_persists_across_reopen() {
    let db = TestDb::new();
    let mut store = CareStore::open(&db.0, 100_000).unwrap();
    drop(store);
    let conn = Connection::open(&db.0).unwrap();
    conn.execute("UPDATE pet_state SET intimacy = 90 WHERE id = 1", [])
        .unwrap();
    drop(conn);
    store = CareStore::open(&db.0, 100_000).unwrap();
    let first = store
        .apply_touch("session-a", 1, "pet-a", HitRegion::UpperBody, 100_000)
        .unwrap();
    assert_eq!(
        (first.decision.rule_id.as_str(), first.state.needs.intimacy),
        ("boundary_first", 85)
    );
    let duplicate = store
        .apply_touch("session-a", 1, "pet-a", HitRegion::UpperBody, 100_001)
        .unwrap();
    assert!(duplicate.replayed);
    assert_eq!(duplicate.state.needs.intimacy, 85);
    assert!(matches!(
        store.apply_touch("session-a", 1, "pet-a", HitRegion::LowerBody, 100_001),
        Err(StoreError::InvalidTouchId)
    ));
    let second = store
        .apply_touch("session-a", 2, "pet-a", HitRegion::LowerBody, 110_000)
        .unwrap();
    assert_eq!(
        (
            second.decision.rule_id.as_str(),
            second.state.needs.intimacy
        ),
        ("boundary_second", 70)
    );
    drop(store);
    let mut store = CareStore::open(&db.0, 115_000).unwrap();
    let third = store
        .apply_touch("session-b", 1, "pet-a", HitRegion::UpperBody, 120_000)
        .unwrap();
    assert_eq!(
        (
            third.decision.rule_id.as_str(),
            third.decision.intimacy_delta,
            third.state.needs.intimacy
        ),
        ("boundary_third", -70, 0)
    );
    let fourth = store
        .apply_touch("session-b", 2, "pet-a", HitRegion::LowerBody, 121_000)
        .unwrap();
    assert_eq!(
        (
            fourth.decision.rule_id.as_str(),
            fourth.decision.intimacy_delta
        ),
        ("boundary_withdrawn", 0)
    );
    assert_eq!(store.load_at(121_000).unwrap().needs.intimacy, 0);
}

#[test]
fn normal_gain_respects_cooldown_daily_cap_and_clock_rollback() {
    let db = TestDb::new();
    let mut store = CareStore::open(&db.0, 0).unwrap();
    drop(store);
    let conn = Connection::open(&db.0).unwrap();
    conn.execute("UPDATE pet_state SET intimacy = 30 WHERE id = 1", [])
        .unwrap();
    drop(conn);
    store = CareStore::open(&db.0, 0).unwrap();
    let first = store
        .apply_touch("a", 1, "pet-a", HitRegion::Head, 1_000)
        .unwrap();
    assert_eq!(first.decision.intimacy_delta, 1);
    assert_eq!(
        store
            .apply_touch("a", 2, "pet-a", HitRegion::Head, 2_000)
            .unwrap()
            .decision
            .intimacy_delta,
        0
    );
    let rolled_back = store
        .apply_touch("a", 3, "pet-a", HitRegion::Face, 500)
        .unwrap();
    assert_eq!(rolled_back.occurred_utc_ms, 2_000);
    assert_eq!(rolled_back.decision.intimacy_delta, 1);
    for n in 0..4 {
        let at = 602_000 + n * 601_000;
        let region = if n % 2 == 0 {
            HitRegion::Head
        } else {
            HitRegion::Face
        };
        assert_eq!(
            store
                .apply_touch("a", 4 + n as u64, "pet-a", region, at)
                .unwrap()
                .decision
                .intimacy_delta,
            1
        );
    }
    let capped = store
        .apply_touch("a", 8, "pet-b", HitRegion::Head, 3_100_000)
        .unwrap();
    assert_eq!(capped.decision.intimacy_delta, 0);
}

#[test]
fn version_one_care_database_migrates_without_losing_state() {
    let db = TestDb::new();
    let mut store = CareStore::open(&db.0, 0).unwrap();
    store
        .apply_touch("a", 1, "pet-a", HitRegion::Body, 100)
        .unwrap();
    drop(store);
    let conn = Connection::open(&db.0).unwrap();
    conn.execute_batch("DROP TABLE touch_events; PRAGMA user_version = 1; DELETE FROM schema_migrations WHERE version = 2;").unwrap();
    drop(conn);
    let mut store = CareStore::open(&db.0, 100).unwrap();
    assert_eq!(store.load_at(100).unwrap().needs.intimacy, 20);
    assert_eq!(
        store
            .apply_touch("b", 1, "pet-a", HitRegion::UpperBody, 100)
            .unwrap()
            .state
            .needs
            .intimacy,
        15
    );
}
