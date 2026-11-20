use chrono::{Datelike, Duration, NaiveDate, NaiveTime, TimeZone, Utc};
use tracking_core::{
    ArchiveScope, Category, Command, Dataset, Interval, Kind, Period, Settings, TrackerState, Zone,
    ZoneStyle, export_archive, read_archive,
    reports::{Span, hourly_alignment, report, zone_breakdown},
};
use uuid::Uuid;

fn date() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, 21).unwrap()
}

fn time(hour: u32, minute: u32) -> chrono::DateTime<Utc> {
    Utc.from_utc_datetime(&date().and_hms_opt(hour, minute, 0).unwrap())
}

fn period(category: Option<Uuid>) -> Period {
    Period {
        start: time(9, 0),
        end: time(12, 0),
        note: "Разработка".into(),
        category,
        kind: Kind::Work,
        pauses: vec![Interval {
            start: time(10, 15),
            end: time(10, 45),
        }],
    }
}

fn zone(category: Option<Uuid>) -> Zone {
    Zone {
        name: "Рабочее окно".into(),
        start: NaiveTime::from_hms_opt(10, 0, 0).unwrap(),
        end: NaiveTime::from_hms_opt(11, 0, 0).unwrap(),
        weekdays: vec![date().weekday().num_days_from_sunday() as u8],
        parent: None,
        category,
        style: ZoneStyle::Solid,
        color: Some("#3b82f6".into()),
        full_range: false,
        exclude_from_breakdown: false,
    }
}

#[test]
fn reopening_reset_and_finishing_never_duplicate_or_lose_the_saved_session() {
    let mut state = TrackerState::default();
    let id = Uuid::new_v4();
    let original = period(None);
    state
        .execute(Command::SavePeriod(id, original.clone()), time(12, 0))
        .unwrap();
    state
        .execute(Command::ReopenSession(id), time(12, 5))
        .unwrap();
    state.execute(Command::ResetTimer, time(12, 6)).unwrap();
    assert_eq!(state.dataset.periods[&id], original);

    state
        .execute(Command::ReopenSession(id), time(12, 7))
        .unwrap();
    state.execute(Command::ResumeTimer, time(12, 8)).unwrap();
    state.execute(Command::FinishSession, time(12, 20)).unwrap();
    assert_eq!(state.dataset.periods.len(), 1);
    assert_eq!(state.dataset.periods[&id].duration_millis(), 162 * 60_000);
}

#[test]
fn scoped_archives_apply_only_the_selected_sections() {
    let old_id = Uuid::new_v4();
    let new_id = Uuid::new_v4();
    let mut current = Dataset::default();
    current.periods.insert(old_id, period(None));
    current.settings.work_minutes = 37;
    let mut source = Dataset::default();
    source.periods.insert(new_id, period(None));
    source.zones.insert(Uuid::new_v4(), zone(None));

    let history = export_archive(&source, ArchiveScope::Periods).unwrap();
    let append = read_archive(&history)
        .unwrap()
        .apply(&current, false)
        .unwrap();
    assert_eq!(append.periods.len(), 2);
    assert_eq!(append.settings.work_minutes, 37);
    assert!(append.zones.is_empty());
    let replace = read_archive(&history)
        .unwrap()
        .apply(&append, true)
        .unwrap();
    assert_eq!(replace.periods, source.periods);

    let preferences = export_archive(&source, ArchiveScope::Settings).unwrap();
    let settings = read_archive(&preferences)
        .unwrap()
        .apply(&current, true)
        .unwrap();
    assert_eq!(settings.periods, current.periods);
    assert_eq!(settings.zones, source.zones);
    assert_eq!(settings.settings, source.settings);
}

#[test]
fn nested_zones_validate_boundaries_and_are_removed_with_the_parent() {
    let mut state = TrackerState::default();
    let parent_id = Uuid::new_v4();
    let child_id = Uuid::new_v4();
    let parent = zone(None);
    let mut child = Zone {
        parent: Some(parent_id),
        ..parent.clone()
    };
    state
        .execute(Command::SaveZone(parent_id, parent), time(9, 0))
        .unwrap();
    child.end = NaiveTime::from_hms_opt(12, 0, 0).unwrap();
    assert!(
        state
            .execute(Command::SaveZone(child_id, child.clone()), time(9, 0))
            .is_err()
    );
    child.end = NaiveTime::from_hms_opt(10, 30, 0).unwrap();
    state
        .execute(Command::SaveZone(child_id, child), time(9, 0))
        .unwrap();
    assert_eq!(state.dataset.zones[&child_id].style, ZoneStyle::Hatched);
    state
        .execute(Command::DeleteZone(parent_id), time(9, 0))
        .unwrap();
    assert!(state.dataset.zones.is_empty());
}

#[test]
fn planned_time_and_alignment_account_for_pauses() {
    let category_id = Uuid::new_v4();
    let zone_id = Uuid::new_v4();
    let mut state = TrackerState::default();
    state
        .execute(
            Command::SaveCategory(
                category_id,
                Category {
                    name: "Проект".into(),
                    color: "#3b82f6".into(),
                    archived: false,
                },
            ),
            time(9, 0),
        )
        .unwrap();
    state
        .execute(
            Command::SaveZone(zone_id, zone(Some(category_id))),
            time(9, 0),
        )
        .unwrap();
    state
        .execute(
            Command::SavePeriod(Uuid::new_v4(), period(Some(category_id))),
            time(12, 0),
        )
        .unwrap();
    state
        .execute(Command::ArchiveCategory(category_id, true), time(12, 1))
        .unwrap();

    let breakdown = zone_breakdown(&state.dataset, date(), &Utc);
    assert_eq!(breakdown[&Some(zone_id)].planned_ms, 60 * 60_000);
    assert_eq!(
        breakdown[&Some(zone_id)].by_category[&Some(category_id)],
        30 * 60_000
    );
    assert_eq!(
        breakdown[&None].by_category[&Some(category_id)],
        120 * 60_000
    );

    let report = report(&state.dataset, date(), Span::Day, Some(Kind::Work), &Utc);
    assert_eq!(report.sessions, 1);
    assert_eq!(report.counts[9], 1);
    assert_eq!(report.active_ms, 150 * 60_000);
    assert_eq!(report.alignment.ratio(), Some(1.0));
    assert_eq!(
        hourly_alignment(&state.dataset, date(), &Utc)[10].matched_ms,
        30 * 60_000
    );
}

#[test]
fn reminders_repeat_only_while_running_and_finishing_can_start_the_break() {
    let mut state = TrackerState::default();
    state
        .execute(
            Command::SaveSettings(Settings {
                work_minutes: 1,
                reminder_interval_minutes: 1,
                auto_start_break: true,
                ..Settings::default()
            }),
            time(9, 0),
        )
        .unwrap();
    let start = time(9, 0);
    state
        .execute(
            Command::StartTimer {
                note: String::new(),
                category: None,
            },
            start,
        )
        .unwrap();
    assert!(state.reminder(start + Duration::seconds(59)).is_none());
    assert_eq!(state.reminder(time(9, 1)).unwrap().occurrence, 0);
    assert_eq!(state.reminder(time(9, 2)).unwrap().occurrence, 1);
    state.execute(Command::PauseTimer, time(9, 3)).unwrap();
    assert!(state.reminder(time(9, 4)).is_none());
    state.execute(Command::FinishSession, time(9, 5)).unwrap();
    assert_eq!(state.timer.unwrap().kind, Kind::Break);
}
