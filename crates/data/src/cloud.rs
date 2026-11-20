use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use chrono::Utc;
use reqwest::{
    StatusCode, Url,
    blocking::{Client, RequestBuilder, Response},
};
use serde::Deserialize;
use serde_json::json;
use tracking_core::Dataset;
use uuid::Uuid;

use crate::{Account, Preference, merge};

#[derive(Clone)]
pub struct Supabase {
    client: Client,
    url: Url,
    key: String,
}

#[derive(Deserialize)]
pub struct Session {
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
    pub fn new(url: &str, key: String) -> Result<Self> {
        let mut url = Url::parse(url).context("Неверный URL проекта Supabase.")?;
        ensure!(
            url.scheme() == "https"
                || (url.scheme() == "http"
                    && matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))),
            "Для Supabase укажите HTTPS URL."
        );
        ensure!(
            url.path() == "/"
                && url.query().is_none()
                && url.fragment().is_none()
                && url.username().is_empty()
                && url.password().is_none(),
            "Укажите URL проекта без пути."
        );
        url.set_path("/");
        ensure!(
            !key.trim().is_empty(),
            "Отсутствует публичный ключ Supabase."
        );
        let client = Client::builder()
            .timeout(Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Self { client, url, key })
    }

    pub fn account(&self, session: &Session) -> Account {
        Account {
            project: self.url.to_string(),
            user: session.user.id,
        }
    }

    fn request(&self, method: reqwest::Method, path: &str) -> RequestBuilder {
        self.client
            .request(method, self.url.join(path).expect("fixed Supabase route"))
            .header("apikey", &self.key)
    }

    pub fn request_code(&self, email: &str) -> Result<()> {
        ensure!(!email.is_empty(), "Введите email.");
        checked(
            self.request(reqwest::Method::POST, "auth/v1/otp")
                .json(&json!({ "email": email, "create_user": true }))
                .send(),
        )?;
        Ok(())
    }

    pub fn verify_code(&self, email: &str, code: &str) -> Result<Session> {
        ensure!(
            !email.is_empty() && !code.is_empty(),
            "Введите email и код из письма."
        );
        Ok(checked(
            self.request(reqwest::Method::POST, "auth/v1/verify")
                .json(&json!({ "email": email, "token": code, "type": "email" }))
                .send(),
        )?
        .json()?)
    }

    fn refresh(&self, session: &mut Session) -> Result<()> {
        if session.expires_at > Utc::now().timestamp() + 60 {
            return Ok(());
        }
        let response = self
            .request(reqwest::Method::POST, "auth/v1/token")
            .query(&[("grant_type", "refresh_token")])
            .json(&json!({ "refresh_token": session.refresh_token }))
            .send();
        let next: Session = checked(response)?.json()?;
        ensure!(
            next.user.id == session.user.id,
            "Изменился аккаунт Supabase."
        );
        *session = next;
        Ok(())
    }

    pub fn synchronize(
        &self,
        session: &mut Session,
        local: &Dataset,
        baseline: Option<&Dataset>,
        preference: Preference,
    ) -> Result<Dataset> {
        self.refresh(session)?;
        let empty = Dataset::default();
        let base = baseline.unwrap_or(&empty);
        for _ in 0..3 {
            let remote = self.read(session)?;
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
            if self.write(session, remote.as_ref(), &merged)? {
                return Ok(merged);
            }
        }
        bail!("Облачная копия обновилась во время синхронизации. Повторите попытку.")
    }

    fn read(&self, session: &Session) -> Result<Option<CloudRow>> {
        let rows: Vec<CloudRow> = checked(
            self.request(reqwest::Method::GET, "rest/v1/tracking_state")
                .bearer_auth(&session.access_token)
                .query(&[("select", "revision,format,data"), ("limit", "1")])
                .send(),
        )?
        .json()?;
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

    fn write(
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
                .query(&[("revision", format!("eq.{}", row.revision))])
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
                .send(),
        )?
        .json()?;
        Ok(rows.len() == 1)
    }
}

fn checked(response: reqwest::Result<Response>) -> Result<Response> {
    let response = response.context("Не удалось связаться с Supabase.")?;
    match response.status() {
        status if status.is_success() => Ok(response),
        StatusCode::UNAUTHORIZED => bail!("Сессия Supabase истекла. Войдите заново."),
        StatusCode::TOO_MANY_REQUESTS => {
            bail!("Supabase ограничил частоту запросов. Повторите позже.")
        }
        status => {
            // Read the response so setup and authorization errors include the server's explanation.
            let body = response.text().unwrap_or_default();
            let json = serde_json::from_str::<serde_json::Value>(&body).unwrap_or_default();
            let detail = ["message", "msg", "error_description"]
                .into_iter()
                .find_map(|key| json[key].as_str())
                .unwrap_or(body.trim());
            let detail: String = detail.chars().take(300).collect();
            bail!("Supabase HTTP {status}. {detail}")
        }
    }
}
