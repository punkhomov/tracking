use std::sync::mpsc;

use app_platform::cloud::{
    Account, CloudRequest, CloudResponse, CloudState, Connection, Preference, Setup,
};
use chrono::{DateTime, Utc};
use egui::{Context, TextEdit, Ui};
use tracking_core::Command;

use crate::{
    Tracker,
    pages::Screen,
    theme::{card, primary},
};

const GUIDE: &str = "https://github.com/punkhomov/tracking/blob/main/docs/supabase.md";

pub(crate) struct CloudPanel {
    pub(crate) state: CloudState,
    draft: Option<ConnectionDraft>,
    email: String,
    code: String,
    code_sent: bool,
    account: Option<Account>,
    pending: Option<Pending>,
    conflict: Option<usize>,
    error: Option<String>,
    status: Option<&'static str>,
}

#[derive(Default)]
struct ConnectionDraft {
    url: String,
    key: String,
}

struct Pending {
    result: mpsc::Receiver<Result<CloudResponse, String>>,
    sync: bool,
}

enum Action {
    Setup(Setup),
    Request(CloudRequest),
    Synchronize(Preference),
}

impl CloudPanel {
    pub(crate) fn new(state: CloudState) -> Self {
        Self {
            state,
            draft: None,
            email: String::new(),
            code: String::new(),
            code_sent: false,
            account: None,
            pending: None,
            conflict: None,
            error: None,
            status: None,
        }
    }

    pub(crate) fn syncing(&self) -> bool {
        self.pending.as_ref().is_some_and(|pending| pending.sync)
    }

    pub(crate) fn saved(&mut self, state: CloudState) {
        if self.state.setup != state.setup {
            self.clear_login();
            self.error = None;
            self.status = None;
        }
        self.draft = None;
        self.state = state;
    }

    fn clear_login(&mut self) {
        self.account = None;
        self.code_sent = false;
        self.code.clear();
        self.conflict = None;
    }

    fn content(&mut self, ui: &mut Ui) -> Option<Action> {
        ui.heading("Синхронизация Supabase");
        if let Some(error) = &self.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        if let Some(status) = self.status {
            ui.label(status);
        }
        if let Some(pending) = &self.pending {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(if pending.sync {
                    "Синхронизация…"
                } else {
                    "Запрос к Supabase…"
                });
            });
            return None;
        }
        if let Some(draft) = &mut self.draft {
            field(
                ui,
                "URL проекта",
                &mut draft.url,
                "https://your-project.supabase.co",
                512,
            );
            field(
                ui,
                "Публичный ключ",
                &mut draft.key,
                "sb_publishable_…",
                2048,
            );
            ui.hyperlink_to("Инструкция по настройке Supabase и Brevo", GUIDE);
            let mut action = None;
            let mut cancel = false;
            ui.horizontal_wrapped(|ui| {
                if primary(ui, "Сохранить подключение").clicked() {
                    match Connection::new(&draft.url, &draft.key) {
                        Ok(connection) => {
                            action = Some(Action::Setup(Setup::Configured(connection)))
                        }
                        Err(error) => self.error = Some(error.to_string()),
                    }
                }
                cancel = ui.button("Отмена").clicked();
            });
            if cancel || ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                self.draft = None;
                self.error = None;
                return None;
            }
            return action;
        }
        let Setup::Configured(connection) = &self.state.setup else {
            ui.label("Локальный режим");
            if ui.button("Настроить Supabase").clicked() {
                self.draft = Some(ConnectionDraft::default());
            }
            return None;
        };

        ui.label(connection.url());
        if let Some(account) = &self.account {
            ui.label(format!("Вход выполнен как {}", self.email));
            if let Some(count) = self.conflict {
                ui.label(format!(
                    "Конфликтующих изменений: {count}. Выберите версию для этих записей."
                ));
                let mut choice = None;
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Локальные изменения").clicked() {
                        choice = Some(Action::Synchronize(Preference::Local));
                    }
                    if ui.button("Облачные изменения").clicked() {
                        choice = Some(Action::Synchronize(Preference::Remote));
                    }
                    if ui.button("Отмена").clicked() {
                        self.conflict = None;
                    }
                });
                return choice;
            }
            let changed = self
                .state
                .last_sync
                .as_ref()
                .is_some_and(|point| point.account != *account);
            if changed {
                ui.label("Локальные данные ранее синхронизировались с другим аккаунтом. Подтвердите объединение с этим аккаунтом.");
            }
            let mut action = None;
            ui.horizontal_wrapped(|ui| {
                if primary(
                    ui,
                    if changed {
                        "Объединить с этим аккаунтом"
                    } else {
                        "Синхронизировать"
                    },
                )
                .clicked()
                {
                    action = Some(Action::Synchronize(Preference::Ask));
                }
                if ui.button("Выйти на этом устройстве").clicked() {
                    action = Some(Action::Request(CloudRequest::SignOut));
                }
            });
            return action;
        }

        let mut action = None;
        if self.code_sent {
            ui.label(format!("Код отправлен на {}", self.email));
            field(ui, "Код из письма", &mut self.code, "", 10);
            let mut change_email = false;
            ui.horizontal_wrapped(|ui| {
                if primary(ui, "Войти").clicked() {
                    action = Some(Action::Request(CloudRequest::VerifyCode {
                        connection: connection.clone(),
                        email: self.email.clone(),
                        code: self.code.clone(),
                    }));
                }
                if ui.button("Отправить код ещё раз").clicked() {
                    action = Some(Action::Request(CloudRequest::RequestCode {
                        connection: connection.clone(),
                        email: self.email.clone(),
                    }));
                }
                change_email = ui.button("Другой email").clicked();
            });
            if change_email {
                self.clear_login();
                self.error = None;
                self.status = None;
            }
        } else {
            field(ui, "Email", &mut self.email, "name@example.com", 254);
            ui.horizontal_wrapped(|ui| {
                if primary(ui, "Отправить код").clicked() {
                    self.email = self.email.trim().to_owned();
                    action = Some(Action::Request(CloudRequest::RequestCode {
                        connection: connection.clone(),
                        email: self.email.clone(),
                    }));
                }
                if ui.button("Изменить подключение").clicked() {
                    self.error = None;
                    self.status = None;
                    self.draft = Some(ConnectionDraft {
                        url: connection.url().into(),
                        key: connection.key().into(),
                    });
                }
                if ui.button("Отключить Supabase").clicked() {
                    action = Some(Action::Setup(Setup::Local));
                }
            });
        }
        action
    }
}

fn field(ui: &mut Ui, name: &str, value: &mut String, hint: &str, limit: usize) {
    let label = ui.label(name);
    ui.add(
        TextEdit::singleline(value)
            .hint_text(hint)
            .desired_width(420.0_f32.min(ui.available_width()))
            .char_limit(limit),
    )
    .labelled_by(label.id);
}

impl Tracker {
    pub(crate) fn offer_cloud(&mut self, ui: &mut Ui) {
        if !matches!(self.cloud.state.setup, Setup::Undecided) {
            return;
        }
        card(ui, |ui| {
            ui.label("Подключите свой проект Supabase для синхронизации между устройствами.");
            ui.horizontal_wrapped(|ui| {
                if ui.button("Настроить Supabase").clicked() {
                    self.cloud.draft = Some(ConnectionDraft::default());
                    self.screen = Screen::Settings;
                }
                if ui.button("Продолжить локально").clicked() {
                    self.set_cloud_setup(Setup::Local, ui.ctx());
                }
            });
        });
    }

    pub(crate) fn cloud_settings(&mut self, ui: &mut Ui) {
        card(ui, |ui| match self.cloud.content(ui) {
            Some(Action::Setup(setup)) => self.set_cloud_setup(setup, ui.ctx()),
            Some(Action::Request(request)) => self.request_cloud(request, ui.ctx()),
            Some(Action::Synchronize(preference)) => self.request_cloud(
                CloudRequest::Synchronize {
                    dataset: Box::new(self.state.dataset.clone()),
                    baseline: self.cloud.state.last_sync.clone().map(Box::new),
                    preference,
                },
                ui.ctx(),
            ),
            None => {}
        });
    }

    fn set_cloud_setup(&mut self, setup: Setup, ctx: &Context) {
        let mut cloud = self.cloud.state.clone();
        cloud.setup = setup;
        self.persist(
            self.state.clone(),
            Some(cloud),
            ctx,
            false,
            Some("Настройки Supabase сохранены."),
        );
    }

    fn request_cloud(&mut self, request: CloudRequest, ctx: &Context) {
        if self.pending.is_some() || self.io.is_some() || self.cloud.pending.is_some() {
            return;
        }
        self.cloud.error = None;
        self.cloud.status = None;
        let sync = matches!(request, CloudRequest::Synchronize { .. });
        let (send, result) = mpsc::channel();
        let ctx = ctx.clone();
        self.cloud.pending = Some(Pending { result, sync });
        self.platform.cloud(
            request,
            Box::new(move |result| {
                let _ = send.send(result.map_err(|error| format!("{error:#}")));
                ctx.request_repaint();
            }),
        );
    }

    pub(crate) fn poll_cloud(&mut self, now: DateTime<Utc>, ctx: &Context) {
        let Some(pending) = self.cloud.pending.take() else {
            return;
        };
        match pending.result.try_recv() {
            Err(mpsc::TryRecvError::Empty) => self.cloud.pending = Some(pending),
            Err(mpsc::TryRecvError::Disconnected) => {
                self.cloud.error = Some("Запрос к Supabase не завершился.".into())
            }
            Ok(Err(error)) => self.cloud.error = Some(error),
            Ok(Ok(CloudResponse::CodeSent)) => {
                self.cloud.code_sent = true;
                self.cloud.code.clear();
            }
            Ok(Ok(CloudResponse::SignedIn(account))) => {
                self.cloud.clear_login();
                self.cloud.account = Some(account);
            }
            Ok(Ok(CloudResponse::Conflict(count))) => self.cloud.conflict = Some(count),
            Ok(Ok(CloudResponse::SignedOut)) => {
                self.cloud.clear_login();
                self.cloud.status = Some("Выполнен выход на этом устройстве.");
            }
            Ok(Ok(CloudResponse::SessionExpired)) => {
                self.cloud.clear_login();
                self.cloud.error = Some("Сессия Supabase недоступна. Войдите снова.".into());
            }
            Ok(Ok(CloudResponse::Synchronized(point))) => {
                self.cloud.conflict = None;
                let mut next = self.state.clone();
                match next.execute(
                    Command::ApplySyncedDataset(Box::new(point.dataset.clone())),
                    now,
                ) {
                    Ok(()) => {
                        let cloud = CloudState {
                            setup: self.cloud.state.setup.clone(),
                            last_sync: Some(*point),
                        };
                        self.persist(
                            next,
                            Some(cloud),
                            ctx,
                            false,
                            Some("Синхронизация завершена."),
                        );
                    }
                    Err(error) => self.cloud.error = Some(error.to_string()),
                }
            }
        }
    }
}
