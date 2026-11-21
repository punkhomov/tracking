use std::collections::BTreeMap;

use chrono::{DateTime, Datelike, Days, Duration, NaiveDate, NaiveTime, TimeZone, Timelike, Utc};
use uuid::Uuid;

use crate::{Dataset, Kind, Moments, Period};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum Span {
    #[default]
    Day,
    Week,
    Month,
}

#[derive(Clone, Copy)]
pub struct ZoneSlice {
    pub id: Uuid,
    pub range: Moments,
}

#[derive(Default)]
pub struct Breakdown {
    pub sessions: BTreeMap<Option<Uuid>, usize>,
    pub planned_ms: i64,
    pub by_category: BTreeMap<Option<Uuid>, i64>,
}

#[derive(Default, Clone, Copy)]
pub struct Alignment {
    pub matched_ms: i64,
    pub other_ms: i64,
    pub uncategorized_ms: i64,
}

impl Alignment {
    pub fn ratio(self) -> Option<f64> {
        let scored = self.matched_ms + self.other_ms;
        (scored > 0).then(|| self.matched_ms as f64 / scored as f64)
    }

    fn add(&mut self, score: Self) {
        self.matched_ms += score.matched_ms;
        self.other_ms += score.other_ms;
        self.uncategorized_ms += score.uncategorized_ms;
    }
}

pub struct ActivityDay {
    pub date: NaiveDate,
    pub work_ms: i64,
    pub alignment: Alignment,
}

pub struct Report {
    pub sessions: usize,
    pub active_ms: i64,
    pub counts: Vec<usize>,
    pub days: Vec<ActivityDay>,
    pub alignment: Alignment,
}

pub fn span_dates(today: NaiveDate, span: Span, first_day: &str) -> (NaiveDate, NaiveDate) {
    match span {
        Span::Day => (today, today + Days::new(1)),
        Span::Week => {
            let first = match first_day {
                "sunday" => 0,
                "saturday" => 6,
                _ => 1,
            };
            let offset = (today.weekday().num_days_from_sunday() + 7 - first) % 7;
            let start = today - Days::new(u64::from(offset));
            (start, start + Days::new(7))
        }
        Span::Month => {
            let start = today.with_day(1).unwrap_or(today);
            (start, start + chrono::Months::new(1))
        }
    }
}

pub fn local_time<T: TimeZone>(date: NaiveDate, time: NaiveTime, tz: &T) -> DateTime<Utc> {
    let nominal = date.and_time(time);
    // A local boundary may land in a daylight-saving gap or a skipped calendar day.
    for offset in 0..=1440 {
        if let Some(value) = tz
            .from_local_datetime(&(nominal + Duration::minutes(offset)))
            .earliest()
        {
            return value.with_timezone(&Utc);
        }
    }
    nominal.and_utc()
}

pub fn day_range<T: TimeZone>(date: NaiveDate, tz: &T) -> Moments {
    (
        local_time(date, NaiveTime::MIN, tz),
        local_time(date + Days::new(1), NaiveTime::MIN, tz),
    )
}

pub fn zone_slices<T: TimeZone>(dataset: &Dataset, date: NaiveDate, tz: &T) -> Vec<ZoneSlice> {
    let mut result: Vec<_> = dataset
        .zones
        .iter()
        .filter_map(|(&id, zone)| {
            if zone.exclude_from_breakdown || !zone.is_scheduled_on(date) {
                return None;
            }
            if let Some(parent) = zone.parent {
                let parent = dataset.zones.get(&parent)?;
                if parent.exclude_from_breakdown || !parent.is_scheduled_on(date) {
                    return None;
                }
            }
            let start = local_time(date, zone.start, tz);
            let end = local_time(date, zone.end, tz);
            (start < end).then_some(ZoneSlice {
                id,
                range: (start, end),
            })
        })
        .collect();
    result.sort_by_key(|slice| {
        (
            dataset.zones[&slice.id].parent.is_some(),
            slice.range.0,
            slice.id,
        )
    });
    result
}

pub fn zone_breakdown<T: TimeZone>(
    dataset: &Dataset,
    date: NaiveDate,
    tz: &T,
) -> BTreeMap<Option<Uuid>, Breakdown> {
    let day = day_range(date, tz);
    let slices = zone_slices(dataset, date, tz);
    let mut result = BTreeMap::new();
    let roots: Vec<_> = slices
        .iter()
        .filter(|slice| dataset.zones[&slice.id].parent.is_none())
        .collect();
    for root in &roots {
        let children = slices
            .iter()
            .filter(|slice| dataset.zones[&slice.id].parent == Some(root.id))
            .filter_map(|slice| intersect(slice.range, root.range));
        let breaks = union(children);
        let planned_ms = millis(root.range) - breaks.into_iter().map(millis).sum::<i64>();
        result.insert(
            Some(root.id),
            Breakdown {
                planned_ms,
                ..Breakdown::default()
            },
        );
    }
    let covered = union(roots.iter().map(|root| root.range));
    for period in work_periods(dataset, day) {
        let mut last = None;
        for root in &roots {
            let value = period.active_millis(root.range);
            if value > 0 {
                last = Some(root.id);
                add_duration(&mut result, last, period.category, value);
            }
        }
        let total = period.active_millis(day);
        let in_zones: i64 = covered
            .iter()
            .map(|&range| period.active_millis(range))
            .sum();
        let remainder = total - in_zones;
        let target = if remainder < 10 * 60_000 { last } else { None };
        add_duration(&mut result, target, period.category, remainder);
        if (day.0..day.1).contains(&period.start) {
            let home = roots
                .iter()
                .find(|zone| (zone.range.0..zone.range.1).contains(&period.start))
                .map(|zone| zone.id);
            *result
                .entry(home)
                .or_default()
                .sessions
                .entry(period.category)
                .or_default() += 1;
        }
    }
    result
}

pub fn report<T: TimeZone>(
    dataset: &Dataset,
    today: NaiveDate,
    span: Span,
    kind: Option<Kind>,
    tz: &T,
) -> Report {
    let (start_date, end_date) = span_dates(today, span, &dataset.settings.first_day_of_week);
    let range = (day_range(start_date, tz).0, day_range(end_date, tz).0);
    let day_count = (end_date - start_date).num_days() as usize;
    let mut result = Report {
        sessions: 0,
        active_ms: 0,
        counts: vec![0; if span == Span::Day { 24 } else { day_count }],
        days: Vec::with_capacity(day_count),
        alignment: Alignment::default(),
    };
    for period in dataset
        .periods
        .values()
        .filter(|period| kind.is_none_or(|kind| period.kind == kind))
    {
        if intersect((period.start, period.end), range).is_some() {
            result.active_ms += period.active_millis(range);
        }
        if (range.0..range.1).contains(&period.start) {
            let local = period.start.with_timezone(tz);
            let bucket = if span == Span::Day {
                local.hour() as usize
            } else {
                (local.date_naive() - start_date).num_days() as usize
            };
            if let Some(count) = result.counts.get_mut(bucket) {
                *count += 1;
                result.sessions += 1;
            }
        }
    }

    for offset in 0..day_count as u64 {
        let date = start_date + Days::new(offset);
        let day = day_range(date, tz);
        let slices = linked_zones(dataset, date, tz);
        let periods: Vec<_> = work_periods(dataset, day).collect();
        let work_ms = periods.iter().map(|period| period.active_millis(day)).sum();
        let alignment = score(&periods, day, &slices);
        result.alignment.add(alignment);
        result.days.push(ActivityDay {
            date,
            work_ms,
            alignment,
        });
    }
    result
}

pub fn hourly_alignment<T: TimeZone>(
    dataset: &Dataset,
    today: NaiveDate,
    tz: &T,
) -> [Alignment; 24] {
    let mut results = [Alignment::default(); 24];
    for offset in 0..30 {
        let date = today - Days::new(offset);
        let slices = linked_zones(dataset, date, tz);
        if slices.is_empty() {
            continue;
        }
        let day = day_range(date, tz);
        let periods: Vec<_> = work_periods(dataset, day).collect();
        let mut start = day.0;
        while start < day.1 {
            let end = (start + Duration::hours(1)).min(day.1);
            let hour = start.with_timezone(tz).hour() as usize;
            results[hour].add(score(&periods, (start, end), &slices));
            start = end;
        }
    }
    results
}

fn score(periods: &[&Period], range: Moments, zones: &[(Moments, Uuid)]) -> Alignment {
    let mut result = Alignment::default();
    for (zone, target) in zones {
        let Some(overlap) = intersect(*zone, range) else {
            continue;
        };
        for period in periods {
            let value = period.active_millis(overlap);
            match period.category {
                Some(category) if category == *target => result.matched_ms += value,
                Some(_) => result.other_ms += value,
                None => result.uncategorized_ms += value,
            }
        }
    }
    result
}

fn linked_zones<T: TimeZone>(dataset: &Dataset, date: NaiveDate, tz: &T) -> Vec<(Moments, Uuid)> {
    zone_slices(dataset, date, tz)
        .into_iter()
        .filter_map(|slice| {
            let zone = &dataset.zones[&slice.id];
            if zone.parent.is_some() {
                return None;
            }
            Some((slice.range, zone.category?))
        })
        .collect()
}

fn add_duration(
    result: &mut BTreeMap<Option<Uuid>, Breakdown>,
    zone: Option<Uuid>,
    category: Option<Uuid>,
    value: i64,
) {
    if value > 0 {
        *result
            .entry(zone)
            .or_default()
            .by_category
            .entry(category)
            .or_default() += value;
    }
}

fn work_periods(dataset: &Dataset, range: Moments) -> impl Iterator<Item = &Period> {
    dataset.periods.values().filter(move |period| {
        period.kind == Kind::Work && intersect((period.start, period.end), range).is_some()
    })
}

pub fn intersect(left: Moments, right: Moments) -> Option<Moments> {
    let range = (left.0.max(right.0), left.1.min(right.1));
    (range.0 < range.1).then_some(range)
}

fn millis(range: Moments) -> i64 {
    (range.1 - range.0).num_milliseconds()
}

fn union(ranges: impl IntoIterator<Item = Moments>) -> Vec<Moments> {
    let mut ranges: Vec<_> = ranges.into_iter().collect();
    ranges.sort_unstable();
    let mut result: Vec<Moments> = Vec::new();
    for range in ranges {
        if let Some(last) = result.last_mut()
            && range.0 <= last.1
        {
            last.1 = last.1.max(range.1);
        } else {
            result.push(range);
        }
    }
    result
}
