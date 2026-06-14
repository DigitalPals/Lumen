use lumen_config::schemas::modules::VpnConfig;
use lumen_network::NetworkService;

use super::{VpnModule, helpers};

impl VpnModule {
    pub(super) fn compute_display(
        config: &VpnConfig,
        network: &NetworkService,
    ) -> (String, String) {
        let active = network.vpn.active.get();
        let tailscale = network.vpn.tailscale.get();

        (
            helpers::vpn_icon(config, &active, tailscale.as_ref()),
            helpers::vpn_label(config, &active, tailscale.as_ref()),
        )
    }
}
