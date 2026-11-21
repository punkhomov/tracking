use std::{
    env,
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    sync::{Arc, Mutex},
    time::Duration,
};

use app_platform::cloud::TimerSync;
use tracking_core::{Command, TrackerState};

use super::*;

#[derive(Clone, Default)]
struct TestStorage(Arc<Mutex<Option<Vec<u8>>>>);

impl SessionStorage for TestStorage {
    fn load(&self) -> Result<Option<SavedSession>> {
        self.0
            .lock()
            .unwrap()
            .as_deref()
            .map(serde_json::from_slice)
            .transpose()
            .map_err(Into::into)
    }

    fn save(&self, session: &SavedSession) -> Result<()> {
        *self.0.lock().unwrap() = Some(serde_json::to_vec(session)?);
        Ok(())
    }

    fn clear(&self) -> Result<()> {
        *self.0.lock().unwrap() = None;
        Ok(())
    }
}

type MockStep = (&'static str, &'static str, &'static str, serde_json::Value);

fn serve<const N: usize>(
    listener: TcpListener,
    steps: [MockStep; N],
) -> std::thread::JoinHandle<Result<()>> {
    std::thread::spawn(move || -> Result<()> {
        for (route, expected, status, body) in steps {
            let (mut stream, _) = listener.accept()?;
            stream.set_read_timeout(Some(Duration::from_secs(5)))?;
            let mut input = BufReader::new(&stream);
            let mut line = String::new();
            input.read_line(&mut line)?;
            assert!(line.starts_with(route), "Unexpected request: {line}");
            let mut length = 0;
            let mut authorization = String::new();
            loop {
                line.clear();
                ensure!(input.read_line(&mut line)? != 0, "Incomplete request");
                if line == "\r\n" {
                    break;
                }
                if let Some((name, value)) = line.split_once(':')
                    && name.eq_ignore_ascii_case("authorization")
                {
                    authorization = value.trim().into();
                }
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse()?;
                }
            }
            let mut request_body = vec![0; length];
            input.read_exact(&mut request_body)?;
            if expected.starts_with("Bearer ") {
                assert_eq!(authorization, expected);
            } else {
                assert!(std::str::from_utf8(&request_body)?.contains(expected));
            }
            let body = body.to_string();
            write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )?;
        }
        Ok(())
    })
}

#[test]
fn restores_the_valid_access_token_without_network() -> Result<()> {
    let connection = Connection::new("http://127.0.0.1:1", "sb_publishable_test")?;
    let account = Account {
        project: connection.url().into(),
        user: Uuid::new_v4(),
    };
    let storage = TestStorage::default();
    storage.save(&SavedSession {
        account: account.clone(),
        refresh_token: "synthetic-refresh".into(),
        access: Some(SavedAccess {
            token: "synthetic-access".into(),
            expires_at: Utc::now().timestamp() + 3600,
        }),
    })?;
    let mut service = Service::with_session_storage(storage);
    let response = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(service.execute(CloudRequest::RestoreSession(connection)))?;
    assert!(matches!(response, CloudResponse::SignedIn(found) if found == account));
    let (_, session) = service.signed_in.unwrap();
    assert_eq!(session.access_token, "synthetic-access");
    assert_eq!(session.refresh_token, "synthetic-refresh");
    Ok(())
}

#[test]
fn remembers_and_rotates_the_session_across_restarts() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let connection = Connection::new(
        &format!("http://{}", listener.local_addr()?),
        "sb_publishable_test",
    )?;
    let account = Account {
        project: connection.url().into(),
        user: Uuid::new_v4(),
    };
    let session = |token| {
        json!({
            "access_token": "synthetic-access",
            "refresh_token": token,
            "expires_at": 0,
            "user": {"id": account.user},
        })
    };
    let mut signed_in_again = session("second-login");
    signed_in_again["expires_at"] = json!(Utc::now().timestamp() + 3600);
    let steps = [
        (
            "POST /auth/v1/verify ",
            "123456",
            "200 OK",
            session("initial"),
        ),
        (
            "POST /auth/v1/token?",
            "initial",
            "503 Unavailable",
            json!({}),
        ),
        (
            "POST /auth/v1/token?",
            "initial",
            "200 OK",
            session("restored"),
        ),
        (
            "POST /auth/v1/token?",
            "restored",
            "200 OK",
            session("rotated"),
        ),
        (
            "GET /rest/v1/tracking_timer?",
            "",
            "500 Unavailable",
            json!({}),
        ),
        (
            "POST /auth/v1/token?",
            "rotated",
            "400 Bad Request",
            json!({"error_code": "refresh_token_not_found"}),
        ),
        ("POST /auth/v1/verify ", "123456", "200 OK", signed_in_again),
        (
            "POST /auth/v1/logout?scope=local ",
            "Bearer synthetic-access",
            "503 Unavailable",
            json!({}),
        ),
    ];
    let server = serve(listener, steps);

    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(async {
            let store = TestStorage::default();
            let verify = || CloudRequest::VerifyCode {
                connection: connection.clone(),
                email: "fixture@example.test".into(),
                code: "123456".into(),
            };
            let restore = || CloudRequest::RestoreSession(connection.clone());
            let mut service = Service::with_session_storage(store.clone());
            assert!(matches!(service.execute(restore()).await?, CloudResponse::NoSavedSession));
            assert!(matches!(service.execute(verify()).await?, CloudResponse::SignedIn(found) if found == account));
            let legacy = store.load()?.unwrap().without_cached_access();
            store.save(&legacy)?;
            drop(service);

            let mut service = Service::with_session_storage(store.clone());
            let other = Connection::new("https://other.example.test", "sb_publishable_test")?;
            assert!(matches!(service.execute(CloudRequest::RestoreSession(other)).await?, CloudResponse::NoSavedSession));
            assert!(service.execute(restore()).await.is_err());
            assert_eq!(store.load()?.unwrap().refresh_token, "initial");
            assert!(matches!(service.execute(restore()).await?, CloudResponse::SignedIn(found) if found == account));
            assert_eq!(store.load()?.unwrap().refresh_token, "restored");
            assert!(service.execute(CloudRequest::SyncTimer { account: account.clone(), update: None }).await.is_err());
            assert_eq!(store.load()?.unwrap().refresh_token, "rotated");
            drop(service);

            let mut service = Service::with_session_storage(store.clone());
            assert!(matches!(service.execute(restore()).await?, CloudResponse::SessionExpired));
            assert!(store.load()?.is_none());
            assert!(matches!(service.execute(verify()).await?, CloudResponse::SignedIn(_)));
            assert!(matches!(service.execute(CloudRequest::SignOut).await?, CloudResponse::SignedOut));
            assert!(store.load()?.is_none());
            Ok::<_, anyhow::Error>(())
        })?;
    server.join().expect("mock auth server panicked")
}

#[test]
fn rejected_access_token_refreshes_once_and_only_auth_can_end_the_session() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let connection = Connection::new(
        &format!("http://{}", listener.local_addr()?),
        "sb_publishable_test",
    )?;
    let account = Account {
        project: connection.url().into(),
        user: Uuid::new_v4(),
    };
    let expires = Utc::now().timestamp() + 3600;
    let session = |access, refresh| {
        json!({
            "access_token": access,
            "refresh_token": refresh,
            "expires_at": expires,
            "user": {"id": account.user},
        })
    };
    let get = "GET /rest/v1/tracking_timer?";
    let refresh = "POST /auth/v1/token?grant_type=refresh_token ";
    let rejected = json!({"code":"PGRST301","message":"JWT expired"});
    let server = serve(
        listener,
        [
            (
                get,
                "Bearer old-access",
                "401 Unauthorized",
                rejected.clone(),
            ),
            (
                refresh,
                "old-refresh",
                "200 OK",
                session("new-access", "new-refresh"),
            ),
            (get, "Bearer new-access", "200 OK", json!([])),
            (
                get,
                "Bearer new-access",
                "401 Unauthorized",
                json!({"message":"Invalid API key"}),
            ),
            (
                get,
                "Bearer new-access",
                "401 Unauthorized",
                rejected.clone(),
            ),
            (
                refresh,
                "new-refresh",
                "200 OK",
                session("last-access", "last-refresh"),
            ),
            (
                get,
                "Bearer last-access",
                "401 Unauthorized",
                rejected.clone(),
            ),
            (
                get,
                "Bearer last-access",
                "401 Unauthorized",
                rejected.clone(),
            ),
            (refresh, "last-refresh", "503 Unavailable", json!({})),
            (get, "Bearer last-access", "401 Unauthorized", rejected),
            (
                refresh,
                "last-refresh",
                "400 Bad Request",
                json!({"error_code":"refresh_token_not_found"}),
            ),
        ],
    );
    let store = TestStorage::default();
    store.save(&SavedSession {
        account: account.clone(),
        refresh_token: "old-refresh".into(),
        access: Some(SavedAccess {
            token: "old-access".into(),
            expires_at: expires,
        }),
    })?;
    tokio::runtime::Builder::new_current_thread().enable_all().build()?.block_on(async {
        let mut service = Service::with_session_storage(store.clone());
        let sync = || CloudRequest::SyncTimer { account: account.clone(), update: None };
        assert!(matches!(service.execute(CloudRequest::RestoreSession(connection)).await?, CloudResponse::SignedIn(_)));
        assert!(matches!(service.execute(sync()).await?, CloudResponse::TimerSynchronized { .. }));
        assert_eq!(store.load()?.unwrap().refresh_token, "new-refresh");

        assert!(service.execute(sync()).await.is_err());
        assert_eq!(store.load()?.unwrap().refresh_token, "new-refresh");
        assert!(matches!(service.execute(sync()).await, Err(error) if error.is::<AccessTokenRejected>()));
        assert_eq!(store.load()?.unwrap().refresh_token, "last-refresh");
        assert!(service.execute(sync()).await.is_err());
        assert_eq!(store.load()?.unwrap().refresh_token, "last-refresh");

        assert!(matches!(service.execute(sync()).await?, CloudResponse::SessionExpired));
        assert!(store.load()?.is_none());
        Ok::<_, anyhow::Error>(())
    })?;
    server.join().expect("mock auth server panicked")
}

#[test]
#[ignore = "requires local Supabase and a fresh synthetic account"]
fn two_devices_receive_the_timer_and_reject_a_late_finish() -> Result<()> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(two_devices())
}

async fn two_devices() -> Result<()> {
    let connection = Connection::new(
        &env::var("TRACKING_TEST_URL")?,
        &env::var("TRACKING_TEST_KEY")?,
    )?;
    let first = Supabase::new(&connection)?;
    let second = Supabase::new(&connection)?;
    let desktop = sign_in(&first).await?;
    let phone = sign_in(&second).await?;
    let account = first.account(&desktop);
    let mut changes = realtime::Subscription::default();
    wait_for_change(&mut changes, &first, &desktop).await?;

    let mut desktop_timer = TimerSync::default();
    let mut desktop_state = TrackerState::default();
    let empty = first.sync_timer(&desktop, None).await?;
    assert_eq!(empty.revision, 0);
    desktop_timer.apply(&mut desktop_state, account.clone(), empty, None);
    let now = Utc::now();
    let mut started = desktop_state.clone();
    started.execute(
        Command::StartTimer {
            note: "Test timer".into(),
            category: None,
        },
        now,
    )?;
    assert!(desktop_timer.record(&desktop_state, &started));
    let start = update(&desktop_timer, &account);
    let start_id = start.change.id;
    let running = first.sync_timer(&desktop, Some(&start)).await?;
    assert_eq!(running.revision, 1);
    assert_eq!(running.change_id, Some(start_id));
    desktop_timer.apply(&mut started, account.clone(), running, Some(start_id));
    wait_for_change(&mut changes, &first, &desktop).await?;

    let running = second.sync_timer(&phone, None).await?;
    let session_id = running.timer.as_ref().unwrap().id;
    let mut phone_timer = TimerSync::default();
    let mut phone_state = TrackerState::default();
    phone_timer.apply(&mut phone_state, account.clone(), running, None);

    let mut offline_finish = started.clone();
    offline_finish.execute(Command::FinishSession, now + chrono::TimeDelta::seconds(10))?;
    assert!(desktop_timer.record(&started, &offline_finish));
    desktop_timer = serde_json::from_str(&serde_json::to_string(&desktop_timer)?)?;

    let mut online_finish = phone_state.clone();
    online_finish.execute(Command::FinishSession, now + chrono::TimeDelta::seconds(15))?;
    assert!(phone_timer.record(&phone_state, &online_finish));
    let accepted_id = update(&phone_timer, &account).change.id;
    let expected = second
        .sync_timer(&phone, Some(&update(&phone_timer, &account)))
        .await?;
    assert_eq!(expected.revision, 2);
    assert_eq!(expected.change_id, Some(accepted_id));
    assert!(expected.timer.is_none());
    wait_for_change(&mut changes, &first, &desktop).await?;
    let replay = second
        .sync_timer(&phone, Some(&update(&phone_timer, &account)))
        .await?;
    assert!(replay == expected);

    let stale = update(&desktop_timer, &account);
    let stale_id = stale.change.id;
    let canonical = first.sync_timer(&desktop, Some(&stale)).await?;
    assert!(canonical == expected);
    desktop_timer.apply(
        &mut offline_finish,
        account.clone(),
        canonical,
        Some(stale_id),
    );
    assert!(desktop_timer.has_conflict());
    assert!(desktop_timer.is_pending());

    let data = first
        .read(&desktop)
        .await?
        .context("Missing canonical history")?
        .data;
    assert_eq!(data.periods.len(), 1);
    assert_eq!(
        data.periods[&session_id],
        online_finish.dataset.periods[&session_id]
    );
    assert_ne!(
        data.periods[&session_id],
        offline_finish.dataset.periods[&session_id]
    );
    Ok(())
}

async fn sign_in(client: &Supabase) -> Result<Session> {
    Ok(checked(
        client
            .request(reqwest::Method::POST, "auth/v1/token")
            .query(&[("grant_type", "password")])
            .json(&json!({
                "email": env::var("TRACKING_TEST_EMAIL")?,
                "password": env::var("TRACKING_TEST_PASSWORD")?,
            }))
            .send()
            .await,
    )
    .await?
    .json()
    .await?)
}

fn update(sync: &TimerSync, account: &Account) -> TimerUpdate {
    match sync.request(account) {
        CloudRequest::SyncTimer {
            update: Some(update),
            ..
        } => update,
        _ => panic!("Missing queued timer operation"),
    }
}

async fn wait_for_change(
    changes: &mut realtime::Subscription,
    client: &Supabase,
    session: &Session,
) -> Result<()> {
    for _ in 0..120 {
        if changes.poll(&client.url, &client.key, session) {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(125)).await;
    }
    bail!("Timed out waiting for the timer Realtime subscription or event");
}
