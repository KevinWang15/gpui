//! Windows per-user Launch-at-Login (`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`)
//! backed by `windows-registry` with zero additional dependencies.

use anyhow::{Context as _, Result};
use gpui::AutostartOptions;
use windows_registry::CURRENT_USER;

const RUN_KEY_PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

pub(crate) fn is_launch_at_login(app_name: &str) -> Result<bool> {
    let key = match CURRENT_USER.open(RUN_KEY_PATH) {
        Ok(key) => key,
        Err(_) => return Ok(false),
    };
    match key.get_string(app_name) {
        Ok(value) => Ok(!value.trim().is_empty()),
        Err(_) => Ok(false),
    }
}

pub(crate) fn set_launch_at_login(options: &AutostartOptions, enabled: bool) -> Result<()> {
    let key = CURRENT_USER
        .create(RUN_KEY_PATH)
        .context("opening HKCU Run registry key")?;
    if enabled {
        let cmd = options
            .windows_command_line()
            .context("formatting launch-at-login command line")?;
        key.set_string(options.app_name.as_ref(), &cmd)
            .context("writing HKCU Run registry value")?;
    } else {
        // Ignore missing value so disabling an already-disabled entry succeeds idempotently.
        if key.get_string(options.app_name.as_ref()).is_ok() {
            key.remove_value(options.app_name.as_ref())
                .context("removing HKCU Run registry value")?;
        }
    }
    Ok(())
}
