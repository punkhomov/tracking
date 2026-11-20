use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};
use chrono::{DateTime, NaiveTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{Category, Dataset, Interval, Kind, Period, Settings, Zone, ZoneStyle};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyArchive {
    #[serde(rename = "type")]
    kind: ArchiveScope,
    settings: Option<Settings>,
    periods: Option<Vec<LegacyPeriod>>,
    categories: Option<Vec<LegacyCategory>>,
    timeline_zones: Option<Vec<LegacyZone>>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArchiveScope {
    Settings,
    Periods,
    #[default]
    Full,
}

impl ArchiveScope {
    pub fn has_settings(self) -> bool {
        self != Self::Periods
    }

    pub fn has_history(self) -> bool {
        self != Self::Settings
    }
}

#[derive(Clone)]
pub struct ImportArchive {
    pub scope: ArchiveScope,
    pub settings: Option<Settings>,
    pub periods: Option<BTreeMap<Uuid, Period>>,
    pub categories: Option<BTreeMap<Uuid, Category>>,
    pub zones: Option<BTreeMap<Uuid, Zone>>,
}

#[derive(Deserialize)]
struct LegacyCategory {
    id: i64,
    #[serde(flatten)]
    value: Category,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyPeriod {
    id: i64,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    #[serde(default)]
    note: String,
    category_id: Option<i64>,
    #[serde(rename = "type", default)]
    kind: Kind,
    #[serde(default)]
    pause_intervals: Vec<Interval>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyZone {
    id: String,
    name: String,
    start_time: String,
    end_time: String,
    weekdays: Vec<u8>,
    parent_id: Option<String>,
    color: Option<String>,
    #[serde(default)]
    style: ZoneStyle,
    #[serde(default)]
    force_full_range_in_timeline: bool,
    #[serde(default)]
    exclude_from_zone_breakdown: bool,
    linked_category_id: Option<i64>,
}

fn legacy_id(kind: &str, old: impl std::fmt::Display) -> Uuid {
    Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("dev.punkhomov.tracking/v1/{kind}/{old}").as_bytes(),
    )
}

pub fn read_archive(content: &str) -> Result<ImportArchive> {
    let json: serde_json::Value = serde_json::from_str(content).context("Неверный JSON.")?;
    match json["version"].as_u64() {
        Some(1) => {
            let legacy: LegacyArchive = serde_json::from_value(json)?;
            let scope = legacy.kind;
            let periods = if scope.has_history() {
                legacy.periods.map(|periods| {
                    periods
                        .into_iter()
                        .map(|mut period| {
                            let pauses = normalize_pauses(&mut period);
                            (
                                legacy_id("period", period.id),
                                Period {
                                    start: period.start,
                                    end: period.end,
                                    note: period.note,
                                    category: period
                                        .category_id
                                        .map(|id| legacy_id("category", id)),
                                    kind: period.kind,
                                    pauses,
                                },
                            )
                        })
                        .collect()
                })
            } else {
                None
            };
            let categories = if scope.has_history() {
                legacy.categories.map(|categories| {
                    categories
                        .into_iter()
                        .map(|category| (legacy_id("category", category.id), category.value))
                        .collect()
                })
            } else {
                None
            };
            let zones = if scope.has_settings() {
                legacy
                    .timeline_zones
                    .map(|zones| {
                        zones
                            .into_iter()
                            .map(|zone| {
                                Ok((
                                    legacy_id("zone", zone.id),
                                    Zone {
                                        name: zone.name,
                                        start: legacy_time(&zone.start_time)?,
                                        end: legacy_time(&zone.end_time)?,
                                        weekdays: zone.weekdays,
                                        parent: zone.parent_id.map(|id| legacy_id("zone", id)),
                                        color: zone.color,
                                        style: zone.style,
                                        full_range: zone.force_full_range_in_timeline,
                                        exclude_from_breakdown: zone.exclude_from_zone_breakdown,
                                        category: zone
                                            .linked_category_id
                                            .map(|id| legacy_id("category", id)),
                                    },
                                ))
                            })
                            .collect::<Result<_>>()
                    })
                    .transpose()?
            } else {
                None
            };
            Ok(ImportArchive {
                scope,
                periods,
                categories,
                zones,
                settings: legacy.settings.filter(|_| scope.has_settings()),
            })
        }
        Some(2) => {
            let archive: Archive = serde_json::from_value(json)?;
            let scope = archive.scope;
            let dataset = archive.dataset;
            Ok(ImportArchive {
                scope,
                settings: scope.has_settings().then_some(dataset.settings),
                periods: scope.has_history().then_some(dataset.periods),
                categories: scope.has_history().then_some(dataset.categories),
                zones: scope.has_settings().then_some(dataset.zones),
            })
        }
        _ => bail!("Неизвестная версия экспорта."),
    }
}

impl ImportArchive {
    pub fn apply(self, current: &Dataset, replace_history: bool) -> Result<Dataset> {
        let mut next = current.clone();
        if let Some(settings) = self.settings {
            next.settings = settings;
        }
        if let Some(zones) = self.zones {
            next.zones = zones;
        }
        apply_records(&mut next.periods, self.periods, replace_history);
        apply_records(&mut next.categories, self.categories, replace_history);
        next.validate()?;
        Ok(next)
    }
}

fn apply_records<T>(
    current: &mut BTreeMap<Uuid, T>,
    records: Option<BTreeMap<Uuid, T>>,
    replace: bool,
) {
    if let Some(records) = records {
        if replace {
            *current = records;
        } else {
            for (id, record) in records {
                current.entry(id).or_insert(record);
            }
        }
    }
}

pub fn import_json(current: &Dataset, content: &str) -> Result<Dataset> {
    let mut archive = read_archive(content)?;
    if let Some(incoming) = &mut archive.zones {
        incoming.extend(current.zones.clone());
    }
    archive.apply(current, false)
}

#[derive(Serialize, Deserialize)]
struct Archive {
    version: u32,
    #[serde(default)]
    scope: ArchiveScope,
    dataset: Dataset,
}

pub fn export_json(dataset: &Dataset) -> Result<String> {
    export_archive(dataset, ArchiveScope::Full)
}

pub fn export_archive(dataset: &Dataset, scope: ArchiveScope) -> Result<String> {
    let dataset = match scope {
        ArchiveScope::Full => dataset.clone(),
        ArchiveScope::Settings => Dataset {
            settings: dataset.settings.clone(),
            zones: dataset.zones.clone(),
            ..Dataset::default()
        },
        ArchiveScope::Periods => Dataset {
            periods: dataset.periods.clone(),
            categories: dataset.categories.clone(),
            ..Dataset::default()
        },
    };
    Ok(serde_json::to_string_pretty(&Archive {
        version: 2,
        scope,
        dataset,
    })?)
}

fn legacy_time(time: &str) -> Result<NaiveTime> {
    NaiveTime::parse_from_str(time, "%H:%M")
        .or_else(|_| time.parse())
        .context("Неверное время в зоне старого архива.")
}

fn normalize_pauses(period: &mut LegacyPeriod) -> Vec<Interval> {
    // The old editor could move period boundaries while leaving its pauses in place.
    period.pause_intervals.sort_by_key(|pause| pause.start);
    let mut pauses: Vec<Interval> = Vec::new();
    for pause in &period.pause_intervals {
        let start = pause.start.max(period.start);
        let end = pause.end.min(period.end);
        if start >= end {
            continue;
        }
        if let Some(last) = pauses.last_mut()
            && start <= last.end
        {
            last.end = last.end.max(end);
        } else {
            pauses.push(Interval { start, end });
        }
    }
    pauses
}
