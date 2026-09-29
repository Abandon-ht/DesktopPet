use pet_persistence::{CareStore, MemoryStore};

#[test]
fn existing_version_two_care_archive_migrates_without_losing_pet_state() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("care.sqlite3");
    let initial = CareStore::open(&path, 1_000)
        .unwrap()
        .load_at(1_000)
        .unwrap();
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch(
        "DROP TABLE memory_turns; DROP TABLE memory_items; DROP TABLE memory_state;
        DELETE FROM schema_migrations WHERE version = 3; PRAGMA user_version = 2;",
    )
    .unwrap();
    drop(db);
    let mut restored = CareStore::open(&path, 1_000).unwrap();
    assert_eq!(restored.load_at(1_000).unwrap(), initial);
    assert!(!MemoryStore::open(&path).unwrap().enabled().unwrap());
}

#[test]
fn migrates_care_database_and_keeps_suggestions_out_of_replies_until_confirmed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("care.sqlite3");
    CareStore::open(&path, 1_000).unwrap();
    let mut store = MemoryStore::open(&path).unwrap();
    assert!(!store.enabled().unwrap());
    assert!(
        store
            .append_turn("我喜欢茶", "知道了", 2_000)
            .unwrap()
            .is_none()
    );
    store.set_enabled(true).unwrap();
    let epoch = store.epoch().unwrap();
    store
        .append_turn("明天继续聊去海边", "好，明天接着聊", 1_500)
        .unwrap();
    assert!(store.recall("海边").unwrap().contains("明天继续聊去海边"));
    store.suggest("用户喜欢热茶", 2_000, epoch).unwrap();
    assert!(!store.recall("热茶").unwrap().contains("用户喜欢热茶"));
    let item = store.snapshot().unwrap().items.remove(0);
    store
        .update_item(item.id, &item.content, true, true, 3_000)
        .unwrap();
    assert!(store.recall("热茶").unwrap().contains("用户喜欢热茶"));
    store.set_summary("下次继续聊海边").unwrap();
    assert!(store.recall("海边").unwrap().contains("下次继续聊海边"));
    assert!(!store.suggest("过期摘要任务", 3_000, epoch).unwrap());
    store.set_enabled(false).unwrap();
    assert!(store.recall("热茶").unwrap().is_empty());
    assert!(!store.suggest("过期任务", 4_000, epoch).unwrap());
}

#[test]
fn forgetting_and_clearing_remove_all_retrievable_sources_and_invalidate_jobs() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("care.sqlite3");
    CareStore::open(&path, 1_000).unwrap();
    let mut store = MemoryStore::open(&path).unwrap();
    store.set_enabled(true).unwrap();
    let epoch = store.epoch().unwrap();
    store.add_user_item("用户喜欢热茶", 2_000).unwrap();
    store.append_turn("我喜欢茶", "知道了", 2_000).unwrap();
    let turn = store.pending_extraction().unwrap().unwrap();
    store.mark_extracted(turn.id, epoch).unwrap();
    store.compact("正在讨论喝茶", turn.id, epoch).unwrap();
    let item = store.snapshot().unwrap().items.remove(0);
    assert!(store.forget_item(item.id).unwrap());
    let snapshot = store.snapshot().unwrap();
    assert!(snapshot.summary.is_empty());
    assert!(snapshot.items.is_empty());
    assert_eq!(snapshot.uncompressed_turns, 0);
    assert!(!store.suggest("过期任务", 3_000, epoch).unwrap());
    store.add_user_item("新记忆", 4_000).unwrap();
    store.clear().unwrap();
    assert!(store.snapshot().unwrap().items.is_empty());
}

#[test]
fn two_workers_cannot_overwrite_the_same_compaction() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("care.sqlite3");
    CareStore::open(&path, 1_000).unwrap();
    let mut first = MemoryStore::open(&path).unwrap();
    let mut second = MemoryStore::open(&path).unwrap();
    first.set_enabled(true).unwrap();
    let epoch = first.epoch().unwrap();
    let (id, _) = first
        .append_turn("早上好", "早上好", 2_000)
        .unwrap()
        .unwrap();
    first.mark_extracted(id, epoch).unwrap();
    assert!(first.compact("第一份摘要", id, epoch).unwrap());
    assert!(!second.compact("过时的摘要", id, epoch).unwrap());
    assert_eq!(second.snapshot().unwrap().summary, "第一份摘要");
}
