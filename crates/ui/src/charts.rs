use std::collections::BTreeMap;

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, Utc};
use egui::{
    Align, Align2, Color32, FontId, Layout, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2,
};
use tracking_core::{
    Dataset, Kind, Moments, ZoneStyle,
    reports::{
        Alignment, Breakdown, Report, Span, ZoneSlice, day_range, hourly_alignment, intersect,
        report, zone_breakdown, zone_slices,
    },
};
use uuid::Uuid;

use crate::{
    Tracker, category_name,
    components::{card, segmented},
    date_format, default_color, duration,
    editors::weekday_label,
    kind_name,
    pages::span_label,
    theme::{CONTROL_HEIGHT, accent, parse_color},
    time_format,
};

#[derive(Default)]
pub(crate) struct DailyCache {
    key: Option<(u64, NaiveDate)>,
    full_day: bool,
    slices: Vec<ZoneSlice>,
    periods: Vec<Uuid>,
    breakdown: BTreeMap<Option<Uuid>, Breakdown>,
    alignment: Option<[Alignment; 24]>,
}

pub(crate) struct ReportCache {
    key: Option<(u64, NaiveDate, Span, Option<Kind>)>,
    span: Span,
    kind: Option<Kind>,
    value: Option<Report>,
}

impl Default for ReportCache {
    fn default() -> Self {
        Self {
            key: None,
            span: Span::Day,
            kind: Some(Kind::Work),
            value: None,
        }
    }
}

impl DailyCache {
    fn refresh(&mut self, dataset: &Dataset, date: NaiveDate, revision: u64) {
        if self.key == Some((revision, date)) {
            return;
        }
        self.key = Some((revision, date));
        self.slices = zone_slices(dataset, date, &Local);
        self.breakdown = zone_breakdown(dataset, date, &Local);
        let day = day_range(date, &Local);
        self.periods = dataset
            .periods
            .iter()
            .filter_map(|(&id, period)| intersect((period.start, period.end), day).map(|_| id))
            .collect();
        let tracked = dataset.zones.values().any(|zone| {
            zone.parent.is_none() && !zone.exclude_from_breakdown && zone.category.is_some()
        });
        self.periods
            .sort_unstable_by_key(|id| dataset.periods[id].start);
        self.alignment = tracked.then(|| hourly_alignment(dataset, date, &Local));
    }
}

impl Tracker {
    pub(crate) fn timeline(&mut self, ui: &mut Ui, now: DateTime<Utc>) {
        let date = now.with_timezone(&Local).date_naive();
        let dataset = &self.state.dataset;
        self.daily.refresh(dataset, date, self.revision);
        card(ui, |ui| {
            let header_layout = if ui.available_width() >= 400.0 {
                Layout::left_to_right(Align::Center)
            } else {
                Layout::top_down(Align::Min)
            };
            ui.allocate_ui_with_layout(
                vec2(ui.available_width(), CONTROL_HEIGHT),
                header_layout,
                |ui| {
                    ui.heading("Таймлайн дня");
                    segmented(
                        ui,
                        &mut self.daily.full_day,
                        [(true, "Весь день"), (false, "От первой сессии")],
                    );
                },
            );
            let live = self
                .state
                .timer
                .as_ref()
                .map(|timer| (timer.id, timer.preview(now)));
            let (day_start, day_end) = day_range(date, &Local);
            let mut range = (day_start, day_end);
            if !self.daily.full_day && (!self.daily.periods.is_empty() || live.is_some()) {
                let earliest = self
                    .daily
                    .periods
                    .iter()
                    .map(|id| dataset.periods[id].start)
                    .chain(live.as_ref().map(|(_, period)| period.start))
                    .chain(
                        self.daily
                            .slices
                            .iter()
                            .filter(|slice| dataset.zones[&slice.id].full_range)
                            .map(|slice| slice.range.0),
                    )
                    .min();
                if let Some(start) = earliest {
                    range.0 = (start - Duration::minutes(15)).clamp(day_start, day_end);
                    range.1 = self
                        .daily
                        .periods
                        .iter()
                        .map(|id| dataset.periods[id].end)
                        .chain(
                            self.daily
                                .slices
                                .iter()
                                .filter(|slice| dataset.zones[&slice.id].full_range)
                                .map(|slice| slice.range.1),
                        )
                        .fold(now, DateTime::max);
                    range.1 = (range.1 + Duration::minutes(15)).clamp(range.0, day_end);
                    if range.0 == range.1 {
                        range = (day_start, day_end);
                    }
                }
            }
            draw_timeline(ui, &self.daily, dataset, now, range, live.as_ref());
            if self.daily.periods.is_empty() && live.is_none() {
                ui.weak("Сегодня ещё нет сессий.");
            }
        });

        if !self.daily.breakdown.is_empty() {
            card(ui, |ui| {
                ui.heading("Сводка по зонам");
                for zone in self
                    .daily
                    .slices
                    .iter()
                    .map(|slice| Some(slice.id))
                    .chain([None])
                {
                    let Some(detail) = self.daily.breakdown.get(&zone) else {
                        continue;
                    };
                    let name = zone.map_or("Без зоны", |id| &dataset.zones[&id].name);
                    ui.horizontal_wrapped(|ui| {
                        ui.strong(name);
                        if zone.is_some() {
                            ui.weak(format!("План {}", duration(detail.planned_ms)));
                        }
                        let total: i64 = detail.by_category.values().sum();
                        ui.weak(format!("Факт {}", duration(total)));
                    });
                    ui.horizontal_wrapped(|ui| {
                        let categories: std::collections::BTreeSet<_> = detail
                            .by_category
                            .keys()
                            .chain(detail.sessions.keys())
                            .copied()
                            .collect();
                        for category in categories {
                            ui.label(format!(
                                "{} · {} · {} сесс.",
                                category_name(dataset, category),
                                duration(
                                    detail
                                        .by_category
                                        .get(&category)
                                        .copied()
                                        .unwrap_or_default()
                                ),
                                detail.sessions.get(&category).copied().unwrap_or_default(),
                            ));
                        }
                    });
                }
            });
        }
        if let Some(buckets) = &self.daily.alignment {
            card(ui, |ui| {
                ui.heading("Соответствие плану по часам");
                ui.weak("За последние 30 дней");
                draw_alignment(ui, buckets, dataset);
            });
        }
    }

    pub(crate) fn statistics(&mut self, ui: &mut Ui, now: DateTime<Utc>) {
        let date = now.with_timezone(&Local).date_naive();
        let dataset = &self.state.dataset;
        let cache = &mut self.report;
        card(ui, |ui| {
            ui.heading("Статистика");
            ui.horizontal_wrapped(|ui| {
                for span in [Span::Day, Span::Week, Span::Month] {
                    ui.selectable_value(&mut cache.span, span, span_label(Some(span)));
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.selectable_value(&mut cache.kind, None, "Все сессии");
                ui.selectable_value(&mut cache.kind, Some(Kind::Work), kind_name(Kind::Work));
                ui.selectable_value(&mut cache.kind, Some(Kind::Break), kind_name(Kind::Break));
            });
        });
        let selected = cache.kind;
        let key = (self.revision, date, cache.span, selected);
        if cache.key != Some(key) {
            cache.key = Some(key);
            cache.value = Some(report(dataset, date, cache.span, selected, &Local));
        }
        let Some(report) = &cache.value else {
            return;
        };
        card(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.strong(format!("{} сессий", report.sessions));
                ui.strong(duration(report.active_ms));
                if report.sessions > 0 {
                    ui.weak(format!(
                        "В среднем {}",
                        duration(report.active_ms / report.sessions as i64)
                    ));
                }
            });
            let title = match cache.span {
                Span::Day => "Распределение по часам",
                Span::Week => "Распределение по дням недели",
                Span::Month => "Распределение по дням месяца",
            };
            ui.label(title);
            let labels: Vec<_> = if cache.span == Span::Day {
                (0..24).map(|hour| hour_label(hour, dataset)).collect()
            } else {
                report
                    .days
                    .iter()
                    .map(|day| {
                        if cache.span == Span::Week {
                            weekday_label(day.date.weekday().num_days_from_sunday() as u8)
                                .to_owned()
                        } else {
                            day.date.day().to_string()
                        }
                    })
                    .collect()
            };
            counts_chart(ui, &report.counts, &labels);
        });
        card(ui, |ui| {
            ui.heading("Активность");
            activity(ui, &report.days, cache.span, dataset);
        });
        if report.alignment.ratio().is_some() || report.alignment.uncategorized_ms > 0 {
            card(ui, |ui| {
                ui.heading("Соответствие зонам");
                ui.horizontal_wrapped(|ui| {
                    if let Some(ratio) = report.alignment.ratio() {
                        ui.strong(format!("{:.0}% совпадение", ratio * 100.0));
                    }
                    ui.label(format!(
                        "По плану {}",
                        duration(report.alignment.matched_ms)
                    ));
                    ui.label(format!(
                        "Другая категория {}",
                        duration(report.alignment.other_ms)
                    ));
                    ui.label(format!(
                        "Без категории {}",
                        duration(report.alignment.uncategorized_ms)
                    ));
                });
                let scored: Vec<_> = report
                    .days
                    .iter()
                    .filter_map(|day| day.alignment.ratio())
                    .collect();
                if !scored.is_empty() {
                    let average = scored.iter().sum::<f64>() / scored.len() as f64;
                    ui.weak(format!(
                        "В среднем по дням с измерениями {:.0}%",
                        average * 100.0
                    ));
                }
            });
        }
    }
}

fn draw_timeline(
    ui: &mut Ui,
    cache: &DailyCache,
    dataset: &Dataset,
    now: DateTime<Utc>,
    range: Moments,
    live: Option<&(Uuid, tracking_core::Period)>,
) {
    let (area, _) = ui.allocate_exact_size(vec2(ui.available_width(), 73.0), Sense::hover());
    let track = Rect::from_min_max(
        area.min + vec2(3.0, 1.0),
        pos2(area.right() - 3.0, area.top() + 48.0),
    );
    let span = (range.1 - range.0).num_milliseconds().max(1);
    let time_x = |time: DateTime<Utc>| {
        track.left()
            + track.width()
                * ((time - range.0).num_milliseconds() as f32 / span as f32).clamp(0.0, 1.0)
    };
    let painter = ui.painter().with_clip_rect(area);
    painter.rect_filled(track, 5, ui.visuals().extreme_bg_color);
    let axis_color = ui.visuals().weak_text_color();
    for slice in &cache.slices {
        let zone = &dataset.zones[&slice.id];
        let Some((start, end)) = intersect(slice.range, range) else {
            continue;
        };
        let rect = Rect::from_min_max(
            pos2(time_x(start), track.top()),
            pos2(time_x(end), track.bottom()),
        );
        let color = zone
            .color
            .as_deref()
            .and_then(parse_color)
            .unwrap_or(axis_color);
        match zone.style {
            ZoneStyle::Solid => {
                painter.rect_filled(rect, 3, alpha(color, 72));
            }
            ZoneStyle::Border => {
                painter.rect_stroke(rect, 3, Stroke::new(1.4, color), StrokeKind::Inside);
            }
            ZoneStyle::Hatched => {
                let clipped = painter.with_clip_rect(rect);
                let mut x = rect.left() - rect.height();
                while x < rect.right() {
                    clipped.line_segment(
                        [pos2(x, rect.bottom()), pos2(x + rect.height(), rect.top())],
                        Stroke::new(1.0, alpha(color, 110)),
                    );
                    x += 7.0;
                }
            }
        }
        ui.interact(rect, ui.id().with(("zone", slice.id)), Sense::hover())
            .on_hover_text(&zone.name);
    }
    for pair in cache.periods.windows(2) {
        let gap = (
            dataset.periods[&pair[0]].end,
            dataset.periods[&pair[1]].start,
        );
        if let Some((start, end)) = intersect(gap, range) {
            let y = track.bottom() - 5.0;
            painter.line_segment(
                [pos2(time_x(start), y), pos2(time_x(end), y)],
                Stroke::new(1.0, alpha(axis_color, 115)),
            );
        }
    }
    let finished = cache
        .periods
        .iter()
        .filter(|id| live.is_none_or(|(live_id, _)| **id != *live_id))
        .map(|id| (*id, &dataset.periods[id]));
    for (id, period) in finished.chain(live.iter().map(|(id, period)| (*id, period))) {
        for (index, segment) in period.segments().enumerate() {
            let Some((start, end)) = intersect(segment, range) else {
                continue;
            };
            let rect = Rect::from_min_max(
                pos2(time_x(start), track.top() + 17.0),
                pos2(
                    time_x(end).max(time_x(start) + 1.0).min(track.right()),
                    track.bottom() - 7.0,
                ),
            );
            let color = match period.kind {
                Kind::Work => {
                    default_color(period.category.and_then(|id| dataset.categories.get(&id)))
                }
                Kind::Break => Color32::from_rgb(125, 109, 164),
            };
            painter.rect_filled(rect, 2, color);
            ui.interact(rect, ui.id().with(("period", id, index)), Sense::hover())
                .on_hover_text(format!(
                    "{} · {}{}",
                    category_name(dataset, period.category),
                    duration(period.duration_millis()),
                    if period.note.is_empty() {
                        String::new()
                    } else {
                        format!("\n{}", period.note)
                    },
                ));
        }
    }
    if now >= range.0 && now <= range.1 {
        let x = time_x(now);
        painter.line_segment(
            [pos2(x, track.top()), pos2(x, track.bottom())],
            Stroke::new(1.5, ui.visuals().error_fg_color),
        );
    }
    let points = if area.width() < 410.0 { 3 } else { 5 };
    for index in 0..points {
        let moment = range.0 + Duration::milliseconds(span * index / (points - 1));
        let align = if index == 0 {
            Align2::LEFT_TOP
        } else if index == points - 1 {
            Align2::RIGHT_TOP
        } else {
            Align2::CENTER_TOP
        };
        let local = moment.with_timezone(&Local);
        let label = if index == points - 1
            && dataset.settings.hour_format == "24h"
            && local.time() == chrono::NaiveTime::MIN
        {
            "24:00".to_owned()
        } else {
            local.format(time_format(&dataset.settings)).to_string()
        };
        painter.text(
            pos2(time_x(moment), track.bottom() + 7.0),
            align,
            label,
            FontId::proportional(11.0),
            axis_color,
        );
    }
}

fn counts_chart(ui: &mut Ui, counts: &[usize], labels: &[String]) {
    if counts.is_empty() {
        return;
    }
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 120.0), Sense::hover());
    let width = rect.width() / counts.len() as f32;
    let max = counts.iter().copied().max().unwrap_or(0).max(1) as f32;
    let label_stride = (38.0 / width).ceil() as usize;
    let accent = accent(ui);
    let painter = ui.painter();
    for (index, count) in counts.iter().enumerate() {
        let center = rect.left() + width * (index as f32 + 0.5);
        let bottom = rect.bottom() - 19.0;
        let height = ((*count as f32 / max) * (rect.height() - 22.0)).max(2.0);
        let bar = Rect::from_center_size(
            pos2(center, bottom - height * 0.5),
            vec2((width - 3.0).clamp(1.0, 46.0), height),
        );
        painter.rect_filled(
            bar,
            2,
            if *count == 0 {
                ui.visuals().extreme_bg_color
            } else {
                accent
            },
        );
        ui.interact(bar, ui.id().with(("distribution", index)), Sense::hover())
            .on_hover_text(format!("{} · {count} сессий", labels[index]));
        if index.is_multiple_of(label_stride.max(1)) {
            painter.text(
                pos2(center, bottom + 4.0),
                Align2::CENTER_TOP,
                &labels[index],
                FontId::proportional(10.0),
                ui.visuals().weak_text_color(),
            );
        }
    }
}

fn activity(
    ui: &mut Ui,
    days: &[tracking_core::reports::ActivityDay],
    span: Span,
    dataset: &Dataset,
) {
    let peak = days.iter().map(|day| day.work_ms).max().unwrap_or(1).max(1);
    let first = match dataset.settings.first_day_of_week.as_str() {
        "sunday" => 0,
        "saturday" => 6,
        _ => 1,
    };
    let offset = if span == Span::Month {
        days.first().map_or(0, |day| {
            (day.date.weekday().num_days_from_sunday() + 7 - first) % 7
        }) as usize
    } else {
        0
    };
    let cell_width = ((ui.available_width() - 6.0 * 5.0) / 7.0).clamp(16.0, 56.0);
    let slots = (offset + days.len()).div_ceil(7) * 7;
    for row in 0..slots / 7 {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
            for slot in row * 7..(row + 1) * 7 {
                let (rect, response) =
                    ui.allocate_exact_size(vec2(cell_width, 31.0), Sense::hover());
                let Some(day) = slot.checked_sub(offset).and_then(|index| days.get(index)) else {
                    continue;
                };
                ui.painter().rect_filled(
                    rect,
                    4,
                    if day.work_ms == 0 {
                        ui.visuals().extreme_bg_color
                    } else {
                        alpha(
                            accent(ui),
                            (38.0 + 130.0 * day.work_ms as f32 / peak as f32) as u8,
                        )
                    },
                );
                ui.painter().text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    day.date.day(),
                    FontId::proportional(12.0),
                    ui.visuals().text_color(),
                );
                response.on_hover_text(format!(
                    "{} · {}",
                    day.date.format(date_format(&dataset.settings)),
                    duration(day.work_ms)
                ));
            }
        });
    }
}

fn draw_alignment(ui: &mut Ui, buckets: &[Alignment; 24], dataset: &Dataset) {
    let values = buckets.map(|value| value.matched_ms - value.other_ms);
    let peak = values
        .iter()
        .map(|value| value.abs())
        .max()
        .unwrap_or(1)
        .max(1);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 83.0), Sense::hover());
    let width = rect.width() / 24.0;
    let mid = rect.center().y - 5.0;
    let painter = ui.painter();
    painter.hline(
        rect.x_range(),
        mid,
        Stroke::new(1.0, ui.visuals().weak_text_color()),
    );
    for (hour, score) in values.into_iter().enumerate() {
        let x = rect.left() + width * hour as f32;
        let y = mid - 30.0 * score as f32 / peak as f32;
        let bar = Rect::from_two_pos(pos2(x + 1.0, mid), pos2(x + width - 1.0, y));
        painter.rect_filled(
            bar,
            1,
            if score < 0 {
                ui.visuals().error_fg_color
            } else {
                accent(ui)
            },
        );
        let area = Rect::from_min_size(Pos2::new(x, rect.top()), vec2(width, 66.0));
        let value = buckets[hour];
        ui.interact(area, ui.id().with(("alignment", hour)), Sense::hover())
            .on_hover_text(format!(
                "{}\nПо плану {} · Другая категория {}",
                hour_label(hour, dataset),
                duration(value.matched_ms),
                duration(value.other_ms),
            ));
        if hour.is_multiple_of(if rect.width() < 450.0 { 6 } else { 3 }) {
            painter.text(
                pos2(x, rect.bottom() - 3.0),
                Align2::LEFT_BOTTOM,
                hour_label(hour, dataset),
                FontId::proportional(10.0),
                ui.visuals().weak_text_color(),
            );
        }
    }
}

fn hour_label(hour: usize, dataset: &Dataset) -> String {
    chrono::NaiveTime::from_hms_opt(hour as u32, 0, 0)
        .unwrap()
        .format(if dataset.settings.hour_format == "12h" {
            "%I %p"
        } else {
            "%H"
        })
        .to_string()
}

fn alpha(color: Color32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), a)
}
