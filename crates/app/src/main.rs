mod app;
mod views;

use std::path::PathBuf;

use anyhow::{Context as _, Result};
use gpui::{
    App, AppContext, Application, Bounds, TitlebarOptions, WindowBounds, WindowOptions, px, size,
};
use gpui_component::Root;
use tracking_data::{Database, cloud::Supabase};

fn main() -> Result<()> {
    dotenvy::dotenv().ok();
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
    let (database, state) = Database::open(&path)?;
    let cloud = match (
        std::env::var("TRACKING_SUPABASE_URL"),
        std::env::var("TRACKING_SUPABASE_KEY"),
    ) {
        (Ok(url), Ok(key)) if !url.trim().is_empty() && !key.trim().is_empty() => {
            Supabase::new(url.trim(), key.trim().to_owned()).map(Some)
        }
        (Err(_), Err(_)) => Ok(None),
        _ => Err(anyhow::anyhow!(
            "Заполните TRACKING_SUPABASE_URL и TRACKING_SUPABASE_KEY."
        )),
    };

    Application::new().run(move |cx: &mut App| {
        gpui_component::init(cx);
        cx.on_window_closed(|cx| {
            // X11 holds a borrow of the platform during the close callback.
            cx.spawn(async |cx| {
                let _ = cx.update(|cx| {
                    if cx.windows().is_empty() {
                        cx.quit();
                    }
                });
            })
            .detach();
        })
        .detach();

        let bounds = Bounds::centered(None, size(px(1160.0), px(800.0)), cx);
        if let Err(error) = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(880.0), px(600.0))),
                titlebar: Some(TitlebarOptions {
                    title: Some("Tracking".into()),
                    ..Default::default()
                }),
                app_id: Some("dev.punkhomov.tracking".into()),
                ..Default::default()
            },
            move |window, cx| {
                let view = cx.new(|cx| app::Tracker::new(database, state, cloud, window, cx));
                cx.new(|cx| Root::new(view, window, cx))
            },
        ) {
            eprintln!("Не удалось открыть окно: {error:#}");
            cx.quit();
        }
        cx.activate(true);
    });
    Ok(())
}
