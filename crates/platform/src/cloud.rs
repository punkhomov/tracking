use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use tracking_core::Dataset;
use url::Url;
use uuid::Uuid;

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct CloudState {
    pub setup: Setup,
    pub last_sync: Option<SyncPoint>,
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

#[derive(Clone, Serialize, Deserialize)]
pub struct SyncPoint {
    pub account: Account,
    pub dataset: Dataset,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Preference {
    Ask,
    Local,
    Remote,
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
    Synchronize {
        dataset: Box<Dataset>,
        baseline: Option<Box<SyncPoint>>,
        preference: Preference,
    },
    SignOut,
}

pub enum CloudResponse {
    CodeSent,
    SignedIn(Account),
    Synchronized(Box<SyncPoint>),
    Conflict(usize),
    SignedOut,
    SessionExpired,
}

pub type CloudComplete = Box<dyn FnOnce(Result<CloudResponse>) + Send>;

#[cfg(test)]
mod tests {
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
}
