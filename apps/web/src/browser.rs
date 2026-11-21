use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use anyhow::{Context, Result, anyhow, ensure};
use app_platform::{
    ArchiveFile, Complete, Platform, SaveComplete,
    cloud::{CloudComplete, CloudRequest, CloudState},
};
use app_ui::Tracker;
use chrono::Utc;
use eframe::{
    egui,
    wasm_bindgen::{JsCast, JsValue},
};
use idb::{
    Database, ObjectStore, TransactionMode,
    builder::{DatabaseBuilder, ObjectStoreBuilder},
};
use serde::{Deserialize, Serialize};
use tracking_core::TrackerState;

const STORE: &str = "snapshot";
const KEY: &str = "tracker";

#[derive(Default, Deserialize, Serialize)]
struct Snapshot {
    revision: u64,
    state: TrackerState,
    #[serde(default)]
    cloud: CloudState,
}

#[derive(Clone)]
struct BrowserStore {
    database: Rc<Database>,
    revision: Rc<Cell<u64>>,
    cloud: Rc<RefCell<Option<tracking_cloud::Service>>>,
}

impl BrowserStore {
    async fn open() -> Result<(Self, Snapshot)> {
        let database = DatabaseBuilder::new("tracking-app")
            .version(1)
            .add_object_store(ObjectStoreBuilder::new(STORE))
            .build()
            .await?;
        let transaction = database.transaction(&[STORE], TransactionMode::ReadOnly)?;
        let snapshot = read(&transaction.object_store(STORE)?).await?;
        transaction.await?;

        let snapshot = snapshot.unwrap_or_default();
        snapshot.state.dataset.validate()?;
        Ok((
            Self {
                database: Rc::new(database),
                revision: Rc::new(Cell::new(snapshot.revision)),
                cloud: Rc::new(RefCell::new(Some(tracking_cloud::Service::default()))),
            },
            snapshot,
        ))
    }

    async fn write(&self, state: TrackerState, cloud: CloudState) -> Result<()> {
        let previous = self.revision.get();
        let revision = previous
            .checked_add(1)
            .context("Исчерпаны ревизии хранилища.")?;
        let value = JsValue::from_str(&serde_json::to_string(&Snapshot {
            revision,
            state,
            cloud,
        })?);
        let transaction = self
            .database
            .transaction(&[STORE], TransactionMode::ReadWrite)?;
        let store = transaction.object_store(STORE)?;
        let stored = read(&store).await?.map_or(0, |snapshot| snapshot.revision);
        ensure!(
            previous == stored,
            "Данные изменились в другой вкладке. Перезагрузите страницу."
        );
        store.put(&value, Some(&JsValue::from_str(KEY)))?.await?;
        ensure!(
            transaction.await?.is_committed(),
            "Не удалось сохранить данные."
        );
        self.revision.set(revision);
        Ok(())
    }
}

async fn read(store: &ObjectStore) -> Result<Option<Snapshot>> {
    store
        .get(JsValue::from_str(KEY))?
        .await?
        .map(|value| {
            let snapshot = value.as_string().context("Неверный формат хранилища.")?;
            serde_json::from_str(&snapshot).context("Не удалось прочитать сохранённые данные.")
        })
        .transpose()
}

impl Platform for BrowserStore {
    fn save(&mut self, state: &TrackerState, cloud: &CloudState, complete: SaveComplete) {
        let store = self.clone();
        let state = state.clone();
        let cloud = cloud.clone();
        wasm_bindgen_futures::spawn_local(async move {
            complete(
                store
                    .write(state, cloud)
                    .await
                    .context("Сохранение в браузере"),
            );
        });
    }

    fn cloud(&mut self, request: CloudRequest, complete: CloudComplete) {
        let Some(mut service) = self.cloud.take() else {
            return complete(Err(anyhow!("Запрос к Supabase уже выполняется.")));
        };
        let cloud = self.cloud.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let result = service.execute(request).await;
            cloud.replace(Some(service));
            complete(result);
        });
    }
    fn open_archive(&self, complete: Complete<Option<ArchiveFile>>) {
        let picker = pick_file();
        wasm_bindgen_futures::spawn_local(async move {
            let result = async {
                let (input, selected) = picker?;
                let event = wasm_bindgen_futures::JsFuture::from(selected).await;
                let file = input.files().and_then(|files| files.get(0));
                input.remove();
                event.map_err(js_error)?;
                let Some(file) = file else {
                    return Ok(None);
                };
                let buffer = wasm_bindgen_futures::JsFuture::from(file.array_buffer())
                    .await
                    .map_err(js_error)?;
                let text = String::from_utf8(js_sys::Uint8Array::new(&buffer).to_vec())
                    .context("Архив должен быть в UTF-8.")?;
                Ok(Some(ArchiveFile {
                    name: file.name(),
                    text,
                }))
            }
            .await;
            complete(result);
        });
    }

    fn save_archive(&self, name: &str, contents: String, complete: Complete<bool>) {
        complete(download(name, &contents).map(|()| true));
    }

    fn notify(&self, title: &str, body: &str) {
        use web_sys::{
            HtmlAudioElement, Notification, NotificationOptions, NotificationPermission,
        };
        if notifications_available()
            && Notification::permission() == NotificationPermission::Granted
        {
            let options = NotificationOptions::new();
            options.set_body(body);
            let _ = Notification::new_with_options(title, &options);
        }
        if let Ok(audio) = HtmlAudioElement::new_with_src(concat!(
            "data:audio/wav;base64,",
            include_str!("reminder.b64")
        )) && let Ok(play) = audio.play()
        {
            wasm_bindgen_futures::spawn_local(async move {
                let _ = wasm_bindgen_futures::JsFuture::from(play).await;
            });
        }
    }

    fn request_notifications(&self, complete: Complete<bool>) {
        if !notifications_available() {
            complete(Err(anyhow!("Уведомления недоступны в этом браузере.")));
            return;
        }
        let request = web_sys::Notification::request_permission();
        wasm_bindgen_futures::spawn_local(async move {
            let result = async {
                let promise = request.map_err(js_error)?;
                let result = wasm_bindgen_futures::JsFuture::from(promise)
                    .await
                    .map_err(js_error)?;
                Ok(result.as_string().as_deref() == Some("granted"))
            }
            .await;
            complete(result);
        });
    }

    fn set_title(&self, title: &str) {
        if let Some(document) = web_sys::window().and_then(|window| window.document()) {
            document.set_title(title);
        }
    }

    fn system_dark(&self) -> bool {
        web_sys::window()
            .and_then(|window| {
                window
                    .match_media("(prefers-color-scheme: dark)")
                    .ok()
                    .flatten()
            })
            .is_some_and(|query| query.matches())
    }
}

fn notifications_available() -> bool {
    web_sys::window().is_some_and(|window| {
        js_sys::Reflect::has(window.as_ref(), &JsValue::from_str("Notification")).unwrap_or(false)
    })
}

fn pick_file() -> Result<(web_sys::HtmlInputElement, js_sys::Promise)> {
    let document = web_sys::window()
        .and_then(|window| window.document())
        .context("Браузер недоступен.")?;
    let input = document
        .create_element("input")
        .map_err(js_error)?
        .dyn_into::<web_sys::HtmlInputElement>()
        .map_err(|_| anyhow!("Не удалось открыть диалог выбора файла."))?;
    input.set_type("file");
    input.set_accept(".json,application/json");
    input.set_hidden(true);
    document
        .body()
        .context("Браузер недоступен.")?
        .append_child(&input)
        .map_err(js_error)?;
    let selected = js_sys::Promise::new(&mut |resolve, reject| {
        input.set_onchange(Some(&resolve));
        if let Err(error) = input.add_event_listener_with_callback("cancel", &resolve) {
            let _ = reject.call1(&JsValue::UNDEFINED, &error);
        }
    });
    input.click();
    Ok((input, selected))
}

fn download(name: &str, contents: &str) -> Result<()> {
    use web_sys::{Blob, BlobPropertyBag, HtmlAnchorElement, Url};
    let document = web_sys::window()
        .and_then(|window| window.document())
        .context("Браузер недоступен.")?;
    let link = document
        .create_element("a")
        .map_err(js_error)?
        .dyn_into::<HtmlAnchorElement>()
        .map_err(|_| anyhow!("Не удалось создать ссылку на архив."))?;
    let properties = BlobPropertyBag::new();
    properties.set_type("application/json");
    let parts = js_sys::Array::of1(&JsValue::from_str(contents));
    let blob = Blob::new_with_str_sequence_and_options(&parts, &properties).map_err(js_error)?;
    let url = Url::create_object_url_with_blob(&blob).map_err(js_error)?;
    link.set_href(&url);
    link.set_download(name);
    link.click();
    Url::revoke_object_url(&url).map_err(js_error)?;
    Ok(())
}

fn js_error(error: JsValue) -> anyhow::Error {
    anyhow!("Ошибка браузера: {error:?}")
}

pub fn run() {
    let _ = eframe::WebLogger::init(log::LevelFilter::Info);
    wasm_bindgen_futures::spawn_local(async {
        let Some(document) = web_sys::window().and_then(|window| window.document()) else {
            log::error!("Не удалось получить документ браузера.");
            return;
        };
        let status = document.get_element_by_id("status");
        match start(&document).await {
            Ok(()) => {
                if let Some(status) = status {
                    status.remove();
                }
            }
            Err(error) => {
                if let Some(status) = status {
                    status
                        .set_text_content(Some(&format!("Не удалось открыть Tracking. {error:#}")));
                }
                log::error!("{error:#}");
            }
        }
    });
}

async fn start(document: &web_sys::Document) -> Result<()> {
    let canvas = document
        .get_element_by_id("tracking")
        .and_then(|element| element.dyn_into::<web_sys::HtmlCanvasElement>().ok())
        .context("Не найден canvas приложения.")?;
    let (store, snapshot) = BrowserStore::open()
        .await
        .context("Не удалось открыть IndexedDB.")?;
    eframe::WebRunner::new()
        .start(
            canvas,
            eframe::WebOptions::default(),
            Box::new(move |_| {
                Ok(Box::new(WebApp(Tracker::new(
                    snapshot.state,
                    snapshot.cloud,
                    store,
                ))))
            }),
        )
        .await
        .map_err(|error| anyhow!("Ошибка запуска eframe: {error:?}"))
}

struct WebApp(Tracker);

impl eframe::App for WebApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.0.ui(ui, Utc::now());
    }
}
