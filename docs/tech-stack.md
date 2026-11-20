# Стек и платформы

| Область | Выбор |
| --- | --- |
| Язык и сборка | Стабильный Rust, Cargo Workspace, общий lockfile. |
| Интерфейс | egui. Нативное окно и WASM используют те же виджеты и стили. |
| Запуск | eframe управляет winit на desktop и canvas в браузере. |
| Рендеринг | wgpu через eframe. Прямой доступ к wgpu добавляется только при конкретной задаче рисования, которую egui не покрывает. |
| Web | `wasm32-unknown-unknown`, Trunk, минимальная HTML-оболочка с canvas. |
| Данные | serde для сериализации. SQLite на desktop. IndexedDB — исходный выбор для браузерного адаптера. |
| Облако | Supabase Auth и PostgREST с клиентским publishable key и изоляцией записей по RLS. |

Официальный шаблон eframe задаёт способ запуска через `run_native` и `WebRunner`. Для Tracking подбирайте совместимые версии egui и eframe вместе, проверяйте активные Cargo features и держите wgpu-бэкенд в обеих сборках. WebGPU — основной браузерный путь. Поддержку WebGL2 как резервного бэкенда проверяйте для выбранной версии wgpu и списка браузеров. При отсутствии подходящего GPU-бэкенда браузерная оболочка показывает причину отказа. [eframe_template](https://github.com/emilk/eframe_template/tree/f108546a2b9f2fc6a82543ceae1625a3893305cb)

## Особенности платформ

- Desktop-развёртывание открывает окно нативного бинарника. Системные файловые диалоги и уведомления вызывает desktop-адаптер.
- Web выбирает файлы через браузерный диалог и сохраняет экспорт загрузкой. Хранение и запросы выполняются асинхронно. Учитывайте квоту и отказ в доступе к хранилищу, ограничения фоновых вкладок и разрешения на уведомления. Пересчитывайте таймер при возвращении к вкладке. Для web-запросов к Supabase проверяйте CORS и ограничения происхождения при входе.
- В обе сборки попадает только публичная конфигурация Supabase. Ключи `service_role` и другие серверные секреты остаются на сервере. Локальная запись и облачная синхронизация имеют отдельные результаты.

## Запуск и проверки

Общий экран egui доступен в пакетах `app-desktop` и `app-web`. Команды для GPUI-прототипа приведены отдельно в README.

```sh
cargo run --locked -p app-desktop
rustup target add wasm32-unknown-unknown
cargo install --locked --version 0.22.2 trunk
cd apps/web
trunk serve --locked
```

Из корня проекта проверяйте форматирование, тесты ядра и команд, зависимости между слоями, нативную сборку на Windows, Linux и macOS и WASM. CI должен проверять транзитивные зависимости переносимых модулей по `cargo metadata` и собирать браузерный артефакт через Trunk. Добавьте реальный browser smoke-тест с запуском приложения и сохранением тестовой сессии.

```sh
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
cargo check --locked -p app-web --target wasm32-unknown-unknown
cd apps/web
trunk build --release --locked
```

Идея автоматизировать зависимости и WASM-проверку заимствована из DesignCraft. Размер workspace, состав проверок и выбор адаптеров определяются задачами Tracking. [DesignCraft AGENTS.md](https://github.com/storytold/designcraft/blob/4000591c6681d01386b9576f9a050b8460e6f2e3/AGENTS.md) · [Craft web runtime](https://github.com/storytold/craft-contrib/blob/5b1a2b1982fe7c774739d06d4a3818033cf4884c/rfcs/0023-web-runtime.md)
