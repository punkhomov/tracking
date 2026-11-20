use chrono::{Duration, TimeZone, Utc};
use tracking_core::{Command, Kind, TrackerState};

#[test]
fn finishing_a_restored_timer_records_one_session_and_advances_mode() {
    let start = Utc.with_ymd_and_hms(2026, 11, 20, 10, 0, 0).unwrap();
    let mut state = TrackerState::default();
    state
        .execute(
            Command::StartTimer {
                note: "Ревью".into(),
                category: None,
            },
            start,
        )
        .unwrap();
    state
        .execute(Command::PauseTimer, start + Duration::seconds(20))
        .unwrap();

    let mut restored: TrackerState =
        serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
    restored
        .execute(Command::ResumeTimer, start + Duration::seconds(40))
        .unwrap();
    restored
        .execute(Command::FinishSession, start + Duration::seconds(60))
        .unwrap();

    assert_eq!(restored.dataset.periods.len(), 1);
    let session = restored.dataset.periods.values().next().unwrap();
    assert_eq!(session.duration_millis(), 40_000);
    assert_eq!(session.note, "Ревью");
    assert!(restored.timer.is_none());
    assert_eq!(restored.next_kind, Kind::Break);
}

#[test]
fn a_rejected_finish_keeps_the_current_timer() {
    let now = Utc.with_ymd_and_hms(2026, 11, 20, 10, 0, 0).unwrap();
    let mut state = TrackerState::default();
    state
        .execute(
            Command::StartTimer {
                note: String::new(),
                category: None,
            },
            now,
        )
        .unwrap();
    let original = state.clone();

    assert!(state.execute(Command::FinishSession, now).is_err());
    assert_eq!(state, original);
}
