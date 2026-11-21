# Стек и платформы

| Область | Выбор |
| --- | --- |
| Язык и сборка | Стабильный Rust, Cargo Workspace, общий lockfile. |
| Интерфейс | Общий egui для desktop и web. Jetpack Compose для Android. |
| Запуск | eframe управляет winit на desktop и canvas в браузере. |
| Рендеринг | wgpu через eframe. Прямой доступ к wgpu добавляется только при конкретной задаче рисования, которую egui не покрывает. |
| Web | `wasm32-unknown-unknown`, Trunk, минимальная HTML-оболочка с canvas. |
| Android | Kotlin, Gradle и Android NDK. UniFFI подключает команды Rust к Kotlin. |
| Данные | serde для сериализации. SQLite на desktop и Android, IndexedDB на web. |
| Облако | Supabase Auth с email OTP, PostgREST с публичным ключом и изоляция записей по RLS. |

Официальный шаблон eframe задаёт способ запуска через `run_native` и `WebRunner`. Для Tracking подбирайте совместимые версии egui и eframe вместе, проверяйте активные Cargo features и держите wgpu-бэкенд в обеих сборках. WebGPU — основной браузерный путь. Поддержку WebGL2 как резервного бэкенда проверяйте для выбранной версии wgpu и списка браузеров. При отсутствии подходящего GPU-бэкенда браузерная оболочка показывает причину отказа. [eframe_template](https://github.com/emilk/eframe_template/tree/f108546a2b9f2fc6a82543ceae1625a3893305cb)

## Версии зависимостей

При добавлении и обновлении зависимостей Cargo выбирает версии, опубликованные
как минимум семь дней назад. Сборки с `--locked` используют версии из
`Cargo.lock`. [Конфигурация Cargo](../.cargo/config.toml)

## Особенности платформ

- Desktop-развёртывание открывает окно нативного бинарника. Системные файловые диалоги и уведомления вызывает desktop-адаптер.
- Web выбирает файлы через браузерный диалог и сохраняет экспорт загрузкой. Хранение и запросы выполняются асинхронно. Учитывайте квоту и отказ в доступе к хранилищу, ограничения фоновых вкладок и разрешения на уведомления. Пересчитывайте таймер при возвращении к вкладке. Для web-запросов к Supabase проверяйте CORS и ограничения происхождения при входе.
- Android хранит SQLite в приватном каталоге приложения. Команды и чтение SQLite выполняются вне UI-потока. Сборка включает библиотеки ARM64 и x86-64, а CI проверяет сценарий Compose на эмуляторе x86-64.
- URL и публичный ключ Supabase вводятся в приложении. Ключ `service_role` и пароль SMTP остаются на сервере. Локальная запись и облачная синхронизация имеют отдельные результаты. [Настройка Supabase](supabase.md)

## Запуск и проверки

Пакеты `app-desktop` и `app-web` запускают общий интерфейс egui.

```sh
cargo run --locked -p app-desktop
rustup target add wasm32-unknown-unknown
cargo install --locked --version 0.22.2 trunk
cd apps/web
trunk serve --locked
```

Из корня проекта проверяйте форматирование, тесты ядра и команд, зависимости между слоями, нативную сборку на Windows, Linux и macOS и WASM. CI проверяет зависимости переносимых модулей по `cargo tree` и собирает браузерный артефакт через Trunk. Добавьте реальный browser smoke-тест с запуском приложения и сохранением тестовой сессии.

```sh
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
cargo check --locked -p app-web --target wasm32-unknown-unknown
cd apps/web
trunk build --release --locked
```

Идея автоматизировать зависимости и WASM-проверку заимствована из DesignCraft. Размер workspace, состав проверок и выбор адаптеров определяются задачами Tracking. [DesignCraft AGENTS.md](https://github.com/storytold/designcraft/blob/4000591c6681d01386b9576f9a050b8460e6f2e3/AGENTS.md) · [Craft web runtime](https://github.com/storytold/craft-contrib/blob/5b1a2b1982fe7c774739d06d4a3818033cf4884c/rfcs/0023-web-runtime.md)
