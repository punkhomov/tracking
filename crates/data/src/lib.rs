pub mod cloud;
mod merge;

pub use merge::{Conflict, Preference, merge};

use std::{path::Path, time::Duration};

use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use tracking_core::{Dataset, Kind, Timer};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    pub project: String,
    pub user: Uuid,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct LocalState {
    pub dataset: Dataset,
    pub timer: Option<Timer>,
    pub next_kind: Kind,
    pub account: Option<Account>,
    pub baseline: Option<Dataset>,
}

pub struct Database {
    connection: Connection,
    generation: i64,
}

impl Database {
    pub fn open(path: &Path) -> Result<(Self, LocalState)> {
        let connection = Connection::open(path).context("Не удалось открыть базу трекера.")?;
        connection.busy_timeout(Duration::from_secs(2))?;
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS local_state (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                generation INTEGER NOT NULL,
                body TEXT NOT NULL
            );",
        )?;
        let row: Option<(i64, String)> = connection
            .query_row(
                "SELECT generation, body FROM local_state WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let (generation, state) = match row {
            Some((generation, body)) => {
                let state: LocalState = serde_json::from_str(&body)?;
                state.dataset.validate()?;
                (generation, state)
            }
            None => (0, LocalState::default()),
        };
        Ok((
            Self {
                connection,
                generation,
            },
            state,
        ))
    }

    pub fn save(&mut self, state: &LocalState) -> Result<()> {
        let body = serde_json::to_string(state)?;
        let saved = if self.generation == 0 {
            self.connection.execute(
                "INSERT INTO local_state VALUES (1, 1, ?1) ON CONFLICT DO NOTHING",
                [body],
            )?
        } else {
            self.connection.execute(
                "UPDATE local_state SET generation = generation + 1, body = ?1
                 WHERE id = 1 AND generation = ?2",
                params![body, self.generation],
            )?
        };
        ensure!(
            saved == 1,
            "База изменилась в другом окне. Перезапустите трекер."
        );
        self.generation += 1;
        Ok(())
    }
}
