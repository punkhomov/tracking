use chrono::{DateTime, Local, NaiveDate, Utc};
use egui::{Align, ComboBox, Layout, RichText, ScrollArea, TextStyle, Ui, vec2};
use tracking_core::{
    ArchiveScope, Command, Dataset, Kind, Period, export_archive,
    reports::{Span, span_dates},
};
use uuid::Uuid;

use crate::{
    IoEvent, Tracker, category_name, completion,
    components::{action_row, actions_menu, card, color_dot, primary, primary_add},
    date_format, default_color, duration,
    editors::{Editor, weekday_label, weekday_order},
    kind_name,
    theme::{CONTROL_HEIGHT, parse_color},
    time_format,
};

const HISTORY_ROW_HEIGHT: f32 = 60.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum Screen {
    #[default]
    Today,
    History,
    Stats,
    Categories,
    Zones,
    Settings,
}

impl Screen {
    pub const ALL: [Self; 6] = [
        Self::Today,
        Self::History,
        Self::Stats,
        Self::Categories,
        Self::Zones,
        Self::Settings,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Today => "Сегодня",
            Self::History => "История",
            Self::Stats => "Статистика",
            Self::Categories => "Категории",
            Self::Zones => "Зоны дня",
            Self::Settings => "Настройки",
        }
    }
}

#[derive(Default)]
pub(crate) struct HistoryFilters {
    query: String,
    kind: Option<Kind>,
    span: Option<Span>,
    cached: Option<(u64, NaiveDate)>,
    rows: Vec<HistoryRow>,
}

enum HistoryRow {
    Day(NaiveDate),
    Session(Uuid),
}

#[derive(Clone, Copy)]
enum RowAction {
    Edit(Uuid),
    Delete(Uuid),
}

impl HistoryFilters {
    fn rebuild(&mut self, dataset: &Dataset, date: NaiveDate, revision: u64) {
        if self.cached == Some((revision, date)) {
            return;
        }
        self.cached = Some((revision, date));
        self.rows.clear();
        let range = self
            .span
            .map(|span| span_dates(date, span, &dataset.settings.first_day_of_week));
        let query = self.query.trim().to_lowercase();
        let mut periods: Vec<_> = dataset
            .periods
            .iter()
            .filter(|(_, period)| {
                let day = period.start.with_timezone(&Local).date_naive();
                self.kind.is_none_or(|kind| period.kind == kind)
                    && range.is_none_or(|(start, end)| day >= start && day < end)
                    && (query.is_empty()
                        || period.note.to_lowercase().contains(&query)
                        || category_name(dataset, period.category)
                            .to_lowercase()
                            .contains(&query))
            })
            .collect();
        periods.sort_unstable_by_key(|(id, period)| std::cmp::Reverse((period.start, **id)));
        let mut previous = None;
        for (&id, period) in periods {
            let day = period.start.with_timezone(&Local).date_naive();
            if previous != Some(day) {
                self.rows.push(HistoryRow::Day(day));
                previous = Some(day);
            }
            self.rows.push(HistoryRow::Session(id));
        }
    }
}

impl Tracker {
    pub(crate) fn today_history(&mut self, ui: &mut Ui, now: DateTime<Utc>) {
        let today = now.with_timezone(&Local).date_naive();
        let mut action = None;
        card(ui, |ui| {
            action_row(ui, |ui| {
                ui.heading("Сегодняшние сессии");
                if ui.button("Добавить сессию").clicked() {
                    self.open_editor(Editor::new_period(None, &self.state.dataset, now));
                }
                if ui.button("Вся история").clicked() {
                    self.screen = Screen::History;
                }
            });
            let mut periods: Vec<_> = self
                .state
                .dataset
                .periods
                .iter()
                .filter(|(_, period)| period.start.with_timezone(&Local).date_naive() == today)
                .collect();
            periods.sort_unstable_by_key(|(id, period)| std::cmp::Reverse((period.start, **id)));
            if periods.is_empty() {
                ui.weak("Сегодня ещё нет завершённых сессий.");
            }
            for (&id, period) in periods.into_iter().take(8) {
                if let Some(clicked) = session_row(ui, &self.state.dataset, id, period) {
                    action = Some(clicked);
                }
            }
        });
        self.apply_row(action, now);
    }

    pub(crate) fn history(&mut self, ui: &mut Ui, now: DateTime<Utc>) {
        let today = now.with_timezone(&Local).date_naive();
        let mut action = None;
        card(ui, |ui| {
            action_row(ui, |ui| {
                ui.heading("Вся история");
                if primary_add(ui, "Добавить сессию").clicked() {
                    self.open_editor(Editor::new_period(None, &self.state.dataset, now));
                }
                if ui
                    .add_enabled(
                        !self.state.dataset.periods.is_empty(),
                        egui::Button::new("Очистить историю"),
                    )
                    .clicked()
                {
                    self.open_editor(Editor::confirm(
                        "Очистить историю",
                        "Все завершённые сессии будут удалены.",
                        Command::ClearHistory,
                    ));
                }
            });
            let mut changed = ui
                .add(
                    egui::TextEdit::singleline(&mut self.history.query)
                        .hint_text("Поиск по заметке или категории")
                        .desired_width(ui.available_width()),
                )
                .changed();
            action_row(ui, |ui| {
                ComboBox::from_id_salt("history-date")
                    .selected_text(span_label(self.history.span))
                    .show_ui(ui, |ui| {
                        for span in [None, Some(Span::Day), Some(Span::Week), Some(Span::Month)] {
                            changed |= ui
                                .selectable_value(&mut self.history.span, span, span_label(span))
                                .changed();
                        }
                    });
                ComboBox::from_id_salt("history-kind")
                    .selected_text(self.history.kind.map_or("Все типы", kind_name))
                    .show_ui(ui, |ui| {
                        for kind in [None, Some(Kind::Work), Some(Kind::Break)] {
                            changed |= ui
                                .selectable_value(
                                    &mut self.history.kind,
                                    kind,
                                    kind.map_or("Все типы", kind_name),
                                )
                                .changed();
                        }
                    });
            });
            if changed {
                self.history.cached = None;
            }
            self.history
                .rebuild(&self.state.dataset, today, self.revision);
            if self.history.rows.is_empty() {
                ui.weak("Сессий по выбранным фильтрам нет.");
            }
            ScrollArea::vertical()
                .id_salt("history-rows")
                .max_height(510.0)
                .auto_shrink([false, true])
                .show_rows(
                    ui,
                    HISTORY_ROW_HEIGHT,
                    self.history.rows.len(),
                    |ui, range| {
                        for index in range {
                            ui.allocate_ui(vec2(ui.available_width(), HISTORY_ROW_HEIGHT), |ui| {
                                ui.set_min_height(HISTORY_ROW_HEIGHT);
                                match self.history.rows[index] {
                                    HistoryRow::Day(day) => {
                                        ui.add_space(8.0);
                                        ui.label(
                                            RichText::new(
                                                day.format(date_format(
                                                    &self.state.dataset.settings,
                                                ))
                                                .to_string(),
                                            )
                                            .strong(),
                                        );
                                        ui.separator();
                                    }
                                    HistoryRow::Session(id) => {
                                        if let Some(clicked) = session_row(
                                            ui,
                                            &self.state.dataset,
                                            id,
                                            &self.state.dataset.periods[&id],
                                        ) {
                                            action = Some(clicked);
                                        }
                                    }
                                }
                            });
                        }
                    },
                );
        });
        self.apply_row(action, now);
    }

    fn apply_row(&mut self, action: Option<RowAction>, now: DateTime<Utc>) {
        match action {
            Some(RowAction::Edit(id)) => {
                self.open_editor(Editor::new_period(Some(id), &self.state.dataset, now))
            }
            Some(RowAction::Delete(id)) => self.open_editor(Editor::confirm(
                "Удалить сессию",
                "Сессия будет удалена из истории.",
                Command::DeletePeriod(id),
            )),
            None => {}
        }
    }

    pub(crate) fn categories(&mut self, ui: &mut Ui, now: DateTime<Utc>) {
        let mut edit = None;
        let mut archive = None;
        card(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.heading("Категории");
                if primary_add(ui, "Добавить категорию").clicked() {
                    edit = Some(None);
                }
            });
            if self.state.dataset.categories.is_empty() {
                ui.weak("Категорий пока нет.");
            }
            for archived in [false, true] {
                let mut categories: Vec<_> = self
                    .state
                    .dataset
                    .categories
                    .iter()
                    .filter(|(_, category)| category.archived == archived)
                    .collect();
                if categories.is_empty() {
                    continue;
                }
                categories.sort_by_key(|(_, category)| &category.name);
                ui.strong(if archived {
                    "В архиве"
                } else {
                    "Активные"
                });
                for (&id, category) in categories {
                    ui.push_id(id, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            color_dot(ui, default_color(Some(category)));
                            ui.strong(&category.name);
                            if ui.button("Изменить").clicked() {
                                edit = Some(Some(id));
                            }
                            if ui
                                .button(if archived {
                                    "Восстановить"
                                } else {
                                    "В архив"
                                })
                                .clicked()
                            {
                                archive = Some(Command::ArchiveCategory(id, !archived));
                            }
                        });
                    });
                }
            }
        });
        if let Some(id) = edit {
            self.open_editor(Editor::new_category(id, &self.state.dataset, false));
        } else if let Some(command) = archive {
            self.dispatch(command, now, ui.ctx(), false);
        }
    }

    pub(crate) fn zones(&mut self, ui: &mut Ui, now: DateTime<Utc>) {
        let mut editor = None;
        let dataset = &self.state.dataset;
        let today = now.with_timezone(&Local).date_naive();
        card(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.heading("Зоны таймлайна");
                if primary_add(ui, "Добавить зону").clicked() {
                    editor = Some(Editor::new_zone(None, None, dataset, today));
                }
            });
            if dataset.zones.is_empty() {
                ui.weak("Настройте интервалы, в которые планируете работать.");
            }
            let mut zones: Vec<_> = dataset.zones.iter().collect();
            zones.sort_by_key(|(id, zone)| {
                let root = zone.parent.and_then(|parent| dataset.zones.get(&parent));
                (
                    root.unwrap_or(zone).start,
                    zone.parent.unwrap_or(**id),
                    zone.parent.is_some(),
                    zone.start,
                    **id,
                )
            });
            for (&id, zone) in zones {
                ui.push_id(id, |ui| {
                    ui.separator();
                    ui.horizontal_wrapped(|ui| {
                        if zone.parent.is_some() {
                            ui.weak("↳");
                        }
                        if let Some(color) = zone.color.as_deref().and_then(parse_color) {
                            color_dot(ui, color);
                        }
                        ui.strong(&zone.name);
                        let format = time_format(&dataset.settings);
                        ui.weak(format!(
                            "{}–{}",
                            zone.start.format(format),
                            zone.end.format(format)
                        ));
                        let days = weekday_order(&dataset.settings.first_day_of_week)
                            .into_iter()
                            .filter(|day| zone.weekdays.contains(day))
                            .map(weekday_label)
                            .collect::<Vec<_>>()
                            .join(" ");
                        ui.weak(days);
                    });
                    ui.horizontal_wrapped(|ui| {
                        let format = date_format(&dataset.settings);
                        if let Some(start) = zone.starts_on {
                            ui.weak(format!("С {}", start.format(format)));
                        }
                        if let Some(end) = zone.ends_on {
                            ui.weak(format!("По {}", end.format(format)));
                        }
                        if zone.ends_on.is_some_and(|end| end < today)
                            || zone
                                .parent
                                .and_then(|id| dataset.zones.get(&id))
                                .and_then(|parent| parent.ends_on)
                                .is_some_and(|end| end < today)
                        {
                            ui.weak("Завершена");
                        }
                        if let Some(category) = zone.category {
                            ui.weak(category_name(dataset, Some(category)));
                        }
                        if zone.exclude_from_breakdown {
                            ui.weak("Скрыта");
                        }
                        if ui.button("Изменить").clicked() {
                            editor = Some(Editor::new_zone(Some(id), None, dataset, today));
                        }
                        if zone.parent.is_none() && ui.button("Добавить вложенную").clicked()
                        {
                            editor = Some(Editor::new_zone(None, Some(id), dataset, today));
                        }
                        if ui.button("Удалить").clicked() {
                            editor = Some(Editor::confirm(
                                "Удалить зону",
                                "Зона и все её вложенные зоны будут удалены.",
                                Command::DeleteZone(id),
                            ));
                        }
                    });
                });
            }
        });
        if let Some(editor) = editor {
            self.open_editor(editor);
        }
    }

    pub(crate) fn settings(&mut self, ui: &mut Ui, now: DateTime<Utc>) {
        self.cloud_settings(ui);
        let mut save = false;
        card(ui, |ui| {
            ui.heading("Таймер");
            number(ui, "Работа, мин", &mut self.settings.work_minutes, 1..=60);
            number(ui, "Перерыв, мин", &mut self.settings.break_minutes, 1..=30);
            number(
                ui,
                "Повтор напоминания, мин",
                &mut self.settings.reminder_interval_minutes,
                1..=30,
            );
            number(
                ui,
                "Минимальная пауза, сек",
                &mut self.settings.min_pause_seconds,
                5..=60,
            );
            ui.checkbox(&mut self.settings.auto_start_break, "Автостарт перерыва");
            ui.checkbox(
                &mut self.settings.auto_start_pomodoro,
                "Автостарт после перерыва",
            );
        });
        card(ui, |ui| {
            ui.heading("Оформление и регион");
            picker(
                ui,
                "Тема",
                &mut self.settings.theme,
                &[
                    ("system", "Системная"),
                    ("light", "Светлая"),
                    ("dark", "Тёмная"),
                ],
            );
            picker(
                ui,
                "Первый день недели",
                &mut self.settings.first_day_of_week,
                &[
                    ("monday", "Понедельник"),
                    ("sunday", "Воскресенье"),
                    ("saturday", "Суббота"),
                ],
            );
            picker(
                ui,
                "Формат даты",
                &mut self.settings.date_format,
                &[
                    ("DD.MM.YYYY", "ДД.ММ.ГГГГ"),
                    ("YYYY-MM-DD", "ГГГГ-ММ-ДД"),
                    ("MM/DD/YYYY", "ММ/ДД/ГГГГ"),
                ],
            );
            picker(
                ui,
                "Формат времени",
                &mut self.settings.hour_format,
                &[("24h", "24 часа"), ("12h", "12 часов")],
            );
            save = primary(ui, "Сохранить настройки").clicked();
        });
        if save {
            self.dispatch(
                Command::SaveSettings(self.settings.clone()),
                now,
                ui.ctx(),
                false,
            );
        }
        card(ui, |ui| {
            ui.heading("Данные и уведомления");
            ComboBox::from_id_salt("archive-scope")
                .selected_text(archive_label(self.archive_scope))
                .show_ui(ui, |ui| {
                    for scope in [
                        ArchiveScope::Full,
                        ArchiveScope::Periods,
                        ArchiveScope::Settings,
                    ] {
                        ui.selectable_value(&mut self.archive_scope, scope, archive_label(scope));
                    }
                });
            ui.horizontal_wrapped(|ui| {
                if ui.button("Экспорт JSON").clicked() {
                    self.export(ui, now);
                }
                if ui.button("Импорт JSON").clicked() {
                    let (send, receive) = std::sync::mpsc::channel();
                    self.error = None;
                    self.status = None;
                    self.platform
                        .open_archive(completion(send, ui.ctx(), IoEvent::Opened));
                    self.io = Some(receive);
                }
            });
            if ui.button("Разрешить или проверить уведомления").clicked()
            {
                let (send, receive) = std::sync::mpsc::channel();
                self.error = None;
                self.status = None;
                self.platform.request_notifications(completion(
                    send,
                    ui.ctx(),
                    IoEvent::Permission,
                ));
                self.io = Some(receive);
            }
        });
    }

    fn export(&mut self, ui: &Ui, now: DateTime<Utc>) {
        let (send, receive) = std::sync::mpsc::channel();
        self.error = None;
        self.status = None;
        match export_archive(&self.state.dataset, self.archive_scope) {
            Ok(contents) => {
                let label = match self.archive_scope {
                    ArchiveScope::Full => "backup",
                    ArchiveScope::Periods => "history",
                    ArchiveScope::Settings => "settings",
                };
                let name = format!(
                    "tracking-{label}-{}.json",
                    now.with_timezone(&Local).date_naive()
                );
                self.platform.save_archive(
                    &name,
                    contents,
                    completion(send, ui.ctx(), IoEvent::Exported),
                );
                self.io = Some(receive);
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }
}

fn session_row(ui: &mut Ui, dataset: &Dataset, id: Uuid, period: &Period) -> Option<RowAction> {
    ui.push_id(id, |ui| {
        let mut action = None;
        let name = if period.note.is_empty() {
            kind_name(period.kind)
        } else {
            &period.note
        };
        let format = time_format(&dataset.settings);
        let start = period.start.with_timezone(&Local);
        let end = period.end.with_timezone(&Local);
        let mut subtitle = format!(
            "{}–{}  ·  {}",
            start.format(format),
            end.format(format),
            kind_name(period.kind)
        );
        if period.category.is_some() {
            subtitle.push_str(&format!("  ·  {}", category_name(dataset, period.category)));
        }
        let width = ui.available_width();
        let trailing = Layout::right_to_left(Align::Center);
        let leading = Layout::left_to_right(Align::Center);
        ui.allocate_ui_with_layout(vec2(width, CONTROL_HEIGHT), trailing, |ui| {
            actions_menu(ui, |ui| {
                if ui.button("Редактировать").clicked() {
                    action = Some(RowAction::Edit(id));
                    ui.close();
                }
                if ui.button("Удалить").clicked() {
                    action = Some(RowAction::Delete(id));
                    ui.close();
                }
            });
            ui.with_layout(leading, |ui| {
                let category = period.category.and_then(|id| dataset.categories.get(&id));
                color_dot(ui, default_color(category));
                ui.add(egui::Label::new(RichText::new(name).strong()).truncate())
                    .on_hover_text(name);
            });
        });
        let subtitle_height = ui.text_style_height(&TextStyle::Body);
        ui.allocate_ui_with_layout(vec2(width, subtitle_height), trailing, |ui| {
            ui.weak(duration(period.duration_millis()));
            ui.with_layout(leading, |ui| {
                ui.add(egui::Label::new(RichText::new(&subtitle).weak()).truncate())
                    .on_hover_text(subtitle);
            });
        });
        action
    })
    .inner
}

fn number(ui: &mut Ui, label: &'static str, value: &mut u32, range: std::ops::RangeInclusive<u32>) {
    ui.horizontal_wrapped(|ui| {
        let label = ui.label(label);
        ui.add(
            egui::DragValue::new(value)
                .range(range)
                .clamp_existing_to_range(false),
        )
        .labelled_by(label.id);
    });
}

fn picker(ui: &mut Ui, label: &'static str, current: &mut String, options: &[(&str, &str)]) {
    ui.horizontal_wrapped(|ui| {
        let label = ui.label(label);
        let selected = options
            .iter()
            .find(|(value, _)| value == current)
            .map_or(current.as_str(), |(_, label)| *label);
        ComboBox::from_id_salt(label.id)
            .selected_text(selected)
            .show_ui(ui, |ui| {
                for (value, display) in options {
                    if ui.selectable_label(current == value, *display).clicked() {
                        *current = (*value).to_string();
                    }
                }
            })
            .response
            .labelled_by(label.id);
    });
}

pub(crate) fn span_label(span: Option<Span>) -> &'static str {
    match span {
        None => "За всё время",
        Some(Span::Day) => "День",
        Some(Span::Week) => "Неделя",
        Some(Span::Month) => "Месяц",
    }
}

pub(crate) fn archive_label(scope: ArchiveScope) -> &'static str {
    match scope {
        ArchiveScope::Full => "Все данные",
        ArchiveScope::Periods => "История и категории",
        ArchiveScope::Settings => "Настройки и зоны",
    }
}
