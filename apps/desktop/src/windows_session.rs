use std::{
    collections::HashMap,
    fs::{File, OpenOptions, TryLockError},
    path::Path,
};

use anyhow::{Context, Result, bail};
use keyring_core::{Entry, Error, api::CredentialStoreApi};
use tracking_cloud::{SavedSession, SessionStorage};
use uuid::Uuid;

pub(super) struct Store {
    entry: Entry,
    // One worker owns refresh token rotation for this database file.
    _lock: File,
}

impl Store {
    pub(super) fn open(database: &Path) -> Result<Self> {
        let database = database
            .canonicalize()
            .context("Не удалось определить путь профиля.")?;
        let mut lock_path = database.as_os_str().to_owned();
        lock_path.push(".supabase.lock");
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path)
            .context("Не удалось открыть блокировку сессии Supabase.")?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                bail!("Синхронизация этого профиля уже открыта в другом окне Tracking.")
            }
            Err(TryLockError::Error(error)) => {
                return Err(error).context("Не удалось заблокировать сессию Supabase.");
            }
        }
        let profile = Uuid::new_v5(
            &Uuid::NAMESPACE_URL,
            database.as_os_str().as_encoded_bytes(),
        );
        let target = format!("dev.punkhomov.Tracking/Supabase/{profile}");
        let modifiers = HashMap::from([("target", target.as_str()), ("persistence", "Local")]);
        let entry = windows_native_keyring_store::Store::new()
            .and_then(|store| store.build("Tracking Supabase", "", Some(&modifiers)))
            .context("Диспетчер учётных данных Windows недоступен.")?;
        Ok(Self { entry, _lock: lock })
    }
}

impl SessionStorage for Store {
    fn load(&self) -> Result<Option<SavedSession>> {
        match self.entry.get_secret() {
            Ok(secret) => serde_json::from_slice(&secret)
                .map(Some)
                .map_err(|_| anyhow::anyhow!("Не удалось прочитать сохранённую сессию Supabase.")),
            Err(Error::NoEntry) => Ok(None),
            Err(error) => Err(error)
                .context("Не удалось прочитать сессию в диспетчере учётных данных Windows."),
        }
    }

    fn save(&self, session: &SavedSession) -> Result<()> {
        self.entry
            .set_secret(&serde_json::to_vec(session)?)
            .context("Не удалось сохранить сессию в диспетчере учётных данных Windows.")
    }

    fn clear(&self) -> Result<()> {
        match self.entry.delete_credential() {
            Ok(()) | Err(Error::NoEntry) => Ok(()),
            Err(error) => Err(error)
                .context("Не удалось удалить сессию из диспетчера учётных данных Windows."),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn credential_survives_reopening_the_profile() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let profile = directory.path().join("tracking.sqlite3");
        File::create(&profile)?;
        let value = json!({
            "account": {"project": "https://example.test/", "user": Uuid::new_v4()},
            "refresh_token": "synthetic-credential",
        });
        let session = serde_json::from_value(value.clone())?;
        {
            let store = Store::open(&profile)?;
            assert!(Store::open(&profile).is_err());
            store.save(&session)?;
        }
        let store = Store::open(&profile)?;
        let saved = store.load();
        store.clear()?;
        assert_eq!(serde_json::to_value(saved?)?, value);
        assert!(store.load()?.is_none());
        Ok(())
    }
}
