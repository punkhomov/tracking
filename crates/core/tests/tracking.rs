use chrono::{DateTime, Duration, Utc};
use tracking_core::{Dataset, Kind, Settings, Timer, export_json, import_json, summarize};

fn time(text: &str) -> DateTime<Utc> {
    text.parse().unwrap()
}

#[test]
fn pauses_across_restart_and_report_boundaries() {
    let start = time("2026-11-20T23:55:00Z");
    let settings = Settings::default();
    let mut timer = Timer::start(Kind::Work, start, "Проверка".into(), None);
    timer.pause(start + Duration::seconds(20));
    timer.resume(start + Duration::seconds(24), &settings);
    timer.pause(start + Duration::minutes(4));

    let stored = serde_json::to_string(&timer).unwrap();
    let mut reopened: Timer = serde_json::from_str(&stored).unwrap();
    assert_eq!(reopened.elapsed_seconds(start + Duration::minutes(8)), 240);
    reopened.resume(start + Duration::minutes(6), &settings);
    let session = reopened
        .finish(start + Duration::minutes(8), &settings)
        .unwrap();

    let midnight = start + Duration::minutes(5);
    assert_eq!(session.pauses.len(), 1);
    assert_eq!(session.duration_millis(), 360_000);
    assert_eq!(
        summarize([&session], (start, midnight)).work_millis,
        240_000
    );
    assert_eq!(
        summarize([&session], (midnight, session.end)).work_millis,
        120_000
    );
}

#[test]
fn reads_legacy_links_and_clips_pauses_left_outside_an_edited_period() {
    let archive = r##"{
      "version": 1,
      "type": "full",
      "exportedAt": "2026-11-20T12:00:00Z",
      "settings": {"pomodoroDuration": 30, "dateFormat": "YYYY-MM-DD"},
      "categories": [{"id": 7, "name": "Работа", "color": "#39548d", "archived": false}],
      "periods": [
        {"id": 7, "start": "2026-11-20T08:00:00Z", "end": "2026-11-20T08:20:00Z",
         "duration": 20, "categoryId": 7, "note": "Встреча",
         "pauseIntervals": [{"start": "2026-11-20T07:59:00Z", "end": "2026-11-20T08:05:00Z"},
                            {"start": "2026-11-20T08:25:00Z", "end": "2026-11-20T08:30:00Z"}]}
      ],
      "timelineZones": [
        {"id": "a", "name": "Утро", "startTime": "08:00", "endTime": "09:30",
         "weekdays": [5], "parentId": null, "linkedCategoryId": 7, "forceFullRangeInTimeline": true},
        {"id": "b", "name": "Встречи", "startTime": "08:30", "endTime": "09:15",
         "weekdays": [5], "parentId": "a", "style": "hatched"}
      ]
    }"##;

    let dataset = import_json(&Dataset::default(), archive).unwrap();
    let (&id, period) = dataset.periods.iter().next().unwrap();
    let &category = dataset.categories.keys().next().unwrap();
    let (&zone_id, zone) = dataset
        .zones
        .iter()
        .find(|(_, zone)| zone.name == "Утро")
        .unwrap();
    let child = dataset
        .zones
        .values()
        .find(|zone| zone.name == "Встречи")
        .unwrap();
    assert_ne!(id, category);
    assert_eq!(period.kind, Kind::Work);
    assert_eq!(period.category, Some(category));
    assert_eq!(period.pauses.len(), 1);
    assert_eq!(period.pauses[0].start, period.start);
    assert_eq!(period.duration_millis(), 900_000);
    assert_eq!(zone.category, Some(category));
    assert_eq!(child.parent, Some(zone_id));
    assert!(zone.starts_on.is_none());
    assert!(zone.ends_on.is_none());
    assert!(zone.excluded_dates.is_empty());
    assert!(zone.full_range);
    assert_eq!(dataset.settings.work_minutes, 30);
    assert_eq!(import_json(&dataset, archive).unwrap(), dataset);
    let backup = export_json(&dataset).unwrap();
    assert_eq!(import_json(&Dataset::default(), &backup).unwrap(), dataset);
}
