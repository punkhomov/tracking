use std::sync::Arc;

use anyhow::{Result, anyhow};
use tracking_cloud::{SavedSession, SessionStorage};

use crate::TrackerError;

#[uniffi::export(foreign)]
pub trait SessionVault: Send + Sync {
    fn load(&self) -> Result<Option<Vec<u8>>, TrackerError>;
    fn save(&self, session: Vec<u8>) -> Result<(), TrackerError>;
    fn clear(&self) -> Result<(), TrackerError>;
}

pub(super) struct AndroidStorage(pub Arc<dyn SessionVault>);

impl SessionStorage for AndroidStorage {
    fn load(&self) -> Result<Option<SavedSession>> {
        self.0
            .load()?
            .map(|bytes| {
                serde_json::from_slice(&bytes)
                    .map_err(|_| anyhow!("Не удалось прочитать сохранённую сессию Supabase."))
            })
            .transpose()
    }

    fn save(&self, session: &SavedSession) -> Result<()> {
        Ok(self.0.save(serde_json::to_vec(session)?)?)
    }

    fn clear(&self) -> Result<()> {
        Ok(self.0.clear()?)
    }
}
