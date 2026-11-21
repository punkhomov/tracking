use std::{
    path::Path,
    sync::{Mutex, MutexGuard},
};

use anyhow::Result;
use chrono::{DateTime, Utc};
use tracking_core::{Command, Kind, TrackerState};
use tracking_data::{Database, LocalState};

mod cloud;
mod features;

uniffi::setup_scaffolding!();

#[derive(Debug, thiserror::Error, uniffi::Error)]
#[uniffi(flat_error)]
pub enum TrackerError {
    #[error("{0}")]
    Operation(#[from] anyhow::Error),
}

#[derive(Clone, Copy, uniffi::Enum)]
pub enum SessionKind {
    Work,
    Break,
}

impl From<Kind> for SessionKind {
    fn from(kind: Kind) -> Self {
        match kind {
            Kind::Work => Self::Work,
            Kind::Break => Self::Break,
        }
    }
}

impl From<SessionKind> for Kind {
    fn from(kind: SessionKind) -> Self {
        match kind {
            SessionKind::Work => Self::Work,
            SessionKind::Break => Self::Break,
        }
    }
}

#[derive(PartialEq, Eq, Debug, uniffi::Enum)]
pub enum TimerStatus {
    Ready,
    Running,
    Paused,
}

#[derive(uniffi::Record)]
pub struct TimerView {
    pub session_id: Option<String>,
    pub kind: SessionKind,
    pub status: TimerStatus,
    pub elapsed_seconds: i64,
    pub target_seconds: i64,
    pub note: String,
    pub category_id: Option<String>,
}

#[derive(uniffi::Record)]
pub struct SessionView {
    pub id: String,
    pub kind: SessionKind,
    pub start_millis: i64,
    pub end_millis: i64,
    pub duration_millis: i64,
    pub note: String,
    pub category_id: Option<String>,
}

#[derive(uniffi::Object)]
pub struct Tracker {
    local: Mutex<Store>,
    cloud: Mutex<cloud::Client>,
}

#[uniffi::export]
impl Tracker {
    #[uniffi::constructor]
    pub fn open(path: String) -> Result<Self, TrackerError> {
        let (database, state) = Database::open(Path::new(&path))?;
        Ok(Self {
            local: Mutex::new(Store {
                database,
                state,
                timer_changed: false,
                dataset_changed: false,
            }),
            cloud: Mutex::new(cloud::Client::new()?),
        })
    }

    pub fn timer(&self) -> Result<TimerView, TrackerError> {
        Ok(self.lock()?.timer(Utc::now()))
    }

    pub fn history(&self) -> Result<Vec<SessionView>, TrackerError> {
        let store = self.lock()?;
        let mut sessions: Vec<_> = store.state.dataset.periods.iter().collect();
        sessions.sort_unstable_by_key(|(id, period)| (std::cmp::Reverse(period.start), **id));
        Ok(sessions
            .into_iter()
            .map(|(id, period)| SessionView {
                id: id.to_string(),
                kind: period.kind.into(),
                start_millis: period.start.timestamp_millis(),
                end_millis: period.end.timestamp_millis(),
                duration_millis: period.duration_millis(),
                note: period.note.clone(),
                category_id: period.category.map(|id| id.to_string()),
            })
            .collect())
    }

    pub fn select_kind(&self, kind: SessionKind) -> Result<(), TrackerError> {
        self.change(Command::SelectKind(kind.into()), None)
    }

    pub fn start(&self, details: features::SessionDetails) -> Result<(), TrackerError> {
        self.change(
            Command::StartTimer {
                note: details.note,
                category: features::optional_id(details.category_id)?,
            },
            None,
        )
    }

    pub fn pause(&self) -> Result<(), TrackerError> {
        self.change(Command::PauseTimer, None)
    }

    pub fn resume(&self, details: Option<features::SessionDetails>) -> Result<(), TrackerError> {
        self.change(Command::ResumeTimer, details.map(session_edit).transpose()?)
    }

    pub fn finish(&self, details: Option<features::SessionDetails>) -> Result<(), TrackerError> {
        self.change(
            Command::FinishSession,
            details.map(session_edit).transpose()?,
        )
    }

    pub fn reset(&self) -> Result<(), TrackerError> {
        self.change(Command::ResetTimer, None)
    }

    pub fn reopen(&self, id: String) -> Result<(), TrackerError> {
        self.change(Command::ReopenSession(features::parse_id(&id)?), None)
    }
}

impl Tracker {
    fn lock(&self) -> Result<MutexGuard<'_, Store>> {
        self.local
            .lock()
            .map_err(|_| anyhow::anyhow!("Перезапустите Tracking для восстановления таймера."))
    }

    fn change(&self, command: Command, edit: Option<Command>) -> Result<(), TrackerError> {
        self.lock()?.apply(command, edit, Utc::now())?;
        Ok(())
    }
}

fn session_edit(details: features::SessionDetails) -> Result<Command> {
    Ok(Command::EditTimer {
        note: details.note,
        category: features::optional_id(details.category_id)?,
    })
}

struct Store {
    database: Database,
    state: LocalState,
    timer_changed: bool,
    dataset_changed: bool,
}

impl Store {
    fn timer(&self, now: DateTime<Utc>) -> TimerView {
        let timer = self.state.timer.as_ref();
        let kind = timer.map_or(self.state.next_kind, |timer| timer.kind);
        TimerView {
            session_id: timer.map(|timer| timer.id.to_string()),
            kind: kind.into(),
            status: match timer {
                Some(timer) if timer.is_paused() => TimerStatus::Paused,
                Some(_) => TimerStatus::Running,
                None => TimerStatus::Ready,
            },
            elapsed_seconds: timer.map_or(0, |timer| timer.elapsed_seconds(now)),
            target_seconds: self.state.dataset.settings.target_seconds(kind),
            note: timer.map_or_else(String::new, |timer| timer.note.clone()),
            category_id: timer
                .and_then(|timer| timer.category)
                .map(|id| id.to_string()),
        }
    }

    fn apply(&mut self, command: Command, edit: Option<Command>, now: DateTime<Utc>) -> Result<()> {
        let previous = self.tracker_state();
        let mut next = previous.clone();
        if let Some(edit) = edit {
            next.execute(edit, now)?;
        }
        next.execute(command, now)?;
        let mut cloud = self.state.cloud.clone();
        let timer_changed = cloud.record_timer(&previous, &next);
        let dataset_changed = previous.dataset != next.dataset;
        self.save(next, cloud)?;
        self.timer_changed |= timer_changed;
        self.dataset_changed |= dataset_changed;
        Ok(())
    }

    fn tracker_state(&self) -> TrackerState {
        TrackerState {
            dataset: self.state.dataset.clone(),
            timer: self.state.timer.clone(),
            next_kind: self.state.next_kind,
        }
    }

    fn save(&mut self, state: TrackerState, cloud: app_platform::cloud::CloudState) -> Result<()> {
        let next = LocalState {
            dataset: state.dataset,
            timer: state.timer,
            next_kind: state.next_kind,
            cloud,
        };
        self.database.save(&next)?;
        self.state = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;

    use super::*;

    #[test]
    fn restores_timer_and_saves_the_edited_session() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("tracking.sqlite3");
        let path = path.to_string_lossy();
        let start = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
        let tracker = Tracker::open(path.to_string())?;
        tracker.lock()?.apply(
            Command::StartTimer {
                note: "Первая заметка".into(),
                category: None,
            },
            None,
            start,
        )?;
        let id = tracker.lock()?.timer(start).session_id.unwrap();
        drop(tracker);

        let tracker = Tracker::open(path.to_string())?;
        let pause = start + TimeDelta::seconds(30);
        let restored = tracker.lock()?.timer(pause);
        assert_eq!(restored.status, TimerStatus::Running);
        assert_eq!(restored.elapsed_seconds, 30);
        assert_eq!(restored.note, "Первая заметка");
        tracker.lock()?.apply(Command::PauseTimer, None, pause)?;
        tracker.lock()?.apply(
            Command::FinishSession,
            Some(Command::EditTimer {
                note: "Итоговая заметка".into(),
                category: None,
            }),
            pause + TimeDelta::seconds(60),
        )?;
        drop(tracker);

        let tracker = Tracker::open(path.to_string())?;
        assert_eq!(tracker.timer()?.status, TimerStatus::Ready);
        let history = tracker.history()?;
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].id, id);
        assert_eq!(history[0].duration_millis, 30_000);
        assert_eq!(history[0].note, "Итоговая заметка");
        Ok(())
    }

    #[test]
    fn failed_write_keeps_the_current_timer_unchanged() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("tracking.sqlite3");
        let path = path.to_string_lossy();
        let first = Tracker::open(path.to_string())?;
        let second = Tracker::open(path.to_string())?;
        first.start(features::SessionDetails {
            note: "Сохранена".into(),
            category_id: None,
        })?;
        assert!(
            second
                .start(features::SessionDetails {
                    note: "Не сохранена".into(),
                    category_id: None
                })
                .is_err()
        );
        assert_eq!(second.timer()?.status, TimerStatus::Ready);
        assert_eq!(first.timer()?.note, "Сохранена");
        Ok(())
    }
}
