use chrono::{DateTime, Datelike, Days, Local, LocalResult, NaiveDate, NaiveTime, TimeZone, Utc};
use gpui::{
    AnyElement, App, Context, Div, FontWeight, IntoElement, Render, Rgba, SharedString, Window,
    div, prelude::*, px, relative, rgb,
};
use gpui_component::{Theme, ThemeMode, input::Input};
use tracking_core::{Kind, Moments, Period, Settings, ZoneStyle, summarize};
use tracking_data::Preference;
use uuid::Uuid;

use crate::app::{Action, Form, SettingOption, Span, Tab, Tracker, read};

const PAGE_SIZE: usize = 30;
const DAYS: [&str; 7] = ["Вс", "Пн", "Вт", "Ср", "Чт", "Пт", "Сб"];
const MONTHS: [&str; 12] = [
    "января",
    "февраля",
    "марта",
    "апреля",
    "мая",
    "июня",
    "июля",
    "августа",
    "сентября",
    "октября",
    "ноября",
    "декабря",
];

#[derive(Clone, Copy)]
struct Palette {
    bg: Rgba,
    sidebar: Rgba,
    tile: Rgba,
    hover: Rgba,
    border: Rgba,
    fg: Rgba,
    muted: Rgba,
    accent: Rgba,
    accent_text: Rgba,
    selected: Rgba,
    rest: Rgba,
    alert: Rgba,
}

impl Palette {
    fn get(cx: &App) -> Self {
        let colors = if Theme::global(cx).mode == ThemeMode::Dark {
            [
                0x101217, 0x151820, 0x181c25, 0x242c3a, 0x303746, 0xf1f3f8, 0xa4afc2, 0xaec2ff,
                0x19243d, 0x26344f, 0x79d7be, 0xffaf9b,
            ]
        } else {
            [
                0xf2f4f8, 0xe9edf5, 0xffffff, 0xe8edf8, 0xd5dce8, 0x20273a, 0x515d74, 0x3955ac,
                0xffffff, 0xdde6ff, 0x18765e, 0xa5352f,
            ]
        };
        let [
            bg,
            sidebar,
            tile,
            hover,
            border,
            fg,
            muted,
            accent,
            accent_text,
            selected,
            rest,
            alert,
        ] = colors.map(rgb);
        Self {
            bg,
            sidebar,
            tile,
            hover,
            border,
            fg,
            muted,
            accent,
            accent_text,
            selected,
            rest,
            alert,
        }
    }
}

#[derive(Clone, Copy)]
enum Button {
    Plain,
    Select,
    Primary,
    Danger,
}

impl Render for Tracker {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::get(cx);
        let content = if let Some(form) = &self.form {
            self.editor(form, p, cx)
        } else {
            match self.tab {
                Tab::Today => self.today(p, cx),
                Tab::History => self.history(p, cx),
                Tab::Stats => self.stats(p, cx),
                Tab::Categories => self.categories(p, cx),
                Tab::Zones => self.zones(p, cx),
                Tab::Settings => self.settings(p, cx),
            }
        };
        div()
            .size_full()
            .flex()
            .bg(p.bg)
            .text_color(p.fg)
            .child(self.sidebar(p, cx))
            .child(
                column()
                    .size_full()
                    .min_w_0()
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .items_center()
                            .justify_between()
                            .border_b_1()
                            .border_color(p.border)
                            .px_8()
                            .py_5()
                            .child(
                                div()
                                    .text_2xl()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(self.tab.title()),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(p.muted)
                                    .child(long_date(Local::now().date_naive())),
                            ),
                    )
                    .child(
                        div()
                            .id("screen")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .child(
                                column()
                                    .w_full()
                                    .max_w(px(1140.0))
                                    .mx_auto()
                                    .p_8()
                                    .gap_5()
                                    .when_some(
                                        self.feedback.as_ref(),
                                        |element, (error, message)| {
                                            element.child(
                                                panel(p)
                                                    .text_color(if *error {
                                                        p.alert
                                                    } else {
                                                        p.rest
                                                    })
                                                    .child(message.clone()),
                                            )
                                        },
                                    )
                                    .child(content),
                            ),
                    ),
            )
    }
}

impl Tracker {
    fn sidebar(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let status = if self.cloud_error.is_some() {
            "Ошибка настройки"
        } else if self.cloud_busy() {
            "Обмен данными…"
        } else if self.session.is_some() {
            "Supabase подключён"
        } else {
            "Локальное хранение"
        };
        column()
            .flex_none()
            .w(px(206.0))
            .h_full()
            .bg(p.sidebar)
            .border_r_1()
            .border_color(p.border)
            .p_4()
            .gap_1()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .p_2()
                    .mb_6()
                    .child(
                        div()
                            .flex()
                            .size(px(36.0))
                            .items_center()
                            .justify_center()
                            .rounded(px(10.0))
                            .bg(p.accent)
                            .text_color(p.accent_text)
                            .font_weight(FontWeight::BOLD)
                            .child("T"),
                    )
                    .child(
                        div()
                            .text_xl()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Tracking"),
                    ),
            )
            .children(Tab::ALL.into_iter().map(|tab| {
                let selected = self.tab == tab;
                let hover = p.hover;
                div()
                    .id(tab.title())
                    .cursor_pointer()
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .bg(if selected { p.selected } else { p.sidebar })
                    .text_color(if selected { p.accent } else { p.fg })
                    .hover(move |style| style.bg(hover))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.act(Action::Tab(tab), window, cx)
                    }))
                    .child(tab.title())
            }))
            .child(
                div()
                    .mt_auto()
                    .px_3()
                    .py_2()
                    .text_xs()
                    .text_color(p.muted)
                    .child(status),
            )
    }

    fn today(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let now = Utc::now();
        let settings = &self.state.dataset.settings;
        let timer = self.state.timer.as_ref();
        let kind = timer.map_or(self.state.next_kind, |timer| timer.kind);
        let target = settings.target_seconds(kind);
        let elapsed = timer.map_or(0, |timer| timer.elapsed_seconds(now));
        let paused = timer.is_some_and(tracking_core::Timer::is_paused);
        let value = if elapsed > target {
            format!("+{}", clock(elapsed - target))
        } else {
            clock(target - elapsed)
        };
        let timer_color = if kind == Kind::Work { p.accent } else { p.rest };
        let timer_label = if paused {
            "На паузе"
        } else {
            kind_name(kind)
        };

        let controls = if timer.is_some() {
            let start_pause = if paused {
                ("Продолжить", Action::Start)
            } else {
                ("Пауза", Action::Pause)
            };
            row()
                .child(button(
                    "pause",
                    start_pause.0,
                    Button::Plain,
                    start_pause.1,
                    p,
                    cx,
                ))
                .child(button(
                    "finish",
                    "Сохранить сессию",
                    Button::Primary,
                    Action::Finish,
                    p,
                    cx,
                ))
                .child(button(
                    "discard",
                    "Сбросить",
                    Button::Plain,
                    Action::ConfirmDiscard,
                    p,
                    cx,
                ))
        } else {
            row()
                .child(button(
                    "start",
                    "Запустить",
                    Button::Primary,
                    Action::Start,
                    p,
                    cx,
                ))
                .child(button(
                    "mode",
                    if kind == Kind::Work {
                        "Выбрать перерыв"
                    } else {
                        "Выбрать работу"
                    },
                    Button::Plain,
                    Action::ToggleKind,
                    p,
                    cx,
                ))
                .when(
                    self.state
                        .dataset
                        .periods
                        .values()
                        .any(|period| period.end <= now),
                    |element| {
                        element.child(button(
                            "reopen",
                            "Продолжить последнюю",
                            Button::Plain,
                            Action::Reopen,
                            p,
                            cx,
                        ))
                    },
                )
        };
        let work_details = if kind == Kind::Work && timer.is_none() {
            column()
                .gap_3()
                .child(Input::new(&self.note))
                .child(self.category_picker(self.category, true, p, cx))
        } else if let Some(timer) = timer {
            column().text_sm().text_color(p.muted).child(format!(
                "{}{}",
                self.category_name(timer.category),
                if timer.note.is_empty() {
                    String::new()
                } else {
                    format!("  ·  {}", timer.note)
                }
            ))
        } else {
            column()
        };
        let hero = panel(p)
            .p_6()
            .gap_4()
            .child(
                row()
                    .justify_between()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(timer_color)
                            .child(timer_label),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(p.muted)
                            .child(format!("Цель {} мин", target / 60)),
                    ),
            )
            .child(
                div()
                    .text_size(px(68.0))
                    .line_height(px(75.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(value),
            )
            .child(
                div().h(px(4.0)).rounded_full().bg(p.hover).child(
                    div()
                        .h_full()
                        .rounded_full()
                        .bg(timer_color)
                        .w(relative((elapsed as f32 / target as f32).clamp(0.0, 1.0))),
                ),
            )
            .child(work_details)
            .when_some(self.reminder, |element, message| {
                element.child(div().text_sm().text_color(p.rest).child(message))
            })
            .child(controls)
            .when(self.confirm, |element| {
                element.child(confirm("Сбросить текущую сессию?", Action::Discard, p, cx))
            });
        let date = Local::now().date_naive();
        column()
            .gap_5()
            .child(hero)
            .child(self.timeline(date, p, cx))
            .child(
                column()
                    .gap_2()
                    .child(
                        row()
                            .justify_between()
                            .child(heading("Последние сессии"))
                            .child(button(
                                "all-history",
                                "Вся история",
                                Button::Plain,
                                Action::Tab(Tab::History),
                                p,
                                cx,
                            )),
                    )
                    .children(
                        self.ordered_periods()
                            .into_iter()
                            .filter(|(_, period)| {
                                period.start.with_timezone(&Local).date_naive() == date
                            })
                            .take(4)
                            .map(|(id, period)| self.period_row(id, period, p, cx)),
                    ),
            )
    }

    fn timeline(&self, date: NaiveDate, p: Palette, cx: &mut Context<Self>) -> Div {
        let day_range = (day_start(date), day_start(date + Days::new(1)));
        let current = self
            .state
            .timer
            .as_ref()
            .map(|timer| timer.preview(Utc::now()));
        let sessions = self
            .state
            .dataset
            .periods
            .iter()
            .filter(|(id, _)| {
                self.state
                    .timer
                    .as_ref()
                    .is_none_or(|timer| timer.id != **id)
            })
            .map(|(_, period)| period)
            .chain(current.iter())
            .filter(|period| period.end > day_range.0 && period.start < day_range.1);
        let day = date.weekday().num_days_from_sunday() as u8;
        let zones: Vec<_> = self
            .state
            .dataset
            .zones
            .values()
            .filter_map(|zone| {
                if !zone.weekdays.contains(&day) || zone.exclude_from_breakdown {
                    return None;
                }
                Some((zone, at(date, zone.start)?, at(date, zone.end)?))
            })
            .collect();
        let range = if self.full_day {
            day_range
        } else {
            let default_start =
                at(date, NaiveTime::from_hms_opt(8, 0, 0).unwrap()).unwrap_or(day_range.0);
            let default_end =
                at(date, NaiveTime::from_hms_opt(20, 0, 0).unwrap()).unwrap_or(day_range.1);
            let (mut start, mut end) = sessions
                .clone()
                .map(|period| (period.start, period.end))
                .reduce(|left, right| (left.0.min(right.0), left.1.max(right.1)))
                .unwrap_or((default_start, default_end));
            for (zone, from, to) in &zones {
                if zone.full_range {
                    start = start.min(*from);
                    end = end.max(*to);
                }
            }
            let padding = chrono::Duration::minutes(30);
            (
                (start - padding).max(day_range.0),
                (end + padding).min(day_range.1),
            )
        };
        let width = (range.1 - range.0).num_milliseconds().max(1) as f32;
        let fraction = |time: DateTime<Utc>| (time - range.0).num_milliseconds() as f32 / width;
        let bar = |start: DateTime<Utc>, end: DateTime<Utc>, color: Rgba, style: ZoneStyle| {
            let from = fraction(start.max(range.0));
            let to = fraction(end.min(range.1));
            (to > from).then(|| {
                div()
                    .absolute()
                    .top(px(0.0))
                    .bottom(px(0.0))
                    .left(relative(from))
                    .w(relative(to - from))
                    .rounded(px(3.0))
                    .overflow_hidden()
                    .when(style != ZoneStyle::Solid, |element| {
                        element.border_1().border_color(color)
                    })
                    .when(style == ZoneStyle::Solid, |element| element.bg(color))
                    .when(style == ZoneStyle::Hatched, |element| {
                        element.flex().gap_1().children(
                            (0..100).map(|_| div().w(px(1.0)).h_full().flex_none().bg(color)),
                        )
                    })
            })
        };
        let bars = sessions.flat_map(|period| {
            let color = if period.kind == Kind::Break {
                p.rest
            } else {
                self.category_color(period.category, p.accent)
            };
            period
                .segments()
                .filter_map(move |(start, end)| bar(start, end, color, ZoneStyle::Solid))
        });
        let lanes = zones.into_iter().filter_map(|(zone, start, end)| {
            let block = bar(
                start,
                end,
                hex(zone.color.as_deref()).unwrap_or(p.accent),
                zone.style,
            )?;
            Some(timeline_lane(&zone.name, [block], p))
        });
        let format_time = |moment: DateTime<Utc>| {
            moment
                .with_timezone(&Local)
                .format(time_format(&self.state.dataset.settings))
                .to_string()
        };
        let end_label = if self.full_day && self.state.dataset.settings.hour_format != "12h" {
            "24:00".to_owned()
        } else {
            format_time(range.1)
        };
        panel(p)
            .gap_3()
            .child(
                row()
                    .justify_between()
                    .child(heading("Таймлайн дня"))
                    .child(button(
                        "timeline-range",
                        if self.full_day {
                            "Вокруг сессий"
                        } else {
                            "Весь день"
                        },
                        Button::Plain,
                        Action::TimelineRange,
                        p,
                        cx,
                    )),
            )
            .child(
                div()
                    .ml(px(120.0))
                    .flex()
                    .justify_between()
                    .text_xs()
                    .text_color(p.muted)
                    .child(format_time(range.0))
                    .child(format_time(range.0 + (range.1 - range.0) / 2))
                    .child(end_label),
            )
            .child(timeline_lane("Сессии", bars, p))
            .children(lanes)
    }

    fn ordered_periods(&self) -> Vec<(Uuid, &Period)> {
        let mut periods: Vec<_> = self
            .state
            .dataset
            .periods
            .iter()
            .map(|(id, period)| (*id, period))
            .collect();
        periods.sort_by_key(|(id, period)| std::cmp::Reverse((period.start, *id)));
        periods
    }

    fn period_row(&self, id: Uuid, period: &Period, p: Palette, cx: &mut Context<Self>) -> Div {
        let settings = &self.state.dataset.settings;
        let start = period.start.with_timezone(&Local);
        let end = period.end.with_timezone(&Local);
        let color = if period.kind == Kind::Break {
            p.rest
        } else {
            self.category_color(period.category, p.accent)
        };
        row()
            .p_3()
            .rounded_lg()
            .bg(p.tile)
            .border_1()
            .border_color(p.border)
            .child(div().size(px(8.0)).flex_none().rounded_full().bg(color))
            .child(
                column()
                    .min_w_0()
                    .flex_1()
                    .gap_1()
                    .child(div().truncate().font_weight(FontWeight::MEDIUM).child(
                        if period.note.is_empty() {
                            kind_name(period.kind).to_owned()
                        } else {
                            period.note.clone()
                        },
                    ))
                    .child(div().text_xs().text_color(p.muted).child(format!(
                        "{} · {}–{}  ·  {}",
                        start.format(date_format(settings)),
                        start.format(time_format(settings)),
                        end.format(time_format(settings)),
                        self.category_name(period.category),
                    ))),
            )
            .child(
                div()
                    .flex_none()
                    .text_sm()
                    .text_color(color)
                    .child(duration(period.duration_millis())),
            )
            .child(button(
                format!("period-{id}"),
                "Изменить",
                Button::Plain,
                Action::Period(Some(id)),
                p,
                cx,
            ))
    }

    fn history(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let query = read(&self.search, cx).trim().to_lowercase();
        let matches: Vec<_> = self
            .ordered_periods()
            .into_iter()
            .filter(|(_, period)| {
                self.history_kind.is_none_or(|kind| period.kind == kind)
                    && (query.is_empty()
                        || period.note.to_lowercase().contains(&query)
                        || self
                            .category_name(period.category)
                            .to_lowercase()
                            .contains(&query))
            })
            .collect();
        let pages = matches.len().div_ceil(PAGE_SIZE).max(1);
        let page = self.history_page.min(pages - 1);
        column()
            .gap_3()
            .child(
                row()
                    .justify_between()
                    .child(
                        row().children(
                            [None, Some(Kind::Work), Some(Kind::Break)]
                                .into_iter()
                                .map(|kind| {
                                    let label = kind.map_or("Все сессии", kind_name);
                                    button(
                                        label,
                                        label,
                                        selection(self.history_kind == kind),
                                        Action::HistoryFilter(kind),
                                        p,
                                        cx,
                                    )
                                }),
                        ),
                    )
                    .child(button(
                        "add-period",
                        "Добавить сессию",
                        Button::Primary,
                        Action::Period(None),
                        p,
                        cx,
                    )),
            )
            .child(Input::new(&self.search))
            .children(
                matches
                    .iter()
                    .skip(page * PAGE_SIZE)
                    .take(PAGE_SIZE)
                    .map(|(id, period)| self.period_row(*id, period, p, cx)),
            )
            .when(matches.is_empty(), |element| {
                element.child(panel(p).text_color(p.muted).child("Сессий не найдено."))
            })
            .when(pages > 1, |element| {
                element.child(
                    row()
                        .when(page > 0, |element| {
                            element.child(button(
                                "prev",
                                "Назад",
                                Button::Plain,
                                Action::HistoryPage(page - 1),
                                p,
                                cx,
                            ))
                        })
                        .child(format!("{} из {pages}", page + 1))
                        .when(page + 1 < pages, |element| {
                            element.child(button(
                                "next",
                                "Дальше",
                                Button::Plain,
                                Action::HistoryPage(page + 1),
                                p,
                                cx,
                            ))
                        }),
                )
            })
    }

    fn stats(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let date = Local::now().date_naive();
        let settings = &self.state.dataset.settings;
        let week_start = match settings.first_day_of_week.as_str() {
            "sunday" => 0,
            "saturday" => 6,
            _ => 1,
        };
        let weekday = date.weekday().num_days_from_sunday();
        let (start, end) = match self.stats_span {
            Span::Day => (date, date + Days::new(1)),
            Span::Week => {
                let start = date - Days::new(u64::from((weekday + 7 - week_start) % 7));
                (start, start + Days::new(7))
            }
            Span::Month => {
                let start = date.with_day(1).unwrap_or(date);
                let end = start
                    .checked_add_months(chrono::Months::new(1))
                    .unwrap_or(start);
                (start, end)
            }
        };
        let range = (day_start(start), day_start(end));
        let periods = &self.state.dataset.periods;
        let summary = summarize(periods.values(), range);
        let mut categories: Vec<_> = summary.by_category.into_iter().collect();
        categories.sort_by_key(|(id, millis)| std::cmp::Reverse((*millis, *id)));

        let buckets = stats_buckets(start, end, self.stats_span);
        let samples: Vec<_> = buckets
            .iter()
            .map(|(label, range)| (label, summarize(periods.values(), *range).work_millis))
            .collect();
        let peak = samples
            .iter()
            .map(|(_, value)| *value)
            .max()
            .unwrap_or_default()
            .max(1);

        let mut aligned = 0;
        let mut mismatched = 0;
        for day in dates(start, end) {
            let weekday = day.weekday().num_days_from_sunday() as u8;
            for zone in self.state.dataset.zones.values().filter(|zone| {
                zone.parent.is_none()
                    && !zone.exclude_from_breakdown
                    && zone.weekdays.contains(&weekday)
            }) {
                if let (Some(category), Some(start), Some(end)) =
                    (zone.category, at(day, zone.start), at(day, zone.end))
                {
                    for period in periods
                        .values()
                        .filter(|period| period.kind == Kind::Work && period.category.is_some())
                    {
                        if period.category == Some(category) {
                            aligned += period.active_millis((start, end));
                        } else {
                            mismatched += period.active_millis((start, end));
                        }
                    }
                }
            }
        }

        column()
            .gap_5()
            .child(
                row().children(
                    [
                        (Span::Day, "День"),
                        (Span::Week, "Неделя"),
                        (Span::Month, "Месяц"),
                    ]
                    .map(|(span, label)| {
                        button(
                            label,
                            label,
                            selection(self.stats_span == span),
                            Action::Span(span),
                            p,
                            cx,
                        )
                    }),
                ),
            )
            .child(
                row()
                    .child(metric("Работа", duration(summary.work_millis), p.accent, p))
                    .child(metric(
                        "Перерывы",
                        duration(summary.break_millis),
                        p.rest,
                        p,
                    ))
                    .child(metric(
                        "Сессии",
                        (summary.work_count + summary.break_count).to_string(),
                        p.fg,
                        p,
                    )),
            )
            .child(
                panel(p).gap_4().child(heading("Рабочее время")).child(
                    div()
                        .h(px(134.0))
                        .flex()
                        .gap_1()
                        .children(samples.into_iter().map(|(label, value)| {
                            column()
                                .min_w_0()
                                .flex_1()
                                .h_full()
                                .items_center()
                                .justify_end()
                                .gap_2()
                                .child(
                                    div()
                                        .w_full()
                                        .max_w(px(44.0))
                                        .rounded_t_md()
                                        .h(px(if value == 0 {
                                            2.0
                                        } else {
                                            (value as f32 / peak as f32 * 108.0).max(3.0)
                                        }))
                                        .bg(if value == 0 { p.border } else { p.accent }),
                                )
                                .child(div().text_xs().text_color(p.muted).child(label.clone()))
                        })),
                ),
            )
            .child(
                panel(p)
                    .gap_3()
                    .child(heading("По категориям"))
                    .when(categories.is_empty(), |element| {
                        element.child(
                            div()
                                .text_color(p.muted)
                                .child("За этот период нет рабочей активности."),
                        )
                    })
                    .children(categories.into_iter().map(|(id, millis)| {
                        row()
                            .child(
                                div()
                                    .size(px(8.0))
                                    .rounded_full()
                                    .bg(self.category_color(id, p.accent)),
                            )
                            .child(div().flex_1().child(self.category_name(id)))
                            .child(div().text_color(p.muted).child(duration(millis)))
                    })),
            )
            .when(aligned + mismatched > 0, |element| {
                element.child(
                    panel(p)
                        .gap_2()
                        .child(heading("Совпадение с планом"))
                        .child(format!(
                            "{}% · {} из {}",
                            (aligned as f64 / (aligned + mismatched) as f64 * 100.0).round(),
                            duration(aligned),
                            duration(aligned + mismatched)
                        )),
                )
            })
    }

    fn categories(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        column()
            .gap_3()
            .child(row().justify_end().child(button(
                "add-category",
                "Добавить категорию",
                Button::Primary,
                Action::Category(None),
                p,
                cx,
            )))
            .when(self.state.dataset.categories.is_empty(), |element| {
                element.child(panel(p).text_color(p.muted).child("Категорий пока нет."))
            })
            .children(self.state.dataset.categories.iter().map(|(id, category)| {
                row()
                    .p_3()
                    .rounded_md()
                    .bg(p.tile)
                    .border_1()
                    .border_color(p.border)
                    .child(
                        div()
                            .size(px(9.0))
                            .rounded_full()
                            .bg(hex(Some(&category.color)).unwrap_or(p.accent)),
                    )
                    .child(div().flex_1().child(category.name.clone()))
                    .when(category.archived, |element| {
                        element.child(div().text_sm().text_color(p.muted).child("В архиве"))
                    })
                    .child(button(
                        format!("category-{id}"),
                        "Изменить",
                        Button::Plain,
                        Action::Category(Some(*id)),
                        p,
                        cx,
                    ))
            }))
    }

    fn zones(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let zones = &self.state.dataset.zones;
        let mut ordered: Vec<_> = zones.iter().collect();
        ordered.sort_by_key(|(id, zone)| {
            (
                zone.parent.unwrap_or(**id),
                zone.parent.is_some(),
                zone.start,
            )
        });
        column()
            .gap_3()
            .child(row().justify_end().child(button(
                "add-zone",
                "Добавить зону",
                Button::Primary,
                Action::Zone(None),
                p,
                cx,
            )))
            .when(zones.is_empty(), |element| {
                element.child(
                    panel(p)
                        .text_color(p.muted)
                        .child("Добавьте часы работы или перерывов, чтобы видеть их на таймлайне."),
                )
            })
            .children(ordered.into_iter().map(|(id, zone)| {
                let days = zone
                    .weekdays
                    .iter()
                    .filter_map(|day| DAYS.get(usize::from(*day)).copied())
                    .collect::<Vec<_>>()
                    .join(", ");
                row()
                    .p_3()
                    .rounded_md()
                    .bg(p.tile)
                    .border_1()
                    .border_color(p.border)
                    .when(zone.parent.is_some(), |element| element.ml_6())
                    .child(
                        div()
                            .size(px(9.0))
                            .rounded_full()
                            .bg(hex(zone.color.as_deref()).unwrap_or(p.accent)),
                    )
                    .child(column().flex_1().child(zone.name.clone()).child(
                        div().text_sm().text_color(p.muted).child(format!(
                            "{}–{}  ·  {days}",
                            zone.start.format("%H:%M"),
                            zone.end.format("%H:%M")
                        )),
                    ))
                    .child(button(
                        format!("zone-{id}"),
                        "Изменить",
                        Button::Plain,
                        Action::Zone(Some(*id)),
                        p,
                        cx,
                    ))
            }))
    }

    fn editor(&self, form: &Form, p: Palette, cx: &mut Context<Self>) -> Div {
        let (title, content, remove_label) = match form {
            Form::Period(form) => (
                if form.period.is_some() {
                    "Изменить сессию"
                } else {
                    "Добавить сессию"
                },
                column()
                    .gap_3()
                    .child(
                        row()
                            .child(label("Начало", Input::new(&form.start), p))
                            .child(label("Конец", Input::new(&form.end), p)),
                    )
                    .child(label("Заметка", Input::new(&form.note), p))
                    .child(row().child(button(
                        "form-kind",
                        kind_name(form.kind),
                        Button::Select,
                        Action::ChooseKind,
                        p,
                        cx,
                    )))
                    .when(form.kind == Kind::Work, |element| {
                        element.child(self.category_picker(form.category, false, p, cx))
                    })
                    .when(
                        form.period
                            .as_ref()
                            .is_some_and(|period| !period.pauses.is_empty()),
                        |element| {
                            element.child(button(
                                "clear-pauses",
                                "Очистить паузы",
                                selection(form.clear_pauses),
                                Action::ClearPauses,
                                p,
                                cx,
                            ))
                        },
                    ),
                form.period.as_ref().map(|_| "Удалить"),
            ),
            Form::Category(form) => (
                "Категория",
                column()
                    .gap_3()
                    .child(label("Название", Input::new(&form.name), p))
                    .child(label("Цвет · #RRGGBB", Input::new(&form.color), p)),
                self.state
                    .dataset
                    .categories
                    .contains_key(&form.id)
                    .then_some(if form.archived {
                        "Вернуть из архива"
                    } else {
                        "В архив"
                    }),
            ),
            Form::Zone(form) => (
                "Зона дня",
                column()
                    .gap_3()
                    .child(label("Название", Input::new(&form.name), p))
                    .child(
                        row()
                            .child(label("Начало", Input::new(&form.start), p))
                            .child(label("Конец", Input::new(&form.end), p))
                            .child(label("Цвет · #RRGGBB", Input::new(&form.color), p)),
                    )
                    .child(row().children([1, 2, 3, 4, 5, 6, 0].map(|day| {
                        button(
                            format!("day-{day}"),
                            DAYS[usize::from(day)],
                            selection(form.weekdays.contains(&day)),
                            Action::ZoneDay(day),
                            p,
                            cx,
                        )
                    })))
                    .child(
                        div()
                            .text_sm()
                            .text_color(p.muted)
                            .child("Родительская зона"),
                    )
                    .child(
                        row()
                            .flex_wrap()
                            .child(button(
                                "parent-none",
                                "Без родителя",
                                selection(form.parent.is_none()),
                                Action::ZoneParent(None),
                                p,
                                cx,
                            ))
                            .children(
                                self.state
                                    .dataset
                                    .zones
                                    .iter()
                                    .filter(|(id, zone)| **id != form.id && zone.parent.is_none())
                                    .map(|(id, zone)| {
                                        button(
                                            format!("parent-{id}"),
                                            zone.name.clone(),
                                            selection(form.parent == Some(*id)),
                                            Action::ZoneParent(Some(*id)),
                                            p,
                                            cx,
                                        )
                                    }),
                            ),
                    )
                    .child(
                        row().flex_wrap().children(
                            [
                                (ZoneStyle::Solid, "Заливка"),
                                (ZoneStyle::Border, "Обводка"),
                                (ZoneStyle::Hatched, "Штриховка"),
                            ]
                            .map(|(style, label)| {
                                button(
                                    label,
                                    label,
                                    selection(form.style == style),
                                    Action::ZoneStyle(style),
                                    p,
                                    cx,
                                )
                            }),
                        ),
                    )
                    .when(form.parent.is_none(), |element| {
                        element
                            .child(label(
                                "Ожидаемая категория",
                                self.category_picker(form.category, false, p, cx),
                                p,
                            ))
                            .child(
                                row()
                                    .flex_wrap()
                                    .child(button(
                                        "exclude-zone",
                                        "Скрыть из сводки",
                                        selection(form.exclude),
                                        Action::ZoneExclude,
                                        p,
                                        cx,
                                    ))
                                    .child(button(
                                        "full-zone",
                                        "Показывать зону целиком",
                                        selection(form.full_range),
                                        Action::ZoneFullRange,
                                        p,
                                        cx,
                                    )),
                            )
                    }),
                self.state
                    .dataset
                    .zones
                    .contains_key(&form.id)
                    .then_some("Удалить"),
            ),
        };
        panel(p)
            .gap_4()
            .child(heading(title))
            .child(content)
            .child(
                row()
                    .child(button(
                        "save",
                        "Сохранить",
                        Button::Primary,
                        Action::Save,
                        p,
                        cx,
                    ))
                    .child(button(
                        "cancel",
                        "Отмена",
                        Button::Plain,
                        Action::Cancel,
                        p,
                        cx,
                    ))
                    .when_some(remove_label, |element, label| {
                        let action = if matches!(form, Form::Category(_)) {
                            Action::Delete
                        } else {
                            Action::ConfirmDelete
                        };
                        element.child(button("remove-edit", label, Button::Danger, action, p, cx))
                    }),
            )
            .when(self.confirm, |element| {
                element.child(confirm(
                    if matches!(form, Form::Zone(_)) {
                        "Удалить зону вместе с вложенными зонами?"
                    } else {
                        "Удалить сессию?"
                    },
                    Action::Delete,
                    p,
                    cx,
                ))
            })
    }

    fn settings(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let form = &self.settings_form;
        let settings = &form.draft;
        column()
            .gap_5()
            .child(
                panel(p)
                    .gap_4()
                    .child(heading("Таймер"))
                    .child(
                        row()
                            .child(label("Работа, мин", Input::new(&form.work), p))
                            .child(label("Перерыв, мин", Input::new(&form.rest), p))
                            .child(label("Напоминание, мин", Input::new(&form.reminder), p))
                            .child(label("Пауза от, сек", Input::new(&form.pause), p)),
                    )
                    .child(
                        row()
                            .flex_wrap()
                            .child(button(
                                "auto-break",
                                "Автостарт перерыва",
                                selection(settings.auto_start_break),
                                Action::SettingOption(SettingOption::AutomaticBreak),
                                p,
                                cx,
                            ))
                            .child(button(
                                "auto-work",
                                "Автостарт работы",
                                selection(settings.auto_start_pomodoro),
                                Action::SettingOption(SettingOption::AutomaticWork),
                                p,
                                cx,
                            )),
                    ),
            )
            .child(
                panel(p)
                    .gap_4()
                    .child(heading("Внешний вид и регион"))
                    .child(setting_options(
                        "Тема",
                        &[
                            ("system", "Системная"),
                            ("dark", "Тёмная"),
                            ("light", "Светлая"),
                        ],
                        &settings.theme,
                        SettingOption::Theme,
                        p,
                        cx,
                    ))
                    .child(setting_options(
                        "Первый день недели",
                        &[
                            ("monday", "Понедельник"),
                            ("sunday", "Воскресенье"),
                            ("saturday", "Суббота"),
                        ],
                        &settings.first_day_of_week,
                        SettingOption::Week,
                        p,
                        cx,
                    ))
                    .child(setting_options(
                        "Дата",
                        &[
                            ("DD.MM.YYYY", "ДД.ММ.ГГГГ"),
                            ("YYYY-MM-DD", "ГГГГ-ММ-ДД"),
                            ("MM/DD/YYYY", "ММ/ДД/ГГГГ"),
                        ],
                        &settings.date_format,
                        SettingOption::Date,
                        p,
                        cx,
                    ))
                    .child(setting_options(
                        "Время",
                        &[("24h", "24 часа"), ("12h", "12 часов")],
                        &settings.hour_format,
                        SettingOption::Hour,
                        p,
                        cx,
                    ))
                    .child(row().child(button(
                        "save-settings",
                        "Сохранить настройки",
                        Button::Primary,
                        Action::SaveSettings,
                        p,
                        cx,
                    ))),
            )
            .child(
                panel(p).gap_3().child(heading("Импорт и экспорт")).child(
                    row()
                        .child(button(
                            "import",
                            "Импортировать JSON",
                            Button::Plain,
                            Action::Import,
                            p,
                            cx,
                        ))
                        .child(button(
                            "export",
                            "Сохранить архив",
                            Button::Plain,
                            Action::Export,
                            p,
                            cx,
                        )),
                ),
            )
            .child(self.cloud_view(p, cx))
    }

    fn cloud_view(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let content = if let Some(error) = &self.cloud_error {
            column().text_color(p.alert).child(error.clone())
        } else if self.cloud.is_none() {
            column().text_sm().text_color(p.muted).child(
                "Задайте TRACKING_SUPABASE_URL и TRACKING_SUPABASE_KEY в .env для подключения.",
            )
        } else if self.cloud_busy() {
            column()
                .text_sm()
                .text_color(p.muted)
                .child(self.cloud_label.clone())
        } else if self.session.is_some() {
            column()
                .gap_3()
                .child(
                    div()
                        .text_sm()
                        .text_color(if self.conflict { p.alert } else { p.muted })
                        .child(self.cloud_label.clone()),
                )
                .child(
                    row()
                        .flex_wrap()
                        .when(self.conflict, |element| {
                            element
                                .child(button(
                                    "local-conflict",
                                    "Локальные версии",
                                    Button::Plain,
                                    Action::Sync(Preference::Local),
                                    p,
                                    cx,
                                ))
                                .child(button(
                                    "remote-conflict",
                                    "Облачные версии",
                                    Button::Plain,
                                    Action::Sync(Preference::Remote),
                                    p,
                                    cx,
                                ))
                        })
                        .child(button(
                            "sync",
                            "Синхронизировать",
                            Button::Primary,
                            Action::Sync(Preference::Ask),
                            p,
                            cx,
                        ))
                        .child(button(
                            "sign-out",
                            "Выйти",
                            Button::Plain,
                            Action::SignOut,
                            p,
                            cx,
                        )),
                )
        } else {
            column()
                .gap_3()
                .child(
                    div()
                        .text_sm()
                        .text_color(p.muted)
                        .child(self.cloud_label.clone()),
                )
                .child(
                    row()
                        .child(label("Email", Input::new(&self.email), p))
                        .child(button(
                            "request",
                            "Отправить код",
                            Button::Plain,
                            Action::RequestCode,
                            p,
                            cx,
                        )),
                )
                .child(
                    row()
                        .child(label("Одноразовый код", Input::new(&self.otp), p))
                        .child(button(
                            "verify",
                            "Войти",
                            Button::Primary,
                            Action::VerifyCode,
                            p,
                            cx,
                        )),
                )
        };
        panel(p).gap_3().child(heading("Supabase")).child(content)
    }

    fn category_picker(
        &self,
        selected: Option<Uuid>,
        timer: bool,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let select = |id| {
            if timer {
                Action::TimerCategory(id)
            } else {
                Action::ChooseCategory(id)
            }
        };
        row()
            .flex_wrap()
            .child(button(
                "category-none",
                "Без категории",
                selection(selected.is_none()),
                select(None),
                p,
                cx,
            ))
            .children(
                self.state
                    .dataset
                    .categories
                    .iter()
                    .filter(|(id, category)| !category.archived || Some(**id) == selected)
                    .map(|(id, category)| {
                        button(
                            format!("choose-{id}"),
                            category.name.clone(),
                            selection(selected == Some(*id)),
                            select(Some(*id)),
                            p,
                            cx,
                        )
                    }),
            )
    }

    fn category_name(&self, id: Option<Uuid>) -> String {
        id.and_then(|id| self.state.dataset.categories.get(&id))
            .map_or_else(|| "Без категории".into(), |category| category.name.clone())
    }

    fn category_color(&self, id: Option<Uuid>, fallback: Rgba) -> Rgba {
        hex(id
            .and_then(|id| self.state.dataset.categories.get(&id))
            .map(|category| category.color.as_str()))
        .unwrap_or(fallback)
    }
}

fn setting_options(
    label_text: &'static str,
    values: &[(&'static str, &'static str)],
    current: &str,
    change: impl Fn(&'static str) -> SettingOption,
    p: Palette,
    cx: &mut Context<Tracker>,
) -> Div {
    label(
        label_text,
        row()
            .flex_wrap()
            .children(values.iter().map(|(value, title)| {
                button(
                    *value,
                    *title,
                    selection(current == *value),
                    Action::SettingOption(change(value)),
                    p,
                    cx,
                )
            })),
        p,
    )
}

fn confirm(message: &'static str, action: Action, p: Palette, cx: &mut Context<Tracker>) -> Div {
    row()
        .flex_wrap()
        .child(div().text_color(p.alert).child(message))
        .child(button(
            "confirm",
            "Подтвердить",
            Button::Danger,
            action,
            p,
            cx,
        ))
        .child(button(
            "cancel-confirm",
            "Отмена",
            Button::Plain,
            Action::Cancel,
            p,
            cx,
        ))
}

fn timeline_lane(label: &str, bars: impl IntoIterator<Item = Div>, p: Palette) -> Div {
    row()
        .gap_0()
        .child(
            div()
                .w(px(120.0))
                .flex_none()
                .pr_3()
                .text_xs()
                .text_color(p.muted)
                .truncate()
                .child(label.to_owned()),
        )
        .child(
            div()
                .relative()
                .flex_1()
                .h(px(12.0))
                .rounded_sm()
                .bg(p.hover)
                .children(bars),
        )
}

fn button(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    style: Button,
    action: Action,
    p: Palette,
    cx: &mut Context<Tracker>,
) -> AnyElement {
    let (background, foreground) = match style {
        Button::Plain => (p.tile, p.fg),
        Button::Select => (p.selected, p.accent),
        Button::Primary => (p.accent, p.accent_text),
        Button::Danger => (p.tile, p.alert),
    };
    let hover = if matches!(style, Button::Primary) {
        Rgba {
            a: 0.86,
            ..p.accent
        }
    } else {
        p.hover
    };
    div()
        .id(id.into())
        .flex_none()
        .rounded_md()
        .px_3()
        .py_2()
        .cursor_pointer()
        .border_1()
        .border_color(if matches!(style, Button::Primary) {
            p.accent
        } else {
            p.border
        })
        .bg(background)
        .text_color(foreground)
        .text_sm()
        .font_weight(FontWeight::MEDIUM)
        .hover(move |element| element.bg(hover))
        .on_click(cx.listener(move |this, _, window, cx| this.act(action.clone(), window, cx)))
        .child(label.into())
        .into_any_element()
}

fn selection(selected: bool) -> Button {
    if selected {
        Button::Select
    } else {
        Button::Plain
    }
}
fn column() -> Div {
    div().flex().flex_col()
}
fn row() -> Div {
    div().flex().items_center().gap_2()
}
fn panel(p: Palette) -> Div {
    column()
        .p_4()
        .rounded_lg()
        .border_1()
        .border_color(p.border)
        .bg(p.tile)
}
fn heading(value: &'static str) -> Div {
    div()
        .text_lg()
        .font_weight(FontWeight::SEMIBOLD)
        .child(value)
}

fn label(text: &'static str, body: impl IntoElement, p: Palette) -> Div {
    column()
        .flex_1()
        .min_w_0()
        .gap_1()
        .child(div().text_sm().text_color(p.muted).child(text))
        .child(body)
}

fn metric(label_text: &'static str, value: String, color: Rgba, p: Palette) -> Div {
    panel(p)
        .flex_1()
        .gap_2()
        .child(div().text_sm().text_color(p.muted).child(label_text))
        .child(
            div()
                .text_2xl()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(color)
                .child(value),
        )
}

fn kind_name(kind: Kind) -> &'static str {
    if kind == Kind::Work {
        "Работа"
    } else {
        "Перерыв"
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

fn duration(millis: i64) -> String {
    let seconds = millis / 1000;
    if seconds >= 3600 {
        format!("{} ч {:02} мин", seconds / 3600, seconds % 3600 / 60)
    } else if seconds >= 60 {
        format!("{} мин", seconds / 60)
    } else {
        format!("{seconds} с")
    }
}

fn date_format(settings: &Settings) -> &'static str {
    match settings.date_format.as_str() {
        "YYYY-MM-DD" => "%Y-%m-%d",
        "MM/DD/YYYY" => "%m/%d/%Y",
        _ => "%d.%m.%Y",
    }
}

fn time_format(settings: &Settings) -> &'static str {
    if settings.hour_format == "12h" {
        "%I:%M %p"
    } else {
        "%H:%M"
    }
}

fn long_date(date: NaiveDate) -> String {
    format!(
        "{} {}, {}",
        date.day(),
        MONTHS[date.month0() as usize],
        DAYS[date.weekday().num_days_from_sunday() as usize]
    )
}

fn hex(color: Option<&str>) -> Option<Rgba> {
    let value = color?.strip_prefix('#')?;
    (value.len() == 6)
        .then(|| u32::from_str_radix(value, 16).ok().map(rgb))
        .flatten()
}

fn dates(start: NaiveDate, end: NaiveDate) -> impl Iterator<Item = NaiveDate> {
    std::iter::successors(Some(start), |day| day.succ_opt()).take_while(move |day| *day < end)
}

fn at(date: NaiveDate, time: NaiveTime) -> Option<DateTime<Utc>> {
    match Local.from_local_datetime(&date.and_time(time)) {
        LocalResult::Single(value) | LocalResult::Ambiguous(value, _) => {
            Some(value.with_timezone(&Utc))
        }
        LocalResult::None => None,
    }
}

fn day_start(date: NaiveDate) -> DateTime<Utc> {
    (0..1440)
        .find_map(|minute| {
            NaiveTime::from_hms_opt(minute / 60, minute % 60, 0).and_then(|time| at(date, time))
        })
        .unwrap_or_else(|| date.and_time(NaiveTime::MIN).and_utc())
}

fn stats_buckets(start: NaiveDate, end: NaiveDate, span: Span) -> Vec<(String, Moments)> {
    if span == Span::Day {
        let (begin, finish) = (day_start(start), day_start(end));
        std::iter::successors(Some(begin), |value| {
            Some(*value + chrono::Duration::hours(1))
        })
        .take_while(|value| *value < finish)
        .map(|value| {
            let hour = value.with_timezone(&Local).hour();
            let label = if hour % 6 == 0 {
                format!("{hour:02}")
            } else {
                String::new()
            };
            (
                label,
                (value, (value + chrono::Duration::hours(1)).min(finish)),
            )
        })
        .collect()
    } else {
        dates(start, end)
            .map(|date| {
                let label = if span == Span::Week {
                    DAYS[date.weekday().num_days_from_sunday() as usize].to_owned()
                } else if date.day() % 5 == 0 {
                    date.day().to_string()
                } else {
                    String::new()
                };
                (label, (day_start(date), day_start(date + Days::new(1))))
            })
            .collect()
    }
}

use chrono::Timelike;
