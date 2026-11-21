use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use tracking_core::{Command, Dataset, Kind, Period, Timer, TrackerState};
use url::Url;
use uuid::Uuid;

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudState {
    pub setup: Setup,
    pub last_sync: Option<SyncPoint>,
    #[serde(default)]
    pub timer: TimerSync,
}

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Setup {
    #[default]
    Undecided,
    Local,
    Configured(Connection),
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Connection {
    url: String,
    key: String,
}

impl Connection {
    pub fn new(url: &str, key: &str) -> Result<Self> {
        let url = Url::parse(url.trim()).context("Неверный URL проекта Supabase.")?;
        ensure!(
            url.scheme() == "https"
                || (url.scheme() == "http"
                    && matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))),
            "Для Supabase укажите HTTPS URL. HTTP доступен для локального проекта."
        );
        ensure!(
            url.path() == "/"
                && url.query().is_none()
                && url.fragment().is_none()
                && url.username().is_empty()
                && url.password().is_none(),
            "Укажите URL проекта без пути и параметров."
        );
        let key = key.trim();
        let publishable = key.strip_prefix("sb_publishable_").is_some_and(|suffix| {
            !suffix.is_empty()
                && suffix
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        });
        let legacy_anon = || {
            let mut segments = key.split('.');
            let (Some(header), Some(payload), Some(signature), None) = (
                segments.next(),
                segments.next(),
                segments.next(),
                segments.next(),
            ) else {
                return false;
            };
            !header.is_empty()
                && !signature.is_empty()
                && key
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
                && URL_SAFE_NO_PAD.decode(payload).ok().is_some_and(|decoded| {
                    serde_json::from_slice::<serde_json::Value>(&decoded)
                        .is_ok_and(|claims| claims["role"] == "anon")
                })
        };
        ensure!(
            key.len() <= 2048 && (publishable || legacy_anon()),
            "Укажите ключ sb_publishable_… или публичный anon. Секретные ключи здесь недопустимы."
        );
        Ok(Self {
            url: url.to_string(),
            key: key.into(),
        })
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn key(&self) -> &str {
        &self.key
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    pub project: String,
    pub user: Uuid,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncPoint {
    pub account: Account,
    pub dataset: Dataset,
}

pub use tracking_core::merge::Preference;

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimerSync {
    account: Option<Account>,
    revision: i64,
    pending: std::collections::VecDeque<TimerChange>,
    conflict: Option<TimerSnapshot>,
}

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimerSnapshot {
    pub revision: i64,
    pub change_id: Option<Uuid>,
    pub timer: Option<Timer>,
    pub next_kind: Kind,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimerChange {
    pub id: Uuid,
    pub timer: Option<Timer>,
    pub next_kind: Kind,
    pub finished: Option<(Uuid, Period)>,
}

impl TimerSync {
    pub fn record(&mut self, before: &TrackerState, after: &TrackerState) -> bool {
        if self.account.is_none() || before.timer == after.timer {
            return false;
        }
        let finished = before.timer.as_ref().and_then(|timer| {
            after.dataset.periods.get(&timer.id).and_then(|period| {
                (before.dataset.periods.get(&timer.id) != Some(period))
                    .then(|| (timer.id, period.clone()))
            })
        });
        self.enqueue(after, finished);
        true
    }

    fn enqueue(&mut self, after: &TrackerState, finished: Option<(Uuid, Period)>) {
        self.pending.push_back(TimerChange {
            id: Uuid::new_v4(),
            timer: after.timer.clone(),
            next_kind: after.next_kind,
            finished,
        });
    }

    pub fn is_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    pub fn has_conflict(&self) -> bool {
        self.conflict.is_some()
    }

    pub fn allows_account(&self, account: &Account) -> bool {
        !self.is_pending() || self.account.as_ref() == Some(account)
    }

    pub fn request(&self, account: &Account) -> CloudRequest {
        CloudRequest::SyncTimer {
            account: account.clone(),
            update: self.pending.front().map(|change| TimerUpdate {
                change: Box::new(change.clone()),
                revision: self.revision,
            }),
        }
    }

    pub fn apply(
        &mut self,
        local: &mut TrackerState,
        account: Account,
        snapshot: TimerSnapshot,
        submitted: Option<Uuid>,
    ) {
        if !self.allows_account(&account) {
            return;
        }
        let binding = self.account.as_ref() != Some(&account);
        if binding {
            self.account = Some(account);
            self.revision = snapshot.revision;
            if local.timer.is_some() && local.timer != snapshot.timer {
                self.enqueue(local, None);
                if snapshot.timer.is_some() {
                    self.conflict = Some(snapshot);
                }
                return;
            }
        }
        if let Some(id) = submitted {
            if snapshot.change_id != Some(id) {
                self.conflict = Some(snapshot);
                return;
            }
            if self.pending.front().is_some_and(|change| change.id == id) {
                self.pending.pop_front();
            }
        } else if self.is_pending() && self.revision != snapshot.revision {
            self.conflict = Some(snapshot);
            return;
        }
        if self.pending.is_empty() {
            let incoming = self.revision != snapshot.revision
                || local.timer != snapshot.timer
                || (binding && snapshot.revision > 0);
            local.timer = snapshot.timer;
            if incoming {
                local.next_kind = snapshot.next_kind;
            }
        }
        self.revision = snapshot.revision;
    }
}

pub struct TimerUpdate {
    pub revision: i64,
    pub change: Box<TimerChange>,
}

pub enum CloudRequest {
    RequestCode {
        connection: Connection,
        email: String,
    },
    VerifyCode {
        connection: Connection,
        email: String,
        code: String,
    },
    RestoreSession(Connection),
    Synchronize {
        dataset: Box<Dataset>,
        baseline: Option<Box<SyncPoint>>,
        preference: Preference,
    },
    SyncTimer {
        account: Account,
        update: Option<TimerUpdate>,
    },
    PollRealtime,
    SignOut,
}

pub enum CloudResponse {
    CodeSent,
    SignedIn(Account),
    NoSavedSession,
    Synchronized(Box<SyncPoint>),
    Conflict(usize),
    TimerSynchronized {
        account: Account,
        snapshot: TimerSnapshot,
        submitted: Option<Uuid>,
    },
    RealtimeChanged(bool),
    SignedOut,
    SessionExpired,
}

impl CloudState {
    pub fn record_timer(&mut self, before: &TrackerState, after: &TrackerState) -> bool {
        let Setup::Configured(connection) = &self.setup else {
            return false;
        };
        self.timer
            .account
            .as_ref()
            .is_some_and(|account| account.project == connection.url())
            && self.timer.record(before, after)
    }

    pub fn matches_account(&self, account: &Account) -> bool {
        self.last_sync
            .as_ref()
            .is_none_or(|point| point.account == *account)
            && self.timer.allows_account(account)
    }

    pub fn dataset_request(&self, dataset: &Dataset, preference: Preference) -> CloudRequest {
        CloudRequest::Synchronize {
            dataset: Box::new(dataset.clone()),
            baseline: self.last_sync.clone().map(Box::new),
            preference,
        }
    }

    pub fn apply_dataset(
        &mut self,
        state: &mut TrackerState,
        submitted: &Dataset,
        point: SyncPoint,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<()> {
        let merged = if state.dataset == *submitted {
            point.dataset.clone()
        } else {
            tracking_core::merge::merge(
                submitted,
                &state.dataset,
                &point.dataset,
                Preference::Local,
            )?
        };
        state.execute(Command::ApplySyncedDataset(Box::new(merged)), now)?;
        self.last_sync = Some(point);
        Ok(())
    }
}

#[derive(Default)]
pub struct SyncSchedule {
    timer_at: i64,
    dataset_at: i64,
    realtime_at: i64,
}

impl SyncSchedule {
    pub fn timer_changed(&mut self) {
        self.timer_at = 0;
    }

    pub fn dataset_changed(&mut self, now: i64) {
        self.dataset_at = self.dataset_at.min(now + 1);
    }

    pub fn refresh(&mut self) {
        self.timer_at = 0;
        self.dataset_at = 0;
    }

    pub fn next(
        &mut self,
        cloud: &CloudState,
        dataset: &Dataset,
        account: &Account,
        dataset_conflict: bool,
        now: i64,
    ) -> Option<CloudRequest> {
        if !cloud.matches_account(account) || cloud.timer.has_conflict() {
            return None;
        }
        if now >= self.timer_at {
            self.timer_at = now + 20;
            return Some(cloud.timer.request(account));
        }
        if !cloud.timer.is_pending() && !dataset_conflict && now >= self.dataset_at {
            self.dataset_at = now + 30;
            return Some(cloud.dataset_request(dataset, Preference::Ask));
        }
        if now >= self.realtime_at {
            self.realtime_at = now + 1;
            return Some(CloudRequest::PollRealtime);
        }
        None
    }
}

pub type CloudComplete = Box<dyn FnOnce(Result<CloudResponse>) + Send>;

#[cfg(test)]
mod tests {
    use chrono::{TimeDelta, Utc};
    use tracking_core::Category;

    use super::*;

    #[test]
    fn accepts_only_public_keys_and_project_origins() {
        let url = "https://example.supabase.co";
        let connection = Connection::new(url, " sb_publishable_test-key ").unwrap();
        assert_eq!(connection.url(), "https://example.supabase.co/");
        assert_eq!(connection.key(), "sb_publishable_test-key");
        let legacy_key = |role| {
            let claims = serde_json::json!({ "role": role });
            format!(
                "header.{}.signature",
                URL_SAFE_NO_PAD.encode(claims.to_string())
            )
        };
        assert!(Connection::new(url, &legacy_key("anon")).is_ok());
        assert!(Connection::new(url, &legacy_key("service_role")).is_err());
        assert!(Connection::new(url, "sb_secret_test").is_err());
        assert!(Connection::new("https://example.test/rest/v1", connection.key()).is_err());
        assert!(Connection::new("http://example.test", connection.key()).is_err());
        assert!(Connection::new("http://127.0.0.1:54321", connection.key()).is_ok());
    }

    #[test]
    fn remote_timer_change_cannot_rebase_an_offline_command() {
        let now = chrono::DateTime::from_timestamp(1_805_000_000, 0).unwrap();
        let account = Account {
            project: "https://example.supabase.co/".into(),
            user: Uuid::new_v4(),
        };
        let timer = Timer::start(Kind::Work, now, "Работа".into(), None);
        let mut sync = TimerSync::default();
        let mut state = TrackerState::default();
        sync.apply(
            &mut state,
            account.clone(),
            TimerSnapshot {
                revision: 3,
                timer: Some(timer.clone()),
                ..TimerSnapshot::default()
            },
            None,
        );
        let mut finished = state.clone();
        finished
            .execute(Command::FinishSession, now + TimeDelta::seconds(30))
            .unwrap();
        assert!(sync.record(&state, &finished));
        let mut paused = timer;
        paused.pause(now + TimeDelta::seconds(25));

        sync.apply(
            &mut finished,
            account.clone(),
            TimerSnapshot {
                revision: 4,
                timer: Some(paused),
                ..TimerSnapshot::default()
            },
            None,
        );
        let restored: TimerSync =
            serde_json::from_str(&serde_json::to_string(&sync).unwrap()).unwrap();
        assert!(restored.has_conflict());
        assert!(restored.is_pending());
        assert!(finished.timer.is_none());
        assert_eq!(finished.dataset.periods.len(), 1);
        let CloudRequest::SyncTimer {
            update: Some(update),
            ..
        } = restored.request(&account)
        else {
            panic!("missing pending finish");
        };
        assert_eq!(update.revision, 3);
        assert!(update.change.finished.is_some());
    }

    #[test]
    fn applying_a_sync_keeps_a_local_edit_made_during_the_request() {
        let submitted = Dataset::default();
        let mut state = TrackerState::default();
        let local = Uuid::new_v4();
        let remote = Uuid::new_v4();
        let category = Category {
            name: "Работа".into(),
            color: "#1f5c4b".into(),
            archived: false,
        };
        state.dataset.categories.insert(local, category.clone());
        let mut cloud = CloudState::default();
        let mut dataset = Dataset::default();
        dataset.categories.insert(remote, category);
        let point = SyncPoint {
            account: Account {
                project: "https://example.supabase.co/".into(),
                user: Uuid::new_v4(),
            },
            dataset,
        };
        cloud
            .apply_dataset(&mut state, &submitted, point, Utc::now())
            .unwrap();
        assert_eq!(state.dataset.categories.len(), 2);
        assert!(
            !cloud
                .last_sync
                .unwrap()
                .dataset
                .categories
                .contains_key(&local)
        );
    }
}
