mod realtime;
#[cfg(test)]
mod tests;

use std::{fmt, time::Duration};

use anyhow::{Context, Result, bail, ensure};
use app_platform::cloud::{
    Account, CloudRequest, CloudResponse, Connection, Preference, SyncPoint, TimerSnapshot,
    TimerUpdate,
};
use chrono::Utc;
use reqwest::{Client, RequestBuilder, Response, StatusCode, Url};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracking_core::{
    Dataset,
    merge::{Conflict, merge},
};
use uuid::Uuid;

#[derive(Default)]
pub struct Service {
    signed_in: Option<(Supabase, Session)>,
    realtime: realtime::Subscription,
    storage: Option<Box<dyn SessionStorage>>,
}

#[derive(Serialize, Deserialize)]
pub struct SavedSession {
    account: Account,
    refresh_token: String,
}

pub trait SessionStorage: Send {
    fn load(&self) -> Result<Option<SavedSession>>;
    fn save(&self, session: &SavedSession) -> Result<()>;
    fn clear(&self) -> Result<()>;
}

impl Service {
    pub fn with_session_storage(storage: impl SessionStorage + 'static) -> Self {
        Self {
            storage: Some(Box::new(storage)),
            ..Self::default()
        }
    }

    async fn restore_session(&mut self, connection: &Connection) -> Result<CloudResponse> {
        let Some(storage) = &self.storage else {
            return Ok(CloudResponse::NoSavedSession);
        };
        let Some(saved) = storage.load()? else {
            return Ok(CloudResponse::NoSavedSession);
        };
        if saved.account.project != connection.url() {
            return Ok(CloudResponse::NoSavedSession);
        }
        let client = Supabase::new(connection)?;
        match client.exchange_refresh_token(&saved.refresh_token).await {
            Ok(session) => {
                ensure!(
                    client.account(&session) == saved.account,
                    "Изменился аккаунт Supabase."
                );
                self.sign_in(client, session)
            }
            Err(error) if error.is::<AuthRequired>() => {
                self.sign_out()?;
                Ok(CloudResponse::SessionExpired)
            }
            Err(error) => Err(error),
        }
    }

    fn sign_in(&mut self, client: Supabase, session: Session) -> Result<CloudResponse> {
        client.save_session(&session, self.storage.as_deref())?;
        let account = client.account(&session);
        self.realtime = Default::default();
        self.signed_in = Some((client, session));
        Ok(CloudResponse::SignedIn(account))
    }

    fn sign_out(&mut self) -> Result<()> {
        if let Some(storage) = &self.storage {
            storage.clear()?;
        }
        self.signed_in = None;
        self.realtime = Default::default();
        Ok(())
    }

    pub async fn execute(&mut self, request: CloudRequest) -> Result<CloudResponse> {
        match request {
            CloudRequest::RequestCode { connection, email } => {
                Supabase::new(&connection)?.request_code(&email).await?;
                Ok(CloudResponse::CodeSent)
            }
            CloudRequest::VerifyCode {
                connection,
                email,
                code,
            } => {
                let client = Supabase::new(&connection)?;
                let session = client.verify_code(&email, &code).await?;
                self.sign_in(client, session)
            }
            CloudRequest::RestoreSession(connection) => self.restore_session(&connection).await,
            CloudRequest::Synchronize {
                dataset,
                baseline,
                preference,
            } => {
                let Some((client, session)) = &mut self.signed_in else {
                    return Ok(CloudResponse::SessionExpired);
                };
                let account = client.account(session);
                let baseline = baseline
                    .as_deref()
                    .filter(|point| point.account == account)
                    .map(|point| &point.dataset);
                match client
                    .synchronize(
                        session,
                        &dataset,
                        baseline,
                        preference,
                        self.storage.as_deref(),
                    )
                    .await
                {
                    Ok(dataset) => Ok(CloudResponse::Synchronized(Box::new(SyncPoint {
                        account,
                        dataset,
                    }))),
                    Err(error) if error.is::<AuthRequired>() => {
                        self.sign_out()?;
                        Ok(CloudResponse::SessionExpired)
                    }
                    Err(error) => match error.downcast_ref::<Conflict>() {
                        Some(conflict) => Ok(CloudResponse::Conflict(conflict.count)),
                        None => Err(error),
                    },
                }
            }
            CloudRequest::SyncTimer { account, update } => {
                let Some((client, session)) = &mut self.signed_in else {
                    return Ok(CloudResponse::SessionExpired);
                };
                if client.account(session) != account {
                    return Ok(CloudResponse::SessionExpired);
                }
                let submitted = update.as_ref().map(|update| update.change.id);
                match client
                    .sync_timer(session, update, self.storage.as_deref())
                    .await
                {
                    Ok(snapshot) => Ok(CloudResponse::TimerSynchronized {
                        account,
                        snapshot,
                        submitted,
                    }),
                    Err(error) if error.is::<AuthRequired>() => {
                        self.sign_out()?;
                        Ok(CloudResponse::SessionExpired)
                    }
                    Err(error) => Err(error),
                }
            }
            CloudRequest::PollRealtime => {
                let changed = self.signed_in.as_ref().is_some_and(|(client, session)| {
                    self.realtime.poll(&client.url, &client.key, session)
                });
                Ok(CloudResponse::RealtimeChanged(changed))
            }
            CloudRequest::SignOut => {
                self.sign_out()?;
                Ok(CloudResponse::SignedOut)
            }
        }
    }
}

struct Supabase {
    client: Client,
    url: Url,
    key: String,
}

#[derive(Deserialize)]
struct Session {
    access_token: String,
    refresh_token: String,
    expires_at: i64,
    user: User,
}

#[derive(Deserialize)]
struct User {
    id: Uuid,
}

#[derive(Deserialize)]
struct CloudRow {
    revision: i64,
    format: u32,
    data: Dataset,
}

impl Supabase {
    fn new(connection: &Connection) -> Result<Self> {
        let connection = Connection::new(connection.url(), connection.key())?;
        let url = Url::parse(connection.url())?;
        #[cfg(not(target_arch = "wasm32"))]
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        #[cfg(target_arch = "wasm32")]
        let client = Client::new();
        Ok(Self {
            client,
            url,
            key: connection.key().to_owned(),
        })
    }

    fn account(&self, session: &Session) -> Account {
        Account {
            project: self.url.to_string(),
            user: session.user.id,
        }
    }

    fn request(&self, method: reqwest::Method, path: &str) -> RequestBuilder {
        let request = self
            .client
            .request(method, self.url.join(path).expect("fixed Supabase route"))
            .header("apikey", &self.key)
            .timeout(Duration::from_secs(15));
        #[cfg(target_arch = "wasm32")]
        let request = request.fetch_credentials_omit().fetch_cache_no_store();
        request
    }

    async fn request_code(&self, email: &str) -> Result<()> {
        let email = email.trim();
        ensure!(
            email
                .split_once('@')
                .is_some_and(|(name, host)| !name.is_empty() && !host.is_empty()),
            "Введите email."
        );
        checked(
            self.request(reqwest::Method::POST, "auth/v1/otp")
                .json(&json!({ "email": email, "create_user": false }))
                .send()
                .await,
        )
        .await?;
        Ok(())
    }

    async fn verify_code(&self, email: &str, code: &str) -> Result<Session> {
        let code = code.trim();
        ensure!(
            (6..=10).contains(&code.len()) && code.bytes().all(|byte| byte.is_ascii_digit()),
            "Введите цифры из письма."
        );
        Ok(checked(
            self.request(reqwest::Method::POST, "auth/v1/verify")
                .json(&json!({ "email": email.trim(), "token": code, "type": "email" }))
                .send()
                .await,
        )
        .await?
        .json()
        .await?)
    }

    async fn refresh(
        &self,
        session: &mut Session,
        storage: Option<&dyn SessionStorage>,
    ) -> Result<()> {
        if session.expires_at > Utc::now().timestamp() + 60 {
            return Ok(());
        }
        let next = self.exchange_refresh_token(&session.refresh_token).await?;
        ensure!(
            next.user.id == session.user.id,
            "Изменился аккаунт Supabase."
        );
        self.save_session(&next, storage)?;
        *session = next;
        Ok(())
    }

    fn save_session(&self, session: &Session, storage: Option<&dyn SessionStorage>) -> Result<()> {
        if let Some(storage) = storage {
            storage.save(&SavedSession {
                account: self.account(session),
                refresh_token: session.refresh_token.clone(),
            })?;
        }
        Ok(())
    }

    async fn exchange_refresh_token(&self, refresh_token: &str) -> Result<Session> {
        let response = self
            .request(reqwest::Method::POST, "auth/v1/token")
            .query(&[("grant_type", "refresh_token")])
            .json(&json!({ "refresh_token": refresh_token }))
            .send()
            .await
            .context("Не удалось связаться с Supabase для восстановления входа.")?;
        if response.status().is_success() {
            return response
                .json()
                .await
                .map_err(|_| anyhow::anyhow!("Неверный ответ Supabase при обновлении сессии."));
        }
        let status = response.status();
        let body: serde_json::Value = response.json().await.unwrap_or_default();
        if matches!(
            body["error_code"].as_str(),
            Some(
                "refresh_token_not_found"
                    | "refresh_token_already_used"
                    | "session_not_found"
                    | "session_expired"
                    | "user_not_found"
                    | "user_banned"
            )
        ) {
            return Err(AuthRequired.into());
        }
        bail!("Не удалось обновить сессию Supabase (HTTP {status}).")
    }

    async fn synchronize(
        &self,
        session: &mut Session,
        local: &Dataset,
        baseline: Option<&Dataset>,
        preference: Preference,
        storage: Option<&dyn SessionStorage>,
    ) -> Result<Dataset> {
        self.refresh(session, storage).await?;
        let empty = Dataset::default();
        let base = baseline.unwrap_or(&empty);
        for _ in 0..3 {
            let remote = self.read(session).await?;
            ensure!(
                baseline.is_none() || remote.is_some(),
                "Облачная копия была удалена. Локальные данные сохранены."
            );
            let remote_data = remote.as_ref().map_or(&empty, |row| &row.data);
            let merged = merge(base, local, remote_data, preference)?;
            merged.validate()?;
            if merged == *remote_data && remote.is_some() {
                return Ok(merged);
            }
            if self.write(session, remote.as_ref(), &merged).await? {
                return Ok(merged);
            }
        }
        bail!("Облачная копия обновилась во время синхронизации. Повторите попытку.")
    }

    async fn sync_timer(
        &self,
        session: &mut Session,
        update: Option<TimerUpdate>,
        storage: Option<&dyn SessionStorage>,
    ) -> Result<TimerSnapshot> {
        self.refresh(session, storage).await?;
        let request = if let Some(update) = &update {
            let change = &update.change;
            self.request(reqwest::Method::POST, "rest/v1/rpc/tracking_apply_timer")
                .json(&json!({
                    "expected_revision": update.revision,
                    "operation_id": change.id,
                    "next_timer": change.timer,
                    "selected_kind": change.next_kind,
                    "finished_id": change.finished.as_ref().map(|(id, _)| id),
                    "finished_period": change.finished.as_ref().map(|(_, period)| period),
                }))
        } else {
            self.request(reqwest::Method::GET, "rest/v1/tracking_timer")
                .query(&[
                    ("select", "revision,change_id,timer,next_kind"),
                    ("user_id", &format!("eq.{}", session.user.id)),
                ])
        };
        let rows: Vec<TimerSnapshot> =
            checked(request.bearer_auth(&session.access_token).send().await)
                .await?
                .json()
                .await?;
        let mut rows = rows.into_iter();
        let snapshot = rows.next();
        ensure!(
            rows.next().is_none() && (update.is_none() || snapshot.is_some()),
            "Некорректный ответ Supabase о таймере."
        );
        Ok(snapshot.unwrap_or_default())
    }

    async fn read(&self, session: &Session) -> Result<Option<CloudRow>> {
        let owner = format!("eq.{}", session.user.id);
        let rows: Vec<CloudRow> = checked(
            self.request(reqwest::Method::GET, "rest/v1/tracking_state")
                .bearer_auth(&session.access_token)
                .query(&[("select", "revision,format,data"), ("user_id", &owner)])
                .send()
                .await,
        )
        .await?
        .json()
        .await?;
        let row = rows.into_iter().next();
        if let Some(row) = &row {
            ensure!(
                row.format == 1 && row.revision > 0,
                "Неизвестная версия облачных данных."
            );
            row.data.validate()?;
        }
        Ok(row)
    }

    async fn write(
        &self,
        session: &Session,
        previous: Option<&CloudRow>,
        merged: &Dataset,
    ) -> Result<bool> {
        let request = if let Some(row) = previous {
            let revision = row
                .revision
                .checked_add(1)
                .context("Исчерпаны версии облачной копии.")?;
            self.request(reqwest::Method::PATCH, "rest/v1/tracking_state")
                .query(&[
                    ("revision", format!("eq.{}", row.revision)),
                    ("user_id", format!("eq.{}", session.user.id)),
                ])
                .header("Prefer", "return=representation")
                .json(&json!({ "revision": revision, "data": merged }))
        } else {
            self.request(reqwest::Method::POST, "rest/v1/tracking_state")
                .query(&[("on_conflict", "user_id")])
                .header(
                    "Prefer",
                    "return=representation,resolution=ignore-duplicates",
                )
                .json(&json!({ "revision": 1, "format": 1, "data": merged }))
        };
        let rows: Vec<serde_json::Value> = checked(
            request
                .query(&[("select", "revision")])
                .bearer_auth(&session.access_token)
                .send()
                .await,
        )
        .await?
        .json()
        .await?;
        Ok(rows.len() == 1)
    }
}

#[derive(Debug)]
struct AuthRequired;

impl fmt::Display for AuthRequired {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(
            "Supabase отклонил ключ или сессию. Проверьте данные подключения и войдите снова.",
        )
    }
}

impl std::error::Error for AuthRequired {}

async fn checked(response: reqwest::Result<Response>) -> Result<Response> {
    let response = response.context("Не удалось связаться с Supabase.")?;
    match response.status() {
        status if status.is_success() => Ok(response),
        StatusCode::UNAUTHORIZED => Err(AuthRequired.into()),
        StatusCode::TOO_MANY_REQUESTS => {
            bail!("Supabase ограничил частоту запросов. Повторите позже.")
        }
        status => {
            // Read the response so setup and authorization errors include the server's explanation.
            let body = response.text().await.unwrap_or_default();
            let json = serde_json::from_str::<serde_json::Value>(&body).unwrap_or_default();
            if matches!(
                json["error_code"].as_str(),
                Some(
                    "refresh_token_not_found"
                        | "refresh_token_already_used"
                        | "session_not_found"
                        | "session_expired"
                )
            ) {
                return Err(AuthRequired.into());
            }
            let detail = ["message", "msg", "error_description"]
                .into_iter()
                .find_map(|key| json[key].as_str())
                .unwrap_or(body.trim());
            let detail: String = detail.chars().take(300).collect();
            bail!("Supabase HTTP {status}. {detail}")
        }
    }
}
