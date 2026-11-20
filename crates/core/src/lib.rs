mod archive;
pub mod reports;
mod state;
mod timer;

pub use archive::{
    ArchiveScope, ImportArchive, export_archive, export_json, import_json, read_archive,
};
pub use state::{Command, Reminder, TrackerState};
pub use timer::Timer;

use std::collections::BTreeMap;

use anyhow::{Result, ensure};
use chrono::{DateTime, NaiveTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub type Moments = (DateTime<Utc>, DateTime<Utc>);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    #[default]
    Work,
    Break,
}

impl Kind {
    pub fn next(self) -> Self {
        match self {
            Self::Work => Self::Break,
            Self::Break => Self::Work,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Interval {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Period {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub note: String,
    pub category: Option<Uuid>,
    pub kind: Kind,
    pub pauses: Vec<Interval>,
}

impl Period {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.start <= self.end, "Конец сессии раньше начала.");
        let mut cursor = self.start;
        for pause in &self.pauses {
            ensure!(
                cursor <= pause.start && pause.start < pause.end && pause.end <= self.end,
                "Паузы должны быть внутри сессии и идти по порядку."
            );
            cursor = pause.end;
        }
        Ok(())
    }

    pub fn segments(&self) -> impl Iterator<Item = Moments> + '_ {
        let starts = std::iter::once(self.start).chain(self.pauses.iter().map(|pause| pause.end));
        let ends = self
            .pauses
            .iter()
            .map(|pause| pause.start)
            .chain([self.end]);
        starts.zip(ends)
    }

    pub fn active_millis(&self, range: Moments) -> i64 {
        self.segments()
            .map(|(start, end)| {
                (end.min(range.1) - start.max(range.0))
                    .num_milliseconds()
                    .max(0)
            })
            .sum()
    }

    pub fn duration_millis(&self) -> i64 {
        self.active_millis((self.start, self.end))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Category {
    pub name: String,
    pub color: String,
    #[serde(default)]
    pub archived: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ZoneStyle {
    #[default]
    Solid,
    Border,
    Hatched,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Zone {
    pub name: String,
    pub start: NaiveTime,
    pub end: NaiveTime,
    pub weekdays: Vec<u8>,
    pub parent: Option<Uuid>,
    pub color: Option<String>,
    pub style: ZoneStyle,
    pub full_range: bool,
    pub exclude_from_breakdown: bool,
    pub category: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    #[serde(rename = "pomodoroDuration")]
    pub work_minutes: u32,
    #[serde(rename = "breakDuration")]
    pub break_minutes: u32,
    pub reminder_interval_minutes: u32,
    #[serde(rename = "minPauseDurationSeconds")]
    pub min_pause_seconds: u32,
    pub auto_start_break: bool,
    pub auto_start_pomodoro: bool,
    pub first_day_of_week: String,
    pub date_format: String,
    pub hour_format: String,
    pub theme: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            work_minutes: 25,
            break_minutes: 5,
            reminder_interval_minutes: 5,
            min_pause_seconds: 10,
            auto_start_break: false,
            auto_start_pomodoro: false,
            first_day_of_week: "monday".into(),
            date_format: "DD.MM.YYYY".into(),
            hour_format: "24h".into(),
            theme: "system".into(),
        }
    }
}

impl Settings {
    pub fn target_seconds(&self, kind: Kind) -> i64 {
        60 * i64::from(match kind {
            Kind::Work => self.work_minutes,
            Kind::Break => self.break_minutes,
        })
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.work_minutes > 0 && self.break_minutes > 0 && self.reminder_interval_minutes > 0,
            "Длительность и интервал напоминаний должны быть больше нуля."
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dataset {
    pub periods: BTreeMap<Uuid, Period>,
    pub categories: BTreeMap<Uuid, Category>,
    pub zones: BTreeMap<Uuid, Zone>,
    pub settings: Settings,
}

impl Dataset {
    pub fn validate(&self) -> Result<()> {
        self.settings.validate()?;
        for period in self.periods.values() {
            period.validate()?;
        }
        for category in self.categories.values() {
            ensure!(
                !category.name.trim().is_empty(),
                "У категории должно быть название."
            );
        }
        for (id, zone) in &self.zones {
            ensure!(
                !zone.name.trim().is_empty()
                    && zone.start < zone.end
                    && !zone.weekdays.is_empty()
                    && zone.weekdays.iter().all(|&day| day < 7)
                    && zone.parent != Some(*id)
                    && zone
                        .parent
                        .and_then(|parent| self.zones.get(&parent))
                        .is_none_or(|parent| parent.parent.is_none()),
                "Проверьте время, дни недели и родительскую зону в «{}».",
                zone.name
            );
        }
        Ok(())
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub work_millis: i64,
    pub break_millis: i64,
    pub work_count: usize,
    pub break_count: usize,
    pub by_category: BTreeMap<Option<Uuid>, i64>,
}

pub fn summarize<'a>(periods: impl IntoIterator<Item = &'a Period>, range: Moments) -> Summary {
    let mut summary = Summary::default();
    for period in periods {
        if period.start >= range.1 || period.end <= range.0 {
            continue;
        }
        let elapsed = period.active_millis(range);
        match period.kind {
            Kind::Work => {
                summary.work_millis += elapsed;
                summary.work_count += 1;
                *summary.by_category.entry(period.category).or_default() += elapsed;
            }
            Kind::Break => {
                summary.break_millis += elapsed;
                summary.break_count += 1;
            }
        }
    }
    summary
}
