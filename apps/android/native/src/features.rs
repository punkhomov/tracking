use anyhow::{Context, Result};
use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use chrono_tz::Tz;
use tracking_core::{
    ArchiveScope, Category, Command, Period, Settings, Zone, ZoneStyle, export_archive,
    read_archive, reports,
};
use uuid::Uuid;

use crate::{SessionKind, Tracker, TrackerError};

#[derive(uniffi::Record)]
pub struct SessionDetails {
    pub note: String,
    pub category_id: Option<String>,
}

#[derive(uniffi::Record)]
pub struct SessionDraft {
    pub id: Option<String>,
    pub kind: SessionKind,
    pub start_millis: i64,
    pub end_millis: i64,
    pub details: SessionDetails,
}

#[derive(uniffi::Record)]
pub struct CategoryView {
    pub id: String,
    pub name: String,
    pub color: String,
    pub archived: bool,
}

#[derive(Clone, Copy, uniffi::Enum)]
pub enum ZoneAppearance {
    Solid,
    Border,
    Hatched,
}

#[derive(uniffi::Record)]
pub struct ZoneDraft {
    pub id: Option<String>,
    pub name: String,
    pub start: String,
    pub end: String,
    pub weekdays: Vec<u8>,
    pub starts_on: Option<String>,
    pub ends_on: Option<String>,
    pub excluded_dates: Vec<String>,
    pub parent: Option<String>,
    pub color: Option<String>,
    pub style: ZoneAppearance,
    pub full_range: bool,
    pub exclude_from_breakdown: bool,
    pub category_id: Option<String>,
}

#[derive(uniffi::Record)]
pub struct SettingsView {
    pub work_minutes: i32,
    pub break_minutes: i32,
    pub reminder_interval_minutes: i32,
    pub min_pause_seconds: i32,
    pub auto_start_break: bool,
    pub auto_start_pomodoro: bool,
    pub first_day_of_week: String,
    pub date_format: String,
    pub hour_format: String,
    pub theme: String,
}

#[derive(uniffi::Record)]
pub struct TimelineEntry {
    pub id: String,
    pub name: String,
    pub start_millis: i64,
    pub end_millis: i64,
    pub color: Option<String>,
    pub planned: bool,
    pub nested: bool,
    pub style: ZoneAppearance,
    pub full_range: bool,
}

#[derive(uniffi::Record)]
pub struct ZoneCategoryView {
    pub name: String,
    pub active_millis: i64,
    pub sessions: i64,
}

#[derive(uniffi::Record)]
pub struct ZoneBreakdownView {
    pub id: String,
    pub name: String,
    pub planned_millis: i64,
    pub categories: Vec<ZoneCategoryView>,
}

#[derive(uniffi::Record)]
pub struct TimelineView {
    pub entries: Vec<TimelineEntry>,
    pub breakdown: Vec<ZoneBreakdownView>,
}

#[derive(uniffi::Enum)]
pub enum ReportSpan {
    Day,
    Week,
    Month,
}

#[derive(uniffi::Record)]
pub struct DayActivity {
    pub date: String,
    pub work_millis: i64,
    pub alignment: Option<f64>,
}

#[derive(uniffi::Record)]
pub struct ReportView {
    pub sessions: i64,
    pub active_millis: i64,
    pub alignment: Option<f64>,
    pub uncategorized_millis: i64,
    pub counts: Vec<i64>,
    pub days: Vec<DayActivity>,
}

#[derive(uniffi::Record)]
pub struct ReminderView {
    pub session_id: String,
    pub kind: SessionKind,
    pub deadline: i64,
    pub occurrence: i64,
}

#[derive(uniffi::Enum)]
pub enum ArchivePart {
    Full,
    History,
    Settings,
}

#[derive(uniffi::Record)]
pub struct ArchiveInfo {
    pub sessions: i64,
    pub categories: i64,
    pub zones: i64,
    pub settings: bool,
}

#[uniffi::export]
impl Tracker {
    pub fn categories(&self) -> Result<Vec<CategoryView>, TrackerError> {
        let store = self.lock()?;
        let mut categories: Vec<_> = store
            .state
            .dataset
            .categories
            .iter()
            .map(|(id, category)| CategoryView {
                id: id.to_string(),
                name: category.name.clone(),
                color: category.color.clone(),
                archived: category.archived,
            })
            .collect();
        categories
            .sort_by(|left, right| (left.archived, &left.name).cmp(&(right.archived, &right.name)));
        Ok(categories)
    }

    pub fn save_category(
        &self,
        id: Option<String>,
        name: String,
        color: String,
    ) -> Result<(), TrackerError> {
        let id = optional_id(id)?.unwrap_or_else(Uuid::new_v4);
        if !(color.starts_with('#')
            && color.len() == 7
            && color.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit))
        {
            return Err(anyhow::anyhow!("Введите цвет в формате #RRGGBB.").into());
        }
        let mut store = self.lock()?;
        let archived = store
            .state
            .dataset
            .categories
            .get(&id)
            .is_some_and(|category| category.archived);
        store.apply(
            Command::SaveCategory(
                id,
                Category {
                    name,
                    color,
                    archived,
                },
            ),
            None,
            Utc::now(),
        )?;
        Ok(())
    }

    pub fn archive_category(&self, id: String, archive: bool) -> Result<(), TrackerError> {
        self.change(Command::ArchiveCategory(parse_id(&id)?, archive), None)
    }

    pub fn save_session(&self, draft: SessionDraft) -> Result<(), TrackerError> {
        let id = optional_id(draft.id)?.unwrap_or_else(Uuid::new_v4);
        let mut store = self.lock()?;
        let pauses = store
            .state
            .dataset
            .periods
            .get(&id)
            .map(|period| period.pauses.clone())
            .unwrap_or_default();
        let period = Period {
            start: instant(draft.start_millis)?,
            end: instant(draft.end_millis)?,
            note: draft.details.note,
            category: optional_id(draft.details.category_id)?,
            kind: draft.kind.into(),
            pauses,
        };
        store.apply(Command::SavePeriod(id, period), None, Utc::now())?;
        Ok(())
    }

    pub fn delete_session(&self, id: String) -> Result<(), TrackerError> {
        self.change(Command::DeletePeriod(parse_id(&id)?), None)
    }

    pub fn clear_history(&self) -> Result<(), TrackerError> {
        self.change(Command::ClearHistory, None)
    }

    pub fn zones(&self) -> Result<Vec<ZoneDraft>, TrackerError> {
        let store = self.lock()?;
        let mut zones: Vec<_> = store
            .state
            .dataset
            .zones
            .iter()
            .map(|(id, zone)| ZoneDraft {
                id: Some(id.to_string()),
                name: zone.name.clone(),
                start: zone.start.format("%H:%M").to_string(),
                end: zone.end.format("%H:%M").to_string(),
                weekdays: zone.weekdays.clone(),
                starts_on: zone.starts_on.map(|date| date.to_string()),
                ends_on: zone.ends_on.map(|date| date.to_string()),
                excluded_dates: zone
                    .excluded_dates
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
                parent: zone.parent.map(|id| id.to_string()),
                color: zone.color.clone(),
                style: zone_style(zone.style),
                full_range: zone.full_range,
                exclude_from_breakdown: zone.exclude_from_breakdown,
                category_id: zone.category.map(|id| id.to_string()),
            })
            .collect();
        zones.sort_by(|left, right| (&left.start, &left.name).cmp(&(&right.start, &right.name)));
        Ok(zones)
    }

    pub fn save_zone(&self, draft: ZoneDraft) -> Result<(), TrackerError> {
        let id = optional_id(draft.id)?.unwrap_or_else(Uuid::new_v4);
        let zone = Zone {
            name: draft.name,
            start: parse_time(&draft.start)?,
            end: parse_time(&draft.end)?,
            weekdays: draft.weekdays,
            starts_on: draft.starts_on.as_deref().map(parse_date).transpose()?,
            ends_on: draft.ends_on.as_deref().map(parse_date).transpose()?,
            excluded_dates: draft
                .excluded_dates
                .iter()
                .map(|date| parse_date(date))
                .collect::<Result<_>>()?,
            parent: optional_id(draft.parent)?,
            color: draft.color,
            style: match draft.style {
                ZoneAppearance::Solid => ZoneStyle::Solid,
                ZoneAppearance::Border => ZoneStyle::Border,
                ZoneAppearance::Hatched => ZoneStyle::Hatched,
            },
            full_range: draft.full_range,
            exclude_from_breakdown: draft.exclude_from_breakdown,
            category: optional_id(draft.category_id)?,
        };
        self.change(Command::SaveZone(id, zone), None)
    }

    pub fn delete_zone(&self, id: String) -> Result<(), TrackerError> {
        self.change(Command::DeleteZone(parse_id(&id)?), None)
    }

    pub fn settings(&self) -> Result<SettingsView, TrackerError> {
        let store = self.lock()?;
        let settings = &store.state.dataset.settings;
        Ok(SettingsView {
            work_minutes: display_number(settings.work_minutes),
            break_minutes: display_number(settings.break_minutes),
            reminder_interval_minutes: display_number(settings.reminder_interval_minutes),
            min_pause_seconds: display_number(settings.min_pause_seconds),
            auto_start_break: settings.auto_start_break,
            auto_start_pomodoro: settings.auto_start_pomodoro,
            first_day_of_week: settings.first_day_of_week.clone(),
            date_format: settings.date_format.clone(),
            hour_format: settings.hour_format.clone(),
            theme: settings.theme.clone(),
        })
    }

    pub fn save_settings(&self, settings: SettingsView) -> Result<(), TrackerError> {
        let settings = Settings {
            work_minutes: number(settings.work_minutes)?,
            break_minutes: number(settings.break_minutes)?,
            reminder_interval_minutes: number(settings.reminder_interval_minutes)?,
            min_pause_seconds: number(settings.min_pause_seconds)?,
            auto_start_break: settings.auto_start_break,
            auto_start_pomodoro: settings.auto_start_pomodoro,
            first_day_of_week: settings.first_day_of_week,
            date_format: settings.date_format,
            hour_format: settings.hour_format,
            theme: settings.theme,
        };
        self.change(Command::SaveSettings(settings), None)
    }

    pub fn timeline(&self, date: String, time_zone: String) -> Result<TimelineView, TrackerError> {
        let date = parse_date(&date)?;
        let tz = time_zone.parse::<Tz>().unwrap_or(chrono_tz::UTC);
        let store = self.lock()?;
        let dataset = &store.state.dataset;
        let range = reports::day_range(date, &tz);
        let categories = &dataset.categories;
        let mut entries: Vec<_> = reports::zone_slices(dataset, date, &tz)
            .into_iter()
            .map(|slice| {
                let zone = &dataset.zones[&slice.id];
                TimelineEntry {
                    id: format!("zone-{}", slice.id),
                    name: zone.name.clone(),
                    start_millis: slice.range.0.timestamp_millis(),
                    end_millis: slice.range.1.timestamp_millis(),
                    color: zone.color.clone().or_else(|| {
                        zone.category
                            .and_then(|id| categories.get(&id))
                            .map(|category| category.color.clone())
                    }),
                    planned: true,
                    nested: zone.parent.is_some(),
                    style: zone_style(zone.style),
                    full_range: zone.full_range,
                }
            })
            .collect();
        let preview = store
            .state
            .timer
            .as_ref()
            .map(|timer| (timer.id, timer.preview(Utc::now())));
        for (id, period) in dataset
            .periods
            .iter()
            .filter(|(id, _)| preview.as_ref().is_none_or(|(active, _)| active != *id))
            .chain(preview.iter().map(|(id, period)| (id, period)))
        {
            let category = period.category.and_then(|id| categories.get(&id));
            for (index, segment) in period.segments().enumerate() {
                if let Some((start, end)) = reports::intersect(segment, range) {
                    entries.push(TimelineEntry {
                        id: format!("session-{id}-{index}"),
                        name: if period.note.is_empty() {
                            category.map_or_else(
                                || match period.kind {
                                    tracking_core::Kind::Work => "Работа".into(),
                                    tracking_core::Kind::Break => "Перерыв".into(),
                                },
                                |category| category.name.clone(),
                            )
                        } else {
                            period.note.clone()
                        },
                        start_millis: start.timestamp_millis(),
                        end_millis: end.timestamp_millis(),
                        color: category.map(|category| category.color.clone()),
                        planned: false,
                        nested: false,
                        style: ZoneAppearance::Solid,
                        full_range: false,
                    });
                }
            }
        }
        entries.sort_by_key(|entry| (entry.start_millis, !entry.planned));
        let breakdown = reports::zone_breakdown(dataset, date, &tz)
            .into_iter()
            .map(|(id, group)| ZoneBreakdownView {
                id: id.map_or_else(|| "none".into(), |id| id.to_string()),
                name: id
                    .and_then(|id| dataset.zones.get(&id))
                    .map_or("Вне зон", |zone| &zone.name)
                    .into(),
                planned_millis: group.planned_ms,
                categories: group
                    .by_category
                    .into_iter()
                    .map(|(id, active_millis)| ZoneCategoryView {
                        name: id
                            .and_then(|id| categories.get(&id))
                            .map_or("Без категории", |category| &category.name)
                            .into(),
                        active_millis,
                        sessions: group.sessions.get(&id).copied().unwrap_or_default() as i64,
                    })
                    .collect(),
            })
            .collect();
        Ok(TimelineView { entries, breakdown })
    }

    pub fn report(
        &self,
        date: String,
        span: ReportSpan,
        kind: Option<SessionKind>,
        time_zone: String,
    ) -> Result<ReportView, TrackerError> {
        let date = parse_date(&date)?;
        let span = match span {
            ReportSpan::Day => reports::Span::Day,
            ReportSpan::Week => reports::Span::Week,
            ReportSpan::Month => reports::Span::Month,
        };
        let tz = time_zone.parse::<Tz>().unwrap_or(chrono_tz::UTC);
        let store = self.lock()?;
        let report = reports::report(&store.state.dataset, date, span, kind.map(Into::into), &tz);
        Ok(ReportView {
            sessions: report.sessions as i64,
            active_millis: report.active_ms,
            alignment: report.alignment.ratio(),
            uncategorized_millis: report.alignment.uncategorized_ms,
            counts: report
                .counts
                .into_iter()
                .map(|count| count as i64)
                .collect(),
            days: report
                .days
                .into_iter()
                .map(|day| DayActivity {
                    date: day.date.to_string(),
                    work_millis: day.work_ms,
                    alignment: day.alignment.ratio(),
                })
                .collect(),
        })
    }

    pub fn reminder(&self) -> Result<Option<ReminderView>, TrackerError> {
        let store = self.lock()?;
        let Some(timer) = &store.state.timer else {
            return Ok(None);
        };
        let settings = &store.state.dataset.settings;
        Ok(timer
            .reminder(Utc::now(), settings)
            .map(|reminder| ReminderView {
                session_id: reminder.timer.to_string(),
                kind: reminder.kind.into(),
                deadline: reminder.deadline,
                occurrence: reminder.occurrence,
            }))
    }

    pub fn archive_info(&self, json: String) -> Result<ArchiveInfo, TrackerError> {
        let archive = read_archive(&json)?;
        Ok(ArchiveInfo {
            sessions: archive.periods.as_ref().map_or(0, |periods| periods.len()) as i64,
            categories: archive
                .categories
                .as_ref()
                .map_or(0, |categories| categories.len()) as i64,
            zones: archive.zones.as_ref().map_or(0, |zones| zones.len()) as i64,
            settings: archive.settings.is_some(),
        })
    }

    pub fn import_archive(&self, json: String, replace: bool) -> Result<(), TrackerError> {
        self.change(
            Command::Import(Box::new(read_archive(&json)?), replace),
            None,
        )
    }

    pub fn export_archive(&self, part: ArchivePart) -> Result<String, TrackerError> {
        let scope = match part {
            ArchivePart::Full => ArchiveScope::Full,
            ArchivePart::History => ArchiveScope::Periods,
            ArchivePart::Settings => ArchiveScope::Settings,
        };
        Ok(export_archive(&self.lock()?.state.dataset, scope)?)
    }
}

pub(super) fn parse_id(id: &str) -> Result<Uuid> {
    Uuid::parse_str(id).context("Некорректный идентификатор записи.")
}

pub(super) fn optional_id(id: Option<String>) -> Result<Option<Uuid>> {
    id.as_deref().map(parse_id).transpose()
}

fn parse_date(value: &str) -> Result<NaiveDate> {
    value
        .trim()
        .parse()
        .context("Введите дату в формате ГГГГ-ММ-ДД.")
}

fn parse_time(value: &str) -> Result<NaiveTime> {
    NaiveTime::parse_from_str(value.trim(), "%H:%M").context("Введите время в формате ЧЧ:ММ.")
}

fn instant(value: i64) -> Result<DateTime<Utc>> {
    DateTime::from_timestamp_millis(value).context("Некорректная дата сессии.")
}

fn number(value: i32) -> Result<u32> {
    u32::try_from(value).context("Введите неотрицательное число.")
}

fn display_number(value: u32) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

fn zone_style(style: ZoneStyle) -> ZoneAppearance {
    match style {
        ZoneStyle::Solid => ZoneAppearance::Solid,
        ZoneStyle::Border => ZoneAppearance::Border,
        ZoneStyle::Hatched => ZoneAppearance::Hatched,
    }
}
