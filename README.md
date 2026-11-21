# Tracking

Pomodoro-трекер на Rust с общим интерфейсом egui для нативных Windows, Linux,
macOS и браузера через WebAssembly. Архитектура следует подходу DesignCraft
из ArtCraft. Интерфейс отправляет команды в переносимое ядро, а платформенные
адаптеры отвечают за хранение, диалоги и уведомления. [Архитектура](docs/architecture.md) · [Функции трекера](docs/product.md)

## Запуск

Таймер с паузой, автостартом и напоминаниями, история с редактированием и
фильтрами, категории, таймлайн со вложенными зонами и статистика доступны
в одном интерфейсе egui. Активный таймер и история восстанавливаются после
перезапуска.

**Windows, Linux и macOS** с установленным rustup и нативным инструментарием сборки.

```sh
cargo run --locked
```

На Windows установите Visual Studio Build Tools с рабочей нагрузкой
«Разработка классических приложений на C++». На Linux зависимости можно
установить командой `bash scripts/install-linux-deps.sh`.

Приложение хранит SQLite в каталоге локальных данных пользователя.
Для отдельного файла задайте `TRACKING_DB`.

**Браузер** с WebGPU или WebGL2.

```sh
rustup target add wasm32-unknown-unknown
cargo install --locked --version 0.22.2 trunk
cd apps/web
trunk serve --locked
```

Откройте адрес, который выведет Trunk. Браузер сохраняет данные в IndexedDB
отдельно для каждого origin. Эти данные пока не синхронизируются между
сборками.

**Android** с API 26 и выше на ARM64 или x86-64. Первый клиент поддерживает
локальный таймер работы и перерыва, паузу, заметки и историю. Сборка требует
JDK 17, Android SDK 36, Build Tools 35.0.0, NDK 28.2.13676358 и rustup.

```sh
rustup target add aarch64-linux-android x86_64-linux-android
cargo install --locked cargo-ndk --version 4.1.2
cd apps/android
./gradlew :app:assembleDebug
```

На Windows используйте `gradlew.bat :app:assembleDebug`. APK лежит в
`apps/android/app/build/outputs/apk/debug/app-debug.apk`. В GitHub Actions
сборка **Android checks** проверяет восстановление таймера после остановки
процесса и прикладывает APK как артефакт `tracking-android-debug`.

## Релизы Windows

Отправьте тег `v*`, чтобы GitHub Actions собрал Windows x64 и опубликовал
`tracking-windows-x64.zip` с `tracking-desktop.exe` в GitHub Releases.

```sh
git tag v0.1.0
git push origin v0.1.0
```

Для пробной сборки запустите **Windows release** вручную в GitHub Actions.
ZIP будет доступен в артефактах запуска.

## Импорт и экспорт

Откройте **Настройки → Импорт JSON**, чтобы загрузить архив v1 или v2.
Перед импортом приложение показывает содержимое файла. По умолчанию
существующие сессии и категории сохраняются. Для замены есть отдельная
опция в окне импорта. Настройки и зоны из файла применяются при импорте.

В тех же настройках можно экспортировать всё, историю с категориями или
настройки с зонами. На Linux файловым диалогам нужен `xdg-desktop-portal`
с backend рабочего стола, например `xdg-desktop-portal-gtk`. Разрешения
на системные уведомления в браузере можно выдать из настроек Tracking
или через настройки сайта.

## Supabase

На первом запуске можно подключить свой проект Supabase или продолжить локально.
Tracking принимает URL проекта и публичный ключ в настройках, входит по коду
из письма и синхронизирует данные по кнопке. [Настройка Supabase, OTP и Brevo](docs/supabase.md)

## Структура

- `crates/core` — таймер, модель сессий, расчёт активного времени и JSON-архив.
- `crates/data` — локальное хранилище SQLite.
- `crates/cloud` — Supabase Auth, Data API и объединение данных.
- `crates/ui` и `crates/platform` — экран egui и контракт сохранения.
- `apps/desktop` и `apps/web` — точки запуска eframe.
- `apps/android` — приложение Jetpack Compose с адаптером UniFFI к Rust.

```sh
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
```
