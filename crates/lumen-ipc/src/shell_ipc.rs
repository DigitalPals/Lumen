//! D-Bus client proxy for shell IPC commands.
#![allow(missing_docs)]

use zbus::{Result, proxy};

/// D-Bus service name for shell IPC.
pub const SERVICE_NAME: &str = "com.lumen.Shell1";

/// D-Bus object path for shell IPC.
pub const SERVICE_PATH: &str = "/com/lumen/Shell";

#[proxy(
    interface = "com.lumen.Shell1",
    default_service = "com.lumen.Shell1",
    default_path = "/com/lumen/Shell",
    gen_blocking = false
)]
pub trait ShellIpc {
    async fn bar_hide(&self, monitor: &str) -> Result<()>;

    async fn bar_show(&self, monitor: &str) -> Result<()>;

    async fn bar_toggle(&self, monitor: &str) -> Result<()>;

    #[zbus(property)]
    fn bar_hidden(&self) -> Result<Vec<String>>;

    #[zbus(property)]
    fn connectors(&self) -> Result<Vec<String>>;
}
