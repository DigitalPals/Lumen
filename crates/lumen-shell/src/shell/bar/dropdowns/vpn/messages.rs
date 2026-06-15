use std::sync::Arc;

use lumen_config::ConfigService;
use lumen_network::NetworkService;
use zbus::zvariant::OwnedObjectPath;

pub(crate) struct VpnDropdownInit {
    pub network: Arc<NetworkService>,
    pub config: Arc<ConfigService>,
}

#[derive(Debug)]
pub(crate) enum VpnDropdownMsg {
    ConnectProfile(OwnedObjectPath),
    DisconnectActive(OwnedObjectPath),
    OpenTailscaleAdmin,
    TailscaleUp,
    TailscaleDown,
}

#[derive(Debug)]
pub(crate) enum VpnDropdownCmd {
    StateChanged,
    ScaleChanged(f32),
    OperationFailed(String),
}
