use std::{env, time::Duration};

use app_platform::cloud::TimerSync;
use tracking_core::{Command, TrackerState};

use super::*;

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
    let mut desktop = sign_in(&first).await?;
    let mut phone = sign_in(&second).await?;
    let account = first.account(&desktop);
    let mut changes = realtime::Subscription::default();
    wait_for_change(&mut changes, &first, &desktop).await?;

    let mut desktop_timer = TimerSync::default();
    let mut desktop_state = TrackerState::default();
    let empty = first.sync_timer(&mut desktop, None).await?;
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
    let running = first.sync_timer(&mut desktop, Some(start)).await?;
    assert_eq!(running.revision, 1);
    assert_eq!(running.change_id, Some(start_id));
    desktop_timer.apply(&mut started, account.clone(), running, Some(start_id));
    wait_for_change(&mut changes, &first, &desktop).await?;

    let running = second.sync_timer(&mut phone, None).await?;
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
        .sync_timer(&mut phone, Some(update(&phone_timer, &account)))
        .await?;
    assert_eq!(expected.revision, 2);
    assert_eq!(expected.change_id, Some(accepted_id));
    assert!(expected.timer.is_none());
    wait_for_change(&mut changes, &first, &desktop).await?;
    let replay = second
        .sync_timer(&mut phone, Some(update(&phone_timer, &account)))
        .await?;
    assert!(replay == expected);

    let stale = update(&desktop_timer, &account);
    let stale_id = stale.change.id;
    let canonical = first.sync_timer(&mut desktop, Some(stale)).await?;
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
