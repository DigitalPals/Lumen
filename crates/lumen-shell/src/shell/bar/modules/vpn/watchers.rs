use std::sync::Arc;

use lumen_config::schemas::modules::VpnConfig;
use lumen_network::NetworkService;
use lumen_widgets::watch;
use relm4::ComponentSender;

use super::{VpnModule, messages::VpnCmd};

pub(super) fn spawn(
    sender: &ComponentSender<VpnModule>,
    config: &VpnConfig,
    network: &Arc<NetworkService>,
) {
    let active = network.vpn.active.clone();
    let profiles = network.vpn.profiles.clone();
    let tailscale = network.vpn.tailscale.clone();

    watch!(
        sender,
        [active.watch(), profiles.watch(), tailscale.watch()],
        |out| {
            let _ = out.send(VpnCmd::StateChanged);
        }
    );

    let connected_icon = config.connected_icon.clone();
    let connecting_icon = config.connecting_icon.clone();
    let disconnected_icon = config.disconnected_icon.clone();

    watch!(
        sender,
        [
            connected_icon.watch(),
            connecting_icon.watch(),
            disconnected_icon.watch()
        ],
        |out| {
            let _ = out.send(VpnCmd::StateChanged);
        }
    );
}
