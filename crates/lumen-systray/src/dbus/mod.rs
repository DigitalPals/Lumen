//! D-Bus interface for the system tray service.
//!
//! Contains the Lumen daemon interface and client-side proxy.

mod client;
mod server;

pub use client::SystemTrayLumenProxy;
pub(crate) use server::SystemTrayDaemon;

/// D-Bus service name.
pub const SERVICE_NAME: &str = "com.lumen.SystemTray1";

/// D-Bus object path.
pub const SERVICE_PATH: &str = "/com/lumen/SystemTray";
