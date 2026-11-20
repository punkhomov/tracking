#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{io::Write, path::PathBuf};

use anyhow::{Context, Result, anyhow};
use app_platform::{ArchiveFile, Complete, Platform, SaveComplete};
use app_ui::Tracker;
use chrono::Utc;
use eframe::egui;
use tracking_core::TrackerState;
use tracking_data::{Database, LocalState};

fn main() -> Result<()> {
    let path = match std::env::var_os("TRACKING_DB") {
        Some(path) => PathBuf::from(path),
        None => directories::ProjectDirs::from("dev", "punkhomov", "Tracking")
            .context("Не найдена папка данных приложения.")?
            .data_local_dir()
            .join("tracking.sqlite3"),
    };
    if let Some(directory) = path.parent().filter(|path| !path.as_os_str().is_empty()) {
        std::fs::create_dir_all(directory)?;
    }
    let (database, local) = Database::open(&path)?;
    let state = TrackerState {
        dataset: local.dataset,
        timer: local.timer,
        next_kind: local.next_kind,
    };
    eframe::run_native(
        "Tracking",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([920.0, 740.0])
                .with_min_inner_size([360.0, 420.0]),
            ..Default::default()
        },
        Box::new(move |cc| {
            let store = DesktopStore {
                database,
                context: cc.egui_ctx.clone(),
                local: LocalState {
                    account: local.account,
                    baseline: local.baseline,
                    ..LocalState::default()
                },
            };
            Ok(Box::new(DesktopApp(Tracker::new(state, store))))
        }),
    )
    .map_err(|error| anyhow!("{error}"))
}

struct DesktopApp(Tracker);

impl eframe::App for DesktopApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.0.ui(ui, Utc::now());
    }
}

struct DesktopStore {
    database: Database,
    context: egui::Context,
    local: LocalState,
}

impl Platform for DesktopStore {
    fn save(&mut self, state: &TrackerState, complete: SaveComplete) {
        self.local.dataset = state.dataset.clone();
        self.local.timer = state.timer.clone();
        self.local.next_kind = state.next_kind;
        complete(self.database.save(&self.local));
    }

    fn open_archive(&self, complete: Complete<Option<ArchiveFile>>) {
        let result = rfd::FileDialog::new()
            .add_filter("JSON", &["json"])
            .pick_file()
            .map(|path| {
                let name = path.file_name().map_or_else(
                    || "Архив".into(),
                    |name| name.to_string_lossy().into_owned(),
                );
                Ok(ArchiveFile {
                    name,
                    text: std::fs::read_to_string(path).context("Не удалось прочитать архив.")?,
                })
            })
            .transpose();
        complete(result);
    }

    fn save_archive(&self, name: &str, contents: String, complete: Complete<bool>) {
        let result = (|| {
            let Some(path) = rfd::FileDialog::new()
                .add_filter("JSON", &["json"])
                .set_file_name(name)
                .save_file()
            else {
                return Ok(false);
            };
            let directory = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or(std::path::Path::new("."));
            let mut file = tempfile::NamedTempFile::new_in(directory)?;
            file.write_all(contents.as_bytes())?;
            file.as_file().sync_all()?;
            file.persist(path).context("Не удалось сохранить архив.")?;
            Ok(true)
        })();
        complete(result);
    }

    fn notify(&self, title: &str, body: &str) {
        let (title, body) = (title.to_owned(), body.to_owned());
        std::thread::spawn(move || {
            let _ = notify(&title, &body);
        });
    }

    fn request_notifications(&self, complete: Complete<bool>) {
        complete(notify("Tracking", "Напоминания включены").map(|()| true));
    }

    fn set_title(&self, title: &str) {
        self.context
            .send_viewport_cmd(egui::ViewportCommand::Title(title.to_owned()));
    }

    fn system_dark(&self) -> bool {
        self.context.system_theme() == Some(egui::Theme::Dark)
    }
}

fn notify(title: &str, body: &str) -> Result<()> {
    let mut notification = notify_rust::Notification::new();
    notification.appname("Tracking").summary(title).body(body);
    #[cfg(not(target_os = "windows"))]
    notification.sound_name(if cfg!(target_os = "macos") {
        "Glass"
    } else {
        "message-new-instant"
    });
    notification
        .show()
        .context("Системные уведомления недоступны.")?;
    Ok(())
}
