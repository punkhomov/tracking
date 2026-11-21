use chrono::Utc;
use ewebsock::{Options, WsEvent, WsMessage, WsReceiver, WsSender};
use reqwest::Url;
use serde_json::{Value, json};

use crate::Session;

const TOPIC: &str = "realtime:tracking_timer";

#[derive(Default)]
pub(crate) struct Subscription {
    socket: Option<Socket>,
    retry_after: i64,
}

struct Socket {
    sender: WsSender,
    receiver: WsReceiver,
    token: String,
    last_reply: i64,
    heartbeat: i64,
    joined: bool,
}

impl Subscription {
    pub(crate) fn poll(&mut self, project: &Url, key: &str, session: &Session) -> bool {
        let now = Utc::now().timestamp();
        if self
            .socket
            .as_ref()
            .is_some_and(|socket| socket.token != session.access_token)
        {
            self.socket = None;
            self.retry_after = now;
        }
        if self.socket.is_none() && now >= self.retry_after {
            self.retry_after = now + 10;
            self.socket = Socket::connect(project, key, session, now);
        }
        let Some(socket) = &mut self.socket else {
            return false;
        };
        match socket.poll(session, now) {
            Some(changed) => changed,
            None => {
                self.socket = None;
                self.retry_after = now + 10;
                false
            }
        }
    }
}

impl Socket {
    fn connect(project: &Url, key: &str, session: &Session, now: i64) -> Option<Self> {
        let mut url = project.join("realtime/v1/websocket").ok()?;
        url.set_scheme(if project.scheme() == "https" {
            "wss"
        } else {
            "ws"
        })
        .ok()?;
        url.query_pairs_mut()
            .append_pair("apikey", key)
            .append_pair("vsn", "1.0.0");
        let (sender, receiver) = ewebsock::connect(
            url.to_string(),
            Options {
                max_incoming_frame_size: 256 * 1024,
                ..Options::default()
            },
        )
        .ok()?;
        Some(Self {
            sender,
            receiver,
            token: session.access_token.clone(),
            last_reply: now,
            heartbeat: now,
            joined: false,
        })
    }

    fn poll(&mut self, session: &Session, now: i64) -> Option<bool> {
        let mut changed = false;
        for _ in 0..64 {
            match self.receiver.try_recv() {
                None => break,
                Some(WsEvent::Opened) => self.send(json!({
                    "topic": TOPIC,
                    "event": "phx_join",
                    "ref": "join",
                    "payload": {
                        "config": {
                            "broadcast": { "ack": false, "self": false },
                            "presence": { "enabled": false, "key": "" },
                            "postgres_changes": [
                                {
                                    "event": "INSERT", "schema": "public", "table": "tracking_timer",
                                    "filter": format!("user_id=eq.{}", session.user.id),
                                },
                                {
                                    "event": "UPDATE", "schema": "public", "table": "tracking_timer",
                                    "filter": format!("user_id=eq.{}", session.user.id),
                                }
                            ],
                            "private": false,
                        },
                        "access_token": self.token,
                    },
                })),
                Some(WsEvent::Closed | WsEvent::Error(_)) => return None,
                Some(WsEvent::Message(WsMessage::Text(text))) => {
                    let Ok(event) = serde_json::from_str::<Value>(&text) else {
                        continue;
                    };
                    match event["event"].as_str() {
                        Some("phx_reply") => {
                            self.last_reply = now;
                            if event["payload"]["status"] != "ok" {
                                return None;
                            }
                            if event["ref"] == "join" {
                                self.joined = true;
                            }
                        }
                        Some("system") if event["payload"]["extension"] == "postgres_changes" => {
                            if event["payload"]["status"] != "ok" {
                                return None;
                            }
                            changed = true;
                        }
                        Some("postgres_changes") if event["topic"] == TOPIC => changed = true,
                        Some("phx_error" | "phx_close") => return None,
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        if now - self.last_reply > 45 {
            return None;
        }
        if self.joined && now - self.heartbeat >= 20 {
            self.send(json!({
                "topic": "phoenix", "event": "heartbeat", "payload": {}, "ref": "heartbeat"
            }));
            self.heartbeat = now;
        }
        Some(changed)
    }

    fn send(&mut self, payload: Value) {
        self.sender.send(WsMessage::Text(payload.to_string()));
    }
}
