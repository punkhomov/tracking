use std::sync::{MutexGuard, TryLockError};

use anyhow::{Context, Result, anyhow};
use app_platform::cloud::{
    Account, CloudRequest, CloudResponse, Connection, Preference, Setup, SyncSchedule,
};
use chrono::Utc;
use tokio::runtime::Runtime;
use tracking_cloud::Service;

use crate::{Tracker, TrackerError};

pub(super) struct Client {
    runtime: Runtime,
    service: Service,
    account: Option<Account>,
    schedule: SyncSchedule,
    dataset_conflict: Option<usize>,
}

impl Client {
    pub(super) fn new(service: Service) -> Result<Self> {
        Ok(Self {
            runtime: tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?,
            service,
            account: None,
            schedule: SyncSchedule::default(),
            dataset_conflict: None,
        })
    }

    fn execute(&mut self, request: CloudRequest) -> Result<CloudResponse> {
        self.runtime.block_on(self.service.execute(request))
    }

    fn set_account(&mut self, account: Option<Account>) {
        self.account = account;
        self.schedule = SyncSchedule::default();
        self.dataset_conflict = None;
    }

    fn sign_out(&mut self) -> Result<()> {
        self.execute(CloudRequest::SignOut)?;
        self.set_account(None);
        Ok(())
    }
}

#[derive(uniffi::Record)]
pub struct SyncView {
    pub url: Option<String>,
    pub key: Option<String>,
    pub signed_in: bool,
    pub needs_confirmation: bool,
    pub timer_pending: bool,
    pub timer_conflict: bool,
    pub dataset_conflicts: Option<u64>,
}

#[derive(uniffi::Enum)]
pub enum SyncChoice {
    Merge,
    Local,
    Remote,
}

#[uniffi::export]
impl Tracker {
    pub fn sync_status(&self) -> Result<SyncView, TrackerError> {
        let client = self.cloud_lock()?;
        let store = self.lock()?;
        let state = &store.state.cloud;
        let connection = match &state.setup {
            Setup::Configured(connection) => Some(connection),
            _ => None,
        };
        let account = client.account.as_ref();
        Ok(SyncView {
            url: connection.map(|connection| connection.url().into()),
            key: connection.map(|connection| connection.key().into()),
            signed_in: account.is_some(),
            needs_confirmation: account.is_some_and(|account| !state.matches_account(account)),
            timer_pending: state.timer.is_pending(),
            timer_conflict: state.timer.has_conflict(),
            dataset_conflicts: client.dataset_conflict.map(|count| count as u64),
        })
    }

    pub fn configure_sync(&self, url: String, key: String) -> Result<(), TrackerError> {
        let setup = Setup::Configured(Connection::new(&url, &key)?);
        let mut client = self.cloud_lock()?;
        let mut store = self.lock()?;
        if store.state.cloud.setup == setup {
            return Ok(());
        }
        client.sign_out()?;
        let mut cloud = store.state.cloud.clone();
        cloud.setup = setup;
        let local = store.tracker_state();
        store.save(local, cloud)?;
        Ok(())
    }

    pub fn restore_session(&self) -> Result<(), TrackerError> {
        let mut client = self.cloud_lock()?;
        if client.account.is_some() {
            return Ok(());
        }
        let connection = match &self.lock()?.state.cloud.setup {
            Setup::Configured(connection) => connection.clone(),
            _ => return Ok(()),
        };
        if let CloudResponse::SignedIn(account) =
            client.execute(CloudRequest::RestoreSession(connection))?
        {
            client.set_account(Some(account));
        }
        Ok(())
    }

    pub fn request_code(&self, email: String) -> Result<(), TrackerError> {
        let mut client = self.cloud_lock()?;
        let connection = self.connection()?;
        client.execute(CloudRequest::RequestCode { connection, email })?;
        Ok(())
    }

    pub fn verify_code(&self, email: String, code: String) -> Result<(), TrackerError> {
        let mut client = self.cloud_lock()?;
        let connection = self.connection()?;
        if let CloudResponse::SignedIn(account) = client.execute(CloudRequest::VerifyCode {
            connection,
            email,
            code,
        })? {
            client.set_account(Some(account));
        }
        Ok(())
    }

    pub fn sign_out(&self) -> Result<(), TrackerError> {
        self.cloud_lock()?.sign_out()?;
        Ok(())
    }

    pub fn synchronize(&self, choice: SyncChoice) -> Result<(), TrackerError> {
        let mut client = self.cloud_lock()?;
        let preference = match choice {
            SyncChoice::Merge => Preference::Ask,
            SyncChoice::Local => Preference::Local,
            SyncChoice::Remote => Preference::Remote,
        };
        let account = client.account.as_ref().context("Войдите в Supabase.")?;
        let request = {
            let store = self.lock()?;
            let cloud = &store.state.cloud;
            if !cloud.timer.allows_account(account) || cloud.timer.is_pending() {
                return Err(
                    anyhow!("На устройстве есть несинхронизированные действия таймера.").into(),
                );
            }
            cloud.dataset_request(&store.state.dataset, preference)
        };
        self.run_sync(&mut client, request)?;
        client.schedule.refresh();
        Ok(())
    }

    pub fn sync_tick(&self) -> Result<Option<bool>, TrackerError> {
        let mut client = match self.cloud.try_lock() {
            Ok(client) => client,
            Err(TryLockError::WouldBlock) => return Ok(None),
            Err(TryLockError::Poisoned(_)) => {
                return Err(anyhow!("Перезапустите Tracking для синхронизации.").into());
            }
        };
        let Some(account) = client.account.clone() else {
            return Ok(None);
        };
        let now = Utc::now().timestamp();
        let request = {
            let mut store = self.lock()?;
            if std::mem::take(&mut store.timer_changed) {
                client.schedule.timer_changed();
            }
            if std::mem::take(&mut store.dataset_changed) {
                client.schedule.dataset_changed(now);
            }
            let conflict = client.dataset_conflict.is_some();
            client.schedule.next(
                &store.state.cloud,
                &store.state.dataset,
                &account,
                conflict,
                now,
            )
        };
        match request {
            Some(request @ CloudRequest::PollRealtime) => {
                self.run_sync(&mut client, request)?;
                Ok(None)
            }
            Some(request) => Ok(Some(self.run_sync(&mut client, request)?)),
            None => Ok(None),
        }
    }
}

impl Tracker {
    fn cloud_lock(&self) -> Result<MutexGuard<'_, Client>> {
        self.cloud
            .lock()
            .map_err(|_| anyhow!("Перезапустите Tracking для синхронизации."))
    }

    fn connection(&self) -> Result<Connection> {
        match &self.lock()?.state.cloud.setup {
            Setup::Configured(connection) => Ok(connection.clone()),
            _ => Err(anyhow!("Сначала настройте подключение к Supabase.")),
        }
    }

    fn run_sync(&self, client: &mut Client, request: CloudRequest) -> Result<bool> {
        let submitted = if let CloudRequest::Synchronize { dataset, .. } = &request {
            Some(dataset.clone())
        } else {
            None
        };
        let response = client.execute(request)?;
        let mut store = self.lock()?;
        let mut state = store.tracker_state();
        let mut cloud = store.state.cloud.clone();
        match response {
            CloudResponse::Synchronized(point) => {
                let base = submitted.context("Отсутствует исходная копия данных.")?;
                cloud.apply_dataset(&mut state, &base, *point, Utc::now())?;
                client.dataset_conflict = None;
            }
            CloudResponse::TimerSynchronized {
                account,
                snapshot,
                submitted,
            } => {
                cloud.timer.apply(&mut state, account, snapshot, submitted);
                if state.timer != store.state.timer || submitted.is_some() {
                    client.schedule.refresh();
                } else if cloud.timer.is_pending() && !cloud.timer.has_conflict() {
                    client.schedule.timer_changed();
                }
            }
            CloudResponse::Conflict(count) => {
                client.dataset_conflict = Some(count);
                return Ok(true);
            }
            CloudResponse::SessionExpired => {
                client.set_account(None);
                return Ok(true);
            }
            CloudResponse::RealtimeChanged(changed) => {
                if changed {
                    client.schedule.refresh();
                }
                return Ok(false);
            }
            _ => return Ok(false),
        }
        if cloud != store.state.cloud || state != store.tracker_state() {
            store.save(state, cloud)?;
            return Ok(true);
        }
        Ok(false)
    }
}
