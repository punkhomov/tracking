use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

use app_platform::cloud::Preference;
use tracking_core::Dataset;
use uuid::Uuid;

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

#[cfg(test)]
mod tests {
    use super::*;
    use tracking_core::Category;

    #[test]
    fn independent_changes_and_deletes_merge_and_same_record_changes_conflict() {
        let mut base = Dataset::default();
        let id = Uuid::new_v4();
        let other = Uuid::new_v4();
        let category = Category {
            name: "Работа".into(),
            color: "#39548d".into(),
            archived: false,
        };
        base.categories.insert(id, category.clone());
        let mut local = base.clone();
        let mut remote = base.clone();
        local.categories.remove(&id);
        remote.categories.insert(other, category.clone());
        let merged = merge(&base, &local, &remote, Preference::Ask).unwrap();
        assert!(!merged.categories.contains_key(&id));
        assert_eq!(merged.categories.get(&other), Some(&category));

        remote.categories.get_mut(&id).unwrap().name = "Обновление".into();
        assert_eq!(
            merge(&base, &local, &remote, Preference::Ask)
                .unwrap_err()
                .count,
            1
        );
        assert_eq!(
            merge(&base, &local, &remote, Preference::Local).unwrap(),
            merged
        );
        assert_eq!(
            merge(&base, &local, &remote, Preference::Remote).unwrap(),
            remote
        );
    }
}
