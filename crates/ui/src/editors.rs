use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use egui::{Color32, ComboBox, Context, Id, Modal, ScrollArea, Ui};
use tracking_core::{
    Category, Command, Dataset, ImportArchive, Kind, Period, Zone, ZoneStyle, read_archive,
};
use uuid::Uuid;

use crate::{
    Tracker, category_picker, default_color, kind_name,
    pages::archive_label,
    theme::{color_string, parse_color, primary},
};

pub(crate) enum Editor {
    Period(PeriodForm),
    Category(CategoryForm),
    Zone(ZoneForm),
    Confirm {
        title: &'static str,
        body: &'static str,
        command: Command,
    },
    Import {
        name: String,
        archive: Box<ImportArchive>,
        replace: bool,
    },
}

pub(crate) struct PeriodForm {
    id: Uuid,
    original: Option<Period>,
    start: String,
    end: String,
    note: String,
    category: Option<Uuid>,
    kind: Kind,
    clear_pauses: bool,
}

pub(crate) struct CategoryForm {
    id: Uuid,
    value: Category,
    color: Color32,
    select: bool,
    existing: bool,
}

pub(crate) struct ZoneForm {
    id: Uuid,
    value: Zone,
    color: Color32,
    start: String,
    end: String,
    starts_on: String,
    ends_on: String,
    excluded_dates: String,
    existing: bool,
}

impl Editor {
    pub fn new_period(id: Option<Uuid>, dataset: &Dataset, now: DateTime<Utc>) -> Self {
        let original = id.and_then(|id| dataset.periods.get(&id).cloned());
        let default_start =
            now - chrono::Duration::minutes(i64::from(dataset.settings.work_minutes));
        let format = |date: DateTime<Utc>| {
            date.with_timezone(&Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        };
        Self::Period(PeriodForm {
            id: id.unwrap_or_else(Uuid::new_v4),
            start: format(
                original
                    .as_ref()
                    .map_or(default_start, |period| period.start),
            ),
            end: format(original.as_ref().map_or(now, |period| period.end)),
            note: original
                .as_ref()
                .map_or_else(String::new, |period| period.note.clone()),
            category: original.as_ref().and_then(|period| period.category),
            kind: original.as_ref().map_or(Kind::Work, |period| period.kind),
            original,
            clear_pauses: false,
        })
    }

    pub fn new_category(id: Option<Uuid>, dataset: &Dataset, select: bool) -> Self {
        let original = id.and_then(|id| dataset.categories.get(&id));
        Self::Category(CategoryForm {
            id: id.unwrap_or_else(Uuid::new_v4),
            value: original.cloned().unwrap_or_else(|| Category {
                name: String::new(),
                color: "#4b836e".into(),
                archived: false,
            }),
            color: default_color(original),
            select,
            existing: original.is_some(),
        })
    }

    pub fn new_zone(
        id: Option<Uuid>,
        parent: Option<Uuid>,
        dataset: &Dataset,
        today: NaiveDate,
    ) -> Self {
        let original = id.and_then(|id| dataset.zones.get(&id).cloned());
        let root = parent.and_then(|parent| dataset.zones.get(&parent));
        let existing = original.is_some();
        let value = original.unwrap_or_else(|| Zone {
            name: String::new(),
            start: root.map_or(NaiveTime::from_hms_opt(10, 0, 0).unwrap(), |zone| {
                zone.start
            }),
            end: root.map_or(NaiveTime::from_hms_opt(19, 0, 0).unwrap(), |zone| zone.end),
            weekdays: root.map_or_else(|| vec![1, 2, 3, 4, 5], |zone| zone.weekdays.clone()),
            starts_on: Some(today),
            ends_on: None,
            excluded_dates: Default::default(),
            parent,
            color: Some("#618abc".into()),
            style: ZoneStyle::Solid,
            full_range: false,
            exclude_from_breakdown: false,
            category: None,
        });
        Self::Zone(ZoneForm {
            id: id.unwrap_or_else(Uuid::new_v4),
            start: value.start.format("%H:%M").to_string(),
            end: value.end.format("%H:%M").to_string(),
            starts_on: value
                .starts_on
                .map_or_else(String::new, |date| date.to_string()),
            ends_on: value
                .ends_on
                .map_or_else(String::new, |date| date.to_string()),
            excluded_dates: value
                .excluded_dates
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
            color: value
                .color
                .as_deref()
                .and_then(parse_color)
                .unwrap_or(Color32::from_rgb(97, 138, 188)),
            existing,
            value,
        })
    }

    pub fn confirm(title: &'static str, body: &'static str, command: Command) -> Self {
        Self::Confirm {
            title,
            body,
            command,
        }
    }

    pub fn import(file: app_platform::ArchiveFile) -> Result<Self, String> {
        let archive = read_archive(&file.text).map_err(|error| format!("{error:#}"))?;
        Ok(Self::Import {
            name: file.name,
            archive: Box::new(archive),
            replace: false,
        })
    }

    pub fn selected_category(&self) -> Option<Uuid> {
        match self {
            Self::Category(form) if form.select => Some(form.id),
            _ => None,
        }
    }

    fn title(&self) -> &'static str {
        match self {
            Self::Period(form) if form.original.is_some() => "Редактировать сессию",
            Self::Period(_) => "Добавить сессию",
            Self::Category(form) if form.existing => "Редактировать категорию",
            Self::Category(_) => "Добавить категорию",
            Self::Zone(form) if form.existing => "Редактировать зону",
            Self::Zone(_) => "Добавить зону",
            Self::Confirm { title, .. } => title,
            Self::Import { .. } => "Импорт JSON",
        }
    }

    fn submit_label(&self) -> &'static str {
        match self {
            Self::Confirm { .. } => "Подтвердить",
            Self::Import { .. } => "Импортировать",
            _ => "Сохранить",
        }
    }

    fn body(&mut self, ui: &mut Ui, dataset: &Dataset) {
        match self {
            Self::Period(form) => form.ui(ui, dataset),
            Self::Category(form) => {
                text_field(ui, "Название", &mut form.value.name, "");
                ui.horizontal(|ui| {
                    ui.label("Цвет");
                    color_edit(ui, &mut form.color);
                });
            }
            Self::Zone(form) => form.ui(ui, dataset),
            Self::Confirm { body, .. } => {
                ui.label(*body);
            }
            Self::Import {
                name,
                archive,
                replace,
            } => {
                ui.label(name.as_str());
                ui.weak(archive_label(archive.scope));
                if let Some(periods) = &archive.periods {
                    ui.label(format!("Сессий в архиве: {}", periods.len()));
                }
                if let Some(categories) = &archive.categories {
                    ui.label(format!("Категорий: {}", categories.len()));
                }
                if let Some(zones) = &archive.zones {
                    ui.label(format!("Зон: {}", zones.len()));
                }
                if archive.settings.is_some() {
                    ui.label("Настройки из архива будут применены.");
                }
                if archive.periods.is_some() || archive.categories.is_some() {
                    ui.checkbox(replace, "Заменить историю и категории из архива");
                    if *replace {
                        ui.colored_label(
                            ui.visuals().error_fg_color,
                            "Текущие записи соответствующих разделов будут удалены.",
                        );
                    } else {
                        ui.weak("Будут добавлены записи, которых ещё нет в истории.");
                    }
                }
            }
        }
    }

    fn command(&self) -> Result<Command, String> {
        match self {
            Self::Period(form) => form.command(),
            Self::Category(form) => {
                let mut category = form.value.clone();
                category.color = color_string(form.color);
                Ok(Command::SaveCategory(form.id, category))
            }
            Self::Zone(form) => form.command(),
            Self::Confirm { command, .. } => Ok(command.clone()),
            Self::Import {
                archive, replace, ..
            } => Ok(Command::Import(archive.clone(), *replace)),
        }
    }
}

impl Tracker {
    pub(crate) fn show_editor(&mut self, ctx: &Context, now: DateTime<Utc>) {
        let Some(editor) = self.editor.as_mut() else {
            return;
        };
        let busy = self.pending.is_some();
        let mut cancel = false;
        let mut submit = false;
        let label = editor.submit_label();
        let response = Modal::new(Id::new("tracking-editor")).show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 58.0).clamp(245.0, 490.0));
            ui.heading(editor.title());
            ScrollArea::vertical()
                .max_height((ctx.content_rect().height() - 145.0).max(130.0))
                .show(ui, |ui| {
                    ui.add_enabled_ui(!busy, |ui| editor.body(ui, &self.state.dataset));
                });
            if let Some(error) = &self.error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            ui.separator();
            ui.add_enabled_ui(!busy, |ui| {
                ui.horizontal_wrapped(|ui| {
                    submit = primary(ui, label).clicked();
                    cancel = ui.button("Отмена").clicked();
                });
            });
        });
        if busy {
            return;
        }
        if cancel || response.should_close() {
            self.editor = None;
            self.error = None;
        } else if submit {
            match editor.command() {
                Ok(command) => self.dispatch(command, now, ctx, true),
                Err(error) => self.error = Some(error),
            }
        }
    }
}

impl PeriodForm {
    fn ui(&mut self, ui: &mut Ui, dataset: &Dataset) {
        text_field(ui, "Начало", &mut self.start, "ГГГГ-ММ-ДД ЧЧ:ММ");
        text_field(ui, "Конец", &mut self.end, "ГГГГ-ММ-ДД ЧЧ:ММ");
        ui.horizontal_wrapped(|ui| {
            for kind in [Kind::Work, Kind::Break] {
                ui.selectable_value(&mut self.kind, kind, kind_name(kind));
            }
            category_picker(ui, "period-category", dataset, &mut self.category);
        });
        let label = ui.label("Заметка");
        ui.add(
            egui::TextEdit::multiline(&mut self.note)
                .desired_rows(3)
                .desired_width(ui.available_width()),
        )
        .labelled_by(label.id);
        if self
            .original
            .as_ref()
            .is_some_and(|period| !period.pauses.is_empty())
        {
            ui.checkbox(&mut self.clear_pauses, "Удалить записанные паузы");
        }
    }

    fn command(&self) -> Result<Command, String> {
        let mut period = self.original.clone().unwrap_or_else(|| Period {
            start: DateTime::<Utc>::UNIX_EPOCH,
            end: DateTime::<Utc>::UNIX_EPOCH,
            note: String::new(),
            category: None,
            kind: Kind::Work,
            pauses: Vec::new(),
        });
        period.start = parse_date(
            &self.start,
            self.original.as_ref().map(|period| period.start),
        )?;
        period.end = parse_date(&self.end, self.original.as_ref().map(|period| period.end))?;
        period.note.clone_from(&self.note);
        period.kind = self.kind;
        period.category = self.category;
        if self.clear_pauses {
            period.pauses.clear();
        }
        Ok(Command::SavePeriod(self.id, period))
    }
}

impl ZoneForm {
    fn ui(&mut self, ui: &mut Ui, dataset: &Dataset) {
        text_field(ui, "Название", &mut self.value.name, "");
        text_field(ui, "Начало", &mut self.start, "ЧЧ:ММ");
        text_field(ui, "Конец", &mut self.end, "ЧЧ:ММ");
        ui.label("Дни недели");
        ui.horizontal_wrapped(|ui| {
            for day in weekday_order(&dataset.settings.first_day_of_week) {
                let selected = self.value.weekdays.contains(&day);
                if ui.selectable_label(selected, weekday_label(day)).clicked() {
                    if selected {
                        self.value.weekdays.retain(|existing| *existing != day);
                    } else {
                        self.value.weekdays.push(day);
                    }
                }
            }
        });
        text_field(ui, "Действует с", &mut self.starts_on, "ГГГГ-ММ-ДД");
        text_field(
            ui,
            "Действует по (включительно)",
            &mut self.ends_on,
            "ГГГГ-ММ-ДД",
        );
        ui.weak("Пустая дата оставляет границу открытой.");
        let label = ui.label("Даты-исключения");
        ui.add(
            egui::TextEdit::multiline(&mut self.excluded_dates)
                .hint_text("ГГГГ-ММ-ДД")
                .desired_rows(2)
                .desired_width(ui.available_width()),
        )
        .labelled_by(label.id);
        ui.weak("По одной дате в строке.");
        ui.label("Родительская зона");
        let parent = self.value.parent.and_then(|id| dataset.zones.get(&id));
        ComboBox::from_id_salt("zone-parent")
            .selected_text(parent.map_or("Основная зона", |zone| &zone.name))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.value.parent, None, "Основная зона");
                for (&id, zone) in &dataset.zones {
                    if id != self.id && zone.parent.is_none() {
                        ui.selectable_value(&mut self.value.parent, Some(id), &zone.name);
                    }
                }
            });
        if self.value.parent.is_none() {
            ui.horizontal_wrapped(|ui| {
                ui.label("Оформление");
                ui.selectable_value(&mut self.value.style, ZoneStyle::Solid, "Заливка");
                ui.selectable_value(&mut self.value.style, ZoneStyle::Border, "Контур");
                color_edit(ui, &mut self.color);
            });
            ui.label("Ожидаемая категория");
            category_picker(ui, "zone-category", dataset, &mut self.value.category);
            ui.checkbox(
                &mut self.value.exclude_from_breakdown,
                "Скрыть зону из таймлайна и сводки",
            );
        }
        ui.checkbox(
            &mut self.value.full_range,
            "Показывать зону целиком в режиме «От первой сессии»",
        );
    }

    fn command(&self) -> Result<Command, String> {
        let mut zone = self.value.clone();
        zone.start = NaiveTime::parse_from_str(self.start.trim(), "%H:%M")
            .map_err(|_| "Время начала должно быть в формате ЧЧ:ММ.")?;
        zone.end = NaiveTime::parse_from_str(self.end.trim(), "%H:%M")
            .map_err(|_| "Время конца должно быть в формате ЧЧ:ММ.")?;
        zone.starts_on = parse_optional_zone_date(&self.starts_on, "Дата начала действия")?;
        zone.ends_on = parse_optional_zone_date(&self.ends_on, "Дата окончания действия")?;
        zone.excluded_dates = self
            .excluded_dates
            .lines()
            .enumerate()
            .map(|(index, line)| (index + 1, line.trim()))
            .filter(|&(_, line)| !line.is_empty())
            .map(|(line_number, line)| {
                NaiveDate::parse_from_str(line, "%Y-%m-%d").map_err(|_| {
                    format!("Укажите дату исключения в строке {line_number} в формате ГГГГ-ММ-ДД.")
                })
            })
            .collect::<Result<_, _>>()?;
        zone.color = Some(color_string(self.color));
        if zone.parent.is_none() && zone.style == ZoneStyle::Hatched {
            zone.style = ZoneStyle::Solid;
        }
        Ok(Command::SaveZone(self.id, zone))
    }
}

fn parse_optional_zone_date(text: &str, title: &str) -> Result<Option<NaiveDate>, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    NaiveDate::parse_from_str(text, "%Y-%m-%d")
        .map(Some)
        .map_err(|_| format!("{title} должна быть в формате ГГГГ-ММ-ДД."))
}

fn text_field(ui: &mut Ui, title: &str, value: &mut String, hint: &str) {
    let label = ui.label(title);
    ui.add(
        egui::TextEdit::singleline(value)
            .hint_text(hint)
            .desired_width(ui.available_width()),
    )
    .labelled_by(label.id);
}

fn color_edit(ui: &mut Ui, color: &mut Color32) {
    let mut rgb = [color.r(), color.g(), color.b()];
    if ui.color_edit_button_srgb(&mut rgb).changed() {
        *color = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
    }
}

fn parse_date(text: &str, original: Option<DateTime<Utc>>) -> Result<DateTime<Utc>, String> {
    let text = text.trim();
    if let Some(date) = original
        && date
            .with_timezone(&Local)
            .format("%Y-%m-%d %H:%M")
            .to_string()
            == text
    {
        return Ok(date);
    }
    let parsed = NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M")
        .map_err(|_| "Введите дату и время в формате ГГГГ-ММ-ДД ЧЧ:ММ.")?;
    Local
        .from_local_datetime(&parsed)
        .earliest()
        .map(|date| date.with_timezone(&Utc))
        .ok_or_else(|| "В местном часовом поясе это время пропущено.".into())
}

pub(crate) fn weekday_order(first: &str) -> [u8; 7] {
    let start = match first {
        "sunday" => 0,
        "saturday" => 6,
        _ => 1,
    };
    std::array::from_fn(|index| (start + index as u8) % 7)
}

pub(crate) fn weekday_label(day: u8) -> &'static str {
    ["Вс", "Пн", "Вт", "Ср", "Чт", "Пт", "Сб"][usize::from(day % 7)]
}
