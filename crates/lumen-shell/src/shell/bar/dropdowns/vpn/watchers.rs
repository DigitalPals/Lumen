use std::sync::Arc;

use lumen_config::ConfigService;
use lumen_network::NetworkService;
use lumen_widgets::watch;
use relm4::ComponentSender;

use super::{VpnDropdown, messages::VpnDropdownCmd};

pub(super) fn spawn(
    sender: &ComponentSender<VpnDropdown>,
    config: &Arc<ConfigService>,
    network: &Arc<NetworkService>,
) {
    let scale = config.config().styling.scale.clone();
    watch!(sender, [scale.watch()], |out| {
        let _ = out.send(VpnDropdownCmd::ScaleChanged(scale.get().value()));
    });

    let active = network.vpn.active.clone();
    let profiles = network.vpn.profiles.clone();
    let tailscale = network.vpn.tailscale.clone();
    let connected_icon = config.config().modules.vpn.connected_icon.clone();
    let connecting_icon = config.config().modules.vpn.connecting_icon.clone();
    let disconnected_icon = config.config().modules.vpn.disconnected_icon.clone();

    watch!(
        sender,
        [
            active.watch(),
            profiles.watch(),
            tailscale.watch(),
            connected_icon.watch(),
            connecting_icon.watch(),
            disconnected_icon.watch()
        ],
        |out| {
            let _ = out.send(VpnDropdownCmd::StateChanged);
        }
    );
}
