use std::{fs, io::Write};

use anyhow::{Context, Result};
use windows_registry::{CURRENT_USER, HSTRING};

use crate::{ICON_PNG, data_directory};

const APP_ID: &str = "dev.punkhomov.Tracking";

pub fn set_identity(notification: &mut notify_rust::Notification) -> Result<()> {
    let directory = data_directory()?;
    let icon = directory.join("notification-icon.png");
    if fs::read(&icon).as_deref().ok() != Some(ICON_PNG) {
        fs::create_dir_all(&directory)?;
        let mut file = tempfile::NamedTempFile::new_in(&directory)?;
        file.write_all(ICON_PNG)?;
        file.persist(&icon)
            .context("Не удалось сохранить иконку уведомлений.")?;
    }

    // Windows resolves unpackaged notification names and icons through the per-user Classes hive.
    let key = CURRENT_USER
        .create(format!(r"Software\Classes\AppUserModelId\{APP_ID}"))
        .context("Не удалось зарегистрировать уведомления Tracking для текущего пользователя.")?;
    key.set_string("DisplayName", "Tracking")?;
    key.set_hstring("IconUri", &HSTRING::from(icon.as_os_str()))?;
    notification.app_id(APP_ID);
    Ok(())
}
