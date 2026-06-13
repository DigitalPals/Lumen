//! D-Bus interface for the power profiles service.
//!
//! Contains the Lumen daemon interface and client-side proxy.

mod client;
mod server;

pub use client::PowerProfilesLumenProxy;
pub(crate) use server::PowerProfilesDaemon;

/// D-Bus service name.
pub const SERVICE_NAME: &str = "com.lumen.PowerProfiles1";

/// D-Bus object path.
pub const SERVICE_PATH: &str = "/com/lumen/PowerProfiles";
