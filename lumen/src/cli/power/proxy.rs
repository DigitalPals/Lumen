//! D-Bus proxy utilities for power profile commands.

use lumen_power_profiles::dbus::PowerProfilesLumenProxy;
use zbus::{Connection, Error as ZbusError};

use crate::cli::dbus;

const SERVICE_NAME: &str = "Power profiles";

/// Creates a PowerProfilesLumenProxy connection.
///
/// # Errors
/// Returns error if D-Bus connection or proxy creation fails.
pub async fn connect() -> Result<(Connection, PowerProfilesLumenProxy<'static>), String> {
    let connection = dbus::session().await?;

    let proxy = PowerProfilesLumenProxy::new(&connection)
        .await
        .map_err(|e| format!("Failed to create power profiles proxy: {e}"))?;

    Ok((connection, proxy))
}

/// Transforms zbus errors into user-friendly messages.
pub fn format_error(operation: &str, error: ZbusError) -> String {
    dbus::format_error(SERVICE_NAME, operation, error)
}
