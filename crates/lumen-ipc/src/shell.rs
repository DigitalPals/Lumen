//! Shared constants and types for lumen-shell IPC.
//!
//! These are used by both the CLI (lumen) and the shell daemon (lumen-shell)
//! to communicate via D-Bus.

use std::collections::HashMap;

use zbus::zvariant::Value;

/// D-Bus application ID for lumen-shell.
pub const APP_ID: &str = "com.lumen.shell";

/// D-Bus object path for lumen-shell.
pub const DBUS_PATH: &str = "/com/lumen/shell";

/// D-Bus interface for GTK application actions.
pub const ACTIONS_INTERFACE: &str = "org.gtk.Actions";

/// Application-level action names.
pub mod actions {
    /// Action to quit the shell gracefully.
    pub const QUIT: &str = "quit";

    /// Action to open the GTK Inspector for debugging.
    pub const INSPECTOR: &str = "inspector";
}

/// Proxy for GApplication's org.gtk.Actions interface.
///
/// Used by the CLI to send actions to the running shell via D-Bus.
#[zbus::proxy(
    interface = "org.gtk.Actions",
    default_path = "/com/lumen/shell",
    default_service = "com.lumen.shell"
)]
pub trait GtkActions {
    /// Activates an action by name.
    fn activate(
        &self,
        action_name: &str,
        parameter: Vec<Value<'_>>,
        platform_data: HashMap<&str, Value<'_>>,
    ) -> zbus::Result<()>;
}
