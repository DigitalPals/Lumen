mod controls;
mod monitoring;
mod types;

use std::sync::Arc;

use derive_more::Debug;
use futures::future::join_all;
use lumen_core::Property;
use lumen_traits::Reactive;
use tokio::process::Command;
use tokio_util::sync::CancellationToken;
use tracing::debug;
pub use types::{TailscaleStatus, VpnConnection, VpnKind, VpnProfile, VpnState};
use zbus::{Connection, zvariant::OwnedObjectPath};

use self::controls::VpnControls;
use crate::{
    core::{
        connection::{ActiveConnection, ActiveConnectionParams},
        settings::Settings,
    },
    error::Error,
    proxy::{active_connection::vpn::VPNConnectionProxy, manager::NetworkManagerProxy},
    types::states::NMVpnConnectionState,
};

/// VPN profile and active connection manager.
#[derive(Debug)]
pub struct VpnManager {
    #[debug(skip)]
    pub(crate) zbus_connection: Connection,
    #[debug(skip)]
    pub(crate) cancellation_token: CancellationToken,
    #[debug(skip)]
    pub(crate) settings: Arc<Settings>,
    /// Saved VPN profiles known to NetworkManager.
    pub profiles: Property<Vec<VpnProfile>>,
    /// Active VPN connections.
    pub active: Property<Vec<VpnConnection>>,
    /// Tailscale status when the `tailscale` CLI is available.
    pub tailscale: Property<Option<TailscaleStatus>>,
}

impl VpnManager {
    pub(crate) async fn new(
        zbus_connection: &Connection,
        settings: Arc<Settings>,
        cancellation_token: &CancellationToken,
    ) -> Result<Arc<Self>, Error> {
        let manager = Arc::new(Self {
            zbus_connection: zbus_connection.clone(),
            cancellation_token: cancellation_token.child_token(),
            settings,
            profiles: Property::new(vec![]),
            active: Property::new(vec![]),
            tailscale: Property::new(None),
        });

        manager.refresh_profiles();
        manager.refresh_active().await;
        manager.refresh_tailscale().await;
        manager.clone().start_monitoring().await?;

        Ok(manager)
    }

    /// Activate a saved VPN profile.
    ///
    /// # Errors
    /// Returns [`Error::OperationFailed`] when NetworkManager cannot activate the profile.
    pub async fn connect_profile(
        &self,
        profile_path: &OwnedObjectPath,
    ) -> Result<OwnedObjectPath, Error> {
        VpnControls::connect_profile(&self.zbus_connection, profile_path).await
    }

    /// Deactivate an active VPN connection.
    ///
    /// # Errors
    /// Returns [`Error::OperationFailed`] when NetworkManager cannot deactivate the connection.
    pub async fn disconnect_active(&self, active_path: &OwnedObjectPath) -> Result<(), Error> {
        VpnControls::disconnect_active(&self.zbus_connection, active_path).await
    }

    /// Run `tailscale up`.
    ///
    /// # Errors
    /// Returns [`Error::OperationFailed`] if the command is unavailable or fails.
    pub async fn tailscale_up(&self) -> Result<(), Error> {
        VpnControls::tailscale_up().await
    }

    /// Run `tailscale down`.
    ///
    /// # Errors
    /// Returns [`Error::OperationFailed`] if the command is unavailable or fails.
    pub async fn tailscale_down(&self) -> Result<(), Error> {
        VpnControls::tailscale_down().await
    }

    pub(crate) fn refresh_profiles(&self) {
        let profiles = self
            .settings
            .connections
            .get()
            .into_iter()
            .filter_map(|connection| {
                let vpn_service_type = connection.vpn_service_type.get();
                let kind = VpnKind::from_connection_settings(
                    &connection.connection_type.get(),
                    vpn_service_type.as_deref(),
                )?;
                Some(VpnProfile {
                    object_path: connection.object_path,
                    name: connection.id.get(),
                    uuid: connection.uuid.get(),
                    kind,
                })
            })
            .collect();

        self.profiles.set(profiles);
    }

    /// Refresh active VPN state from NetworkManager.
    pub async fn refresh_active(&self) {
        let active = match active_vpn_connections(&self.zbus_connection).await {
            Ok(active) => active,
            Err(err) => {
                debug!(error = %err, "cannot refresh active VPN connections");
                return;
            }
        };

        self.active.set(active);
    }

    /// Refresh Tailscale state from the local `tailscale` CLI.
    pub async fn refresh_tailscale(&self) {
        let status = match tailscale_status().await {
            Some(status) if status.available => Some(status),
            _ => None,
        };

        self.tailscale.set(status);
    }
}

async fn active_vpn_connections(connection: &Connection) -> Result<Vec<VpnConnection>, Error> {
    let proxy = NetworkManagerProxy::new(connection).await?;
    let active_paths = proxy.active_connections().await.map_err(Error::DbusError)?;

    let active = join_all(
        active_paths
            .into_iter()
            .map(|path| active_vpn_connection(connection, path)),
    )
    .await
    .into_iter()
    .flatten()
    .collect();

    Ok(active)
}

async fn active_vpn_connection(
    connection: &Connection,
    path: OwnedObjectPath,
) -> Option<VpnConnection> {
    let active = ActiveConnection::get(ActiveConnectionParams {
        connection,
        path: path.clone(),
    })
    .await
    .ok()?;

    let kind = VpnKind::from_nm_type(&active.type_.get())?;
    let state = if active.vpn.get() {
        vpn_state(connection, &path)
            .await
            .unwrap_or_else(|| VpnState::from_active_state(active.state.get()))
    } else {
        VpnState::from_active_state(active.state.get())
    };

    Some(VpnConnection {
        active_path: path,
        profile_path: active.connection_path.get(),
        name: active.id.get(),
        uuid: active.uuid.get(),
        kind,
        state,
    })
}

async fn vpn_state(connection: &Connection, path: &OwnedObjectPath) -> Option<VpnState> {
    let proxy = VPNConnectionProxy::new(connection, path).await.ok()?;
    let raw_state = proxy.vpn_state().await.ok()?;
    Some(VpnState::from_nm_vpn_state(NMVpnConnectionState::from_u32(
        raw_state,
    )))
}

async fn tailscale_status() -> Option<TailscaleStatus> {
    let output = Command::new("tailscale")
        .args(["status", "--json"])
        .output()
        .await
        .ok()?;

    if output.stdout.is_empty() {
        return Some(TailscaleStatus::unavailable());
    }

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    let backend_state = string_at(&json, &["BackendState"]).unwrap_or_else(|| {
        if output.status.success() {
            String::from("Unknown")
        } else {
            String::from("Unavailable")
        }
    });
    let connected = matches!(backend_state.as_str(), "Running" | "Starting");
    let self_node = json.get("Self");
    let self_name = self_node
        .and_then(|node| string_at(node, &["HostName"]))
        .or_else(|| self_node.and_then(|node| string_at(node, &["DNSName"])));
    let tailnet = tailscale_tailnet_name(&json);

    Some(TailscaleStatus {
        available: true,
        backend_state,
        self_name,
        tailnet,
        connected,
    })
}

fn string_at(value: &serde_json::Value, path: &[&str]) -> Option<String> {
    let mut current = value;
    for segment in path {
        current = current.get(*segment)?;
    }

    current.as_str().map(ToOwned::to_owned)
}

fn tailscale_tailnet_name(json: &serde_json::Value) -> Option<String> {
    string_at(json, &["MagicDNSSuffix"]).or_else(|| {
        json.get("CurrentTailnet")
            .and_then(|tailnet| string_at(tailnet, &["Name"]))
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn tailscale_tailnet_name_prefers_magic_dns_suffix() {
        let status = json!({
            "MagicDNSSuffix": "risk-bull.ts.net",
            "CurrentTailnet": {
                "Name": "digitalbrain.nl"
            }
        });

        assert_eq!(
            tailscale_tailnet_name(&status),
            Some(String::from("risk-bull.ts.net"))
        );
    }

    #[test]
    fn tailscale_tailnet_name_falls_back_to_current_tailnet_name() {
        let status = json!({
            "CurrentTailnet": {
                "Name": "digitalbrain.nl"
            }
        });

        assert_eq!(
            tailscale_tailnet_name(&status),
            Some(String::from("digitalbrain.nl"))
        );
    }
}
