use tokio::process::Command;
use tracing::instrument;
use zbus::{Connection, zvariant::OwnedObjectPath};

use crate::{error::Error, proxy::manager::NetworkManagerProxy};

pub(super) struct VpnControls;

impl VpnControls {
    #[instrument(skip(connection), fields(profile = %profile_path), err)]
    pub(super) async fn connect_profile(
        connection: &Connection,
        profile_path: &OwnedObjectPath,
    ) -> Result<OwnedObjectPath, Error> {
        let proxy = NetworkManagerProxy::new(connection).await?;
        let null_path = null_path()?;

        proxy
            .activate_connection(profile_path, &null_path, &null_path)
            .await
            .map_err(|err| Error::OperationFailed {
                operation: "activate vpn connection",
                source: err.into(),
            })
    }

    #[instrument(skip(connection), fields(active = %active_path), err)]
    pub(super) async fn disconnect_active(
        connection: &Connection,
        active_path: &OwnedObjectPath,
    ) -> Result<(), Error> {
        let proxy = NetworkManagerProxy::new(connection).await?;

        proxy
            .deactivate_connection(active_path)
            .await
            .map_err(|err| Error::OperationFailed {
                operation: "deactivate vpn connection",
                source: err.into(),
            })?;

        Ok(())
    }

    #[instrument(err)]
    pub(super) async fn tailscale_up() -> Result<(), Error> {
        run_tailscale_command("up", &[]).await
    }

    #[instrument(err)]
    pub(super) async fn tailscale_down() -> Result<(), Error> {
        match run_tailscale_command("down", &[]).await {
            Ok(()) => Ok(()),
            Err(err) if should_retry_tailscale_down_with_risk(&err) => {
                run_tailscale_command("down", &["--accept-risk=all"]).await
            }
            Err(err) => Err(err),
        }
    }
}

fn null_path() -> Result<OwnedObjectPath, Error> {
    OwnedObjectPath::try_from("/").map_err(|err| Error::OperationFailed {
        operation: "create null object path",
        source: err.into(),
    })
}

async fn run_tailscale_command(arg: &str, extra_args: &[&str]) -> Result<(), Error> {
    let output = Command::new("tailscale")
        .arg(arg)
        .args(extra_args)
        .output()
        .await
        .map_err(|err| Error::OperationFailed {
            operation: "run tailscale command",
            source: err.into(),
        })?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    Err(Error::OperationFailed {
        operation: match arg {
            "up" => "start tailscale",
            "down" => "stop tailscale",
            _ => "run tailscale command",
        },
        source: Box::new(TailscaleCommandError(stderr)),
    })
}

fn should_retry_tailscale_down_with_risk(err: &Error) -> bool {
    let message = err.to_string();
    message.contains("--accept-risk")
        || message.contains("accept-risk")
        || message.contains("lose-ssh")
        || message.contains("risk")
}

#[derive(Debug)]
struct TailscaleCommandError(String);

impl std::fmt::Display for TailscaleCommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.0.is_empty() {
            write!(f, "tailscale command failed")
        } else {
            write!(f, "{}", self.0)
        }
    }
}

impl std::error::Error for TailscaleCommandError {}
