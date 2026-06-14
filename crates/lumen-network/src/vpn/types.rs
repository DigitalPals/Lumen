use zbus::zvariant::OwnedObjectPath;

use crate::types::{
    connectivity::ConnectionType,
    states::{NMActiveConnectionState, NMVpnConnectionState},
};

/// VPN profile or connection backend type.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum VpnKind {
    /// NetworkManager VPN plugin backed profile.
    NetworkManager,
    /// OpenVPN NetworkManager profile.
    OpenVpn,
    /// OpenConnect NetworkManager profile.
    OpenConnect,
    /// strongSwan NetworkManager profile.
    StrongSwan,
    /// NetworkManager WireGuard profile.
    WireGuard,
    /// Tailscale daemon.
    Tailscale,
    /// Unknown or unsupported provider.
    Other(String),
}

impl VpnKind {
    pub(crate) fn from_connection_settings(
        connection_type: &ConnectionType,
        vpn_service_type: Option<&str>,
    ) -> Option<Self> {
        match connection_type {
            ConnectionType::Vpn => Some(Self::from_service_type(vpn_service_type)),
            ConnectionType::WireGuard => Some(Self::WireGuard),
            ConnectionType::Other(kind) if kind == "vpn" => {
                Some(Self::from_service_type(vpn_service_type))
            }
            ConnectionType::Other(kind) if kind == "wireguard" => Some(Self::WireGuard),
            _ => None,
        }
    }

    fn from_service_type(vpn_service_type: Option<&str>) -> Self {
        match vpn_service_type {
            Some("org.freedesktop.NetworkManager.openvpn") => Self::OpenVpn,
            Some("org.freedesktop.NetworkManager.openconnect") => Self::OpenConnect,
            Some("org.freedesktop.NetworkManager.strongswan") => Self::StrongSwan,
            Some(other) => Self::Other(other.to_owned()),
            None => Self::NetworkManager,
        }
    }

    pub(crate) fn from_nm_type(nm_type: &str) -> Option<Self> {
        match nm_type {
            "vpn" => Some(Self::NetworkManager),
            "wireguard" => Some(Self::WireGuard),
            _ => None,
        }
    }

    /// Human-readable provider label.
    pub fn label(&self) -> &str {
        match self {
            Self::NetworkManager => "VPN",
            Self::OpenVpn => "OpenVPN",
            Self::OpenConnect => "OpenConnect",
            Self::StrongSwan => "strongSwan",
            Self::WireGuard => "WireGuard",
            Self::Tailscale => "Tailscale",
            Self::Other(value) => value.as_str(),
        }
    }
}

/// High-level VPN connection state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VpnState {
    /// State cannot be determined.
    Unknown,
    /// VPN is not connected.
    Disconnected,
    /// VPN is connecting.
    Connecting,
    /// VPN needs credentials or authorization.
    NeedsAuth,
    /// VPN is connected.
    Connected,
    /// VPN is disconnecting.
    Disconnecting,
    /// VPN failed.
    Failed,
}

impl VpnState {
    pub(crate) fn from_active_state(state: NMActiveConnectionState) -> Self {
        match state {
            NMActiveConnectionState::Unknown => Self::Unknown,
            NMActiveConnectionState::Activating => Self::Connecting,
            NMActiveConnectionState::Activated => Self::Connected,
            NMActiveConnectionState::Deactivating => Self::Disconnecting,
            NMActiveConnectionState::Deactivated => Self::Disconnected,
        }
    }

    pub(crate) fn from_nm_vpn_state(state: NMVpnConnectionState) -> Self {
        match state {
            NMVpnConnectionState::Unknown => Self::Unknown,
            NMVpnConnectionState::Prepare
            | NMVpnConnectionState::Connect
            | NMVpnConnectionState::IpConfigGet => Self::Connecting,
            NMVpnConnectionState::NeedAuth => Self::NeedsAuth,
            NMVpnConnectionState::Activated => Self::Connected,
            NMVpnConnectionState::Failed => Self::Failed,
            NMVpnConnectionState::Disconnected => Self::Disconnected,
        }
    }
}

/// Saved VPN profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VpnProfile {
    /// NetworkManager settings object path.
    pub object_path: OwnedObjectPath,
    /// Profile name.
    pub name: String,
    /// Stable NetworkManager UUID.
    pub uuid: String,
    /// Backend kind.
    pub kind: VpnKind,
}

/// Active VPN connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VpnConnection {
    /// NetworkManager active connection object path.
    pub active_path: OwnedObjectPath,
    /// NetworkManager settings object path.
    pub profile_path: OwnedObjectPath,
    /// Display name.
    pub name: String,
    /// Stable UUID.
    pub uuid: String,
    /// Backend kind.
    pub kind: VpnKind,
    /// Current state.
    pub state: VpnState,
}

/// Tailscale daemon status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TailscaleStatus {
    /// Whether `tailscale status --json` was available.
    pub available: bool,
    /// Backend state reported by tailscaled.
    pub backend_state: String,
    /// Current device name, when logged in.
    pub self_name: Option<String>,
    /// Current tailnet name, when available.
    pub tailnet: Option<String>,
    /// Whether Tailscale reports an active/running backend state.
    pub connected: bool,
}

impl TailscaleStatus {
    pub(crate) fn unavailable() -> Self {
        Self {
            available: false,
            backend_state: String::from("Unavailable"),
            self_name: None,
            tailnet: None,
            connected: false,
        }
    }
}
