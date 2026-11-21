use anyhow::{Result, ensure};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{Interval, Kind, Period, Reminder, Settings};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Timer {
    pub id: Uuid,
    pub kind: Kind,
    pub start: DateTime<Utc>,
    pub note: String,
    pub category: Option<Uuid>,
    pauses: Vec<Interval>,
    paused_at: Option<DateTime<Utc>>,
}

impl Timer {
    pub fn start(kind: Kind, now: DateTime<Utc>, note: String, category: Option<Uuid>) -> Self {
        Self {
            id: Uuid::new_v4(),
            kind,
            start: now,
            note,
            category,
            pauses: Vec::new(),
            paused_at: None,
        }
    }

    pub fn reopen(id: Uuid, period: &Period) -> Self {
        Self {
            id,
            kind: period.kind,
            start: period.start,
            note: period.note.clone(),
            category: period.category,
            pauses: period.pauses.clone(),
            paused_at: Some(period.end),
        }
    }

    pub fn is_paused(&self) -> bool {
        self.paused_at.is_some()
    }

    pub fn pause(&mut self, now: DateTime<Utc>) {
        if !self.is_paused() {
            self.paused_at = Some(self.checked_time(now));
        }
    }

    pub fn resume(&mut self, now: DateTime<Utc>, settings: &Settings) {
        let now = self.checked_time(now);
        if let Some(start) = self.paused_at.take()
            && now > start
            && (now - start).num_seconds() >= i64::from(settings.min_pause_seconds)
        {
            self.pauses.push(Interval { start, end: now });
        }
    }

    fn checked_time(&self, now: DateTime<Utc>) -> DateTime<Utc> {
        now.max(
            self.paused_at
                .unwrap_or_else(|| self.pauses.last().map_or(self.start, |last| last.end)),
        )
    }

    pub fn elapsed_seconds(&self, now: DateTime<Utc>) -> i64 {
        let end = self.paused_at.unwrap_or_else(|| self.checked_time(now));
        let paused: i64 = self
            .pauses
            .iter()
            .map(|pause| (pause.end - pause.start).num_milliseconds())
            .sum();
        ((end - self.start).num_milliseconds() - paused).max(0) / 1_000
    }

    pub fn reminder(&self, now: DateTime<Utc>, settings: &Settings) -> Option<Reminder> {
        let deadline = settings.target_seconds(self.kind);
        let over = self.elapsed_seconds(now) - deadline;
        (!self.is_paused() && over >= 0).then(|| Reminder {
            timer: self.id,
            kind: self.kind,
            deadline,
            occurrence: over / (i64::from(settings.reminder_interval_minutes.max(1)) * 60),
        })
    }

    pub fn preview(&self, now: DateTime<Utc>) -> Period {
        let end = self.paused_at.unwrap_or_else(|| self.checked_time(now));
        Period {
            start: self.start,
            end,
            note: self.note.clone(),
            category: self.category,
            kind: self.kind,
            pauses: self.pauses.clone(),
        }
    }
    pub fn finish(mut self, now: DateTime<Utc>, settings: &Settings) -> Result<Period> {
        let end = self.checked_time(now);
        ensure!(end > self.start, "Сессия только началась.");
        self.resume(end, settings);
        let period = Period {
            start: self.start,
            end,
            note: self.note,
            category: self.category,
            kind: self.kind,
            pauses: self.pauses,
        };
        period.validate()?;
        Ok(period)
    }
}
