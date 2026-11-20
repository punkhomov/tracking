use anyhow::{Context, Result, ensure};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{Category, Dataset, ImportArchive, Kind, Period, Settings, Timer, Zone, ZoneStyle};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackerState {
    pub dataset: Dataset,
    pub timer: Option<Timer>,
    pub next_kind: Kind,
}

#[derive(Clone)]
pub enum Command {
    SelectKind(Kind),
    StartTimer {
        note: String,
        category: Option<Uuid>,
    },
    EditTimer {
        note: String,
        category: Option<Uuid>,
    },
    PauseTimer,
    ResumeTimer,
    FinishSession,
    ResetTimer,
    ReopenSession(Uuid),
    SavePeriod(Uuid, Period),
    DeletePeriod(Uuid),
    ClearHistory,
    SaveCategory(Uuid, Category),
    ArchiveCategory(Uuid, bool),
    SaveZone(Uuid, Zone),
    DeleteZone(Uuid),
    SaveSettings(Settings),
    Import(Box<ImportArchive>, bool),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Reminder {
    pub timer: Uuid,
    pub kind: Kind,
    pub deadline: i64,
    pub occurrence: i64,
}

impl TrackerState {
    pub fn execute(&mut self, command: Command, now: DateTime<Utc>) -> Result<()> {
        match command {
            Command::SelectKind(kind) => {
                ensure!(self.timer.is_none(), "Сначала завершите текущую сессию.");
                self.next_kind = kind;
            }
            Command::StartTimer { note, category } => {
                ensure!(self.timer.is_none(), "Сессия уже запущена.");
                let (note, category) = session_details(self.next_kind, note, category);
                self.timer = Some(Timer::start(self.next_kind, now, note, category));
            }
            Command::EditTimer { note, category } => {
                let timer = self.timer.as_mut().context("Сначала запустите таймер.")?;
                ensure!(
                    timer.is_paused(),
                    "Поставьте таймер на паузу для редактирования."
                );
                (timer.note, timer.category) = session_details(timer.kind, note, category);
            }
            Command::PauseTimer => {
                self.timer
                    .as_mut()
                    .context("Сначала запустите таймер.")?
                    .pause(now);
            }
            Command::ResumeTimer => {
                self.timer
                    .as_mut()
                    .context("Сначала запустите таймер.")?
                    .resume(now, &self.dataset.settings);
            }
            Command::FinishSession => self.finish(now)?,
            Command::ResetTimer => {
                self.timer = None;
                self.next_kind = Kind::Work;
            }
            Command::ReopenSession(id) => {
                ensure!(self.timer.is_none(), "Сначала завершите текущую сессию.");
                let period = self
                    .dataset
                    .periods
                    .get(&id)
                    .context("Сессия не найдена.")?;
                ensure!(period.end <= now, "Сессия ещё не закончилась.");
                self.next_kind = period.kind;
                self.timer = Some(Timer::reopen(id, period));
            }
            Command::SavePeriod(id, mut period) => {
                self.ensure_not_running(id)?;
                ensure!(
                    period.start < period.end,
                    "Конец сессии должен быть позже начала."
                );
                period.pauses.retain_mut(|pause| {
                    pause.start = pause.start.max(period.start);
                    pause.end = pause.end.min(period.end);
                    pause.start < pause.end
                });
                period.note = period.note.trim().to_owned();
                period.validate()?;
                self.dataset.periods.insert(id, period);
            }
            Command::DeletePeriod(id) => {
                self.ensure_not_running(id)?;
                self.dataset.periods.remove(&id);
            }
            Command::ClearHistory => {
                ensure!(
                    self.timer
                        .as_ref()
                        .is_none_or(|timer| !self.dataset.periods.contains_key(&timer.id)),
                    "Завершите или сбросьте восстановленную сессию."
                );
                self.dataset.periods.clear();
            }
            Command::SaveCategory(id, mut category) => {
                category.name = category.name.trim().to_owned();
                ensure!(!category.name.is_empty(), "Введите название категории.");
                self.dataset.categories.insert(id, category);
            }
            Command::ArchiveCategory(id, archived) => {
                self.dataset
                    .categories
                    .get_mut(&id)
                    .context("Категория не найдена.")?
                    .archived = archived;
            }
            Command::SaveZone(id, zone) => self.save_zone(id, zone)?,
            Command::DeleteZone(id) => {
                self.dataset
                    .zones
                    .retain(|zone_id, zone| *zone_id != id && zone.parent != Some(id));
            }
            Command::SaveSettings(settings) => {
                settings.validate()?;
                self.dataset.settings = settings;
            }
            Command::Import(archive, replace) => {
                ensure!(
                    !replace || self.timer.is_none(),
                    "Завершите текущую сессию перед заменой истории."
                );
                self.dataset = archive.apply(&self.dataset, replace)?;
            }
        }
        Ok(())
    }

    pub fn reminder(&self, now: DateTime<Utc>) -> Option<Reminder> {
        let timer = self.timer.as_ref().filter(|timer| !timer.is_paused())?;
        let settings = &self.dataset.settings;
        let deadline = settings.target_seconds(timer.kind);
        let over = timer.elapsed_seconds(now) - deadline;
        (over >= 0).then(|| Reminder {
            timer: timer.id,
            kind: timer.kind,
            deadline,
            occurrence: over / (i64::from(settings.reminder_interval_minutes.max(1)) * 60),
        })
    }

    fn ensure_not_running(&self, id: Uuid) -> Result<()> {
        ensure!(
            self.timer.as_ref().is_none_or(|timer| timer.id != id),
            "Сначала завершите восстановленную сессию."
        );
        Ok(())
    }

    fn finish(&mut self, now: DateTime<Utc>) -> Result<()> {
        let settings = &self.dataset.settings;
        let timer = self.timer.as_ref().context("Сначала запустите таймер.")?;
        let period = timer.clone().finish(now, settings)?;
        let finished_kind = timer.kind;
        self.dataset.periods.insert(timer.id, period);
        self.next_kind = finished_kind.next();
        let auto_start = match finished_kind {
            Kind::Work => settings.auto_start_break,
            Kind::Break => settings.auto_start_pomodoro,
        };
        self.timer = auto_start.then(|| Timer::start(self.next_kind, now, String::new(), None));
        Ok(())
    }

    fn save_zone(&mut self, id: Uuid, mut zone: Zone) -> Result<()> {
        zone.name = zone.name.trim().to_owned();
        ensure!(!zone.name.is_empty(), "Введите название зоны.");
        ensure!(
            zone.start < zone.end,
            "Конец зоны должен быть позже начала."
        );
        ensure!(
            !zone.weekdays.is_empty() && zone.weekdays.iter().all(|day| *day < 7),
            "Выберите дни недели."
        );
        let children = self
            .dataset
            .zones
            .values()
            .filter(|child| child.parent == Some(id));
        for child in children {
            ensure!(
                zone.parent.is_none(),
                "Зону с дочерними зонами нельзя вложить."
            );
            ensure!(
                zone.start <= child.start && child.end <= zone.end,
                "Новая граница отсекает вложенную зону «{}».",
                child.name
            );
        }
        if let Some(parent) = zone.parent {
            ensure!(parent != id, "Зона не может быть вложена в себя.");
            let root = self
                .dataset
                .zones
                .get(&parent)
                .context("Родительская зона не найдена.")?;
            ensure!(root.parent.is_none(), "Выберите основную зону.");
            ensure!(
                root.start <= zone.start && zone.end <= root.end,
                "Вложенная зона должна быть в пределах основной."
            );
            zone.style = ZoneStyle::Hatched;
            zone.color = None;
            zone.category = None;
            zone.exclude_from_breakdown = false;
        }
        self.dataset.zones.insert(id, zone);
        Ok(())
    }
}

fn session_details(kind: Kind, note: String, category: Option<Uuid>) -> (String, Option<Uuid>) {
    match kind {
        Kind::Work => (note.trim().to_owned(), category),
        Kind::Break => (String::new(), None),
    }
}
