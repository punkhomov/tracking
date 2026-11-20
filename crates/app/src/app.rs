use std::{
    path::PathBuf,
    sync::mpsc,
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result, bail, ensure};
use chrono::{DateTime, Local, NaiveDateTime, NaiveTime, TimeZone, Utc};
use gpui::{App, AppContext, Context, Entity, PathPromptOptions, Window};
use gpui_component::{
    Theme, ThemeMode,
    input::{InputEvent, InputState},
};
use tracking_core::{
    Category, Dataset, Kind, Period, Settings, Timer, Zone, ZoneStyle, export_json, import_json,
};
use tracking_data::{
    Conflict, Database, LocalState, Preference,
    cloud::{Session, Supabase},
    merge,
};
use uuid::Uuid;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Today,
    History,
    Stats,
    Categories,
    Zones,
    Settings,
}
impl Tab {
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

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Span {
    Day,
    Week,
    Month,
}

#[derive(Clone)]
pub enum Action {
    Tab(Tab),
    Start,
    Pause,
    Finish,
    ToggleKind,
    ConfirmDiscard,
    Discard,
    Reopen,
    TimerCategory(Option<Uuid>),
    HistoryFilter(Option<Kind>),
    HistoryPage(usize),
    Span(Span),
    TimelineRange,
    Period(Option<Uuid>),
    Category(Option<Uuid>),
    Zone(Option<Uuid>),
    ChooseKind,
    ChooseCategory(Option<Uuid>),
    ZoneDay(u8),
    ZoneParent(Option<Uuid>),
    ZoneStyle(ZoneStyle),
    ZoneFullRange,
    ZoneExclude,
    ClearPauses,
    ConfirmDelete,
    Delete,
    Save,
    Cancel,
    SettingOption(SettingOption),
    SaveSettings,
    Import,
    Export,
    RequestCode,
    VerifyCode,
    SignOut,
    Sync(Preference),
}

#[derive(Clone, Copy)]
pub enum SettingOption {
    AutomaticBreak,
    AutomaticWork,
    Theme(&'static str),
    Week(&'static str),
    Date(&'static str),
    Hour(&'static str),
}

type Field = Entity<InputState>;

pub struct PeriodForm {
    pub id: Uuid,
    pub period: Option<Period>,
    pub start: Field,
    pub end: Field,
    pub note: Field,
    pub kind: Kind,
    pub category: Option<Uuid>,
    pub clear_pauses: bool,
}

pub struct CategoryForm {
    pub id: Uuid,
    pub name: Field,
    pub color: Field,
    pub archived: bool,
}

pub struct ZoneForm {
    pub id: Uuid,
    pub name: Field,
    pub start: Field,
    pub end: Field,
    pub color: Field,
    pub weekdays: Vec<u8>,
    pub parent: Option<Uuid>,
    pub category: Option<Uuid>,
    pub style: ZoneStyle,
    pub full_range: bool,
    pub exclude: bool,
}

pub enum Form {
    Period(PeriodForm),
    Category(CategoryForm),
    Zone(ZoneForm),
}

pub struct SettingsForm {
    pub work: Field,
    pub rest: Field,
    pub reminder: Field,
    pub pause: Field,
    pub draft: Settings,
}

impl SettingsForm {
    fn new(settings: &Settings, window: &mut Window, cx: &mut App) -> Self {
        Self {
            work: field("25", settings.work_minutes.to_string(), window, cx),
            rest: field("5", settings.break_minutes.to_string(), window, cx),
            reminder: field(
                "5",
                settings.reminder_interval_minutes.to_string(),
                window,
                cx,
            ),
            pause: field("10", settings.min_pause_seconds.to_string(), window, cx),
            draft: settings.clone(),
        }
    }

    fn read(&self, cx: &App) -> Result<Settings> {
        let mut settings = self.draft.clone();
        settings.work_minutes = read(&self.work, cx)
            .parse()
            .context("Проверьте длительность работы.")?;
        settings.break_minutes = read(&self.rest, cx)
            .parse()
            .context("Проверьте длительность перерыва.")?;
        settings.reminder_interval_minutes = read(&self.reminder, cx)
            .parse()
            .context("Проверьте интервал напоминаний.")?;
        settings.min_pause_seconds = read(&self.pause, cx)
            .parse()
            .context("Проверьте порог паузы.")?;
        settings.validate()?;
        Ok(settings)
    }
}

pub struct Tracker {
    database: Database,
    pub state: LocalState,
    pub tab: Tab,
    pub note: Field,
    pub category: Option<Uuid>,
    pub search: Field,
    pub email: Field,
    pub otp: Field,
    pub history_kind: Option<Kind>,
    pub history_page: usize,
    pub stats_span: Span,
    pub full_day: bool,
    pub form: Option<Form>,
    pub confirm: bool,
    pub settings_form: SettingsForm,
    pub feedback: Option<(bool, String)>,
    pub reminder: Option<&'static str>,
    reminder_tick: Option<(Uuid, i64)>,
    pub cloud: Option<Supabase>,
    pub cloud_error: Option<String>,
    pub session: Option<Session>,
    pending: Option<mpsc::Receiver<CloudReply>>,
    pub cloud_label: String,
    pub conflict: bool,
    last_sync_attempt: Instant,
    last_second: i64,
}

enum CloudReply {
    Code(Result<()>),
    Login(Result<Session>),
    Synced {
        session: Session,
        sent: Box<Dataset>,
        result: Box<Result<Dataset>>,
    },
}

impl Tracker {
    pub fn new(
        database: Database,
        state: LocalState,
        cloud: Result<Option<Supabase>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        apply_theme(&state.dataset.settings, window, cx);
        let (cloud, cloud_error) = match cloud {
            Ok(cloud) => (cloud, None),
            Err(error) => (None, Some(error.to_string())),
        };
        let search = field("Заметка или категория", "", window, cx);
        cx.subscribe(&search, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.history_page = 0;
                cx.notify();
            }
        })
        .detach();
        cx.observe_window_appearance(window, |this, window, cx| {
            if this.state.dataset.settings.theme == "system" {
                apply_theme(&this.state.dataset.settings, window, cx);
            }
        })
        .detach();
        cx.spawn_in(window, async |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(250))
                    .await;
                if this
                    .update_in(cx, |this, window, cx| this.tick(window, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        Self {
            database,
            settings_form: SettingsForm::new(&state.dataset.settings, window, cx),
            state,
            tab: Tab::Today,
            note: field("Чем займётесь?", "", window, cx),
            search,
            email: field("you@example.com", "", window, cx),
            otp: field("Код из письма", "", window, cx),
            category: None,
            history_kind: None,
            history_page: 0,
            stats_span: Span::Day,
            full_day: true,
            form: None,
            confirm: false,
            feedback: None,
            reminder: None,
            reminder_tick: None,
            cloud,
            cloud_error,
            session: None,
            pending: None,
            cloud_label: "Локальные данные".into(),
            conflict: false,
            last_sync_attempt: Instant::now(),
            last_second: 0,
        }
    }

    pub fn cloud_busy(&self) -> bool {
        self.pending.is_some()
    }

    fn commit(&mut self, next: LocalState) -> Result<()> {
        next.dataset.validate()?;
        self.database.save(&next)?;
        self.state = next;
        Ok(())
    }

    fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let now = Utc::now();
        if let Some(timer) = &self.state.timer
            && !timer.is_paused()
        {
            let settings = &self.state.dataset.settings;
            let over = timer.elapsed_seconds(now) - settings.target_seconds(timer.kind);
            if over >= 0 {
                let bucket = over / (i64::from(settings.reminder_interval_minutes) * 60);
                if self
                    .reminder_tick
                    .is_none_or(|last| last != (timer.id, bucket))
                {
                    self.reminder_tick = Some((timer.id, bucket));
                    let message = match timer.kind {
                        Kind::Work => "Рабочая цель достигнута. Можно сделать перерыв.",
                        Kind::Break => "Перерыв окончен. Можно вернуться к работе.",
                    };
                    self.reminder = Some(message);
                    std::thread::spawn(move || {
                        let _ = notify_rust::Notification::new()
                            .summary("Tracking")
                            .body(message)
                            .show();
                    });
                    cx.notify();
                }
            }
        }
        if now.timestamp() != self.last_second {
            self.last_second = now.timestamp();
            if self.tab == Tab::Today {
                cx.notify();
            }
        }
        if let Some(pending) = &self.pending {
            match pending.try_recv() {
                Ok(reply) => {
                    self.pending = None;
                    self.handle_reply(reply, window, cx);
                    cx.notify();
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.pending = None;
                    self.cloud_label = "Запрос завершился без ответа".into();
                    cx.notify();
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if self.session.is_some()
            && !self.cloud_busy()
            && !self.conflict
            && self.last_sync_attempt.elapsed() >= Duration::from_secs(60)
        {
            self.start_sync(Preference::Ask);
            cx.notify();
        }
    }

    pub fn act(&mut self, action: Action, window: &mut Window, cx: &mut Context<Self>) {
        self.feedback = None;
        if let Err(error) = self.handle(action, window, cx) {
            self.feedback = Some((true, format!("{error:#}")));
        }
        cx.notify();
    }

    fn handle(
        &mut self,
        action: Action,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        match action {
            Action::Tab(tab) => {
                self.tab = tab;
                self.form = None;
                self.confirm = false;
                if tab == Tab::Settings {
                    self.settings_form =
                        SettingsForm::new(&self.state.dataset.settings, window, cx);
                }
            }
            Action::Start
            | Action::Pause
            | Action::Finish
            | Action::ToggleKind
            | Action::Discard
            | Action::Reopen => {
                self.change_timer(action, window, cx)?;
                self.confirm = false;
            }
            Action::ConfirmDiscard | Action::ConfirmDelete => self.confirm = true,
            Action::TimerCategory(category) => self.category = category,
            Action::HistoryFilter(kind) => {
                self.history_kind = kind;
                self.history_page = 0;
            }
            Action::HistoryPage(page) => self.history_page = page,
            Action::Span(span) => self.stats_span = span,
            Action::TimelineRange => self.full_day = !self.full_day,
            Action::Period(id) => self.period_form(id, window, cx),
            Action::Category(id) => self.category_form(id, window, cx),
            Action::Zone(id) => self.zone_form(id, window, cx),
            Action::ChooseKind => {
                if let Some(Form::Period(form)) = &mut self.form {
                    form.kind = form.kind.next();
                }
            }
            Action::ChooseCategory(category) => match &mut self.form {
                Some(Form::Period(form)) => form.category = category,
                Some(Form::Zone(form)) => form.category = category,
                _ => {}
            },
            Action::ZoneDay(day) => {
                if let Some(Form::Zone(form)) = &mut self.form {
                    if form.weekdays.contains(&day) {
                        form.weekdays.retain(|&value| value != day);
                    } else {
                        form.weekdays.push(day);
                        form.weekdays.sort_unstable();
                    }
                }
            }
            Action::ZoneParent(parent) => {
                if let Some(Form::Zone(form)) = &mut self.form {
                    form.parent = parent;
                    form.style = if parent.is_some() {
                        ZoneStyle::Hatched
                    } else {
                        ZoneStyle::Solid
                    };
                }
            }
            Action::ZoneStyle(style) => {
                if let Some(Form::Zone(form)) = &mut self.form {
                    form.style = style;
                }
            }
            Action::ZoneFullRange => {
                if let Some(Form::Zone(form)) = &mut self.form {
                    form.full_range = !form.full_range;
                }
            }
            Action::ZoneExclude => {
                if let Some(Form::Zone(form)) = &mut self.form {
                    form.exclude = !form.exclude;
                }
            }
            Action::ClearPauses => {
                if let Some(Form::Period(form)) = &mut self.form {
                    form.clear_pauses = !form.clear_pauses;
                }
            }
            Action::Save | Action::Delete => {
                self.save_form(matches!(action, Action::Delete), cx)?
            }
            Action::Cancel => {
                if self.confirm {
                    self.confirm = false;
                } else {
                    self.form = None;
                }
            }
            Action::SettingOption(option) => self.setting_option(option),
            Action::SaveSettings => {
                let mut next = self.state.clone();
                next.dataset.settings = self.settings_form.read(cx)?;
                self.commit(next)?;
                apply_theme(&self.state.dataset.settings, window, cx);
                self.feedback = Some((false, "Настройки сохранены".into()));
            }
            Action::Import => {
                let receiver = cx.prompt_for_paths(PathPromptOptions {
                    files: true,
                    directories: false,
                    multiple: false,
                    prompt: Some("Импортировать JSON".into()),
                });
                cx.spawn_in(window, async move |this, cx| {
                    let result = receiver
                        .await
                        .map_err(anyhow::Error::from)
                        .and_then(|value| value);
                    this.update_in(cx, |this, window, cx| {
                        let result = result.and_then(|paths| {
                            if let Some(path) = paths.and_then(|paths| paths.into_iter().next()) {
                                this.import(path, window, cx)?;
                            }
                            Ok(())
                        });
                        if let Err(error) = result {
                            this.feedback = Some((true, format!("Импорт не выполнен. {error:#}")));
                        }
                        cx.notify();
                    })
                    .ok();
                })
                .detach();
            }
            Action::Export => {
                let directory = directories::UserDirs::new()
                    .and_then(|dirs| dirs.document_dir().map(PathBuf::from))
                    .unwrap_or(std::env::current_dir()?);
                let receiver = cx.prompt_for_new_path(&directory, Some("tracking-backup.json"));
                cx.spawn(async move |this, cx| {
                    let result = receiver
                        .await
                        .map_err(anyhow::Error::from)
                        .and_then(|value| value);
                    this.update(cx, |this, cx| {
                        let result = result.and_then(|path| {
                            if let Some(path) = path {
                                write_archive(&path, &export_json(&this.state.dataset)?)?;
                                this.feedback = Some((false, "Архив сохранён".into()));
                            }
                            Ok(())
                        });
                        if let Err(error) = result {
                            this.feedback = Some((true, format!("Экспорт не выполнен. {error:#}")));
                        }
                        cx.notify();
                    })
                    .ok();
                })
                .detach();
            }
            Action::RequestCode => {
                if let Some(cloud) = self.available_cloud() {
                    let email = read(&self.email, cx);
                    self.run_cloud(move || CloudReply::Code(cloud.request_code(email.trim())));
                }
            }
            Action::VerifyCode => {
                if let Some(cloud) = self.available_cloud() {
                    let email = read(&self.email, cx);
                    let code = read(&self.otp, cx);
                    self.run_cloud(move || {
                        CloudReply::Login(cloud.verify_code(email.trim(), code.trim()))
                    });
                }
            }
            Action::SignOut => {
                if !self.cloud_busy() {
                    self.session = None;
                    self.conflict = false;
                    self.cloud_label = "Локальные данные".into();
                }
            }
            Action::Sync(preference) => self.start_sync(preference),
        }
        Ok(())
    }

    fn change_timer(
        &mut self,
        action: Action,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        let mut next = self.state.clone();
        let now = Utc::now();
        let settings = &next.dataset.settings;
        match action {
            Action::Start => match &mut next.timer {
                Some(timer) => timer.resume(now, settings),
                None => {
                    let work = next.next_kind == Kind::Work;
                    let note = if work {
                        read(&self.note, cx).trim().to_owned()
                    } else {
                        String::new()
                    };
                    let category = self.category.filter(|id| {
                        work && next.dataset.categories.get(id).is_some_and(|c| !c.archived)
                    });
                    next.timer = Some(Timer::start(next.next_kind, now, note, category));
                }
            },
            Action::Pause => {
                if let Some(timer) = &mut next.timer {
                    timer.pause(now);
                }
            }
            Action::Finish => {
                if let Some(timer) = next.timer.take() {
                    let (id, kind) = (timer.id, timer.kind);
                    next.dataset
                        .periods
                        .insert(id, timer.finish(now, settings)?);
                    next.next_kind = kind.next();
                    if (kind == Kind::Work && settings.auto_start_break)
                        || (kind == Kind::Break && settings.auto_start_pomodoro)
                    {
                        next.timer = Some(Timer::start(next.next_kind, now, String::new(), None));
                    }
                }
            }
            Action::ToggleKind if next.timer.is_none() => next.next_kind = next.next_kind.next(),
            Action::Discard => next.timer = None,
            Action::Reopen if next.timer.is_none() => {
                if let Some((id, session)) = next
                    .dataset
                    .periods
                    .iter()
                    .filter(|(_, period)| period.end <= now)
                    .max_by_key(|(_, period)| period.end)
                {
                    next.next_kind = session.kind;
                    next.timer = Some(Timer::reopen(*id, session));
                }
            }
            _ => {}
        }
        self.commit(next)?;
        if matches!(action, Action::Finish | Action::Discard) {
            self.note.update(cx, |input, cx| {
                input.set_value("", window, cx);
            });
            self.reminder = None;
        }
        Ok(())
    }

    fn setting_option(&mut self, option: SettingOption) {
        let settings = &mut self.settings_form.draft;
        match option {
            SettingOption::AutomaticBreak => settings.auto_start_break = !settings.auto_start_break,
            SettingOption::AutomaticWork => {
                settings.auto_start_pomodoro = !settings.auto_start_pomodoro
            }
            SettingOption::Theme(value) => settings.theme = value.to_owned(),
            SettingOption::Week(value) => settings.first_day_of_week = value.to_owned(),
            SettingOption::Date(value) => settings.date_format = value.to_owned(),
            SettingOption::Hour(value) => settings.hour_format = value.to_owned(),
        }
    }

    fn period_form(&mut self, id: Option<Uuid>, window: &mut Window, cx: &mut Context<Self>) {
        let period = id.and_then(|id| self.state.dataset.periods.get(&id).cloned());
        let now = Local::now();
        let date = |value: DateTime<Local>| value.format("%Y-%m-%d %H:%M").to_string();
        self.form = Some(Form::Period(PeriodForm {
            id: id.unwrap_or_else(Uuid::new_v4),
            start: field(
                "ГГГГ-ММ-ДД ЧЧ:ММ",
                date(
                    period
                        .as_ref()
                        .map_or(now - chrono::Duration::minutes(25), |p| {
                            p.start.with_timezone(&Local)
                        }),
                ),
                window,
                cx,
            ),
            end: field(
                "ГГГГ-ММ-ДД ЧЧ:ММ",
                date(period.as_ref().map_or(now, |p| p.end.with_timezone(&Local))),
                window,
                cx,
            ),
            note: field(
                "Заметка",
                period.as_ref().map_or_else(String::new, |p| p.note.clone()),
                window,
                cx,
            ),
            category: period.as_ref().and_then(|p| p.category),
            kind: period.as_ref().map_or(Kind::Work, |p| p.kind),
            period,
            clear_pauses: false,
        }));
        self.confirm = false;
    }

    fn category_form(&mut self, id: Option<Uuid>, window: &mut Window, cx: &mut Context<Self>) {
        let category = id.and_then(|id| self.state.dataset.categories.get(&id));
        self.form = Some(Form::Category(CategoryForm {
            id: id.unwrap_or_else(Uuid::new_v4),
            name: field(
                "Название",
                category.map_or_else(String::new, |c| c.name.clone()),
                window,
                cx,
            ),
            color: field(
                "#8aa9ff",
                category.map_or_else(|| "#8aa9ff".into(), |c| c.color.clone()),
                window,
                cx,
            ),
            archived: category.is_some_and(|category| category.archived),
        }));
        self.confirm = false;
    }

    fn zone_form(&mut self, id: Option<Uuid>, window: &mut Window, cx: &mut Context<Self>) {
        let zone = id.and_then(|id| self.state.dataset.zones.get(&id));
        let time = |value: NaiveTime| value.format("%H:%M").to_string();
        self.form = Some(Form::Zone(ZoneForm {
            id: id.unwrap_or_else(Uuid::new_v4),
            name: field(
                "Название",
                zone.map_or_else(String::new, |z| z.name.clone()),
                window,
                cx,
            ),
            start: field(
                "ЧЧ:ММ",
                zone.map_or_else(|| "09:00".into(), |z| time(z.start)),
                window,
                cx,
            ),
            end: field(
                "ЧЧ:ММ",
                zone.map_or_else(|| "18:00".into(), |z| time(z.end)),
                window,
                cx,
            ),
            color: field(
                "#39548d",
                zone.and_then(|z| z.color.clone())
                    .unwrap_or_else(|| "#39548d".into()),
                window,
                cx,
            ),
            weekdays: zone.map_or_else(|| vec![1, 2, 3, 4, 5], |z| z.weekdays.clone()),
            parent: zone.and_then(|z| z.parent),
            category: zone.and_then(|z| z.category),
            style: zone.map_or(ZoneStyle::Solid, |z| z.style),
            full_range: zone.is_some_and(|z| z.full_range),
            exclude: zone.is_some_and(|z| z.exclude_from_breakdown),
        }));
        self.confirm = false;
    }

    fn save_form(&mut self, delete: bool, cx: &App) -> Result<()> {
        let mut next = self.state.clone();
        match &self.form {
            Some(Form::Period(form)) => {
                ensure!(
                    next.timer.as_ref().is_none_or(|timer| timer.id != form.id),
                    "Сначала завершите активную сессию."
                );
                if delete {
                    next.dataset.periods.remove(&form.id);
                } else {
                    let previous = form.period.as_ref();
                    let period = Period {
                        start: parse_date(&read(&form.start, cx), previous.map(|p| p.start))?,
                        end: parse_date(&read(&form.end, cx), previous.map(|p| p.end))?,
                        note: read(&form.note, cx).trim().to_owned(),
                        category: form.category.filter(|_| form.kind == Kind::Work),
                        kind: form.kind,
                        pauses: previous
                            .filter(|_| !form.clear_pauses)
                            .map_or_else(Vec::new, |p| p.pauses.clone()),
                    };
                    ensure!(period.start < period.end, "Укажите конец позже начала.");
                    period.validate()?;
                    next.dataset.periods.insert(form.id, period);
                }
            }
            Some(Form::Category(form)) => {
                let color = read(&form.color, cx);
                ensure!(is_hex(&color), "Укажите цвет в формате #RRGGBB.");
                next.dataset.categories.insert(
                    form.id,
                    Category {
                        name: read(&form.name, cx).trim().to_owned(),
                        color,
                        archived: form.archived ^ delete,
                    },
                );
            }
            Some(Form::Zone(form)) => {
                if delete {
                    next.dataset
                        .zones
                        .retain(|id, zone| *id != form.id && zone.parent != Some(form.id));
                } else {
                    let color = read(&form.color, cx);
                    ensure!(is_hex(&color), "Укажите цвет в формате #RRGGBB.");
                    next.dataset.zones.insert(
                        form.id,
                        Zone {
                            name: read(&form.name, cx).trim().to_owned(),
                            start: NaiveTime::parse_from_str(read(&form.start, cx).trim(), "%H:%M")
                                .context("Проверьте время начала.")?,
                            end: NaiveTime::parse_from_str(read(&form.end, cx).trim(), "%H:%M")
                                .context("Проверьте время конца.")?,
                            weekdays: form.weekdays.clone(),
                            parent: form.parent,
                            color: Some(color),
                            style: form.style,
                            full_range: form.full_range,
                            exclude_from_breakdown: form.exclude,
                            category: form.category.filter(|_| form.parent.is_none()),
                        },
                    );
                }
            }
            None => return Ok(()),
        }
        self.commit(next)?;
        self.form = None;
        self.confirm = false;
        self.feedback = Some((false, "Изменения сохранены".into()));
        Ok(())
    }

    fn import(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Result<()> {
        let content = std::fs::read_to_string(path)?;
        let mut next = self.state.clone();
        next.dataset = import_json(&next.dataset, &content)?;
        self.commit(next)?;
        apply_theme(&self.state.dataset.settings, window, cx);
        self.settings_form = SettingsForm::new(&self.state.dataset.settings, window, cx);
        self.feedback = Some((false, "Данные из архива добавлены".into()));
        Ok(())
    }

    fn available_cloud(&self) -> Option<Supabase> {
        if self.cloud_busy() {
            None
        } else {
            self.cloud.clone()
        }
    }

    fn run_cloud(&mut self, job: impl FnOnce() -> CloudReply + Send + 'static) {
        let (send, receive) = mpsc::channel();
        self.pending = Some(receive);
        self.cloud_label = "Подключение…".into();
        std::thread::spawn(move || {
            let _ = send.send(job());
        });
    }

    fn start_sync(&mut self, preference: Preference) {
        let Some(cloud) = self.available_cloud() else {
            return;
        };
        let Some(mut session) = self.session.take() else {
            return;
        };
        let sent = self.state.dataset.clone();
        let base = self.state.baseline.clone();
        self.last_sync_attempt = Instant::now();
        self.run_cloud(move || {
            let result = cloud.synchronize(&mut session, &sent, base.as_ref(), preference);
            CloudReply::Synced {
                session,
                sent: Box::new(sent),
                result: Box::new(result),
            }
        });
    }

    fn handle_reply(&mut self, reply: CloudReply, window: &mut Window, cx: &mut Context<Self>) {
        let outcome = match reply {
            CloudReply::Code(result) => result.map(|()| "Код отправлен".into()),
            CloudReply::Login(result) => result.and_then(|session| {
                let cloud = self.cloud.as_ref().context("Supabase не настроен.")?;
                let account = cloud.account(&session);
                let mut next = self.state.clone();
                ensure!(
                    next.account.as_ref().is_none_or(|bound| *bound == account),
                    "Эта база уже связана с другим аккаунтом или проектом."
                );
                next.account = Some(account);
                self.commit(next)?;
                self.session = Some(session);
                self.otp.update(cx, |input, cx| {
                    input.set_value("", window, cx);
                });
                self.start_sync(Preference::Ask);
                Ok("Синхронизация…".into())
            }),
            CloudReply::Synced {
                session,
                sent,
                result,
            } => {
                self.session = Some(session);
                self.conflict = result
                    .as_ref()
                    .as_ref()
                    .err()
                    .is_some_and(|error| error.is::<Conflict>());
                result.and_then(|remote| {
                    let mut next = self.state.clone();
                    next.dataset = merge(&sent, &next.dataset, &remote, Preference::Local)?;
                    next.baseline = Some(remote);
                    let settings_changed = next.dataset.settings != self.state.dataset.settings;
                    self.commit(next)?;
                    if settings_changed {
                        apply_theme(&self.state.dataset.settings, window, cx);
                        self.settings_form =
                            SettingsForm::new(&self.state.dataset.settings, window, cx);
                    }
                    Ok(format!(
                        "Синхронизировано в {}",
                        Local::now().format("%H:%M")
                    ))
                })
            }
        };
        self.cloud_label = outcome.unwrap_or_else(|error| format!("{error:#}"));
    }
}

pub fn read(input: &Field, cx: &App) -> String {
    input.read(cx).value().to_string()
}

fn field(
    placeholder: &'static str,
    value: impl Into<gpui::SharedString>,
    window: &mut Window,
    cx: &mut App,
) -> Field {
    cx.new(|cx| {
        InputState::new(window, cx)
            .placeholder(placeholder)
            .default_value(value)
    })
}

fn parse_date(value: &str, previous: Option<DateTime<Utc>>) -> Result<DateTime<Utc>> {
    let naive = NaiveDateTime::parse_from_str(value.trim(), "%Y-%m-%d %H:%M")
        .context("Введите дату и время в формате ГГГГ-ММ-ДД ЧЧ:ММ.")?;
    if let Some(previous) = previous
        && previous
            .with_timezone(&Local)
            .format("%Y-%m-%d %H:%M")
            .to_string()
            == value.trim()
    {
        return Ok(previous);
    }
    match Local.from_local_datetime(&naive) {
        chrono::LocalResult::Single(time) => Ok(time.with_timezone(&Utc)),
        _ => bail!("Это местное время пропущено или повторяется при переводе часов."),
    }
}

fn is_hex(value: &str) -> bool {
    value.len() == 7
        && value.starts_with('#')
        && value.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit)
}

fn apply_theme(settings: &Settings, window: &mut Window, cx: &mut App) {
    let mode = match settings.theme.as_str() {
        "light" => ThemeMode::Light,
        "dark" => ThemeMode::Dark,
        _ => window.appearance().into(),
    };
    Theme::change(mode, Some(window), cx);
}

fn write_archive(path: &std::path::Path, content: &str) -> Result<()> {
    use std::io::Write;
    let directory = path.parent().context("У файла нет родительской папки.")?;
    let mut output = tempfile::NamedTempFile::new_in(directory)?;
    output.write_all(content.as_bytes())?;
    output.as_file().sync_all()?;
    output.persist(path)?;
    Ok(())
}
