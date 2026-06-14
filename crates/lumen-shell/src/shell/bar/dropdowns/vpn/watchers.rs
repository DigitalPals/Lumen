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

    watch!(
        sender,
        [active.watch(), profiles.watch(), tailscale.watch()],
        |out| {
            let _ = out.send(VpnDropdownCmd::StateChanged);
        }
    );
}
