//! Panel settings command.

use std::{
    io::ErrorKind,
    process::{Command, Stdio},
};

use tracing::info;

use crate::cli::CliAction;

/// Launches the settings application.
///
/// # Errors
///
/// Returns error if settings application cannot be launched.
pub async fn execute() -> CliAction {
    info!("Launching Lumen settings");

    let mut command = if let Ok(current_exe) = std::env::current_exe() {
        let sibling = current_exe.parent().map(|p| p.join("lumen-settings"));
        if let Some(ref sibling_path) = sibling
            && sibling_path.exists()
        {
            Command::new(sibling_path)
        } else {
            Command::new("lumen-settings")
        }
    } else {
        Command::new("lumen-settings")
    };

    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| match e.kind() {
            ErrorKind::NotFound => {
                "lumen-settings not found. Is Lumen installed correctly?".to_string()
            }
            ErrorKind::PermissionDenied => {
                "Permission denied when starting lumen-settings".to_string()
            }
            _ => format!("Failed to launch settings: {e}"),
        })?;

    Ok(())
}
