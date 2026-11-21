mod charts;
mod cloud;
mod editors;
mod pages;
mod theme;

use std::{
    sync::{Arc, mpsc},
    time::Duration,
};

use app_platform::{ArchiveFile, Complete, Platform, cloud::CloudState};
use chrono::{DateTime, Local, Utc};
use egui::{Align, ComboBox, Context, Frame, Layout, Margin, RichText, Ui};
use tracking_core::{
    ArchiveScope, Category, Command, Dataset, Kind, Reminder, Settings, TrackerState,
};
use uuid::Uuid;

use self::{
    charts::{DailyCache, ReportCache},
    cloud::CloudPanel,
    editors::Editor,
    pages::{HistoryFilters, Screen},
    theme::{accent, card, primary},
};

pub struct Tracker {
    state: TrackerState,
    platform: Box<dyn Platform>,
    styles: [Arc<egui::Style>; 2],
    pending: Option<Pending>,
    cloud: CloudPanel,
    io: Option<mpsc::Receiver<Result<IoEvent, String>>>,
    screen: Screen,
    editor: Option<Editor>,
    settings: Settings,
    note: String,
    category: Option<Uuid>,
    error: Option<String>,
    status: Option<&'static str>,
    last_reminder: Option<Reminder>,
    title: String,
    history: HistoryFilters,
    archive_scope: ArchiveScope,
    revision: u64,
    daily: DailyCache,
    report: ReportCache,
}

struct Pending {
    state: TrackerState,
    cloud: Option<CloudState>,
    completed: mpsc::Receiver<Result<(), String>>,
    close_editor: bool,
    message: Option<&'static str>,
}

enum IoEvent {
    Opened(Option<ArchiveFile>),
    Exported(bool),
    Permission(bool),
}

impl Tracker {
    pub fn new(state: TrackerState, cloud: CloudState, platform: impl Platform + 'static) -> Self {
        let mut note = String::new();
        let mut category = None;
        if let Some(timer) = &state.timer {
            note.clone_from(&timer.note);
            category = timer.category;
        }
        Self {
            settings: state.dataset.settings.clone(),
            state,
            platform: Box::new(platform),
            styles: [Arc::new(theme::style(false)), Arc::new(theme::style(true))],
            pending: None,
            cloud: CloudPanel::new(cloud),
            io: None,
            screen: Screen::Today,
            editor: None,
            note,
            category,
            error: None,
            status: None,
            last_reminder: None,
            title: String::new(),
            history: HistoryFilters::default(),
            archive_scope: ArchiveScope::Full,
            revision: 0,
            daily: DailyCache::default(),
            report: ReportCache::default(),
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, now: DateTime<Utc>) {
        self.poll_save();
        self.poll_io();
        self.poll_cloud(now, ui.ctx());
        self.update_timer(now, ui.ctx());
        let dark = match self.state.dataset.settings.theme.as_str() {
            "dark" => true,
            "light" => false,
            _ => self.platform.system_dark(),
        };
        ui.set_style(self.styles[usize::from(dark)].clone());

        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(ui.visuals().extreme_bg_color)
                    .inner_margin(Margin::same(16)),
            )
            .show(ui, |ui| {
                let available = ui.available_width();
                let width = available.min(920.0);
                ui.horizontal_top(|ui| {
                    ui.add_space((available - width) / 2.0);
                    ui.with_layout(Layout::top_down(Align::Min), |ui| {
                        ui.set_width(width);
                        self.header(ui);
                        let busy = self.pending.is_some() || self.io.is_some();
                        ui.add_enabled_ui(!busy, |ui| {
                            egui::ScrollArea::vertical()
                                .id_salt(self.screen)
                                .show(ui, |ui| {
                                    self.screen(ui, now);
                                });
                        });
                    });
                });
            });
        self.show_editor(ui.ctx(), now);
    }

    fn header(&mut self, ui: &mut Ui) {
        ui.label(RichText::new("Tracking").size(29.0).strong());
        ui.horizontal_wrapped(|ui| {
            for screen in Screen::ALL {
                if ui
                    .selectable_label(self.screen == screen, screen.title())
                    .clicked()
                    && self.screen != screen
                {
                    self.screen = screen;
                    self.error = None;
                    self.status = None;
                }
            }
        });
        if let Some(reminder) = self.last_reminder.filter(|reminder| {
            self.state
                .timer
                .as_ref()
                .is_some_and(|timer| timer.id == reminder.timer)
        }) {
            ui.colored_label(accent(ui), reminder_title(reminder.kind));
        }
        if self.pending.is_some() {
            ui.weak("Сохранение…");
        } else if self.cloud.state.timer.has_conflict() {
            ui.colored_label(ui.visuals().error_fg_color, "Конфликт общего таймера");
        } else if self.cloud.state.timer.is_pending() {
            ui.weak("Таймер ожидает синхронизации");
        } else if self.io.is_some() {
            ui.weak("Операция с файлом или разрешениями…");
        }
        if self.editor.is_none() {
            if let Some(error) = &self.error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            if let Some(status) = self.status {
                ui.colored_label(accent(ui), status);
            }
        }
        ui.add_space(6.0);
    }

    fn screen(&mut self, ui: &mut Ui, now: DateTime<Utc>) {
        match self.screen {
            Screen::Today => {
                self.offer_cloud(ui);
                card(ui, |ui| self.timer(ui, now));
                self.timeline(ui, now);
                self.today_history(ui, now);
            }
            Screen::History => self.history(ui, now),
            Screen::Stats => self.statistics(ui, now),
            Screen::Categories => self.categories(ui, now),
            Screen::Zones => self.zones(ui, now),
            Screen::Settings => self.settings(ui, now),
        }
    }

    fn timer(&mut self, ui: &mut Ui, now: DateTime<Utc>) {
        let timer = self.state.timer.as_ref();
        let kind = timer.map_or(self.state.next_kind, |timer| timer.kind);
        let target = self.state.dataset.settings.target_seconds(kind).max(1);
        let elapsed = timer.map_or(0, |timer| timer.elapsed_seconds(now));
        let paused = timer.is_some_and(|timer| timer.is_paused());
        let running = timer.is_some();
        let mut command = None;
        ui.horizontal_wrapped(|ui| {
            for option in [Kind::Work, Kind::Break] {
                if ui
                    .add_enabled(
                        !running,
                        egui::Button::selectable(kind == option, kind_name(option)),
                    )
                    .clicked()
                    && kind != option
                {
                    command = Some(Command::SelectKind(option));
                }
            }
            ui.weak(if paused {
                "На паузе"
            } else if running {
                "В процессе"
            } else {
                "Готов к запуску"
            });
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(clock(elapsed)).size(58.0).strong());
            ui.weak(format!("Цель {} мин", target / 60));
        });
        ui.add(
            egui::ProgressBar::new((elapsed as f32 / target as f32).clamp(0.0, 1.0))
                .fill(accent(ui))
                .desired_height(6.0),
        );

        if kind == Kind::Work && (!running || paused) {
            ui.horizontal_wrapped(|ui| {
                category_picker(
                    ui,
                    "timer-category",
                    &self.state.dataset,
                    &mut self.category,
                );
                if ui.button("Новая категория").clicked() {
                    self.open_editor(Editor::new_category(None, &self.state.dataset, true));
                }
            });
            let label = ui.label("Заметка");
            ui.add(
                egui::TextEdit::multiline(&mut self.note)
                    .hint_text("Что будете делать?")
                    .desired_rows(2)
                    .desired_width(ui.available_width()),
            )
            .labelled_by(label.id);
        } else if let Some(timer) = timer
            && (!timer.note.is_empty() || timer.category.is_some())
        {
            ui.weak(format!(
                "{}  ·  {}",
                category_name(&self.state.dataset, timer.category),
                timer.note
            ));
        }

        ui.horizontal_wrapped(|ui| {
            if running {
                if ui
                    .button(if paused {
                        "Продолжить"
                    } else {
                        "Пауза"
                    })
                    .clicked()
                {
                    command = Some(if paused {
                        Command::ResumeTimer
                    } else {
                        Command::PauseTimer
                    });
                }
                if primary(ui, "Завершить").clicked() {
                    command = Some(Command::FinishSession);
                }
                if ui.button("Сбросить").clicked() {
                    self.open_editor(Editor::confirm(
                        "Сбросить таймер",
                        "Текущая сессия не попадёт в историю.",
                        Command::ResetTimer,
                    ));
                }
            } else {
                if primary(
                    ui,
                    if kind == Kind::Work {
                        "Запустить"
                    } else {
                        "Начать перерыв"
                    },
                )
                .clicked()
                {
                    command = Some(Command::StartTimer {
                        note: self.note.clone(),
                        category: self.category,
                    });
                }
                if kind == Kind::Break && ui.button("Пропустить").clicked() {
                    command = Some(Command::ResetTimer);
                }
                let today = now.with_timezone(&Local).date_naive();
                if let Some((id, _)) = self
                    .state
                    .dataset
                    .periods
                    .iter()
                    .filter(|(_, period)| {
                        period.end <= now
                            && period.start.with_timezone(&Local).date_naive() == today
                    })
                    .max_by_key(|(_, period)| period.end)
                    && ui.button("Вернуть последнюю сессию").clicked()
                {
                    command = Some(Command::ReopenSession(*id));
                }
            }
        });
        if let Some(command) = command {
            self.dispatch(command, now, ui.ctx(), false);
        }
    }

    fn update_timer(&mut self, now: DateTime<Utc>, ctx: &Context) {
        let title = self.state.timer.as_ref().map_or_else(
            || "Tracking".to_owned(),
            |timer| {
                format!(
                    "{}{} · {} — Tracking",
                    if timer.is_paused() { "Пауза " } else { "" },
                    clock(timer.elapsed_seconds(now)),
                    kind_name(timer.kind)
                )
            },
        );
        if self.title != title {
            self.platform.set_title(&title);
            self.title = title;
        }
        if self
            .state
            .timer
            .as_ref()
            .is_some_and(|timer| !timer.is_paused())
        {
            ctx.request_repaint_after(Duration::from_secs(1));
        } else if matches!(self.screen, Screen::Today | Screen::History | Screen::Stats) {
            ctx.request_repaint_after(Duration::from_secs(
                (60 - now.timestamp().rem_euclid(60)) as u64,
            ));
        }
        if let Some(reminder) = self.state.reminder(now)
            && self.last_reminder != Some(reminder)
        {
            self.last_reminder = Some(reminder);
            self.platform.notify(
                reminder_title(reminder.kind),
                "Откройте Tracking, чтобы завершить сессию.",
            );
        }
    }

    fn open_editor(&mut self, editor: Editor) {
        self.error = None;
        self.status = None;
        self.editor = Some(editor);
    }

    fn dispatch(
        &mut self,
        command: Command,
        now: DateTime<Utc>,
        ctx: &Context,
        close_editor: bool,
    ) {
        if self.pending.is_some() {
            return;
        }
        let mut next = self.state.clone();
        let message = matches!(command, Command::SaveSettings(_)).then_some("Настройки сохранены.");
        let result = (|| {
            if matches!(command, Command::ResumeTimer | Command::FinishSession)
                && next.timer.as_ref().is_some_and(|timer| timer.is_paused())
            {
                next.execute(
                    Command::EditTimer {
                        note: self.note.clone(),
                        category: self.category,
                    },
                    now,
                )?;
            }
            next.execute(command, now)
        })();
        if let Err(error) = result {
            self.error = Some(error.to_string());
            return;
        }
        let mut cloud = self.cloud.state.clone();
        let queued = cloud.record_timer(&self.state, &next);
        if queued {
            self.cloud.schedule.timer_changed();
        }
        if next.dataset != self.state.dataset {
            self.cloud.schedule.dataset_changed(now.timestamp());
        }
        self.persist(next, queued.then_some(cloud), ctx, close_editor, message);
    }

    fn persist(
        &mut self,
        next: TrackerState,
        cloud: Option<CloudState>,
        ctx: &Context,
        close_editor: bool,
        message: Option<&'static str>,
    ) {
        self.error = None;
        self.status = None;
        let (send, completed) = mpsc::channel();
        self.platform.save(
            &next,
            cloud.as_ref().unwrap_or(&self.cloud.state),
            completion(send, ctx, |()| ()),
        );
        self.pending = Some(Pending {
            state: next,
            cloud,
            completed,
            close_editor,
            message,
        });
    }

    fn poll_save(&mut self) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        match pending.completed.try_recv() {
            Ok(Ok(())) => {
                if self.state.dataset != pending.state.dataset {
                    self.revision = self.revision.wrapping_add(1);
                }
                if self.state.dataset.settings != pending.state.dataset.settings {
                    self.settings = pending.state.dataset.settings.clone();
                }
                let prev = self.state.timer.as_ref().map(|timer| timer.id);
                let next = pending.state.timer.as_ref().map(|timer| timer.id);
                if prev != next {
                    self.last_reminder = None;
                }
                if prev != next
                    || (self.note == self.state.timer.as_ref().map_or("", |timer| &timer.note)
                        && self.category
                            == self.state.timer.as_ref().and_then(|timer| timer.category)
                        && self.state.timer != pending.state.timer)
                {
                    self.note = pending
                        .state
                        .timer
                        .as_ref()
                        .map_or_else(String::new, |timer| timer.note.clone());
                    self.category = pending
                        .state
                        .timer
                        .as_ref()
                        .and_then(|timer| timer.category);
                }
                if let Some(id) = self.category
                    && pending.state.timer.is_none()
                    && pending
                        .state
                        .dataset
                        .categories
                        .get(&id)
                        .is_none_or(|category| category.archived)
                {
                    self.category = None;
                }
                if let Some(message) = pending.message {
                    self.status = Some(message);
                }
                if pending.close_editor {
                    if let Some(id) = self.editor.as_ref().and_then(Editor::selected_category) {
                        self.category = Some(id);
                    }
                    self.editor = None;
                    self.status = Some("Изменения сохранены.");
                }
                self.state = pending.state;
                if let Some(cloud) = pending.cloud {
                    self.cloud.saved(cloud);
                }
            }
            Ok(Err(error)) => self.error = Some(error),
            Err(mpsc::TryRecvError::Disconnected) => {
                self.error = Some("Не удалось завершить сохранение.".into())
            }
            Err(mpsc::TryRecvError::Empty) => self.pending = Some(pending),
        }
    }

    fn poll_io(&mut self) {
        let Some(receive) = self.io.take() else {
            return;
        };
        match receive.try_recv() {
            Ok(Ok(IoEvent::Opened(Some(file)))) => match Editor::import(file) {
                Ok(editor) => self.open_editor(editor),
                Err(error) => self.error = Some(error),
            },
            Ok(Ok(IoEvent::Exported(true))) => self.status = Some("Архив отправлен на сохранение."),
            Ok(Ok(IoEvent::Permission(true))) => self.status = Some("Уведомления включены."),
            Ok(Ok(IoEvent::Permission(false))) => {
                self.status = Some("Уведомления запрещены в настройках платформы.")
            }
            Ok(Ok(_)) => {}
            Ok(Err(error)) => self.error = Some(error),
            Err(mpsc::TryRecvError::Disconnected) => {
                self.error = Some("Операция не завершилась.".into())
            }
            Err(mpsc::TryRecvError::Empty) => self.io = Some(receive),
        }
    }
}

fn completion<T: 'static, R: 'static>(
    send: mpsc::Sender<Result<R, String>>,
    ctx: &Context,
    map: impl FnOnce(T) -> R + 'static,
) -> Complete<T> {
    let ctx = ctx.clone();
    Box::new(move |result| {
        let _ = send.send(result.map(map).map_err(|error| format!("{error:#}")));
        ctx.request_repaint();
    })
}

fn category_picker(ui: &mut Ui, id: &str, dataset: &Dataset, selected: &mut Option<Uuid>) {
    ComboBox::from_id_salt(id)
        .selected_text(category_name(dataset, *selected))
        .show_ui(ui, |ui| {
            ui.selectable_value(selected, None, "Без категории");
            let mut choices: Vec<_> = dataset
                .categories
                .iter()
                .filter(|(id, category)| {
                    !category.archived || **id == selected.unwrap_or(Uuid::nil())
                })
                .collect();
            choices.sort_by_key(|(_, category)| &category.name);
            for (&id, category) in choices {
                ui.selectable_value(selected, Some(id), &category.name);
            }
        });
}

fn category_name(dataset: &Dataset, id: Option<Uuid>) -> &str {
    id.and_then(|id| dataset.categories.get(&id))
        .map_or("Без категории", |category| &category.name)
}

fn default_color(category: Option<&Category>) -> egui::Color32 {
    category
        .and_then(|category| theme::parse_color(&category.color))
        .unwrap_or(egui::Color32::from_rgb(75, 131, 110))
}

fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Work => "Работа",
        Kind::Break => "Перерыв",
    }
}

fn reminder_title(kind: Kind) -> &'static str {
    match kind {
        Kind::Work => "Время сделать перерыв",
        Kind::Break => "Перерыв окончен",
    }
}

fn clock(seconds: i64) -> String {
    if seconds >= 3600 {
        format!(
            "{}:{:02}:{:02}",
            seconds / 3600,
            seconds % 3600 / 60,
            seconds % 60
        )
    } else {
        format!("{:02}:{:02}", seconds / 60, seconds % 60)
    }
}

fn duration(ms: i64) -> String {
    let seconds = ms / 1_000;
    match seconds {
        0..60 => format!("{seconds} с"),
        60..3600 => format!("{} мин", seconds / 60),
        _ => format!("{} ч {:02} мин", seconds / 3600, seconds % 3600 / 60),
    }
}

fn time_format(settings: &Settings) -> &'static str {
    if settings.hour_format == "12h" {
        "%I:%M %p"
    } else {
        "%H:%M"
    }
}

fn date_format(settings: &Settings) -> &'static str {
    match settings.date_format.as_str() {
        "YYYY-MM-DD" => "%Y-%m-%d",
        "MM/DD/YYYY" => "%m/%d/%Y",
        _ => "%d.%m.%Y",
    }
}
