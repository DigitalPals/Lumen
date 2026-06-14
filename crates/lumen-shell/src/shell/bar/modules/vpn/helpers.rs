use lumen_config::schemas::modules::{TailscaleLabel, VpnConfig};
use lumen_network::vpn::{TailscaleStatus, VpnConnection, VpnState};

use crate::i18n::t;

pub(super) fn vpn_icon(
    config: &VpnConfig,
    active: &[VpnConnection],
    tailscale: Option<&TailscaleStatus>,
) -> String {
    if active
        .iter()
        .any(|connection| connection.state == VpnState::Connecting)
        || tailscale.is_some_and(|status| status.backend_state == "Starting")
    {
        return config.connecting_icon.get().clone();
    }

    if active
        .iter()
        .any(|connection| connection.state == VpnState::Connected)
        || tailscale.is_some_and(|status| status.connected)
    {
        return config.connected_icon.get().clone();
    }

    config.disconnected_icon.get().clone()
}

pub(super) fn vpn_label(
    config: &VpnConfig,
    active: &[VpnConnection],
    tailscale: Option<&TailscaleStatus>,
) -> String {
    let active_count = active
        .iter()
        .filter(|connection| connection.state == VpnState::Connected)
        .count()
        + usize::from(tailscale.is_some_and(|status| status.connected));

    if active_count > 1 {
        return t!("bar-vpn-count", count = active_count);
    }

    if let Some(connection) = active
        .iter()
        .find(|connection| connection.state == VpnState::Connected)
    {
        return connection.name.clone();
    }

    if let Some(status) = tailscale.filter(|status| status.connected) {
        return match config.tailscale_label.get() {
            TailscaleLabel::ServiceName => String::from("Tailscale"),
            TailscaleLabel::Hostname => status
                .self_name
                .clone()
                .unwrap_or_else(|| String::from("Tailscale")),
            TailscaleLabel::Status => t!("bar-vpn-connected"),
        };
    }

    if active
        .iter()
        .any(|connection| connection.state == VpnState::Connecting)
        || tailscale.is_some_and(|status| status.backend_state == "Starting")
    {
        return t!("bar-vpn-connecting");
    }

    if tailscale.is_some() && matches!(config.tailscale_label.get(), TailscaleLabel::Status) {
        return t!("bar-vpn-status-disconnected");
    }

    t!("bar-vpn-disconnected")
}
