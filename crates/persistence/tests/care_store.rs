use pet_core::care::CareAction;
use pet_persistence::{CareStore, StoreError};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct TestDir(PathBuf);
impl TestDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "desktop-pet-care-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}
impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const HOUR: i64 = 3_600_000;

#[test]
fn duplicate_feed_survives_reopen_and_conflicting_id_is_rejected() {
    let directory = TestDir::new();
    let path = directory.path().join("pet.sqlite");
    let mut store = CareStore::open(&path, 0).unwrap();
    let first = store.apply("feed-001", CareAction::Feed, 0).unwrap();
    assert!(!first.replayed);
    assert_eq!(first.state.food, 2);
    drop(store);

    let mut store = CareStore::open(&path, 0).unwrap();
    let duplicate = store.apply("feed-001", CareAction::Feed, 0).unwrap();
    assert!(duplicate.replayed);
    assert_eq!(duplicate.state.food, 2);
    assert!(matches!(
        store.apply("feed-001", CareAction::Rest, 0),
        Err(StoreError::RequestIdConflict)
    ));
    assert_eq!(store.load_at(0).unwrap().food, 2);
}

#[test]
fn rejected_action_rolls_back_and_later_requests_still_work() {
    let directory = TestDir::new();
    let path = directory.path().join("pet.sqlite");
    let mut store = CareStore::open(&path, 0).unwrap();
    for n in 0..3 {
        store
            .apply(&format!("feed-{n}"), CareAction::Feed, 0)
            .unwrap();
    }
    let before = store.load_at(0).unwrap();
    assert!(matches!(
        store.apply("feed-4", CareAction::Feed, HOUR),
        Err(StoreError::Rules(_))
    ));
    assert_eq!(store.load_at(0).unwrap(), before);
    let rest = store.apply("feed-4", CareAction::Rest, 0).unwrap();
    assert!(!rest.replayed);
    assert_eq!(rest.state.food, 0);
}

#[test]
fn offline_decay_is_capped_once_and_rollback_does_not_undo_state() {
    let directory = TestDir::new();
    let path = directory.path().join("pet.sqlite");
    let mut store = CareStore::open(&path, 0).unwrap();
    let after = store.load_at(72 * HOUR).unwrap();
    assert_eq!(after.needs.satiety, 27); // 75 - 24 * 2
    assert_eq!(after.needs.energy, 51);
    assert_eq!(store.load_at(12 * HOUR).unwrap(), after);
    drop(store);
    let mut store = CareStore::open(&path, 72 * HOUR).unwrap();
    assert_eq!(store.load_at(72 * HOUR).unwrap(), after);
    assert_eq!(store.load_at(73 * HOUR).unwrap().needs.satiety, 25);
}

#[test]
fn short_observations_keep_fractional_decay_and_play_cooldown_survives_clock_change() {
    let directory = TestDir::new();
    let mut store = CareStore::open(directory.path().join("pet.sqlite"), 0).unwrap();
    for n in 1..=4 {
        store.load_at(n * HOUR / 4).unwrap();
    }
    assert_eq!(store.load_at(HOUR).unwrap().needs.satiety, 73);
    store.apply("play-a", CareAction::Play, HOUR).unwrap();
    assert!(matches!(
        store.apply("play-b", CareAction::Play, HOUR - 1000),
        Err(StoreError::Rules(_))
    ));
    assert!(matches!(
        store.apply("play-b", CareAction::Play, HOUR + 1000),
        Err(StoreError::Rules(_))
    ));
    store
        .apply("play-b", CareAction::Play, HOUR + 600_000)
        .unwrap();
}

#[test]
fn food_refills_once_on_new_utc_day_even_with_frequent_reads() {
    let directory = TestDir::new();
    let mut store = CareStore::open(directory.path().join("pet.sqlite"), 0).unwrap();
    for n in 0..3 {
        store
            .apply(&format!("feed-{n}"), CareAction::Feed, 0)
            .unwrap();
    }
    assert_eq!(store.load_at(23 * HOUR).unwrap().food, 0);
    assert_eq!(store.load_at(24 * HOUR).unwrap().food, 3);
    store
        .apply("feed-next", CareAction::Feed, 24 * HOUR)
        .unwrap();
    assert_eq!(store.load_at(25 * HOUR).unwrap().food, 2);
    assert_eq!(store.load_at(HOUR).unwrap().food, 2);
}

#[test]
fn large_clock_correction_rebases_play_cooldown_without_changing_needs() {
    let directory = TestDir::new();
    let mut store = CareStore::open(directory.path().join("pet.sqlite"), 0).unwrap();
    let played = store
        .apply("play-forward", CareAction::Play, 72 * HOUR)
        .unwrap();
    let rolled_back = store.load_at(HOUR).unwrap();
    assert_eq!(rolled_back.needs, played.state.needs);
    assert_eq!(rolled_back.last_play_utc_ms, Some(HOUR));
    assert!(matches!(
        store.apply("play-again", CareAction::Play, HOUR + 1000),
        Err(StoreError::Rules(_))
    ));
    store
        .apply("play-again", CareAction::Play, HOUR + 600_000)
        .unwrap();
}

#[test]
fn companion_settings_survive_reopen() {
    let directory = TestDir::new();
    let path = directory.path().join("pet.sqlite");
    let mut store = CareStore::open(&path, 0).unwrap();
    store
        .set_setting("companion", r#"{"enabled":true}"#)
        .unwrap();
    drop(store);
    let store = CareStore::open(&path, 0).unwrap();
    assert_eq!(
        store.setting("companion").unwrap().as_deref(),
        Some(r#"{"enabled":true}"#)
    );
}
