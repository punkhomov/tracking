use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

use tracking_core::Dataset;
use uuid::Uuid;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Preference {
    Ask,
    Local,
    Remote,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Conflict {
    pub count: usize,
}

impl fmt::Display for Conflict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "На устройствах изменены одни и те же записи ({})",
            self.count
        )
    }
}

impl std::error::Error for Conflict {}

pub fn merge(
    base: &Dataset,
    local: &Dataset,
    remote: &Dataset,
    preference: Preference,
) -> Result<Dataset, Conflict> {
    let mut conflicts = 0;
    let settings = choose(
        &base.settings,
        &local.settings,
        &remote.settings,
        preference,
        &mut conflicts,
    )
    .clone();

    let dataset = Dataset {
        settings,
        periods: merge_records(
            &base.periods,
            &local.periods,
            &remote.periods,
            preference,
            &mut conflicts,
        ),
        categories: merge_records(
            &base.categories,
            &local.categories,
            &remote.categories,
            preference,
            &mut conflicts,
        ),
        zones: merge_records(
            &base.zones,
            &local.zones,
            &remote.zones,
            preference,
            &mut conflicts,
        ),
    };
    if conflicts > 0 && preference == Preference::Ask {
        return Err(Conflict { count: conflicts });
    }
    Ok(dataset)
}

fn merge_records<T: Clone + Eq>(
    base: &BTreeMap<Uuid, T>,
    local: &BTreeMap<Uuid, T>,
    remote: &BTreeMap<Uuid, T>,
    preference: Preference,
    conflicts: &mut usize,
) -> BTreeMap<Uuid, T> {
    let keys: BTreeSet<_> = base
        .keys()
        .chain(local.keys())
        .chain(remote.keys())
        .copied()
        .collect();
    keys.into_iter()
        .filter_map(|id| {
            choose(
                &base.get(&id),
                &local.get(&id),
                &remote.get(&id),
                preference,
                conflicts,
            )
            .map(|value| (id, value.clone()))
        })
        .collect()
}

fn choose<'a, T: Eq>(
    base: &T,
    local: &'a T,
    remote: &'a T,
    preference: Preference,
    conflicts: &mut usize,
) -> &'a T {
    if local == remote || remote == base {
        local
    } else if local == base {
        remote
    } else {
        *conflicts += 1;
        if preference == Preference::Remote {
            remote
        } else {
            local
        }
    }
}
