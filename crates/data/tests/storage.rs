use tracking_core::{Kind, Timer};
use tracking_data::Database;

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
