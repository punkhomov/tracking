use tracking_core::{Category, Dataset, Kind, Timer};
use tracking_data::{Database, Preference, merge};
use uuid::Uuid;

#[test]
fn local_save_is_atomic_and_rejects_writes_from_a_stale_window() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tracking.sqlite3");
    let (mut first, mut state) = Database::open(&path).unwrap();
    let (mut stale, mut old_state) = Database::open(&path).unwrap();
    state.timer = Some(Timer::start(
        Kind::Work,
        chrono::Utc::now(),
        "Встреча".into(),
        None,
    ));
    first.save(&state).unwrap();
    old_state.next_kind = Kind::Break;
    assert!(stale.save(&old_state).is_err());
    let (_, read) = Database::open(&path).unwrap();
    assert_eq!(read.timer, state.timer);
    assert_eq!(read.next_kind, Kind::Work);
}

#[test]
fn independent_changes_and_deletes_merge_and_same_record_changes_conflict() {
    let mut base = Dataset::default();
    let id = Uuid::new_v4();
    let other = Uuid::new_v4();
    let category = Category {
        name: "Работа".into(),
        color: "#39548d".into(),
        archived: false,
    };
    base.categories.insert(id, category.clone());
    let mut local = base.clone();
    let mut remote = base.clone();
    local.categories.remove(&id);
    remote.categories.insert(other, category.clone());
    let merged = merge(&base, &local, &remote, Preference::Ask).unwrap();
    assert!(!merged.categories.contains_key(&id));
    assert_eq!(merged.categories.get(&other), Some(&category));

    remote.categories.get_mut(&id).unwrap().name = "Обновление".into();
    assert_eq!(
        merge(&base, &local, &remote, Preference::Ask)
            .unwrap_err()
            .count,
        1
    );
    assert_eq!(
        merge(&base, &local, &remote, Preference::Local).unwrap(),
        merged
    );
    assert_eq!(
        merge(&base, &local, &remote, Preference::Remote).unwrap(),
        remote
    );
}
